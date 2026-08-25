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
use crate::host::{Host, Waited};
use crate::lock;
use crate::observe::{self, Attempt};
use crate::probe;
use crate::registry::{self, Account, Registry, Scope, UNGROUPED};
use crate::switch::{self, Idle, NotSwitched, Resolved, Settled};
use crate::watch::{
    self, Backoff, Considered, Cooled, Fullest, Holding, Lost, Outcome, Policy, Recently, Round,
    Speak, nothing_was_switched,
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
    let watching_alone = match lock::take_all(host, vec![registry::watcher_lock_spec(host)?]) {
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
        &mut Recently::nothing(),
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

    // The three things carried from one round to the next, all in memory and nowhere
    // else: what paces this loop, and what it has already said, belong to the loop
    // rather than to the machine.
    let mut recently = Recently::nothing();
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

        let (waiting_for, spoken) = match one_round(
            host,
            Watcher::Loop,
            &mut recently,
            &mut backoff,
            &mut watching_alone,
        ) {
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
        match crate::lock::take_all(host, vec![registry::watcher_lock_spec(host)?]) {
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
        let watching = switch::nothing_in_flight(&registry)
            .and_then(|settled| permitted(&registry, &settled).ok())?;
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

/// Which watcher a round belongs to.
///
/// One difference, and every part of it is here rather than scattered through the
/// round: where the cooldown is kept, and who decides when the next reading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watcher {
    /// `perch watcher run`: rounds separated by a wait this process takes, so what
    /// paces it lives in memory and dies with it.
    Loop,
    /// `perch watcher check`: one round and out, so what paces it is written to the
    /// registry for the next invocation to read, and how long until that one is
    /// whatever scheduled them.
    Check,
}

impl Watcher {
    /// How long before the watcher reads again, where that is the watcher's to say.
    ///
    /// Given the wait rather than the [`Backoff`] it came off, so the only way to have
    /// one is to have just been charged for it.
    fn asking_again(self, waiting_for: u64) -> Option<u64> {
        match self {
            Watcher::Loop => Some(waiting_for),
            Watcher::Check => None,
        }
    }

    /// What paces this round, put where the round will read it.
    ///
    /// A check takes the Group's record, read under the lock so the cooldown a round is
    /// held by is the one that was on record when it decided. The loop's is already in
    /// the caller's hands.
    fn pacing(
        self,
        carried: &mut Recently,
        registry: &Registry,
        scope: &Scope,
        now: DateTime<Utc>,
    ) {
        match self {
            Watcher::Loop => {}
            Watcher::Check => {
                *carried = Recently::recorded(registry.checked(scope.word()), now);
            }
        }
    }

    /// Which Switch this watcher is about to make, in the terms the Switch itself keeps
    /// ([`switch::Reason`]).
    ///
    /// Handed to the Switch rather than sequenced around it: what paces the next check
    /// has to reach the registry in the same save as the Switch it paces.
    fn reason(self, scope: &Scope, at: DateTime<Utc>) -> switch::Reason {
        match self {
            Watcher::Loop => switch::Reason::Loop,
            Watcher::Check => switch::Reason::Check {
                scope: scope.clone(),
                at,
            },
        }
    }
}

/// The Account being watched, the Scope that said it may be, and the rules it is
/// watched under.
struct Watching {
    account: Account,
    /// The Scope a Switch would be taken within — a Group, or the Accounts in no Group.
    /// Written as a Scope rather than a Group name so that serving the second is real
    /// work rather than a fallthrough: this is a path that can Switch somebody's
    /// Account without being asked.
    scope: Scope,
    policy: Policy,
}

/// Whether there is anything here for a watcher to do, and what.
///
/// Asked every round rather than only at the start, because the answer can stop being
/// yes underneath it, and every failure it gives is the same "not arranged yet". The
/// [`Settled`] is why it cannot be asked too early (ADR an-ordering-is-a-type).
fn permitted(registry: &Registry, _settled: &Settled) -> Result<Watching> {
    let account = registry.active_account().cloned().ok_or_else(|| {
        PerchError::NotFound(
            "Perch holds no active Account, so there is nothing to watch. \
             `perch switch <target>` makes one active."
                .to_string(),
        )
    })?;

    let scope = registry.scope_of(&account);

    // Two independent statements before anything moves unasked, and a Group is the
    // first (ADR a-group-is-a-declaration). In this order, because the declaration
    // comes first.
    if !cycle::may_cycle_within(registry, &scope) {
        return Err(PerchError::NotInterchangeable(format!(
            "{} is in no Group, and nothing has said the Accounts in no Group \
             are interchangeable at all — so there is nowhere for the watcher \
             to Switch it to. Nothing is being watched.\n\
             `perch config set {UNGROUPED} interchangeable true` says they are, \
             and `perch config set {UNGROUPED} watcher-may-act true` then says \
             the watcher may act on them. Both, because being interchangeable \
             is a declaration somebody makes and letting the watcher act is a \
             grant, and neither implies the other.\n\
             Putting it in a Group with `perch group move {} <group>` is the \
             narrower statement, and is what Groups are for.",
            registry.named_for_the_user(account.email()),
            account.email(),
        )));
    }

    // The Scope's own grant, and nowhere else it could come from
    // (ADR a-setting-names-its-scope).
    let settings = registry.settings(&scope);
    if !settings.watcher_may_act {
        return Err(PerchError::Invalid(format!(
            "{} has not been told the watcher may act on it, so nothing is \
             being watched. Anything that changes underneath you only ever \
             does so because you said it could.\n\
             `perch config set {} watcher-may-act true` says it may.",
            scope.described(),
            scope.word(),
        )));
    }

    Ok(Watching {
        account,
        scope,
        policy: Policy::of(&settings),
    })
}

/// One round: read, decide, and act if acting is what was decided.
///
/// The registry lock is taken here and given back when this returns rather than held
/// for the life of the loop, which would shut every other `perch` out of the machine
/// for as long as the loop ran.
fn one_round(
    host: &dyn Host,
    watcher: Watcher,
    recently: &mut Recently,
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
    let watching = match permitted(&registry, &settled) {
        Ok(watching) => watching,
        Err(not_arranged) => return Ok(Verdict::NotArranged(not_arranged)),
    };
    let email = watching.account.email().to_string();

    watcher.pacing(recently, &registry, &watching.scope, host.now());

    // Once per round, and handed to everything in it that wants one: probed where it is
    // wanted, an acting round walks `PATH` and spawns `claude --version` three times.
    let installed = probe::Installed::probed(host);

    // The one Account Refreshed, and nearly all of the network this loop spends.
    // Renewed either side of it, as the loop renews either side of the wait: up to six
    // requests at thirty seconds each go out under this call alone.
    let report = observe::refresh(
        host,
        &mut perch,
        &mut registry,
        std::slice::from_ref(&email),
        &installed,
        &mut || watching_alone.goes_on(),
    );
    // Worth saying and not worth holding a decision over: the figure this round decides
    // on is the one that was just read, and the next round reads its own.
    if let Some(not_kept) = &report.not_kept {
        host.note(not_kept);
    }

    // Before the reading is judged, because a round that stopped read nothing and a
    // hold charged for it would pace the Back-off off a question nobody was asked.
    if let Some(lost) = report.stopped {
        return Ok(Verdict::Decided(Round {
            fullest: None,
            threshold: watching.policy.threshold,
            outcome: nothing_was_switched(lost),
        }));
    }

    // A hold said against the wait it earned. `waiting_for` is passed in rather than
    // charged here, because not every hold is a question nobody answered.
    let held = |why: String, waiting_for: u64| {
        Ok(Verdict::Decided(Round {
            fullest: None,
            threshold: watching.policy.threshold,
            outcome: Outcome::Held {
                why,
                retrying_in: watcher.asking_again(waiting_for),
            },
        }))
    };

    // Never on a figure it did not just read.
    if let Some(refused) = refused_the_reading(&report.attempts) {
        let waiting_for = match refused.paced {
            true => backoff.could_not_read(),
            false => watch::REFRESH_INTERVAL_MILLIS,
        };
        return held(refused.why, waiting_for);
    }
    let account = registry
        .account(&email)
        .expect("the Account just refreshed is one Perch holds");
    let Some(fullest) = Fullest::of(account) else {
        // Read, and carrying no Quota Window Perch could make anything of. Unreachable,
        // and answered anyway: acting on an Account whose fullness is unknown is the
        // one thing this round may never do.
        return held(
            "Anthropic answered without a Quota Window Perch could read, so \
             there was no figure to decide on."
                .to_string(),
            backoff.could_not_read(),
        );
    };
    // A figure, so whatever was wrong is over. Said here rather than at the end of the
    // round: a round that finds nowhere to go has read perfectly well.
    backoff.read();

    // Two asks that hand on what they earned: [`act`] is reachable only through a
    // [`Cooled`], and a `Cooled` only through a [`Crossed`]. The figure comes back out
    // of each.
    let (fullest, outcome) = match fullest.crossed(watching.policy.threshold) {
        Err(under) => (under, Outcome::Waiting),
        Ok(crossed) => match crossed.cooled(recently, host.now()) {
            // Before the candidates are read, so a round that may not act spends
            // nothing finding out where it would have gone.
            Err(cooling) => (
                crossed.fullest().clone(),
                Outcome::Cooling { why: cooling.why },
            ),
            Ok(cooled) => {
                let fullest = cooled.fullest().clone();
                let outcome = act(
                    host,
                    &mut perch,
                    &mut registry,
                    &watching,
                    watcher,
                    recently,
                    &cooled,
                    &installed,
                    watching_alone,
                )?;
                (fullest, outcome)
            }
        },
    };
    Ok(Verdict::Decided(Round {
        fullest: Some(fullest),
        threshold: watching.policy.threshold,
        outcome,
    }))
}

/// What a round came to, before anybody has decided what to do about it.
///
/// A round that *decided* something is the same for both watchers; a machine that is
/// *not arranged for watching* is not, because the loop holds on it and a Check exits
/// on it. Not a *turn*, which is one Account's pass through the Refresh path.
enum Verdict {
    /// The round read, and decided.
    Decided(Round),
    /// The watch was lost before the round read anything: settling the Landing walks
    /// the Credential Store of every Account Perch holds, and this Watcher stopped
    /// being the one to act part way through. No figure, no threshold and nothing
    /// decided — which is why it is not a [`Verdict::Decided`] carrying an
    /// [`Outcome::Stopped`].
    Lost(Lost),
    /// There was nothing here the watcher may act on, and this says why — something the
    /// machine has not been arranged for, or a Switch left in flight that nothing here
    /// can settle.
    NotArranged(PerchError),
}

/// Why the reading cannot be acted on, or `None` when it can.
///
/// `None` is the one answer that lets a Switch happen, so an empty report is answered
/// as a hold rather than as an absence of objections. None of the reasons names the
/// Account: the decision line has.
fn refused_the_reading(attempts: &[Attempt]) -> Option<Refusal> {
    let Some(attempt) = attempts.first() else {
        return Some(Refusal::paced(
            "nothing was read at all, so there was no current figure to decide \
             on."
            .to_string(),
        ));
    };
    match &attempt.outcome {
        observe::Outcome::Observed => None,
        observe::Outcome::Throttled => Some(Refusal::paced(
            "Anthropic is rate-limiting reads of this Account's usage, so \
             nothing current could be read."
                .to_string(),
        )),
        // Paced only where the round asked Anthropic something: a Renewal refused
        // because a client is holding the Profile, or an Account sharing one, sent
        // nothing, and a Back-off paces questions nobody is answering.
        observe::Outcome::Failed { why, spent } => Some(match spent {
            true => Refusal::paced(why.clone()),
            false => Refusal::unpaced(why.clone()),
        }),
        // An Account already Quarantined is not asked at all (`observe` returns
        // before the first request), so this round spent nothing and a Back-off
        // paces questions nobody is answering.
        observe::Outcome::Quarantined { why, .. } => Some(Refusal::unpaced(format!(
            "{}. {}",
            why.because(),
            crate::registry::how_to_repair(&attempt.email),
        ))),
        // The round stopped rather than being refused, and `refresh` reports that
        // through `Report::stopped` rather than as an attempt against an Account.
        observe::Outcome::Stopped(_) => None,
    }
}

/// Why a round had no figure to decide on, and whether the Back-off paces it.
///
/// The two travel together because a hold reported against a wait it did not earn
/// is the failure either half alone allows: a repair made a minute after the
/// Quarantine would otherwise wait out a doubling nothing spent.
struct Refusal {
    why: String,
    paced: bool,
}

impl Refusal {
    fn paced(why: String) -> Refusal {
        Refusal { why, paced: true }
    }

    fn unpaced(why: String) -> Refusal {
        Refusal { why, paced: false }
    }
}

/// The Account is full enough to move off, so this is the whole of what the watcher
/// does about it: read the candidates, choose, and Switch.
///
/// Read here rather than kept warm — the only moment their figures are worth
/// anything, and the moment they are cheapest to get.
// Eight, and the eighth is the point: `probed` is what the round already asked the
// machine and must not ask again.
#[allow(clippy::too_many_arguments)]
fn act(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    watching: &Watching,
    watcher: Watcher,
    recently: &mut Recently,
    cooled: &Cooled<'_>,
    probed: &Result<probe::Installed>,
    watching_alone: &mut Watch<'_>,
) -> Result<Outcome> {
    let scope = watching.scope.clone();
    let outgoing = watching.account.clone();

    // Asked before the candidates are read: the burst spends an hourly allowance that
    // does not refill early, one read per candidate, and a `perch run` held open in
    // another terminal would spend it every round.
    let installed = probed
        .as_ref()
        .map_err(|why| PerchError::Other(why.to_string()))?
        .clone();
    let idle = match switch::refuse_if_live(host, &outgoing, &installed) {
        Ok(idle) => idle,
        Err(not_idle) => return watch::refused_or_raised(not_idle),
    };

    // The burst, and the longest thing a round does: one read per candidate, each
    // bounded only at thirty seconds, over as many as the Scope holds. It takes the
    // watch rather than a renewal, so it ends where the watch does.
    let read = observe::refresh(
        host,
        perch,
        registry,
        &addresses_of(&considered(registry, watching, cooled, &idle)),
        probed,
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

    // The margin, applied to the figures as this round has them.
    let set_aside = watch::set_aside(
        &watching.policy,
        &watching.scope,
        &considered(registry, watching, cooled, &idle),
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
        &installed,
        &choice.account,
        Some(&outgoing),
        watcher.reason(&scope, acted_at),
    );
    watching_alone.kept_up();

    // Only a Switch that happened starts a cooldown, and a Switch that moved and then
    // *failed* is one — which is why the question is asked of both ways out rather than
    // of the successful one.
    let moved = match &switched {
        Ok(switched) => switched.moved,
        Err(not_switched) => not_switched.moved,
    };
    if moved {
        recently.switched(acted_at);
    }

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

/// The Accounts a Switch could land on, with the figures this round has of them.
///
/// Called twice a round, through the same walk both times, and the one funnel that
/// produces candidate addresses — which is why it takes both witnesses and reads
/// neither.
fn considered(
    registry: &Registry,
    watching: &Watching,
    _cooled: &Cooled<'_>,
    _idle: &Idle,
) -> Vec<Considered> {
    watching
        .scope
        .accounts(registry)
        .iter()
        .filter(|account| {
            // Through the registry's own answer rather than `!=`, which would be
            // correct only by two facts that are true two modules away.
            !registry::same_name(account.email(), watching.account.email())
                && cycle::is_a_candidate(registry, account)
        })
        .map(|account| Considered {
            email: account.email().to_string(),
            named: registry.named_for_the_user(account.email()),
            fullest: Fullest::of(account),
        })
        .collect()
}

/// Their addresses, which is all a Refresh takes.
fn addresses_of(considered: &[Considered]) -> Vec<String> {
    considered
        .iter()
        .map(|candidate| candidate.email.clone())
        .collect()
}

/// A sentence, with whatever else has to be said on the same line after it.
fn also(said: String, notes: &[String]) -> String {
    match notes.is_empty() {
        true => said,
        false => format!("{said} {}", notes.join(" ")),
    }
}
