//! `perch tui` — the interactive view, and the loop that draws it.
//!
//! The picker is one command among several rather than the primary surface
//! (ADR 0011), so nothing here may be the only way to do anything. What it adds
//! is the choice made by eye: the Accounts, their Groups, and how full they are,
//! side by side.
//!
//! It **acts, and acts on exactly two things**: a Switch and a Run ([`act`]).
//! Both have plain command forms, which is what keeps ADR 0011's constraint
//! honest — nothing here is only here. `add`, `remove`, `purge` and `config`
//! stay out, because a keystroke away from an irreversible act is the wrong
//! ergonomics for the one surface being navigated by arrow key.
//!
//! Three rules shape the loop.
//!
//! The **first frame is drawn from cache** and never blocks on the network (ADR
//! 0015), with the age of every figure on it. A picker that hangs before showing
//! anything is worse than one showing numbers a few minutes old and saying so.
//!
//! A **Refresh is asked for and taken elsewhere** ([`refresh`]). The frame loop
//! is what makes the terminal answer to a keystroke, so it is the one thing that
//! must not sit in an HTTP round trip: a Refresh over five Accounts is five
//! Renewals and five reads, and a loop waiting on that is a screen that ignores
//! Ctrl-C.
//!
//! The **terminal is given back** — on the way out, on an error, and on a panic
//! ([`terminal`]). A TUI that dies in raw mode is one somebody has to `reset`
//! their way out of.
//!
//! The terminal is the one effect that does not go through the Host port. It is
//! a seam of its own, [`Screen`], for the reason ADR 0016 gives: what is on the
//! other side of it is ratatui's business, and a Host that knew about frames
//! would be a Host every non-TUI test had to carry. [`fake::FakeScreen`] draws
//! into a buffer a test can read, so the frame loop is driven with no terminal
//! at all.

pub mod act;
pub mod fake;
pub mod model;
pub mod refresh;
pub mod terminal;
pub mod view;

use std::io::Write;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::commands::say;
use crate::error::Result;
use crate::host::Host;
use crate::registry::Registry;

pub use model::{Asked, Left, Model, Refreshing, Tab};
pub use refresh::{Refreshed, Refresher};

/// How long the loop waits for a keystroke before drawing again.
///
/// It is also how sharply the display notices what nobody pressed a key for: a
/// Refresh coming back, and the age on every figure growing. Short enough that
/// neither lags behind what is true, long enough that a terminal left open
/// overnight is not redrawing four times a second.
pub const FRAME_MILLIS: u64 = 250;

/// What the person at the terminal did, in Perch's own words rather than
/// crossterm's.
///
/// Its own type so the frame loop and the model are driven by a test that names
/// what somebody pressed rather than by fabricated key events — and so which
/// keys mean what is decided in exactly one place ([`Signal::of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Give the terminal back and go.
    Leave,
    /// The next view, and the one before it.
    NextTab,
    PreviousTab,
    /// Down and up the listing.
    Down,
    Up,
    /// Read Utilization from Anthropic — the only thing here that touches the
    /// network, and only ever because somebody asked (ADR 0015).
    Refresh,
    /// Make the Account under the cursor the active one.
    Switch,
    /// Launch a client as the Account under the cursor, in this terminal alone.
    Run,
    /// The terminal is a different size now. Carried with the size so a test
    /// can resize the surface it is drawing into; the real terminal is asked
    /// its own size at the next draw and ignores these.
    Resized(u16, u16),
}

impl Signal {
    /// What a terminal event means to Perch, or `None` for the events it has no
    /// use for.
    ///
    /// A key *press* and nothing else: Windows reports the release of every key
    /// as an event of its own, and a loop that read both would act twice on one
    /// keystroke.
    pub fn of(event: &Event) -> Option<Signal> {
        match event {
            Event::Resize(width, height) => Some(Signal::Resized(*width, *height)),
            Event::Key(key) if key.kind == KeyEventKind::Press => Signal::of_key(key),
            _ => None,
        }
    }

