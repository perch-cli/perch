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
    /// Every lock taken, in the order it was taken, with when Perch last said
    /// it was still there.
    taken: Vec<(LockSpec, DateTime<Utc>)>,
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
        let now = self.host.now();
        for (lock, said) in &mut self.taken {
            if (now - *said).num_milliseconds() >= lock.update_millis {
                // A lock that cannot be touched is one somebody else has
                // already taken away; the operation carries on regardless,
                // because stopping half way is worse than finishing.
                let _ = self.host.touch(&lock.dir);
                *said = now;
            }
        }
    }

    /// Gives every lock back, last taken first.
    ///
    /// Best-effort: the work under the lock has already happened by the time
    /// this runs, and a release that fails costs another process the staleness
    /// window rather than costing anyone their Credential.
    fn release(&mut self) {
        for (lock, _) in self.taken.drain(..).rev() {
            let _ = self.host.remove_dir_all(&lock.dir);
        }
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// Runs `work` with every lock in `locks` held, in the order given, and gives
/// them all back afterwards however it ends.
pub fn under<T>(
    host: &dyn Host,
    locks: Vec<LockSpec>,
    work: impl FnOnce(&mut Held<'_>) -> Result<T>,
) -> Result<T> {
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
        held.taken.push((lock, host.now()));
    }

    work(&mut held)
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
        "Claude Code is holding {} ({}) and did not give it back.\n\
         Nothing was changed. Try again in a moment; if it persists, quit \
         Claude Code and run this again.",
        lock.name,
        lock.dir.display()
    )))
}

/// Whether a lock is one its holder died still holding.
///
/// A holder says it is still there by touching the artifact; an artifact that
/// has gone quiet for longer than its staleness window belongs to nobody. A
/// modification time that cannot be read at all is the same answer, because a
/// lock nothing can be established about is not one anybody is waiting on.
fn abandoned(host: &dyn Host, lock: &LockSpec) -> bool {
    match host.modified_at(&lock.dir) {
        Ok(modified) => (host.now() - modified).num_milliseconds() > lock.stale_millis,
        Err(_) => true,
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
            dir: PathBuf::from(dir),
            stale_millis: 60_000,
            update_millis: 5_000,
        }
    }

    #[test]
    fn work_that_outlasts_the_update_interval_says_the_lock_is_still_held() {
        let host = FakeHost::new();
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");

        under(&host, vec![lock.clone()], |held| {
            // A keychain that stopped to ask the user for permission, which is
            // the way a Switch stalls in practice.
            host.sleep(6_000);
            held.renew();
            Ok(())
        })
        .expect("the work runs");

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

        under(
            &host,
            vec![a_lock("/Users/someone/.claude.json.lock")],
            |held| {
                held.renew();
                Ok(())
            },
        )
        .expect("the work runs");

        assert!(
            !host
                .effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Touched(_)))
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
