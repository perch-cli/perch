//! `perch watch` — the watcher that Cycles on your behalf when the Account you
//! are on runs low.
//!
//! A loop you can see and kill. What it decides and why is [`crate::watch`];
//! the round it takes to decide it is here.
//!
//! Three arrangements and one behaviour (ADR 0040). Typed at a terminal it is
//! this loop. Run by the machine's own service manager — a Service, which
//! [`crate::service`] installs — it is *the same loop*, supervised, which is the
//! whole of why there is no second policy to keep in agreement with this one.
//! `perch watch --once` is one round of it for a scheduler, with what was
//! decided in the exit code because nobody is reading a terminal; the difference
//! between that and the loop is [`Watcher`] and nothing else.
//!
//! Perch never backgrounds itself. Scheduling and supervision are the operating
//! system's job, which is ADR 0013's line and is not one ADR 0040 repealed —
//! what it repealed is the idea that Perch may not hand the operating system a
//! unit file to do it with.
//!
//! Two things follow from a loop nobody may be watching, and both are here. It
//! **holds rather than stops** when the machine is not arranged for watching, so
//! a supervisor is never handed a deliberate exit to respawn; and an unchanged
//! hold is **said once an hour rather than every round**, so a log nobody reads
//! until something is wrong is not five hundred identical lines a day deep by
//! the time they read it.
//!
//! One round is: Refresh the active Account, from Anthropic rather than from
//! cache; say what that came to against the Group's threshold; and where it is
//! at or over, rank the Group's other Accounts *now* and Switch to the best of
//! them. Why the candidates are read then rather than kept warm is
//! [`act`]'s to explain; how often the active one is read is
//! [`crate::watch::REFRESH_INTERVAL_MILLIS`]'s.
//!
//! Being over the threshold is not on its own a reason to move. Two constants
//! pace the rest of it (ADR 0046): a cooldown under how often a Switch may
//! happen at all, checked here before anything is read, and a margin under where
//! one may land, applied by setting candidates aside for the ranking in [`act`].
//! The one thing carried between rounds is a
//! [`Recently`](crate::watch::Recently), which is what the cooldown is measured
//! from — in memory for the loop, and on the registry for a check, which has no
//! memory of the one before it.
//!
//! What it does when it acts is a Switch, whole: the outgoing Credential is
//! Captured first (ADR 0006), Claude Code's locks are taken, and a Live
//! Profile's token is never Renewed (ADR 0005). Running while Claude Code is
//! working is the normal case rather than the exception.
//!
//! Nothing a *round* takes is held across the wait — not the registry lock, not
//! Claude Code's locks, not a session marker. That is what makes Ctrl-C safe:
//! the loop spends nearly all of its life in the one place where being killed
//! costs nothing, and the interrupt it takes over from the default handler
//! (Ctrl-C, and the `SIGTERM` a service manager sends) is only there so that a
//! stop arriving mid-Switch lets that Switch finish first.
//!
//! The one thing held for the whole of the process is the watcher lock, which
//! is what makes this the only Watcher on the machine (ADR 0040). It is the
//! single artifact a Watcher leaves behind, it is given back however the process
//! ends, and a second Watcher that finds it held says so and comes back rather
//! than exiting — because exiting is what a supervisor turns into a crash loop.
//!
//! A Ctrl-C typed at a terminal reaches the whole foreground process group, so
//! the `curl` or `security` a round happens to be waiting on dies with it. That
//! is not a special case here: a Refresh that could not be read is a held
//! decision and a Switch that never started is a refused one, so the round ends
//! by saying what happened and the loop then stops at the wait, exactly as it
//! would have anyway.

use std::io::Write;

use chrono::{DateTime, Utc};

use crate::adopt;
use crate::commands::say;
use crate::cycle::{self, Scope};
use crate::error::{EXIT_OK, PerchError, Result};
use crate::host::{Host, Waited};
use crate::lock;
use crate::observe::{self, Attempt};
use crate::probe;
use crate::registry::{self, Account, Registry, UNGROUPED};
use crate::switch;
use crate::watch::{
    self, Backoff, Considered, Fullest, Holding, Outcome, Policy, Recently, Round, Speak,
};

/// How the watcher was asked for: as the loop, or as one check.
#[derive(Debug, Default, Clone, Copy)]
pub struct WatchArgs {
    /// Take one round and exit, reporting what it decided in the exit code.
    pub once: bool,
}