    fn of_key(key: &KeyEvent) -> Option<Signal> {
        // Ctrl-C first, because raw mode is what makes it a keystroke rather
        // than a signal: nothing else in Perch has to catch it, and here
        // nothing else would.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Some(Signal::Leave),
                _ => None,
            };
        }
        // Alt and Super the same way, and they have to be said rather than
        // fallen through: the match below reads `key.code` alone, so without
        // this `Alt-x` hands the terminal to a client and `Alt-Enter` Switches.
        // A terminal sends a great many of those combinations for meanings of
        // its own, and none of them is a keystroke Perch was offered.
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return None;
        }
        match key.code {
            KeyCode::Char('q') => Some(Signal::Leave),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => Some(Signal::NextTab),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => Some(Signal::PreviousTab),
            KeyCode::Down | KeyCode::Char('j') => Some(Signal::Down),
            KeyCode::Up | KeyCode::Char('k') => Some(Signal::Up),
            KeyCode::Char('r') => Some(Signal::Refresh),
            KeyCode::Enter => Some(Signal::Switch),
            // `x` rather than `R`, which would sit one shift away from the key
            // that Refreshes. A mistyped Refresh costs a round trip; a mistyped
            // Run hands the terminal to a client, and the two should not be
            // neighbours.
            KeyCode::Char('x') => Some(Signal::Run),
            _ => None,
        }
    }
}

/// The terminal, as the frame loop needs it: somewhere to draw, and somewhere
/// keystrokes come from.
pub trait Screen {
    /// Draws one frame of what the model currently says.
    fn draw(&mut self, model: &Model) -> Result<()>;

    /// The next thing the person did, or `None` when `millis` went by and they
    /// did nothing.
    ///
    /// A wait with an end to it, rather than a read that blocks: the loop has
    /// its own reasons to come round again — a Refresh landing, an age growing
    /// — and none of them is a keystroke.
    fn next(&mut self, millis: u64) -> Result<Option<Signal>>;
}

/// The frame loop: draw what is known, take what was pressed, and come round.
///
/// It is handed a Registry rather than reading one, because the first frame is
/// drawn from what is already on disk (ADR 0015) — the loading happens before
/// the terminal is ever entered, and a Refresh replaces it wholesale from
/// somewhere that is not this loop.
///
/// It ends with what the picker was for: leaving, or the Account to launch a
/// client as. A Run is not something the loop can do and come back from — it
/// lasts as long as somebody's session — so it is what the loop ends *with*
/// rather than something it takes ([`Left`]).
pub fn browse(
    host: &dyn Host,
    registry: Registry,
    screen: &mut dyn Screen,
    refresher: &mut dyn Refresher,
) -> Result<Left> {
    let mut model = Model::new(registry, host.now());

    loop {
        screen.draw(&model)?;

        if let Some(signal) = screen.next(FRAME_MILLIS)? {
            match model.act_on(signal) {
                Asked::Nothing => {}
                Asked::ForARefresh => refresher.ask(model.accounts_on_show()),
                Asked::ForASwitch => act::switch(host, &mut model),
            }
        }

        // Whatever came back while the loop was drawing. Asked for after the
        // keystroke rather than before it, so a Refresh that landed within this
        // very round is on the next frame rather than the one after.
        if let Some(refreshed) = refresher.collect() {
            model.refreshed(refreshed);
        }

        // The ages on the figures are how old they are *now*, and a frame drawn
        // ten minutes after the last keystroke says ten minutes rather than
        // whatever it said when it was first drawn.
        model.now = host.now();

        if let Some(left) = &model.leaving {
            return Ok(left.clone());
        }
    }
}

