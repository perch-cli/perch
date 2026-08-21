//! Claude Code's own locks, taken in Claude Code's own order.
//!
//! Perch invents no scheme of its own: it takes the locks the process it is
//! racing already takes, because a lock only excludes whoever agrees to honor it
//! (ADR claude-code-chooses-the-store). Under them, Claude Code's re-read of the
//! store sees a swapped, non-expired Credential and abandons its own refresh.
//!
//! A lock artifact is a directory: `mkdir` either creates one or fails, so the
//! same call asks and answers. Which directories, in which order and under what
//! staleness is [`crate::probe`]'s.

use std::path::{Path, PathBuf};

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
    /// Perch has established that it no longer holds this lock. A lock artifact
    /// carries no holder identity, so that timestamp is one: Perch alone touches
    /// a lock it holds, and an artifact saying anything else belongs to somebody
    /// who took it over.
    stamp: Option<DateTime<Utc>>,
    /// When Perch last said the lock was still there, on Perch's own clock.
    said: DateTime<Utc>,
}

impl Taken {
    /// Records that this hold is over, and says why alongside what it costs.
    ///
    /// One place, because the two ways a hold ends have to leave the same state
    /// behind and only differ in the sentence: what the lock protected is the
    /// same either way, and it is [`LockSpec::lost_means`] that says it.
    fn give_up(&mut self, host: &dyn Host, why: &str) {
        self.stamp = None;
        host.note(&format!("{why} {}", self.lock.lost_means));
    }

    /// Ends the hold if the artifact has not been touched inside its own
    /// staleness window, whatever the reason it was not. What bounds "a hiccup
    /// is not a takeover", and judged on the stamp rather than on Perch's
    /// patience because a contender decides by the stamp: past `stale_millis`,
    /// any Claude Code is entitled to clear the artifact and take the lock.
    fn let_go_if_stale(&mut self, host: &dyn Host, now: DateTime<Utc>) {
        let Some(stamp) = self.stamp else { return };
        if (now - stamp).num_milliseconds() <= self.lock.stale_millis {
            return;
        }
        self.give_up(
            host,
            &format!(
                "{} ({}) went {}ms without being touched, which is longer than \
                 anything else will wait before taking it over.",
                self.lock.name,
                self.lock.dir.display(),
                (now - stamp).num_milliseconds(),
            ),
        );
    }
}

impl<'a> Held<'a> {
    /// Says that the locks are still held, for the holders of the ones whose
    /// update interval has passed. A holder that goes quiet for longer than a
    /// lock's staleness window is taken to have died; Perch's hold is short, but
    /// a keychain that stops to ask the user for permission stretches it without
    /// warning.
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

            // Asked *before* the touch: the timestamp Perch last left is the
            // only identity a lock artifact has, and touching overwrites the
            // evidence the question is about.
            match host.modified_at(&taken.lock.dir) {
                Ok(seen) if still_ours(taken, seen) => {}
                // A stamp that disagrees and an artifact that is gone are both
                // a takeover, said out loud and never given back afterwards.
                // Anything else is Perch's own I/O, and is told apart below.
                Ok(_) | Err(HostError::NotFound { .. }) => {
                    taken.give_up(
                        host,
                        &format!(
                            "{} ({}) was taken over while Perch was working under it.",
                            taken.lock.name,
                            taken.lock.dir.display(),
                        ),
                    );
                    continue;
                }
                // There and unreadable. Nothing is evidence either way, so
                // nothing is concluded and nothing is touched: touching would
                // overwrite the one stamp that makes the check possible.
                Err(_) => {
                    taken.let_go_if_stale(host, now);
                    continue;
                }
            }

            // Ours, and the artifact would not take a fresh timestamp: on
            // Windows the handle contention `rename_replacing` retries for, so a
            // hiccup rather than a loss. The stamp Perch knows is still there.
            if host.touch(&taken.lock.dir).is_err() {
                taken.let_go_if_stale(host, now);
                continue;
            }

