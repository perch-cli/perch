//! `perch tui` — the interactive view.
//!
//! The command is the small part: refuse where there is no terminal, read what
//! Perch holds, take the terminal, and hand over to the frame loop
//! ([`crate::tui`]). The registry is read *before* the terminal is entered,
//! which is the whole of ADR 0015's rule about the first frame — it is drawn
//! from what is already on disk, so it appears at once and says how old every
//! figure on it is.

use std::io::Write;

use crate::adopt;
use crate::commands::run::RunArgs;
use crate::commands::{run as run_command, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::tui::Left;
use crate::tui::refresh::InAThread;
use crate::tui::terminal::TerminalScreen;

/// Opens the view, and hands over to whatever the person chose in it.
///
/// The status comes back rather than being folded into success or failure,
/// because one of the two things the picker acts on is a Run — and a Run is a
/// way of launching a program (ADR 0010). What the client said is what a script
/// reads, whether it was launched from a command line or from a cursor.
pub fn run(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    if !host.is_interactive() {
        return Err(PerchError::Other(
            "`perch tui` draws in a terminal, and this is not one. Everything it \
             shows has a plain command form: `perch list` for the Accounts, \
             `perch status` for where you are (ADR 0011)."
                .to_string(),
        ));
    }

    let registry = adopt::ensure_adopted(host)?;
    // Anything already written belongs in the scrollback, where it can be read
    // afterwards — not under an alternate screen about to cover it and then be
    // thrown away. Adoption itself says nothing here: it takes no writer, and
    // its remarks go out through `Host::note`. This is about whatever the
    // caller had written before handing the stream over.
    out.flush().map_err(write_failed)?;

    let mut refresher = InAThread::new();
    let browsed = {
        // The terminal goes back when this is dropped, which happens whether
        // the loop returns or fails — and before anything below writes a line,
        // because a line written to the alternate screen is a line thrown away
        // with it.
        let mut screen = TerminalScreen::enter()?;
        crate::tui::browse(host, registry, &mut screen, &mut refresher)
    };

    // Whichever failed, the loop's failure is the one worth reporting: it is
    // what the user was doing, and the other is a line that could not be
    // written about the way out of it.
    let finished = crate::tui::finish(&mut refresher, out);
    hand_over(host, browsed.and_then(|left| finished.map(|()| left))?, out)
}

/// What the view was left for.
///
/// Reached with the terminal already given back and any outstanding Refresh
/// already waited for, which is the only state a Run may start from: a Run
/// lasts as long as somebody's session, and nothing may be held for that long.
///
/// Its own function because that ordering is the whole of what it depends on,
/// and because a Run launched from a cursor has to be the same Run as one
/// launched from a command line — arguments and all, which here is none of
/// them: Claude Code, against the chosen Profile, with nothing passed on.
pub fn hand_over(host: &dyn Host, left: Left, out: &mut dyn Write) -> Result<i32> {
    match left {
        Left::Alone => Ok(crate::error::EXIT_OK),
        Left::ToRun(email) => run_command::run(
            host,
            RunArgs {
                target: email,
                command: Vec::new(),
            },
            out,
        ),
    }
}
