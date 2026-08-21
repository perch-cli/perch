//! `perch holdings export <path>` — the whole machine, in one `age` file (ADR
//! the-holdings-go-out-sealed).
//!
//! The only command that turns the Credentials in a Credential Store into a
//! file, and the only artifact that makes a dead machine, a mistaken `perch
//! remove` or a new laptop cost anything less than a login for every
//! subscription.
//!
//! **It takes everything and has no target.** No `--account`, no Group: a
//! selective export is a partial restore, which is the failure this file exists
//! to prevent wearing a feature's clothes. So the surface is a path, and the
//! command is the same command however many Accounts Perch holds.
//!
//! **The passphrase is required, not offered.** An optional one is one people
//! skip, and the failure is silent until it isn't. It is prompted, confirmed,
//! and never taken as an argument — an argument sits in the process table for
//! anything on the machine to read, which is the same rule an access token
//! travels under (ADR a-crate-must-not-cost-a-seam).
//!
//! Nothing is Renewed, nothing is Rotated, and no Live Profile is written: an
//! Export reads what is stored, and refuses rather than writing a file that is
//! less than the whole.

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
    // Before the passphrase, because all three are refusals somebody should
    // meet before typing one twice — and before the registry is even read,
    // because none of them depends on what it says and reading it is what
    // adopts the login on a machine Perch has never run on
    // (ADR a-login-perch-does-not-need). The two about the path are asked again
    // below, where the write is: what refuses this command early has to refuse
    // `perch holdings purge` too, and the check that lives in only one of two
    // callers is the check that stops being made.
    refuse_without_a_terminal(host, "perch holdings export")?;
    refuse_a_directory_that_is_not_there(host, path)?;
    refuse_an_occupied_path(host, path)?;

    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
    // Before anything is read out of a Credential Store, for the reason every
    // other command that reads through the live one settles first
    // (ADR a-switch-is-written-down-first): a registry holding a Landing
    // answers "who is active" with the Account being *left*, and the live
    // Credential during one may be either Account's. An Export that took the
    // live copy on that answer wrote one Account's refresh token under the
    // other's address — a file that restores two Accounts onto one token, which
    // the first Renewal then Rotates out from under one of them.
    crate::switch::resolve_a_landing(host, &mut perch, &mut registry)?;

    // Nothing to hand back to: this command's own failure says where the file
    // is, because the path is the argument the person typed.
    let mut landed = None;
    write_the_export(host, &mut perch, &registry, path, &mut landed, out)
}

/// Everything an Export is, given a registry somebody else has read: the path
/// refusals, the passphrase, the gather, the seal, the file and what was
/// written.
///
/// Shared with `perch holdings purge`, which offers to write one before it
/// destroys anything (ADR the-holdings-go-out-sealed) and holds the registry
/// lock across the offer — so it cannot go through [`run`], which would take
/// that lock a second time and wait out its own hold.
pub fn write_the_export(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &Registry,
    path: &Path,
    landed: &mut Option<PathBuf>,
    out: &mut dyn Write,
) -> Result<()> {
    refuse_a_directory_that_is_not_there(host, path)?;
    refuse_an_occupied_path(host, path)?;
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

    // Asked again here, because the check above was two blocking questions ago
    // and the write below replaces whatever is at the path rather than failing
    // on it. A second `perch holdings export` aimed at the same path — or
    // anything else that arrived while the passphrase was being typed — would
    // otherwise be the one thing this refusal exists to stop, just slower.
    refuse_an_occupied_path(host, path)?;

    // And the hold, over the same window and for the same reason the two
    // destructive commands re-check theirs. The registry above was read before
    // the passphrase prompts; another `perch add` that claimed a stale lock
    // while somebody was typing has recorded an Account this copy does not
    // hold, and sealing that copy writes a file presenting itself as
    // *everything* Perch holds. A selective Export is the failure the whole
    // format exists to prevent, and one wearing a complete Export's clothes is
    // worse than a refusal — it is only found out at the restore.
    still_ours(perch, "exported")?;
    host.write_private_file(path, &sealed)
        .map_err(|err| PerchError::file_write(path.to_path_buf(), err))?;

    // Recorded the instant the bytes land, and before the one fallible thing
    // left. `report` writes to the terminal, and a terminal that has gone away
    // — a closed pty, a SIGHUP — fails it: the caller then saw an `Err` and
    // concluded no Export had been written, while an armored file holding a
    // working Credential for every Account sat at a path nothing had mentioned.
    // The next `perch holdings purge` offering that path then aborted before
    // asking anything, because an Export refuses a path that is taken — which
    // is the sequence `purge::still_standing` exists to close.
    *landed = Some(path.to_path_buf());

    report(out, path, &export)
}

/// Refuses to write over whatever is already at the path.
///
/// The one argument this command takes is a path somebody typed, and what it
/// does with it is replace the file there. A mistyped one is otherwise a backup
/// command that destroys the thing it was pointed at — and an Export that lands
/// on a previous Export is the older backup gone, which is exactly the artifact
/// this command exists to accumulate.
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
/// 0700 — which is right for the ones Perch owns and presumptuous for a path
/// somebody typed. `perch holdings export ~/backups/2026/perch.age` on a
/// machine with no `backups` is a typo more often than it is an instruction,
/// and the repair is a `mkdir` they meant to type.
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
/// restored is already gone. Empty is refused for the same reason an optional
/// passphrase was — it is the same skip, typed rather than configured.
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
/// What an Export carries is what an Export is, and keeping the passphrase away
/// from the file is what the prompt above says while somebody is choosing one
/// — so neither is said again here, where every run would say it
/// (ADR perch-says-what-it-did). The Accounts that came without a Credential
/// are the one thing this can report that another Export would not.
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
