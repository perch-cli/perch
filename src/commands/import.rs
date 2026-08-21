//! `perch holdings import <path>` — a whole machine, put back
//! (ADR the-holdings-go-out-sealed).
//!
//! The exact inverse of `perch holdings export`: the registry and every
//! Credential, so a new laptop arrives with the setup the old one had rather
//! than a pile of nameless logins. That pair — an Export written before a
//! Purge, an Import after it — is the whole of what makes "I can move to a new
//! machine" true.
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
use std::path::Path;

use zeroize::Zeroizing;

use crate::commands::{ask_passphrase, refuse_without_a_terminal, say, still_ours};
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::host::{Host, HostError};
use crate::import;
use crate::registry;

pub fn run(host: &dyn Host, path: &Path, out: &mut dyn Write) -> Result<()> {
    // Both before the passphrase, because both are refusals somebody should meet
    // before typing one. The file comes first: a path that is a typo is answered
    // by naming the path, not by advice about a machine the user was never
    // asking about.
    refuse_without_a_terminal(host, "perch holdings import")?;
    let sealed = read_the_file(host, path)?;

    let mut perch = registry::lock(host)?;
    let held = registry::load(host)?;
    import::refuse_a_machine_that_is_not_empty(held.as_ref())?;

    let passphrase = the_passphrase(host, out)?;
    let export = export::unseal(&sealed, &passphrase)?;
    let restored = import::restored(&export, &registry::registry_path(host)?)?;

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

    // The Import is complete by this line: every Credential is placed and the
    // registry is written. What is left is saying so, and a terminal that has
    // gone away — a closed pty, a `SIGHUP`, a pipe whose reader exited — makes
    // that write fail. Raised bare, a machine that *is* restored reported a
    // non-zero exit with nothing saying otherwise, and the obvious next move
    // then hits `refuse_a_machine_that_is_not_empty`, whose advice is that
    // `perch holdings purge` "gives the machine back and is what makes room".
    //
    // `commands::export` carries `landed` for this and `purge::still_standing`
    // closes the same gap; an Import is the one of the three that never got it.
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
        // An Export is `age`'s *armored* form — its text encoding — which is
        // what lets it go through the Host port's ordinary private write. So
        // the read is a read of text, and a binary `age` file fails UTF-8
        // decoding here, before `unseal` ever sees it and before any of the
        // four refusals it distinguishes can speak.
        //
        // Worth naming rather than passing through as "stream did not contain
        // valid UTF-8", because the person who reaches it is somebody who took
        // their own backup with plain `age -p` — the binary default — and is
        // reading this on the day the machine it would have restored is gone.
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
/// Once rather than twice, which is the one place an Import differs from the
/// Export it mirrors: a passphrase being *chosen* is confirmed because a file
/// nobody can open is not discovered until it is needed, and a passphrase being
/// *checked* is answered by the file itself a moment later.
///
/// And asked bare, which is the second place. The Export's prompt is preceded
/// by what the passphrase is protecting, because somebody choosing one has a
/// decision to make and no way back from it; there is no decision here, and a
/// preamble before every Import would be prose earning its place from the
/// question rather than from the answer (ADR perch-says-what-it-did).
fn the_passphrase(host: &dyn Host, out: &mut dyn Write) -> Result<Zeroizing<String>> {
    ask_passphrase(host, out, "Passphrase: ")?.ok_or_else(|| {
        PerchError::Invalid(
            "No passphrase was typed, and there is no way into an Export without \
             one. Nothing was imported."
                .to_string(),
        )
    })
}

/// What arrived.
///
/// Nothing arrives active on any Import and an Import carries the whole
/// registry on every one, so neither is said here: both are what the guide
/// establishes once rather than what this repeats (ADR perch-says-what-it-did).
/// The Accounts the file held no Credential for are the one thing this can
/// report that another Import would not.
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

    // The repair, which is nothing where nothing came back bare, and is the
    // whole of what this paragraph is for — so it is the condition rather than
    // a second thing asked after one. The mirror of this in `export.rs` gets
    // the plural right by not naming an Account at all.
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
