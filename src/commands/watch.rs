//! `perch watcher run` and `perch watcher check` — the round, taken.
//!
//! What a round *decides* is [`crate::watch`]'s. One policy in three arrangements
//! (ADR the-machine-runs-the-watcher): typed at a terminal, run by the machine's own
//! service manager, or one round for a scheduler — and the difference between them is
//! [`Watcher`] and nothing else.
//!
//! Nothing a round takes is held across the wait — not the registry lock, not Claude
//! Code's locks — which is what makes a stop safe. The exception is the watcher lock,
//! held for the whole process and renewed as it goes.

use std::io::Write;

use chrono::{DateTime, Utc};

use crate::adopt;
use crate::commands::say;
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::{Host, Waited};
use crate::live;
use crate::lock::{self, Lost};
use crate::observe;
use crate::probe;
use crate::registry::{self, Registry};
use crate::round::{self, Verdict, Watching};
use crate::switch::{self, NotSwitched, Resolved};
use crate::watch::{
    self, Backoff, Cooled, Holding, Outcome, Recently, Speak, Watcher, nothing_was_switched,
};

/// One round, for whatever scheduled it.
///
/// The same policy and the same decision line as the loop; what differs is that the
/// line goes to standard output for cron to capture and what was decided goes into the
/// exit code (ADR a-watcher-knob-is-arithmetic).
pub fn check(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    host.listen_for_interrupts();

    // The same lock a loop takes: a Check firing while a Service runs is the double-
    // Switch it exists for. Held rather than refused, and `20` tells a scheduler to
    // come back.
    let watching_alone = match lock::take_all(host, vec![holdings::watcher_lock_spec(host)?]) {
        Ok(held) => held,
        Err(PerchError::Busy(why)) => {
            say(out, &watch::held_line(&why, None, host.now()))?;
            return Ok(crate::error::EXIT_HELD);
        }
        Err(other) => return Err(other),
    };
    let mut watching_alone = Watch::taken(host, watching_alone);

    // Nothing carried in: the cooldown comes off the registry inside the round, and a
    // back-off would pace a loop this process does not have.
    let verdict = match one_round(
        host,
        Watcher::Check,
        &mut Backoff::none(),
        &mut watching_alone,
    ) {
        Ok(verdict) => verdict,
        // Another `perch` holding the registry is ordinary, so it is a held round
        // rather than a raise, which would reach a cron mailbox by way of standard
        // error.
        Err(PerchError::Busy(why)) => {
            say(out, &watch::held_line(&why, None, host.now()))?;
            return Ok(crate::error::EXIT_HELD);
        }
        Err(other) => return Err(other),
    };
    // A Check exits on a machine that is not arranged for watching, where the loop
    // holds: a scheduler has to be told.
    let round = match verdict {
        Verdict::Decided(round) => round,
        Verdict::NotArranged(why) => return Err(why),
        // Said and exited rather than raised: a Check that was interrupted read no
        // figure, and `20` is what tells a scheduler to come back.
        Verdict::Lost(lost) => {
            say(out, &watch::stopped_line(lost, host.now()))?;
            return Ok(crate::error::EXIT_HELD);
        }
    };
    say(out, &round.line(host.now()))?;
    Ok(round.outcome.exit_code())
}

