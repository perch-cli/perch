//! What a round decides, between what it means and what it does.
//!
//! `watch` holds what a figure *means* and reaches neither the network nor the
//! filesystem; `commands::watch` holds what a round *does* and reaches both.
//! Between them sit the decisions naming an `observe::Attempt`, a
//! `registry::Settled` and a `cycle` candidate — the lowest module reaching all
//! three is this one, and it is what the round was given an interface for
//! (ADR code-lives-where-it-reaches). Nothing here reaches the machine either,
//! so every answer below can be argued with in a unit test.

use chrono::{DateTime, Utc};

use crate::config::Scope;
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::live::Idle;
use crate::lock::Lost;
use crate::name::{self, UNGROUPED};
use crate::observe::{self, Attempt};
use crate::registry::Settled;
use crate::registry::{Account, Registry};
use crate::watch::{
    self, Considered, Cooled, Figure, Fullest, Outcome, Pacing, Policy, Recently, Round, Watcher,
    nothing_was_switched,
};

/// The Account being watched, the Scope that said it may be, and the rules it is
/// watched under.
#[derive(Debug)]
pub struct Watching {
    pub account: Account,
    /// The Scope a Switch would be taken within — a Group, or the Accounts in no Group.
    /// Written as a Scope rather than a Group name so that serving the second is real
    /// work rather than a fallthrough: this is a path that can Switch somebody's
    /// Account without being asked.
    pub scope: Scope,
    pub policy: Policy,
}

/// Whether there is anything here for a watcher to do, and what.
///
/// Asked every round rather than only at the start, because the answer can stop being
/// yes underneath it, and every failure it gives is the same "not arranged yet". The
/// [`Settled`] is why it cannot be asked too early (ADR an-ordering-is-a-type).
pub fn permitted(registry: &Registry, settled: &Settled) -> Result<Watching> {
    let account = registry.active_account(settled).cloned().ok_or_else(|| {
        PerchError::NotFound(
            "Perch holds no active Account, so there is nothing to watch. \
             `perch switch <target>` makes one active."
                .to_string(),
        )
    })?;

    let scope = registry.scope_of(&account);

    // The refusals are this module's to word; which of the two applies is asked
    // where every other reader of it asks (ADR a-setting-names-its-scope).
    match cycle::may_act_within(registry, &scope) {
        cycle::MayAct::Undeclared { .. } => {
            return Err(PerchError::NotInterchangeable(format!(
                "{} is in no Group, and nothing has said the Accounts in no Group \
                 are interchangeable at all, so nothing is being watched.\n\
                 `perch config set {UNGROUPED} interchangeable true` says they are, \
                 and `perch config set {UNGROUPED} watcher-may-act true` then says \
                 the watcher may act. Both are needed.\n\
                 Putting it in a Group with `perch group move {} <group>` is the \
                 narrower way.",
                registry.named_for_the_user(account.email()),
                account.email(),
            )));
        }
        cycle::MayAct::Ungranted => {
            return Err(PerchError::Invalid(format!(
                "{} has not been told the watcher may act on it, so nothing is \
                 being watched.\n\
                 `perch config set {} watcher-may-act true` says it may.",
                scope.described(),
                scope.word(),
            )));
        }
        cycle::MayAct::May => {}
    }

    let policy = Policy::of(&registry.settings(&scope));
    Ok(Watching {
        account,
        scope,
        policy,
    })
}

/// What a round read, before anything has decided what it means.
///
/// Every field is a reading rather than a judgment — the figures that came back, the
/// Account as the Refresh left it, the rules it is watched under, and what paces it.
pub struct Reading<'a> {
    /// The Account the figures were just written to, which is what a Fullest is taken
    /// from. Not [`Watching::account`], which is the copy read before the Refresh.
    pub account: &'a Account,
    pub report: &'a observe::Report,
    pub policy: &'a Policy,
    pub recently: &'a Recently,
    pub now: DateTime<Utc>,
}

