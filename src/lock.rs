//! Claude Code's own locks, taken in Claude Code's own order.
//!
//! Perch does not invent a scheme of its own: it takes the locks the process it
//! is racing already takes, because a lock only excludes whoever agrees to
//! honour it (ADR 0001). Under them, Claude Code's double-checked re-read of the
//! credential store sees a swapped, non-expired Credential and abandons the
//! refresh it was about to make — which is what makes swapping a live Credential
//! safe at all.
//!
//! A lock artifact is a directory. `mkdir` either creates one or fails, with
//! nothing in between, so the same call both asks and answers. Which
//! directories, in which order, and under what staleness is Claude Code's
//! business and therefore [`crate::probe`]'s; this module knows only how to
//! hold what it is handed.

use chrono::{DateTime, Utc};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::probe::LockSpec;

/// How many times a contended lock is waited on before Perch gives up. Claude
/// Code holds these for one refresh, so a lock still held after this long is
/// far more likely to belong to something wedged than to something working.
const ATTEMPTS: u32 = 5;

/// How long to wait between attempts. Claude Code jitters the same wait; Perch
/// takes these locks once per command rather than in a loop, so a fixed wait
/// has nothing to spread out.
const WAIT_MILLIS: u64 = 1_000;

/// Locks that are held right now, and are released when this is dropped.
pub struct Held<'a> {
    host: &'a dyn Host,
    /// Every lock taken, in the order it was taken.
    taken: Vec<Taken>,
}

/// One lock Perch holds, and the evidence that it still does.
struct Taken {
    lock: LockSpec,
    /// The artifact's modification time as Perch last left it, and `None` once
    /// Perch has established that it no longer holds this lock.
    ///
    /// A lock artifact carries no holder identity — it is a bare directory,
    /// which is Claude Code's protocol and not Perch's to change. But Perch is
    /// the only thing that touches a lock it holds, so the timestamp it last
    /// wrote *is* an identity of a kind: an artifact still saying what Perch
    /// left it saying is the artifact Perch took, and one saying anything else
    /// belongs to somebody who took it over.
    stamp: Option<DateTime<Utc>>,
    /// When Perch last said the lock was still there, on Perch's own clock.
    said: DateTime<Utc>,
}

impl<'a> Held<'a> {
    /// Says that the locks are still held, for the holders of the ones whose
    /// update interval has passed.
    ///
    /// A holder that goes quiet for longer than a lock's staleness window is
    /// taken to have died, and another process will take the lock out from
    /// under it. Perch's hold is short, but a keychain that stops to ask the
    /// user for permission can stretch it without warning.
    pub fn renew(&mut self) {
        let host = self.host;
        let now = host.now();
        for taken in &mut self.taken {
            if (now - taken.said).num_milliseconds() < taken.lock.update_millis {
                continue;
            }
            taken.said = now;
            if taken.stamp.is_none() {
                continue;
            }

            // A lock that cannot be touched, or that no longer says what Perch
            // left it saying, is one somebody else has taken over. The work
            // carries on regardless — stopping half way through writing a
            // Credential is worse than finishing — but it is said out loud, and
            // the lock is never given back afterwards, because giving back
            // somebody else's lock is how the loss spreads to a third process.
            let touched = host.touch(&taken.lock.dir).is_ok();
            let stamp = host.modified_at(&taken.lock.dir).ok();
            if touched && still_ours(taken, stamp) {
                taken.stamp = stamp;
                continue;
            }

            taken.stamp = None;
            host.note(&format!(
                "{} ({}) was taken over while Perch was working under it, so \
                 something else may be writing the same Credential. Perch is \
                 finishing what it started rather than stopping half way; check \
                 the Account you land on.",
                taken.lock.name,
                taken.lock.dir.display(),
            ));
        }
    }

