//! The commands themselves. Each one takes a `&dyn Host` and a writer and
//! returns a `Result`, so behaviour tests drive the real code with no process,
//! no filesystem, and no keychain.

pub mod add;
pub mod alias;
pub mod config;
pub mod enable;
pub mod export;
pub mod group;
pub mod import;
pub mod list;
pub mod purge;
pub mod relogin;
pub mod remove;
pub mod run;
pub mod status;
pub mod switch;
pub mod tui;
pub mod watch;

use std::io::Write;

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
/// means everywhere it is offered.
///
/// Here rather than at the two call sites for the reason `say` is here: the two
/// commands that emit one were spelling the same three-line incantation, and a
/// rule about machine-readable output — pretty or compact, stdout or elsewhere —
/// is a rule with one place to hold it.
pub fn say_json(out: &mut dyn Write, document: &serde_json::Value) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(document).map_err(|err| PerchError::Other(err.to_string()))?;
    say(out, &rendered)
}

/// Puts a question to the person at the terminal and waits for their answer,
/// or `None` at end of input.
///
/// The question is written without a newline and flushed, so the answer is
/// typed where the question ends. Every command that asks one asks it this way:
/// a question the terminal has not been shown yet is a command that looks hung.
pub fn ask(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{question}").map_err(write_failed)?;
    out.flush().map_err(write_failed)?;
    host.read_line()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))
}

/// The same, with the answer as it will be compared: trimmed of what the Return
/// key leaves behind, and folded to lower case.
///
/// Every question Perch puts is answered by matching a word — `y`, `n`, `purge`
/// — and a caller doing its own trimming and folding is how one of them comes to
/// accept `Y ` and another not to. What each word *means* stays with the caller,
/// including what to make of `None`: end of input is agreement to nothing and
/// refusal of nothing, and only the question knows which of those it wanted.
pub fn ask_a_word(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    Ok(ask(host, out, question)?.map(|typed| typed.trim().to_lowercase()))
}

/// The same, for a question whose answer the terminal must never show — the
/// passphrase an Export is encrypted with (ADR 0014).
///
/// The newline afterwards is Perch's to write, because the one the terminal
/// would have written is the one that was suppressed: with echo off, the Return
/// that ended the answer never reached the screen, so whatever is said next
/// would otherwise be written where the question ended.
pub fn ask_secret(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{question}").map_err(write_failed)?;
    out.flush().map_err(write_failed)?;
    let answered = host
        .read_secret()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))?;
    writeln!(out).map_err(write_failed)?;
    Ok(answered)
}

/// Refuses to go on if the registry lock went stale while a question was
/// waiting for an answer.
///
/// A question put to a person is the one wait in Perch with no bound on it —
/// somebody may answer in a second or walk away and answer after lunch — so it
/// is the one place a hold can go stale under a command that is otherwise
/// behaving. Another `perch` may then have taken the lock and changed the
/// registry, which leaves this one holding a copy that is out of date and about
/// to write it back over theirs.
///
/// Asked before the first irreversible thing rather than at the save, because
/// finding out at the save is finding out after the Credentials are gone.
///
/// One copy of the guard and one copy of its sentence, because the commands
/// that need it are the destructive ones and two copies is how the third of
/// them ships without either. `did` is what this command will not have done —
/// "removed", "purged", "exported".
///
/// A general failure rather than `Busy`, which reads as the obvious tidy-up and
/// is not (ADR 0036). Nothing *has* been written here — this is asked before
/// the first irreversible thing — so the promise `Busy` makes would hold; what
/// does not hold is folding this together with the two other sentences about a
/// lost hold, one of which is reached after a Credential has moved.
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

/// The passphrase somebody typed, or `None` when they typed none.
///
/// Empty and end of input are one answer here, because they are one event: a
/// pipe that closed reads as nobody having typed anything, and typing nothing is
/// the same skip an optional passphrase would have been. What it *cost* is the
/// caller's to say — an Export that was not written and one that was not opened
/// are different pieces of news — so this only reports that there was no answer.
pub fn ask_passphrase(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
) -> Result<Option<String>> {
    let typed = ask_secret(host, out, question)?.unwrap_or_default();
    Ok(Some(typed).filter(|typed| !typed.trim().is_empty()))
}

