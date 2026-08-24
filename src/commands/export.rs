//! `perch holdings export <path>` — the whole machine, in one `age` file
//! (ADR the-holdings-go-out-sealed).
//!
//! The only command that turns the Credentials in a Credential Store into a
//! file, and the only artifact that makes a dead machine, a mistaken
//! `perch remove` or a new laptop cost less than a login per subscription.
//!
//! It takes everything and has no target, the passphrase is required rather than
//! offered and never taken as an argument, and nothing is Renewed, Rotated or
//! written into a Live Profile.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::adopt;
use zeroize::Zeroizing;

use crate::commands::{ask_passphrase, ask_secret, refuse_without_a_terminal, say, still_ours};
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::host::Host;
use crate::registry::Registry;

pub fn run(host: &dyn Host, path: &Path, out: &mut dyn Write) -> Result<()> {
    // Before the passphrase, because all three are refusals somebody should meet
    // before typing one twice — and before the registry is read, since reading it
    // adopts the login on a fresh machine (ADR a-login-perch-does-not-need).
    refuse_without_a_terminal(host, "perch holdings export")?;
    refuse_a_directory_that_is_not_there(host, path)?;
    refuse_an_occupied_path(host, path)?;
    refuse_a_path_perchs_home_would_take(host, path)?;

    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    let mut landed = None;
    let written = write_the_export(host, &mut perch, &mut registry, path, &mut landed, out);
    match (written, landed) {
        // The bytes land before the report, so a terminal that has gone away
        // fails a command whose file is there — and a re-run is refused for the
        // path being taken, which reads as somebody else's file.
        (Err(error), Some(at)) => Err(error.with_note(&format!(
            "The Export at {} was written and holds a working Credential for \
             every Account. Only saying so failed, so there is nothing to run \
             again — keep it somewhere you would keep those, or delete it.",
            at.display(),
        ))),
        (written, _) => written,
    }
}

/// Everything an Export is, given a registry somebody else has read: the path
/// refusals, the passphrase, the gather, the seal, the file and the report.
///
/// Shared with `perch holdings purge`, which holds the registry lock across the
/// offer it makes — so it cannot go through [`run`] and wait out its own hold.
pub fn write_the_export(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    path: &Path,
    landed: &mut Option<PathBuf>,
    out: &mut dyn Write,
) -> Result<()> {
    refuse_a_directory_that_is_not_there(host, path)?;
    refuse_an_occupied_path(host, path)?;
    refuse_a_path_perchs_home_would_take(host, path)?;

    // Before a Credential Store is read: during a Landing the live one may be
    // either Account's, so each Credential would be gathered out of its own
    // Profile — where the outgoing Account's is the copy a Rotation retired.
    crate::commands::a_settled_landing(host, perch, registry)?;

    if registry.accounts.is_empty() {
        return Err(PerchError::NotFound(
            "Perch holds no Accounts, so there is nothing to export.\n\
             `perch add` logs one in."
                .to_string(),
        ));
    }

    let passphrase = agreed_passphrase(host, out)?;
    let export = export::gather(host, registry)?;
    let sealed = export::seal(&export, &passphrase)?;

    // Asked again, because the check above was two blocking questions ago and the
    // write below replaces whatever is at the path rather than failing on it.
    refuse_an_occupied_path(host, path)?;

    // And the hold, over the same window. The registry was read before the
    // prompts, and sealing a copy another `perch` has since added to writes a
    // file presenting itself as *everything* Perch holds while being partial.
    still_ours(perch, "exported")?;
    host.write_private_file(path, &sealed)
        .map_err(|err| PerchError::file_write(path.to_path_buf(), err))?;

    // Recorded the instant the bytes land, and before the one fallible thing
    // left: `report` writes to a terminal that may have gone away, and an `Err`
    // there is not the same thing as an Export that was never written.
    *landed = Some(path.to_path_buf());

    report(out, path, &export)
}

