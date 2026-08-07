//! `perch export <path>` — the whole machine, in one `age` file (ADR 0014).
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
//! travels under (ADR 0021).
//!
//! Nothing is Renewed, nothing is Rotated, and no Live Profile is written: an
//! Export reads what is stored, and refuses rather than writing a file that is
//! less than the whole.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::adopt;
use crate::commands::{ask_passphrase, ask_secret, refuse_without_a_terminal, say};
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::host::Host;

#[derive(Debug, Clone)]
pub struct ExportArgs {
    /// Where to write the `age` file.
    pub path: PathBuf,
}

pub fn run(host: &dyn Host, args: ExportArgs, out: &mut dyn Write) -> Result<()> {
    // Before the passphrase, because all three are refusals somebody should meet
    // before typing one twice — and before the registry is even read, because
    // none of them depends on what it says.
    refuse_without_a_terminal(host, "perch export")?;
    refuse_a_directory_that_is_not_there(host, &args.path)?;
    refuse_an_occupied_path(host, &args.path)?;

    let (_perch, registry) = adopt::ensure_adopted_exclusively(host, out)?;
    if registry.accounts.is_empty() {
        return Err(PerchError::NotFound(
            "Perch holds no Accounts, so there is nothing to export.\n\
             `perch add` logs one in."
                .to_string(),
        ));
    }

    let passphrase = agreed_passphrase(host, out)?;
    let export = export::gather(host, &registry)?;
    let sealed = export::seal(&export, &passphrase)?;

    // Asked again here, because the check above was two blocking questions ago
    // and the write below replaces whatever is at the path rather than failing
    // on it. A second `perch export` aimed at the same path — or anything else
    // that arrived while the passphrase was being typed — would otherwise be the
    // one thing this refusal exists to stop, just slower.
    refuse_an_occupied_path(host, &args.path)?;
    host.write_private_file(&args.path, &sealed)
        .map_err(|err| PerchError::file_write(args.path.clone(), err))?;

    report(out, &args.path, &export)
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
/// somebody typed. `perch export ~/backups/2026/perch.age` on a machine with no
/// `backups` is a typo more often than it is an instruction, and the repair is a
/// `mkdir` they meant to type.
fn refuse_a_directory_that_is_not_there(host: &dyn Host, path: &Path) -> Result<()> {
    // No parent, or an empty one, is the current directory: `perch export
    // perch.age` names somewhere that exists by definition.
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
fn agreed_passphrase(host: &dyn Host, out: &mut dyn Write) -> Result<String> {
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

/// What was written, and the one thing about it the user has to keep elsewhere.
fn report(out: &mut dyn Write, path: &Path, export: &Export) -> Result<()> {
    let accounts = export.accounts();
    say(
        out,
        &format!(
            "Exported {accounts} {} to {}, with everything the registry says \
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
                "Neither Credential Store held anything for {}, so the Export \
                 carries the {} without one — Quarantine reason and all. \
                 `perch relogin` is what ends that, and it is worth doing before \
                 this file is the only copy.",
                bare.join(", "),
                if bare.len() == 1 {
                    "Account"
                } else {
                    "Accounts"
                },
            ),
        )?;
    }

    say(
        out,
        "Keep the passphrase somewhere that is not beside the file. Without it \
         there is nothing in there, and nothing Perch holds can get it back.",
    )
}