/// The loop, for the person who typed it or the Service running it for them.
pub fn keep_watching(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    // Before anything else, so that a Ctrl-C — or the `SIGTERM` a service manager stops
    // this with — is a request to finish rather than a process killed in the middle of
    // a Switch.
    host.listen_for_interrupts();

    // The two things carried from one round to the next, both in memory and nowhere
    // else: what the loop is waiting out and what it has already said belong to the
    // loop. What paces a Switch does not — it is read off the registry each round.
    let mut backoff = Backoff::none();
    let mut holding = Holding::nothing();

    // Exactly one Watcher per person per machine. Kept by name rather than dropped into
    // a `_`, because a hold nothing renews is one the next Check clears.
    let Some(watching_alone) = take_the_watch(host, out, &mut holding)? else {
        return stopped(out);
    };
    let mut watching_alone = Watch::taken(host, watching_alone);

    say(out, &opening(host)?)?;

    loop {
        // Twice a round, and this is the half that bounds the gap: the window is the
        // longest wait plus a round, and a round is bounded by nothing but the network.
        if let Err(lost) = watching_alone.goes_on() {
            return left(out, lost);
        }

        let (waiting_for, spoken) =
            match one_round(host, Watcher::Loop, &mut backoff, &mut watching_alone) {
                Ok(Verdict::Decided(round)) => {
                    let waiting_for = round.waiting_for();
                    let line = round.line(host.now());
                    match round.held_because() {
                        // The wait this round announced is what the coalescing compares, so
                        // a back-off that has doubled is said again.
                        Some(why) => (waiting_for, Spoken::held(why, Some(waiting_for), line)),
                        None => (waiting_for, Spoken::Decided(line)),
                    }
                }
                // The machine is not arranged for watching, which the loop holds on.
                // Nothing is charged to the back-off: this round asked the registry rather
                // than Anthropic.
                Ok(Verdict::NotArranged(why)) => held_before_a_round(&why.to_string(), host.now()),
                // Out at once rather than round the loop to the ask at the top: this round
                // asked it already, and the answer to a sticky question does not change.
                Ok(Verdict::Lost(lost)) => return left(out, lost),
                // Held like any other round that could not read. Ending the watcher over a
                // contended registry would let a `perch status --refresh` stop it silently.
                Err(PerchError::Busy(why)) => held_before_a_round(&why, host.now()),
                Err(other) => return Err(other),
            };

        say_it(out, &mut holding, spoken, host.now())?;

        // The other half, here because the round's own work is over: everything above
        // may have waited on Claude Code's locks or on a keychain that stopped to ask.
        if let Err(lost) = watching_alone.goes_on() {
            return left(out, lost);
        }

        // The one place the loop holds nothing it took this round, and therefore How
        // long is the round's to say: read off it rather than worked out again here, so
        // the wait the line promised and the wait taken cannot differ.
        if host.wait(waiting_for) == Waited::Interrupted {
            break;
        }
    }

    stopped(out)
}

/// The watch this process holds, as one question rather than three.
///
/// `renew`, `still_held` and `asked_to_stop` were asked apart at nine points and five
/// reviews found a long step running between two of them. One call asks all three, and
/// the long steps take this rather than a bare renewal (ADR an-invariant-gets-a-door).
struct Watch<'a> {
    host: &'a dyn Host,
    held: lock::Held<'a>,
}

impl<'a> Watch<'a> {
    fn taken(host: &'a dyn Host, held: lock::Held<'a>) -> Watch<'a> {
        Watch { host, held }
    }

    /// Renews the hold and says whether this Watcher is still the one to act.
    ///
    /// Called for its answer wherever a round is about to spend something or change
    /// something, and called for the renewal everywhere else — the same call, so the
    /// two cannot come apart.
    fn goes_on(&mut self) -> std::result::Result<(), Lost> {
        self.held.renew();
        if !self.held.still_held() {
            return Err(Lost::HandedOver);
        }
        if self.host.asked_to_stop() {
            return Err(Lost::Stopped);
        }
        Ok(())
    }

    /// The renewal without the question, for the one place the answer changes
    /// nothing: the Credential has already moved, and there is no step left to
    /// stop before.
    fn kept_up(&mut self) {
        self.held.renew();
    }
}

/// What the loop says on the way out when something other than the person at the
/// terminal ended it.
fn left(out: &mut dyn Write, lost: Lost) -> Result<()> {
    match lost {
        Lost::HandedOver => handed_over(out),
        Lost::Stopped => stopped(out),
    }
}

/// What the loop says on the way out, which is that it is out.
///
/// One word, because everything else that is true of a stop is true of every stop
/// without exception (ADR perch-says-what-it-did).
fn stopped(out: &mut dyn Write) -> Result<()> {
    say(out, "Stopped.")
}

/// What the loop says when it leaves because it is no longer the Watcher.
///
/// Not [`stopped`]: this is a stop nobody asked for, and a refusal says why. The lock
/// is not given back — it is somebody else's now.
fn handed_over(out: &mut dyn Write) -> Result<()> {
    say(
        out,
        "Stopped: another Watcher has taken the watch over, so this one is no \
         longer the only one deciding. Its lock is left where it is, no file of \
         its own was written, and the Account you are on is the one it last \
         Switched to.",
    )
}

/// Becomes the only Watcher on this machine, holding until whoever has the watch gives
/// it back — `None` where it was asked to stop while it was waiting.
///
/// Holding rather than exiting is what lets the watcher lock have a staleness window
/// measured in tens of minutes.
fn take_the_watch<'a>(
    host: &'a dyn Host,
    out: &mut dyn Write,
    holding: &mut Holding,
) -> Result<Option<crate::lock::Held<'a>>> {
    loop {
        match crate::lock::take_all(host, vec![holdings::watcher_lock_spec(host)?]) {
            Ok(held) => return Ok(Some(held)),
            Err(PerchError::Busy(why)) => {
                let (waiting_for, spoken) = held_before_a_round(&why, host.now());
                say_it(out, holding, spoken, host.now())?;
                if host.wait(waiting_for) == Waited::Interrupted {
                    return Ok(None);
                }
            }
            Err(other) => return Err(other),
        }
    }
}