/// Refuses to write the Export inside Perch's own home.
///
/// Here rather than at either command, because both doors owe it: a Purge that
/// wrote its offer there deletes it moments later, and `perch holdings export`
/// writes the same file at the same path with no Purge in sight yet.
fn refuse_a_path_perchs_home_would_take(host: &dyn Host, path: &Path) -> Result<()> {
    let home = crate::registry::perch_home(host)?;
    if !crate::host::is_inside(host, path, &home) {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "{} is inside {}, which is Perch's own — and `perch holdings purge` \
         deletes that directory whole, so an Export written there goes with the \
         Holdings it is the only copy of.\n\
         Name a path somewhere Perch does not own.",
        path.display(),
        home.display(),
    )))
}

/// Refuses to write over whatever is already at the path.
///
/// The one argument is a path somebody typed and what this does with it is
/// replace the file there — so a mistyped one makes a backup command destroy
/// what it was pointed at, and one Export on another is the older backup gone.
fn refuse_an_occupied_path(host: &dyn Host, path: &Path) -> Result<()> {
    if !host.path_exists(path) {
        return Ok(());
    }
    Err(PerchError::Conflict(format!(
        "{} is already there, and an Export is never written over anything.\n\
         Name a path that is free, or move what is at that one first.",
        path.display(),
    )))
}

/// Refuses a path whose directory is not there, rather than making it.
///
/// The private write below would create it, and every directory above it, at
/// 0700 — right for the ones Perch owns and presumptuous for a path somebody
/// typed, where a missing directory is a typo more often than an instruction.
fn refuse_a_directory_that_is_not_there(host: &dyn Host, path: &Path) -> Result<()> {
    // No parent, or an empty one, is the current directory: `perch holdings
    // export perch.age` names somewhere that exists by definition.
    let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) else {
        return Ok(());
    };
    if host.path_exists(dir) {
        return Ok(());
    }
    Err(PerchError::NotFound(format!(
        "{} is not a directory that exists, so there is nowhere to write {}.\n\
         Perch will not make it: a directory it created for a path you typed \
         would be one you did not ask for, at permissions you did not choose.",
        dir.display(),
        path.display(),
    )))
}

/// The passphrase, asked for twice and never shown.
///
/// Confirmed because the failure is silent until it isn't: a passphrase mistyped
/// once is a file nobody finds out is unreadable until the machine it would have
/// restored is gone. Empty is the same skip, typed rather than configured.
fn agreed_passphrase(host: &dyn Host, out: &mut dyn Write) -> Result<Zeroizing<String>> {
    say(
        out,
        "This file holds a working Credential for every Account Perch has. It is \
         encrypted with a passphrase you choose, and there is no way into it \
         without one.",
    )?;

    let Some(typed) = ask_passphrase(host, out, "Passphrase: ")? else {
        return Err(PerchError::Invalid(
            "No passphrase was typed, and an Export cannot be written without \
             one. Nothing was written."
                .to_string(),
        ));
    };

    if ask_secret(host, out, "Again: ")?.unwrap_or_default() != typed {
        return Err(PerchError::Invalid(
            "Those two do not match. Nothing was written, because a passphrase \
             mistyped here is a file nobody discovers is unreadable until the \
             machine it would have restored is gone."
                .to_string(),
        ));
    }
    Ok(typed)
}

/// What was written.
///
/// What an Export carries is what an Export is, and the prompt above says where
/// the passphrase is kept — so neither is said again where every run would say
/// it (ADR perch-says-what-it-did). The Accounts without a Credential are.
fn report(out: &mut dyn Write, path: &Path, export: &Export) -> Result<()> {
    let accounts = export.accounts();
    say(
        out,
        &format!(
            "Exported {} to {}.",
            crate::commands::accounts(accounts),
            path.display(),
        ),
    )?;

    let bare = export.without_a_credential();
    if !bare.is_empty() {
        say(
            out,
            &format!(
                "Neither Credential Store held anything for {}, so the Export \
                 carries the {} without one — Quarantine reason and all. \
                 `perch relogin` is what ends that, and it is worth doing before \
                 this file is the only copy.",
                bare.join(", "),
                match bare.len() {
                    1 => "Account",
                    _ => "Accounts",
                },
            ),
        )?;
    }

    Ok(())
}
