//! The commands themselves. Each one takes a `&dyn Host` and a writer and
//! returns a `Result`, so behavior tests drive the real code with no process,
//! no filesystem, and no keychain.

pub mod add;
pub mod alias;
pub mod config;
pub mod enable;
pub mod export;
pub mod group;
pub mod holdings;
pub mod import;
pub mod list;
pub mod probe;
pub mod purge;
pub mod relogin;
pub mod remove;
pub mod run;
pub mod service;
pub mod status;
pub mod switch;
pub mod triage;
pub mod upgrade;
pub mod version;
pub mod watch;
pub mod watcher;

use std::io::Write;

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::say;

/// Refuses to go on if the registry lock went stale while a question was waiting
/// for an answer — shape 3's guard, and the counterpart to [`only_the_registry`]
/// (ADR one-door-to-the-registry). Asked before the first irreversible thing
/// rather than at the save, because finding out at the save is finding out after
/// the Credentials are gone. `did` is what will not have been done.
pub fn still_ours(perch: &mut crate::lock::Held<'_>, did: &str) -> Result<()> {
    perch.renew();
    if perch.still_held() {
        return Ok(());
    }
    // `Busy` rather than `Other`: the sentence below is "run this again", which
    // is what `EXIT_HELD` promises and what `EXIT_GENERAL` denies.
    Err(PerchError::Busy(format!(
        "Another `perch` changed the registry while that question was waiting \
         for an answer, so this one is working from a copy that is out of \
         date.\n\
         Nothing was {did}. Run this again."
    )))
}

/// The whole of a command that changes the registry and reaches nothing else,
/// under Perch's own lock. Shape 1's door, and the counterpart to
/// [`still_ours`]. **`change` is handed no [`Host`], and that is the point**: a
/// command coming through here cannot reach a Credential Store, write a Profile
/// or touch the Default Profile.
pub fn only_the_registry(
    host: &dyn Host,
    out: &mut dyn Write,
    change: impl FnOnce(&mut crate::registry::Registry) -> Result<Vec<String>>,
) -> Result<()> {
    let (mut perch, mut registry) = crate::adopt::ensure_adopted_exclusively(host)?;
    let said = change(&mut registry)?;
    crate::registry::save(host, &mut perch, &mut registry)?;
    for line in said {
        // On disk by here, so an unnoted failure sends a script back to make a
        // change it has already made (ADR perch-says-what-it-did).
        say::line(out, &line).map_err(|error| {
            error.with_note("The change was saved. Only the report could not be printed.")
        })?;
    }
    Ok(())
}

/// The registry as a listing sees it, opened for what it was asked to do.
///
/// Whether the registry is held for writing is `--refresh`'s business, so the
/// hold lives in here and no caller sees it: a command opens a `Viewing`, asks
/// it for figures, and reads the registry through it.
pub struct Viewing<'a> {
    host: &'a dyn Host,
    perch: Option<crate::lock::Held<'a>>,
    registry: crate::registry::Registry,
}

impl<'a> Viewing<'a> {
    /// Exclusively only where something will be written, which is `--refresh`
    /// and nothing else: two listings drawn at once are ordinary, and a read
    /// that took the write lock would fail on one of them.
    pub fn opened(host: &'a dyn Host, refresh: bool) -> Result<Self> {
        let (perch, registry) = match refresh {
            true => {
                let (perch, registry) = crate::adopt::ensure_adopted_exclusively(host)?;
                (Some(perch), registry)
            }
            false => (None, crate::adopt::ensure_adopted(host)?),
        };
        Ok(Self {
            host,
            perch,
            registry,
        })
    }

    pub fn registry(&self) -> &crate::registry::Registry {
        &self.registry
    }

    /// Current figures for exactly the Accounts the command is about to show,
    /// so narrowing what is shown narrows the reads with it. The empty report
    /// where nobody asked, which renders as "nobody asked".
    pub fn figures_for(&mut self, about: &[String]) -> crate::observe::Report {
        match &mut self.perch {
            Some(perch) => read_now(self.host, perch, &mut self.registry, about),
            None => crate::observe::Report::default(),
        }
    }
}

/// The same read, for a command that holds the registry whatever it was asked.
///
/// A Cycle takes the exclusive hold before it decides anything, so it has no
/// hold in question and no reason to open a [`Viewing`]. Nothing else is held
/// across the read: every caller takes the registry lock and nothing more.
pub fn read_now(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut crate::registry::Registry,
    about: &[String],
) -> crate::observe::Report {
    let installed = crate::probe::Installed::for_the_figures(host);
    crate::observe::refresh(
        host,
        perch,
        registry,
        about,
        &installed,
        // Somebody typed this, so a Watcher running behind it is already
        // keeping the active Account's figure and this read is left to it.
        crate::observe::Spending::BesideTheWatcher,
    )
}

/// Brings the registry on this machine forward, once, ahead of the command.
///
/// Shape 1's sequence without shape 1's door, which adopts a login where there
/// is no registry and a migration has nothing to adopt. Here rather than inside
/// `load`, which cannot take a lock it is already being called under.
pub fn bring_the_registry_forward(host: &dyn Host) -> Result<()> {
    let path = crate::holdings::registry_path(host)?;
    let Some(was) = crate::migration::behind(host, &path) else {
        return Ok(());
    };

    let mut perch = crate::holdings::lock(host)?;
    // Asked again under the lock rather than trusted from outside it: between
    // the two reads, another Perch may have brought the same file forward.
    if crate::migration::behind(host, &path).is_none() {
        return Ok(());
    }
    let renamed = host
        .read_file(&path)
        .map(|held| crate::migration::renames(&held))
        .unwrap_or_default();
    let Some(mut registry) = crate::registry::load(host)? else {
        return Ok(());
    };
    // Through `save` rather than by writing what the step returned: it stamps
    // the version, refuses what a later `load` could not read, and replaces the
    // file in one step, so a migration that fails leaves the old shape intact.
    crate::registry::save(host, &mut perch, &mut registry)?;

    host.note(&crate::migration::brought_forward_note(was, &renamed));
    Ok(())
}

/// The Landing settled, for the four Switch paths somebody types.
///
/// Nobody takes a typed command off the person who typed it, so the ask it
/// passes cannot answer no — and `Resolved`'s stop arm carries what it answered
/// with, which for [`std::convert::Infallible`] is nothing there is a value of.
pub fn a_settled_landing(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut crate::registry::Registry,
) -> Result<crate::registry::Settled> {
    match crate::switch::resolve_a_landing::<std::convert::Infallible>(
        host,
        perch,
        registry,
        &mut || Ok(()),
    )? {
        crate::switch::Resolved::Settled(settled) => Ok(settled),
        crate::switch::Resolved::Stopped(never) => match never {},
    }
}