pub fn run(host: &dyn Host, args: WatchArgs, out: &mut dyn Write) -> Result<i32> {
    match args.once {
        true => check(host, out),
        false => keep_watching(host, out).map(|()| EXIT_OK),
    }
}

/// One round, for whatever scheduled it (ADR 0013).
///
/// The same policy as the loop and the same decision line — the difference is
/// only in how the answer is delivered, because nobody is watching a terminal:
/// the line goes to standard output for cron to capture, and what was decided
/// goes into the exit code for a script to branch on.
///
/// The interrupt handler is taken over here for the same reason the loop takes
/// it: what a check does when it acts is a Switch, and a signal arriving in the
/// middle of one would leave the machine part way through it. There is no wait
/// to answer the interrupt at afterwards, because a check is already leaving.
fn check(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    host.listen_for_interrupts();

    // The same lock a loop takes, and refused the same way (ADR 0040). A Check
    // firing while a Service runs is the double-switch the lock exists for: the
    // loop keeps its Cooldown in memory and a Check reads the one in `checks`,
    // and neither can see the other's — so a Check inside a loop's cooldown
    // would move an Account the loop had just decided not to move.
    //
    // Held rather than refused outright, and `20` is already the code that says
    // so: a scheduler reading it comes back at the next Check, which is exactly
    // right for a lock somebody else is holding now.
    let _watching_alone = match lock::take_all(host, vec![registry::watcher_lock_spec(host)?]) {
        Ok(held) => held,
        Err(PerchError::Busy(why)) => {
            say(out, &watch::held_line(&why, None, host.now()))?;
            return Ok(crate::error::EXIT_HELD);
        }
        Err(other) => return Err(other),
    };

    // Nothing carried in from anywhere: the cooldown comes off the registry
    // inside the round, and a back-off would be pacing a loop this process does
    // not have. How soon to come back is the scheduler's.
    let turn = match one_round(
        host,
        Watcher::Check,
        &mut Recently::nothing(),
        &mut Backoff::none(),
    ) {
        Ok(turn) => turn,
        // The one outcome that reached a check without a line, and the one most
        // likely to recur: `perch status --refresh` holds the registry across
        // every Renewal and every read it makes, comfortably longer than
        // `lock::take` waits. Raised, it went to standard error by way of
        // `main`, so a cron job capturing standard output got a line for every
        // outcome except that one — while the doc above promises "the line goes
        // to standard output for cron to capture". The loop already treats this
        // as an ordinary held round; a check does now too, and exits `EXIT_HELD`
        // as it always did.
        //
        // No interval on it, because a check is exiting and when it comes back
        // is whatever scheduled it to say.
        Err(PerchError::Busy(why)) => {
            say(out, &watch::held_line(&why, None, host.now()))?;
            return Ok(crate::error::EXIT_HELD);
        }
        Err(other) => return Err(other),
    };
    // A Check keeps `14` and `18` exactly as ADR 0013 promised them. Only the
    // loop's exits were repealed, and only because a supervisor crash-loops on
    // one (ADR 0040): a Check is one process reporting to a scheduler, and a
    // scheduler has to be told that this machine is not arranged for what it
    // was asked to do.
    let round = match turn {
        Turn::Decided(round) => round,
        Turn::NotArranged(why) => return Err(why),
    };
    say(out, &round.line(host.now()))?;
    Ok(round.outcome.exit_code())
}

