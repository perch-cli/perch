//! Running a login where it can cost nothing
//! (ADR a-login-perch-does-not-need).
//!
//! `perch add` and `perch relogin` both run a login in a config directory of its
//! own and take what it left behind. This module ends where the login ends: it
//! says who logged in, and nothing about whether that was who was wanted — the
//! two callers differ on exactly that.

use super::profile;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::providers::claude::probe::{self, Credential, Identity, Installed};
use zeroize::Zeroizing;

/// What a login left behind, taken out of the directory it ran in.
pub struct Produced {
    pub identity: Identity,
    pub credential: Credential,
    /// The `.claude.json` the login wrote, kept verbatim — it describes this
    /// Account in Claude Code's own terms, which is more than Perch records.
    ///
    /// Wiped on drop for [`crate::json`]'s reason: this file is where an MCP
    /// server's `env` block lives, and a plain `String` frees one untouched.
    pub identity_json: Zeroizing<String>,
}

pub(crate) fn authenticate(host: &dyn Host, claude: &std::path::Path) -> Result<Produced> {
    // Everything that can fail without leaving anything behind happens first,
    // so the directory is made only once nothing before it can refuse.
    let installed = Installed::Said(probe::version_at(host, claude)?);
    let dir =
        holdings::pending_login_dir(crate::providers::provider::Id::Claude, host, host.now())?;
    let store = probe::store_for_profile(host, &dir)?;

    // The login writes its Credential in here, so this is as much a place a
    // Credential lives as a Profile is (ADR claude-code-chooses-the-store).
    host.create_private_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    // Perch's own pid: Perch waits on this login as a Run waits on its client,
    // and a `claude` on an OAuth prompt has no session of its own to mark
    // (ADR a-run-is-one-shot). `profile::discard` takes it with the directory.
    let live = crate::providers::sessions::claim(host, &dir);

    // Every way out from here takes the directory back out again, which a `?`
    // in the middle would quietly stop doing: one left by a failure is one
    // the reaper will not tidy for thirty minutes.
    let produced = live.and_then(|_live| run_the_login(host, claude, &store, &installed));
    profile::discard(host, &store);
    produced
}

fn run_the_login(
    host: &dyn Host,
    claude: &std::path::Path,
    store: &probe::Store,
    installed: &Installed,
) -> Result<Produced> {
    let status = host
        .exec_interactive(
            &claude.to_string_lossy(),
            &[],
            // The store's own spelling, not the caller's: `store_for_profile`
            // normalized this path to derive the Credential Store, and a client
            // told the other spelling writes into a namespace Perch never reads.
            &[("CLAUDE_CONFIG_DIR", &store.config_dir.to_string_lossy())],
        )
        .map_err(|err| PerchError::Other(format!("could not launch a login: {err}")))?;

    what_the_login_left(host, store, installed, status)
}

/// Reads the Account the login produced, or says why there is not one.
fn what_the_login_left(
    host: &dyn Host,
    store: &probe::Store,
    installed: &Installed,
    status: i32,
) -> Result<Produced> {
    let credential = probe::read_credential(host, store, installed)?;
    let identity = probe::read_identity(host, store, installed)?;

    let (credential, identity) = match (credential, identity) {
        (Some(credential), Some(identity)) => (credential, identity),
        // A login that produced neither is one that was abandoned or refused,
        // and the exit status is the only extra thing worth saying about it.
        _ => {
            let ending = if status == 0 {
                "The login did not complete".to_string()
            } else {
                format!("The login exited {status}")
            };
            return Err(PerchError::NotFound(format!("{ending}. Nothing changed.")));
        }
    };

    let identity_json = host
        .read_file(&store.identity_file)
        .map(Zeroizing::new)
        .map_err(|err| {
            PerchError::Other(format!(
                "could not read {}: {err}",
                store.identity_file.display()
            ))
        })?;

    Ok(Produced {
        identity,
        credential,
        identity_json,
    })
}

pub(super) fn discover(
    host: &dyn Host,
    executable: &std::path::Path,
) -> Result<Option<super::super::provider::Authenticated>> {
    let findings = match probe::probe_at(
        host,
        crate::providers::claude::layout::default_profile(host)?,
        executable,
    )? {
        probe::Verdict::Recognized(findings) => findings,
        probe::Verdict::NoLogin { .. } => return Ok(None),
    };
    let configuration = host
        .read_file(&findings.store.identity_file)
        .ok()
        .map(Zeroizing::new)
        .and_then(|contents| probe::oauth_account_block(&contents).map(probe::fresh_identity_file))
        .map(Zeroizing::new);
    Ok(Some(super::super::provider::Authenticated {
        provider: super::super::provider::Id::Claude,
        subject: Some(super::identity::subject(&findings.identity)?),
        identity: findings.identity,
        plan: findings.credential.subscription_type.clone(),
        credential: Zeroizing::new(findings.credential.as_str().to_string()),
        configuration,
    }))
}
