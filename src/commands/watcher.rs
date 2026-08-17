//! `perch watcher` — the Watcher, in each of the arrangements it comes in (ADR
//! 0047).
//!
//! Three arrangements and one behavior (ADR 0040), which the surface used to
//! split three ways: a bare verb, a flag on it, and a tree of its own. One noun
//! and five verbs is that same domain model, written down where a person types
//! it.
//!
//! `run` and `check` are the loop and one round of it, and are
//! [`crate::commands::watch`]'s. `install`, `uninstall` and `status` are about
//! the Service — an arrangement of the Watcher rather than a rival noun, which
//! is why it keeps its glossary entry and loses its tree — and are
//! [`crate::commands::service`]'s.
//!
//! `check` is a verb rather than the flag it was, because it changes both the
//! meaning of the exit code and the lifetime of the command, which is ADR
//! 0047's test. What it exits with is ADR 0013's table, unchanged.

use std::io::Write;

use crate::commands::{service, watch};
use crate::error::{EXIT_OK, Result};
use crate::host::Host;

/// What was asked of `perch watcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Subcommand)]
pub enum WatcherCommand {
    /// Watch the Account you are on, and Cycle when it runs low.
    ///
    /// A loop in this terminal rather than a daemon (ADR 0013): it runs until
    /// you stop it with Ctrl-C, and leaves nothing behind when you do. Only the
    /// active Account is read, and only within a Scope that has been told the
    /// watcher may act on it — `perch config set <group> watcher-may-act true`
    /// for a Group, or the same for `ungrouped` where `interchangeable` is on
    /// as well, because being interchangeable at all is its own yes (ADR 0017).
    ///
    /// Every decision is printed as it is made, including the ones where
    /// nothing happens, which are most of them. They go to standard output, so
    /// redirecting them to a file is yours to do.
    Run,

    /// Take one round and exit, saying what it decided in the exit code.
    ///
    /// For cron or a systemd timer, which is where an unattended watcher
    /// belongs: scheduling is the operating system's job. The policy is the
    /// loop's, run once — and the cooldown that paces it survives between
    /// invocations, so a check every minute still switches no more often than
    /// the cooldown allows.
    Check,

    /// Have this machine run the Watcher for you, starting when you log in.
    ///
    /// The same loop `perch watcher run` runs, supervised by the machine's own
    /// service manager rather than by a terminal you have to keep open (ADR
    /// 0040): a LaunchAgent on macOS, a `systemd --user` unit on Linux, a
    /// Scheduled Task on Windows. Perch never backgrounds itself and there is
    /// no `--detach` — it writes a unit and hands the job over, because
    /// scheduling and supervision are the operating system's.
    ///
    /// Always yours rather than the machine's, and always at login rather than
    /// at boot: every Profile Perch holds is under your home directory, and on
    /// macOS there is no unlocked keychain before somebody logs in. Installing
    /// it as root is refused for that reason.
    ///
    /// Idempotent, and re-running it is the repair for a Service whose binary
    /// moved — which `perch upgrade` does whenever it routes through Homebrew
    /// or npm.
    Install,

    /// Stop the Service and take its unit back.
    ///
    /// `perch watcher run` in a terminal is unaffected, and nothing Perch holds
    /// is touched.
    Uninstall,

    /// Say whether a Service is installed, whether it is running, and whether a
    /// Watcher holds the lock right now.
    ///
    /// A question, so it succeeds either way — branch on `--json` rather than
    /// on the exit code.
    Status {
        /// Report as JSON for whatever is parsing this.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(host: &dyn Host, command: WatcherCommand, out: &mut dyn Write) -> Result<i32> {
    match command {
        WatcherCommand::Run => watch::keep_watching(host, out).map(|()| EXIT_OK),
        WatcherCommand::Check => watch::check(host, out),
        WatcherCommand::Install => service::install(host, out),
        WatcherCommand::Uninstall => service::uninstall(host, out),
        WatcherCommand::Status { json } => service::status(host, json, out),
    }
}
