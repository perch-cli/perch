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
pub mod purge;
pub mod relogin;
pub mod remove;
pub mod run;
pub mod service;
pub mod status;
pub mod switch;
pub mod upgrade;
pub mod version;
pub mod watch;
pub mod watcher;

use std::io::Write;

use crate::error::{PerchError, Result};
use crate::host::Host;

/// A command's output going nowhere — a closed pipe, most often. Not a failure
/// of the thing the command was asked to do, but the command cannot finish
/// saying what it did.
pub fn write_failed(err: std::io::Error) -> PerchError {
    PerchError::Other(format!("Perch could not write its output: {err}"))
}

/// One line to the person running the command, with whatever a terminal would
/// act on taken out (ADR nothing-drawn-is-obeyed). At the writer, because a
/// sentence is a `format!` and there is no column to hang the question on: a
/// plan, a Quota Window's name and a path read out of a unit file reach a
/// person this way, and nobody chose any of them.
pub fn say(out: &mut dyn Write, line: &str) -> Result<()> {
    writeln!(out, "{}", crate::host::Shown::in_prose(line)).map_err(write_failed)
}

/// One document to whatever is parsing the command, which is what `--json`
/// means everywhere it is offered. Here rather than at the call sites because a
/// rule about machine-readable output — pretty or compact, stdout or elsewhere —
/// is a rule with one place to hold it.
pub fn say_json(out: &mut dyn Write, document: &serde_json::Value) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(document).map_err(|err| PerchError::Other(err.to_string()))?;
    // Written rather than said: serde has already escaped a control character as
    // six characters a parser wants, and a second pass would take those six out
    // of a document a script is reading.
    writeln!(out, "{rendered}").map_err(write_failed)
}

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
        say(out, &line).map_err(|error| {
            error.with_note("The change was saved. Only the report could not be printed.")
        })?;
    }
    Ok(())
}

/// The registry, opened for what the command is about to do with it.
///
/// Exclusively only where something will be written, which is `--refresh` and
/// nothing else: two listings drawn at once are ordinary, and a read that took
/// the write lock would fail on one of them.
pub fn opened_for(
    host: &dyn Host,
    refresh: bool,
) -> Result<(Option<crate::lock::Held<'_>>, crate::registry::Registry)> {
    match refresh {
        true => {
            let (perch, registry) = crate::adopt::ensure_adopted_exclusively(host)?;
            Ok((Some(perch), registry))
        }
        false => Ok((None, crate::adopt::ensure_adopted(host)?)),
    }
}

/// Current figures for exactly the Accounts the command is about to show, so
/// narrowing what is shown narrows the reads with it. The empty report where
/// nobody asked, which renders as "nobody asked". Nothing else is held across
/// the read: both callers take the registry lock and nothing more.
pub fn refreshed(
    host: &dyn Host,
    perch: &mut Option<crate::lock::Held<'_>>,
    registry: &mut crate::registry::Registry,
    about: &[String],
) -> crate::observe::Report {
    match perch {
        Some(perch) => crate::observe::refresh(
            host,
            perch,
            registry,
            about,
            &crate::probe::Installed::probed(host),
            // Somebody typed this, so a Watcher running behind it is already
            // keeping the active Account's figure and this read is left to it.
            crate::observe::Spending::BesideTheWatcher,
            // Nothing to lose part way: these two hold the registry lock and
            // nothing else, and neither is a Watcher a signal can displace.
            &mut || Ok(()),
        ),
        None => crate::observe::Report::default(),
    }
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

/// An Account as the two lines announcing a new one name it: the address, and
/// whatever of the organization and the plan is known, parenthesized.
///
/// One place, because an Adoption and an Add are the same news about the same
/// thing and a detail added to one spelling is missing from the other.
pub fn described(email: &str, organization: Option<&str>, plan: Option<&str>) -> String {
    let details: Vec<&str> = [organization, plan].into_iter().flatten().collect();
    match details.is_empty() {
        true => email.to_string(),
        false => format!("{email} ({})", details.join(", ")),
    }
}

/// A count of Accounts, with the noun agreeing with it. One place, because
/// "1 Accounts" is the kind of thing that ships and stays shipped, and seven call
/// sites spelling their own is seven chances at it.
pub fn accounts(count: usize) -> String {
    counted(count, "Account")
}

/// The same for the two nouns a Purge counts where the registry named no Account.
pub fn profiles(count: usize) -> String {
    counted(count, "Profile")
}

pub fn credentials(count: usize) -> String {
    counted(count, "Credential")
}

pub fn groups(count: usize) -> String {
    counted(count, "Group")
}

/// Not a noun of Perch's: what `perch config` says about a command line it was
/// given too many of.
pub fn words(count: usize) -> String {
    counted(count, "word")
}

fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

/// Why a Credential Store that Perch went to empty turned out to hold nothing,
/// said about the store this machine actually has
/// (ADR claude-code-chooses-the-store). One function rather than a copy per
/// caller: the day a third store is added is the day the copies disagree about
/// where a Credential might still be.
pub fn a_store_that_held_nothing(host: &dyn Host) -> &'static str {
    match host.platform() {
        // The item's account name is derived from `$USER`, so a Profile written
        // under one login name keeps its Credential where a Perch under another
        // will not look — the one way an empty store is not an empty Account.
        crate::host::Platform::MacOs => {
            "on macOS a keychain item is filed under `$USER`, so one written \
             under a different login name is still there"
        }
        // The store is a file inside the Profile, so the Profile going is the
        // Credential going, and there is nowhere else for one to be.
        _ => {
            "its Credential Store is a file inside its Profile, and there was no \
             file there"
        }
    }
}

