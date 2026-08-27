//! What a round decides, between what it means and what it does.
//!
//! `watch` holds what a figure *means* and reaches neither the network nor the
//! filesystem; `commands::watch` holds what a round *does* and reaches both.
//! Between them sit the decisions naming an `observe::Attempt`, a
//! `switch::Settled` and a `cycle` candidate — the lowest module reaching all
//! three is this one, and it is what the round was given an interface for
//! (ADR code-lives-where-it-reaches). Nothing here reaches the machine either,
//! so every answer below can be argued with in a unit test.

use crate::cycle;
use crate::error::{PerchError, Result};
use crate::live::Idle;
use crate::name::{self, UNGROUPED};
use crate::observe::{self, Attempt};
use crate::registry::{Account, Registry, Scope};
use crate::switch::Settled;
use crate::watch::{Considered, Cooled, Fullest, Lost, Policy, Round};

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
pub fn permitted(registry: &Registry, _settled: &Settled) -> Result<Watching> {
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

/// The Accounts a Switch could land on, with the figures this round has of them.
///
/// Called twice a round, through the same walk both times, and the one funnel that
/// produces candidate addresses — which is why it takes both witnesses and reads
/// neither.
pub fn considered(
    registry: &Registry,
    watching: &Watching,
    _cooled: &Cooled<'_>,
    _idle: &Idle,
) -> Vec<Considered> {
    let sharers = crate::registry::Sharers::across(registry);
    watching
        .scope
        .accounts(registry)
        .iter()
        .filter(|account| {
            // Through the registry's own answer rather than `!=`, which would be
            // correct only by two facts that are true two modules away.
            !name::same_name(account.email(), watching.account.email())
                && cycle::is_a_candidate(&sharers, account)
        })
        .map(|account| Considered {
            email: account.email().to_string(),
            named: registry.named_for_the_user(account.email()),
            fullest: Fullest::of(account),
        })
        .collect()
}

/// Their addresses, which is all a Refresh takes.
pub fn addresses_of(considered: &[Considered]) -> Vec<String> {
    considered
        .iter()
        .map(|candidate| candidate.email.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::host::FakeHost;
    use crate::observe::Outcome;
    use crate::probe::Installed;
    use crate::registry::{Quarantine, WindowUtilization};
    use crate::watch::Recently;
    use crate::{live, switch};

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
        switch::nothing_in_flight(registry).expect("nothing is in flight")
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

    #[test]
    fn the_account_being_watched_is_never_among_its_own_candidates() {
        let mut registry = granted(declared(watching_one()));
        registry.upsert(ungrouped("spare@example.com", 10.0));
        let watching = asking(&registry).expect("declared and granted");
        let (crossed, idle) = witnesses(&watching.account);

        let candidates = considered(
            &registry,
            &watching,
            &crossed
                .cooled(&Recently::nothing(), now())
                .expect("nothing has Switched, so nothing is cooling"),
            &idle,
        );

        assert_eq!(addresses_of(&candidates), vec!["spare@example.com"]);
    }
}
