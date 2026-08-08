//! `perch import <path>` — a whole machine, put back (ADR 0014).
//!
//! The exact inverse of `perch export`: the registry and every Credential, so a
//! new laptop arrives with the setup the old one had rather than a pile of
//! nameless logins. That pair — an Export written before a Purge, an Import
//! after it — is the whole of what makes "I can move to a new machine" true.
//!
//! **It refuses a machine that already holds an Account.** Merging is where
//! every hard case lives, and refusing keeps an Import the exact inverse of a
//! Purge rather than a second, quieter way of changing what Perch holds.
//!
//! **It adopts nothing.** Every other command reads the registry through
//! [`crate::adopt`], which takes the existing Claude Code login over the first
//! time Perch runs — which here would be an Import that made the machine
//! non-empty on the way to refusing itself for being non-empty. So this one
//! reads the registry directly, and the login that is on the machine is left
//! exactly where it is.
//!
//! **Nothing is made active.** The Account that was active where the Export was
//! taken says nothing about what this machine is running as, so the user
//! Switches afterwards.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::commands::{ask_passphrase, refuse_without_a_terminal, say, still_ours};
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::host::{Host, HostError};
use crate::import;
use crate::registry;

#[derive(Debug, Clone)]
pub struct ImportArgs {
    /// The `age` file to restore from.
    pub path: PathBuf,
}

pub fn run(host: &dyn Host, args: ImportArgs, out: &mut dyn Write) -> Result<()> {
    // Both before the passphrase, because both are refusals somebody should meet
    // before typing one. The file comes first: a path that is a typo is answered
    // by naming the path, not by advice about a machine the user was never
    // asking about.
    refuse_without_a_terminal(host, "perch import")?;
    let sealed = read_the_file(host, &args.path)?;

    let mut perch = registry::lock(host)?;
    let held = registry::load(host)?;
    import::refuse_a_machine_that_is_not_empty(held.as_ref())?;

    let passphrase = the_passphrase(host, out)?;
    let export = export::unseal(&sealed, &passphrase)?;
    let restored = import::restored(&export)?;

    // Nothing above this line has written anything, which is the whole of what
    // "a wrong passphrase fails before anything is written" means.
    //
    // And the last thing before something is: the passphrase prompt above is
    // the one wait here with no bound on it, so the hold taken before it may
    // have gone stale under another `perch` that has since taken it and added
    // an Account. Asked here rather than at the save, because `place` writes
    // every Credential the file holds — and finding out at the save is finding
    // out after the rollback has deleted whatever that other Perch put down.
    still_ours(&mut perch, "imported")?;
    let placed = import::place(host, &export)?;
    registry::save(host, &mut perch, &restored).map_err(|error| {
        placed.undo(host);
        error.with_note(
            "Nothing was imported. The Credentials this had already restored \
             have been taken back out again, so the machine is as it was and \
             the file can be imported again.",
        )
    })?;

    report(out, &args.path, &export)
}

/// The file, as the text `age` wrote. Read before anything else is decided,
/// because a path that is not there is the likeliest thing to be wrong about an
/// Import and the cheapest to say.
fn read_the_file(host: &dyn Host, path: &Path) -> Result<String> {
    match host.read_file(path) {
        Ok(sealed) => Ok(sealed),
        Err(HostError::NotFound { .. }) => Err(PerchError::NotFound(format!(
            "There is no file at {}, so there is nothing to import.",
            path.display(),
        ))),
        Err(err) => Err(PerchError::file_read(path.to_path_buf(), err)),
    }
}

/// The passphrase, asked for once and never shown.
///
/// Once rather than twice, which is the one place an Import differs from the
/// Export it mirrors: a passphrase being *chosen* is confirmed because a file
/// nobody can open is not discovered until it is needed, and a passphrase being
/// *checked* is answered by the file itself a moment later.
fn the_passphrase(host: &dyn Host, out: &mut dyn Write) -> Result<String> {
    say(
        out,
        "This file is encrypted with the passphrase it was written with. Nothing \
         is restored until it opens.",
    )?;

    ask_passphrase(host, out, "Passphrase: ")?.ok_or_else(|| {
        PerchError::Invalid(
            "No passphrase was typed, and there is no way into an Export without \
             one. Nothing was imported."
                .to_string(),
        )
    })
}

/// What arrived, and the one thing the user has to do next.
fn report(out: &mut dyn Write, path: &Path, export: &Export) -> Result<()> {
    let accounts = export.accounts();
    say(
        out,
        &format!(
            "Imported {accounts} {} from {}, with everything the registry said \
             about them: their Aliases, their Groups, whether Cycling may choose \
             them, and what each Group carries.",
            if accounts == 1 { "Account" } else { "Accounts" },
            path.display(),
        ),
    )?;

    let bare = export.without_a_credential();
    if !bare.is_empty() {
        say(
            out,
            &format!(
                "The Export held no Credential for {}, so the {} restored \
                 without one — Quarantine reason and all. {}",
                bare.join(", "),
                if bare.len() == 1 {
                    "Account was"
                } else {
                    "Accounts were"
                },
                registry::how_to_repair(bare[0]),
            ),
        )?;
    }

    say(
        out,
        "Nothing is active: an Import restores what Perch holds and does not \
         touch what Claude Code is logged in as. `perch switch <target>` makes \
         one of them active.",
    )
}