            // Touched, and then unreadable: the stamp Perch remembers has been
            // overwritten by one it cannot read, so every later question answers
            // the wrong way. Given up rather than held blind.
            match host.modified_at(&taken.lock.dir) {
                Ok(stamp) => taken.stamp = Some(stamp),
                Err(err) => taken.give_up(
                    host,
                    &format!(
                        "{} ({}) was renewed and then would not say when it was \
                         written ({err}), which is the only thing that makes a \
                         hold on it checkable.",
                        taken.lock.name,
                        taken.lock.dir.display(),
                    ),
                ),
            }
        }
    }

    /// Whether every lock taken is still Perch's, as of the last `renew`. A hold
    /// that has been lost cannot be regained: whoever took it over is working
    /// under it now, so [`crate::registry::save`] refuses to write where a
    /// Switch already half done finishes.
    pub fn still_held(&self) -> bool {
        self.taken.iter().all(|taken| taken.stamp.is_some())
    }

    /// Gives every lock back, last taken first.
    ///
    /// Only the ones that are still Perch's: removing whatever happens to be at
    /// the path would take a new holder's lock away. Best effort otherwise — the
    /// work under the lock has already happened by the time this runs.
    fn release(&mut self) {
        let host = self.host;
        for taken in self.taken.drain(..).rev() {
            // Removed on no evidence, because the artifact is Perch's until
            // something says otherwise and this is the last chance to give it
            // back. A stamp that disagrees or is gone is somebody else's lock.
            let ours = match host.modified_at(&taken.lock.dir) {
                Ok(seen) => still_ours(&taken, seen),
                Err(HostError::NotFound { .. }) => false,
                Err(_) => taken.stamp.is_some(),
            };
            if ours {
                let _ = host.remove_dir_all(&taken.lock.dir);
            }
        }
    }
}

/// Whether the artifact still says what Perch last left it saying.
///
/// `seen` is a stamp rather than an `Option` because both callers reach this
/// inside an `Ok(seen)` arm, and the artifact that is not there at all is
/// answered by the `Err(NotFound)` arm beside them.
fn still_ours(taken: &Taken, seen: DateTime<Utc>) -> bool {
    taken.stamp == Some(seen)
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// Two holds that have to move together: a Switch runs under Claude Code's locks
/// *and* Perch's registry lock, and both go stale on their own clocks — ten
/// seconds for the config file, ninety for the registry — so every slow step has
/// to renew both. It holds no state of its own; what it buys is that "the slow
/// steps" can be grepped for rather than kept by reading a comment.
pub struct Holds<'a, 'one, 'other> {
    one: &'a mut Held<'one>,
    other: &'a mut Held<'other>,
}

impl<'a, 'one, 'other> Holds<'a, 'one, 'other> {
    /// Two holds this scope has, whoever took them and whenever. The lifetimes
    /// stay apart because they are: a Switch takes Claude Code's locks inside
    /// the very scope that was handed Perch's.
    pub fn of(one: &'a mut Held<'one>, other: &'a mut Held<'other>) -> Holds<'a, 'one, 'other> {
        Holds { one, other }
    }

    /// Runs something slow with both holds renewed either side of it. Before as
    /// well as after: a renewal that only happens *between* steps leaves the
    /// longest step of all running under a lock somebody else may take over, and
    /// the takeover is then discovered when whatever that step did has already
    /// happened.
    pub fn around<T>(&mut self, work: impl FnOnce() -> T) -> T {
        self.renew();
        let done = work();
        self.renew();
        done
    }

    /// The same, for the one step that has to *use* a hold rather than only be
    /// protected by it: writing the registry down mid-Switch
    /// (ADR a-switch-is-written-down-first), which is a save and so takes the
    /// hold it is renewed with. Named for the write rather than for the hold, so
    /// nothing reaches for it to borrow a lock for something else.
    pub fn around_a_registry_write<T>(&mut self, write: impl FnOnce(&mut Held<'other>) -> T) -> T {
        self.renew();
        let done = write(self.other);
        self.renew();
        done
    }

    fn renew(&mut self) {
        self.one.renew();
        self.other.renew();
    }
}

