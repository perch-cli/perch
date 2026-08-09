//! The real terminal: raw mode, the alternate screen, and giving both back.
//!
//! Entering is three calls and leaving is three more, and the whole difficulty
//! is that leaving has to happen on every way out. A normal exit, an error out
//! of the frame loop, and a panic are three different ways, and a TUI that
//! misses one of them leaves somebody typing blind into a shell that no longer
//! echoes — a `reset` they have to know about, in a terminal that will not show
//! them typing it.
//!
//! So it is given back in three places, which between them cover every way out:
//! a [`Drop`] that runs however the loop ends, a panic hook that runs before the
//! panic is printed so the report lands on the ordinary screen rather than on
//! the alternate one that is about to disappear, and — on unix — a signal
//! handler for the two signals that end a process without unwinding it.
//!
//! That last one is not a corner. `SIGHUP` is what a dropped SSH session sends,
//! and leaving a TUI open when the connection goes is the ordinary way to lose
//! one; `SIGTERM` is what every process supervisor and `kill` sends. Neither
//! runs `Drop` and neither is a panic, so before this the terminal that came
//! back was reused by the next shell in raw mode with no cursor. Ctrl-C is not
//! among them: crossterm's raw mode turns `ISIG` off, so it arrives as a
//! keystroke the loop reads rather than as a signal.

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
        // Before raw mode, because what it remembers is the mode to put back.
        signals::remember_the_mode_to_put_back();
        terminal::enable_raw_mode().map_err(the_terminal_refused)?;
        held_by(Some(std::thread::current().id()));
        signals::restore_before_a_signal_ends_this();
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
    // The same fact, where a signal handler can read it. A `Mutex` is not one of
    // the things a handler may touch, and neither is a `ThreadId`: a signal is
    // delivered to whichever thread the operating system picks, so the thread
    // question the panic hook asks has no answer worth having there. What a
    // handler needs is only whether there is a terminal to give back at all.
    signals::the_terminal_is_ours(thread.is_some());
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
            // The restore first, then the report: a report printed into the
            // alternate screen is one printed onto a display that is about to
            // be thrown away.
            if ours_to_give_back() {
                let _ = give_it_back();
            }
            already_there(panicked);
        }));
    });
}

fn the_terminal_refused(err: impl std::fmt::Display) -> PerchError {
    PerchError::Other(format!("The terminal could not be drawn in: {err}"))
}

/// Giving the terminal back from inside a signal handler, where almost nothing
/// may be called.
///
/// `SIGHUP` is a dropped SSH session and `SIGTERM` is `kill` and every process
/// supervisor. Neither unwinds, so neither reaches [`Drop`] or the panic hook,
/// and a `perch tui` ended by either left raw mode on, the alternate screen up
/// and the cursor hidden — for whatever shell came next, which then typed blind
/// into a display it could not scroll.
///
/// Everything here is async-signal-safe, which is why none of it is
/// [`give_it_back`]: crossterm's `execute` allocates and takes a lock, and a
/// handler that allocates on a thread already inside the allocator deadlocks a
/// process that was merely going to exit. So the mode is put back with
/// `tcsetattr` and the two escape sequences are written with `write`, out of a
/// string constant that is already in the binary.
mod signals {
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

    /// Whether there is a terminal to give back. Set on the way in and cleared
    /// on the way out, so a signal arriving before or after the picker finds
    /// nothing to do — and, in particular, so nothing writes escape sequences
    /// onto the stdout `perch list --json` uses.
    ///
    /// Kept on every platform even though only unix acts on it, so the one
    /// question the handler asks has one answer and one test.
    static OURS: AtomicBool = AtomicBool::new(false);

    pub(super) fn the_terminal_is_ours(ours: bool) {
        OURS.store(ours, Relaxed);
    }

    /// Whether a signal arriving now would give the terminal back. Read from a
    /// handler and from the test that pins the two answers together; the
    /// handler reads the static directly, since a call in one is a call it has
    /// to be able to make.
    #[cfg(test)]
    pub(super) fn is_ours() -> bool {
        OURS.load(Relaxed)
    }

    /// Windows has no `SIGHUP` and no `SIGTERM`: a console that goes takes the
    /// process with it, and there is no terminal left to give back.
    #[cfg(not(unix))]
    pub(super) fn remember_the_mode_to_put_back() {}

    #[cfg(not(unix))]
    pub(super) fn restore_before_a_signal_ends_this() {}

    /// The terminal mode as it was before raw mode, for the handler to put back.
    ///
    /// A static because a handler reaches nothing else, and exactly what was
    /// there rather than `stty sane`'s guess at it: a terminal somebody had
    /// already put in a mode of their own gets that mode back.
    #[cfg(unix)]
    struct Mode(std::cell::UnsafeCell<libc::termios>);

    // SAFETY: written once per `enter`, before the handlers that read it are
    // installed, and read only from a handler in a process that is about to
    // die. Nothing else in Perch touches it.
    #[cfg(unix)]
    unsafe impl Sync for Mode {}

    #[cfg(unix)]
    static COOKED: Mode = Mode(std::cell::UnsafeCell::new(unsafe {
        std::mem::MaybeUninit::zeroed().assume_init()
    }));

    /// Whether [`COOKED`] holds a mode read off a real terminal, rather than the
    /// zeroes it starts as. A `tcsetattr` of zeroes would be worse than doing
    /// nothing at all.
    #[cfg(unix)]
    static REMEMBERED: AtomicBool = AtomicBool::new(false);