/// A hold that happened before there was a [`Round`] to hold, and so one with no
/// Account to name; [`one_round`]'s `held` is the other shape. At the ordinary
/// interval, because nothing here spent a request: a contended registry, a lock
/// inside its window and a machine nothing arranged are each an answer.
fn held_before_a_round(why: &str, now: DateTime<Utc>) -> (u64, Spoken) {
    let waiting_for = watch::REFRESH_INTERVAL_MILLIS;
    let line = watch::held_line(why, Some(waiting_for), now);
    (waiting_for, Spoken::held(why, Some(waiting_for), line))
}

/// A round's line, and whether it was a hold — which is the only thing the coalescing
/// needs to know about it.
enum Spoken {
    /// Held, by `why`, coming back in `retrying_in`, and this is the line that says
    /// both in full. The two travel together because a hold that has changed either of
    /// them is one the log has to say again.
    Held {
        why: String,
        retrying_in: Option<u64>,
        in_full: String,
    },
    /// Decided something, and this is the line.
    Decided(String),
}

impl Spoken {
    fn held(why: &str, retrying_in: Option<u64>, in_full: String) -> Spoken {
        Spoken::Held {
            why: why.to_string(),
            retrying_in,
            in_full,
        }
    }
}

/// Says a round, as much of it as is worth saying.
///
/// An unchanged hold is said in full when it starts, as a duration once an hour while
/// it lasts, and as what it cost when it ends. A round that decided something is always
/// said.
fn say_it(
    out: &mut dyn Write,
    holding: &mut Holding,
    spoken: Spoken,
    now: DateTime<Utc>,
) -> Result<()> {
    match spoken {
        Spoken::Held {
            why,
            retrying_in,
            in_full,
        } => match holding.holding(&why, retrying_in, now) {
            Speak::InFull => say(out, &in_full),
            Speak::StillHolding { since } => say(out, &watch::still_holding_line(since, now)),
            Speak::Nothing => Ok(()),
        },
        Spoken::Decided(line) => {
            // The way out of a hold is always said, and said before the decision, so a
            // log reads in the order the things happened.
            if let Some(held_for) = holding.released(now) {
                say(out, &watch::released_line(held_for, now))?;
            }
            say(out, &line)
        }
    }
}

/// What the loop is about to start doing, said before it does it.
///
/// The only place the threshold, the interval, the ceiling and the cooldown are said,
/// because no round re-derives them. A machine that is not arranged for watching is not
/// refused here: the loop starts anyway and holds.
fn opening(host: &dyn Host) -> Result<String> {
    // Read rather than insisted on, for the reason the round beneath it holds rather
    // than exits: raising here would end the loop before the first round could hold on
    // it.
    let watching = adopt::ensure_adopted(host).ok().and_then(|registry| {
        // The one reader that asks whether a Landing is in flight rather than settling
        // one: this holds no lock, and a Switch left in flight is exactly the state
        // where there is nothing to say yet.
        let watching = registry::nothing_in_flight(&registry)
            .and_then(|settled| round::permitted(&registry, &settled).ok())?;
        Some((
            registry.named_for_the_user(watching.account.email()),
            watching,
        ))
    });
    let Some((named, watching)) = watching else {
        return Ok(
            "Started. Nothing is being decided yet — the next line says what is \
             holding it, and the watcher takes over the moment that changes. \
             Ctrl-C stops."
                .to_string(),
        );
    };
    Ok(format!(
        "Watching {} {}. Reading how full it is every {}, and Switching \
         within that Scope when its fullest Quota Window reaches {}% — to an \
         Account at {}% or under, and never twice inside {} minutes. Ctrl-C \
         stops.",
        named,
        watching.scope.within(),
        watch::how_often(),
        watching.policy.threshold,
        watching.policy.ceiling(),
        watch::COOLDOWN_MINUTES,
    ))
}

