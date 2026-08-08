//! A Refresh asked for from the TUI, and taken where the frame loop is not.
//!
//! Every other surface that Refreshes is a command that prints a line and
//! exits, so it can afford to sit in the round trip. The TUI cannot: a Refresh
//! over five Accounts is five Renewals and five reads of `/api/oauth/usage`,
//! and a frame loop waiting on that is a screen that does not redraw when the
//! window is resized and does not leave when Ctrl-C is pressed.
//!
//! So the loop asks, keeps drawing, and picks the answer up whenever it turns
//! up. What is asked of the Refresh in between is nothing at all — a seam of
//! two methods, so a test drives the loop through a Refresh that never comes
//! back without a thread or a network.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use crate::adopt;
use crate::host::RealHost;
use crate::observe;
use crate::registry::Registry;

/// What a Refresh brought back.
///
/// The whole registry rather than the figures alone, because a Refresh writes
/// more than figures: it is the one thing that finds out an Account is beyond
/// repair, and a display that took the numbers but not the Quarantine would
/// show an Account as merely idle.
pub struct Refreshed {
    /// The registry as the Refresh left it, or `None` when it never got far
    /// enough to have one — then what is already on screen stands (ADR 0018).
    pub registry: Option<Registry>,
    /// What could not be read, in the words the Refresh used. Empty when
    /// everything was: the figures then say "just now" themselves.
    pub notes: Vec<String>,
}

impl Refreshed {
    /// One that got nowhere. Nothing to show for it, and this is why.
    pub fn nothing_read(notes: Vec<String>) -> Refreshed {
        Refreshed {
            registry: None,
            notes,
        }
    }
}

/// Somewhere to send a Refresh that is not the frame loop.
pub trait Refresher {
    /// Asks for one over these Accounts. The frame loop only ever calls this
    /// with none outstanding, because the model holds the second key press
    /// until the first Refresh is back.
    fn ask(&mut self, emails: Vec<String>);

    /// What has come back since last time, or `None` while there is nothing to
    /// collect. Never waits: it is called once per frame.
    fn collect(&mut self) -> Option<Refreshed>;

    /// Whether one is still out. Asked once, on the way out of the TUI.
    fn outstanding(&self) -> bool;

    /// Waits up to `millis` for the outstanding one to finish.
    ///
    /// Not for the frame loop, which never waits — for the moment after it,
    /// where leaving means the process ends. A Refresh writes the registry
    /// under Perch's own lock, and a process that exits mid-write leaves that
    /// lock behind for the next command to wait out. So the way out of the TUI
    /// is: give the terminal back, say what is being waited for, and wait a
    /// little.
    fn wait_for_it(&mut self, millis: u64);
}

/// How long the way out waits for a Refresh that is still going.
///
/// Long enough for the ordinary case — a couple of Accounts, each a Renewal and
/// a read — and short enough that `q` still means `q`. A Refresh that outlasts
/// it is left to finish on its own, which is said rather than hidden: the
/// figures still land, and the cost is a lock the next command may wait out.
pub const FINISHING_MILLIS: u64 = 10_000;

/// The real one: a thread of its own, with its own Host, and a channel back.
///
/// A thread rather than something cleverer because a Refresh is one round trip
/// after another and then a write, all of it blocking, and there is at most one
/// in flight. Its Host is built inside the thread rather than shared with the
/// loop's: a `RealHost` remembers what it has already said, which is state, and
/// the point of this is that the two do not have to agree about anything.
#[derive(Default)]
pub struct InAThread {
    /// Where the answer to the outstanding Refresh will arrive, when there is
    /// one outstanding.
    coming: Option<Receiver<Refreshed>>,
}

impl InAThread {
    pub fn new() -> InAThread {
        InAThread::default()
    }
}

impl Refresher for InAThread {
    fn ask(&mut self, emails: Vec<String>) {
        let (send, coming) = mpsc::channel();
        std::thread::spawn(move || {
            // Nobody is listening if the TUI has already left, and that is
            // fine: the figures were still written to the registry.
            let _ = send.send(taken(emails));
        });
        self.coming = Some(coming);
    }

    fn collect(&mut self) -> Option<Refreshed> {
        let coming = self.coming.as_ref()?;
        match coming.try_recv() {
            Ok(refreshed) => {
                self.coming = None;
                Some(refreshed)
            }
            Err(TryRecvError::Empty) => None,
            // The thread is gone without having sent anything, which is a panic
            // in it. The loop is not the place to die of that: it says the
            // Refresh failed and goes on showing what it had.
            Err(TryRecvError::Disconnected) => {
                self.coming = None;
                Some(Refreshed::nothing_read(vec![
                    "The Refresh did not finish. The figures on screen are the ones \
                     from before it."
                        .to_string(),
                ]))
            }
        }
    }

    fn outstanding(&self) -> bool {
        self.coming.is_some()
    }

    fn wait_for_it(&mut self, millis: u64) {
        if let Some(coming) = self.coming.take() {
            // What came back is thrown away: the Refresh wrote the figures to
            // the registry itself, and there is no longer a frame to draw them
            // on. What is being waited for is the write finishing and the lock
            // going back.
            let _ = coming.recv_timeout(Duration::from_millis(millis));
        }
    }
}

