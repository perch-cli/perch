//! What a Group has left to draw on, said without inventing a number.
//!
//! One Account is measured by its most constrained Quota Window, and that figure
//! is its Headroom (ADR 0012). A Group has no equivalent single figure and never
//! will: its Accounts sit on different plans, Perch only ever sees percentages,
//! and a `pro` Account at 50% and a `max` Account at 50% do not have the same
//! quota left. Summing or averaging them produces a number that looks
//! quantitative, is not, and is exactly the kind of number people plan around.
//!
//! So a Group is said two ways, and every figure in both of them is one an
//! Account actually reported rather than one Perch worked out.
//!
//! The **Reserve** is how many of a Group's Accounts still have Headroom and how
//! much the best of them has — a count and one Account's figure.
//!
//! The **per-window rows** are the same discipline one Quota Window kind at a
//! time: the emptiest Account in that window, and how many were read for it.
//! Within one window kind the comparison at least means something — that is what
//! answers "this Group is fine on the weekly window and empty on the five-hour
//! one", which is the case a single figure per Account hides — so it is the only
//! figure Perch draws across a Group's Accounts at all.
//!
//! Fill and room are both percentages and are never left to be told apart by
//! context: a Reserve says how much is *left* and a per-window row says how much
//! is *used*, and the row says the word.

use chrono::{DateTime, Utc};

use crate::cycle::{self, HowMuchIsLeft, Scope};
use crate::registry::{Account, Registry};
use crate::utilization;

/// What a Group has left to draw on.
///
/// Held over the Accounts a Cycle may land on, because drawing on a Group is
/// what Cycling within it does: a Quarantined Account's Credential does not
/// work and a Disabled one is never chosen, so counting either as something the
/// Group still has would be counting quota nothing can spend. They are still
/// listed, and still said here — as what is out of the running rather than as
/// part of what is left.
pub struct Reserve<'a> {
    /// The Accounts a Cycle may land on, in the order the scope ranks them.
    candidates: Vec<&'a Account>,
    /// Those of them with Headroom, best first — an Account and the room in its
    /// fullest Quota Window.
    with_headroom: Vec<(&'a Account, f64)>,
    /// Candidates whose fullest window is full.
    exhausted: usize,
    /// Candidates Perch has never read a figure for.
    unobserved: usize,
    /// How many Accounts the scope holds that a Cycle may not choose at all,
    /// and which way each of them left the running — counted by
    /// [`cycle::out_of_the_running`], so this says what the refusal to Cycle
    /// says.
    not_candidates: usize,
    out_of_the_running: String,
}

