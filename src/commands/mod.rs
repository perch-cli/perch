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
    Err(PerchError::Other(format!(
        "An Export is encrypted with a passphrase, and there is no terminal to \
         prompt for one on.\n\
         There is no flag that answers ahead of time: a passphrase passed as an \
         argument sits in the process table for anything on this machine to \
         read. Run `{command}` where you can type."
    )))
}

/// What the Accounts in no Group are shown under. Being in no Group is not a
/// Group (ADR 0017), so it is never a heading that reads like one.
pub const IN_NO_GROUP: &str = "In no Group";

/// What Cycling will not do with those Accounts until it is told it may (ADR
/// 0017), as a clause both surfaces that show them finish a sentence with. One
/// sentence, because two would sooner or later say two different things.
pub const CYCLING_AMONG_UNGROUPED: &str = "only moves between these when you say it may";
