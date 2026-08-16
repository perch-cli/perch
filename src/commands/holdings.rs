//! `perch holdings` — everything Perch holds on this machine (ADR 0047).
//!
//! Three commands that had no shared prefix because the noun they share had no
//! name. An Export writes the Holdings to one file, an Import puts them back,
//! and a Purge gives them up — and none of the three takes a Target, which is
//! the same fact about all three said once.
//!
//! Nothing of what they do is here. This is where the noun is written down and
//! the three verbs are placed under it; what each one *is* stays in
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
    /// Write everything Perch holds to one encrypted file.
    ///
    /// The registry and every Credential, in the `age` format, so a dead
    /// machine or a new laptop does not cost a login for every subscription
    /// (ADR 0014). There is no per-Account form: a selective export is a
    /// partial restore.
    ///
    /// The passphrase is prompted and confirmed, and cannot be passed as an
    /// argument — an argument sits in the process table for anything on this
    /// machine to read. Without a terminal to type it at, the export is
    /// refused.
    Export {
        /// Where to write the file. Nothing is written over: a path that is
        /// already taken is refused.
        path: PathBuf,
    },

    /// Put a whole machine back from a file `perch holdings export` wrote.
    ///
    /// The exact inverse of an export: the registry and every Credential, so a
    /// new machine arrives with the setup the old one had rather than a pile of
    /// nameless logins (ADR 0014). Credentials land wherever this machine's
    /// Claude Code keeps one, whatever store the file was written from.
    ///
    /// It refuses a Perch that already holds an Account and names `perch
    /// holdings purge` as the way to make room — merging two machines is a
    /// different feature. Nothing is made active by an import; `perch switch`
    /// is what lands.
    Import {
        /// The file to restore from. The passphrase is prompted, and a wrong
        /// one fails before anything is written.
        path: PathBuf,
    },

    /// Give the machine back the state it had before Perch.
    ///
    /// Every Profile, every Credential Perch holds and its own registry, gone in
    /// one act — the exact inverse of an import, and what makes room for one
    /// (ADR 0014). It takes no target: giving up one Account is `perch remove`.
    ///
    /// It offers to write an export first, lists the Accounts that will go by
    /// email address, and wants the word `purge` typed rather than a letter.
    /// Whatever Claude Code is logged in as is left exactly where it is.
    Purge {
        /// Purge without being asked, and write no export.
        ///
        /// An export is a path you name and a passphrase you type, neither of
        /// which a script can be asked for — so this answers both questions at
        /// once. Without a terminal and without this flag, a purge is refused.
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