fn keep_watching(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    // Before anything else, so that a Ctrl-C — or the `SIGTERM` a service
    // manager stops this with — is a request to finish rather than a process
    // killed in the middle of a Switch.
    host.listen_for_interrupts();

    // The three things carried from one round to the next (ADR 0013, ADR 0040).
    // All in memory and nowhere else: what paces this loop, and what this loop
    // has already said, belong to the loop rather than to the machine.
    let mut recently = Recently::nothing();
    let mut backoff = Backoff::none();
    let mut holding = Holding::nothing();

    // Exactly one Watcher per person per machine. Taken before the opening line
    // rather than after it, so a second `perch watch` never claims to be
    // watching anything.
    let Some(_watching_alone) = take_the_watch(host, out, &mut backoff, &mut holding)? else {
        return stopped(out);
    };

    say(out, &opening(host)?)?;

    loop {
        let (waiting_for, spoken) =
            match one_round(host, Watcher::Loop, &mut recently, &mut backoff) {
                Ok(Turn::Decided(round)) => {
                    let waiting_for = round.waiting_for();
                    let line = round.line(host.now());
                    match round.held_because() {
                        // The wait this round announced *is* what it will do next,
                        // so it is what the coalescing compares: a back-off that has
                        // doubled has changed the line, and a changed line is said.
                        Some(why) => (waiting_for, Spoken::held(why, Some(waiting_for), line)),
                        None => (waiting_for, Spoken::Decided(line)),
                    }
                }
                // The machine is not arranged for watching: no active Account, an
                // ungrouped one nobody has declared interchangeable, or a Scope that
                // has not said the watcher may act. ADR 0013 stopped the loop on
                // these; ADR 0040 holds it instead, because a supervisor respawns a
                // deliberate exit until it gives up on the unit, and launchd cannot
                // be told otherwise.
                //
                // Nothing is charged to the back-off, and the ordinary interval is
                // waited: this round asked the registry rather than Anthropic, and
                // pacing a loop on a question that costs nothing would be pacing it
                // on nothing. Nothing was read and nothing was decided, which is
                // exactly what a hold is.
                Ok(Turn::NotArranged(why)) => {
                    let why = why.to_string();
                    let line =
                        watch::held_line(&why, Some(watch::REFRESH_INTERVAL_MILLIS), host.now());
                    (
                        watch::REFRESH_INTERVAL_MILLIS,
                        Spoken::held(&why, Some(watch::REFRESH_INTERVAL_MILLIS), line),
                    )
                }
                // Another `perch` holding the registry is an ordinary event, not a
                // fault: this loop runs for hours beside the commands a person
                // types, and `perch status --refresh` holds the lock across every
                // Renewal and every read it makes — comfortably longer than the few
                // seconds `lock::take` waits. Ending the watcher over that would
                // mean a `perch status` could stop it, silently, and the machine
                // would go unwatched until somebody noticed.
                //
                // So it is held like any other round that could not read: counted
                // against the back-off, said out loud with when it will try again,
                // and gone round again (ADR 0013, ADR 0018).
                Err(PerchError::Busy(why)) => {
                    backoff.failed();
                    let waiting_for = backoff.waiting_for();
                    let line = watch::held_line(&why, Some(waiting_for), host.now());
                    (waiting_for, Spoken::held(&why, Some(waiting_for), line))
                }
                Err(other) => return Err(other),
            };

        say_it(out, &mut holding, spoken, host.now())?;

        // The one place the loop holds nothing it took this round, and
        // therefore the only place it is asked whether to go round again: a
        // stop asked for during a round is answered here, once that round has
        // finished cleanly.
        //
        // How long is the round's to say rather than a constant, because a
        // round that could not read anything is followed by the back-off it
        // printed. Read off the round rather than worked out again here, so the
        // wait the line promised and the wait taken cannot come to differ.
        if host.wait(waiting_for) == Waited::Interrupted {
            break;
        }
    }

    stopped(out)
}

/// What the loop says on the way out.
///
/// It no longer promises that nothing was left behind. A Watcher holds the
/// watcher lock for as long as it runs (ADR 0040), and that promise was written
/// when there was nothing to hold — so it says what is true instead: the lock is
/// given back, and everything else is as it was.
fn stopped(out: &mut dyn Write) -> Result<()> {
    say(
        out,
        "Stopped. The watcher lock is given back, no file of its own was \
         written, and the Account you are on is the one it last Switched to.",
    )
}

/// Becomes the only Watcher on this machine, holding and coming back for as
/// long as somebody else is one.
///
/// `None` where the person — or the service manager — asked it to stop while it
/// was waiting.
///
/// Held rather than refused, and this is the whole reason the watcher lock can
/// be given a staleness window measured in tens of minutes. A `perch watch` that
/// exited here would be a Service that exits at every start until a lock left
/// behind by a `kill -9` goes stale, which is the crash loop ADR 0040 repealed
/// the permission exits to avoid — arriving through the thing that enforces
/// single-instance. So it says who has it and comes back, and the machine heals
/// itself.
fn take_the_watch<'a>(
    host: &'a dyn Host,
    out: &mut dyn Write,
    backoff: &mut Backoff,
    holding: &mut Holding,
) -> Result<Option<crate::lock::Held<'a>>> {
    loop {
        match crate::lock::take_all(host, vec![registry::watcher_lock_spec(host)?]) {
            Ok(held) => return Ok(Some(held)),
            Err(PerchError::Busy(why)) => {
                backoff.failed();
                let waiting_for = backoff.waiting_for();
                let line = watch::held_line(&why, Some(waiting_for), host.now());
                say_it(
                    out,
                    holding,
                    Spoken::held(&why, Some(waiting_for), line),
                    host.now(),
                )?;
                if host.wait(waiting_for) == Waited::Interrupted {
                    return Ok(None);
                }
            }
            Err(other) => return Err(other),
        }
    }
}