    /// Leaves the alternate screen and shows the cursor, in that order — the
    /// same two `LeaveAlternateScreen` and `Show` send, as bytes a handler may
    /// write.
    #[cfg(unix)]
    pub(super) const BACK_TO_THE_ORDINARY_SCREEN: &[u8] = b"\x1b[?1049l\x1b[?25h";

    /// Reads the mode the terminal is in now, which is the one to put back.
    /// Called before raw mode goes on.
    #[cfg(unix)]
    pub(super) fn remember_the_mode_to_put_back() {
        let mut mode: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `mode` is a `termios` this call fills in, and the descriptor
        // is this process's own standard input.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut mode) } != 0 {
            return;
        }
        // SAFETY: nothing else reads or writes this, and the handlers that will
        // are installed after `REMEMBERED` is set below.
        unsafe { *COOKED.0.get() = mode };
        REMEMBERED.store(true, Relaxed);
    }

    /// Puts the terminal back and then dies of the signal as it would have
    /// anyway.
    ///
    /// Re-raised rather than exited, so a parent still sees a process that died
    /// of `SIGTERM` — which is what a supervisor decides on.
    #[cfg(unix)]
    unsafe extern "C" fn give_it_back_and_stop(signal: libc::c_int) {
        if OURS.load(Relaxed) {
            if REMEMBERED.load(Relaxed) {
                // SAFETY: `COOKED` was written before this handler was
                // installed and nothing writes it while a handler may run.
                // `tcsetattr` is async-signal-safe; `TCSANOW` rather than
                // `TCSAFLUSH` because there is no read left for pending input
                // to confuse.
                unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, COOKED.0.get()) };
            }
            // SAFETY: `write` is async-signal-safe, and the buffer is a
            // constant in this binary. A short or failed write is a terminal
            // that has already gone, which is nothing this can do better.
            unsafe {
                libc::write(
                    libc::STDOUT_FILENO,
                    BACK_TO_THE_ORDINARY_SCREEN.as_ptr().cast(),
                    BACK_TO_THE_ORDINARY_SCREEN.len(),
                )
            };
        }
        // SAFETY: both async-signal-safe. The default disposition is put back
        // first, so the signal raised next ends the process rather than
        // arriving here again.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    /// Installs the handlers. Like the panic hook, once and for the life of the
    /// process: what stops them firing when there is nothing to give back is
    /// [`OURS`] rather than their absence.
    #[cfg(unix)]
    pub(super) fn restore_before_a_signal_ends_this() {
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        INSTALLED.call_once(|| {
            for signal in [libc::SIGTERM, libc::SIGHUP] {
                // SAFETY: the handler stores nothing, reads two atomics and a
                // static written before this call, and calls four
                // async-signal-safe functions.
                unsafe {
                    libc::signal(
                        signal,
                        give_it_back_and_stop as *const () as libc::sighandler_t,
                    )
                };
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    /// A signal handler asks the second half of the same question, and gets it
    /// from somewhere it may actually read: not the `Mutex`, which is not one of
    /// the things a handler may touch, and not the thread either — a signal goes
    /// to whichever thread the operating system picks. Both halves move
    /// together, in one test because they are one piece of process-global state
    /// and two tests would race each other over it.
    #[test]
    fn only_the_thread_holding_the_terminal_gives_it_back() {
        held_by(None);
        assert!(
            !ours_to_give_back(),
            "nothing holds it, so there is nothing to give back"
        );
        assert!(!signals::is_ours(), "and a signal has nothing to do either");

        held_by(Some(std::thread::current().id()));
        assert!(ours_to_give_back());
        assert!(signals::is_ours());

        let elsewhere = std::thread::spawn(|| (ours_to_give_back(), signals::is_ours()))
            .join()
            .expect("the thread ran");
        assert!(
            !elsewhere.0,
            "a panic on the Refresh thread must not restore the terminal the \
             frame loop is still drawing in"
        );
        assert!(
            elsewhere.1,
            "but a signal restores it from wherever it lands, because where it \
             lands is not something Perch chooses"
        );

        held_by(None);
        assert!(
            !ours_to_give_back(),
            "and once the TUI has exited, nothing is restored onto stdout"
        );
        assert!(
            !signals::is_ours(),
            "by either route — a `SIGTERM` afterwards must not write escape \
             sequences onto the stdout `perch list --json` uses"
        );
    }

    /// The bytes the handler writes are the ones crossterm would.
    ///
    /// It cannot call crossterm: `execute` allocates and takes a lock, and a
    /// handler that allocates on a thread already inside the allocator
    /// deadlocks a process that was only going to exit. So the sequences are a
    /// constant, and a constant copied out of a library is one that drifts from
    /// it silently — a terminal left on the alternate screen with no cursor,
    /// discovered by whoever's SSH session dropped.
    #[test]
    fn the_escape_sequences_a_signal_writes_are_the_ones_crossterm_sends() {
        use crossterm::Command;

        let mut expected = String::new();
        LeaveAlternateScreen
            .write_ansi(&mut expected)
            .expect("a String takes what it is given");
        Show.write_ansi(&mut expected)
            .expect("a String takes what it is given");

        assert_eq!(
            signals::BACK_TO_THE_ORDINARY_SCREEN,
            expected.as_bytes(),
            "the handler writes {:?} where crossterm sends {expected:?}",
            String::from_utf8_lossy(signals::BACK_TO_THE_ORDINARY_SCREEN),
        );
    }
}
