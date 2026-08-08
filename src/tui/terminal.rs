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
use std::sync::{Mutex, Once, PoisonError};
use std::thread::ThreadId;
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
        held_by(Some(std::thread::current().id()));
        // Raw mode is on, so from here a failure has something to give back.
        if let Err(err) = stdout().execute(EnterAlternateScreen) {
            let _ = give_it_back();
            held_by(None);
            return Err(the_terminal_refused(err));
        }
        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(TerminalScreen { terminal }),
            Err(err) => {
                let _ = give_it_back();
                held_by(None);
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
        held_by(None);
    }
}

/// Which thread has the terminal, or `None` when nothing has.
///
/// The panic hook is process-global and cannot be taken off again — `set_hook`
/// replaces, and putting the previous one back would race any other hook
/// installed since. So the hook stays and this is what it asks, which turns
/// "always restore" into "restore when there is something to restore, and only
/// for the thread that took it".
///
/// Both halves are load-bearing. Without the thread: [`crate::tui::refresh`]
/// runs a Refresh on a thread of its own *while* the frame loop is drawing, and
/// a panic there is not fatal — the loop reads the closed channel, says the
/// Refresh did not finish, and carries on. Restoring the terminal on that
/// thread's panic would leave the loop drawing frames into a cooked,
/// non-alternate terminal, over the top of the panic report. Without the
/// `None`: once `browse` has returned, every later panic in the process would
/// still emit `\x1b[?1049l` and a cursor-show — onto stdout, which is where
/// `perch list --json` writes.
fn terminal_holder() -> &'static Mutex<Option<ThreadId>> {
    static HOLDER: Mutex<Option<ThreadId>> = Mutex::new(None);
    &HOLDER
}

fn held_by(thread: Option<ThreadId>) {
    *terminal_holder()
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = thread;
}

/// Whether the panicking thread is the one holding the terminal.
fn ours_to_give_back() -> bool {
    *terminal_holder()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        == Some(std::thread::current().id())
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
            first(
                || ours_to_give_back().then(give_it_back),
                &*already_there,
                panicked,
            )
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

    /// And it is only given back by the thread that took it, while it holds it.
    ///
    /// The hook is process-global and permanent — `set_hook` replaces, and
    /// putting the previous one back would race anything installed since — so
    /// what stops it firing wrongly is this question rather than its absence.
    ///
    /// A Refresh runs on a thread of its own while the frame loop draws, and a
    /// panic there is not fatal: the loop reads the closed channel and carries
    /// on. Restoring on that thread would leave the loop drawing into a cooked,
    /// non-alternate terminal. And once the TUI has exited, a restore would put
    /// escape sequences on stdout — where `perch list --json` writes.
    #[test]
    fn only_the_thread_holding_the_terminal_gives_it_back() {
        held_by(None);
        assert!(
            !ours_to_give_back(),
            "nothing holds it, so there is nothing to give back"
        );

        held_by(Some(std::thread::current().id()));
        assert!(ours_to_give_back());

        let elsewhere = std::thread::spawn(ours_to_give_back)
            .join()
            .expect("the thread ran");
        assert!(
            !elsewhere,
            "a panic on the Refresh thread must not restore the terminal the \
             frame loop is still drawing in"
        );

        held_by(None);
        assert!(
            !ours_to_give_back(),
            "and once the TUI has exited, nothing is restored onto stdout"
        );
    }
}
