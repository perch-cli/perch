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

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::{self, Host};
use crate::lock;
use crate::probe::{self, Credential, Store};
use crate::profile;
use crate::registry::{self, Account, Quarantine};

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
    /// Set when the Switch did not merely fail but found the incoming Account
    /// unusable for good, so the caller records it rather than letting the same
    /// discovery be made again from scratch next time.
    pub quarantine: Option<Quarantine>,
}

/// A Switch that stopped before it wrote anything, carrying whatever the
/// failure knew about the incoming Account.
///
/// A Switch can find an Account unusable for good — a Profile with neither
/// store holding a Credential — and when it does, the failure itself says which
/// Quarantine that is. Nothing here decides: it reads what was diagnosed where
/// it was diagnosed.
fn stopped(error: PerchError) -> Interrupted {
    let quarantine = match &error {
        PerchError::Quarantined { why, .. } => Some(*why),
        _ => None,
    };
    Interrupted {
        error,
        incoming_is_live: false,
        quarantine,
    }
}

/// Everything the three steps need, established under the locks.
///
/// Under them, not before them: the liveness refusal is a statement about a
/// moment, and taking a lock can take seconds. A `claude` started during that
/// wait is one the refusal never saw, and the Switch would then replace the
/// Credential that session is holding — the mid-task logout ADR 0005 exists to
/// prevent. Once the locks are held, nothing can change the answer, which is
/// the only condition under which asking is worth anything.
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
    let (version, store) = ground(host).map_err(stopped)?;

    let mut incoming_is_live = false;
    let switched: Result<Captured> = lock::under(host, probe::locks_for(&store), |held| {
        let prepared = prepare(host, incoming, outgoing, version, store)?;

        let captured = capture(host, &prepared, outgoing)
            .map_err(|error| error.with_note(&nothing_happened(outgoing)))?;

        held.renew();
        profile::store_credential(host, &prepared.store, prepared.credential.as_str())
            .map_err(|error| error.with_note(&only_captured(&captured, outgoing, incoming)))?;
        incoming_is_live = true;

        held.renew();
        patch_identity(host, &prepared)
            .map_err(|error| error.with_note(&live_but_unnamed(&prepared, outgoing, incoming)))?;

        Ok(captured)
    });

    switched.map_err(|error| Interrupted {
        incoming_is_live,
        ..stopped(error)
    })
}

/// Makes an Account's Credential the live one without Capturing what it
/// replaces.
///
/// Every Switch Captures first, because the live Credential is the only good
/// copy of the outgoing Account's (ADR 0006). A `perch relogin` of the Account
/// you are on is the one case where that is false in both directions: the
/// Credential about to be replaced belongs to the Account being repaired, and a
/// login has already replaced it. Capturing here would write the broken copy
/// over the fresh one — the one mistake this design cannot recover from,
/// arriving as tidiness.
///
/// The Credential written is the one in the Account's own Profile, read back
/// out of it rather than passed in, so the same store that a `perch switch`
/// tomorrow will read is the store this proves works today.
pub fn make_live(host: &dyn Host, account: &Account) -> std::result::Result<(), NotLanded> {
    let (version, store) = ground(host).map_err(|error| NotLanded {
        error,
        is_live: false,
    })?;

    let mut is_live = false;
    let landed = lock::under(host, probe::locks_for(&store), |held| {
        let prepared = prepare(host, account, None, version, store)?;

        profile::store_credential(host, &prepared.store, prepared.credential.as_str())?;
        is_live = true;
        held.renew();
        patch_identity(host, &prepared)
    });

    landed.map_err(|error| NotLanded { error, is_live })
}

