//! `perch holdings` — everything Perch holds on this machine
//! (ADR a-command-names-its-noun).
//!
//! Three commands that had no shared prefix because the noun they share had no
//! name. An Export writes the Holdings to one file, an Import puts them back,
//! and a Purge gives them up — and none of the three takes a Target.
//!
//! Nothing of what they do is here: what each one *is* stays in
//! [`crate::commands::export`], [`crate::commands::import`] and
//! [`crate::commands::purge`].

use std::io::Write;
use std::path::PathBuf;

use crate::commands::{export, import, purge};
use crate::error::Result;
use crate::host::Host;

/// What was asked of `perch holdings`.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum HoldingsCommand {
    /// Write the Registry and every Credential to one encrypted file
    Export {
        /// Where to write the Export
        path: PathBuf,
    },

    /// Restore a machine from an Export
    Import {
        /// The Export to restore from
        path: PathBuf,
    },

    /// Delete every Profile, every Credential and the Registry
    Purge {
        /// Ask nothing, and write no Export
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(host: &dyn Host, command: HoldingsCommand, out: &mut dyn Write) -> Result<()> {
    match command {
        HoldingsCommand::Export { path } => export::run(host, &path, out),
        HoldingsCommand::Import { path } => import::run(host, &path, out),
        HoldingsCommand::Purge { yes } => purge::run(host, yes, out),
    }
}
