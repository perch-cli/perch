//! Running a login where it can cost nothing.
//!
//! Both commands that need Anthropic to authenticate somebody — `perch add` and
//! `perch relogin` — run the login in a config directory of its own and take
//! what it left behind (ADR 0009). The active Account is never read, never
//! written and never logged out by either of them: gaining an Account and
//! repairing one both leave the session you are working in exactly where it was.
//!
//! What is *done* with the result is the caller's, and the two callers differ on
//! the one question that matters — `add` refuses an Account Perch already holds,
//! and `relogin` refuses every Account but that one. So this module ends where
//! the login ends: it says who logged in, and nothing about whether that was who
//! was wanted.

use std::io::Write;

use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Credential, Identity};
use crate::profile;
use crate::registry;

/// What a login left behind, taken out of the directory it ran in.
pub struct Produced {
    pub identity: Identity,
    pub credential: Credential,
    /// The `.claude.json` the login wrote, kept verbatim — it describes this
    /// Account in Claude Code's own terms, which is more than Perch records.
    pub identity_json: String,
}

/// Launches a login and returns what it produced.
///
/// `purpose` is the line said before the browser opens: why Perch is asking for
/// a login and what it is leaving alone. The directory the login runs in is
/// removed whether it worked or not, so an abandoned login costs a directory
/// that is then gone and nothing else.
pub fn perform(host: &dyn Host, out: &mut dyn Write, purpose: &str) -> Result<Produced> {
    let version = probe::claude_version(host)?;
    let dir = registry::pending_login_dir(host, host.now())?;
    // The login writes its Credential in here, so this is as much a place a
    // Credential lives as a Profile is (ADR 0020).
    host.create_private_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    let store = probe::store_for_profile(host, &dir)?;

    say(out, purpose)?;
    say(
        out,
        "Quit Claude Code when the login is done to come back here.\n",
    )?;

    let claude = probe::claude_bin(host)?;
    let status = host
        .exec_interactive(
            &claude.to_string_lossy(),
            &[],
            &[("CLAUDE_CONFIG_DIR", &dir.to_string_lossy())],
        )
        .map_err(|err| PerchError::Other(format!("could not launch a login: {err}")))?;

    let produced = what_the_login_left(host, &store, &version, status);
    profile::discard(host, &store);
    produced
}

/// Reads the Account the login produced, or says why there is not one.
fn what_the_login_left(
    host: &dyn Host,
    store: &probe::Store,
    version: &str,
    status: i32,
) -> Result<Produced> {
    let credential = probe::read_credential(host, store, version)?;
    let identity = probe::read_identity(host, store, version)?;

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

    let identity_json = host.read_file(&store.identity_file).map_err(|err| {
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

/// Keeps the `.claude.json` a login wrote in the Profile the Account settles
/// into. The Identity travels with the Credential it describes.
pub fn carry_identity_file(host: &dyn Host, contents: &str, store: &probe::Store) -> Result<()> {
    host.write_file(&store.identity_file, contents)
        .map_err(|err| PerchError::file_write(store.identity_file.clone(), err))
}

/// How long a pending login is left alone before it is taken to have been
/// abandoned.
///
/// Generous, because the thing on the other side of it is a person finding
/// their password: a login somebody is still driving in another terminal must
/// never be reaped out from under them. A login nobody came back from costs
/// only the time until the next command, and there is no hurry.
const ABANDONED_AFTER_MINUTES: i64 = 30;

/// Deletes what abandoned logins left behind.
///
/// A login writes a complete, working Credential into the directory it runs in
/// — the keychain on macOS, a file everywhere else — and [`perform`] clears
/// that directory on the way out. But Ctrl-C delivers SIGINT to the whole
/// foreground group, and "quit Claude Code when the login is done" is the
/// documented flow, so a login being abandoned is expected rather than
/// exceptional: Perch dies without unwinding and the Credential stays.
///
/// Nothing ever looked for those again. Each one is named after the moment it
/// started, so every abandonment left a new one, and the keychain items are
/// invisible — a slow accumulation of live refresh tokens for Accounts the user
/// believes they never added. This runs at the start of every command, which is
/// the earliest anything can notice.
///
/// Silent and best-effort throughout. It is tidying up on the way to what the
/// user actually asked for, and a directory that will not go is not a reason to
/// fail that.
pub fn reap_abandoned(host: &dyn Host) {
    let Ok(pending) = registry::pending_logins_dir(host) else {
        return;
    };
    // Absent is the ordinary case: no login has ever been run here.
    let Ok(entries) = host.list_dir(&pending) else {
        return;
    };

    let too_old = host.now() - chrono::Duration::minutes(ABANDONED_AFTER_MINUTES);
    for dir in entries {
        // A directory whose age cannot be established is left alone. Being
        // wrong in this direction costs a stale directory; being wrong in the
        // other costs somebody the login they are in the middle of.
        let Some(started_at) = registry::pending_login_started_at(&dir) else {
            continue;
        };
        if started_at > too_old {
            continue;
        }
        if let Ok(store) = probe::store_for_profile(host, &dir) {
            profile::discard(host, &store);
        }
    }
}

/// What every login says about the Account it is leaving alone, when there is
/// one to leave alone.
pub fn leaving_the_active_account_alone(active: Option<&str>) -> String {
    match active {
        Some(active) => format!(" {active} stays active and its session is untouched."),
        None => String::new(),
    }
}