/// One round: read, decide, and act if acting is what was decided.
///
/// The registry lock is taken here and given back when this returns rather than held
/// for the life of the loop, which would shut every other `perch` out of the machine
/// for as long as the loop ran.
fn one_round(
    host: &dyn Host,
    watcher: Watcher,
    backoff: &mut Backoff,
    watching_alone: &mut Watch<'_>,
) -> Result<Verdict> {
    // A machine with no Claude Code login has nothing to adopt. `Busy` is passed
    // through untouched, because both callers answer it differently from "not
    // arranged".
    let (mut perch, mut registry) = match adopt::ensure_adopted_exclusively(host) {
        Ok(both) => both,
        Err(busy @ PerchError::Busy(_)) => return Err(busy),
        Err(not_arranged) => return Ok(Verdict::NotArranged(not_arranged)),
    };

    // A Switch path, so it resolves a Landing first. Where it refuses, nobody is there
    // to answer, so it travels as the same "not arranged for watching".
    let settled = match switch::resolve_a_landing(host, &mut perch, &mut registry, &mut || {
        watching_alone.goes_on()
    }) {
        Ok(Resolved::Settled(settled)) => settled,
        // Before a figure or a policy has been reached, so there is no Round to report
        // this as and the loop takes it as the stop it is.
        Ok(Resolved::Stopped(lost)) => return Ok(Verdict::Lost(lost)),
        // `Busy` passed through untouched, as at the adoption lock above. It arrives
        // before anything has been read, so there is no figure and nothing was decided,
        // which is what a hold is.
        Err(busy @ PerchError::Busy(_)) => return Err(busy),
        Err(unsettled) => return Ok(Verdict::NotArranged(unsettled)),
    };

    // Handed back rather than raised, and as the failure itself rather than as its
    // sentence, so a Check still exits `18` for an ungrouped Account and `14` for a
    // Scope.
    let watching = match round::permitted(&registry, &settled) {
        Ok(watching) => watching,
        Err(not_arranged) => return Ok(Verdict::NotArranged(not_arranged)),
    };
    let email = watching.account.email().to_string();

    // Read under the lock, so the cooldown a round is held by is the one that was on
    // record when it decided — and read every round rather than carried, because a
    // Watcher this Service restarts would otherwise come back owing nobody a wait.
    let recently = Recently::recorded(registry.checked(watching.scope.word()), host.now());

    // Once per round, and handed to everything in it that wants one. Deferred, so a
    // round that refuses nothing forks nothing: this loop runs until the session ends,
    // and every round asking a Node program its version is a cost with no reader.
    let installed = probe::Installed::asked_when_needed(host);

    // The one Account Refreshed, and nearly all of the network this loop spends.
    // Renewed either side of it, as the loop renews either side of the wait: up to six
    // requests at thirty seconds each go out under this call alone.
    let report = observe::refresh(
        host,
        &mut perch,
        &mut registry,
        std::slice::from_ref(&email),
        &installed,
        observe::Spending::ItsOwn,
        &mut || watching_alone.goes_on(),
    );
    // Worth saying and not worth holding a decision over: the figure this round decides
    // on is the one that was just read, and the next round reads its own.
    if let Some(not_kept) = &report.not_kept {
        host.note(not_kept);
    }

    // The Account the Refresh just wrote to, taken out before the closure below needs
    // the registry back: what a figure is read off is this, and `watching.account` is
    // the copy from before the read.
    let account = registry
        .account(&email)
        .expect("the Account just refreshed is one Perch holds")
        .clone();

    let decided = round::decide(
        round::Reading {
            account: &account,
            report: &report,
            policy: &watching.policy,
            recently: &recently,
            now: host.now(),
        },
        watcher,
        backoff,
        // Reached only through a `Cooled`, which is the whole of what the decision
        // above is for: the one irreversible thing a round does is behind it.
        |cooled| {
            act(
                host,
                &mut perch,
                &mut registry,
                &watching,
                watcher,
                cooled,
                &installed,
                watching_alone,
            )
        },
    )?;
    Ok(Verdict::Decided(decided))
}