/// Runs `work` with every lock in `locks` held, in the order given, and gives
/// them all back afterwards however it ends.
///
/// A lock that could not be taken fails the way the work fails, so a caller
/// answering in something richer than a [`PerchError`] has one way to fail.
pub fn under<T, E: From<PerchError>>(
    host: &dyn Host,
    locks: Vec<LockSpec>,
    work: impl FnOnce(&mut Held<'_>) -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    let mut held = take_all(host, locks).map_err(E::from)?;
    work(&mut held)
}

/// Takes every lock in `locks`, in the order given, and hands back the hold,
/// which gives them all back when it is dropped. [`under`] is the shape to reach
/// for; this one is for a hold that has to last as long as a whole command
/// rather than a closure — Perch's registry lock spans a load, whatever the
/// command does, and the save.
pub fn take_all(host: &dyn Host, locks: Vec<LockSpec>) -> Result<Held<'_>> {
    let mut held = Held {
        host,
        taken: Vec::new(),
    };

    for lock in locks {
        // Claude Code creates the config directory before locking it, and so
        // does Perch: a lock refused for a missing parent would read as
        // contention. Privately, because every lock's parent holds a Credential.
        if let Some(parent) = lock.dir.parent() {
            host.create_private_dir_all(parent).map_err(|err| {
                PerchError::Other(format!("could not create {}: {err}", parent.display()))
            })?;
        }

        take(host, &lock)?;

        // The stamp is the only identity a lock artifact has, so a take that
        // cannot read one is a take that failed: `renew`, `still_held` and
        // `release` all answer the wrong way without it.
        let stamp = match host.modified_at(&lock.dir) {
            Ok(stamp) => stamp,
            Err(err) => {
                let _ = host.remove_dir_all(&lock.dir);
                return Err(PerchError::Other(format!(
                    "{} ({}) was taken and then would not say when it was \
                     written ({err}), which is the only thing that makes a hold \
                     on it checkable.\n\
                     Nothing was changed, and the lock was given back.",
                    lock.name,
                    lock.dir.display(),
                )));
            }
        };
        held.taken.push(Taken {
            lock,
            stamp: Some(stamp),
            said: host.now(),
        });
    }

    Ok(held)
}

/// Whether somebody is holding a lock *right now*, without waiting to find out:
/// the question [`take`] answers, minus the part that makes it a request. Taking
/// it properly and giving it back would answer too, but only after four
/// one-second waits, which makes the healthy machine the slow one. `None` where
/// the lock could not be asked about at all, which is not contention.
pub fn is_held(host: &dyn Host, lock: &LockSpec) -> Option<bool> {
    match host.create_dir_exclusive(&lock.dir) {
        Ok(()) => {
            // Given straight back. Nothing was asked for beyond the answer, and
            // an artifact left behind is a lock nothing else can take until it
            // goes stale.
            let _ = host.remove_dir_all(&lock.dir);
            Some(false)
        }
        // A stale artifact reads as nobody holding it, which is `take`'s rule.
        // Nothing is cleared here: a takeover is something a caller asks for by
        // asking to hold the lock.
        Err(HostError::AlreadyExists { .. }) => Some(!abandoned(host, lock)),
        Err(_) => None,
    }
}