/// Refuses the two commands that need a passphrase when there is no terminal to
/// type one at (ADR 0014).
///
/// There is deliberately no flag that answers ahead of time, which is what makes
/// this a refusal rather than a fallback: a passphrase passed as an argument
/// sits in the process table for anything on the machine to read, and one in a
/// shell history outlives the command that used it. So the message names the
/// terminal rather than a way round it — and says so once, because an escape
/// hatch appearing in only one of the two would be the whole of what the
/// required passphrase was for.
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

/// A count of Accounts, with the noun agreeing with it.
///
/// One place, because "1 Accounts" is the kind of thing that ships and stays
/// shipped, and there were three spellings of this fragment across seven call
/// sites — `"Account"/"Accounts"` beside a count somebody else formatted, a
/// private copy in `group`, and another in `reserve`. Which is how one of them
/// gets a plural nobody noticed was missing.
pub fn accounts(count: usize) -> String {
    match count {
        1 => "1 Account".to_string(),
        _ => format!("{count} Accounts"),
    }
}

/// Why a Credential Store that Perch went to empty turned out to hold nothing,
/// said about the store this machine actually has.
///
/// Both callers said the macOS half unconditionally: "on macOS a keychain item
/// is filed under `$USER`, so one written under a different login name is still
/// there." Off macOS there is no keychain and no filing under `$USER` — a
/// Credential lives in a file inside the Profile (ADR 0020) — so the one
/// sentence explaining where a Credential might still be pointed at a place
/// that does not exist, on the two commands whose whole promise is that
/// something is gone.
///
/// One function rather than a copy in each, because the two are answering the
/// same question and the day a third store is added is the day the copies
/// disagree.
pub fn a_store_that_held_nothing(host: &dyn Host) -> &'static str {
    match host.platform() {
        // Perch derives the keychain item's account name from `$USER` (ADR
        // 0008), so a Profile written under one login name keeps its Credential
        // where a Perch running under another will not look — which is the one
        // way an empty store here is not the same as an empty Account.
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

/// Refuses to act on an Account whose Credential no longer works, in the words
/// of whichever command was asked.
///
/// One function rather than the two `run` and `switch` had, which differed only
/// in whether they were handed an email address or an `Account` and in the
/// sentence about what did not happen — and [`Quarantine::refusal`] already
/// takes that sentence as a parameter. Two copies is how the shape of the
/// refusal comes to differ between two commands that meet the same state, and
/// the picker calls both: it names an Account by cursor, so what it says has to
/// be the refusal the command would have given, character for character.
///
/// `consequence` is what did not happen and why it would have been worse than
/// nothing.
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
/// Group (ADR 0017), so it is never a heading that reads like one.
pub const IN_NO_GROUP: &str = "In no Group";

/// What Cycling will not do with those Accounts until it is told it may (ADR
/// 0017), as a clause both surfaces that show them finish a sentence with. One
/// sentence, because two would sooner or later say two different things.
pub const CYCLING_AMONG_UNGROUPED: &str = "only moves between these when you say it may";

/// The same rule, and then what it currently answers.
///
/// The constant on its own prints the same words whether the declaration has
/// been made or not, so the one Setting gating the whole Scope is not readable
/// from it. `perch group list` was fixed for that and the other two surfaces
/// were not: `perch list` printed the bare rule, so somebody who had already
/// run `perch config set cycle-ungrouped true` was still told Cycling moves
/// between these "when you say it may" — implying they had yet to say it.
///
/// A function rather than a third spelling. There were three: the bare
/// constant, "`cycle-ungrouped` is on/off", and "`cycle-ungrouped` is
/// true/false" — and `on`/`off` is not a value the Setting takes, so a reader
/// who typed back what they were shown was refused. What it prints is what
/// `perch config` accepts and prints.
/// The clause alone, without the word the three callers label it with: one of
/// them writes it into a two-column line where "Cycling" is the label.
pub fn cycling_among_ungrouped(registry: &crate::registry::Registry) -> String {
    format!(
        "{CYCLING_AMONG_UNGROUPED} — `cycle-ungrouped` is {}",
        registry.global.cycle_ungrouped
    )
}