/// The Account is full enough to move off, so this is the whole of what the watcher
/// does about it: read the candidates, choose, and Switch.
///
/// Read here rather than kept warm — the only moment their figures are worth
/// anything, and the moment they are cheapest to get.
// Seven, and the seventh is the point: `probed` is what the round already asked the
// machine and must not ask again.
#[allow(clippy::too_many_arguments)]
fn act(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    watching: &Watching,
    watcher: Watcher,
    cooled: &Cooled<'_>,
    probed: &Result<probe::Installed>,
    watching_alone: &mut Watch<'_>,
) -> Result<Outcome> {
    let scope = watching.scope.clone();
    let outgoing = watching.account.clone();

    let installed = match probed.as_ref() {
        Ok(installed) => installed,
        // Not reachable: a round reaches this only on a figure it read, and a figure
        // is read only where the probe answered. Handed on rather than asserted,
        // because what runs this is a Service nobody is watching.
        Err(why) => return Err(PerchError::Other(why.to_string())),
    };

    // Asked before the candidates are read: the burst spends an hourly allowance that
    // does not refill early, one read per candidate, and a `perch run` held open in
    // another terminal would spend it every round.
    let places = [live::Place::of_the_profile(host, &outgoing)?];
    let idle = match live::ask(host, &places) {
        live::Answer::Idle(idle) => idle,
        live::Answer::NotIdle(not_idle) => {
            return watch::refused_or_raised(not_idle, installed);
        }
    };

    // The set a Refresh cannot move, walked once: what changes under one is the
    // figures, and those come back out of `refreshed` below.
    let candidates = round::Candidates::of(registry, watching, cooled, &idle);

    // The burst, and the longest thing a round does: one read per candidate, each
    // bounded only at thirty seconds, over as many as the Scope holds. It takes the
    // watch rather than a renewal, so it ends where the watch does.
    let read = observe::refresh(
        host,
        perch,
        registry,
        &candidates.addresses(),
        probed,
        observe::Spending::ItsOwn,
        &mut || watching_alone.goes_on(),
    );
    // Before the choice rather than after it: a burst that stopped part way leaves
    // every candidate past that point on whatever figure was cached, and choosing on
    // those is the Switch this round is no longer the one to make.
    if let Some(lost) = read.stopped {
        return Ok(nothing_was_switched(lost));
    }

    // What could not be read, carried into the sentence that says where the watcher
    // went: an Account ranked on a figure from an hour ago is the one thing that can
    // make this Switch land somewhere worse than it left.
    let unread = read.notes();

    // The margin, applied to the figures the burst above has just written.
    let set_aside = watch::set_aside(
        &watching.policy,
        &watching.scope,
        &candidates.refreshed(registry),
    );

    let choice = match cycle::choose(
        registry,
        &scope,
        Some(outgoing.email()),
        &set_aside,
        host.now(),
    ) {
        Ok(choice) => choice,
        // Nowhere worth going is an answer rather than a failure, and both ways of
        // getting there are resolved by waiting.
        Err(error @ (PerchError::NoCandidate(_) | PerchError::NothingToDo(_))) => {
            return Ok(Outcome::Nowhere {
                why: also(error.to_string(), &unread),
                looking_again: watcher.asking_again(watch::NOWHERE_INTERVAL_MILLIS),
            });
        }
        Err(error) => return Err(error),
    };

    // The last thing asked before the one irreversible thing a round does. The burst
    // above is bounded by nothing but the network, so it can outlast the watch — and a
    // Switch made after that is the second Watcher deciding beside the first.
    if let Err(lost) = watching_alone.goes_on() {
        return Ok(nothing_was_switched(lost));
    }
    // One instant for both arrangements, and the beginning rather than the end, because
    // that is the one already written down.
    let acted_at = host.now();
    let switched = switch::switch_to(
        host,
        perch,
        registry,
        installed,
        &choice.account,
        Some(&outgoing),
        switch::Reason::Unasked {
            scope: scope.clone(),
            at: acted_at,
        },
    );
    watching_alone.kept_up();

    match switched {
        // Where it went, and nothing about why it won: nobody is at the terminal to be
        // owed a reason. Named as the person named it.
        Ok(_switched) => Ok(Outcome::Switched {
            to: registry.named_for_the_user(choice.account.email()),
            unread,
        }),
        // The machine is part way through a Switch, so it is answered before the
        // refusals below whatever the failure was.
        Err(NotSwitched { error, moved: true }) => Err(error),
        // Nothing was changed, and each of these clears itself. A locked keychain, a
        // probe that cannot find Claude Code, a Profile that will not be written: none
        // of those do, and they keep the code the failure earned.
        Err(NotSwitched {
            error:
                error @ (PerchError::Quarantined { .. }
                | PerchError::ProfileLive(_)
                | PerchError::Busy(_)),
            ..
        }) => Ok(Outcome::Refused {
            // A lock is the one of the three a scheduler should come straight back
            // for, and the round is read as `refused` either way: it decided on a
            // figure it had read, which is what tells it from a hold.
            contended: matches!(error, PerchError::Busy(_)),
            why: error.to_string(),
            // Every candidate was read to get here.
            after_reading: true,
        }),
        Err(NotSwitched { error, .. }) => Err(error),
    }
}

/// A sentence, with whatever else has to be said on the same line after it.
fn also(said: String, notes: &[String]) -> String {
    match notes.is_empty() {
        true => said,
        false => format!("{said} {}", notes.join(" ")),
    }
}
