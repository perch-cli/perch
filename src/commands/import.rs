//! `perch holdings import <path>` — a whole machine, put back
//! (ADR the-holdings-go-out-sealed).
//!
//! The exact inverse of `perch holdings export`: the Registry and every
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
use crate::commands::still_ours;
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::holdings;
use crate::host::{Host, HostError};
use crate::import;
use crate::registry;
use crate::say;
use crate::wait;

pub fn run(host: &dyn Host, path: &Path, out: &mut dyn Write) -> Result<()> {
    // Both before the passphrase, because both are refusals somebody should meet
    // before typing one. The file comes first: a path that is a typo is answered
    // by naming the path, not by advice about a machine nobody asked about.
    ask::needs_a_terminal(host, "perch holdings import")?;
    let sealed = read_the_file(host, path)?;

    let mut perch = holdings::lock(host)?;
    let held = registry::load(host)?;
    import::refuse_a_machine_that_is_not_empty(held.as_ref())?;

    let ((export, mut restored), (), fresh) = wait::across(
        &mut perch,
        |_| {
            let passphrase = the_passphrase(host, out)?;
            let export = export::unseal(&sealed, &passphrase)?;
            let restored = import::restored(&export, &holdings::registry_path(host)?)?;
            Ok((export, restored))
        },
        // The hold alone: a stale one means another `perch` may have put an
        // Account down, and re-taking it *is* the empty-machine re-ask.
        |perch| still_ours(perch, "imported"),
    )?;
    import::place(host, &export, &fresh, || {
        registry::save(host, &mut perch, &mut restored)
    })?;

    // The Import is complete by this line: every Credential is placed and the
    // Registry is written. What is left is saying so, and raised bare, a terminal
    // that has gone away makes a machine that *is* restored exit non-zero.
    report(out, &export).map_err(|error| {
        error.with_note("The Import finished. Only the report could not be printed.")
    })
}

/// The file, as the text `age` wrote. Read before anything else is decided,
/// because a path that is not there is the likeliest thing to be wrong about an
/// Import and the cheapest to say.
fn read_the_file(host: &dyn Host, path: &Path) -> Result<String> {
    match host.read_file(path) {
        Ok(sealed) => Ok(sealed),
        Err(HostError::NotFound { .. }) => Err(PerchError::NotFound(format!(
            "There is no file at {}.",
            path.display(),
        ))),
        // An Export is `age`'s *armored* form, so the read is a read of text and
        // a binary `age` file fails UTF-8 decoding here, before any of `unseal`'s
        // four refusals can speak. Plain `age -p` writes the binary default.
        Err(HostError::Io(err)) if err.kind() == std::io::ErrorKind::InvalidData => {
            Err(PerchError::Invalid(format!(
                "{} is not text, so it is not an Export. `age -d <file> | age -a \
                 -p > <armored>` makes one.",
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
            "No passphrase was typed, and nothing opens an Export without one.".to_string(),
        )
    })
}

/// What arrived.
///
/// Nothing arrives active on any Import and an Import carries the whole Registry
/// on every one, so neither is said here: the guide establishes both. The
/// Accounts the file held no Credential for are what this can report.
fn report(out: &mut dyn Write, export: &Export) -> Result<()> {
    let accounts = export.accounts();
    say::line(out, &format!("Imported {}.", say::accounts(accounts),))?;

    // The repair, which is nothing where nothing came back bare, so it is the
    // condition rather than a second thing asked after one. The mirror of this in
    // `export.rs` gets the plural right by not naming an Account at all.
    let bare = export.without_a_credential();
    if let Some(repair) = registry::how_to_repair_them(&bare) {
        say::line(
            out,
            &format!(
                "Note: the Export held no Credential for {}. {repair}",
                bare.iter()
                    .map(|key| export.registry.named_for_the_user(key))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        )?;
    }

    Ok(())
}