    /// Gives every lock back, last taken first.
    ///
    /// Only the ones that are still Perch's. Removing whatever happens to be at
    /// the path would take a new holder's lock away and leave two processes
    /// believing they have it — turning one lost lock into two.
    ///
    /// Best-effort otherwise: the work under the lock has already happened by
    /// the time this runs, and a release that fails costs another process the
    /// staleness window rather than costing anyone their Credential.
    fn release(&mut self) {
        let host = self.host;
        for taken in self.taken.drain(..).rev() {
            let stamp = host.modified_at(&taken.lock.dir).ok();
            if taken.stamp.is_some() && still_ours(&taken, stamp) {
                let _ = host.remove_dir_all(&taken.lock.dir);
            }
        }
    }
}

/// Whether the artifact still says what Perch last left it saying.
fn still_ours(taken: &Taken, stamp: Option<DateTime<Utc>>) -> bool {
    match (taken.stamp, stamp) {
        (Some(ours), Some(now)) => ours == now,
        _ => false,
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// Runs `work` with every lock in `locks` held, in the order given, and gives
/// them all back afterwards however it ends.
///
/// The work says how it fails, and a lock that could not be taken becomes that
/// same kind of failure. Callers that answer in something richer than a
/// [`PerchError`] — an outcome per Account, say — then have one way to fail
/// rather than two, and nothing has to be carried out of the closure by hand.
pub fn under<T, E: From<PerchError>>(
    host: &dyn Host,
    locks: Vec<LockSpec>,
    work: impl FnOnce(&mut Held<'_>) -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let mut held = take_all(host, locks).map_err(E::from)?;
    work(&mut held)
}

/// Takes every lock in `locks`, in the order given, and hands back the hold —
/// which gives them all back when it is dropped.
///
/// [`under`] is the shape to reach for. This one is for a hold that has to last
/// as long as a whole command rather than as long as a closure: Perch's own
/// registry lock spans a load, whatever the command does, and the save.
pub fn take_all(host: &dyn Host, locks: Vec<LockSpec>) -> Result<Held<'_>> {
    let mut held = Held {
        host,
        taken: Vec::new(),
    };

    for lock in locks {
        // Locks live beside and inside the config directory. Claude Code
        // creates that directory before locking it, and so does Perch: a lock
        // that cannot be taken because its parent is missing would read as
        // contention, which is a different problem with a different answer.
        if let Some(parent) = lock.dir.parent() {
            host.create_dir_all(parent).map_err(|err| {
                PerchError::Other(format!("could not create {}: {err}", parent.display()))
            })?;
        }

        take(host, &lock)?;
        let stamp = host.modified_at(&lock.dir).ok();
        held.taken.push(Taken {
            lock,
            stamp,
            said: host.now(),
        });
    }

    Ok(held)
}

/// Takes one lock, waiting out a holder that is alive and taking over from one
/// that is not.
fn take(host: &dyn Host, lock: &LockSpec) -> Result<()> {
    for attempt in 1..=ATTEMPTS {
        match host.create_dir_exclusive(&lock.dir) {
            Ok(()) => return Ok(()),
            Err(HostError::AlreadyExists { .. }) => {
                if abandoned(host, lock) {
                    // Whoever held this died holding it. Claude Code clears
                    // such a lock and takes it, so Perch does too — leaving it
                    // would mean nobody could ever switch on this machine again.
                    let _ = host.remove_dir_all(&lock.dir);
                    continue;
                }
                if attempt < ATTEMPTS {
                    host.sleep(WAIT_MILLIS);
                }
            }
            Err(err) => {
                return Err(PerchError::Other(format!(
                    "could not take {} at {}: {err}",
                    lock.name,
                    lock.dir.display()
                )));
            }
        }
    }

    Err(PerchError::Other(format!(
        "{} ({}) is held by {} and was not given back.\n\
         Nothing was changed. Try again in a moment; if it persists, quit it \
         and run this again.",
        lock.name,
        lock.dir.display(),
        lock.held_by,
    )))
}

/// Whether a lock is one its holder died still holding.
///
/// A holder says it is still there by touching the artifact; an artifact that
/// has gone quiet for longer than its staleness window belongs to nobody.
fn abandoned(host: &dyn Host, lock: &LockSpec) -> bool {
    match host.modified_at(&lock.dir) {
        Ok(modified) => (host.now() - modified).num_milliseconds() > lock.stale_millis,
        // Gone between the two calls: whoever held it has given it back, and
        // the next attempt simply takes it.
        Err(HostError::NotFound { .. }) => true,
        // Anything else says nothing about the holder, and a lock nothing can
        // be established about is one to wait on rather than one to take over.
        // Waiting costs a command that has to be run again; taking costs two
        // processes writing one Credential.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::host::FakeHost;
    use crate::host::fake::Effect;

    fn a_lock(dir: &str) -> LockSpec {
        LockSpec {
            name: "the refresh lock",
            held_by: "Claude Code",
            dir: PathBuf::from(dir),
            stale_millis: 60_000,
            update_millis: 5_000,
        }
    }

    #[test]
    fn work_that_outlasts_the_update_interval_says_the_lock_is_still_held() {
        let host = FakeHost::new();
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");

        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            // A keychain that stopped to ask the user for permission, which is
            // the way a Switch stalls in practice.
            host.sleep(6_000);
            held.renew();
            Ok(())
        });
        ran.expect("the work runs");

        assert!(
            host.effects().contains(&Effect::Touched(lock.dir.clone())),
            "a holder that goes quiet for longer than the staleness window has \
             its lock taken out from under it: {:?}",
            host.effects()
        );
    }

    #[test]
    fn work_that_finishes_inside_the_update_interval_touches_nothing() {
        let host = FakeHost::new();

        let ran: Result<()> = under(
            &host,
            vec![a_lock("/Users/someone/.claude.json.lock")],
            |held| {
                held.renew();
                Ok(())
            },
        );
        ran.expect("the work runs");

        assert!(
            !host
                .effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Touched(_)))
        );
    }

    /// A lock artifact carries no holder identity, so a release that removed
    /// whatever happened to be at the path would take the *new* holder's lock
    /// away — turning one lost lock into two processes each believing they have
    /// it. What Perch last left the artifact saying is the only identity there
    /// is, and it is checked before anything is given back.
    #[test]
    fn a_lock_somebody_else_has_taken_over_is_not_given_back_for_them() {
        let host = FakeHost::new();
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");

        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            // Perch stalls — a keychain stopping to ask for permission — long
            // enough that somebody takes the lock as abandoned and takes it
            // over. Their hold is the one at the path now.
            host.sleep(90_000);
            host.remove_dir_all(&lock.dir).unwrap();
            host.create_dir_exclusive(&lock.dir).unwrap();
            held.renew();
            Ok(())
        });
        ran.expect("the work finishes rather than stopping half way");

        assert!(
            host.path_exists(Path::new(&lock.dir)),
            "the new holder still has their lock"
        );
        assert!(
            host.notes().iter().any(|note| note.contains("taken over")),
            "and the loss is said out loud rather than passed over: {:?}",
            host.notes()
        );
    }

    /// The other direction: a modification time that cannot be read says
    /// nothing about the holder, and a lock nothing can be established about is
    /// one to wait on. Taking it over costs two processes writing one
    /// Credential; waiting costs a command that has to be run again.
    #[test]
    fn a_lock_nothing_can_be_established_about_is_waited_on_rather_than_taken() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new().with_unreadable_file(&lock.dir, "Permission denied");
        // Held by somebody, and refusing to say when they last said so.
        host.create_dir_exclusive(&lock.dir).unwrap();

        let outcome: Result<()> = under(&host, vec![lock.clone()], |_| Ok(()));

        assert!(outcome.is_err(), "it is not taken over");
        assert!(
            host.effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Slept { .. })),
            "it is waited on"
        );
    }

    #[test]
    fn locks_are_given_back_last_first_however_the_work_ended() {
        let host = FakeHost::new();
        let first = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let second = a_lock("/Users/someone/.claude.lock");

        let outcome: Result<()> = under(&host, vec![first.clone(), second.clone()], |_| {
            Err(PerchError::Other("the work failed".into()))
        });

        assert!(outcome.is_err());
        let taken_and_given: Vec<Effect> = host
            .effects()
            .into_iter()
            .filter(|effect| matches!(effect, Effect::Took(_) | Effect::RemovedDir(_)))
            .collect();
        assert_eq!(
            taken_and_given,
            vec![
                Effect::Took(first.dir.clone()),
                Effect::Took(second.dir.clone()),
                Effect::RemovedDir(second.dir.clone()),
                Effect::RemovedDir(first.dir.clone()),
            ],
            "work that failed still gives back what it took, innermost first"
        );
        assert!(!host.path_exists(Path::new(&first.dir)));
    }
}

