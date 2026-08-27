//! What a Scope has left to draw on, said without inventing a number.
//!
//! The **Reserve** is how many of a Scope's Accounts still have Headroom and how
//! much the best of them has — a count and one Account's own figure, never a
//! pooled one, because a Scope has no single figure to pool
//! (ADR headroom-is-the-worst-window).
//!
//! Said only under a heading that has already named the Scope, which is a
//! narrowed `perch list` and nothing else on the human surface
//! (ADR the-listing-owns-the-set); a `--json` section names its Scope in a key.

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::cycle::{self, HowMuchIsLeft};
use crate::registry::{Account, Registry, Scope};
use crate::say;
use crate::utilization;

/// What a Scope has left to draw on.
///
/// Held over the Accounts a Cycle may land on: a Quarantined Credential does not
/// work and a Disabled Account is never chosen, so counting either would count
/// quota nothing can spend. Both are still said, as what is out of the running.
pub struct Reserve<'a> {
    /// The Accounts a Cycle may land on, in the order the registry holds them.
    /// Deliberately not the Cycle's ranking — `best` reads the separately sorted
    /// `with_headroom` and the rest are counts — because claiming an order
    /// nothing establishes invites the next caller to rely on it.
    candidates: Vec<&'a Account>,
    /// Those of them with Headroom, best first — an Account and the room in its
    /// fullest Quota Window.
    with_headroom: Vec<(&'a Account, f64)>,
    /// Candidates whose fullest window is full.
    exhausted: usize,
    /// Candidates Perch has never read a figure for.
    unobserved: usize,
    /// How many Accounts the Scope holds that a Cycle may not choose at all, and
    /// which way each left the running — counted by [`cycle::out_of_the_running`],
    /// so this says what the refusal to Cycle says.
    not_candidates: usize,
    out_of_the_running: String,
}

