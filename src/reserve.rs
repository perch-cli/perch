//! What a Scope has left to draw on, said without inventing a number.
//!
//! One Account is measured by its most constrained Quota Window, and that figure
//! is its Headroom (ADR 0012). A Scope has no equivalent single figure and never
//! will: its Accounts sit on different plans, Perch only ever sees percentages,
//! and a `pro` Account at 50% and a `max` Account at 50% do not have the same
//! quota left. Summing or averaging them produces a number that looks
//! quantitative, is not, and is exactly the kind of number people plan around.
//!
//! So the **Reserve** is how many of a Scope's Accounts still have Headroom and
//! how much the best of them has — a count and one Account's own figure, every
//! part of it something an Account actually reported rather than something Perch
//! worked out.
//!
//! Said only where a heading has already named the Scope it is about, which is a
//! narrowed `perch list` and nothing else on the human surface (ADR 0058). A
//! `--json` section names its own Scope in a key, so every one of them carries
//! it.

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::commands::accounts;
use crate::cycle::{self, HowMuchIsLeft};
use crate::registry::{Account, Registry, Scope};
use crate::utilization;

/// What a Scope has left to draw on.
///
/// Held over the Accounts a Cycle may land on, because drawing on a Scope is
/// what Cycling within it does: a Quarantined Account's Credential does not
/// work and a Disabled one is never chosen, so counting either as something the
/// Scope still has would be counting quota nothing can spend. They are still
/// listed, and still said here — as what is out of the running rather than as
/// part of what is left.
pub struct Reserve<'a> {
    /// The Accounts a Cycle may land on, in the order the registry holds them.
    /// Nothing here reads them in order — `best` reads the separately sorted
    /// `with_headroom`, and the rest are counts — so this is deliberately not
    /// the Cycle's ranking: claiming an order nothing establishes is an
    /// invitation for the next caller to rely on one that is not there.
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
        // in and the same Scope says the same thing twice — which is all that is
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
    /// the running — because "none" without a reason is a Scope somebody stares
    /// at wondering which.
    pub fn lines(&self, now: DateTime<Utc>) -> Vec<String> {
        let mut lines = vec![match self.best() {
            // The age is the best Account's own, read off the observation its
            // Headroom was measured from — which is why there is one to read:
            // Room is what an Account with a figure has, and the two absences
            // [`HowMuchIsLeft`] tells apart are the other two arms.
            Some((_, percent)) => format!(
                "Reserve: {} of {} {} Headroom, the best {}% left (as of {})",
                self.with_headroom.len(),
                accounts(self.candidates.len()),
                verb(self.with_headroom.len()),
                utilization::percentage(percent),
                utilization::age_phrase(
                    self.best_read_at()
                        .expect("Headroom is measured from a reading"),
                    now,
                ),
            ),
            // Nothing was read to reach this one: being Disabled or Quarantined
            // is a fact about the registry rather than an observation, so there
            // is no age to carry and none is invented.
            //
            // Something is always in the way here, and the parenthetical is not
            // guarded against being empty. Candidates come out empty only by
            // every Account the Scope holds leaving the running; a Scope holding
            // no Account at all would reach this branch with nothing to name,
            // and it cannot — the listing says its own better sentence, that the
            // Scope holds no Accounts yet, and returns before any of this is
            // asked for.
            None if self.candidates.is_empty() => format!(
                "Reserve: none — no Account here may be Cycled to ({})",
                self.out_of_the_running,
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
        //
        // Gated on there being no best, this said the one thing it is for only
        // in the case where the count is nought. With a best, the only age on
        // the line belonged to the *freshest* Account: "2 of 3 have Headroom,
        // the best 93% left (as of 4m ago)" is a count resting on a reading
        // eight hours old and reads as four minutes old. So it is said whenever
        // the oldest is not the one already shown, which is also what keeps it
        // off the line when there is nothing more to add.
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

    /// The same facts as a script reads them.
    ///
    /// Fields rather than the sentence [`lines`] renders, because the listing's
    /// document is structured throughout and a prose sentence in a document is a
    /// thing scripts end up regexing (ADR 0058).
    ///
    /// Every count is over the Accounts a Cycle may choose, so `with_headroom`,
    /// `exhausted` and `never_observed` add up to `candidates`, and those plus
    /// `out_of_the_running` add up to the Accounts in the section beside it.
    /// Which way each of those left the running is not counted again here: the
    /// section's own Accounts carry `disabled` and `quarantined`, and a second
    /// tally of the same fact is how one comes to disagree with the other.
    ///
    /// [`lines`]: Reserve::lines
    pub fn document(&self) -> serde_json::Value {
        json!({
            "candidates": self.candidates.len(),
            "with_headroom": self.with_headroom.len(),
            "exhausted": self.exhausted,
            "never_observed": self.unobserved,
            "out_of_the_running": self.not_candidates,
            // One Account's own figure, named — never a pooled one, and never
            // one no Account reported. `null` where nothing here has Headroom,
            // which is a state rather than nought room (`cycle::headroom_document`
            // refuses the same conflation one Account at a time).
            //
            // Unrounded, like every other percentage in a document: rounding is
            // what a column does to fit.
            "best": self.best().map(|(account, percent)| json!({
                "email": account.email(),
                "percent": percent,
                "observed_at": self.best_read_at().map(|at| at.to_rfc3339()),
            })),
            // The weakest reading the counts above rest on, which is what the
            // sentence quotes for the same reason (ADR 0015). `null` where no
            // candidate has ever been read.
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

    /// The count rests on every candidate's reading, so the age beside it has to
    /// be the oldest of them rather than the best one's own.
    ///
    /// The line was gated on there being no best, so it said the one thing it is
    /// for only when the count was nought. With a best, the only age on screen
    /// belonged to the *freshest* Account: a count of two resting on a reading
    /// eight hours old read as four minutes old, which is the direction that
    /// matters — it overstates what Perch knows.
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

    /// "None" without a reason is a Scope somebody stares at wondering which.
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
                // Counted from cache like every other figure, so it says how
                // old the readings behind it are.
                "Read 4m ago at the oldest.",
            ]
        );
    }

    /// A Quarantined Credential does not work and a Disabled Account is never
    /// chosen, so neither is part of what a Scope has left — and both are said,
    /// because a count that quietly dropped them would not add up to the
    /// Accounts on screen.
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

    /// Counting to "0 of 0" would be arithmetic about a Scope nothing may
    /// reach. What is in the way is the answer instead.
    #[test]
    fn a_scope_nothing_may_cycle_to_says_that_rather_than_counting_to_zero() {
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);

        assert_eq!(
            reserve_of(&holding(vec![broken])),
            ["Reserve: none — no Account here may be Cycled to (1 Quarantined)"]
        );
    }

    /// The same facts, as the shape a script reads (ADR 0058) — with the counts
    /// adding up to the Accounts beside them, which is what makes the document
    /// checkable against its own `accounts` array.
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

    /// No figure is invented for a Scope nothing has ever been read for — a
    /// `percent` of nought and no reading at all are opposite pieces of advice,
    /// and the document says the second as absence rather than as a number.
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