/// A round's line, and whether it was a hold — which is the only thing the
/// coalescing needs to know about it.
enum Spoken {
    /// Held, by `why`, coming back in `retrying_in`, and this is the line that
    /// says both in full. The two travel together because a hold that has
    /// changed either of them is one the log has to say again.
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

/// Says a round, as much of it as is worth saying (ADR 0040).
///
/// A hold repeats until whatever holds it changes, and a Service writes to a log
/// nobody reads until something is wrong — so an unchanged hold is said in full
/// when it starts, as a duration once an hour while it lasts, and as what it
/// cost when it ends. A round that decided something is always said in full,
/// whatever came before it: the decisions are what the log is for.
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
            // The way out of a hold is always said, which is the half of ADR
            // 0013's rule that survives untouched — and it is said before the
            // decision rather than after it, so a log reads in the order the
            // things happened.
            if let Some(held_for) = holding.released(now) {
                say(out, &watch::released_line(held_for, now))?;
            }
            say(out, &line)
        }
    }
}

/// What the loop is about to start doing, said before it does it.
///
/// A machine that is not arranged for watching is no longer a refusal here (ADR
/// 0040). The loop starts anyway and holds, so this says what it *would* be
/// doing and leaves the reason it is not to the first round's held line — which
/// is the line that will repeat, and the one that says when it will ask again.
fn opening(host: &dyn Host) -> Result<String> {
    let registry = adopt::ensure_adopted(host)?;
    let Ok(watching) = permitted(&registry) else {
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
        registry.named_for_the_user(watching.account.email()),
        watching.scope.within(),
        watch::how_often(),
        watching.policy.threshold,
        watching.policy.ceiling(),
        watch::COOLDOWN_MINUTES,
    ))
}

/// Which watcher a round belongs to.
///
/// One difference, and every part of it is here rather than as a test of this
/// enum scattered through the round: where the cooldown is kept between rounds,
/// and who decides when the next reading is (ADR 0013). Both follow from the
/// same thing — a loop is one process a person is watching, and a check is one
/// of a sequence of processes nobody is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watcher {
    /// `perch watch`: rounds separated by a wait this process takes, so what
    /// paces it lives in memory and dies with it.
    Loop,
    /// `perch watch --once`: one round and out, so what paces it is written to
    /// the registry for the next invocation to read, and how long until that
    /// one is whatever scheduled them.
    Check,
}

impl Watcher {
    /// How long before the watcher reads again, where that is the watcher's to
    /// say. A loop backs off and prints the wait it lands on; a check is
    /// leaving, and promising an interval it has no part in would put the one
    /// untrue thing on the line.
    fn asking_again(self, backoff: &Backoff) -> Option<u64> {
        match self {
            Watcher::Loop => Some(backoff.waiting_for()),
            Watcher::Check => None,
        }
    }

    /// What paces this round, put where the round will read it.
    ///
    /// A check remembers nothing of the one before it, so it takes the Group's
    /// record — read under the lock, so the cooldown a round is held by is the
    /// one that was on record when it decided. The loop's is already in the
    /// caller's hands, where it has been since the loop started.
    fn pacing(self, carried: &mut Recently, registry: &Registry, scope: &Scope) {
        match self {
            Watcher::Loop => {}
            Watcher::Check => *carried = Recently::recorded(registry.checked(&scope.key())),
        }
    }

    /// A Switch that happened, remembered where this watcher will find it next
    /// time.
    ///
    /// For a check that is the registry, and the write happens before the
    /// Switch is recorded so that the one save carries both: a cooldown that
    /// did not survive the process would be a check that moved and let the next
    /// one move straight back. The loop's memory is the
    /// [`Recently`](crate::watch::Recently) it is holding, which the round has
    /// already told.
    fn remember(self, registry: &mut Registry, scope: &Scope, off: &str, at: DateTime<Utc>) {
        match self {
            Watcher::Loop => {}
            Watcher::Check => registry.record_check(&scope.key(), off, at),
        }
    }
}

