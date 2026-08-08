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