/// What a command wrote, as the lines a frame can show it in.
///
/// A refusal is a sentence about the Account and then a sentence about the
/// repair, broken where the command broke it: the terminal wraps what is too
/// long for it ([`view`]), and a report reflowed twice reads as neither.
pub(crate) fn lines_of(said: &str) -> Vec<String> {
    said.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// What leaving does about a Refresh that has not come back.
///
/// The loop never waits, but the way out of it is the end of the process — and
/// a Refresh writes the registry under Perch's own lock. A process that exits
/// while its own thread is holding that lock leaves it behind, and the next
/// command waits the staleness window out before it can do anything. So the
/// exit waits a little, with the terminal already given back and the reason on
/// the screen the user has come back to.
///
/// Called after the [`Screen`] has been dropped, which is why it takes a writer
/// rather than drawing: there is no frame to draw on by this point.
pub fn finish(refresher: &mut dyn Refresher, out: &mut dyn Write) -> Result<()> {
    if !refresher.outstanding() {
        return Ok(());
    }
    say(
        out,
        "Finishing the Refresh you asked for — Perch is holding its own lock \
         while it writes what it read.",
    )?;
    refresher.wait_for_it(refresh::FINISHING_MILLIS);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn q_and_ctrl_c_both_leave() {
        assert_eq!(Signal::of(&key(KeyCode::Char('q'))), Some(Signal::Leave));
        assert_eq!(
            Signal::of(&Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL
            ))),
            Some(Signal::Leave)
        );
    }

    /// A plain `c` is not Ctrl-C, and Ctrl-anything-else is not a key Perch has
    /// a meaning for — a terminal sends a great many of those.
    #[test]
    fn a_modifier_is_part_of_which_key_was_pressed() {
        assert_eq!(Signal::of(&key(KeyCode::Char('c'))), None);
        assert_eq!(
            Signal::of(&Event::Key(KeyEvent::new(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL
            ))),
            None
        );

        // Which is true of every modifier and not only Control. Only Control
        // was short-circuited, and the match after it read `key.code` alone —
        // so the two keystrokes with consequences, the one that hands the
        // terminal to a client and the one that Switches, both fired with Alt
        // held.
        for held in [KeyModifiers::ALT, KeyModifiers::SUPER] {
            for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Char('q')] {
                assert_eq!(
                    Signal::of(&Event::Key(KeyEvent::new(code, held))),
                    None,
                    "{code:?} with {held:?} held is not a keystroke Perch was offered"
                );
            }
        }
    }

    #[test]
    fn the_views_move_by_tab_and_by_arrow() {
        assert_eq!(Signal::of(&key(KeyCode::Tab)), Some(Signal::NextTab));
        assert_eq!(Signal::of(&key(KeyCode::Right)), Some(Signal::NextTab));
        assert_eq!(
            Signal::of(&key(KeyCode::BackTab)),
            Some(Signal::PreviousTab)
        );
        assert_eq!(Signal::of(&key(KeyCode::Left)), Some(Signal::PreviousTab));
    }

    /// Windows reports a press and a release for one keystroke. Acting on both
    /// is a `q` that leaves twice and a `j` that moves two rows.
    #[test]
    fn a_key_being_let_go_of_is_not_a_second_keystroke() {
        let mut released = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        released.kind = KeyEventKind::Release;

        assert_eq!(Signal::of(&key(KeyCode::Char('j'))), Some(Signal::Down));
        assert_eq!(Signal::of(&Event::Key(released)), None);
    }

    #[test]
    fn the_two_acting_keys_are_enter_and_x() {
        assert_eq!(Signal::of(&key(KeyCode::Enter)), Some(Signal::Switch));
        assert_eq!(Signal::of(&key(KeyCode::Char('x'))), Some(Signal::Run));
    }

    /// The picker acts on exactly two things (ADR 0011), and nothing
    /// destructive is a keystroke away: `add`, `remove`, `purge` and `config`
    /// have no key here and are reached by typing their names.
    ///
    /// Written as the whole vocabulary rather than as a list of keys that do
    /// nothing, because the vocabulary is the thing being constrained: a
    /// keystroke that removed an Account would have to add a Signal, and that
    /// is what this fails on rather than on somebody having thought to try `d`.
    #[test]
    fn no_key_reaches_anything_but_looking_switching_and_running() {
        let allowed = [
            Signal::Leave,
            Signal::NextTab,
            Signal::PreviousTab,
            Signal::Down,
            Signal::Up,
            Signal::Refresh,
            Signal::Switch,
            Signal::Run,
        ];

        let typed = ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .map(KeyCode::Char)
            .chain([
                KeyCode::Enter,
                KeyCode::Esc,
                KeyCode::Backspace,
                KeyCode::Delete,
                KeyCode::Home,
                KeyCode::End,
                KeyCode::PageUp,
                KeyCode::PageDown,
                KeyCode::Tab,
                KeyCode::BackTab,
                KeyCode::Up,
                KeyCode::Down,
                KeyCode::Left,
                KeyCode::Right,
            ]);

        for code in typed {
            if let Some(signal) = Signal::of(&key(code)) {
                assert!(allowed.contains(&signal), "{code:?} means {signal:?}");
            }
        }
    }

    #[test]
    fn a_resize_carries_the_new_size() {
        assert_eq!(
            Signal::of(&Event::Resize(40, 10)),
            Some(Signal::Resized(40, 10))
        );
    }
}
