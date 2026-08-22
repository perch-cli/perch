//! Running a login where it can cost nothing
//! (ADR a-login-perch-does-not-need).
//!
//! `perch add` and `perch relogin` both run a login in a config directory of its
//! own and take what it left behind. This module ends where the login ends: it
//! says who logged in, and nothing about whether that was who was wanted — the
//! two callers differ on exactly that.

use std::io::Write;

use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Credential, Identity, Installed};
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
/// `purpose` is the line said before the browser opens: why Perch is asking, and
/// which Account it is leaving alone. The directory the login runs in is removed
/// whether it worked or not.
pub fn perform(host: &dyn Host, out: &mut dyn Write, purpose: &str) -> Result<Produced> {
    // Everything that can fail without leaving anything behind happens first,
    // so the directory is made only once nothing before it can refuse.
    let installed = Installed::probed(host)?;
    let claude = probe::claude_bin(host)?;
    let dir = registry::pending_login_dir(host, host.now())?;
    let store = probe::store_for_profile(host, &dir)?;

    // The login writes its Credential in here, so this is as much a place a
    // Credential lives as a Profile is (ADR claude-code-chooses-the-store).
    host.create_private_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    // Perch's own pid: Perch waits on this login as a Run waits on its client,
    // and a `claude` on an OAuth prompt has no session of its own to mark
    // (ADR a-run-is-one-shot). `profile::discard` takes it with the directory.
    let _live = probe::claim(host, &dir).ok();

    // Every way out from here takes the directory back out again, which a `?`
    // in the middle would quietly stop doing: one left by a failure is one
    // `reap_abandoned` will not tidy for thirty minutes.
    let produced = run_the_login(host, out, purpose, &claude, &store, &installed);
    profile::discard(host, &store);
    produced
}

fn run_the_login(
    host: &dyn Host,
    out: &mut dyn Write,
    purpose: &str,
    claude: &std::path::Path,
    store: &probe::Store,
    installed: &Installed,
) -> Result<Produced> {
    // Neither narrates a step Perch took: one is what the browser about to open
    // is for, the other an instruction somebody has to follow before the command
    // can finish (ADR perch-says-what-it-did).
    say(out, purpose)?;
    say(
        out,
        "Quit Claude Code when the login is done to come back here.\n",
    )?;

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
///
/// Through the same write `switch` patches the Default Profile's copy with,
/// which is what creates the file closed rather than at the process umask.
pub fn carry_identity_file(host: &dyn Host, contents: &str, store: &probe::Store) -> Result<()> {
    crate::host::write_atomically(host, &store.identity_file, contents)
        .map_err(|err| PerchError::file_write(store.identity_file.clone(), err))
}

/// How long a pending login is left alone before it is taken to have been
/// abandoned. Generous, because what is on the other side of it is a person
/// finding their password.
const ABANDONED_AFTER_MINUTES: i64 = 30;

/// Deletes what abandoned logins left behind.
///
/// Two things have to be true before one is reaped: it is older than any login
/// somebody could plausibly still be in, and nothing is running against it.
/// Silent and best-effort — this is tidying on the way to what was asked for.
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
        // Age is evidence and not proof, so the same evidence every other write
        // asks for (ADR a-profile-is-live-by-evidence): a login somebody is in
        // the middle of is a Live Profile, and nothing reaps one however old.
        if probe::anything_running(host, &dir) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::prelude::*;
    use crate::host::{Execution, FakeHost, fake::Effect};

    /// A writer that is not there — the ordinary closed pipe.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the pipe closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn a_machine_with_claude_code() -> FakeHost {
        FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 0,
                    stdout: "2.1.221 (Claude Code)\n".to_string(),
                    stderr: String::new(),
                },
            )
    }

    /// A closed pipe is the failure that needs no arranging: it lands between
    /// making the directory and discarding it.
    #[test]
    fn a_login_that_could_not_be_announced_takes_its_directory_back_out() {
        let host = a_machine_with_claude_code();
        let dir = registry::pending_login_dir(&host, host.now()).expect("home is known");

        assert!(
            perform(&host, &mut Closed, "why Perch is asking").is_err(),
            "the line before the browser could not be written"
        );

        assert!(
            host.effects()
                .iter()
                .any(|effect| matches!(effect, Effect::RemovedDir(at) if at == &dir)),
            "the directory it made for the login is gone again: {:?}",
            host.effects()
        );
    }
}
