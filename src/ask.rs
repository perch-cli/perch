//! A question put to the person at the terminal, and what they answered.
//!
//! One module for the rules a question is asked under, because a second spelling
//! of any of them is a command that accepts what its neighbor refuses: no
//! newline before the answer, a flush so the terminal has the question, one
//! folding of what was typed, and end of input as agreement to nothing.
//!
//! Perch asks little and asks it the same way everywhere
//! (ADR perch-does-not-draw). What is *said* rather than asked is
//! [`crate::say`], and a remark from below is `Host::note`.

use std::io::Write;

use zeroize::Zeroizing;

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::say;

/// Puts a question to the person at the terminal and waits for their answer, or
/// `None` at end of input. Written without a newline and flushed, so the answer
/// is typed where the question ends: a question the terminal has not been shown
/// yet is a command that looks hung.
pub fn line(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{}", crate::host::Shown::of(question)).map_err(say::failed)?;
    out.flush().map_err(say::failed)?;
    host.read_line()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))
}

/// The same, with the answer as it will be compared: trimmed and folded to lower
/// case, so one caller does not come to accept `Y ` where another does not. What
/// each word *means* stays with the caller, `None` included — end of input is
/// agreement to nothing and refusal of nothing, and only the question knows
/// which of those it wanted.
pub fn a_word(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    Ok(line(host, out, question)?.map(|typed| typed.trim().to_lowercase()))
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
    Ok(match a_word(host, out, question)?.as_deref() {
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
pub fn a_secret(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
) -> Result<Option<Zeroizing<String>>> {
    write!(out, "{}", crate::host::Shown::of(question)).map_err(say::failed)?;
    out.flush().map_err(say::failed)?;
    let answered = host
        .read_secret()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))?;
    writeln!(out).map_err(say::failed)?;
    // Already wiped-on-drop when it arrives, because the port says so. Wrapping
    // it again here would claim the wiped buffer is the one the terminal was
    // read into, which is the adapter's to keep rather than this line's.
    Ok(answered)
}

/// The passphrase somebody typed, or `None` when they typed none. Empty and end
/// of input are one answer, because they are one event. What it *cost* is the
/// caller's to say: an Export that was not written and one that was not opened
/// are different pieces of news.
pub fn a_passphrase(
    host: &dyn Host,
    out: &mut dyn Write,
    question: &str,
) -> Result<Option<Zeroizing<String>>> {
    let typed = a_secret(host, out, question)?.unwrap_or_default();
    Ok(Some(typed).filter(|typed| !typed.trim().is_empty()))
}

/// Refuses the two commands that need a passphrase where there is no terminal to
/// type one at. There is deliberately no flag that answers ahead of time, so this
/// is a refusal rather than a fallback and it names the terminal rather than a
/// way round it — said once, because an escape hatch in only one of the two would
/// be the whole of what the required passphrase was for.
pub fn needs_a_terminal(host: &dyn Host, command: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A prompt draws a value nobody chose: `perch remove` puts the address in
    /// the question, and an address is Claude Code's rather than anybody's
    /// choice. `registry::validate` lets a terminal-obeyed character through on
    /// the stated grounds that it is drawn through `Shown`.
    #[test]
    fn a_question_draws_what_it_names_as_stripped_as_a_listing_does() {
        let host = crate::host::FakeHost::new().with_answers(&["n"]);
        let mut out = Vec::new();

        let answered = line(
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
}
