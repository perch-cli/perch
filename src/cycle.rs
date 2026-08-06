//! The Cycle: which Account a Switch lands on when nobody named one.
//!
//! Two rules, and everything here follows from them.
//!
//! An Account's headroom is its **worst** Quota Window, and the Account whose
//! worst is best wins (ADR 0012). Being blocked by any window blocks you
//! completely, so this is the only ranking that measures what actually stops
//! work: when Perch says 40% headroom, that is true of every window and nothing
//! surprising blocks you five minutes later.
//!
//! How headroom is *measured* is fixed. Which Account to prefer is the Group's
//! to say, and is a separate axis on top of it: the most headroom, or the
//! soonest-resetting window so perishable quota is spent rather than wasted
//! (ADR 0002). A Strategy reorders the candidates and cannot promote one that
//! the measurement rules out, so an exhausted Account is never chosen however
//! soon it comes back.
//!
//! A Cycle never leaves the scope it started in (ADR 0002). A work subscription
//! running dry must not land on a personal Account, so the scope is a Group —
//! the declaration that a set of Accounts is interchangeable — or the ungrouped
//! Accounts, which are a scope only when a global setting says so (ADR 0017).
//!
//! The three honest non-outcomes matter as much as the choice. Every Account
//! exhausted, already on the best one, and nobody having declared these
//! Accounts interchangeable each perform no Switch, explain themselves, and
//! exit with a code of their own rather than pretending to have worked.
//!
//! Nothing here reaches the network or the filesystem: ranking is on the cached
//! figures and their ages (ADR 0015), which is what makes it a pure decision
//! that can be argued with in a unit test.

use chrono::{DateTime, Utc};

use crate::error::{PerchError, Result};
use crate::registry::{Account, CachedUtilization, Registry, Strategy, WindowUtilization};
use crate::utilization;

/// Where a Cycle may look for a landing place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The Accounts in one Group, named as the Group was declared.
    Group(String),
    /// The Accounts in no Group at all — a scope only because a global setting
    /// says those Accounts are interchangeable (ADR 0017).
    Ungrouped,
}

impl Scope {
    fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Group(name) => registry.accounts_in(name),
            Scope::Ungrouped => registry.ungrouped_accounts(),
        }
    }

    /// Which Account this scope prefers when more than one would serve.
    ///
    /// A Strategy is a Group's to carry (ADR 0002), and the Accounts in no
    /// Group are not a Group (ADR 0017) — nothing holds a Strategy for them,
    /// so they Cycle the default way.
    fn strategy(&self, registry: &Registry) -> Strategy {
        match self {
            Scope::Group(name) => registry
                .group(name)
                .map_or_else(Strategy::default, |config| config.strategy),
            Scope::Ungrouped => Strategy::default(),
        }
    }

    /// What the Cycle is about to do, said before it does it.
    pub fn announcement(&self) -> String {
        match self {
            Scope::Group(name) => format!("Cycling within Group `{name}`."),
            Scope::Ungrouped => "Cycling among the Accounts in no Group.".to_string(),
        }
    }

    /// The scope as the middle of a sentence: "every Account in {}".
    fn described(&self) -> String {
        match self {
            Scope::Group(name) => format!("Group `{name}`"),
            Scope::Ungrouped => "no Group".to_string(),
        }
    }
}

/// Where a bare `perch switch` may look, given the Account it would be leaving.
///
/// An ungrouped Account is the ordinary starting state rather than an edge case
/// — adoption leaves the first Account in no Group — so this is a refusal the
/// user is expected to meet, and it names both ways out of it.
pub fn scope_for(registry: &Registry, leaving: &Account) -> Result<Scope> {
    match &leaving.group {
        Some(group) => Ok(Scope::Group(group.clone())),
        None if registry.global.cycle_ungrouped => Ok(Scope::Ungrouped),
        None => Err(PerchError::NotInterchangeable(format!(
            "{} is in no Group, so nothing has declared which Accounts it is \
             interchangeable with. Nothing was changed.\n\
             Either put it in a Group with `perch group move {} <group>`, or \
             declare that every ungrouped Account is interchangeable with \
             `perch config set cycle-ungrouped true`.",
            registry.named_for_the_user(leaving.email()),
            leaving.email(),
        ))),
    }
}