impl<'a> Reserve<'a> {
    /// What one Scope has left, read from the cache alone
    /// (ADR a-figure-carries-its-age). Every candidate is classified exactly
    /// once into one of [`HowMuchIsLeft`]'s three answers, which is what makes
    /// the counts add up to the Accounts on screen.
    pub fn of(registry: &'a Registry, scope: &Scope) -> Reserve<'a> {
        let accounts = scope.accounts(registry);
        let sharers = crate::registry::Sharers::across(registry);
        let candidates: Vec<&Account> = accounts
            .iter()
            .copied()
            .filter(|account| cycle::is_a_candidate(&sharers, account))
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
        // Best first, stable, so a tie keeps the registry's order. Deliberately
        // not a claim about where a Cycle would land: under `soonest-reset` the
        // Account with the most room is not the one a Cycle prefers.
        with_headroom.sort_by(|(_, ours), (_, theirs)| theirs.total_cmp(ours));

        Reserve {
            exhausted,
            unobserved,
            not_candidates: accounts.len() - candidates.len(),
            out_of_the_running: cycle::out_of_the_running(&sharers, &accounts),
            candidates,
            with_headroom,
        }
    }

    /// The Reserve as it is read: a count, and the best Account's own figure with
    /// the age of the observation it came from. Where nothing is left, the answer
    /// is what is in the way — exhausted, never observed, or out of the running —
    /// because "none" without a reason is a Scope somebody stares at wondering
    /// which.
    pub fn lines(&self, now: DateTime<Utc>) -> Vec<String> {
        let mut lines = vec![match self.best() {
            // The best Account's own age, read off the observation its Headroom
            // was measured from — which is why there is one to read at all.
            Some((_, percent)) => format!(
                "Reserve: {} of {} {} Headroom, the best {}% left (as of {})",
                self.with_headroom.len(),
                say::accounts(self.candidates.len()),
                verb(self.with_headroom.len()),
                utilization::percentage(percent),
                utilization::age_phrase(
                    self.best_read_at()
                        .expect("Headroom is measured from a reading"),
                    now,
                ),
            ),
            // No age: being Disabled or Quarantined is a fact about the
            // registry rather than an observation. The parenthetical is never
            // empty — an empty Scope is the listing's own sentence, said first.
            None if self.candidates.is_empty() => format!(
                "Reserve: none — no Account here may be Cycled to ({})",
                self.out_of_the_running,
            ),
            None => format!(
                "Reserve: none of {} {} Headroom ({})",
                say::accounts(self.candidates.len()),
                verb(self.candidates.len()),
                self.why_not(),
            ),
        }];

        // The oldest reading the count rests on, which is the weakest thing it
        // rests on — the age already shown belongs to the best Account alone. On
        // a line of its own, because the sentence above is as wide as a terminal.
        if let Some(oldest) = self.oldest_reading()
            && self.best_read_at() != Some(oldest)
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

    /// The same facts as a script reads them: fields rather than the sentence
    /// [`Reserve::lines`] renders, since a prose sentence in a document is a
    /// thing scripts end up regexing. Every count is over the Accounts a Cycle
    /// may choose, so they add up to the Accounts in the section beside it, and
    /// which way each left the running is not counted a second time here.
    pub fn document(&self) -> serde_json::Value {
        json!({
            "candidates": self.candidates.len(),
            "with_headroom": self.with_headroom.len(),
            "exhausted": self.exhausted,
            "never_observed": self.unobserved,
            "out_of_the_running": self.not_candidates,
            // One Account's own figure, named. `null` where nothing here has
            // Headroom, which is a state rather than nought room. Unrounded,
            // like every percentage in a document.
            "best": self.best().map(|(account, percent)| json!({
                "email": account.email(),
                "percent": percent,
                "observed_at": self.best_read_at().map(|at| at.to_rfc3339()),
            })),
            // The weakest reading the counts above rest on, which is what the
            // sentence quotes. `null` where no candidate has ever been read.
            "oldest_observed_at": self.oldest_reading().map(|at| at.to_rfc3339()),
        })
    }

    /// The Account with the most Headroom, and how much.
    fn best(&self) -> Option<(&'a Account, f64)> {
        self.with_headroom.first().copied()
    }

    /// When the figure the Reserve quotes was read.
    fn best_read_at(&self) -> Option<DateTime<Utc>> {
        let (account, _) = self.best()?;
        Some(account.observed_utilization()?.observed_at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::tests::{account, holding, now, window};
    use crate::registry::Quarantine;

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    fn reserve_of(registry: &Registry) -> Vec<String> {
        Reserve::of(registry, &work()).lines(now())
    }

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

    #[test]
    fn the_age_beside_a_count_is_the_oldest_reading_it_rests_on() {
        let mut stale = account("stale@example.com", vec![window("5-hour", 60.0)]);
        stale.utilization.as_mut().expect("it was read").observed_at =
            now() - chrono::Duration::hours(8);
        let registry = holding(vec![
            stale,
            account("fresh@example.com", vec![window("5-hour", 7.0)]),
        ]);

        let said = reserve_of(&registry);

        assert!(
            said[0].contains("2 of 2 Accounts have Headroom") && said[0].contains("as of 4m ago"),
            "the best Account's own figure carries its own age: {said:?}"
        );
        assert_eq!(
            said.get(1).map(String::as_str),
            Some("Read 8h ago at the oldest."),
            "and the count says the weakest thing it rests on: {said:?}"
        );
    }

    #[test]
    fn a_scope_with_nothing_left_says_what_is_in_the_way() {
        let registry = holding(vec![
            account("full@example.com", vec![window("5-hour", 100.0)]),
            account("unread@example.com", vec![]),
        ]);

        assert_eq!(
            reserve_of(&registry),
            [
                "Reserve: none of 2 Accounts have Headroom (1 exhausted, 1 never observed)",
                "Read 4m ago at the oldest.",
            ]
        );
    }

    #[test]
    fn what_a_cycle_may_not_choose_is_not_counted_as_something_the_scope_has() {
        let mut spared = account("spared@example.com", vec![window("5-hour", 0.0)]);
        spared.disabled = true;
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

    #[test]
    fn a_scope_nothing_may_cycle_to_says_that_rather_than_counting_to_zero() {
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);

        assert_eq!(
            reserve_of(&holding(vec![broken])),
            ["Reserve: none — no Account here may be Cycled to (1 Quarantined)"]
        );
    }

    /// The counts add up to the Accounts beside them, which is what makes the
    /// document checkable against its own `accounts` array.
    #[test]
    fn the_document_says_the_same_counts_the_sentence_does() {
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let mut stale = account("stale@example.com", vec![window("5-hour", 90.0)]);
        stale.utilization.as_mut().expect("it was read").observed_at =
            now() - chrono::Duration::hours(8);
        let registry = holding(vec![
            account("fresh@example.com", vec![window("5-hour", 7.0)]),
            stale,
            account("full@example.com", vec![window("5-hour", 100.0)]),
            account("unread@example.com", vec![]),
            broken,
        ]);

        let document = Reserve::of(&registry, &work()).document();

        assert_eq!(document["candidates"], 4);
        assert_eq!(document["with_headroom"], 2);
        assert_eq!(document["exhausted"], 1);
        assert_eq!(document["never_observed"], 1);
        assert_eq!(document["out_of_the_running"], 1);
        assert_eq!(document["best"]["email"], "fresh@example.com");
        assert_eq!(
            document["best"]["percent"], 93.0,
            "unrounded, and one Account's own: {document}"
        );
        assert_eq!(
            document["best"]["observed_at"],
            (now() - chrono::Duration::minutes(4)).to_rfc3339(),
            "the figure carries the age of the reading it came from: {document}"
        );
        assert_eq!(
            document["oldest_observed_at"],
            (now() - chrono::Duration::hours(8)).to_rfc3339(),
            "and the counts carry the weakest reading they rest on: {document}"
        );
    }

    #[test]
    fn a_document_reports_no_figure_for_a_scope_nothing_was_ever_read_for() {
        let registry = holding(vec![
            account("unread@example.com", vec![]),
            account("also-unread@example.com", vec![]),
        ]);

        let document = Reserve::of(&registry, &work()).document();

        assert_eq!(document["candidates"], 2);
        assert_eq!(document["never_observed"], 2);
        assert!(document["best"].is_null(), "{document}");
        assert!(document["oldest_observed_at"].is_null(), "{document}");
    }
}
