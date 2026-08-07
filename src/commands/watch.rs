//! `perch watch` — the watcher that Cycles on your behalf when the Account you
//! are on runs low.
//!
//! A loop in a terminal you can see and kill, and deliberately not a daemon
//! (ADR 0013): no service to install, no lifecycle to manage on three
//! platforms, and nothing left behind when it stops. What it decides and why is
//! [`crate::watch`]; the round it takes to decide it is here.
//!
//! `perch watch --once` is one of those rounds, for cron or a systemd timer to
//! run: the same policy and the same decision line, with what was decided in
//! the exit code because nobody is reading a terminal. Scheduling is the
//! operating system's job, and the whole of the difference between the two is
//! [`Watcher`].
//!
//! One round is: Refresh the active Account, from Anthropic rather than from
//! cache; say what that came to against the Group's threshold; and where it is
//! at or over, rank the Group's other Accounts *now* and Switch to the best of
//! them. Why the candidates are read then rather than kept warm is
//! [`act`]'s to explain; how often the active one is read is
//! [`crate::watch::REFRESH_INTERVAL_MILLIS`]'s.
//!
//! Being over the threshold is not on its own a reason to move. The rest of the
//! Group's [`Policy`](crate::watch::Policy) is what stops two Accounts either
//! side of it from trading places every few minutes: a cooldown under how often
//! a Switch may happen at all, checked here before anything is read, and a
//! margin under where one may land, applied by setting candidates aside for the
//! ranking in [`act`]. The one thing carried between rounds is a
//! [`Recently`](crate::watch::Recently), which is what those two are measured
//! from — in memory for the loop, and on the registry for a check, which has no
//! memory of the one before it.
//!
//! What it does when it acts is a Switch, whole: the outgoing Credential is
//! Captured first (ADR 0006), Claude Code's locks are taken, and a Live
//! Profile's token is never Renewed (ADR 0005). Running while Claude Code is
//! working is the normal case rather than the exception.
//!
//! Nothing is held across the wait — not the registry lock, not Claude Code's
//! locks, not a session marker. That is what makes Ctrl-C safe: the loop spends
//! nearly all of its life in the one place where being killed costs nothing,
//! and the interrupt it takes over from the default handler is only there so
//! that a Ctrl-C arriving mid-Switch lets that Switch finish first.
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
use crate::commands::{say, switch as switch_command};
use crate::cycle::{self, Scope};
use crate::error::{EXIT_OK, PerchError, Result};
use crate::host::{Host, Waited};
use crate::observe::{self, Attempt};
use crate::registry::{Account, Registry};
use crate::switch::{self, Interrupted};
use crate::watch::{self, Backoff, Considered, Fullest, Outcome, Policy, Recently, Round};

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

    // Nothing carried in from anywhere: the cooldown and the no-return come off
    // the registry inside the round, and a back-off would be pacing a loop this
    // process does not have. How soon to come back is the scheduler's.
    let round = one_round(
        host,
        out,
        Watcher::Check,
        &mut Recently::nothing(),
        &mut Backoff::none(),
    )?;
    say(out, &round.line(host.now()))?;
    Ok(round.outcome.exit_code())
}

fn keep_watching(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    // Before the first round, so that a Ctrl-C during it is a request to stop
    // rather than a process killed in the middle of a Switch.
    host.listen_for_interrupts();

    let watching = opening(host, out)?;
    say(out, &watching)?;

    // The two things carried from one round to the next, and the reason the
    // cooldown is the loop's rather than the machine's (ADR 0013). Both are in
    // memory and nowhere else, which is what "writes no file of its own" means.
    let mut recently = Recently::nothing();
    let mut backoff = Backoff::none();

    loop {
        let round = one_round(host, out, Watcher::Loop, &mut recently, &mut backoff)?;
        say(out, &round.line(host.now()))?;

        // The one place the loop holds nothing, and therefore the only place it
        // is asked whether to go round again: a stop asked for during a round
        // is answered here, once that round has finished cleanly.
        //
        // How long is the round's to say rather than a constant, because a
        // round that could not read anything is followed by the back-off it
        // printed. Read off the round rather than worked out again here, so the
        // wait the line promised and the wait taken cannot come to differ.
        if host.wait(round.waiting_for()) == Waited::Interrupted {
            break;
        }
    }

    say(
        out,
        "Stopped. Nothing was left behind: the watcher holds no lock, writes no \
         file of its own, and the Account you are on is the one it last \
         Switched to.",
    )
}