/// The Account being watched, the Scope that said it may be, and the rules it
/// is watched under.
struct Watching {
    account: Account,
    /// The Scope a Switch would be taken within — a Group, or the Accounts in
    /// no Group (ADR 0017, amended). Written as a Scope rather than a Group
    /// name so that serving the second is real work rather than a fallthrough:
    /// this is a code path that can Switch somebody's Account without being
    /// asked, and that is the correct amount of friction for it.
    scope: Scope,
    policy: Policy,
}

/// Whether there is anything here for a watcher to do, and what.
///
/// Asked every round rather than only at the start, because the answer can stop
/// being yes underneath it: an Account can be moved out of its Group, and a
/// Group can be told to stop letting the watcher act, while the loop is
/// sleeping. A loop still *acting* on permission that has been withdrawn is the
/// exact thing "nothing changes underneath you unless you said it could" is
/// about.
///
/// Acting, rather than existing. ADR 0013 stopped the loop on a `no` here and
/// ADR 0040 holds it instead, which changes nothing about the grant: a holding
/// Watcher does not Refresh, does not rank and does not Switch. What it changes
/// is that a Service whose permission was withdrawn on Tuesday is watching again
/// the moment it is granted on Wednesday, rather than being a unit somebody has
/// to remember to start.
///
/// Every failure it can give is that same "not arranged yet" — no active
/// Account, an ungrouped one nobody has declared interchangeable, a Scope that
/// has not said the watcher may act — which is what lets the caller hold on all
/// of them without sorting them from the failures that mean something is broken.
fn permitted(registry: &Registry) -> Result<Watching> {
    let account = registry.active_account().cloned().ok_or_else(|| {
        PerchError::NotFound(
            "Perch holds no active Account, so there is nothing to watch. \
             `perch switch <target>` makes one active."
                .to_string(),
        )
    })?;

    let scope = match account.group.clone() {
        Some(group) => Scope::Group(group),
        None => Scope::Ungrouped,
    };

    // The one place the layering is deliberately not uniform:
    // `watcher-may-act` does not reach the Accounts in no Group by
    // Inheritance, and is gated behind `cycle-ungrouped` instead. The reason is
    // the whole of ADR 0017 (amended), and is not repeated here.
    //
    // Asked before `watcher-may-act` so that somebody who has said neither is
    // told about the declaration rather than about the permission — the
    // declaration is the one that has to come first.
    if scope == Scope::Ungrouped && !registry.global.cycle_ungrouped {
        return Err(PerchError::NotInterchangeable(format!(
            "{} is in no Group, and nothing has said the Accounts in no Group \
             are interchangeable at all — so there is nowhere for the watcher \
             to Switch it to. Nothing is being watched.\n\
             `perch config set cycle-ungrouped true` says they are, and \
             `perch config set {UNGROUPED} watcher-may-act true` then says the \
             watcher may act on them. Both, because a `watcher-may-act` set at \
             Global is about your Groups and must not authorise moving you onto \
             a personal subscription (ADR 0017).\n\
             Putting it in a Group with `perch group move {} <group>` is the \
             narrower statement, and is what Groups are for.",
            registry.named_for_the_user(account.email()),
            account.email(),
        )));
    }

    let settings = registry.in_force(&scope.config());
    if !settings.watcher_may_act {
        return Err(PerchError::Invalid(format!(
            "{} has not been told the watcher may act on it, so nothing is \
             being watched. Nothing only ever changes underneath you because \
             you said it could.\n\
             `perch config set {} watcher-may-act true` says it may.",
            scope.config().described(),
            scope.key(),
        )));
    }

    Ok(Watching {
        account,
        scope,
        policy: Policy::of(&settings),
    })
}

