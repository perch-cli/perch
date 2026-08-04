//! The Switch: three ordered steps, and never two.
//!
//! Making an Account active is a Capture of the outgoing Credential into its
//! own Profile, a write of the incoming one to the Default Profile, and a patch
//! of the Identity to match — in that order, under Claude Code's locks (ADR
//! 0006). The order is not a preference:
//!
//! - Capturing **first** is what stops a Switch poisoning the Account it
//!   leaves. Anthropic retires a refresh token when it Rotates one, so the copy
//!   in an Account's Profile is several Rotations behind by the time you switch
//!   away, and the live copy is the only good one there is.
//! - Patching the Identity **last** means the moment Claude Code disagrees with
//!   itself is as short as possible, and that if anything fails the file still
//!   names an Account whose Credential Perch can put back.
//!
//! Shared State is not touched. No Reconcile, no `projects[<cwd>]`: those are
//! the Run path (ADR 0003, ADR 0010) and a Switch needs none of it, which is
//! exactly why memory, settings, plugins and project history follow the person
//! across Accounts for free.

use std::path::Path;

use crate::error::{PerchError, Result};
use crate::host::{self, Host};
use crate::probe::{self, Credential, Store};
use crate::profile;
use crate::registry::Account;
use crate::{keychain, lock};

/// What the Capture found, which is the part of a Switch worth saying out loud:
/// it is the part that protects the Account being left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    /// The live Credential went back into the outgoing Account's Profile.
    Copied { from: String },
    /// Nothing was live to Capture — Claude Code is logged out.
    NothingLive,
    /// Perch holds no active Account, so there was nothing to Capture into.
    NoOutgoing,
}

/// A Switch that stopped part way, and what the machine is holding now.
pub struct Interrupted {
    pub error: PerchError,
    /// Whether the incoming Account's Credential is the live one. It decides
    /// which Account Perch must now record as active — being active is a fact
    /// about which Credential is in the Default Profile, not a wish.
    pub incoming_is_live: bool,
}

/// Everything the three steps need, established before any lock is taken.
///
/// Reading the incoming Credential up front keeps the lock held for as short a
/// time as possible, and is safe because a Profile that nothing is running
/// against does not change underneath Perch — which is what the liveness
/// refusal has just established.
struct Prepared {
    version: String,
    store: Store,
    credential: Credential,
    /// The `oauthAccount` block to write, ready to splice in.
    identity_block: String,
}

/// Makes `incoming` the active Account, Capturing `outgoing` on the way out.
pub fn perform(
    host: &dyn Host,
    incoming: &Account,
    outgoing: Option<&Account>,
) -> std::result::Result<Captured, Interrupted> {
    let prepared = prepare(host, incoming, outgoing).map_err(|error| Interrupted {
        error,
        incoming_is_live: false,
    })?;

    let mut incoming_is_live = false;
    let switched = lock::under(host, probe::locks_for(&prepared.store), |held| {
        let captured = capture(host, &prepared, outgoing)
            .map_err(|error| error.with_note(&nothing_happened(outgoing)))?;

        held.renew();
        profile::store_credential(
            host,
            &prepared.store.keychain_service,
            &prepared.store.keychain_account,
            prepared.credential.as_str(),
        )
        .map_err(|error| error.with_note(&only_captured(&captured, outgoing, incoming)))?;
        incoming_is_live = true;

        held.renew();
        patch_identity(host, &prepared)
            .map_err(|error| error.with_note(&live_but_unnamed(&prepared, outgoing, incoming)))?;

        Ok(captured)
    });

    switched.map_err(|error| Interrupted {
        error,
        incoming_is_live,
    })
}

/// Whether the machine already says what a Switch to this Account would make it
/// say — in both the places that have to agree.
///
/// A Switch is only complete when the live Credential and the Identity name the
/// same Account. One interrupted between those two steps leaves them
/// disagreeing, and running the same command again is how it is repaired: so
/// this asks Claude Code's own file rather than only Perch's record of who is
/// active, or the repair would be turned away as unnecessary.
pub fn already_landed(host: &dyn Host, account: &Account) -> Result<bool> {
    let version = probe::claude_version(host)?;
    let store = probe::default_store(host)?;
    Ok(probe::read_identity(host, &store, &version)?
        .is_some_and(|identity| identity.email.eq_ignore_ascii_case(account.email())))
}

fn prepare(host: &dyn Host, incoming: &Account, outgoing: Option<&Account>) -> Result<Prepared> {
    let version = probe::claude_version(host)?;
    let store = probe::default_store(host)?;

    // Before anything is written, and in the order the user would care about:
    // the Profile being read from, then the one being written back to.
    refuse_if_live(host, incoming)?;
    if let Some(outgoing) = outgoing.filter(|account| account.profile.dir != incoming.profile.dir) {
        refuse_if_live(host, outgoing)?;
    }

    let raw = host
        .keychain_get(
            &incoming.profile.keychain_service,
            &incoming.profile.keychain_account,
        )
        .map_err(|err| match err {
            keychain::KeychainError::NotFound { .. } => PerchError::NotFound(format!(
                "Perch holds no Credential for {}.\n\
                 Nothing was changed. Log that Account in again with `perch relogin {}`.",
                incoming.email(),
                incoming.email()
            )),
            other => PerchError::from(other),
        })?;
    let credential = probe::understand_credential(
        raw,
        &format!("the Credential Perch holds for {}", incoming.email()),
        &version,
    )?;

    Ok(Prepared {
        identity_block: identity_block_for(host, incoming)?,
        version,
        store,
        credential,
    })
}

