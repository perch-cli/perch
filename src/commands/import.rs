//! `perch holdings import <path>` — a whole machine, put back
//! (ADR the-holdings-go-out-sealed).
//!
//! The exact inverse of `perch holdings export`: the registry and every
//! Credential, so a new laptop arrives with the setup the old one had rather
//! than a pile of nameless logins.
//!
//! It refuses a machine that already holds an Account, it adopts nothing —
//! [`crate::adopt`] would make the machine non-empty on the way to refusing
//! itself for being non-empty — and nothing is made active.

use std::io::Write;
use std::path::Path;

use zeroize::Zeroizing;

use crate::ask;
use crate::commands::{say, still_ours};
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::holdings;
use crate::host::{Host, HostError};
use crate::import;
use crate::probe::Installed;
use crate::registry;

pub fn run(host: &dyn Host, path: &Path, out: &mut dyn Write) -> Result<()> {
    // Both before the passphrase, because both are refusals somebody should meet
    // before typing one. The file comes first: a path that is a typo is answered
    // by naming the path, not by advice about a machine nobody asked about.
    ask::needs_a_terminal(host, "perch holdings import")?;
    let sealed = read_the_file(host, path)?;

    let mut perch = holdings::lock(host)?;
    let held = registry::load(host)?;
    import::refuse_a_machine_that_is_not_empty(held.as_ref())?;

    let passphrase = the_passphrase(host, out)?;
    let (export, renamed) = export::unseal(&sealed, &passphrase)?;
    // Before anything is written, so a person who stops here has been told what
    // the Export would arrive as. `bring_forward` says the same about this
    // machine's own registry; an Import is the other way a registry moves.
    if let Some(said) = crate::migration::what_was_renamed_said(&renamed) {
        host.note(&said);
    }
    let mut restored = import::restored(&export, &holdings::registry_path(host)?)?;

    // Nothing above this line has written anything, and the passphrase prompt is
    // the one wait here with no bound on it — so the hold taken before it may be
    // one another `perch` has since claimed and put an Account down under.
    still_ours(&mut perch, "imported")?;
    let installed = Installed::probed_or_absent(host);
    let placed = import::place(host, &export, &installed)?;
    registry::save(host, &mut perch, &mut restored).map_err(|error| {
        placed.undo(host);
        error.with_note(
            "Nothing was imported. The Credentials this had already restored \
             have been taken back out again, and the file can be imported \
             again.",
        )
    })?;

    // The Import is complete by this line: every Credential is placed and the
    // registry is written. What is left is saying so, and raised bare, a terminal
    // that has gone away makes a machine that *is* restored exit non-zero.
    report(out, path, &export).map_err(|error| {
        error.with_note(
            "The Import itself finished: every Credential the Export held is \
             restored and Perch's registry is written. Only the report of it \
             could not be, so there is nothing to run again — `perch list` says \
             what arrived.",
        )
    })
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
        // An Export is `age`'s *armored* form, so the read is a read of text and
        // a binary `age` file fails UTF-8 decoding here, before any of `unseal`'s
        // four refusals can speak. Plain `age -p` writes the binary default.
        Err(HostError::Io(err)) if err.kind() == std::io::ErrorKind::InvalidData => {
            Err(PerchError::Invalid(format!(
                "{} is not text, so it is not an Export. An Export is `age`'s \
                 armored form, which is what `perch holdings export` writes \
                 and what `age -a -p` writes.\n\
                 A binary `age` file can be turned into one: `age -d <file> | \
                 age -a -p > <armored>`.",
                path.display(),
            )))
        }
        Err(err) => Err(PerchError::file_read(path.to_path_buf(), err)),
    }
}

/// The passphrase, asked for once and never shown.
///
/// Once rather than twice, and bare rather than after a preamble, the two places
/// an Import differs from the Export it mirrors: a passphrase being *checked* is
/// answered by the file, and no decision needs one (ADR perch-says-what-it-did).
fn the_passphrase(host: &dyn Host, out: &mut dyn Write) -> Result<Zeroizing<String>> {
    ask::a_passphrase(host, out, "Passphrase: ")?.ok_or_else(|| {
        PerchError::Invalid(
            "No passphrase was typed, and there is no way into an Export without \
             one. Nothing was imported."
                .to_string(),
        )
    })
}

/// What arrived.
///
/// Nothing arrives active on any Import and an Import carries the whole registry
/// on every one, so neither is said here: the guide establishes both. The
/// Accounts the file held no Credential for are what this can report.
fn report(out: &mut dyn Write, path: &Path, export: &Export) -> Result<()> {
    let accounts = export.accounts();
    say(
        out,
        &format!(
            "Imported {} from {}.",
            crate::commands::accounts(accounts),
            path.display(),
        ),
    )?;

    // The repair, which is nothing where nothing came back bare, so it is the
    // condition rather than a second thing asked after one. The mirror of this in
    // `export.rs` gets the plural right by not naming an Account at all.
    let bare = export.without_a_credential();
    if let Some(repair) = registry::how_to_repair_them(&bare) {
        say(
            out,
            &format!(
                "The Export held no Credential for {}, so the {} restored \
                 without one — Quarantine reason and all. {repair}",
                bare.join(", "),
                match bare.len() {
                    1 => "Account was",
                    _ => "Accounts were",
                },
            ),
        )?;
    }

    Ok(())
}