/// Why there is no active Account, in the terms the way out depends on: holding
/// nothing, a login is the way in; holding Accounts, Perch has merely been left
/// on nobody and naming one is what `perch switch` is for. `because` is what the
/// command wanted an active Account for. One function, because two commands meet
/// this state and only one of them told the difference.
pub fn no_active_account(registry: &crate::registry::Registry, because: &str) -> PerchError {
    if registry.accounts.is_empty() {
        return PerchError::NotFound(format!(
            "Perch holds no Accounts{because}. Run `claude` and log in, then run \
             Perch again."
        ));
    }
    PerchError::NotFound(format!(
        "Perch holds no active Account{because}. `perch switch <target>` makes \
         {} active.",
        match registry.accounts.len() {
            1 => "the one it holds".to_string(),
            held => format!("one of the {held} it holds"),
        }
    ))
}

/// Refuses to act on an Account whose Credential no longer works, in the words of
/// whichever command was asked; `consequence` is what did not happen and why it
/// would have been worse than nothing. One function rather than one per command:
/// `perch run` and `perch switch` meet this state over the same Account and must
/// not describe it in two ways.
pub fn refuse_a_quarantined_account(
    registry: &crate::registry::Registry,
    email: &str,
    consequence: &str,
) -> Result<()> {
    let account = registry.held(email)?;
    match account.quarantine {
        None => Ok(()),
        Some(why) => Err(why.refusal(&registry.named_for_the_user(email), email, consequence)),
    }
}

/// Refuses to write into a Profile a client is holding, over the one or two this
/// command writes; `also_the_default_profile` is why that one joins them. One
/// function rather than one per command: each asks twice, and two spellings of
/// one pair of checks is how the second ask comes to be weaker than the first
/// (ADR a-profile-is-live-by-evidence).
pub fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &crate::registry::Account,
    also_the_default_profile: Option<&'static str>,
    installed: &crate::probe::Installed,
) -> Result<()> {
    let mut places = vec![crate::live::Place::of_the_profile(host, account)?];
    if let Some(why) = also_the_default_profile {
        // Its Credential is the one a running client is holding, and this would
        // replace it rather than renew it.
        places.push(crate::live::Place::new(
            why,
            crate::holdings::the_default_profile(host)?.config_dir,
        ));
    }

    crate::live::ask(host, &places).idle_or(installed, &crate::live::NOTHING_WAS_CHANGED)?;
    Ok(())
}

/// What the Accounts in no Group are shown under. Being in no Group is not a
/// Group (ADR a-group-is-a-declaration), so this never reads like a Group's
/// name.
pub const IN_NO_GROUP: &str = "In no Group";

/// What Cycling will not do with the Accounts in no Group until it is told it
/// may, and which way the Setting gating the Scope is set — both halves, because
/// the rule alone reads as "you have yet to say it" to somebody who has. Keyed
/// from [`crate::config::Setting`] and in the values a `set` takes, so neither
/// can drift from it. The clause carries no label; a caller supplies one.
pub fn cycling_among_ungrouped(registry: &crate::registry::Registry) -> String {
    format!(
        "only moves between these when you say it may — `{}` is {}",
        crate::config::Setting::Interchangeable.as_str(),
        registry.ungrouped.interchangeable
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    /// The way out turns on what is held, and both commands that meet this state
    /// read the same sentence: a login is the answer only where there is nothing
    /// to switch to.
    #[test]
    fn what_to_do_about_no_active_account_depends_on_what_perch_holds() {
        let empty = Registry::default();
        let said = no_active_account(&empty, "").to_string();
        assert!(said.contains("no Accounts"), "{said}");
        assert!(said.contains("`claude`"), "{said}");

        let mut held = Registry::default();
        held.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let said = no_active_account(&held, ", so there is no Group to Cycle within").to_string();
        assert!(said.contains("no Group to Cycle within"), "{said}");
        assert!(said.contains("the one it holds"), "{said}");
        assert!(
            !said.contains("`claude`"),
            "a login repairs nothing here: {said}"
        );
    }
}