/// What the loop is about to start doing, said before it does it.
///
/// It is also where a watcher that may not act says so and exits rather than
/// idling forever having decided nothing.
fn opening(host: &dyn Host, out: &mut dyn Write) -> Result<String> {
    let registry = adopt::ensure_adopted(host, out)?;
    let watching = permitted(&registry)?;
    Ok(format!(
        "Watching {} in Group `{}`. Reading how full it is every {}, and \
         Switching within the Group when its fullest Quota Window reaches {}% \
         — to an Account at {}% or under, and never twice inside {} minutes. \
         Ctrl-C stops.",
        registry.named_for_the_user(watching.account.email()),
        watching.group,
        watch::how_often(),
        watching.policy.threshold,
        watching.policy.ceiling(),
        watching.policy.cooldown_minutes,
    ))
}

/// Which watcher a round belongs to.
///
/// One difference, and every part of it is here rather than as a test of this
/// enum scattered through the round: where the cooldown and the no-return are
/// kept between rounds, and who decides when the next reading is (ADR 0013).
/// Both follow from the same thing — a loop is one process a person is
/// watching, and a check is one of a sequence of processes nobody is.
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
    fn pacing(self, carried: &mut Recently, registry: &Registry, group: &str) {
        match self {
            Watcher::Loop => {}
            Watcher::Check => *carried = Recently::recorded(registry.checked(group)),
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
    fn remember(self, registry: &mut Registry, group: &str, off: &str, at: DateTime<Utc>) {
        match self {
            Watcher::Loop => {}
            Watcher::Check => registry.record_check(group, off, at),
        }
    }
}

/// The Account being watched, the Group that said it may be, and the rules it
/// is watched under.
struct Watching {
    account: Account,
    group: String,
    policy: Policy,
}

/// Whether there is anything here for a watcher to do, and what.
///
/// Asked every round rather than only at the start, because the answer can stop
/// being yes underneath it: an Account can be moved out of its Group, and a
/// Group can be told to stop letting the watcher act, while the loop is
/// sleeping. A loop still running on permission that has been withdrawn is the
/// exact thing "nothing changes underneath you unless you said it could" is
/// about.
fn permitted(registry: &Registry) -> Result<Watching> {
    let account = registry.active_account().cloned().ok_or_else(|| {
        PerchError::NotFound(
            "Perch holds no active Account, so there is nothing to watch. \
             `perch switch <target>` makes one active."
                .to_string(),
        )
    })?;

    // `cycle-ungrouped` (ADR 0017) grants the watcher nothing, and is not
    // consulted here. Permission to Switch when you ask and permission to
    // Switch while nobody is looking are different grants, and the second has
    // no owner when there is no Group to carry it.
    let Some(group) = account.group.clone() else {
        return Err(PerchError::NotInterchangeable(format!(
            "{} is in no Group, so nothing carries permission for the watcher \
             to act on it. Nothing is being watched.\n\
             Put it in a Group with `perch group move {} <group>`, then let the \
             watcher act on that Group with `perch config set <group> \
             watcher-may-act true`.",
            registry.named_for_the_user(account.email()),
            account.email(),
        )));
    };

    let config = registry.group(&group).cloned().unwrap_or_default();
    if !config.watcher_may_act {
        return Err(PerchError::Invalid(format!(
            "Group `{group}` has not been told the watcher may act on it, so \
             nothing is being watched. A Group only ever changes underneath you \
             because you said it could.\n\
             `perch config set {group} watcher-may-act true` says it may."
        )));
    }

    Ok(Watching {
        account,
        group,
        policy: Policy::of(&config),
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
    out: &mut dyn Write,
    watcher: Watcher,
    recently: &mut Recently,
    backoff: &mut Backoff,
) -> Result<Round> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host, out)?;
    let watching = permitted(&registry)?;
    let email = watching.account.email().to_string();

    watcher.pacing(recently, &registry, &watching.group);

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
        Ok(Round {
            email: email.clone(),
            fullest: None,
            threshold: watching.policy.threshold,
            outcome: Outcome::Held {
                why,
                retrying_in: watcher.asking_again(backoff),
            },
        })
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
    } else if let Some(why) = recently.resting(&watching.policy, host.now()) {
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
    Ok(Round {
        email,
        fullest: Some(fullest),
        threshold: watching.policy.threshold,
        outcome,
    })
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
    let scope = Scope::Group(watching.group.clone());
    let outgoing = watching.account.clone();

    // The Account this watcher just came off is not read and not landed on
    // (ADR 0013). Coming straight back is the second half of a ping-pong, and
    // an Account nothing could be Switched to is an allowance spent on nothing.
    let barred = recently
        .barred(&watching.policy, host.now())
        .map(str::to_string);
    let read = observe::refresh(
        host,
        perch,
        registry,
        &worth_reading(&considered(registry, watching), barred.as_deref()),
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
        &watching.group,
        &considered(registry, watching),
        barred.as_deref(),
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

    let landed = switch::perform(host, &choice.account, Some(&outgoing));
    match landed {
        Ok(_captured) => {
            // Only a Switch that happened starts a cooldown. A round that was
            // refused or found nowhere to go has changed nothing, and making it
            // wait would be pacing the watcher on its failures.
            recently.switched(outgoing.email(), host.now());
            watcher.remember(registry, &watching.group, outgoing.email(), host.now());
            switch_command::record_active(host, perch, registry, &choice.account)?;
            Ok(Outcome::Switched {
                because: also(choice.because, &unread),
            })
        }
        // Nothing was changed, so there is nothing to look at and nothing to
        // repair — a client holding the outgoing Profile, most often, which
        // stops holding it when it exits. The round says so and the loop goes
        // on watching.
        Err(Interrupted {
            error,
            incoming_is_live: false,
            quarantine,
        }) => {
            if let Some(why) = quarantine {
                switch_command::record_quarantine(
                    host,
                    perch,
                    registry,
                    choice.account.email(),
                    why,
                );
            }
            Ok(Outcome::Refused {
                why: error.to_string(),
            })
        }
        // The incoming Credential is live and something after that failed, so
        // the machine is part way through a Switch. The loop stops on it: a
        // watcher that carried on watching would be deciding what to do next
        // about a machine nobody has looked at yet.
        Err(Interrupted { error, .. }) => {
            // Which Account is active is a fact about which Credential is in
            // the Default Profile, so it is recorded before the failure is
            // reported — anything else would send the next Capture into the
            // wrong Profile (ADR 0006).
            if let Err(unrecorded) =
                switch_command::record_active(host, perch, registry, &choice.account)
            {
                return Err(error.with_note(&unrecorded.to_string()));
            }
            Err(error)
        }
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
    registry
        .accounts_in(&watching.group)
        .iter()
        .filter(|account| {
            account.email() != watching.account.email() && account.enabled && !account.quarantined()
        })
        .map(|account| Considered {
            email: account.email().to_string(),
            named: registry.named_for_the_user(account.email()),
            fullest: Fullest::of(account),
        })
        .collect()
}

/// Which of them are worth spending a read of the network on: all but the one
/// no-return has barred, because a read for a choice that cannot be made is an
/// allowance spent on nothing.
///
/// The barred Account stays in [`considered`] even though it is not read — it
/// has to be set aside by name, and leaving it out of that list would leave it
/// a candidate.
fn worth_reading(considered: &[Considered], barred: Option<&str>) -> Vec<String> {
    considered
        .iter()
        .filter(|candidate| Some(candidate.email.as_str()) != barred)
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