/// One turn: read, decide, and act if acting is what was decided.
///
/// The registry lock is taken here and given back when this returns, rather
/// than being held for the life of the loop. A watcher that held it would shut
/// every other `perch` out of the machine for as long as it ran, and would
/// leave a lock behind if it were killed.
fn one_round(
    host: &dyn Host,
    watcher: Watcher,
    recently: &mut Recently,
    backoff: &mut Backoff,
) -> Result<Turn> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
    // Handed back rather than raised, so the two callers can answer it
    // differently without this one having to know which is asking (ADR 0040).
    // A loop holds on it and a Check exits on it, and the difference is
    // *whether there is a process left to hold* rather than anything about what
    // was found.
    //
    // Carried as the failure itself rather than as its sentence, so a Check
    // still exits `18` for an ungrouped Account and `14` for a Scope that has
    // not said the watcher may act — the codes ADR 0013 promised a scheduler,
    // which nothing here repeals.
    let watching = match permitted(&registry) {
        Ok(watching) => watching,
        Err(not_arranged) => return Ok(Turn::NotArranged(not_arranged)),
    };
    let email = watching.account.email().to_string();

    watcher.pacing(recently, &registry, &watching.scope);

    // The one Account Refreshed, and nearly all of the network this loop
    // spends (ADR 0013).
    let report = observe::refresh(
        host,
        &mut perch,
        &mut registry,
        std::slice::from_ref(&email),
    );
    // Worth saying and not worth holding a decision over: the figure this round
    // decides on is the one that was just read, whether or not it survived to
    // the next round — which will read its own.
    if let Some(not_kept) = &report.not_kept {
        host.note(not_kept);
    }

    // A hold is not only a decision not to act: it is also the loop deciding to
    // ask less often, because the endpoint it is asking has a budget and is
    // already refusing. Both live here, in the one place a hold is made, so a
    // failure cannot be reported without being counted. The wait it lands on
    // goes on the line, so a person reading the log knows when the watcher
    // comes back rather than wondering whether it has given up.
    let mut held = |why: String| {
        backoff.failed();
        Ok(Turn::Decided(Round {
            email: email.clone(),
            fullest: None,
            threshold: watching.policy.threshold,
            outcome: Outcome::Held {
                why,
                retrying_in: watcher.asking_again(backoff),
            },
        }))
    };

    // Never on a figure it did not just read. Acting on a cached one would be a
    // Switch made on evidence the user already had, and a held decision costs
    // nothing (ADR 0013).
    if let Some(refused) = refused_the_reading(&report.attempts) {
        return held(refused);
    }
    let account = registry
        .account(&email)
        .expect("the Account just refreshed is one Perch holds");
    let Some(fullest) = Fullest::of(account) else {
        // Read, and carrying no Quota Window Perch could make anything of. Not
        // a reading of zero: an answer that says nothing about how full an
        // Account is says nothing about whether to leave it.
        return held(
            "Anthropic answered without a Quota Window Perch could read, so \
             there was no figure to decide on."
                .to_string(),
        );
    };
    // A figure, so whatever was wrong is over. Said here rather than at the end
    // of the round, because what the loop decides about a figure it has is no
    // evidence about the endpoint that gave it: a round that finds nowhere to
    // go has read perfectly well.
    backoff.read();

    let outcome = if !fullest.at_or_over(watching.policy.threshold) {
        Outcome::Waiting
    } else if let Some(why) = recently.resting(host.now()) {
        // Before the candidates are read, so a round that may not act spends
        // nothing finding out where it would have gone.
        Outcome::Cooling { why }
    } else {
        act(
            host,
            &mut perch,
            &mut registry,
            &watching,
            watcher,
            recently,
        )?
    };
    Ok(Turn::Decided(Round {
        email,
        fullest: Some(fullest),
        threshold: watching.policy.threshold,
        outcome,
    }))
}

/// What a round came to, before anybody has decided what to do about it.
///
/// The distinction ADR 0040 turns on. A round that *decided* something is the
/// same for both watchers, and a machine that is *not arranged for watching* is
/// not: the loop holds on it and comes back, because a supervisor respawns a
/// deliberate exit until it gives up on the unit; a Check exits on it, because
/// there is no process left to hold and a scheduler has to be told.
///
/// Kept as an enum rather than answered inside the round, so neither answer is
/// the one the round happened to be written for. `permitted` is the only thing
/// that produces the second, and it produces nothing else — which is why the
/// failure travels whole rather than as a sentence: a Check's exit code is the
/// one the failure earned.
enum Turn {
    /// The round read, and decided.
    Decided(Round),
    /// There was nothing here to watch, and this says what is missing.
    NotArranged(PerchError),
}