/// Takes one lock, waiting out a holder that is alive and taking over from one
/// that is not.
fn take(host: &dyn Host, lock: &LockSpec) -> Result<()> {
    for attempt in 1..=ATTEMPTS {
        match host.create_dir_exclusive(&lock.dir) {
            Ok(()) => return Ok(()),
            Err(HostError::AlreadyExists { .. }) => {
                // Asked here only to keep the ordinary contended wait free of
                // the claim below. `take_over` answers whether it took the lock
                // rather than whether it cleared one, so `false` is a holder.
                if abandoned(host, lock) && take_over(host, lock)? {
                    return Ok(());
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

    // Busy rather than a general failure: nothing is wrong and nothing was
    // changed, and the two callers that run unattended have to tell this from a
    // fault.
    Err(PerchError::Busy(format!(
        "{} ({}) is held by {} and was not given back.\n\
         Nothing was changed. Try again in a moment; if it persists, quit it \
         and run this again.",
        lock.name,
        lock.dir.display(),
        lock.held_by,
    )))
}

/// Where a process says it is the one clearing this abandoned lock.
///
/// Beside the lock rather than inside it, because what is being claimed is the
/// right to *delete* the lock. [`crate::reconcile`] holds it back along with the
/// lock it guards (ADR everything-but-the-account).
fn takeover_claim(lock: &LockSpec) -> PathBuf {
    let mut claim = lock.dir.clone().into_os_string();
    claim.push(TAKEOVER_SUFFIX);
    PathBuf::from(claim)
}

/// The suffix that names one.
const TAKEOVER_SUFFIX: &str = ".perch-takeover";

/// Clears an abandoned lock for the one process that gets to, answering whether
/// this call may now create it — `false` being somebody else clearing, or a lock
/// that was not abandoned after all. `mkdir` cannot be raced, so the claim
/// decides who clears; staleness is asked again under it, and the lock is taken
/// under it too, so the lock is never absent while the claim is free.
fn take_over(host: &dyn Host, lock: &LockSpec) -> Result<bool> {
    let claim = takeover_claim(lock);
    // A claim outliving the process that made it would stop this lock ever
    // being taken over again, so one as stale as the lock it guards is cleared.
    if let Err(refused) = host.create_dir_exclusive(&claim) {
        if matches!(refused, HostError::AlreadyExists { .. })
            && gone_quiet_for(host, &claim, lock.stale_millis)
        {
            let _ = host.remove_dir_all(&claim);
        }
        return Ok(false);
    }

    // Asked again, under the claim. The answer read before it was about a
    // moment that has passed, and what it decides is a deletion.
    let taken = match abandoned(host, lock) {
        true => clear_the_abandoned(host, lock)
            .map(|cleared| cleared && host.create_dir_exclusive(&lock.dir).is_ok()),
        false => Ok(false),
    };

    // On every way out, including the refusal: a claim that stays is a lock
    // nothing can ever take over.
    let _ = host.remove_dir_all(&claim);
    taken
}

/// Clears a lock nobody is holding any more, refusing rather than spinning when
/// what is in the way is not a lock at all: `remove_dir_all` does not follow the
/// last component, so a plain file at the path fails with `ENOTDIR` for ever and
/// no amount of waiting changes it. `Ok(true)` where the lock was cleared,
/// `Ok(false)` where trying again might work, a refusal for the wedge.
fn clear_the_abandoned(host: &dyn Host, lock: &LockSpec) -> Result<bool> {
    let Err(err) = host.remove_dir_all(&lock.dir) else {
        return Ok(true);
    };

    // Asked as `is_file`, which is the definition of what the refusal claims.
    // Asked as "can it be listed", a root-owned directory a `sudo claude` left
    // fails the same way and would be reported as not being a directory.
    if !host.is_file(&lock.dir) {
        return Ok(false);
    }

    Err(PerchError::Other(format!(
        "{} ({}) is not a lock directory and could not be cleared: {err}.\n\
         Nothing was changed. A lock is a directory, and nothing will be \
         able to take this one until whatever is at that path is removed.",
        lock.name,
        lock.dir.display(),
    )))
}

/// Whether a lock is one its holder died still holding.
///
/// A holder says it is still there by touching the artifact; an artifact that
/// has gone quiet for longer than its staleness window belongs to nobody.
fn abandoned(host: &dyn Host, lock: &LockSpec) -> bool {
    gone_quiet_for(host, &lock.dir, lock.stale_millis)
}

/// Whether an artifact has said nothing for longer than it is allowed to.
///
/// Shared by the lock and by the claim on taking one over, because they go
/// stale by the same rule and for the same reason — a directory nobody is
/// touching belongs to a process that is not there.
fn gone_quiet_for(host: &dyn Host, at: &Path, stale_millis: i64) -> bool {
    match host.modified_at(at) {
        Ok(modified) => (host.now() - modified).num_milliseconds() > stale_millis,
        // Gone between the two calls: whoever held it has given it back, and
        // the next attempt simply takes it.
        Err(HostError::NotFound { .. }) => true,
        // Anything else says nothing about the holder, and a lock nothing can be
        // established about is one to wait on rather than one to take over.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::host::prelude::*;
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
            lost_means: "Perch is finishing what it started.",
        }
    }

    /// A lock somebody else is already holding, planted the way the machine
    /// would have to produce one: the parent first, because
    /// `create_dir_exclusive` is `mkdir` rather than `mkdir -p`.
    fn already_held(host: &FakeHost, lock: &LockSpec) {
        if let Some(parent) = lock.dir.parent() {
            host.create_dir_all(parent).expect("the Profile is there");
        }
        host.create_dir_exclusive(&lock.dir)
            .expect("nobody was holding it yet");
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
    fn renewing_a_lock_nobody_touched_keeps_it_rather_than_losing_it_to_itself() {
        let host = FakeHost::new();
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");

        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            // Three stalls, each past the update interval: a hold whose first
            // renewal lost it would go quiet from here on.
            for _ in 0..3 {
                host.sleep(6_000);
                held.renew();
                assert!(held.still_held(), "nobody has taken it");
            }
            Ok(())
        });
        ran.expect("the work runs");

        assert!(
            host.notes().is_empty(),
            "nothing was taken over, so nothing is said: {:?}",
            host.notes()
        );
        assert!(
            !host.path_exists(Path::new(&lock.dir)),
            "and a lock Perch still holds is a lock Perch gives back"
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
            already_held(&host, &lock);
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

    #[test]
    fn a_slow_step_renews_both_holds_before_it_starts_and_not_only_after() {
        let theirs = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let ours = a_lock("/Users/someone/.config/perch/.registry.lock");
        let host = FakeHost::new();

        /// Somebody who wants the lock and takes it only if it looks abandoned,
        /// which is the protocol every holder is judged by.
        fn a_contender_looks(host: &FakeHost, lock: &LockSpec) {
            let left = host.modified_at(&lock.dir).expect("the artifact is there");
            if (host.now() - left).num_milliseconds() >= lock.stale_millis {
                host.remove_dir_all(&lock.dir).unwrap();
                already_held(host, lock);
            }
        }

        let mut both_held = false;
        let taken: Result<()> = under(&host, vec![ours.clone()], |perch| {
            under(&host, vec![theirs.clone()], |held| {
                // Whatever the step is preceded by — `prepare`, in a Switch.
                // Under the window on its own.
                host.sleep(55_000);

                let mut holds = Holds::of(held, perch);
                holds.around(|| {
                    // And the step, also under the window on its own. Together
                    // they are over it, so what decides this is whether the
                    // holds were renewed on the way in.
                    host.sleep(55_000);
                    a_contender_looks(&host, &theirs);
                    a_contender_looks(&host, &ours);
                });

                both_held = held.still_held() && perch.still_held();
                Ok(())
            })
        });
        taken.expect("the work finishes");

        assert!(
            both_held,
            "a step whose hold was renewed before it began is a step nobody \
             could have judged abandoned: {:?}",
            host.notes()
        );

        // And the contender is a real one: left alone past the window, the same
        // fixture takes the lock. Without this the assertion above would pass
        // for a contender that never looks.
        let alone = a_lock("/Users/someone/.claude/.unrenewed.lock");
        let left_alone: Result<()> = under(&host, vec![alone.clone()], |_| {
            let taken_at = host.modified_at(&alone.dir).expect("it was taken");
            host.sleep(90_000);
            a_contender_looks(&host, &alone);
            assert_ne!(
                host.modified_at(&alone.dir).expect("somebody holds it now"),
                taken_at,
                "the contender takes a lock nobody renewed"
            );
            Ok(())
        });
        left_alone.expect("the work finishes");
    }

    #[test]
    fn a_touch_that_will_not_go_through_is_a_hiccup_rather_than_a_takeover() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let mut still_held = false;
        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            // Past the update interval so a renewal is due, and well inside the
            // staleness window so the hiccup is the only thing under test —
            // `let_go_if_stale` owns what happens beyond it.
            host.sleep(10_000);
            host.set_unwritable(&lock.dir, "Permission denied");
            held.renew();
            still_held = held.still_held();
            host.writable_again(&lock.dir);
            Ok(())
        });
        ran.expect("the work finishes");

        assert!(still_held, "the hold Perch has is the hold Perch reports");
        assert!(
            !host.notes().iter().any(|note| note.contains("taken over")),
            "and nobody is blamed for a filesystem that hiccuped: {:?}",
            host.notes()
        );
        assert!(
            !host.path_exists(Path::new(&lock.dir)),
            "the artifact is given back rather than leaked for the whole \
             staleness window"
        );
    }

    #[test]
    fn an_artifact_that_will_not_say_when_it_was_written_is_left_alone() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let mut still_held = false;
        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            // Inside the staleness window, for the reason the touch case above
            // gives.
            host.sleep(10_000);
            host.set_unreadable(&lock.dir, "Permission denied");
            held.renew();
            still_held = held.still_held();
            host.forget_unreadable(&lock.dir);
            Ok(())
        });
        ran.expect("the work finishes");

        assert!(
            still_held,
            "nothing was established, so nothing is concluded"
        );
        assert!(
            !host
                .effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Touched(_))),
            "and the stamp the check rests on is not overwritten: {:?}",
            host.effects()
        );
    }

    /// The config lock is where this bites: `probe` gives it 10s against a 5s
    /// update, so one missed renewal reaches the boundary.
    #[test]
    fn a_hiccup_that_outlasts_the_staleness_window_is_a_hold_perch_stops_claiming() {
        for (what, unreadable) in [("the touch", false), ("the read", true)] {
            let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
            let host = FakeHost::new();

            let mut still_held = true;
            let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
                if unreadable {
                    host.set_unreadable(&lock.dir, "Permission denied");
                } else {
                    host.set_unwritable(&lock.dir, "Permission denied");
                }
                // Past the window rather than merely past the update interval,
                // which is what tells this apart from a hiccup.
                host.sleep(lock.stale_millis as u64 + 1_000);
                held.renew();
                still_held = held.still_held();
                if unreadable {
                    host.forget_unreadable(&lock.dir);
                } else {
                    host.writable_again(&lock.dir);
                }
                Ok(())
            });
            ran.expect("the work finishes");

            assert!(
                !still_held,
                "{what}: a lock anything else may now take is not one Perch \
                 goes on reporting: {:?}",
                host.notes()
            );
            assert!(
                host.notes()
                    .iter()
                    .any(|note| note.contains("without being touched")),
                "{what}: and it says which lock and why, rather than blaming a \
                 takeover nobody made: {:?}",
                host.notes()
            );
        }
    }

    #[test]
    fn a_lock_perch_still_holds_is_given_back_even_when_the_artifact_will_not_be_read() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let ran: Result<()> = under(&host, vec![lock.clone()], |_held| {
            // Unreadable from here to the end, so the read `release` makes is
            // the one that fails — a `chmod` on the parent, an EIO, the handle
            // contention Windows shows for a directory something else is in.
            host.set_unreadable(&lock.dir, "Permission denied");
            Ok(())
        });
        ran.expect("the work finishes");

        host.forget_unreadable(&lock.dir);
        assert!(
            !host.path_exists(Path::new(&lock.dir)),
            "an unreadable artifact is Perch's own I/O faltering, not evidence \
             that somebody else took the lock"
        );
    }

    #[test]
    fn a_lock_somebody_else_has_taken_over_is_left_where_it_is() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let ran: Result<()> = under(&host, vec![lock.clone()], |_held| {
            // Whoever took it over cleared the stale artifact and made their
            // own, which carries their timestamp rather than Perch's.
            host.sleep(90_000);
            host.touch(Path::new(&lock.dir)).expect("they take it");
            Ok(())
        });
        ran.expect("the work finishes");

        assert!(
            host.path_exists(Path::new(&lock.dir)),
            "the new holder keeps their lock"
        );
    }

    #[test]
    fn the_directory_a_lock_brings_into_being_is_the_owners_alone() {
        let host = FakeHost::new();
        let lock = a_lock("/Users/someone/.config/perch/profiles/some-account/.oauth_refresh.lock");

        let held = take_all(&host, vec![lock.clone()]).expect("the lock is free");
        drop(held);

        assert_eq!(
            host.mode_of(lock.dir.parent().expect("a lock has a parent")),
            Some(crate::host::PRIVATE_DIR_MODE),
            "the Profile this lock is inside is about to hold a Credential"
        );
    }

    #[test]
    fn a_lock_nothing_can_be_established_about_is_waited_on_rather_than_taken() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new().with_unreadable_file(&lock.dir, "Permission denied");
        // Held by somebody, and refusing to say when they last said so.
        already_held(&host, &lock);

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
    fn a_lock_that_will_not_say_when_it_was_written_is_given_back_rather_than_held() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        // Nothing is at the path, so the take itself succeeds — and then the
        // stamp cannot be read.
        let host = FakeHost::new().with_unreadable_file(&lock.dir, "Permission denied");

        let mut ran = false;
        let outcome: Result<()> = under(&host, vec![lock.clone()], |_| {
            ran = true;
            Ok(())
        });

        let refusal = outcome.expect_err("a hold nothing can be checked is no hold");
        assert!(!ran, "and the work under it never started");
        assert!(
            refusal
                .to_string()
                .contains("would not say when it was written"),
            "{refusal}"
        );
        assert!(
            host.effects()
                .iter()
                .any(|effect| matches!(effect, Effect::RemovedDir(at) if at == &lock.dir)),
            "the directory it had just made is given back: {:?}",
            host.effects()
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

    /// The behavior every rule below is a qualification of.
    #[test]
    fn a_lock_that_has_gone_quiet_for_longer_than_its_window_is_taken_over() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();
        let long_ago = host.now() - chrono::Duration::seconds(120);
        let host = host.with_dir_held_since(&lock.dir, long_ago);

        take(&host, &lock).expect("nobody is holding it any more");

        assert!(
            !host.path_exists(&takeover_claim(&lock)),
            "and the claim on clearing it is given back: {:?}",
            host.effects()
        );
    }

    /// Asserted as the loser sees it: while somebody else holds the claim, an
    /// abandoned lock is not cleared however stale it looks.
    #[test]
    fn an_abandoned_lock_is_not_cleared_while_somebody_else_is_clearing_it() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();
        let now = host.now();
        let host = host
            .with_dir_held_since(&lock.dir, now - chrono::Duration::seconds(120))
            // The other perch, between its two calls.
            .with_dir_held_since(takeover_claim(&lock), now);

        assert!(
            !take_over(&host, &lock).expect("nothing failed"),
            "this call does not get to clear it"
        );
        assert!(
            host.path_exists(&lock.dir),
            "so the artifact the other perch is about to replace is still there"
        );
        assert!(
            host.path_exists(&takeover_claim(&lock)),
            "and its claim was not taken away either"
        );
    }

    /// The winner's side of the same moment: by the time the claim is free, the
    /// lock is one somebody is holding.
    #[test]
    fn a_lock_taken_over_while_this_one_waited_is_not_taken_over_again() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();
        let now = host.now();
        // What the winner left: a fresh lock, and the claim given back.
        let host = host.with_dir_held_since(&lock.dir, now);

        assert!(
            !take_over(&host, &lock).expect("nothing failed"),
            "the answer read before the claim was about a moment that has passed"
        );
        assert!(host.path_exists(&lock.dir), "somebody is holding it");
    }

    /// Asserted as an ordering, because that is the whole of it: there is no
    /// moment at which the lock is absent and the claim is free.
    #[test]
    fn the_lock_a_takeover_makes_comes_into_being_before_the_claim_is_given_back() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();
        let long_ago = host.now() - chrono::Duration::seconds(120);
        let host = host.with_dir_held_since(&lock.dir, long_ago);
        host.forget_effects();

        assert!(
            take_over(&host, &lock).expect("nothing failed"),
            "the lock was abandoned and nobody else was clearing it"
        );

        let claim = takeover_claim(&lock);
        let effects = host.effects();
        let took = effects
            .iter()
            .position(|effect| matches!(effect, Effect::Took(at) if *at == lock.dir))
            .expect("the lock was taken");
        let gave_back = effects
            .iter()
            .position(|effect| matches!(effect, Effect::RemovedDir(at) if *at == claim))
            .expect("the claim was given back");
        assert!(
            took < gave_back,
            "the lock is taken under the claim, not after it: {effects:?}"
        );
        assert!(host.path_exists(&lock.dir), "and it is held on the way out");
        assert!(!host.path_exists(&claim), "with the claim given back");
    }

    #[test]
    fn a_claim_left_behind_by_a_death_is_cleared_rather_than_waited_on_for_ever() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();
        let long_ago = host.now() - chrono::Duration::seconds(120);
        let host = host
            .with_dir_held_since(&lock.dir, long_ago)
            .with_dir_held_since(takeover_claim(&lock), long_ago);

        assert!(
            !take_over(&host, &lock).expect("nothing failed"),
            "this attempt makes no claim of its own"
        );
        assert!(
            !host.path_exists(&takeover_claim(&lock)),
            "but it clears the one nobody is behind"
        );

        take(&host, &lock).expect("so the next attempt takes the lock");
    }
}

/// The exclusivity claim itself, on a real filesystem and with real threads.
/// `mkdir` either creates a directory or fails, and that is the whole of the
/// lock protocol; everything above checks it sequentially. Ungated, because it
/// works in a directory of its own in `temp_dir` and touches nothing the
/// developer owns (ADR a-suite-is-named-and-gated).
#[cfg(test)]
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
            lost_means: "Perch is finishing what it started.",
        };

        // Incremented on the way in and decremented on the way out, so anything
        // above one means two holders overlapped. `taken` counts the holds that
        // happened, since a thread that loses every attempt proves nothing.
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