/// The exclusivity claim itself, on a real filesystem and with real threads.
///
/// `mkdir` either creates a directory or fails, with nothing in between, and
/// that is the whole of the lock protocol — every other rule here is about who
/// may take over from whom. Everything above checks it sequentially, which is
/// to say it checks a claim about concurrency without any.
///
/// Behind the `contract` feature with the rest of what asserts against the real
/// machine: eight threads contending really do wait on each other, which is
/// seconds of wall clock that should not be spent by every `cargo test`.
#[cfg(all(test, feature = "contract"))]
mod exclusivity {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::host::RealHost;
    use crate::probe::LockSpec;

    #[test]
    fn only_one_of_eight_threads_holds_the_lock_at_a_time() {
        const THREADS: usize = 8;
        const EACH: usize = 5;

        let dir = std::env::temp_dir().join(format!("perch-lock-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Long enough that nothing here is ever taken as abandoned: a takeover
        // is a different rule, and this test is about the one underneath it.
        let lock = LockSpec {
            name: "the refresh lock",
            held_by: "Claude Code",
            dir: dir.join(".oauth_refresh.lock"),
            stale_millis: 600_000,
            update_millis: 600_000,
        };

        // Incremented on the way in and decremented on the way out, so anything
        // above one means two holders overlapped. `taken` counts how many holds
        // actually happened, since a thread that loses every attempt proves
        // nothing either way.
        static INSIDE: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        static TAKEN: AtomicUsize = AtomicUsize::new(0);
        INSIDE.store(0, Ordering::SeqCst);
        MOST.store(0, Ordering::SeqCst);
        TAKEN.store(0, Ordering::SeqCst);

        std::thread::scope(|threads| {
            for _ in 0..THREADS {
                let lock = lock.clone();
                threads.spawn(move || {
                    let host = RealHost::new();
                    for _ in 0..EACH {
                        let Ok(held) = take_all(&host, vec![lock.clone()]) else {
                            continue;
                        };
                        TAKEN.fetch_add(1, Ordering::SeqCst);
                        let now = INSIDE.fetch_add(1, Ordering::SeqCst) + 1;
                        MOST.fetch_max(now, Ordering::SeqCst);
                        // Long enough that an overlap would be observed rather
                        // than raced past.
                        std::thread::sleep(std::time::Duration::from_micros(100));
                        INSIDE.fetch_sub(1, Ordering::SeqCst);
                        drop(held);
                    }
                });
            }
        });

        let most = MOST.load(Ordering::SeqCst);
        let taken = TAKEN.load(Ordering::SeqCst);
        let leftover = PathBuf::from(&lock.dir).exists();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(most, 1, "{most} threads held the lock at once");
        assert!(taken > THREADS, "only {taken} holds happened at all");
        assert!(!leftover, "the last holder gave it back");
    }
}
