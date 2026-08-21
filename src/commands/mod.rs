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
    PerchError::Other(err.to_string())
}

/// One line to the person running the command.
pub fn say(out: &mut dyn Write, line: &str) -> Result<()> {
    writeln!(out, "{line}").map_err(write_failed)
}

/// One document to whatever is parsing the command, which is what `--json`
/// means everywhere it is offered. Here rather than at the call sites because a
/// rule about machine-readable output — pretty or compact, stdout or elsewhere —
/// is a rule with one place to hold it.
pub fn say_json(out: &mut dyn Write, document: &serde_json::Value) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(document).map_err(|err| PerchError::Other(err.to_string()))?;
    say(out, &rendered)
}

/// Puts a question to the person at the terminal and waits for their answer, or
/// `None` at end of input. Written without a newline and flushed, so the answer
/// is typed where the question ends: a question the terminal has not been shown
/// yet is a command that looks hung.
pub fn ask(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{question}").map_err(write_failed)?;
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
    write!(out, "{question}").map_err(write_failed)?;
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
    Err(PerchError::Other(format!(
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
    crate::registry::save(host, &mut perch, &registry)?;
    for line in said {
        say(out, &line)?;
    }
    Ok(())
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

/// A count of Accounts, with the noun agreeing with it. One place, because
/// "1 Accounts" is the kind of thing that ships and stays shipped, and seven call
/// sites spelling their own is seven chances at it.
pub fn accounts(count: usize) -> String {
    match count {
        1 => "1 Account".to_string(),
        _ => format!("{count} Accounts"),
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