/// Why the reading cannot be acted on, or `None` when it can.
///
/// `None` is the one answer that lets a Switch happen, so it is given only for
/// an Account that was read: an empty report is answered as a hold rather than
/// as an absence of objections. That case cannot arise — a Refresh of the
/// Account Perch just read out of its own registry always reports on it — and
/// it is written out anyway, because this is the single place enforcing the
/// single rule the watcher has, and "nobody objected" is exactly how such a
/// rule stops being enforced.
///
/// None of the reasons names the Account: the decision line has already said
/// which Account it is about, and twice over is noise on a line that has to
/// stay readable at one every couple of minutes.
fn refused_the_reading(attempts: &[Attempt]) -> Option<String> {
    let Some(attempt) = attempts.first() else {
        return Some(
            "nothing was read at all, so there was no current figure to decide \
             on."
            .to_string(),
        );
    };
    match &attempt.outcome {
        observe::Outcome::Observed => None,
        observe::Outcome::Throttled => Some(
            "Anthropic is rate-limiting reads of this Account's usage, so \
             nothing current could be read."
                .to_string(),
        ),
        observe::Outcome::Failed(why) => Some(why.clone()),
        observe::Outcome::Quarantined { why, .. } => Some(format!(
            "{}. {}",
            why.because(),
            crate::registry::how_to_repair(&attempt.email),
        )),
    }
}

/// The Account is full enough to move off. This is the whole of what the
/// watcher does about it, which is a Switch and nothing else.
///
/// The candidates are read here rather than kept warm. This is the only moment
/// their figures have to be worth anything, and it is the moment they are
/// cheapest to get: a candidate is by definition an Account nothing is running
/// against, so ADR 0005 permits Renewing it to ask. What reading them every
/// round instead would cost is [`crate::watch::REFRESH_INTERVAL_MILLIS`]'s
/// arithmetic, and it is why this is a burst at a crossing rather than a
/// second loop.
fn act(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    watching: &Watching,
    watcher: Watcher,
    recently: &mut Recently,
) -> Result<Outcome> {
    let scope = watching.scope.clone();
    let outgoing = watching.account.clone();

    // Asked before the candidates are read rather than only by the Switch
    // below, which is the same bargain `perch purge` makes about its questions:
    // the first ask is what stops the command spending something on a caller it
    // was always going to refuse.
    //
    // What is spent here is an hourly allowance that does not refill early (ADR
    // 0015), one read per candidate, and the state it was being spent on is not
    // a momentary one: a `perch run` held open in another terminal keeps the
    // outgoing Profile Live for as long as somebody is working in it, and the
    // watcher came round every two and a half minutes and read every candidate
    // again. A Group of three burned about twenty-four reads an hour on each of
    // them, indefinitely, and throttled the `perch status --refresh` the user
    // types — arriving at exactly the moment the watcher matters most. ADR 0013
    // rules this out in as many words: candidates are ranked at the moment a
    // decision is taken, not kept warm.
    //
    // Reported as the Switch would have reported it, because it is the same
    // refusal about the same Profile — the Switch simply no longer gets to be
    // the one to notice.
    // Every way this can fail is answered, because an `if let` over one variant
    // drops the rest on the floor. `refuse_if_live` refuses with `ProfileLive`,
    // but it also fails with `ProbeRefused` for a `sessions` directory that is
    // there and will not be read — the root-owned one a `sudo claude` leaves —
    // and with `Invalid` for an address no Profile can be named after. Discarded,
    // those became a round that carried on to spend a Renewal on every candidate
    // and then met the same failure in the Switch below, which is the one thing
    // the ask above exists to prevent: the allowance is gone by the time the
    // round finds out it was never going to move.
    let installed = probe::Installed::probed(host)?;
    match switch::refuse_if_live(host, &outgoing, &installed) {
        Ok(()) => {}
        Err(refused @ PerchError::ProfileLive(_)) => {
            return Ok(Outcome::Refused {
                why: refused.to_string(),
            });
        }
        // The same ending the Switch below gives them, which is where they were
        // being met before: the loop stops rather than deciding what to do next
        // about a machine nobody has looked at.
        Err(other) => return Err(other),
    }

    let read = observe::refresh(
        host,
        perch,
        registry,
        &addresses_of(&considered(registry, watching)),
    );
    // What could not be read, carried into the sentence that says where the
    // watcher went: an Account ranked on a figure from an hour ago is the one
    // thing that can make this Switch land somewhere worse than it left, so it
    // is said on the line rather than left for somebody to work out.
    let unread = read.notes();

    // The margin, applied to the figures as this round has them: which Accounts
    // are not empty enough — or not legible enough — to be worth the move.
    let set_aside = watch::set_aside(
        &watching.policy,
        &watching.scope,
        &considered(registry, watching),
    );

    let choice = match cycle::choose(
        registry,
        &scope,
        Some(outgoing.email()),
        &set_aside,
        host.now(),
    ) {
        Ok(choice) => choice,
        // Nowhere worth going is an answer rather than a failure: every
        // candidate is exhausted, or the Account Perch is on is still the best
        // of them. Both are resolved by waiting, which is what the loop is for.
        Err(error @ (PerchError::NoCandidate(_) | PerchError::NothingToDo(_))) => {
            return Ok(Outcome::Nowhere {
                why: also(error.to_string(), &unread),
            });
        }
        Err(error) => return Err(error),
    };

    let landing = switch::perform(
        host,
        perch,
        &installed,
        &choice.account,
        Some(&outgoing),
        registry,
    );

    // Only a Switch that happened starts a cooldown. A round that was refused
    // or found nowhere to go has changed nothing, and making it wait would be
    // pacing the watcher on its failures.
    //
    // A Switch that moved and then failed counts, and this is written before
    // the recording below so that the one save carries both. Without it a check
    // that moved and then failed left no record of having moved, and the next
    // scheduled one was free to move straight back: exactly the "cooldown that
    // did not survive the process" ADR 0013 names. Asking the Landing rather
    // than the outcome is what makes the two cases one.
    let moved = landing.moved();
    if moved {
        recently.switched(host.now());
        watcher.remember(registry, &watching.scope, outgoing.email(), host.now());
    }

    match landing.record(host, perch, registry) {
        Ok(_captured) => Ok(Outcome::Switched {
            because: also(choice.because, &unread),
        }),
        // Nothing was changed, so there is nothing to look at and nothing to
        // repair — a client holding the outgoing Profile, most often, which
        // stops holding it when it exits. The round says so and the loop goes
        // on watching.
        //
        // Only where the next round genuinely has a different answer to give.
        // A refusal is reported as `nothing to do now`, which is a scheduler's
        // cue to come back in five minutes and expect the machine to have moved
        // on — true of a client that will exit and of an Account just
        // Quarantined, which `record` has already written down and which is
        // passed over from the next round onwards. A locked keychain, a probe
        // that cannot find Claude
        // Code, a Profile that will not be written: none of those clear
        // themselves, and folding them in here had `perch watch --once` exiting
        // 15 every five minutes forever while a cron mailbox read "nothing to
        // do" — for a machine that needed somebody to look at it. They keep the
        // code the failure earned (the exit-code table promises `11` for a keychain
        // nobody can reach), and the loop stops on them rather than retrying a
        // full Capture-and-write every two and a half minutes.
        //
        // The incoming Credential being live and something after that failing
        // is not one of them: the machine is part way through a Switch, and a
        // watcher that carried on watching would be deciding what to do next
        // about a machine nobody has looked at yet. So it is answered first,
        // whatever the failure was.
        Err(error) if moved => Err(error),
        // A Quarantine has already been written by `record`, so the next round
        // passes the Account over rather than making the same discovery again.
        Err(error @ (PerchError::Quarantined { .. } | PerchError::ProfileLive(_))) => {
            Ok(Outcome::Refused {
                why: error.to_string(),
            })
        }
        Err(other) => Err(other),
    }
}