impl<'a> Reserve<'a> {
    /// What one scope has left, read from the cache alone (ADR 0015).
    ///
    /// Every candidate is classified exactly once, into exactly one of the three
    /// answers [`HowMuchIsLeft`] has. That is what makes the counts add up to
    /// the Accounts on screen: two passes asking two questions could disagree,
    /// and a tally nobody can check is a tally nobody should read.
    pub fn of(registry: &'a Registry, scope: &Scope) -> Reserve<'a> {
        let accounts = scope.accounts(registry);
        let candidates: Vec<&Account> = accounts
            .iter()
            .copied()
            .filter(|account| cycle::is_a_candidate(account))
            .collect();

        let mut with_headroom = Vec::new();
        let mut exhausted = 0;
        let mut unobserved = 0;
        for account in &candidates {
            match cycle::how_much_is_left(account) {
                HowMuchIsLeft::Room(percent) => with_headroom.push((*account, percent)),
                HowMuchIsLeft::Exhausted => exhausted += 1,
                HowMuchIsLeft::NeverObserved => unobserved += 1,
            }
        }
        // Best first. Stable, so a tie keeps the order the registry holds them
        // in and the same Group says the same thing twice — which is all that is
        // claimed here. It is deliberately *not* a claim about where a Cycle
        // would land: under `soonest-reset` the Account with the most room is
        // not the one a Cycle prefers, and a Reserve is about what is there
        // rather than about which of it gets chosen.
        with_headroom.sort_by(|(_, ours), (_, theirs)| theirs.total_cmp(ours));

        Reserve {
            exhausted,
            unobserved,
            not_candidates: accounts.len() - candidates.len(),
            out_of_the_running: cycle::out_of_the_running(&accounts),
            candidates,
            with_headroom,
        }
    }

    /// The Reserve as it is read: a count, and the best Account's own figure
    /// with the age of the observation it came from.
    ///
    /// Never one pooled figure, whichever way it falls. Where nothing is left
    /// the answer is what is in the way — exhausted, never observed, or out of
    /// the running — because "none" without a reason is a Group somebody stares
    /// at wondering which.
    pub fn lines(&self, now: DateTime<Utc>) -> Vec<String> {
        let mut lines = vec![match self.best() {
            Some((account, percent)) => format!(
                "Reserve: {} of {} {} Headroom, the best {}% left ({})",
                self.with_headroom.len(),
                accounts(self.candidates.len()),
                verb(self.with_headroom.len()),
                crate::utilization::percentage(percent),
                observed(account, now),
            ),
            // Nothing was read to reach this one: being Disabled or Quarantined
            // is a fact about the registry rather than an observation, so there
            // is no age to carry and none is invented.
            None if self.candidates.is_empty() => format!(
                "Reserve: none — no Account here may be Cycled to{}",
                match self.out_of_the_running.is_empty() {
                    true => String::new(),
                    false => format!(" ({})", self.out_of_the_running),
                }
            ),
            None => format!(
                "Reserve: none of {} {} Headroom ({})",
                accounts(self.candidates.len()),
                verb(self.candidates.len()),
                self.why_not(),
            ),
        }];

        // The count above is read from cache like every other figure, so it says
        // how old the readings behind it are (ADR 0015). The oldest of them,
        // because that is the weakest thing the count rests on — and on a line
        // of its own, because the sentence above is already as long as a
        // terminal is wide.
        if self.best().is_none()
            && let Some(oldest) = self.oldest_reading()
        {
            lines.push(format!(
                "Read {} at the oldest.",
                utilization::age_phrase(oldest, now)
            ));
        }

        // Said on a line of its own for the same reason, and only when there is
        // something for it to say.
        if !self.candidates.is_empty() && !self.out_of_the_running.is_empty() {
            lines.push(format!(
                "{}, so nothing Cycles to {}.",
                self.out_of_the_running,
                if self.not_candidates == 1 {
                    "it"
                } else {
                    "them"
                }
            ));
        }
        lines
    }

    /// One row per Quota Window kind: the emptiest Account in it, how many were
    /// read for it, and the age of the figure quoted.
    ///
    /// The figure is one Account's own, so it is the best that window can
    /// currently offer rather than an average of Accounts that cannot be added
    /// up. In the order the windows were first seen, which is the order
    /// Anthropic reports them in.
    ///
    /// Nothing at all below two observed candidates: with one, the row is that
    /// Account's row said twice.
    pub fn window_lines(&self, now: DateTime<Utc>) -> Vec<String> {
        let read_for: Vec<&Account> = self
            .candidates
            .iter()
            .copied()
            .filter(|account| account.observed_utilization().is_some())
            .collect();
        if read_for.len() < 2 {
            return Vec::new();
        }

        let kinds = window_kinds(&read_for);
        let width = utilization::window_width(kinds.iter().map(String::as_str));
        let mut lines = Vec::new();
        for kind in kinds {
            let mut emptiest: Option<(&Account, f64)> = None;
            let mut read = 0;
            for account in &read_for {
                let cached = account
                    .observed_utilization()
                    .expect("every Account here carries an observation");
                let Some(window) = cached.windows.iter().find(|held| held.window == kind) else {
                    continue;
                };
                read += 1;
                if emptiest.is_none_or(|(_, least)| window.used_percent < least) {
                    emptiest = Some((account, window.used_percent));
                }
            }
            let Some((account, used_percent)) = emptiest else {
                continue;
            };
            // "used", because the line above it says how much is *left*: two
            // percentages of the same window an inch apart, and the reader is
            // not asked to tell them apart by context.
            lines.push(format!(
                "{kind:<width$} emptiest {:>3}% used across {} ({})",
                crate::utilization::percentage(used_percent),
                accounts(read),
                observed(account, now),
            ));
        }
        lines
    }

    /// The Account with the most Headroom, and how much.
    fn best(&self) -> Option<(&'a Account, f64)> {
        self.with_headroom.first().copied()
    }

    /// Why no candidate has Headroom, counted the two ways it happens.
    fn why_not(&self) -> String {
        let mut why = Vec::new();
        if self.exhausted > 0 {
            why.push(format!("{} exhausted", self.exhausted));
        }
        if self.unobserved > 0 {
            why.push(format!("{} never observed", self.unobserved));
        }
        why.join(", ")
    }

    /// When the least recently read of the candidates was read, or `None` where
    /// none of them ever has been.
    fn oldest_reading(&self) -> Option<DateTime<Utc>> {
        self.candidates
            .iter()
            .filter_map(|account| Some(account.observed_utilization()?.observed_at))
            .min()
    }
}

/// "has" for one Account and "have" for any other number of them.
fn verb(count: usize) -> &'static str {
    match count {
        1 => "has",
        _ => "have",
    }
}

