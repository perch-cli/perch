//! A terminal and a Refresh that a test drives, for the same reason
//! [`crate::host::FakeHost`] exists: the frame loop under test is the real one.
//!
//! The screen draws into a buffer instead of a terminal and keeps every frame
//! as text, so a test says what somebody pressed and then reads what they would
//! have seen. The Refresh answers when the test says it does, which is what
//! lets one assert that the loop kept drawing while a Refresh was out — the
//! whole point of taking it off the loop.

use std::collections::VecDeque;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

use crate::error::{PerchError, Result};
use crate::tui::model::Model;
use crate::tui::refresh::{Refreshed, Refresher};
use crate::tui::{Screen, Signal, view};

/// A terminal of a fixed size that remembers what was drawn in it.
pub struct FakeScreen {
    terminal: Terminal<TestBackend>,
    /// What the person does, in order. `None` is a wait that ended with
    /// nobody having pressed anything, which is most of them on a real
    /// terminal.
    doing: VecDeque<Option<Signal>>,
    drawn: Vec<String>,
    /// The last frame's cells, styles and all.
    ///
    /// Kept beside the text rather than instead of it, because the text is what
    /// nearly every test wants: a Perch frame says what it means in characters
    /// so that it survives a pipe and a colour-blind palette, and `>` and `*`
    /// are assertable without any of this. What is *not* a character is which
    /// column the keys are in — that one is a style by design — so a fake
    /// holding symbols alone could not tell a sidebar holding the keys from one
    /// that had lost them, and did not.
    styled: Option<Buffer>,
}

impl FakeScreen {
    /// A screen the size of an ordinary terminal, with a script of what
    /// somebody does at it.
    pub fn scripted(doing: Vec<Option<Signal>>) -> FakeScreen {
        FakeScreen::sized(80, 24, doing)
    }

    pub fn sized(width: u16, height: u16, doing: Vec<Option<Signal>>) -> FakeScreen {
        FakeScreen {
            terminal: Terminal::new(TestBackend::new(width, height))
                .expect("a buffer is always available to draw in"),
            doing: doing.into(),
            drawn: Vec::new(),
            styled: None,
        }
    }

    /// Every frame, in the order they were drawn.
    pub fn frames(&self) -> &[String] {
        &self.drawn
    }

    /// The frame that was on screen when the loop ended.
    pub fn last_frame(&self) -> &str {
        self.drawn.last().map(String::as_str).unwrap_or_default()
    }

    /// How the cell where `said` begins was emphasised in the last frame.
    ///
    /// The question `frames` cannot answer, and the one the two sidebars and
    /// the two listings settle between them: reversed is "the keys are here",
    /// bold is "this is where they would come back to".
    ///
    /// `None` is a frame that does not say it at all, which is a different
    /// failure from one that says it without emphasis — so the two are not
    /// collapsed into a default.
    pub fn emphasis_on(&self, said: &str) -> Option<Modifier> {
        let buffer = self.styled.as_ref()?;
        (0..buffer.area.height).find_map(|row| {
            let line: String = (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect();
            let at = line.find(said)?;
            // `find` answers in bytes and a buffer is indexed in cells. Every
            // label a test asks about is one Perch wrote, so the two agree.
            let column = line[..at].chars().count() as u16;
            Some(buffer[(column, row)].modifier)
        })
    }

    /// Whether anything drawn so far said this.
    pub fn ever_said(&self, said: &str) -> bool {
        self.drawn.iter().any(|frame| frame.contains(said))
    }
}

impl Screen for FakeScreen {
    fn draw(&mut self, model: &Model) -> Result<()> {
        self.terminal
            .draw(|frame| view::render(frame, model))
            .expect("a buffer is always available to draw in");
        let buffer = self.terminal.backend().buffer();
        self.drawn.push(as_text(buffer));
        self.styled = Some(buffer.clone());
        Ok(())
    }

    fn next(&mut self, _millis: u64) -> Result<Option<Signal>> {
        match self.doing.pop_front() {
            Some(signal) => {
                // A resize is the one signal the screen itself acts on: the
                // real terminal is asked its size at the next draw, and this
                // one has to be told.
                if let Some(Signal::Resized(width, height)) = signal {
                    self.terminal.backend_mut().resize(width, height);
                }
                Ok(signal)
            }
            // Never a Leave that the test did not write: a loop that ran past
            // its script is one whose ending the test is no longer describing,
            // and a fake that quietly ended it would hide exactly that.
            None => Err(PerchError::Other(
                "the frame loop asked for a keystroke the test did not script".to_string(),
            )),
        }
    }
}

/// The rendered buffer as a person would read it, one line per row with the
/// trailing blanks cut off.
fn as_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// A Refresh that comes back when the test says so.
#[derive(Default)]
pub struct FakeRefresher {
    /// What each Refresh was asked to read, in order.
    asked: Vec<Vec<String>>,
    /// How many more times [`Refresher::collect`] answers with nothing before
    /// the outstanding Refresh lands. Counted down per Refresh, and put back
    /// when the next one is asked for: `rounds` is how long *a* Refresh takes,
    /// not how long the first one took.
    still_out: usize,
    takes: usize,
    /// What every Refresh comes back with. Kept rather than taken, because a
    /// Refresher that could answer once looked identical to a frozen loop: the
    /// second `r` set `out` and then collected `None` for ever, so the display
    /// drew `Refreshing…` to the end of the run and `outstanding` stayed true.
    /// A test written to say that a Refresh which came back is not one that is
    /// still out would have asserted against that instead of failing.
    coming: Option<Refreshed>,
    /// Whether one has been asked for and not yet collected — the same
    /// question the real one answers by whether its channel is still there.
    out: bool,
    waited: bool,
}

impl FakeRefresher {
    /// One that never answers: what a Refresh looks like from the loop's side
    /// while Anthropic is thinking.
    pub fn out_for_ever() -> FakeRefresher {
        FakeRefresher::default()
    }

    /// One that answers with this, after `rounds` frames have gone by.
    pub fn answering(refreshed: Refreshed, rounds: usize) -> FakeRefresher {
        FakeRefresher {
            still_out: rounds,
            takes: rounds,
            coming: Some(refreshed),
            ..FakeRefresher::default()
        }
    }

    /// What each Refresh was asked to read.
    pub fn asked(&self) -> &[Vec<String>] {
        &self.asked
    }

    /// Whether the way out waited for an outstanding Refresh.
    pub fn was_waited_for(&self) -> bool {
        self.waited
    }
}

impl Refresher for FakeRefresher {
    fn ask(&mut self, emails: Vec<String>) {
        self.asked.push(emails);
        self.still_out = self.takes;
        self.out = true;
    }

    fn collect(&mut self) -> Option<Refreshed> {
        if !self.out {
            return None;
        }
        if self.still_out > 0 {
            self.still_out -= 1;
            return None;
        }
        let refreshed = self.coming.clone()?;
        self.out = false;
        Some(refreshed)
    }

    fn outstanding(&self) -> bool {
        self.out
    }

    /// Nothing to wait for that a test would have to spend the time on: the
    /// waiting is what the real one does about a thread, and there is no
    /// thread here.
    fn wait_for_it(&mut self, _millis: u64) {
        self.waited = true;
    }
}
