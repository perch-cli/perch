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
use crate::commands::write_failed;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::tui::refresh::InAThread;
use crate::tui::terminal::TerminalScreen;

pub fn run(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    if !host.is_interactive() {
        return Err(PerchError::Other(
            "`perch tui` draws in a terminal, and this is not one. Everything it \
             shows has a plain command form: `perch list` for the Accounts, \
             `perch status` for where you are (ADR 0011)."
                .to_string(),
        ));
    }

    // Whatever adoption has to say belongs in the scrollback, where it can be
    // read afterwards — not under an alternate screen that is about to cover
    // it and then be thrown away.
    let registry = adopt::ensure_adopted(host, out)?;
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
    browsed.and(finished)
}