/// Every Quota Window kind these Accounts were read for, in the order they were
/// first seen.
fn window_kinds(accounts: &[&Account]) -> Vec<String> {
    let mut kinds: Vec<String> = Vec::new();
    for account in accounts {
        let cached = account
            .observed_utilization()
            .expect("every Account here carries an observation");
        for window in &cached.windows {
            if !kinds.contains(&window.window) {
                kinds.push(window.window.clone());
            }
        }
    }
    kinds
}

/// "3 Accounts", "1 Account" — a count that reads as a sentence rather than as
/// a number with a plural bolted on.
fn accounts(count: usize) -> String {
    match count {
        1 => "1 Account".to_string(),
        _ => format!("{count} Accounts"),
    }
}

/// When the figure being quoted was read (ADR 0015), for the Account it was read
/// for.
fn observed(account: &Account, now: DateTime<Utc>) -> String {
    match account.observed_utilization() {
        Some(cached) => format!("as of {}", utilization::age_phrase(cached.observed_at, now)),
        None => "never observed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::tests::{account, holding, now, resetting, window};
    use crate::registry::Quarantine;

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    fn reserve_of(registry: &Registry) -> Vec<String> {
        Reserve::of(registry, &work()).lines(now())
    }

    fn windows_of(registry: &Registry) -> Vec<String> {
        Reserve::of(registry, &work()).window_lines(now())
    }

    /// The count and one Account's own figure — never a pooled one.
    #[test]
    fn a_reserve_is_a_count_and_the_best_accounts_own_figure() {
        let registry = holding(vec![
            account("tired@example.com", vec![window("5-hour", 90.0)]),
            account("fresh@example.com", vec![window("5-hour", 7.0)]),
            account("full@example.com", vec![window("5-hour", 100.0)]),
        ]);

        assert_eq!(
            reserve_of(&registry),
            ["Reserve: 2 of 3 Accounts have Headroom, the best 93% left (as of 4m ago)"]
        );
    }

    /// A `pro` Account at 50% and a `max` Account at 50% do not have the same
    /// quota left, and Perch never sees the allowance behind either — so the
    /// only figure it may quote is one an Account reported.
    #[test]
    fn no_figure_in_a_reserve_is_one_no_account_reported() {
        let registry = holding(vec![
            account("one@example.com", vec![window("5-hour", 20.0)]),
            account("two@example.com", vec![window("5-hour", 60.0)]),
        ]);

        let said = reserve_of(&registry).join(" ");

        assert!(said.contains("the best 80% left"), "{said}");
        for pooled in ["60%", "40%", "120%"] {
            assert!(
                !said.contains(pooled),
                "{pooled} is a figure no Account reported: {said}"
            );
        }
    }

    /// "None" without a reason is a Group somebody stares at wondering which.
    #[test]
    fn a_group_with_nothing_left_says_what_is_in_the_way() {
        let registry = holding(vec![
            account("full@example.com", vec![window("5-hour", 100.0)]),
            account("unread@example.com", vec![]),
        ]);

        assert_eq!(
            reserve_of(&registry),
            [
                "Reserve: none of 2 Accounts have Headroom (1 exhausted, 1 never observed)",
                // Counted from cache like every other figure, so it says how
                // old the readings behind it are.
                "Read 4m ago at the oldest.",
            ]
        );
    }

    /// A Quarantined Credential does not work and a Disabled Account is never
    /// chosen, so neither is part of what a Group has left — and both are said,
    /// because a count that quietly dropped them would not add up to the
    /// Accounts on screen.
    #[test]
    fn what_a_cycle_may_not_choose_is_not_counted_as_something_the_group_has() {
        let mut spared = account("spared@example.com", vec![window("5-hour", 0.0)]);
        spared.enabled = false;
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let registry = holding(vec![
            account("usable@example.com", vec![window("5-hour", 40.0)]),
            spared,
            broken,
        ]);

        assert_eq!(
            reserve_of(&registry),
            [
                "Reserve: 1 of 1 Account has Headroom, the best 60% left (as of 4m ago)",
                "1 disabled, 1 Quarantined, so nothing Cycles to them.",
            ]
        );
    }

    /// Counting to "0 of 0" would be arithmetic about a Group nothing may
    /// reach. What is in the way is the answer instead.
    #[test]
    fn a_group_nothing_may_cycle_to_says_that_rather_than_counting_to_zero() {
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);

        assert_eq!(
            reserve_of(&holding(vec![broken])),
            ["Reserve: none — no Account here may be Cycled to (1 Quarantined)"]
        );
    }

    /// The case a single figure per Account hides: fine on one window, empty on
    /// another. Each row quotes the emptiest Account in that window, which is
    /// the best the Group can currently offer there.
    #[test]
    fn each_quota_window_kind_gets_a_row_of_its_own() {
        let registry = holding(vec![
            account(
                "one@example.com",
                vec![window("5-hour", 96.0), window("7-day", 30.0)],
            ),
            account(
                "two@example.com",
                vec![window("5-hour", 91.0), window("7-day", 12.0)],
            ),
        ]);

        assert_eq!(
            windows_of(&registry),
            [
                "5-hour emptiest  91% used across 2 Accounts (as of 4m ago)",
                "7-day  emptiest  12% used across 2 Accounts (as of 4m ago)",
            ]
        );
    }

    /// A window only some Accounts were read for says how many, rather than
    /// implying the Group was read for all of them.
    #[test]
    fn a_window_row_counts_the_accounts_it_was_actually_read_for() {
        let registry = holding(vec![
            account(
                "one@example.com",
                vec![window("5-hour", 40.0), window("7-day-opus", 8.0)],
            ),
            account("two@example.com", vec![window("5-hour", 60.0)]),
            account("three@example.com", vec![window("5-hour", 70.0)]),
        ]);

        assert_eq!(
            windows_of(&registry),
            [
                "5-hour     emptiest  40% used across 3 Accounts (as of 4m ago)",
                "7-day-opus emptiest   8% used across 1 Account (as of 4m ago)",
            ]
        );
    }

    /// With one Account read, the row is that Account's row said twice.
    #[test]
    fn a_scope_with_one_observed_account_gets_no_per_window_rows_at_all() {
        let registry = holding(vec![
            account("one@example.com", vec![resetting("5-hour", 40.0, 3)]),
            account("unread@example.com", vec![]),
        ]);

        assert!(windows_of(&registry).is_empty());
    }
}
