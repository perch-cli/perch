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

use zeroize::Zeroizing;

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

/// Puts a question to the person at the terminal and waits for their answer, or
/// `None` at end of input. Written without a newline and flushed, so the answer
/// is typed where the question ends: a question the terminal has not been shown
/// yet is a command that looks hung.
pub fn ask(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{}", crate::host::Shown::of(question)).map_err(write_failed)?;
    out.flush().map_err(write_failed)?;
    host.read_line()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))
}

/// The same, with the answer as it will be compared: trimmed and folded to lower
/// case, so one caller does not come to accept `Y ` where another does not. What
/// each word *means* stays with the caller, `None` included — end of input is
/// agreement to nothing and refusal of nothing, and only the question knows
/// which of those it wanted.
pub fn ask_a_word(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    Ok(ask(host, out, question)?.map(|typed| typed.trim().to_lowercase()))
}

/// What a bare Return means, which is the half of a yes-or-no question the
/// prompt's own `[y/N]` or `[Y/n]` is already telling the reader.
#[derive(Clone, Copy)]
pub enum Presumed {
    Yes,
    No,
}

/// Whether somebody agreed, asked the same way wherever it is asked.
///
/// End of input is never agreement, whichever way the default points: a pipe
/// that closed is nobody, and nobody is not there to type the passphrase or to
/// want the Credential gone (ADR a-refusal-is-a-promise).
pub fn said_yes(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
    presumed: Presumed,
) -> Result<bool> {
    Ok(match ask_a_word(host, out, question)?.as_deref() {
        None => false,
        Some("y" | "yes") => true,
        Some("n" | "no") => false,
        Some(_) => matches!(presumed, Presumed::Yes),
    })
}

/// The same, for a question whose answer the terminal must never show — the
/// passphrase an Export is encrypted with (ADR the-holdings-go-out-sealed). The
/// newline afterwards is Perch's to write: with echo off, the Return that ended
/// the answer never reached the screen, so whatever is said next would otherwise
/// land where the question ended.
pub fn ask_secret(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
) -> Result<Option<Zeroizing<String>>> {
    write!(out, "{}", crate::host::Shown::of(question)).map_err(write_failed)?;
    out.flush().map_err(write_failed)?;
    let answered = host
        .read_secret()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))?;
    writeln!(out).map_err(write_failed)?;
    // Already wiped-on-drop when it arrives, because the port says so. Wrapping
    // it again here would claim the wiped buffer is the one the terminal was
    // read into, which is the adapter's to keep rather than this line's.
    Ok(answered)
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

/// The Landing settled, for the four Switch paths somebody types.
///
/// Nobody takes a typed command off the person who typed it, so the ask answers
/// `Ok`, the walk runs to an answer, and the witness is discarded: none of the
/// four has a step left that a stop would be in front of.
pub fn a_settled_landing(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut crate::registry::Registry,
) -> Result<()> {
    match crate::switch::resolve_a_landing(host, perch, registry, &mut || Ok(()))? {
        crate::switch::Resolved::Settled(_) => Ok(()),
        crate::switch::Resolved::Stopped(_) => {
            unreachable!("the ask a typed command passes answers `Ok`")
        }
    }
}

/// The passphrase somebody typed, or `None` when they typed none. Empty and end
/// of input are one answer, because they are one event. What it *cost* is the
/// caller's to say: an Export that was not written and one that was not opened
/// are different pieces of news.
pub fn ask_passphrase(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
) -> Result<Option<Zeroizing<String>>> {
    let typed = ask_secret(host, out, question)?.unwrap_or_default();
    Ok(Some(typed).filter(|typed| !typed.trim().is_empty()))
}

/// Refuses the two commands that need a passphrase where there is no terminal to
/// type one at. There is deliberately no flag that answers ahead of time, so this
/// is a refusal rather than a fallback and it names the terminal rather than a
/// way round it — said once, because an escape hatch in only one of the two would
/// be the whole of what the required passphrase was for.
pub fn refuse_without_a_terminal(host: &dyn Host, command: &str) -> Result<()> {
    if host.is_interactive() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "An Export is encrypted with a passphrase, and there is no terminal to \
         prompt for one on.\n\
         There is no flag that answers ahead of time: a passphrase passed as an \
         argument sits in the process table for anything on this machine to \
         read. Run `{command}` where you can type."
    )))
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
            crate::registry::the_default_profile(host)?.config_dir,
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

    /// A prompt draws a value nobody chose: `perch remove` puts the address in
    /// the question, and an address is Claude Code's rather than anybody's
    /// choice. `registry::validate` lets a terminal-obeyed character through on
    /// the stated grounds that it is drawn through `Shown`.
    #[test]
    fn a_question_draws_what_it_names_as_stripped_as_a_listing_does() {
        let host = crate::host::FakeHost::new().with_answers(&["n"]);
        let mut out = Vec::new();

        let answered = ask(
            &host,
            &mut out,
            "Remove safe@x.com\u{1b}[2K\rReally other@x.com? [y/N]: ",
        )
        .expect("the question is put");

        assert_eq!(answered.as_deref(), Some("n"));
        let drawn = String::from_utf8(out).expect("what a terminal is shown");
        assert!(
            !drawn.contains('\u{1b}') && !drawn.contains('\r'),
            "nothing the terminal acts on survives the question: {drawn:?}"
        );
        assert!(
            drawn.contains("safe@x.com") && drawn.contains("other@x.com"),
            "and every character it may draw is still there: {drawn:?}"
        );
    }

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