/// A `make_live` that stopped part way, and what the machine is holding now.
///
/// The same distinction [`Interrupted`] draws, for the same reason: a failure
/// after the Credential was written but before the Identity was patched has
/// still changed which Account the machine is acting as, and a caller that
/// records who is active has to record what is true rather than what it asked
/// for.
pub struct NotLanded {
    pub error: PerchError,
    /// Whether the Account's Credential is the live one despite the failure.
    pub is_live: bool,
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

/// The two things that are true whatever else is: which Claude Code is
/// installed, and where the Default Profile is. Established before the locks,
/// because the locks are derived from the second of them.
fn ground(host: &dyn Host) -> Result<(String, Store)> {
    Ok((probe::claude_version(host)?, probe::default_store(host)?))
}

fn prepare(
    host: &dyn Host,
    incoming: &Account,
    outgoing: Option<&Account>,
    version: String,
    store: Store,
) -> Result<Prepared> {
    // Before anything is written, and only of the Profile that is written to.
    //
    // The Capture writes the live Credential into the outgoing Account's
    // Profile, and a client running there is holding that file — writing under
    // it is the mid-task logout ADR 0005 exists to prevent. The incoming
    // Account's Profile is only ever *read* from, and reading a Credential to
    // copy it into the Default Profile takes nothing away from the session that
    // is using it (ADR 0027). Refusing that as well would make a Run and a
    // Switch lock each other out for no reason: an Account you are running in
    // one terminal is exactly the Account you would want active in the others.
    if let Some(outgoing) = outgoing {
        refuse_if_live(host, outgoing, &version)?;
    }

    // From whichever of the Profile's two Credential Stores holds one (ADR
    // 0020): an Account is switchable to as long as its Credential is
    // somewhere Claude Code would have looked.
    let held = credentials::read(host, &incoming.store(host)?)?.ok_or_else(|| {
        PerchError::Quarantined {
            why: Quarantine::NoCredential,
            said: format!(
                "Perch holds no Credential for {}, so it is Quarantined — it \
                 stays listed and named, and nothing switches to it until it has \
                 been logged into again.\n\
                 Nothing was changed. {}",
                incoming.email(),
                registry::how_to_repair(incoming.email()),
            ),
        }
    })?;
    let credential = probe::understand_credential(
        held.credential,
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

    profile::store_credential(host, &outgoing.store(host)?, live.as_str())?;

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
        Err(err) => return Err(PerchError::file_read(file.clone(), err)),
    };

    host::write_atomically(host, file, &patched)
        .map_err(|err| PerchError::file_write(file.clone(), err))
}

/// The `oauthAccount` block for an Account.
///
/// Its own Profile holds the block Claude Code wrote when that Account logged
/// in, which carries fields beyond the Identity Perch records — so that block
/// is preferred verbatim, and one is composed only for the Accounts that have
/// none, such as the login adoption took over (ADR 0009).
fn identity_block_for(host: &dyn Host, incoming: &Account) -> Result<String> {
    let kept = incoming.store(host)?.identity_file;
    let held = host
        .read_file(&kept)
        .ok()
        .and_then(|contents| probe::oauth_account_block(&contents).map(str::to_string));

    Ok(held.unwrap_or_else(|| incoming.identity.oauth_account_block()))
}

/// Refuses to touch a Profile something else is holding (ADR 0005).
///
/// Public because `perch relogin` asks it before it spends a login rather than
/// after: a Profile Perch may not write to is a Profile no browser round trip
/// was ever going to repair.
pub fn refuse_if_live(host: &dyn Host, account: &Account, version: &str) -> Result<()> {
    refuse_if_live_in(
        host,
        &account.profile_dir(host)?,
        &format!("{}'s Profile", account.email()),
        version,
    )
}

/// The same, of a config directory named rather than derived — the Default
/// Profile, which belongs to no one Account and is where a repair of the Account
/// you are on has to land.
pub fn refuse_if_live_in(
    host: &dyn Host,
    config_dir: &Path,
    whose: &str,
    version: &str,
) -> Result<()> {
    let running = probe::live_clients(host, config_dir, version)?;
    if running.is_empty() {
        return Ok(());
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(PerchError::ProfileLive(format!(
        "A client is running against {whose} (pid {}).\n\
         Nothing was changed. That Credential belongs to it until it exits — \
         quit it, or switch to a different Account.",
        pids.join(", ")
    )))
}

/// Both Profiles a command may write into, refused while a client is holding
/// either.
///
/// One place, because every command that needs this asks it twice — once before
/// it puts an unbounded wait to the person, and once after, because the first
/// answer is about a machine that has since had minutes to move on. Two
/// spellings of the same pair of checks is how the second ask comes to be
/// weaker than the first, and the second ask is the one standing between a
/// running session and a Credential written out from under it.
///
/// `the_default_profile_too` is the sentence saying why the Default Profile is
/// written as well, and `None` is a command that leaves it alone. Named rather
/// than derived, because what makes the Default Profile the wrong thing to
/// overwrite is different for a repair and for a removal, and the refusal is
/// read by somebody deciding which client to quit.
pub fn refuse_if_live_anywhere(
    host: &dyn Host,
    account: &Account,
    the_default_profile_too: Option<&str>,
    version: &str,
) -> Result<()> {
    refuse_if_live(host, account, version)?;

    if let Some(whose) = the_default_profile_too {
        // Its Credential is the one a running client is holding, and this would
        // replace it rather than renew it — the mid-task logout ADR 0005 exists
        // to prevent.
        refuse_if_live_in(
            host,
            &probe::default_store(host)?.config_dir,
            whose,
            version,
        )?;
    }
    Ok(())
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