/// The Accounts a Switch could land on, with the figures this round has of
/// them.
///
/// The Account being left is not among them — it was read at the top of this
/// round, which is what got us here — and neither is one that is disabled or
/// Quarantined, because ranking has never been able to choose either.
///
/// Called twice a round, before the Refresh to say what to read and after it to
/// say what the margin makes of what was read, so that the figure the margin
/// judges is the one the ranking will use. Both times through the same walk:
/// two lists of "the Accounts that could be landed on" would have to be kept in
/// step, and an Account in one and not the other is one the watcher never reads
/// and lands on anyway.
fn considered(registry: &Registry, watching: &Watching) -> Vec<Considered> {
    watching
        .scope
        .accounts(registry)
        .iter()
        .filter(|account| {
            account.email() != watching.account.email() && cycle::is_a_candidate(account)
        })
        .map(|account| Considered {
            email: account.email().to_string(),
            named: registry.named_for_the_user(account.email()),
            fullest: Fullest::of(account),
        })
        .collect()
}

/// Their addresses, which is all a Refresh takes.
///
/// Every one of them, and that is the whole of the rule now: the Account this
/// watcher just came off used to be dropped here as well, because no-return
/// could not be Switched back to and a read for a choice that cannot be made is
/// an allowance spent on nothing. It never dropped anything — the cooldown had
/// already ended the round before this was reached (ADR 0046).
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