/// How full an Account is, measured its only honest way: by the Quota Window
/// that is fullest (ADR 0012).
#[derive(Debug, Clone, PartialEq)]
enum Headroom {
    /// Every Quota Window has at least this much room left.
    Room {
        percent: f64,
        fullest_window: String,
        /// When the fullest window comes back, if the observation carried it.
        /// The window that decides the headroom is the window whose reset
        /// decides how perishable that headroom is, so one Quota Window
        /// answers both questions and the two Strategies cannot end up reading
        /// different windows.
        resets_at: Option<DateTime<Utc>>,
        observed_at: DateTime<Utc>,
    },
    /// A Quota Window is full, so the Account is blocked whatever its others
    /// say. It frees up when the last of its full windows resets — not the
    /// first, which would still leave it blocked by the others.
    Exhausted { frees_at: Option<DateTime<Utc>> },
    /// No figure has ever been observed. Never read as room: "no figure" and
    /// "plenty of room" are opposite pieces of advice.
    Unobserved,
}

impl Headroom {
    /// What ranking sorts on, higher being better. Known room beats an
    /// unknown, and an unknown beats a window that is full — treating "never
    /// observed" as good news is exactly the mistake the ordering exists to
    /// prevent.
    ///
    /// The Strategy reorders Accounts that have room and does nothing else. It
    /// cannot promote one over an Account with more evidence behind it, which
    /// is what keeps it an axis on top of ADR 0012's measurement rather than a
    /// way round it. Hence four tiers rather than three: soonest-reset ranks on
    /// a reset time where Perch has one and falls back to the room it can see
    /// where it has not, because the Strategy says which figure to prefer and
    /// not which figures to invent.
    ///
    /// Nothing compares a reset time against a percentage. They only ever meet
    /// as tiers, and a tie inside one is broken by the order the Accounts were
    /// added.
    fn ranking(&self, strategy: Strategy) -> (u8, f64) {
        match self {
            Headroom::Room {
                percent, resets_at, ..
            } => match (strategy, resets_at) {
                // Sooner is better, so the figure sorted on is the reset time
                // negated.
                (Strategy::SoonestReset, Some(at)) => (3, -(at.timestamp() as f64)),
                _ => (2, *percent),
            },
            Headroom::Unobserved => (1, 0.0),
            Headroom::Exhausted { .. } => (0, 0.0),
        }
    }

    /// Whether the Strategy got the figure it asked for.
    ///
    /// Soonest-reset falls back to headroom for an Account whose figure does
    /// not say when it comes back, so the sentences that quote a reason have to
    /// know which of the two they are quoting. Claiming an Account resets
    /// soonest on the strength of a reset time Perch has not got would be the
    /// one thing worse than falling back silently.
    fn ranked_on_its_reset(&self, strategy: Strategy) -> bool {
        matches!(
            (self, strategy),
            (
                Headroom::Room {
                    resets_at: Some(_),
                    ..
                },
                Strategy::SoonestReset
            )
        )
    }

    fn is_exhausted(&self) -> bool {
        matches!(self, Headroom::Exhausted { .. })
    }

    /// The figure as a clause, for the sentences that quote it — the one that
    /// says why an Account won, and the one that says why staying put is
    /// already the best you can do. Both make the same promise about the same
    /// number, so they make it in the same words.
    ///
    /// The clause is the one the Strategy judged on, because a number quoted
    /// as the reason has to be the number that decided it.
    fn as_a_clause(&self, strategy: Strategy, now: DateTime<Utc>) -> Option<String> {
        let Headroom::Room {
            percent,
            fullest_window,
            resets_at,
            observed_at,
        } = self
        else {
            return None;
        };
        let age = utilization::age_phrase(*observed_at, now);
        Some(match (strategy, resets_at) {
            (Strategy::MostHeadroom, _) => format!(
                "{percent:.0}% headroom, which is true of every one of its Quota \
                 Windows — {fullest_window} is its fullest, as of {age}"
            ),
            (Strategy::SoonestReset, Some(at)) => format!(
                "{percent:.0}% headroom, and the window that leaves it least — \
                 {fullest_window} — resets at {}, as of {age}",
                utilization::reset_phrase(*at, now),
            ),
            // Ranked on its room, because that is what there was to rank it on.
            (Strategy::SoonestReset, None) => format!(
                "{percent:.0}% headroom, which is true of every one of its Quota \
                 Windows — {fullest_window} is its fullest — and no cached \
                 figure says when that one comes back, as of {age}"
            ),
        })
    }
}

