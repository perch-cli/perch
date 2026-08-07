//! The real terminal: raw mode, the alternate screen, and giving both back.
//!
//! Entering is three calls and leaving is three more, and the whole difficulty
//! is that leaving has to happen on every way out. A normal exit, an error out
//! of the frame loop, and a panic are three different ways, and a TUI that
//! misses one of them leaves somebody typing blind into a shell that no longer
//! echoes — a `reset` they have to know about, in a terminal that will not show
//! them typing it.
//!
//! So it is given back in two places, which between them cover all three: a
//! [`Drop`] that runs however the loop ends, and a panic hook that runs before
//! the panic is printed so the report lands on the ordinary screen rather than
//! on the alternate one that is about to disappear.

use std::io::{Stdout, stdout};
use std::sync::Once;
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{ExecutableCommand, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::error::{PerchError, Result};
use crate::tui::model::Model;
use crate::tui::{Screen, Signal, view};

/// The terminal, entered. Dropping it puts the terminal back.
pub struct TerminalScreen {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalScreen {
    /// Takes the terminal over: raw mode, so a keystroke arrives as one rather
    /// than a line at a time and Ctrl-C arrives at all, and the alternate
    /// screen, so what was in the scrollback is still there afterwards.
    pub fn enter() -> Result<TerminalScreen> {
        restore_before_a_panic_is_printed();
        terminal::enable_raw_mode().map_err(the_terminal_refused)?;
        // Raw mode is on, so from here a failure has something to give back.
        if let Err(err) = stdout().execute(EnterAlternateScreen) {
            let _ = give_it_back();
            return Err(the_terminal_refused(err));
        }
        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(TerminalScreen { terminal }),
            Err(err) => {
                let _ = give_it_back();
                Err(the_terminal_refused(err))
            }
        }
    }
}

impl Screen for TerminalScreen {
    fn draw(&mut self, model: &Model) -> Result<()> {
        self.terminal
            .draw(|frame| view::render(frame, model))
            .map(|_| ())
            .map_err(the_terminal_refused)
    }

    fn next(&mut self, millis: u64) -> Result<Option<Signal>> {
        if !event::poll(Duration::from_millis(millis)).map_err(the_terminal_refused)? {
            return Ok(None);
        }
        let event: Event = event::read().map_err(the_terminal_refused)?;
        Ok(Signal::of(&event))
    }
}

impl Drop for TerminalScreen {
    /// However the loop ended — a keystroke, an error, an unwinding panic —
    /// this is the last thing that happens to the terminal.
    fn drop(&mut self) {
        let _ = give_it_back();
    }
}

/// Raw mode off, then the ordinary screen and the cursor back — and every one
/// of them attempted even if an earlier one failed, because each is a way of
/// leaving the terminal unusable on its own.
///
/// The cursor is here because ratatui hides it for every frame that does not
/// place one, and nothing but this shows it again: `LeaveAlternateScreen` does
/// not, on most terminals. An invisible cursor is the same `reset`-your-way-out
/// of it that raw mode is.
fn give_it_back() -> std::io::Result<()> {
    let raw = terminal::disable_raw_mode();
    let left = stdout().execute(LeaveAlternateScreen).map(|_| ());
    let shown = stdout().execute(Show).map(|_| ());
    raw.and(left).and(shown)
}

/// Puts the terminal back before the panic hook already installed says
/// anything.
///
/// [`Drop`] alone is not enough. A panic prints before it unwinds, so a report
/// written first would be written onto the alternate screen and then thrown
/// away with it — the bug would be invisible and the user would have nothing to
/// paste. Perch's own hook ([`crate::report`]) is what is being run underneath
/// here, so a panic in the TUI still says what to report and where.
///
/// Once, because entering twice in one process would otherwise stack a second
/// restore on top of the first — harmless, but it is the shape of thing that
/// stops being harmless.
fn restore_before_a_panic_is_printed() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let already_there = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panicked| {
            first(give_it_back, &*already_there, panicked)
        }));
    });
}

/// One thing done before another, which is the whole of what the TUI's panic
/// hook adds — written as a function so the order can be asserted on without a
/// panic to raise or a terminal to raise it in.
fn first<A: ?Sized, R>(before: impl FnOnce() -> R, then: &dyn Fn(&A), argument: &A) {
    let _ = before();
    then(argument);
}

fn the_terminal_refused(err: impl std::fmt::Display) -> PerchError {
    PerchError::Other(format!("The terminal could not be drawn in: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The restore has to happen *before* the report is printed, or the report
    /// is printed onto a screen that is about to be thrown away.
    #[test]
    fn the_terminal_is_given_back_before_the_panic_is_reported() {
        static ORDER: Mutex<Vec<&str>> = Mutex::new(Vec::new());

        first(
            || {
                ORDER
                    .lock()
                    .expect("nothing panicked here")
                    .push("restored")
            },
            &|_: &u8| ORDER.lock().expect("nor here").push("reported"),
            &0,
        );

        assert_eq!(*ORDER.lock().expect("still fine"), ["restored", "reported"]);
    }
}
