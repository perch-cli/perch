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

use crate::ask;
use crate::commands::still_ours;
use crate::error::{PerchError, Result};
use crate::export::{self, Export};
use crate::host::Host;
use crate::registry::Registry;
use crate::say;
use crate::wait;

pub fn run(host: &dyn Host, path: &Path, out: &mut dyn Write) -> Result<()> {
    // Before the passphrase, because the refusals are ones somebody should meet
    // before typing one twice — and before the registry is read, since reading it
    // adopts the login on a fresh machine (ADR a-login-perch-does-not-need).
    ask::needs_a_terminal(host, "perch holdings export")?;
    let mut destination = Destination::for_an_export(host, path)?;

    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    let installed = crate::probe::Installed::for_a_report(host);

    let written = write_the_export(
        host,
        &mut perch,
        &mut registry,
        &mut destination,
        &installed,
        out,
    );
    match (written, destination.landed()) {
        // The bytes land before the report, so a terminal that has gone away
        // fails a command whose file is there — and a re-run is refused for the
        // path being taken, which reads as somebody else's file.
        (Err(error), Some(at)) => Err(error.with_note(&format!(
            "The Export at {} was written and holds a working Credential for \
             every Account. Only saying so failed, so there is nothing to run \
             again. Keep it somewhere you would keep those, or delete it.",
            at.display(),
        ))),
        (written, _) => written,
    }
}

/// Everything an Export is, given a registry somebody else has read and a
/// [`Destination`] already proven fit: passphrase, gather, seal, file, report.
///
/// Shared with `perch holdings purge`, which holds the registry lock across the
/// offer it makes — so it cannot go through [`run`] and wait out its own hold.
pub fn write_the_export(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    destination: &mut Destination,
    installed: &crate::probe::Installed,
    out: &mut dyn Write,
) -> Result<()> {
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

    // Liveness deliberately not among these: nothing here writes a Profile.
    // The hold is — sealing a copy another `perch` has added to since the
    // prompts writes a file claiming to be everything Perch holds while partial.
    let mut standing = wait::Standing::of()
        .and(|_: &mut crate::lock::Held<'_>| destination.still_free(host))
        .and(|perch| still_ours(perch, "exported"));
    standing.establish(perch)?;

    let ((export, sealed), (), fresh) = wait::across(
        perch,
        |_| {
            let passphrase = agreed_passphrase(host, out)?;
            let export = export::gather(host, registry, installed)?;
            let sealed = export::seal(&export, &passphrase)?;
            Ok((export, sealed))
        },
        |perch| standing.establish(perch),
    )?;
    drop(standing);
    destination.write(host, &sealed, &fresh)?;

    report(out, destination.path(), &export)
}

/// The path an Export lands at, proven fit before anything is spent on it.
///
/// The refusals of the path's shape run once, in the constructor. Which of them
/// survives a wait is the type's to know: only [`Destination::still_free`] can
/// go stale, and [`Destination::write`] is the only way to the bytes.
pub struct Destination {
    path: PathBuf,
    landed: bool,
}

impl Destination {
    /// A path an Export may land at: a directory that exists, nothing already
    /// at it, and outside the home a Purge deletes whole.
    pub fn for_an_export(host: &dyn Host, path: &Path) -> Result<Destination> {
        refuse_a_directory_that_is_not_there(host, path)?;
        refuse_an_occupied_path(host, path)?;
        refuse_a_path_perchs_home_would_take(host, path)?;
        Ok(Destination {
            path: path.to_path_buf(),
            landed: false,
        })
    }

    /// The one refusal a wait lets go stale: the path again, because the write
    /// replaces whatever is at it rather than failing on it.
    pub fn still_free(&self, host: &dyn Host) -> Result<()> {
        refuse_an_occupied_path(host, &self.path)
    }

    /// The one write, and the step nothing takes back: the seal is as many
    /// questions old as the passphrase took, so it lands only on [`wait::Fresh`].
    pub fn write(&mut self, host: &dyn Host, sealed: &str, _fresh: &wait::Fresh) -> Result<()> {
        host.write_private_file(&self.path, sealed)
            .map_err(|err| PerchError::file_write(self.path.clone(), err))?;
        self.landed = true;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the bytes are, once they are: recorded the instant they land,
    /// because a report failing afterwards is not an Export never written.
    pub fn landed(&self) -> Option<&Path> {
        self.landed.then_some(self.path.as_path())
    }
}

/// Refuses to write the Export inside Perch's own home.
///
/// Here rather than at either command, because both doors owe it: a Purge that
/// wrote its offer there deletes it moments later, and `perch holdings export`
/// writes the same file at the same path with no Purge in sight yet.
fn refuse_a_path_perchs_home_would_take(host: &dyn Host, path: &Path) -> Result<()> {
    let home = crate::holdings::perch_home(host)?;
    if !crate::host::is_inside(host, path, &home) {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "{} is inside {}, which `perch holdings purge` deletes whole, so an \
         Export written there goes with the Holdings it is the only copy of.\n\
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
    say::line(
        out,
        "This file holds a working Credential for every Account Perch has. It is \
         encrypted with a passphrase you choose, and there is no way into it \
         without one.",
    )?;

    let Some(typed) = ask::a_passphrase(host, out, "Passphrase: ")? else {
        return Err(PerchError::Invalid(
            "No passphrase was typed, and an Export cannot be written without \
             one. Nothing was written."
                .to_string(),
        ));
    };

    if ask::a_secret(host, out, "Again: ")?.unwrap_or_default() != typed {
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
    say::line(
        out,
        &format!(
            "Exported {} to {}.",
            say::accounts(accounts),
            path.display(),
        ),
    )?;

    let bare = export.without_a_credential();
    if !bare.is_empty() {
        say::line(
            out,
            &format!(
                "Neither Credential Store held anything for {}, so the Export \
                 carries the {} without a Credential. `perch relogin` is worth \
                 doing before this file is the only copy.",
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
