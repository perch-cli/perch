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

            // Asked *before* the touch, and this is the whole of the ordering:
            // the timestamp Perch last left is the only identity a lock
            // artifact has, and touching overwrites the very evidence the
            // question is about. Asked afterwards, every renewal compares a
            // stamp against the one it has just replaced and reports a takeover
            // that never happened.
            //
            // A lock that no longer says what Perch left it saying is one
            // somebody else has taken over. It is said out loud, and the lock
            // is never given back afterwards, because giving back somebody
            // else's lock is how the loss spreads to a third process.
            //
            // A stamp that disagrees, and an artifact that is not there at all,
            // are both that. Anything *else* the filesystem says is Perch's own
            // I/O faltering, and is told apart below rather than folded in with
            // it: giving a hold up is expensive in three separate ways, and
            // none of them is worth spending on a filesystem that hiccuped.
            match host.modified_at(&taken.lock.dir) {
                Ok(seen) if still_ours(taken, Some(seen)) => {}
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
                // There and unreadable. Nothing here is evidence either way, so
                // nothing is concluded and nothing is touched — touching an
                // artifact Perch cannot check would overwrite the one stamp
                // that makes the check possible. The next `renew` asks again,
                // and `update_millis` is short enough against `stale_millis`
                // that there are a dozen more chances before the artifact goes
                // stale. One that never becomes readable ends as a genuine
                // takeover, which the arm above catches as one.
                Err(_) => continue,
            }

            // Ours, and the artifact would not take a fresh timestamp. On
            // Windows that is the handle contention `rename_replacing` retries
            // for, arriving at a `touch_now` that does not — a hiccup rather
            // than a loss. Nothing is inconsistent in leaving the hold as it
            // was: an artifact that was not touched still carries the stamp
            // Perch knows, so the next `renew` simply tries again.
            if host.touch(&taken.lock.dir).is_err() {
                continue;
            }

            // Touched, and then the artifact would not say what it now carries.
            // This is the one branch where the hold really is over: the stamp
            // Perch remembers has just been overwritten by one it cannot read,
            // so every question asked later answers the wrong way — the same
            // dead end `take` refuses a lock outright for. Given up rather than
            // held blind, and said as what it is rather than as a takeover.
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

    /// Whether every lock taken is still Perch's, as of the last [`renew`].
    ///
    /// [`renew`]: Held::renew
    ///
    /// A hold that has been lost is not a hold that can be regained: whoever
    /// took it over is working under it now. What a caller does about that
    /// depends on what it was going to do next — [`crate::registry::save`]
    /// refuses to write, where a Switch already half done finishes.
    pub fn still_held(&self) -> bool {
        self.taken.iter().all(|taken| taken.stamp.is_some())
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
            // `still_ours` is already `false` for a hold Perch has established
            // it no longer has, which is what a `stamp` of `None` means.
            if still_ours(&taken, host.modified_at(&taken.lock.dir).ok()) {
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
        //
        // Privately, because every parent a lock has is a directory that holds
        // or is about to hold a Credential — a Profile, or Perch's own home.
        // Every lock's parent, which is the whole of it: a Profile for Claude
        // Code's three, and Perch's own home for the registry's.
        // `observe::renew_under_the_lock` takes a Profile's lock off a purely
        // derived path, so before this a `perch status --refresh` was enough to
        // create the Profile of an Account whose directory had gone, at 0755,
        // ready for the next `perch relogin` to write a plaintext Credential
        // into.
        if let Some(parent) = lock.dir.parent() {
            host.create_private_dir_all(parent).map_err(|err| {
                PerchError::Other(format!("could not create {}: {err}", parent.display()))
            })?;
        }

        take(host, &lock)?;

        // The stamp is the only identity a lock artifact has, so a take that
        // cannot read one is a take that failed — every question asked later
        // answers the wrong way without it. `renew` skips the touch and says
        // nothing, so the lock goes stale under a command that is behaving
        // perfectly; `still_held` is false for ever, so `registry::save`
        // refuses to write and tells the user another `perch` took the lock
        // over when none did; and `release` will not give the artifact back, so
        // Perch's own lock leaks for the whole staleness window. There is no
        // right behaviour left, so the directory just created goes back and the
        // failure is reported as itself.
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

/// Takes one lock, waiting out a holder that is alive and taking over from one
/// that is not.
fn take(host: &dyn Host, lock: &LockSpec) -> Result<()> {
    for attempt in 1..=ATTEMPTS {
        match host.create_dir_exclusive(&lock.dir) {
            Ok(()) => return Ok(()),
            Err(HostError::AlreadyExists { .. }) => {
                // Asked here only to keep the ordinary contended wait free of
                // the artifact below: a lock Claude Code is holding right now is
                // the common case, and it is not a takeover. The answer that
                // decides anything is the one `take_over` asks under the claim.
                if abandoned(host, lock) && take_over(host, lock)? {
                    // Tried again now rather than after this attempt's wait:
                    // the lock is free as of this instant, and the attempt that
                    // found it abandoned is the one that should get it.
                    // Otherwise a takeover on the last attempt clears the lock
                    // and then reports it as held — about a lock this very call
                    // just freed.
                    if host.create_dir_exclusive(&lock.dir).is_ok() {
                        return Ok(());
                    }
                    // Somebody else got in between, which makes them a holder
                    // like any other: wait on them.
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
    // fault. `perch watch` holds the round and comes back; a scheduler reading
    // the exit code of `perch watch --once` does the same.
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
/// right to *delete* the lock. Held for the two calls it takes to ask whether
/// the lock is stale and to remove it, and given back on every way out.
///
/// [`crate::reconcile`] holds this back along with the lock it guards: it is an
/// answer about one configuration directory, and one crossed into another is
/// the hazard ADR 0026's denylist exists for.
fn takeover_claim(lock: &LockSpec) -> PathBuf {
    let mut claim = lock.dir.clone().into_os_string();
    claim.push(TAKEOVER_SUFFIX);
    PathBuf::from(claim)
}

/// The suffix that names one. Public to [`crate::reconcile`], which has to
/// recognise it without knowing what it is for.
pub const TAKEOVER_SUFFIX: &str = ".perch-takeover";

/// Clears an abandoned lock, but only for the one process that gets to.
///
/// Answers whether this call is now free to create the lock. `false` is
/// "somebody else is doing this, or it turned out not to be abandoned after
/// all" — both of which mean waiting like an ordinary contender.
///
/// The claim is what this is for. `abandoned` and `remove_dir_all` are two
/// calls with a gap between them, and two perches that both read the same stale
/// timestamp both removed and both created: the second one's `remove_dir_all`
/// took away the lock the first had *just made*, and both walked away believing
/// they held it. `mkdir` is the only operation here that cannot be raced, so it
/// is what decides who clears — and the staleness question is asked again
/// underneath it, where the answer cannot change. That second ask is the whole
/// of the fix: the loser arrives after the winner has already taken the lock,
/// finds an artifact touched a moment ago, and waits on it.
///
/// A claim outliving the process that made it would stop this lock ever being
/// taken over again, which is the wedge [`clear_the_abandoned`] is written
/// about. So one as stale as the lock it guards is itself cleared, and the next
/// attempt a second later gets it.
fn take_over(host: &dyn Host, lock: &LockSpec) -> Result<bool> {
    let claim = takeover_claim(lock);
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
    let cleared = match abandoned(host, lock) {
        true => clear_the_abandoned(host, lock).map(|()| true),
        false => Ok(false),
    };

    // On every way out, including the refusal: a claim that stays is a lock
    // nothing can ever take over.
    let _ = host.remove_dir_all(&claim);
    cleared
}

/// Clears a lock nobody is holding any more, refusing rather than spinning when
/// what is in the way is not a lock at all.
///
/// A lock is a directory, and `remove_dir_all` does not follow the last
/// component — so a plain file or a dangling symlink at the path fails with
/// `ENOTDIR`, for ever. Discarded, that failure became five attempts of no
/// progress and then `Busy`, which says the lock "is held by Claude Code and was
/// not given back" and advises quitting it and trying again. There is no Claude
/// Code to quit and the advice never works: every Switch, every Run and every
/// Renewal against that path fails that way until somebody deletes it by hand.
///
/// So this is a refusal of its own, naming the path and saying what is wrong
/// with it — the one message that turns an unrecoverable state into a
/// five-second fix.
fn clear_the_abandoned(host: &dyn Host, lock: &LockSpec) -> Result<()> {
    host.remove_dir_all(&lock.dir).map_err(|err| {
        PerchError::Other(format!(
            "{} ({}) is not a lock directory and could not be cleared: {err}.\n\
             Nothing was changed. A lock is a directory, and nothing will be \
             able to take this one until whatever is at that path is removed.",
            lock.name,
            lock.dir.display(),
        ))
    })
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
            lost_means: "Perch is finishing what it started.",
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

    /// The renewal has to survive itself. A holder says it is still there by
    /// touching the artifact, and the artifact's timestamp is also the only
    /// evidence that the artifact is still Perch's — so a renewal that checks
    /// the evidence *after* writing it reads its own touch as somebody else's
    /// and declares the lock lost on the first renewal of every hold.
    ///
    /// What that costs is here rather than in the ordering: work carries on
    /// under a hold Perch has stopped renewing, the lock goes stale under it,
    /// and it is never given back — so the next command waits out a staleness
    /// window on an artifact nobody holds.
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

    /// The mirror, and the reason a takeover is read off the stamp alone: a
    /// touch that will not go through is this machine's I/O faltering, not
    /// another process arriving. On Windows it is the handle contention
    /// `rename_replacing` retries for, reaching a `touch_now` that does not.
    ///
    /// Read as a loss it would cost three things at once, none of them
    /// recoverable within the command: `still_held` false for the rest of it,
    /// so `registry::save` refuses and blames a `perch` that does not exist; a
    /// `release` that then declines to give Perch's *own* artifact back,
    /// leaving the next command to wait out the whole staleness window; and a
    /// line telling somebody their lock was taken when it was not.
    #[test]
    fn a_touch_that_will_not_go_through_is_a_hiccup_rather_than_a_takeover() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let mut still_held = false;
        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            host.sleep(90_000);
            host.set_unwritable(&lock.dir, "Permission denied");
            held.renew();
            still_held = held.still_held();
            host.forget_unwritable(&lock.dir);
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

    /// The same for the read either side of the touch. An artifact that is
    /// there and will not say when it was written is no evidence about who
    /// holds it — and it is deliberately not touched on the way past, because
    /// touching would overwrite the one stamp that makes the check possible.
    #[test]
    fn an_artifact_that_will_not_say_when_it_was_written_is_left_alone() {
        let lock = a_lock("/Users/someone/.claude/.oauth_refresh.lock");
        let host = FakeHost::new();

        let mut still_held = false;
        let ran: Result<()> = under(&host, vec![lock.clone()], |held| {
            host.sleep(90_000);
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

    /// Every parent a lock has is a directory that holds or is about to hold a
    /// Credential — a Profile, or Perch's own home — so bringing one into being
    /// at whatever the umask happens to be is not a thing this may do.
    /// `registry::lock` creates its own parent privately and says exactly this,
    /// which left the instance here as the one that could still do it: a
    /// Profile's lock is taken off a derived path by
    /// `observe::renew_under_the_lock`, so a `perch status --refresh` against an
    /// Account whose directory had gone was enough to make it, 0755, ready for
    /// the next `perch relogin` to write a plaintext Credential into.
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

    /// And a lock that is *taken* and then will not say when it was written is
    /// a take that failed, rather than a hold with no identity.
    ///
    /// The stamp is the only identity a lock artifact has. Without one, `renew`
    /// skips the touch and says nothing — so the lock goes stale under a
    /// command that is behaving perfectly — `still_held` is false for ever, so
    /// `registry::save` refuses to write and blames a `perch` that does not
    /// exist, and `release` will not give the artifact back. Three wrong
    /// answers, none of them recoverable, so the take is what fails.
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

    /// A lock whose holder died is taken over, which is the behaviour every
    /// rule below is a qualification of.
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

    /// The race the claim exists for.
    ///
    /// `abandoned` and `remove_dir_all` are two calls with a gap between them.
    /// Two perches that both read the same stale timestamp both removed and
    /// both created — and the second one's removal took away the lock the first
    /// had just made, so both walked away believing they held it. Two processes
    /// writing one Credential, which is the one thing the lock exists to
    /// prevent.
    ///
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

    /// The winner's side of the same moment, which is what makes the loser's
    /// wait right rather than merely late: by the time the claim is free, the
    /// lock is one somebody is holding, and the staleness question asked under
    /// the claim says so.
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

    /// A claim outliving the process that made it would stop this lock ever
    /// being taken over again — the wedge `clear_the_abandoned` exists to keep
    /// a machine out of, arriving through the thing that fixed it. So one as
    /// stale as the lock it guards is cleared, and the next attempt gets it.
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
            lost_means: "Perch is finishing what it started.",
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