/// The Refresh itself, run on the thread: take Perch's lock, read what was
/// asked for, and hand back what the registry says afterwards.
fn taken(emails: Vec<String>) -> Refreshed {
    // Its remarks are kept rather than printed, because stderr is where a frame
    // is: a note about a Credential written to a store Perch would rather not
    // have used would land in the middle of the display. They come back as
    // notes instead, which is where every other thing this could not do goes.
    let host = RealHost::keeping_its_remarks();

    // Exclusively, because a Refresh writes: figures, and any Quarantine it
    // discovers. Another `perch` holding the lock is a Refresh that says so
    // rather than one that waits for ever.
    let (mut perch, mut registry) = match adopt::ensure_adopted_exclusively(&host) {
        Ok(held) => held,
        Err(refused) => return Refreshed::nothing_read(vec![refused.to_string()]),
    };

    let report = observe::refresh(&host, &mut perch, &mut registry, &emails);
    let mut notes = report.notes();
    notes.extend(host.remarks());

    Refreshed {
        registry: Some(registry),
        notes,
    }
}

/// What the frame loop sees, driven through the channel the real Refresh
/// answers on.
///
/// [`InAThread::ask`] and [`taken`] are deliberately not exercised here: between
/// them they build a [`RealHost`], take Perch's lock and write the registry of
/// whoever is running the tests. What is worth asserting is everything either
/// side of that — a channel is handed to `coming` directly, which is exactly
/// what `ask` does with it, and the loop is then asked the questions it asks
/// once a frame.
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A Refresh that has been asked for and not yet answered, and the end of
    /// the channel the thread would have answered on.
    fn outstanding_one() -> (InAThread, mpsc::Sender<Refreshed>) {
        let (send, coming) = mpsc::channel();
        (
            InAThread {
                coming: Some(coming),
            },
            send,
        )
    }

    fn a_refresh() -> Refreshed {
        Refreshed {
            registry: Some(Registry::default()),
            notes: vec!["one window could not be read".to_string()],
        }
    }

    #[test]
    fn nothing_is_outstanding_until_a_refresh_is_asked_for() {
        let mut fresh = InAThread::new();

        assert!(!fresh.outstanding());
        assert!(fresh.collect().is_none());
    }

    /// The loop calls this once a frame and must never be held up by it: while
    /// the Refresh is still going there is simply nothing to take.
    #[test]
    fn a_refresh_still_going_gives_the_frame_loop_nothing_and_keeps_it_drawing() {
        let (mut refresher, _thread) = outstanding_one();

        assert!(refresher.collect().is_none());
        assert!(refresher.collect().is_none());
        assert!(
            refresher.outstanding(),
            "it has not answered, so it is still out"
        );
    }

    #[test]
    fn the_answer_is_collected_once_and_then_nothing_is_outstanding() {
        let (mut refresher, thread) = outstanding_one();
        thread.send(a_refresh()).expect("the loop is listening");

        let collected = refresher.collect().expect("the answer is there to take");
        assert!(collected.registry.is_some());
        assert_eq!(collected.notes, ["one window could not be read"]);

        assert!(!refresher.outstanding());
        assert!(
            refresher.collect().is_none(),
            "the same answer is not handed out twice"
        );
    }

    /// A panic on the Refresh thread drops the sender without anything having
    /// been sent. The frame loop is not the place to die of that (ADR 0018):
    /// what was already on screen stands, and a note says why it did not move.
    #[test]
    fn a_refresh_thread_that_died_becomes_a_note_rather_than_a_frame_loop_that_stops() {
        let (mut refresher, thread) = outstanding_one();
        drop(thread);

        let collected = refresher.collect().expect("a disconnect is an answer too");
        assert!(
            collected.registry.is_none(),
            "nothing was read, so nothing replaces what is on screen"
        );
        assert_eq!(collected.notes.len(), 1);
        assert!(
            collected.notes[0].contains("did not finish"),
            "{:?}",
            collected.notes
        );

        assert!(!refresher.outstanding());
        assert!(refresher.collect().is_none(), "and it is not asked again");
    }

    #[test]
    fn a_refresh_that_read_nothing_has_no_registry_to_show_for_it() {
        let nothing = Refreshed::nothing_read(vec!["Perch's lock is held".to_string()]);

        assert!(nothing.registry.is_none());
        assert_eq!(nothing.notes, ["Perch's lock is held"]);
    }

    /// The way out of the TUI waits so the registry write finishes and Perch's
    /// lock goes back. With nothing outstanding there is nothing to wait for,
    /// and `q` must not pause on the way out.
    #[test]
    fn leaving_with_no_refresh_outstanding_waits_for_nothing() {
        let mut fresh = InAThread::new();

        let began = Instant::now();
        fresh.wait_for_it(FINISHING_MILLIS);

        assert!(
            began.elapsed() < Duration::from_millis(FINISHING_MILLIS),
            "it waited out the full allowance with nothing to wait for"
        );
    }

    #[test]
    fn leaving_returns_as_soon_as_the_refresh_is_done() {
        let (mut refresher, thread) = outstanding_one();
        thread.send(a_refresh()).expect("the loop is listening");

        let began = Instant::now();
        refresher.wait_for_it(FINISHING_MILLIS);

        assert!(began.elapsed() < Duration::from_millis(FINISHING_MILLIS));
        assert!(!refresher.outstanding());
    }

    /// A Refresh that outlasts the allowance is given up on rather than holding
    /// the process open for ever. Nothing kills the thread — it goes on to
    /// finish the write that the wait was about — but nobody is listening for
    /// what it hands back, which is the case [`InAThread::ask`] discards the
    /// send result for.
    #[test]
    fn a_refresh_that_outlasts_the_wait_is_given_up_on_and_answers_to_nobody() {
        let (mut refresher, thread) = outstanding_one();

        let began = Instant::now();
        refresher.wait_for_it(20);

        assert!(
            began.elapsed() < Duration::from_millis(FINISHING_MILLIS),
            "it waited the allowance it was given, not the one the way out uses"
        );
        assert!(
            !refresher.outstanding(),
            "it was given up on, so nothing waits for it twice"
        );
        assert!(
            thread.send(a_refresh()).is_err(),
            "there is no frame left to draw the answer on"
        );
    }
}