/// What reads a figure Perch has not got, said the same way wherever the
/// absence of one is the reason for the answer.
const HOW_TO_GET_FIGURES: &str = "`perch status --group --refresh` reads current figures.";

fn headroom_of(account: &Account) -> Headroom {
    let Some(cached) = account.observed_utilization() else {
        return Headroom::Unobserved;
    };
    let fullest = fullest_window(cached).expect("an observation carries at least one window");
    if fullest.used_percent >= 100.0 {
        return Headroom::Exhausted {
            frees_at: frees_at(cached),
        };
    }
    Headroom::Room {
        percent: 100.0 - fullest.used_percent,
        fullest_window: fullest.window.clone(),
        resets_at: fullest.resets_at,
        observed_at: cached.observed_at,
    }
}

fn fullest_window(cached: &CachedUtilization) -> Option<&WindowUtilization> {
    cached
        .windows
        .iter()
        .max_by(|left, right| left.used_percent.total_cmp(&right.used_percent))
}

/// When an exhausted Account can be used again: the last of its full windows to
/// reset. `None` when any of them does not say, because the wait is then at
/// least that long and Perch will not guess at it.
fn frees_at(cached: &CachedUtilization) -> Option<DateTime<Utc>> {
    let mut last = None;
    for window in cached
        .windows
        .iter()
        .filter(|window| window.used_percent >= 100.0)
    {
        let resets_at = window.resets_at?;
        last = Some(last.map_or(resets_at, |last: DateTime<Utc>| last.max(resets_at)));
    }
    last
}

/// An Account with what ranking made of it.
struct Ranked<'a> {
    account: &'a Account,
    headroom: Headroom,
}

/// The Account a Cycle picked, with what it will say about having picked it.
#[derive(Debug)]
pub struct Choice {
    /// Owned, so the caller can go on to write the registry it came from.
    pub account: Account,
    /// Why this Account won, ready to print before the Switch.
    pub because: String,
    /// What the figures it won on cannot promise, ready to print after (ADR
    /// 0015). Absent when there were no figures to be stale.
    pub caveat: Option<String>,
}

