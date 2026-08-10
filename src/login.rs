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
    // Everything that can fail without leaving anything behind happens first,
    // so the directory is made only once nothing before it can refuse. All
    // three are derivations rather than effects: which Claude Code is
    // installed, where to find it, and what a Profile at that path would be
    // called.
    let version = probe::claude_version(host)?;
    let claude = probe::claude_bin(host)?;
    let dir = registry::pending_login_dir(host, host.now())?;
    let store = probe::store_for_profile(host, &dir)?;

    // The login writes its Credential in here, so this is as much a place a
    // Credential lives as a Profile is (ADR 0020).
    host.create_private_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    // And Perch says so itself, before the browser opens. `reap_abandoned`
    // protects a login somebody is in the middle of by asking whether anything is
    // running against the directory — which reads a session marker, and the only
    // thing that writes one is `perch run`. Nothing wrote one here, so the
    // protection its comment describes did not exist: a `perch watch --once` from
    // cron, firing while somebody hunted for their second factor, deleted the
    // Credential the login had just written and left the `perch add` driving it
    // reporting that the login did not complete.
    //
    // Perch's own pid, because Perch is waiting on this login exactly as a Run
    // waits on its client — so ADR 0027's argument that a Run may corroborate its
    // own Profile holds here word for word. A `claude` sitting on an OAuth prompt
    // in a directory it has never had a session in is the least likely thing to
    // have written a marker of its own, which is why depending on it was the
    // wrong way round. `profile::discard` takes it with the directory.
    mark_live(host, &dir);

    // From here every way out has to take the directory back out again, which
    // is what the doc above promises and what `?` in the middle of this would
    // quietly stop doing. A pending login nobody reaps is only a directory —
    // no Credential has been written yet — but `reap_abandoned` exists because
    // they accumulate, and one left by a failure is one it will not tidy for
    // thirty minutes.
    let produced = run_the_login(host, out, purpose, &claude, &dir, &store, &version);
    profile::discard(host, &store);
    produced
}

/// Says that Perch is driving a login in this directory, so nothing reaps it.
///
/// Best effort, unlike the same write in a Run. A Run refuses when it cannot mark
/// its Profile, because what it is protecting is a Credential a client is about to
/// hold for hours; here the directory holds nothing yet, the window is thirty
/// minutes, and refusing a login over it would turn a tidying-up detail into a
/// reason somebody cannot add an Account at all. What is lost without it is the
/// protection alone, which is where this started.
fn mark_live(host: &dyn Host, dir: &std::path::Path) {
    let pid = host.process_id();
    if host.create_dir_all(&probe::sessions_dir(dir)).is_ok() {
        let _ = host.write_file(
            &probe::session_marker_at(dir, pid),
            &probe::session_marker(pid, host.now()),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_the_login(
    host: &dyn Host,
    out: &mut dyn Write,
    purpose: &str,
    claude: &std::path::Path,
    dir: &std::path::Path,
    store: &probe::Store,
    version: &str,
) -> Result<Produced> {
    say(out, purpose)?;
    say(
        out,
        "Quit Claude Code when the login is done to come back here.\n",
    )?;

    let status = host
        .exec_interactive(
            &claude.to_string_lossy(),
            &[],
            &[("CLAUDE_CONFIG_DIR", &dir.to_string_lossy())],
        )
        .map_err(|err| PerchError::Other(format!("could not launch a login: {err}")))?;

    what_the_login_left(host, store, version, status)
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
///
/// Through the same write `switch` patches the Default Profile's copy with,
/// which is where the rule about this file's mode is written down: `.claude.json`
/// holds MCP configuration, an MCP server entry routinely carries an API key in
/// its `env` block, and a file Perch is the first to create is created closed
/// rather than open (ADR 0020). A plain `write_file` creates at the process
/// umask, so every Profile `perch add`, `perch relogin` and an Import made held
/// that file at 0644 — and because the rule for a file that already exists is to
/// *carry its mode across*, it stayed 0644 for the life of the Profile while a
/// Carry wrote the person's `projects` entry into it on every Run. On unix the
/// 0700 Profile directory contained the damage; on Windows nothing narrows
/// either the directory or the file.
pub fn carry_identity_file(host: &dyn Host, contents: &str, store: &probe::Store) -> Result<()> {
    crate::host::write_atomically(host, &store.identity_file, contents)
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
/// Two things have to be true before one is reaped: it is older than any login
/// somebody could plausibly still be in, and nothing is running against it. The
/// second is what the first cannot promise — a login somebody is still driving
/// in another terminal must never be reaped out from under them, and age alone
/// says nothing about that.
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
        // Age is evidence and not proof. Thirty minutes is not generous for a
        // login that wants a password manager, a second factor and a browser
        // that opened on the wrong profile — and this runs at the start of
        // every command, so any `perch status` typed in another terminal was
        // free to delete the Credential Claude Code had just written and leave
        // the `perch add` driving it reporting that the login did not complete.
        //
        // So the same evidence every other write asks for (ADR 0022): a session
        // marker naming a process that is still the one that wrote it. A login
        // somebody is in the middle of is a Live Profile, and nothing reaps one
        // however old it is.
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

    /// The directory a login runs in comes back out however `perform` ends.
    ///
    /// The doc above says so — "removed whether it worked or not" — and it was
    /// true of the login itself and not of the steps around it: several `?` sat
    /// between making the directory and discarding it, so a failure at any of
    /// them left a pending login behind that `reap_abandoned` would not tidy
    /// for thirty minutes. A closed pipe is the one that needs no arranging.
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
