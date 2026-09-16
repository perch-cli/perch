//! `perch watcher` — one noun and five verbs, for the three arrangements the Watcher
//! comes in (ADR a-command-names-its-noun).
//!
//! `run` and `check` are the loop and one round of it, and are
//! [`crate::commands::watch`]'s. `install`, `uninstall` and `status` are about the
//! Service — an arrangement of the Watcher rather than a rival noun, which is why
//! it keeps its glossary entry and has no tree of its own — and are
//! [`crate::commands::service`]'s. `check` is a verb rather than a flag on `run`
//! because it changes both the meaning of the exit code and the lifetime of the
//! command.

use std::io::Write;

use crate::commands::{service, watch};
use crate::error::{EXIT_OK, Result};
use crate::host::Host;

/// What was asked of `perch watcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Subcommand)]
pub enum WatcherCommand {
    /// Watch the active Account and Cycle when it runs low
    Run,

    /// Take one round and exit, for cron or a timer
    Check,

    /// Run the Watcher as a Service, starting when you log in
    Install,

    /// Stop the Service and remove its unit
    Uninstall,

    /// Say whether a Service is installed, running, and watching
    Status {
        /// Print JSON
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