/// Picks the Account to Switch to, or explains why none is worth switching to.
///
/// `leaving` is the Account Perch is on, when it is one of the candidates. It
/// is ranked like any other and never chosen: landing where you already are
/// would rewrite Credentials for nothing.
pub fn choose(
    registry: &Registry,
    scope: &Scope,
    leaving: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Choice> {
    let strategy = scope.strategy(registry);
    let accounts = scope.accounts(registry);
    if accounts.is_empty() {
        return Err(PerchError::NoCandidate(format!(
            "{} holds no Accounts, so there is nowhere to Cycle to. Nothing was \
             changed.",
            scope.described(),
        )));
    }

    let mut ranked: Vec<Ranked> = accounts
        .iter()
        .filter(|account| account.enabled && !account.quarantined())
        .map(|account| Ranked {
            account,
            headroom: headroom_of(account),
        })
        .collect();
    if ranked.is_empty() {
        return Err(PerchError::NoCandidate(nobody_is_a_candidate(
            scope, &accounts,
        )));
    }

    // Stable, so Accounts that rank identically stay in the order they were
    // added and the same command twice makes the same choice.
    ranked.sort_by(|left, right| {
        let (theirs, them) = right.headroom.ranking(strategy);
        let (ours, us) = left.headroom.ranking(strategy);
        theirs.cmp(&ours).then(them.total_cmp(&us))
    });

    if ranked.iter().all(|ranked| ranked.headroom.is_exhausted()) {
        return Err(PerchError::NoCandidate(everyone_is_exhausted(
            registry, scope, &ranked, now,
        )));
    }

    let here = ranked
        .iter()
        .find(|ranked| Some(ranked.account.email()) == leaving);
    let elsewhere: Vec<&Ranked> = ranked
        .iter()
        .filter(|ranked| Some(ranked.account.email()) != leaving && !ranked.headroom.is_exhausted())
        .collect();

    // Staying put is the right answer only when Perch can see that it is: an
    // Account it has never observed is not evidence that moving would gain
    // nothing, and out of the box no Account has been observed at all.
    if let Some(here) = here
        && let Headroom::Room { .. } = here.headroom
        && elsewhere
            .iter()
            .all(|other| other.headroom.ranking(strategy) <= here.headroom.ranking(strategy))
    {
        return Err(PerchError::NothingToDo(already_the_best(
            registry, scope, here, strategy, now,
        )));
    }

    // Everything else is exhausted and the Account Perch is on is not, which
    // leaves nowhere better to go even though it was never ranked against
    // anything.
    let Some(best) = elsewhere.first() else {
        let alone = here.expect("something unexhausted is here or elsewhere");
        return Err(PerchError::NothingToDo(format!(
            "{} is the only Account in {} that is not exhausted, and Perch has \
             never observed how full it is. Nothing was changed — {HOW_TO_GET_FIGURES}",
            registry.named_for_the_user(alone.account.email()),
            scope.described(),
        )));
    };

    Ok(Choice {
        account: best.account.clone(),
        because: chosen_because(registry, best, strategy, now),
        caveat: staleness(registry, best),
    })
}

/// Why the winner won, in the terms it was actually judged on — which is not
/// always the terms the Strategy asked for.
///
/// A choice Perch could not rank the way it was told to is said plainly rather
/// than dressed up as one it could: the user should know what the choice rested
/// on before they wonder why it filled up so fast.
fn chosen_because(
    registry: &Registry,
    best: &Ranked,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> String {
    let named = registry.named_for_the_user(best.account.email());
    let Some(figure) = best.headroom.as_a_clause(strategy, now) else {
        return format!(
            "Perch has never observed how full {named} is, so this was not a \
             ranked choice — {HOW_TO_GET_FIGURES}"
        );
    };
    match strategy {
        Strategy::MostHeadroom => format!("{named} has the most room: {figure}."),
        Strategy::SoonestReset if best.headroom.ranked_on_its_reset(strategy) => {
            format!("{named} resets soonest: {figure}.")
        }
        // Nothing that could be moved to says when it comes back — an Account
        // that did would have outranked this one — so the Cycle fell back to
        // the room it could see rather than switching on nothing.
        Strategy::SoonestReset => format!(
            "{named} has the most room: {figure}. No cached figure says when any \
             of them comes back, so there was no reset time to prefer one on — \
             {HOW_TO_GET_FIGURES}"
        ),
    }
}

/// What the cache cannot promise, said before the user finds out (ADR 0015).
fn staleness(registry: &Registry, best: &Ranked) -> Option<String> {
    let Headroom::Room { .. } = best.headroom else {
        return None;
    };
    Some(format!(
        "That figure is what Perch last observed rather than what Anthropic \
         says now. If {} turns out fuller than it implied, the figure was \
         stale — `perch status --refresh` reads a current one.",
        registry.named_for_the_user(best.account.email()),
    ))
}

/// The scope holds Accounts, but none of them is a candidate.
///
/// Each Account is counted once. One that is both disabled and Quarantined is
/// still one Account, and a tally that put it in both buckets could add up to
/// more Accounts than the scope holds — a reason that does not survive being
/// checked teaches the reader to stop checking.
fn nobody_is_a_candidate(scope: &Scope, accounts: &[&Account]) -> String {
    let quarantined = accounts.iter().filter(|a| a.quarantined()).count();
    let disabled = accounts
        .iter()
        .filter(|a| !a.enabled && !a.quarantined())
        .count();
    let mut why = Vec::new();
    if disabled > 0 {
        why.push(format!("{disabled} disabled"));
    }
    if quarantined > 0 {
        why.push(format!("{quarantined} Quarantined"));
    }
    format!(
        "No Account in {} is a Cycle candidate ({}), so there is nowhere to \
         Switch to. Nothing was changed.",
        scope.described(),
        why.join(", "),
    )
}

/// Every candidate is blocked. Naming the one that comes back soonest is the
/// whole of the answer: it lets the user decide to wait rather than being moved
/// somewhere useless.
fn everyone_is_exhausted(
    registry: &Registry,
    scope: &Scope,
    ranked: &[Ranked],
    now: DateTime<Utc>,
) -> String {
    let soonest = ranked
        .iter()
        .filter_map(|ranked| match ranked.headroom {
            Headroom::Exhausted {
                frees_at: Some(at), ..
            } => Some((at, ranked.account)),
            _ => None,
        })
        .min_by_key(|(at, _)| *at);
    let unsaid = ranked
        .iter()
        .filter(|ranked| matches!(ranked.headroom, Headroom::Exhausted { frees_at: None }))
        .count();

    let mut waiting = match soonest {
        Some((at, account)) => format!(
            "{} frees up soonest, at {}.",
            registry.named_for_the_user(account.email()),
            utilization::reset_phrase(at, now),
        ),
        // Every figure predates the recording of reset times, or none of them
        // carried one. Saying nothing would read as "never".
        None => format!("No cached figure says when any of them frees up — {HOW_TO_GET_FIGURES}"),
    };
    // An Account whose full window carries no reset time cannot be ranked for
    // how soon it comes back, and could well come back first. Leaving it out
    // silently would turn "the soonest Perch can vouch for" into advice to wait
    // longer than you have to.
    if unsaid > 0 && soonest.is_some() {
        waiting.push_str(&format!(
            " {unsaid} of them cache no reset time, so the wait could be \
             shorter than that — {HOW_TO_GET_FIGURES}"
        ));
    }

    format!(
        "Every Account in {} is exhausted, so there is nowhere useful to \
         Switch. Nothing was changed.\n{waiting}",
        scope.described(),
    )
}

/// Rewriting Credentials to land where you already are is the one thing a
/// Cycle can do that is worse than doing nothing.
fn already_the_best(
    registry: &Registry,
    scope: &Scope,
    here: &Ranked,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> String {
    let named = registry.named_for_the_user(here.account.email());
    let scope = scope.described();
    // Said of the comparison Perch actually made. Under soonest-reset it has
    // only compared the Accounts whose figures carry a reset time, so claiming
    // it beat the ones that do not would be claiming a comparison it could not
    // make.
    let standing = if here.headroom.ranked_on_its_reset(strategy) {
        format!(
            "{named} already comes back soonest of the Accounts in {scope} whose figures say when they do"
        )
    } else {
        format!("{named} is already the best Account in {scope}")
    };
    format!(
        "{standing}, with {}. Nothing was changed — {HOW_TO_GET_FIGURES}",
        here.headroom
            .as_a_clause(strategy, now)
            .expect("staying put is only said of a figure that says to"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::Quarantine;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()
    }

    /// A reason that does not survive being checked teaches the reader to stop
    /// checking, and "2 disabled, 2 Quarantined" out of two Accounts is exactly
    /// that.
    #[test]
    fn an_account_that_is_both_disabled_and_quarantined_is_counted_once() {
        let mut broken = account("broken@example.com", vec![]);
        broken.enabled = false;
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let mut reserved = account("reserved@example.com", vec![]);
        reserved.enabled = false;

        let said = nobody_is_a_candidate(&Scope::Ungrouped, &[&broken, &reserved]);

        assert!(said.contains("1 disabled"), "{said}");
        assert!(said.contains("1 Quarantined"), "{said}");
    }

    fn account(email: &str, windows: Vec<WindowUtilization>) -> Account {
        Account {
            identity: Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            enabled: true,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: (!windows.is_empty()).then(|| CachedUtilization {
                observed_at: now() - chrono::Duration::minutes(4),
                windows,
            }),
        }
    }

    fn window(name: &str, used_percent: f64) -> WindowUtilization {
        WindowUtilization {
            window: name.to_string(),
            used_percent,
            resets_at: None,
        }
    }

    fn resetting(name: &str, used_percent: f64, hours: i64) -> WindowUtilization {
        WindowUtilization {
            resets_at: Some(now() + chrono::Duration::hours(hours)),
            ..window(name, used_percent)
        }
    }

    fn holding(accounts: Vec<Account>) -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.active = accounts.first().map(|first| first.email().to_string());
        for account in accounts {
            registry.upsert(account);
        }
        registry
    }

    /// The same Group, told to prefer the other of the two Strategies.
    fn preferring(mut registry: Registry, strategy: crate::registry::Strategy) -> Registry {
        registry.groups.get_mut("work").expect("declared").strategy = strategy;
        registry
    }

    fn cycle(registry: &Registry) -> Result<Choice> {
        choose(
            registry,
            &Scope::Group("work".to_string()),
            registry.active.as_deref(),
            now(),
        )
    }

    #[test]
    fn headroom_is_the_room_left_in_the_fullest_window() {
        let headroom = headroom_of(&account(
            "a@example.com",
            vec![window("5-hour", 4.0), window("7-day", 95.0)],
        ));
        assert!(
            matches!(
                headroom,
                Headroom::Room {
                    percent: 5.0,
                    ref fullest_window,
                    ..
                } if fullest_window == "7-day"
            ),
            "the window with the least room decides, so the number is true of \
             every window: {headroom:?}"
        );
    }

    #[test]
    fn a_full_window_exhausts_an_account_however_empty_its_others_are() {
        let headroom = headroom_of(&account(
            "a@example.com",
            vec![window("5-hour", 0.0), window("7-day", 100.0)],
        ));
        assert!(headroom.is_exhausted());
    }

    #[test]
    fn an_exhausted_account_frees_up_when_its_last_full_window_resets() {
        let headroom = headroom_of(&account(
            "a@example.com",
            vec![
                resetting("5-hour", 100.0, 1),
                resetting("7-day", 100.0, 50),
                // Not full, so its reset has no bearing on the wait.
                resetting("7-day-opus", 3.0, 100),
            ],
        ));
        assert_eq!(
            headroom,
            Headroom::Exhausted {
                frees_at: Some(now() + chrono::Duration::hours(50))
            }
        );
    }

    #[test]
    fn a_full_window_that_does_not_say_when_it_resets_leaves_the_wait_unknown() {
        let headroom = headroom_of(&account(
            "a@example.com",
            vec![window("5-hour", 100.0), resetting("7-day", 100.0, 3)],
        ));
        assert_eq!(headroom, Headroom::Exhausted { frees_at: None });
    }

    #[test]
    fn an_account_with_no_figure_is_ranked_below_one_with_room_and_above_one_that_is_full() {
        let over_a_figure = holding(vec![
            account("here@example.com", vec![window("5-hour", 100.0)]),
            account("unobserved@example.com", vec![]),
            account("nearly-full@example.com", vec![window("5-hour", 99.0)]),
        ]);
        assert_eq!(
            cycle(&over_a_figure)
                .expect("there is somewhere to go")
                .account
                .email(),
            "nearly-full@example.com",
            "1% of room that Perch has seen beats an unknown it has not"
        );

        let over_a_full_window = holding(vec![
            account("here@example.com", vec![window("5-hour", 100.0)]),
            account("full@example.com", vec![window("5-hour", 100.0)]),
            account("unobserved@example.com", vec![]),
        ]);
        assert_eq!(
            cycle(&over_a_full_window)
                .expect("there is somewhere to go")
                .account
                .email(),
            "unobserved@example.com",
            "an unknown beats a window that is certainly full"
        );
    }

    #[test]
    fn staying_put_is_only_the_answer_when_perch_can_see_that_it_is() {
        let observed = holding(vec![
            account("here@example.com", vec![window("5-hour", 10.0)]),
            account("there@example.com", vec![window("5-hour", 50.0)]),
        ]);
        assert!(matches!(cycle(&observed), Err(PerchError::NothingToDo(_))));

        let unobserved = holding(vec![
            account("here@example.com", vec![]),
            account("there@example.com", vec![]),
        ]);
        assert_eq!(
            cycle(&unobserved)
                .expect("nothing observed is no reason to stay")
                .account
                .email(),
            "there@example.com",
        );
    }

    #[test]
    fn a_tie_between_observed_accounts_keeps_the_one_it_is_on() {
        let registry = holding(vec![
            account("here@example.com", vec![window("5-hour", 42.0)]),
            account("there@example.com", vec![window("5-hour", 42.0)]),
        ]);

        let error = cycle(&registry).expect_err("moving would gain nothing");
        assert_eq!(error.exit_code(), crate::error::EXIT_NOTHING_TO_DO);
    }

    #[test]
    fn an_account_whose_wait_is_unknown_is_not_left_out_of_the_all_exhausted_answer_silently() {
        let registry = holding(vec![
            account("here@example.com", vec![resetting("5-hour", 100.0, 3)]),
            account("known@example.com", vec![resetting("7-day", 100.0, 9)]),
            // Full, and its figure carries no reset time — so it could be the
            // one that comes back first and there is no way to tell.
            account("unsaid@example.com", vec![window("5-hour", 100.0)]),
        ]);

        let error = cycle(&registry).expect_err("there is nowhere useful to go");

        assert!(error.to_string().contains("here@example.com"), "{error}");
        assert!(
            error.to_string().contains("1 of them cache no reset time"),
            "advising a three-hour wait while one Account may be back sooner is \
             worse than saying so: {error}"
        );
    }

    #[test]
    fn the_soonest_resetting_account_wins_where_the_group_prefers_perishable_quota() {
        let accounts = vec![
            account("here@example.com", vec![resetting("5-hour", 96.0, 2)]),
            account("roomiest@example.com", vec![resetting("5-hour", 5.0, 8)]),
            account("soonest@example.com", vec![resetting("5-hour", 60.0, 1)]),
        ];
        let registry = holding(accounts);

        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "roomiest@example.com",
            "the default Strategy prefers the most room"
        );

        let soonest = preferring(registry, Strategy::SoonestReset);
        let choice = cycle(&soonest).expect("there is room");

        assert_eq!(
            choice.account.email(),
            "soonest@example.com",
            "quota an hour from being thrown away costs nothing to spend"
        );
        assert!(
            choice.because.contains("resets soonest"),
            "{}",
            choice.because
        );
    }

    #[test]
    fn a_figure_with_no_reset_time_is_never_read_as_the_soonest_to_reset() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![window("5-hour", 60.0)]),
                // Less room, but it is the only Account whose figure says when
                // its quota comes back — so it is the only one that can be
                // ranked on when its quota comes back.
                account("says@example.com", vec![resetting("5-hour", 90.0, 5)]),
            ]),
            Strategy::SoonestReset,
        );

        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "says@example.com",
            "an unknown reset is not evidence of an imminent one"
        );
    }

    #[test]
    fn with_no_reset_time_anywhere_soonest_reset_falls_back_to_the_room_it_can_see() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![window("5-hour", 90.0)]),
                account("middling@example.com", vec![window("5-hour", 50.0)]),
                account("roomiest@example.com", vec![window("5-hour", 5.0)]),
            ]),
            Strategy::SoonestReset,
        );

        let choice = cycle(&registry).expect("there is room to move to");

        assert_eq!(
            choice.account.email(),
            "roomiest@example.com",
            "a Strategy says which figure to prefer, not which figures to \
             invent: with no reset time to rank on, switching on the order the \
             Accounts were added would be switching on nothing"
        );
        assert!(
            choice.because.contains("no reset time to prefer one on"),
            "and the fallback is said rather than passed off as the ranking \
             that was asked for: {}",
            choice.because
        );
    }

    #[test]
    fn that_fallback_can_still_conclude_there_is_nowhere_better_to_go() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![window("5-hour", 10.0)]),
                account("fuller@example.com", vec![window("5-hour", 50.0)]),
            ]),
            Strategy::SoonestReset,
        );

        let error = cycle(&registry).expect_err("moving would land somewhere fuller");

        assert_eq!(error.exit_code(), crate::error::EXIT_NOTHING_TO_DO);
        assert!(
            !error.to_string().contains("comes back soonest"),
            "nothing said when anything comes back, so nothing may claim to \
             come back soonest: {error}"
        );
    }

    #[test]
    fn a_scope_where_nobody_is_a_candidate_says_which_way_each_account_left_it() {
        let mut registry = holding(vec![
            account("here@example.com", vec![window("5-hour", 100.0)]),
            account("off@example.com", vec![window("5-hour", 1.0)]),
            account("broken@example.com", vec![window("5-hour", 1.0)]),
        ]);
        registry.account_mut("here@example.com").unwrap().enabled = false;
        registry.account_mut("off@example.com").unwrap().enabled = false;
        registry.quarantine("broken@example.com", Quarantine::RenewalRejected);

        let error = cycle(&registry).expect_err("nobody is a candidate");

        assert_eq!(error.exit_code(), crate::error::EXIT_NO_CANDIDATE);
        assert!(error.to_string().contains("2 disabled"), "{error}");
        assert!(error.to_string().contains("1 Quarantined"), "{error}");
    }
}