/// Step one: the live Credential goes back where it belongs.
fn capture(host: &dyn Host, prepared: &Prepared, outgoing: Option<&Account>) -> Result<Captured> {
    let Some(outgoing) = outgoing else {
        return Ok(Captured::NoOutgoing);
    };

    let live = probe::read_credential(host, &prepared.store, &prepared.version)?;
    let Some(live) = live else {
        return Ok(Captured::NothingLive);
    };

    profile::store_credential(
        host,
        &outgoing.profile.keychain_service,
        &outgoing.profile.keychain_account,
        live.as_str(),
    )?;

    Ok(Captured::Copied {
        from: outgoing.email().to_string(),
    })
}

/// Step three: `.claude.json` comes to name the Account whose Credential is now
/// live — that key of it, and nothing else of it (ADR 0001).
fn patch_identity(host: &dyn Host, prepared: &Prepared) -> Result<()> {
    let file = &prepared.store.identity_file;
    let patched = match host.read_file(file) {
        Ok(contents) => probe::patch_oauth_account(
            &contents,
            &prepared.identity_block,
            file,
            &prepared.version,
        )?,
        // No file at all is a Claude Code that has never been run here. One
        // holding the Account and nothing else is exactly what it would write
        // for itself, and leaves it displaying the Account it is acting as.
        Err(host::HostError::NotFound { .. }) => {
            probe::fresh_identity_file(&prepared.identity_block)
        }
        Err(err) => {
            return Err(PerchError::FileRead {
                path: file.clone(),
                source: std::io::Error::other(err.to_string()),
            });
        }
    };

    host::write_atomically(host, file, &patched).map_err(|err| PerchError::FileWrite {
        path: file.clone(),
        source: std::io::Error::other(err.to_string()),
    })
}

/// The `oauthAccount` block for an Account.
///
/// Its own Profile holds the block Claude Code wrote when that Account logged
/// in, which carries fields beyond the Identity Perch records — so that block
/// is preferred verbatim, and one is composed only for the Accounts that have
/// none, such as the login adoption took over (ADR 0009).
fn identity_block_for(host: &dyn Host, incoming: &Account) -> Result<String> {
    let kept = probe::store_for_profile(host, &incoming.profile.dir)?.identity_file;
    let held = host
        .read_file(&kept)
        .ok()
        .and_then(|contents| probe::oauth_account_block(&contents).map(str::to_string));

    Ok(held.unwrap_or_else(|| incoming.identity.oauth_account_block()))
}

/// Refuses to touch a Profile something else is holding (ADR 0005).
fn refuse_if_live(host: &dyn Host, account: &Account) -> Result<()> {
    let running = probe::live_clients(host, &account.profile.dir);
    if running.is_empty() {
        return Ok(());
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(PerchError::ProfileLive(format!(
        "A client is running against {}'s Profile (pid {}).\n\
         Nothing was changed. That Credential belongs to it until it exits — \
         quit it, or switch to a different Account.",
        account.email(),
        pids.join(", ")
    )))
}

fn nothing_happened(outgoing: Option<&Account>) -> String {
    match outgoing {
        Some(outgoing) => format!(
            "Nothing was switched. {} is still the active Account and its live \
             Credential is untouched.",
            outgoing.email()
        ),
        None => "Nothing was switched.".to_string(),
    }
}

fn only_captured(captured: &Captured, outgoing: Option<&Account>, incoming: &Account) -> String {
    let mut note = match (captured, outgoing) {
        (Captured::Copied { from }, _) => format!(
            "{from}'s live Credential was Captured into its own Profile first, \
             so nothing has been lost."
        ),
        (_, Some(outgoing)) => format!("{}'s Profile is unchanged.", outgoing.email()),
        (_, None) => String::new(),
    };
    if !note.is_empty() {
        note.push(' ');
    }
    note.push_str(&format!(
        "The live Credential was not replaced, so {} was not made active.",
        incoming.email()
    ));
    note
}

fn live_but_unnamed(prepared: &Prepared, outgoing: Option<&Account>, incoming: &Account) -> String {
    let named = match outgoing {
        Some(outgoing) => outgoing.email().to_string(),
        None => "another Account".to_string(),
    };
    format!(
        "{incoming} is active — its Credential is the live one and was recorded \
         as such — but {file} still names {named}, so Claude Code will act as \
         {incoming} while displaying {named}.\n\
         Run `perch switch {incoming}` again to finish the job.",
        incoming = incoming.email(),
        file = display(&prepared.store.identity_file),
    )
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