/// What one reading comes to, decided here and nowhere else. `act` is handed a
/// [`Cooled`], which is the only way to have one — so a round that may not act
/// cannot call it, and what decides that is this function rather than the order of
/// statements around it — and the [`Pacing`], because what it spends and what it
/// cannot read are charged where they happen.
pub fn decide(
    reading: Reading<'_>,
    watcher: Watcher,
    pacing: &mut Pacing,
    act: impl FnOnce(&Cooled<'_>, &mut Pacing) -> Result<Outcome>,
) -> Result<Round> {
    let threshold = reading.policy.threshold;

    // Before the reading is judged, because a round that stopped read nothing and a
    // hold charged for it would pace the Back-off off a question nobody was asked.
    if let Some(lost) = reading.report.stopped {
        return Ok(Round {
            fullest: None,
            threshold,
            outcome: nothing_was_switched(lost),
            watcher,
        });
    }

    // A hold said against the wait it earned. `waiting_for` is passed in rather than
    // charged here, because not every hold is a question nobody answered.
    let held = |why: String, waiting_for: u64| Round {
        fullest: None,
        threshold,
        outcome: Outcome::Held {
            why,
            retrying_in: waiting_for,
        },
        watcher,
    };

    // Never on a figure it did not just read.
    if let Some(refused) = refused_the_reading(&reading.report.attempts) {
        let waiting_for = refused.charged(pacing);
        return Ok(held(refused.why, waiting_for));
    }

    let Some(fullest) = Fullest::of(reading.account) else {
        // Read, and carrying no Quota Window Perch could make anything of. Unreachable,
        // and answered anyway: acting on an Account whose fullness is unknown is the
        // one thing this round may never do.
        return Ok(held(
            "Anthropic answered without a Quota Window Perch could read, so \
             there was no figure to decide on."
                .to_string(),
            pacing.backoff.could_not_read(),
        ));
    };

    // Two asks that hand on what they earned, and the figure comes back out of each.
    let (fullest, outcome) = match fullest.crossed(threshold) {
        Err(under) => (under, Outcome::Waiting),
        Ok(crossed) => match crossed.cooled(reading.recently, reading.now) {
            // Before the candidates are read, so a round that may not act spends
            // nothing finding out where it would have gone.
            Err(cooling) => (
                crossed.fullest().clone(),
                Outcome::Cooling { why: cooling.why },
            ),
            Ok(cooled) => (
                cooled.fullest().clone(),
                match pacing.rest.resting(reading.now) {
                    // Before the candidates are read, for the same reason as the
                    // cooldown: the burst that read them last is what is resting.
                    Some(why) => Outcome::Nowhere { why },
                    None => act(&cooled, pacing)?,
                },
            ),
        },
    };
    // A round that read everything it asked for, so whatever was wrong is over. A
    // hold from the Act is the one round that asked and was not answered.
    match &outcome {
        Outcome::Held { .. } => {}
        Outcome::Waiting
        | Outcome::Cooling { .. }
        | Outcome::Switched { .. }
        | Outcome::Nowhere { .. }
        | Outcome::Refused { .. }
        | Outcome::HandedOver { .. }
        | Outcome::Stopped { .. } => pacing.backoff.read(),
    }
    Ok(Round {
        fullest: Some(fullest),
        threshold,
        outcome,
        watcher,
    })
}

/// What a round came to, before anybody has decided what to do about it.
///
/// A round that *decided* something is the same for both watchers; a machine that is
/// *not arranged for watching* is not, because the loop holds on it and a Check exits
/// on it. Not a *turn*, which is one Account's pass through the Refresh path.
pub enum Verdict {
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
pub fn refused_the_reading(attempts: &[Attempt]) -> Option<Refusal> {
    let Some(attempt) = attempts.first() else {
        return Some(Refusal::paced(
            "nothing was read at all, so there was no current figure to decide \
             on."
            .to_string(),
        ));
    };
    Refusal::of(attempt)
}

/// Why no candidate's figure can be acted on, or `None` while any was read.
///
/// One unread candidate among read ones is set aside by the Margin; every one unread
/// is a round with no figure to decide on, and a hold is what that is. The reasons
/// are `read.unread()`'s, so the line says which candidate and why.
pub fn refused_the_candidates(
    considered: &[Considered],
    read: &observe::Report,
) -> Option<Refusal> {
    let none_read = !considered.is_empty()
        && considered
            .iter()
            .all(|candidate| candidate.figure == Figure::Unread);
    if !none_read {
        return None;
    }
    let paced = read
        .attempts
        .iter()
        .filter_map(Refusal::of)
        .any(|refusal| refusal.paced);
    Some(Refusal {
        why: format!(
            "no candidate could be read, so nothing was decided on the figures \
             Perch already had. {}",
            read.unread().join(" "),
        ),
        paced,
    })
}

impl Refusal {
    /// Why one reading cannot be acted on, or `None` when it can.
    fn of(attempt: &Attempt) -> Option<Refusal> {
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
            // Neither reaches an `Attempt` here: a round that stopped is reported
            // through `Report::stopped`, and a round reads under `Spending::ItsOwn`,
            // so the Watcher is never told to stand aside for itself.
            observe::Outcome::Stopped(_) | observe::Outcome::JustRead => None,
        }
    }

    /// The wait this refusal earns, charged where it is paced: a hold cannot be
    /// reported against a wait it has not just paid for.
    pub fn charged(&self, pacing: &mut Pacing) -> u64 {
        match self.paced {
            true => pacing.backoff.could_not_read(),
            false => watch::REFRESH_INTERVAL_MILLIS,
        }
    }
}

/// Why a round had no figure to decide on, and whether the Back-off paces it.
///
/// The two travel together because a hold reported against a wait it did not earn
/// is the failure either half alone allows: a repair made a minute after the
/// Quarantine would otherwise wait out a doubling nothing spent.
pub struct Refusal {
    pub why: String,
    pub paced: bool,
}

impl Refusal {
    fn paced(why: String) -> Refusal {
        Refusal { why, paced: true }
    }

    fn unpaced(why: String) -> Refusal {
        Refusal { why, paced: false }
    }
}

/// The Accounts a Switch could land on this round, walked once.
///
/// Their addresses are what a Refresh takes; their figures are what the Margin is
/// applied to, and are worth having only once that Refresh has written them — so a
/// figure comes off [`Candidates::refreshed`] and off nothing else.
pub struct Candidates(Vec<Candidate>);

/// One of them as the walk settles it, which is everything about a candidate a
/// Refresh does not move.
struct Candidate {
    email: String,
    named: String,
}

impl Candidates {
    /// The walk, and the one funnel that produces candidate addresses — which is why
    /// it takes both witnesses and reads neither.
    pub fn of(
        registry: &Registry,
        watching: &Watching,
        _cooled: &Cooled<'_>,
        _idle: &Idle,
    ) -> Candidates {
        let sharers = crate::registry::Sharers::across(registry);
        Candidates(
            watching
                .scope
                .accounts(registry)
                .iter()
                .filter(|account| {
                    // Through the Registry's own answer rather than `!=`, which would be
                    // correct only by two facts that are true two modules away.
                    !name::same_name(account.email(), watching.account.email())
                        && cycle::is_a_candidate(&sharers, account)
                })
                .map(|account| Candidate {
                    email: account.email().to_string(),
                    named: registry.named_for_the_user(account.email()),
                })
                .collect(),
        )
    }

    /// Their addresses, which is all a Refresh takes.
    pub fn addresses(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|candidate| candidate.email.clone())
            .collect()
    }

    /// The same candidates, carrying the figures the Refresh in `read` has just
    /// written, each judged by the Scope's Measure. Only those: one the Refresh did
    /// not read is [`Figure::Unread`] whatever the cache holds, and one gone from the
    /// Registry since the walk carries none — both what the Margin sets aside.
    pub fn refreshed(
        self,
        registry: &Registry,
        measure: cycle::Measure,
        read: &observe::Report,
    ) -> Vec<Considered> {
        self.0
            .into_iter()
            .map(|candidate| Considered {
                figure: match read.observed(&candidate.email) {
                    false => Figure::Unread,
                    true => registry
                        .account(&candidate.email)
                        .and_then(|account| Fullest::measured(account, measure))
                        .map_or(Figure::Unobserved, Figure::Read),
                },
                email: candidate.email,
                named: candidate.named,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::host::FakeHost;
    use crate::live;
    use crate::observe::Outcome;
    use crate::probe::Installed;
    use crate::registry::{Quarantine, WindowUtilization};
    use crate::watch::Recently;

    const WATCHED: &str = "watched@example.com";

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()
    }

    /// The shared fixture's Account, taken out of the Group it puts one in: the
    /// Ungrouped Scope is the one whose declaration and whose grant are two
    /// separate statements, which is what these cases are about.
    fn ungrouped(email: &str, used_percent: f64) -> Account {
        let mut account = cycle::tests::account(
            email,
            vec![WindowUtilization {
                window: "5-hour".to_string(),
                used_percent,
                resets_at: None,
            }],
        );
        account.group = None;
        account
    }

    fn watching_one() -> Registry {
        let mut registry = Registry::default();
        registry.upsert(ungrouped(WATCHED, 90.0));
        registry.settle(Some(WATCHED.to_string()));
        registry
    }

    fn read(email: &str, used_percent: f64) -> (Account, observe::Report) {
        (
            ungrouped(email, used_percent),
            observe::Report {
                attempts: vec![Attempt {
                    email: email.to_string(),
                    named: email.to_string(),
                    outcome: Outcome::Observed,
                }],
                asked: true,
                ..observe::Report::default()
            },
        )
    }

    fn at(threshold: u8) -> Policy {
        Policy {
            threshold,
            margin: 10,
        }
    }

    /// Why a hold held, or `None` where the round decided something else.
    fn waits_because(outcome: watch::Outcome) -> Option<String> {
        match outcome {
            watch::Outcome::Held { why, .. } => Some(why),
            _ => None,
        }
    }

    /// What every arm that may not act is handed. Being called is the failure, so
    /// this says so rather than each test carrying a flag to check afterwards.
    fn never_acts(_: &Cooled<'_>, _: &mut Pacing) -> Result<watch::Outcome> {
        panic!("this reading may not act")
    }

    /// What the one arm that may act does, so the round's outcome is its outcome.
    fn switched() -> watch::Outcome {
        watch::Outcome::Switched {
            to: "somewhere@example.com".to_string(),
            unread: Vec::new(),
        }
    }

    #[test]
    fn a_figure_under_the_threshold_waits_and_never_reaches_the_act() {
        let (account, report) = read(WATCHED, 50.0);

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &Recently::nothing(),
                now: now(),
            },
            Watcher::Loop,
            &mut Pacing::none(),
            // Nothing may act on an Account it may stay on.
            never_acts,
        )
        .expect("a reading under the threshold decides");

        assert!(matches!(round.outcome, watch::Outcome::Waiting));
        assert_eq!(round.fullest.expect("it read one").used_percent, 50.0);
    }

    /// The Cooldown is read at the top of every round, so this is the arm that keeps a
    /// Watcher its Service just restarted from Switching again immediately.
    #[test]
    fn a_figure_over_the_threshold_inside_the_cooldown_cools_without_acting() {
        let (account, report) = read(WATCHED, 90.0);
        let mut recently = Recently::nothing();
        recently.switched(now() - chrono::Duration::minutes(1));

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &recently,
                now: now(),
            },
            Watcher::Loop,
            &mut Pacing::none(),
            // It spends nothing finding out where it would have gone.
            never_acts,
        )
        .expect("a reading inside the cooldown decides");

        assert!(matches!(round.outcome, watch::Outcome::Cooling { .. }));
        assert_eq!(
            round
                .fullest
                .expect("the figure comes back out")
                .used_percent,
            90.0,
            "a hold still says what it was holding on"
        );
    }

    #[test]
    fn a_figure_over_the_threshold_and_out_of_the_cooldown_is_the_one_reading_that_acts() {
        let (account, report) = read(WATCHED, 90.0);
        let acted = std::cell::Cell::new(false);

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &Recently::nothing(),
                now: now(),
            },
            Watcher::Loop,
            &mut Pacing::none(),
            |_cooled, _pacing| {
                acted.set(true);
                Ok(switched())
            },
        )
        .expect("a reading that may act decides");

        assert!(acted.get(), "this is the one arm that acts");
        assert_eq!(round.outcome, switched(), "and the outcome is what it did");
    }

    /// A round that stopped read nothing, so a hold charged for it would pace the
    /// Back-off off a question nobody was asked.
    #[test]
    fn a_round_that_lost_the_watch_decides_on_no_figure_and_charges_nothing() {
        let (account, mut report) = read(WATCHED, 90.0);
        report.stopped = Some(Lost::HandedOver);
        let mut pacing = Pacing::none();

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &Recently::nothing(),
                now: now(),
            },
            Watcher::Loop,
            &mut pacing,
            never_acts,
        )
        .expect("a round that stopped is still a round");

        assert!(round.fullest.is_none(), "it read no figure to decide on");
        assert_eq!(
            pacing.backoff.could_not_read(),
            Pacing::none().backoff.could_not_read(),
            "and nothing was charged against a question it never asked"
        );
    }

    /// The burst is the one thing a round spends on more than one Account, and the
    /// rest is asked before it for the reason the cooldown is: a round that will not
    /// read the candidates has no business finding out where it would have gone.
    #[test]
    fn a_burst_that_went_out_is_not_repeated_inside_the_cooldown_and_the_round_says_so() {
        let (account, report) = read(WATCHED, 90.0);
        let mut pacing = Pacing::none();
        pacing.rest.found_nowhere(
            now() - chrono::Duration::minutes(2),
            "Every Account in Group `work` is exhausted.",
        );

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &Recently::nothing(),
                now: now(),
            },
            Watcher::Loop,
            &mut pacing,
            never_acts,
        )
        .expect("a resting burst is still a round");

        let watch::Outcome::Nowhere { why } = &round.outcome else {
            panic!(
                "nowhere to go, on the burst that said so: {:?}",
                round.outcome
            );
        };
        assert!(why.contains("is exhausted"), "the burst's reason: {why}");
        assert!(why.contains("2 minutes ago"), "{why}");
        assert_eq!(
            round.waiting_for(),
            watch::REFRESH_INTERVAL_MILLIS,
            "the Account being watched is still read at the interval"
        );
        assert_eq!(
            round.fullest.as_ref().expect("it read one").used_percent,
            90.0,
            "and the figure it read is the one on the line"
        );
    }

    /// The Act reads the candidates, and a hold it reports is one it charged: the
    /// Back-off doubles across rounds whose own reading was fine.
    #[test]
    fn a_hold_from_the_act_keeps_its_charge_where_a_round_that_read_drops_it() {
        let (account, report) = read(WATCHED, 90.0);
        let policy = at(80);
        let recently = Recently::nothing();
        let mut pacing = Pacing::none();
        let reading = || Reading {
            account: &account,
            report: &report,
            policy: &policy,
            recently: &recently,
            now: now(),
        };
        let held = |_: &Cooled<'_>, pacing: &mut Pacing| {
            Ok(watch::Outcome::Held {
                why: "no candidate could be read.".to_string(),
                retrying_in: pacing.backoff.could_not_read(),
            })
        };

        let first = decide(reading(), Watcher::Loop, &mut pacing, held).expect("a round");
        let second = decide(reading(), Watcher::Loop, &mut pacing, held).expect("a round");
        let read_through =
            decide(reading(), Watcher::Loop, &mut pacing, |_, _| Ok(switched())).expect("a round");
        let after = decide(reading(), Watcher::Loop, &mut pacing, held).expect("a round");

        assert_eq!(first.waiting_for(), watch::REFRESH_INTERVAL_MILLIS);
        assert_eq!(second.waiting_for(), watch::REFRESH_INTERVAL_MILLIS * 2);
        assert_eq!(read_through.waiting_for(), watch::REFRESH_INTERVAL_MILLIS);
        assert_eq!(
            after.waiting_for(),
            watch::REFRESH_INTERVAL_MILLIS,
            "the burst that read dropped the whole of it"
        );
    }

    /// The whole of what a `Watcher` decides, asked of the one arm that names a wait.
    #[test]
    fn a_check_says_no_next_reading_where_a_loop_says_when() {
        let (account, mut report) = read(WATCHED, 90.0);
        report.attempts[0].outcome = Outcome::Throttled;

        let held = |watcher| {
            decide(
                Reading {
                    account: &account,
                    report: &report,
                    policy: &at(80),
                    recently: &Recently::nothing(),
                    now: now(),
                },
                watcher,
                &mut Pacing::none(),
                never_acts,
            )
            .expect("a reading nothing could be made of is still a round")
            .line(now())
        };

        assert!(
            held(Watcher::Loop).contains("Asking again in"),
            "a loop takes the wait itself, so it says how long"
        );
        assert!(
            !held(Watcher::Check).contains("Asking again"),
            "a check exits, so whatever scheduled it decides"
        );
    }

    /// Read, and carrying no Quota Window Perch could make anything of. Nothing in
    /// production produces it — Anthropic answering at all means a window — and the
    /// arm exists because acting on an Account whose fullness is unknown is the one
    /// thing a round may never do. Only a unit test can put a round in front of it.
    #[test]
    fn a_reading_with_no_window_in_it_holds_rather_than_acting_on_an_unknown_fullness() {
        let mut account = cycle::tests::account(WATCHED, vec![]);
        account.group = None;
        let (_, report) = read(WATCHED, 90.0);

        let round = decide(
            Reading {
                account: &account,
                report: &report,
                policy: &at(80),
                recently: &Recently::nothing(),
                now: now(),
            },
            Watcher::Loop,
            &mut Pacing::none(),
            never_acts,
        )
        .expect("a reading with nothing in it is still a round");

        assert!(round.fullest.is_none(), "there was no figure to decide on");
        let held = waits_because(round.outcome).expect("an unknown fullness is a hold");
        assert!(held.contains("Quota Window"), "{held}");
    }

    fn declared(mut registry: Registry) -> Registry {
        registry.ungrouped.interchangeable = true;
        registry
    }

    fn granted(mut registry: Registry) -> Registry {
        registry
            .settings_mut(&Scope::Ungrouped)
            .expect("the Ungrouped Scope carries Settings")
            .watcher_may_act = true;
        registry
    }

    /// The witness `permitted` takes and reads nothing of.
    fn settled(registry: &Registry) -> Settled {
        crate::registry::nothing_in_flight(registry).expect("nothing is in flight")
    }

    fn asking(registry: &Registry) -> Result<Watching> {
        permitted(registry, &settled(registry))
    }

    #[test]
    fn a_machine_on_nobody_has_nothing_to_watch() {
        let refused = asking(&Registry::default()).expect_err("Perch is on nobody");

        assert!(matches!(refused, PerchError::NotFound(_)), "{refused:?}");
    }

    #[test]
    fn ungrouped_accounts_nobody_declared_interchangeable_are_not_watched() {
        let refused = asking(&watching_one()).expect_err("nothing declared them a set");

        assert!(
            matches!(refused, PerchError::NotInterchangeable(_)),
            "{refused:?}"
        );
        let said = refused.to_string();
        // Both statements, because a grant alone is not enough for the
        // Ungrouped Accounts and a reader told only half is sent back twice.
        assert!(said.contains("interchangeable true"), "{said}");
        assert!(said.contains("watcher-may-act true"), "{said}");
    }

    #[test]
    fn a_scope_that_never_granted_the_watcher_anything_is_not_watched() {
        let refused = asking(&declared(watching_one())).expect_err("nothing granted it");

        assert!(matches!(refused, PerchError::Invalid(_)), "{refused:?}");
        assert!(
            refused.to_string().contains("watcher-may-act true"),
            "{refused}"
        );
    }

    #[test]
    fn a_declared_and_granted_scope_is_watched_under_its_own_settings() {
        let registry = granted(declared(watching_one()));

        let watching = asking(&registry).expect("declared and granted");

        assert_eq!(watching.account.email(), WATCHED);
        assert_eq!(watching.scope, Scope::Ungrouped);
        assert_eq!(
            watching.policy,
            Policy::of(&registry.settings(&Scope::Ungrouped))
        );
    }

    fn attempt(outcome: Outcome) -> Vec<Attempt> {
        vec![Attempt {
            email: WATCHED.to_string(),
            named: WATCHED.to_string(),
            outcome,
        }]
    }

    #[test]
    fn a_round_that_read_nothing_at_all_is_held_rather_than_unobjected_to() {
        let refusal = refused_the_reading(&[]).expect("an empty report is not agreement");

        assert!(refusal.paced, "nothing read is a question nobody answered");
    }

    #[test]
    fn only_a_reading_that_happened_lets_a_switch_through() {
        assert!(refused_the_reading(&attempt(Outcome::Observed)).is_none());
    }

    #[test]
    fn a_back_off_paces_only_the_refusals_that_spent_a_request() {
        let sent = refused_the_reading(&attempt(Outcome::Failed {
            why: "Anthropic would not answer".to_string(),
            spent: true,
        }))
        .expect("a failure is not a figure");
        let never_sent = refused_the_reading(&attempt(Outcome::Failed {
            why: "a client is holding the Profile".to_string(),
            spent: false,
        }))
        .expect("a failure is not a figure");

        assert!(sent.paced, "a question that went out and came back");
        assert!(
            !never_sent.paced,
            "a Back-off paces questions, not refusals"
        );
        assert!(never_sent.why.contains("holding the Profile"));
    }

    #[test]
    fn a_quarantine_is_unpaced_and_says_how_to_end_it() {
        let refusal = refused_the_reading(&attempt(Outcome::Quarantined {
            why: Quarantine::RenewalRejected,
            detail: None,
        }))
        .expect("a Quarantined Account is no figure");

        // A repair made a minute later would otherwise wait out a doubling
        // nothing spent.
        assert!(!refusal.paced, "the round asked Anthropic nothing");
        assert!(refusal.why.contains("relogin"), "{}", refusal.why);
    }

    #[test]
    fn throttling_is_paced_because_the_request_went_out() {
        let refusal = refused_the_reading(&attempt(Outcome::Throttled)).expect("no figure");

        assert!(refusal.paced);
    }

    /// The two witnesses `considered` takes and reads neither of. `Cooled` is
    /// reachable only through a `Crossed`, which is why the fixture crosses one.
    fn witnesses(account: &Account) -> (crate::watch::Crossed, Idle) {
        let crossed = Fullest::of(account)
            .expect("the fixture carries a window")
            .crossed(80)
            .expect("90 is over 80");
        let idle = live::ask(&FakeHost::new(), &[])
            .idle_or(&Installed::unknown("2.1.221"), &live::NOTHING_WAS_CHANGED)
            .expect("no Place was asked about, so nothing is live");
        (crossed, idle)
    }

    /// A Scope holding the Account being watched and one spare, with the walk
    /// already made — which is the state a round is in when it Refreshes.
    fn walked() -> (Registry, Candidates) {
        let mut registry = granted(declared(watching_one()));
        registry.upsert(ungrouped("spare@example.com", 10.0));
        let watching = asking(&registry).expect("declared and granted");
        let (crossed, idle) = witnesses(&watching.account);
        let cooled = crossed
            .cooled(&Recently::nothing(), now())
            .expect("nothing has Switched, so nothing is cooling");

        let candidates = Candidates::of(&registry, &watching, &cooled, &idle);
        (registry, candidates)
    }

    #[test]
    fn the_account_being_watched_is_never_among_its_own_candidates() {
        let (_, candidates) = walked();

        assert_eq!(candidates.addresses(), vec!["spare@example.com"]);
    }

    /// The load-bearing half of the ordering: the walk happens before the burst
    /// and the Margin is applied to what the burst wrote, so a figure that moved
    /// in between is the one a candidate is judged on.
    #[test]
    fn the_figure_a_candidate_is_judged_on_is_the_one_read_after_the_walk() {
        let (mut registry, candidates) = walked();
        registry
            .account_mut("spare@example.com")
            .expect("the spare is held")
            .utilization = ungrouped("spare@example.com", 70.0).utilization;

        let refreshed = candidates.refreshed(
            &registry,
            cycle::Measure::Worst,
            &observed("spare@example.com"),
        );

        let Figure::Read(fullest) = &refreshed
            .first()
            .expect("the spare is still a candidate")
            .figure
        else {
            panic!("the spare was read: {refreshed:?}");
        };
        assert_eq!(
            fullest.used_percent, 70.0,
            "the walk saw 10, and the Refresh wrote 70"
        );
    }

    #[test]
    fn a_candidate_gone_between_the_walk_and_the_reading_carries_no_figure() {
        let (mut registry, candidates) = walked();
        registry.forget("spare@example.com");

        assert_eq!(
            candidates.refreshed(
                &registry,
                cycle::Measure::Worst,
                &observed("spare@example.com")
            )[0]
            .figure,
            Figure::Unobserved,
            "no figure is what the Margin sets aside"
        );
    }

    /// A Report saying `email` was read this round.
    fn observed(email: &str) -> observe::Report {
        observe::Report {
            attempts: vec![Attempt {
                email: email.to_string(),
                named: email.to_string(),
                outcome: Outcome::Observed,
            }],
            asked: true,
            ..observe::Report::default()
        }
    }

    /// The figure the cache holds is not the figure a candidate is judged on: a
    /// read that failed leaves the candidate unread, whatever was cached.
    #[test]
    fn a_candidate_whose_read_failed_is_unread_whatever_the_cache_holds() {
        let (registry, candidates) = walked();
        let throttled = observe::Report {
            attempts: vec![Attempt {
                email: "spare@example.com".to_string(),
                named: "spare@example.com".to_string(),
                outcome: Outcome::Throttled,
            }],
            asked: true,
            ..observe::Report::default()
        };

        let considered = candidates.refreshed(&registry, cycle::Measure::Worst, &throttled);

        assert_eq!(
            considered[0].figure,
            Figure::Unread,
            "the cached 10% is not it"
        );
        let refusal = refused_the_candidates(&considered, &throttled).expect("nothing was read");
        assert!(refusal.paced, "the request went out");
        assert!(refusal.why.contains("rate-limiting"), "{}", refusal.why);
        assert!(
            !refusal.why.contains("cached figure is what you see"),
            "a Watcher decides on no cached figure, so it points at none: {}",
            refusal.why
        );
    }

    #[test]
    fn one_candidate_read_among_unread_ones_is_a_decision_rather_than_a_hold() {
        let considered = vec![
            Considered {
                email: "a@example.com".to_string(),
                named: "a@example.com".to_string(),
                figure: Figure::Unread,
            },
            Considered {
                email: "b@example.com".to_string(),
                named: "b@example.com".to_string(),
                figure: Figure::Read(Fullest {
                    window: "5-hour".to_string(),
                    used_percent: 5.0,
                }),
            },
        ];

        assert!(
            refused_the_candidates(&considered, &observed("b@example.com")).is_none(),
            "the Margin sets the unread one aside, and the round chooses among the rest"
        );
        assert!(
            refused_the_candidates(&[], &observe::Report::asked_for()).is_none(),
            "no candidates at all is nowhere to go, not a failure to read"
        );
    }
}
