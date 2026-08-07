//! The watcher's round: what it read, what it decided, and why it said so.
//!
//! `perch watch` is a loop in a terminal, not a daemon (ADR 0013). Everything
//! that follows from that is here: how often it asks, what it asks about, and
//! the one line it prints for every answer — including the answers where
//! nothing happens, which are most of them.
//!
//! The decision log is the whole of the evidence that the policy works. It is
//! what makes "why did it switch just then" answerable without reading the
//! source, so a line names the figure that was read, the threshold it was read
//! against, what was decided, and why — in that order, every time, whatever was
//! decided. A rotated logfile is what a daemon needs because nobody is
//! watching; this is a loop somebody is watching, so it goes to standard output
//! and redirection is their call.
//!
//! Nothing here reaches the network or the filesystem. What a round *does* is
//! [`crate::commands::watch`]'s; what a round *means* is here, where it can be
//! argued with in a unit test.

use chrono::{DateTime, Duration, SecondsFormat, Utc};

use crate::error::{EXIT_HELD, EXIT_NO_CANDIDATE, EXIT_NOTHING_TO_DO, EXIT_OK};
use crate::registry::{Account, Checked, GroupConfig};

/// How long the watcher waits between Refreshing the Account it is on.
///
/// Anthropic's usage endpoint allows roughly 28-30 reads an hour per Account
/// (ADR 0015). Two and a half minutes is twenty-four of them, which leaves room
/// for the `perch status --refresh` somebody types while the watcher is running
/// rather than spending the whole allowance on the loop and having the user's
/// own question refused.
///
/// It is the Refresh of **one** Account, and the case for that is the same
/// arithmetic read the other way: at twenty-four an hour each, a Group of two
/// would already be at the limit and a Group of four past it. Said here,
/// because this is the number it is the reason for (ADR 0013).
pub const REFRESH_INTERVAL_MILLIS: u64 = 150_000;

/// The longest the loop will leave between two Refreshes, however long a
/// failure lasts.
///
/// Eight ordinary intervals — twenty minutes, which is three reads an hour
/// rather than twenty-four. Bounded rather than doubling for as long as the
/// failure does, because the endpoint coming back does not announce itself and
/// the only way the watcher finds out is by asking: a loop that had backed off
/// to an hour would come back long after the crossing it was left running for.
/// Twenty minutes is the order of thing a five-hour window forgives — fifteen
/// is what the cooldown was set at on the same reasoning (ADR 0013).
pub const LONGEST_WAIT_MILLIS: u64 = REFRESH_INTERVAL_MILLIS * 8;

/// How often that is, for the line that says what the loop is about to do.
///
/// Derived rather than written out, so the sentence and the constant cannot
/// come to disagree — and in one form rather than a special case per shape,
/// because a branch this constant never reaches is a branch nothing tests.
pub fn how_often() -> String {
    how_long(REFRESH_INTERVAL_MILLIS)
}

/// A wait, as the line that quotes one says it.
fn how_long(millis: u64) -> String {
    let seconds = millis / 1_000;
    format!("{}m{:02}s", seconds / 60, seconds % 60)
}

/// How long the loop waits before it asks again: the ordinary interval, until a
/// Refresh fails and then a growing multiple of it.
///
/// A held decision costs nothing, which is the whole reason the watcher holds
/// one rather than acting on a cached figure. Held at the ordinary cadence for
/// as long as the loop is left running, though, it costs an endpoint with a
/// 28-30 an hour budget twenty-four questions an hour that it is already
/// refusing to answer. So the wait doubles with each failure and goes back to
/// the interval on the first Refresh that works: a transient failure is
/// recovered from at the ordinary cadence, and a persistent one settles at
/// [`LONGEST_WAIT_MILLIS`].
///
/// It grows from the interval rather than under it, so no back-off ever asks
/// faster than the loop asks when everything works. The arithmetic putting that
/// cadence inside Anthropic's allowance is [`REFRESH_INTERVAL_MILLIS`]'s, and a
/// retry is not the place to spend the room it left over.
///
/// This is not a [`Cooldown`](Policy::cooldown): a cooldown paces Switches the
/// watcher *may* make and is the Group's to set, and a back-off paces questions
/// nobody is answering and is arithmetic about the endpoint. They are counted
/// separately because a failure that cleared would otherwise leave the watcher
/// waiting out a rest it never earned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backoff {
    /// Refreshes that have failed in a row, and nothing else: a count reset by
    /// the first one that works is a count that cannot outlive the failure.
    failures: u32,
}

impl Backoff {
    /// A loop that has read nothing yet and is owed no wait beyond its own.
    pub fn none() -> Backoff {
        Backoff::default()
    }

    /// A Refresh that could not be read.
    pub fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }

    /// A Refresh that was read. The first one clears the whole of the back-off
    /// rather than winding it down a step: the failure is over, and a watcher
    /// still asking every twenty minutes about an endpoint that is answering
    /// would be pacing itself on something that has stopped happening.
    pub fn read(&mut self) {
        self.failures = 0;
    }

    /// How long to leave it before asking again, as things stand.
    pub fn waiting_for(&self) -> u64 {
        let doublings = self.failures.saturating_sub(1);
        // Saturating throughout, so a loop left running against a dead endpoint
        // for a week arrives at the longest wait rather than back at the
        // interval: an overflow here would be a watcher that quietly started
        // hammering again after however many hours it took to wrap.
        let factor = 2u64.checked_pow(doublings).unwrap_or(u64::MAX);
        REFRESH_INTERVAL_MILLIS
            .saturating_mul(factor)
            .min(LONGEST_WAIT_MILLIS)
    }
}

/// The rules a Group gives the watcher for Switching within it (ADR 0013).
///
/// Four numbers, and each answers a different question about the same move:
/// *when* it is wanted, *how often* it may happen, *how much better* the
/// destination has to be, and *whether* the Account just left counts. Read off
/// the Group rather than held as constants, because all four are preferences —
/// unlike [`REFRESH_INTERVAL_MILLIS`], which is arithmetic about Anthropic's
/// allowance and is nobody's to prefer.
///
/// A copy taken at the top of a round rather than a borrow of the Group, so the
/// round decides against one policy throughout: a `perch config set` landing
/// mid-round cannot make the threshold that was read differ from the margin
/// that is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// How full the Account you are on has to be before moving off it is
    /// wanted, as a percentage of its fullest Quota Window.
    pub threshold: u8,
    /// The least wall-clock between two Switches, in minutes.
    pub cooldown_minutes: u32,
    /// How far under the threshold a candidate has to be to be worth moving to,
    /// in percentage points.
    pub margin: u8,
    /// Whether the Account just left is barred for one cooldown.
    pub no_return: bool,
}

impl Policy {
    pub fn of(config: &GroupConfig) -> Policy {
        Policy {
            threshold: config.watcher_threshold_percent,
            cooldown_minutes: config.watcher_cooldown_minutes,
            margin: config.watcher_margin_percent,
            no_return: config.watcher_no_return,
        }
    }

    /// The Utilization a candidate has to be at or under to be worth moving to
    /// — the threshold less the margin.
    ///
    /// Named for the figure rather than for the rule, because "barred" in this
    /// module is no-return's word and one of the two would end up meaning the
    /// other.
    ///
    /// Saturating, so a margin wider than the threshold is a Group that will
    /// only move to an Account with nothing used at all rather than a Group
    /// that will never move at all. Refusing that arrangement instead would
    /// make the order two `perch config set`s are typed in matter, which is the
    /// kind of rule a script finds out about at three in the morning.
    pub fn ceiling(&self) -> u8 {
        self.threshold.saturating_sub(self.margin)
    }

    pub fn cooldown(&self) -> Duration {
        Duration::minutes(i64::from(self.cooldown_minutes))
    }
}

/// The Quota Window an Account's fullness is judged by, and how full it is.
///
/// One window rather than all of them, and always the fullest: being blocked by
/// any window blocks you completely, so the fullest is the one that decides
/// when the Account stops being usable (ADR 0012). It is the same window the
/// Cycle ranks candidates on, which is what stops the watcher acting on one
/// measure and choosing on another.
///
/// It is a [`WindowUtilization`](crate::registry::WindowUtilization) with the
/// reset time taken off rather than the window itself, and deliberately: a
/// reset time is what a Strategy ranks *candidates* on, and it has no bearing
/// on whether the Account you are on is full. Carrying one here would put the
/// wrong figure within reach of the comparison this type exists for — "it comes
/// back in twenty minutes" is not a reason to stay on an Account that is full
/// now, and a threshold that quietly grew a second term would be a policy
/// nobody wrote down.
#[derive(Debug, Clone, PartialEq)]
pub struct Fullest {
    pub window: String,
    pub used_percent: f64,
}

impl Fullest {
    /// What an Account's cached figure makes of it, or `None` where there is no
    /// figure. Never read as empty: "no figure" and "plenty of room" are
    /// opposite pieces of advice.
    pub fn of(account: &Account) -> Option<Fullest> {
        crate::cycle::fullest_window_of(account).map(|window| Fullest {
            window: window.window.clone(),
            used_percent: window.used_percent,
        })
    }

    /// Whether the Account is full enough that moving off it is wanted.
    ///
    /// At the threshold rather than past it: a threshold of 80 is the figure a
    /// person set as the point they want to be moved at, and an 80 that waited
    /// for 81 would be a setting that means something other than what it says.
    pub fn at_or_over(&self, threshold: u8) -> bool {
        self.used_percent >= f64::from(threshold)
    }

    fn as_a_clause(&self) -> String {
        format!("{:.0}% used, fullest {}", self.used_percent, self.window)
    }
}

/// The one thing carried from one round to the next: when the watcher last
/// Switched, and what it Switched off.
///
/// Where it is carried is the loop's and the check's one difference (ADR 0013).
/// The loop keeps it in memory and nowhere else, which is the whole of why
/// `perch watch` still "writes no file of its own": a cooldown is about the loop
/// somebody is running, not about the machine, and two watchers would be two
/// people watching with one pacing the other's decisions. Stopping the loop and
/// starting it again is a person saying "go on then", and it starts with
/// nothing to wait for.
///
/// A `perch watch --once` is a fresh process every time and the sequence of
/// them is the watcher, so there is no memory for it to be carried in and it
/// comes back off the registry — [`Recently::recorded`], from what the check
/// before it wrote down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recently {
    switched: Option<Switched>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Switched {
    at: DateTime<Utc>,
    off: String,
}

impl Recently {
    /// A loop that has just started, which owes nobody a wait.
    pub fn nothing() -> Recently {
        Recently::default()
    }

    /// What a scheduled check inherits from the one before it: the Switch its
    /// Group has on record, or nothing where none has happened.
    pub fn recorded(checked: Option<&Checked>) -> Recently {
        Recently {
            switched: checked.map(|checked| Switched {
                at: checked.switched_at,
                off: checked.switched_off.clone(),
            }),
        }
    }

    pub fn switched(&mut self, off: &str, at: DateTime<Utc>) {
        self.switched = Some(Switched {
            at,
            off: off.to_string(),
        });
    }

    /// Why nothing may move yet, or `None` when something may.
    ///
    /// The cooldown is the floor under how often the watcher acts, and it is
    /// checked before the candidates are read rather than after: a round that
    /// cannot act has no business spending an allowance on figures it will not
    /// use.
    pub fn resting(&self, policy: &Policy, now: DateTime<Utc>) -> Option<String> {
        let left = self.left_of_the_cooldown(policy, now)?;
        let switched = self.switched.as_ref()?;
        // The rule is named rather than only described, because a check's line
        // is read out of a cron mailbox by somebody who has to know which
        // setting to reach for.
        Some(format!(
            "the last Switch was {} ago and this Group's cooldown leaves at \
             least {} between two, so nothing moves for another {}.",
            minutes(now - switched.at),
            minutes(policy.cooldown()),
            minutes(left),
        ))
    }

    /// The Account not to go back to, or `None` when there is none.
    ///
    /// A second lock on the same door as [`Self::resting`], and the honest
    /// thing to say about it is that today the first lock always reaches it
    /// first: no Switch happens inside the cooldown, so no Switch back can
    /// either, and the two windows coincide by construction. That is why a
    /// Group with `watcher-cooldown-minutes 0` has no no-return whatever
    /// `watcher-no-return` says — a no-return of no minutes bars nothing, and
    /// `perch config` says so rather than leaving it to be discovered.
    ///
    /// It is written all the same, because the rule about *where* not to go is
    /// not the rule about *whether* to go, and a rule nobody wrote down is one
    /// that gets relaxed by accident the first time the other one is. The
    /// unit test below is what holds it; the loop cannot, because the loop
    /// never gets far enough to ask.
    pub fn barred(&self, policy: &Policy, now: DateTime<Utc>) -> Option<&str> {
        if !policy.no_return {
            return None;
        }
        self.left_of_the_cooldown(policy, now)?;
        self.switched.as_ref().map(|switched| switched.off.as_str())
    }

    /// How much of the cooldown is left, or `None` where none of it is — which
    /// is also the answer when nothing has been Switched yet.
    fn left_of_the_cooldown(&self, policy: &Policy, now: DateTime<Utc>) -> Option<Duration> {
        let switched = self.switched.as_ref()?;
        let left = policy.cooldown() - (now - switched.at);
        (left > Duration::zero()).then_some(left)
    }
}

/// A span as a count of minutes, for the sentences that quote one. Rounded
/// down, because "another 1 minute" said of fifty seconds is a promise the
/// clock keeps and "another 2" is one it does not.
fn minutes(span: Duration) -> String {
    match span.num_minutes() {
        1 => "1 minute".to_string(),
        count => format!("{count} minutes"),
    }
}

/// A candidate as this round read it, ready to be judged against the margin.
///
/// The name is carried alongside the address because the sentence explaining
/// why an Account was passed over is read by the person who named it, and
/// `crate::registry::Registry::named_for_the_user` is the only thing that knows
/// what they called it.
#[derive(Debug, Clone, PartialEq)]
pub struct Considered {
    pub email: String,
    pub named: String,
    pub fullest: Option<Fullest>,
}

/// The Accounts a Switch may not land on this round, and the one sentence
/// saying why.
///
/// The margin is what kills the ping-pong (ADR 0013): at an 80% threshold
/// nothing is moved to unless it is at 70% or better, so two Accounts hovering
/// either side of the line cannot trade places with each other every few
/// minutes. It is applied by setting candidates aside rather than by second
/// guessing the winner, because the Strategy is entitled to rank whatever
/// clears the bar — a `soonest-reset` Group would otherwise be told there is
/// nowhere to go while a perfectly empty Account sat behind the fullest one.
///
/// An Account Perch has never observed at all is set aside too. The Cycle ranks
/// one above an exhausted Account, which is right for a Switch somebody asked
/// for — but here it would be the watcher moving to an Account it knows nothing
/// about, and "no figure" is not evidence of room. A candidate whose Refresh
/// merely failed this round is *not* this case: it still carries whatever was
/// cached, is judged by the margin on that, and what could not be read is said
/// on the decision line instead.
pub fn set_aside(
    policy: &Policy,
    group: &str,
    considered: &[Considered],
    barred: Option<&str>,
) -> crate::cycle::SetAside {
    let mut emails = Vec::new();
    let mut clauses = Vec::new();
    for candidate in considered {
        let why = if Some(candidate.email.as_str()) == barred {
            format!(
                "{} is the Account this watcher just left, and no-return holds \
                 for the rest of the cooldown",
                candidate.named,
            )
        } else {
            match &candidate.fullest {
                Some(fullest) if fullest.used_percent > f64::from(policy.ceiling()) => format!(
                    "{} is at {:.0}% used and nothing over {}% is worth moving to",
                    candidate.named,
                    fullest.used_percent,
                    policy.ceiling(),
                ),
                Some(_) => continue,
                None => format!(
                    "Perch has never observed how full {} is, and a Switch onto \
                     a figure it has not got is a Switch made blind",
                    candidate.named,
                ),
            }
        };
        emails.push(candidate.email.clone());
        clauses.push(why);
    }

    // Nothing set aside is nothing to explain, and a sentence built out of no
    // clauses would be one waiting to be printed with a hole in it.
    if emails.is_empty() {
        return crate::cycle::SetAside::nothing();
    }
    crate::cycle::SetAside {
        because: format!(
            "Nothing in Group `{group}` is worth Switching to yet — {}. Nothing \
             was changed.",
            clauses.join("; "),
        ),
        emails,
    }
}

/// What a round decided.
///
/// Six outcomes, and five of them change nothing. That is the ordinary shape
/// of watching something: the decisions where nothing happens are the ones that
/// have to be printed most carefully, because they are the evidence that the
/// watcher is awake and has an opinion.
///
/// They are also five different reasons for nothing happening, and the
/// difference is the whole of what a person reading the log needs: waiting
/// resolves itself, cooling resolves itself by the clock, nowhere resolves
/// itself by a reset, held resolves itself by the network coming back, and
/// refused resolves itself when whatever is running stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The Account is not full enough to want moving off.
    Waiting,
    /// It was, and the Group's cooldown has not run out since the last Switch.
    /// Nothing was read of the candidates: a round that may not act has no
    /// business spending an allowance on figures it cannot use.
    Cooling { why: String },
    /// It was, and this is why the Account it landed on won.
    ///
    /// No field for where it went, because the reason names it: the Cycle
    /// writes "overflow@example.com has the most room: …", and a second place
    /// saying which Account that was is a second place that can disagree with
    /// the first.
    Switched { because: String },
    /// It was, and there was nowhere worth going — every candidate exhausted,
    /// or none of them a candidate at all. Nothing to fix and nothing to
    /// retry: the answer is to wait, so the loop does.
    Nowhere { why: String },
    /// The figures could not be read, so nothing was decided on the ones Perch
    /// already had. A Switch made on a cached figure is a Switch the user could
    /// have made themselves without leaving a process running (ADR 0013).
    ///
    /// The only outcome that carries how long the loop then waits, because it
    /// is the only one where that is not the ordinary interval — and a hold
    /// whose line did not say when it would try again would read as a watcher
    /// that had given up.
    ///
    /// Absent where nothing here decides when the next reading is: a
    /// `perch watch --once` is exiting, and when it comes back is whatever
    /// scheduled it to say. Promising an interval it has no part in would be
    /// the one thing on the line that was not true.
    Held {
        why: String,
        retrying_in: Option<u64>,
    },
    /// A Switch was wanted, was attempted, and was turned away without
    /// changing anything — a client running against the Profile the Capture
    /// would write into, most often (ADR 0027). Distinct from a dead end,
    /// because this one is about the machine rather than about the quota, and
    /// it clears when whatever was running stops.
    Refused { why: String },
}

impl Outcome {
    /// What a single check reports to whatever scheduled it (ADR 0013).
    ///
    /// The existing table rather than a code per outcome: a scheduler branches
    /// on what it can do about the answer, and three of the six outcomes leave
    /// it with the same thing to do — nothing now, and come back at the next
    /// Check. Which of the three it was is on the decision line, where a person
    /// reading a cron mailbox needs it and a script does not.
    ///
    /// Only 20 is new, and only because a scheduler retrying in five minutes
    /// has to tell a figure that could not be read from a Group with nowhere to
    /// go: the first resolves itself and the second does not.
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Switched { .. } => EXIT_OK,
            // Nothing to do *now*, three ways: the Account is not full enough,
            // the cooldown is not up, or a client was holding the Profile and
            // will not be holding it for long.
            Outcome::Waiting | Outcome::Cooling { .. } | Outcome::Refused { .. } => {
                EXIT_NOTHING_TO_DO
            }
            Outcome::Nowhere { .. } => EXIT_NO_CANDIDATE,
            Outcome::Held { .. } => EXIT_HELD,
        }
    }

    /// The word the line is read by, in a column so a day of them can be
    /// skimmed for the ones that did something.
    fn word(&self) -> &'static str {
        match self {
            Outcome::Waiting => "waiting",
            Outcome::Cooling { .. } => "cooling",
            Outcome::Switched { .. } => "switched",
            Outcome::Nowhere { .. } => "nowhere",
            Outcome::Held { .. } => "held",
            Outcome::Refused { .. } => "refused",
        }
    }
}

/// One turn of the loop, whole: what was read, what it was read against, and
/// what came of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    /// The Account that was Refreshed, which is the active one and only ever
    /// the active one (ADR 0013).
    pub email: String,
    /// How full it was, as this round read it. Absent when the read failed,
    /// which is the whole of why nothing was decided.
    pub fullest: Option<Fullest>,
    /// The Group's `watcher-threshold-percent`, quoted because a decision
    /// nobody can see the threshold of is a decision nobody can argue with.
    pub threshold: u8,
    pub outcome: Outcome,
}

impl Round {
    /// How long the loop leaves it before the next round.
    ///
    /// Read off the round rather than kept beside it, so the wait the line
    /// promised and the wait the loop takes are the same number. A round that
    /// read a figure is followed by the ordinary interval whatever it decided
    /// about it: nothing is wrong with the endpoint, and a watcher that paced
    /// itself on finding nowhere to go would be slowest to notice the moment
    /// somewhere opened up.
    pub fn waiting_for(&self) -> u64 {
        match &self.outcome {
            Outcome::Held {
                retrying_in: Some(millis),
                ..
            } => *millis,
            // Named one by one rather than caught by a wildcard, so an outcome
            // added later has to say what the loop does after it instead of
            // inheriting an answer nobody chose for it. A hold carrying no wait
            // is a check's, and a check exits rather than asking this.
            Outcome::Held {
                retrying_in: None, ..
            }
            | Outcome::Waiting
            | Outcome::Cooling { .. }
            | Outcome::Switched { .. }
            | Outcome::Nowhere { .. }
            | Outcome::Refused { .. } => REFRESH_INTERVAL_MILLIS,
        }
    }

    /// The decision line, as it is printed: one line, whatever happened.
    pub fn line(&self, now: DateTime<Utc>) -> String {
        format!(
            "{}  {:<8}  {} {}; threshold {}% — {}",
            now.to_rfc3339_opts(SecondsFormat::Secs, true),
            self.outcome.word(),
            self.email,
            self.figure(),
            self.threshold,
            one_line(&self.reason()),
        )
    }

    fn figure(&self) -> String {
        match &self.fullest {
            Some(fullest) => fullest.as_a_clause(),
            // Said as a figure that was not read rather than left out, because
            // a line missing the number is a line that reads as an oversight.
            None => "unread".to_string(),
        }
    }

    /// Why the outcome is the outcome. The Switch and the dead end quote the
    /// Cycle's own words, so the reason the watcher gives for landing somewhere
    /// is the reason `perch switch` would have given for landing there.
    fn reason(&self) -> String {
        match &self.outcome {
            Outcome::Waiting => "under it, so nothing was wanted.".to_string(),
            Outcome::Cooling { why } => format!("over it, and too soon to move again: {why}"),
            Outcome::Switched { because } => format!("over it. Switched — {because}"),
            Outcome::Nowhere { why } => format!("over it, and nowhere to go: {why}"),
            Outcome::Held {
                why,
                retrying_in: Some(millis),
            } => format!(
                "nothing current to decide on, so nothing was decided: {why} \
                 Asking again in {}.",
                how_long(*millis),
            ),
            Outcome::Held {
                why,
                retrying_in: None,
            } => format!("nothing current to decide on, so nothing was decided: {why}"),
            Outcome::Refused { why } => {
                format!("over it, and the Switch was turned away: {why}")
            }
        }
    }
}

/// A message from anywhere else, as one line.
///
/// The refusals the Cycle writes are written to be read on a terminal by
/// somebody who just typed the command, so they run to two or three lines. A
/// decision log is a line per decision — a reason that wraps onto its own line
/// stops being attached to the decision it explains the moment two rounds are
/// read together.
fn one_line(said: &str) -> String {
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()
    }

    fn round(fullest: Option<Fullest>, outcome: Outcome) -> Round {
        Round {
            email: "someone@example.com".to_string(),
            fullest,
            threshold: 80,
            outcome,
        }
    }

    fn at(used_percent: f64) -> Option<Fullest> {
        Some(Fullest {
            window: "5-hour".to_string(),
            used_percent,
        })
    }

    /// Every line answers the same four questions, whatever it decided —
    /// otherwise the log is only readable for the rounds that did something,
    /// and those are the rounds least in need of explaining.
    #[test]
    fn every_decision_names_the_figure_the_threshold_the_outcome_and_the_reason() {
        let decisions = [
            round(at(42.0), Outcome::Waiting),
            round(
                at(86.0),
                Outcome::Cooling {
                    why: "the last Switch was 4 minutes ago.".to_string(),
                },
            ),
            round(
                at(86.0),
                Outcome::Switched {
                    because: "overflow@example.com has the most room: 95% headroom.".to_string(),
                },
            ),
            round(
                at(86.0),
                Outcome::Nowhere {
                    why: "every Account in Group `work` is exhausted.".to_string(),
                },
            ),
            round(
                None,
                Outcome::Held {
                    why: "Anthropic could not be reached.".to_string(),
                    retrying_in: Some(REFRESH_INTERVAL_MILLIS),
                },
            ),
            round(
                at(86.0),
                Outcome::Refused {
                    why: "a client is running against that Profile.".to_string(),
                },
            ),
        ];

        for decision in decisions {
            let line = decision.line(now());

            assert!(line.starts_with("2026-08-04T12:00:00Z"), "{line}");
            assert!(line.contains(decision.outcome.word()), "{line}");
            assert!(line.contains("threshold 80%"), "{line}");
            assert!(line.contains("someone@example.com"), "{line}");
            assert!(!line.trim_end().is_empty(), "{line}");
            assert!(
                !line.contains('\n'),
                "a reason on a second line stops being attached to the decision \
                 it explains: {line}"
            );
        }
    }

    /// A round that read nothing says so. The figure Perch already had is not
    /// quoted in its place — a number on the line reads as the number the
    /// decision was made on, and this one was made *because* there was none.
    #[test]
    fn a_round_that_could_not_read_the_figure_quotes_no_figure() {
        let line = round(
            None,
            Outcome::Held {
                why: "Anthropic is rate-limiting reads of this Account.".to_string(),
                retrying_in: Some(REFRESH_INTERVAL_MILLIS),
            },
        )
        .line(now());

        assert!(line.contains("unread"), "{line}");
        assert!(
            !line.contains('%') || line.contains("threshold 80%"),
            "{line}"
        );
        assert!(line.contains("rate-limiting"), "{line}");
    }

    /// A hold is the one outcome that changes when the loop comes back, so it
    /// is the one that has to say. Without it the log reads as a watcher that
    /// noticed something was wrong and stopped having opinions about it.
    #[test]
    fn a_held_round_says_when_it_will_ask_again() {
        let line = round(
            None,
            Outcome::Held {
                why: "Anthropic could not be reached.".to_string(),
                retrying_in: Some(600_000),
            },
        )
        .line(now());

        assert!(line.contains("held"), "{line}");
        assert!(line.contains("could not be reached"), "{line}");
        assert!(line.contains("10m00s"), "and when it comes back: {line}");
    }

    /// A check's hold says the same thing without the promise: when it comes
    /// back is whatever scheduled it to say, and a line quoting an interval this
    /// process has no part in would be the one untrue thing on it.
    #[test]
    fn a_held_check_says_what_held_it_and_promises_nothing_about_coming_back() {
        let line = round(
            None,
            Outcome::Held {
                why: "Anthropic could not be reached.".to_string(),
                retrying_in: None,
            },
        )
        .line(now());

        assert!(line.contains("held"), "{line}");
        assert!(line.contains("could not be reached"), "{line}");
        assert!(!line.contains("Asking again"), "{line}");
        assert!(!line.contains("m00s"), "no interval at all: {line}");
    }

    /// What a check reports to whatever scheduled it, over every outcome there
    /// is: five codes, four of them the table's already (ADR 0013).
    ///
    /// Written as a match rather than a list, so an outcome added later cannot
    /// reach a scheduler without somebody deciding what it means to one — and
    /// asserted against the whole set, because a code outside it is one no
    /// script that read the table would know what to do with.
    #[test]
    fn every_outcome_a_check_can_have_reports_a_code_from_the_table() {
        let outcomes = [
            Outcome::Waiting,
            Outcome::Cooling { why: String::new() },
            Outcome::Switched {
                because: String::new(),
            },
            Outcome::Nowhere { why: String::new() },
            Outcome::Held {
                why: String::new(),
                retrying_in: None,
            },
            Outcome::Refused { why: String::new() },
        ];

        for outcome in &outcomes {
            let code = outcome.exit_code();
            assert!(
                [EXIT_OK, EXIT_NOTHING_TO_DO, EXIT_NO_CANDIDATE, EXIT_HELD].contains(&code),
                "{outcome:?} exits {code}, which is not in the table `--once` \
                 documents"
            );
        }

        // The distinctions a scheduler acts on: a Switch happened, a Switch
        // could not be decided on, and a Switch was decided against for want of
        // anywhere to go.
        assert_eq!(outcomes[2].exit_code(), EXIT_OK);
        assert_eq!(outcomes[4].exit_code(), EXIT_HELD);
        assert_eq!(outcomes[3].exit_code(), EXIT_NO_CANDIDATE);
        for nothing_now in [&outcomes[0], &outcomes[1], &outcomes[5]] {
            assert_eq!(
                nothing_now.exit_code(),
                EXIT_NOTHING_TO_DO,
                "{nothing_now:?} leaves a scheduler the same thing to do — \
                 nothing now, and come back at the next Check"
            );
        }
    }

    /// A cooldown is a rule with a name, and the line a scheduler captures is
    /// read by somebody who has to know which setting to reach for.
    #[test]
    fn a_cooling_round_names_the_rule_that_held_it() {
        let mut recently = Recently::nothing();
        recently.switched("left@example.com", now());

        let why = recently
            .resting(&policy(), now() + Duration::minutes(4))
            .expect("four minutes into a fifteen minute cooldown");

        assert!(why.contains("cooldown"), "{why}");
    }

    /// What a check inherits from the one before it, which is the whole of what
    /// makes a sequence of them a watcher rather than a Switch on a timer.
    #[test]
    fn a_check_is_paced_by_what_the_one_before_it_recorded() {
        let recorded = Recently::recorded(Some(&Checked {
            switched_at: now(),
            switched_off: "left@example.com".to_string(),
        }));

        assert!(
            recorded
                .resting(&policy(), now() + Duration::minutes(4))
                .is_some()
        );
        assert_eq!(
            recorded.barred(&policy(), now() + Duration::minutes(4)),
            Some("left@example.com"),
        );
        assert_eq!(
            recorded.resting(&policy(), now() + Duration::minutes(15)),
            None,
            "one cooldown, counted from the Switch that was recorded rather \
             than from the process that read it"
        );
        assert_eq!(
            Recently::recorded(None),
            Recently::nothing(),
            "a Group nothing has Switched within owes nobody a wait"
        );
    }

    /// The wait the line promises is the wait the loop takes, because they are
    /// the same number read out of the same place.
    #[test]
    fn a_round_that_read_a_figure_is_followed_by_the_ordinary_interval() {
        for outcome in [
            Outcome::Waiting,
            Outcome::Cooling { why: String::new() },
            Outcome::Switched {
                because: String::new(),
            },
            Outcome::Nowhere { why: String::new() },
            Outcome::Refused { why: String::new() },
        ] {
            let round = round(at(86.0), outcome);
            assert_eq!(
                round.waiting_for(),
                REFRESH_INTERVAL_MILLIS,
                "{:?} is a decision about a figure that was read, not about an \
                 endpoint that would not answer",
                round.outcome,
            );
        }

        assert_eq!(
            round(
                None,
                Outcome::Held {
                    why: String::new(),
                    retrying_in: Some(600_000),
                },
            )
            .waiting_for(),
            600_000,
        );
    }

    /// The wait after `failures` Refreshes have failed in a row.
    fn after_failing(failures: u32) -> u64 {
        let mut backoff = Backoff::none();
        for _ in 0..failures {
            backoff.failed();
        }
        backoff.waiting_for()
    }

    /// A transient failure is recovered from at the ordinary cadence: the first
    /// retry is a round like any other, and only being wrong twice in a row
    /// buys any patience.
    #[test]
    fn the_first_failure_is_retried_at_the_ordinary_interval() {
        assert_eq!(after_failing(0), REFRESH_INTERVAL_MILLIS);
        assert_eq!(after_failing(1), REFRESH_INTERVAL_MILLIS);
    }

    /// It doubles, and then it stops. Doubling forever would have the watcher
    /// come back hours after the crossing it was left running for.
    #[test]
    fn the_wait_doubles_with_every_failure_and_stops_at_the_longest() {
        let waits: Vec<u64> = (1..=6).map(after_failing).collect();

        assert_eq!(
            waits,
            vec![150_000, 300_000, 600_000, 1_200_000, 1_200_000, 1_200_000],
        );
        assert_eq!(LONGEST_WAIT_MILLIS, 1_200_000, "twenty minutes");
    }

    /// Arithmetic that saturates rather than wrapping: a loop left running
    /// against a dead endpoint for a week arrives at the longest wait and stays
    /// there, rather than quietly starting to hammer again. Two hundred
    /// failures is long past where doubling overflows a `u64`, which is the
    /// step that would otherwise come back round to a short wait.
    #[test]
    fn a_failure_that_never_clears_never_comes_back_round_to_the_interval() {
        assert_eq!(after_failing(200), LONGEST_WAIT_MILLIS);
    }

    /// No back-off ever asks faster than the loop does when everything works,
    /// so the arithmetic that puts the ordinary cadence inside Anthropic's
    /// allowance covers a failing endpoint too.
    #[test]
    fn no_wait_is_ever_shorter_than_an_ordinary_round() {
        for failures in 0..10 {
            let reads_an_hour = 3_600_000 / after_failing(failures);
            assert!(
                reads_an_hour <= 3_600_000 / REFRESH_INTERVAL_MILLIS,
                "a retry that asked faster than the loop would spend the room \
                 the interval left for the user's own `perch status --refresh`"
            );
        }
    }

    /// The first Refresh that works clears the whole of it. Winding it down a
    /// step at a time would pace the watcher on a failure that has stopped
    /// happening.
    #[test]
    fn the_first_reading_that_works_puts_the_wait_back_to_the_interval() {
        let mut backoff = Backoff::none();
        for _ in 0..4 {
            backoff.failed();
        }

        backoff.read();

        assert_eq!(backoff, Backoff::none());
        assert_eq!(backoff.waiting_for(), REFRESH_INTERVAL_MILLIS);
    }

    /// The Cycle's refusals are written for a terminal and run to several
    /// lines. They arrive here as one.
    #[test]
    fn a_multi_line_reason_arrives_as_one_line() {
        let line = round(
            at(90.0),
            Outcome::Nowhere {
                why: "Every Account in Group `work` is exhausted, so there is \
                      nowhere useful to Switch. Nothing was changed.\n\
                      overflow@example.com frees up soonest, at 15:00."
                    .to_string(),
            },
        )
        .line(now());

        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("frees up soonest"), "{line}");
    }

    /// A threshold is the figure somebody set as the point they want moving at.
    #[test]
    fn the_threshold_is_a_point_reached_rather_than_one_passed() {
        let fullest = Fullest {
            window: "5-hour".to_string(),
            used_percent: 80.0,
        };
        assert!(fullest.at_or_over(80));
        assert!(!fullest.at_or_over(81));
    }

    fn policy() -> Policy {
        Policy::of(&GroupConfig::default())
    }

    /// The Group's defaults, read as the watcher reads them.
    #[test]
    fn the_default_policy_moves_you_at_eighty_and_only_onto_seventy_or_better() {
        let policy = policy();
        assert_eq!(policy.threshold, 80);
        assert_eq!(policy.ceiling(), 70);
        assert_eq!(policy.cooldown(), Duration::minutes(15));
        assert!(policy.no_return);
    }

    /// A margin wider than the threshold is a Group that will only move onto an
    /// Account with nothing used, rather than one that has quietly stopped
    /// moving at all or one whose two settings have to be typed in the right
    /// order.
    #[test]
    fn a_margin_wider_than_the_threshold_bars_everything_but_an_empty_account() {
        let strict = Policy {
            threshold: 50,
            margin: 90,
            ..policy()
        };
        assert_eq!(strict.ceiling(), 0);
    }

    /// The cooldown is a floor under how often the watcher acts, and it is
    /// counted from the Switch rather than from the round.
    #[test]
    fn nothing_moves_again_until_the_cooldown_has_run_out() {
        let mut recently = Recently::nothing();
        assert_eq!(
            recently.resting(&policy(), now()),
            None,
            "a loop that has just started owes nobody a wait"
        );

        recently.switched("left@example.com", now());

        let waiting = recently
            .resting(&policy(), now() + Duration::minutes(4))
            .expect("four minutes into a fifteen minute cooldown");
        assert!(waiting.contains("4 minutes ago"), "{waiting}");
        assert!(waiting.contains("15 minutes"), "the cooldown: {waiting}");
        assert!(
            waiting.contains("another 11"),
            "and what is left: {waiting}"
        );

        assert_eq!(
            recently.resting(&policy(), now() + Duration::minutes(15)),
            None,
            "a cooldown of fifteen minutes is over after fifteen minutes, not \
             after sixteen"
        );
    }

    /// A cooldown of nothing is a Group that has asked for no cooldown, not one
    /// that waits a round anyway.
    #[test]
    fn a_cooldown_of_zero_never_holds_anything_back() {
        let mut recently = Recently::nothing();
        recently.switched("left@example.com", now());
        let eager = Policy {
            cooldown_minutes: 0,
            ..policy()
        };

        assert_eq!(recently.resting(&eager, now()), None);
        assert_eq!(recently.barred(&eager, now()), None);
    }

    /// The Account just left is barred by name, and the Group can say it should
    /// not be — the cooldown and the no-return are two rules, and turning one
    /// off is not a way of giving up the other.
    #[test]
    fn the_account_just_left_is_no_candidate_until_the_cooldown_has_passed() {
        let mut recently = Recently::nothing();
        recently.switched("left@example.com", now());

        assert_eq!(
            recently.barred(&policy(), now() + Duration::minutes(4)),
            Some("left@example.com"),
        );
        assert_eq!(
            recently.barred(&policy(), now() + Duration::minutes(15)),
            None,
            "one cooldown, and no longer"
        );
        assert_eq!(
            recently.barred(
                &Policy {
                    no_return: false,
                    ..policy()
                },
                now() + Duration::minutes(4)
            ),
            None,
            "a Group that says coming back is fine is a Group that may come back"
        );
    }

    fn considered(named: &str, used_percent: Option<f64>) -> Considered {
        Considered {
            email: format!("{named}@example.com"),
            named: format!("{named}@example.com"),
            fullest: used_percent.map(|used_percent| Fullest {
                window: "5-hour".to_string(),
                used_percent,
            }),
        }
    }

    /// The margin, which is the whole of what stops a ping-pong: at an 80%
    /// threshold nothing under 70% may be moved to, so the Account that is only
    /// just emptier than the one you are on is passed over.
    #[test]
    fn a_candidate_that_is_barely_emptier_than_the_threshold_is_set_aside() {
        let set_aside = set_aside(
            &policy(),
            "work",
            &[
                considered("just-under", Some(74.0)),
                considered("at-the-bar", Some(70.0)),
                considered("roomy", Some(5.0)),
            ],
            None,
        );

        assert_eq!(set_aside.emails, vec!["just-under@example.com".to_string()]);
        assert!(set_aside.because.contains("74%"), "{}", set_aside.because);
        assert!(
            set_aside.because.contains("70%"),
            "and the figure it was judged against: {}",
            set_aside.because
        );
    }

    /// The Cycle ranks an Account nobody has ever read above an exhausted one,
    /// which is right for a Switch somebody asked for. Unasked it is a move onto
    /// an Account the watcher knows nothing about, and "no figure" is not
    /// evidence of room.
    #[test]
    fn a_candidate_no_figure_was_ever_read_of_is_set_aside_rather_than_read_as_empty() {
        let set_aside = set_aside(&policy(), "work", &[considered("unseen", None)], None);

        assert_eq!(set_aside.emails, vec!["unseen@example.com".to_string()]);
        assert!(
            set_aside.because.contains("never observed"),
            "{}",
            set_aside.because
        );
    }

    /// A barred Account is set aside whatever its figure says, and the reason
    /// names the rule rather than the number — it was never judged on a number.
    #[test]
    fn the_barred_account_is_set_aside_however_empty_it_looks() {
        let set_aside = set_aside(
            &policy(),
            "work",
            &[considered("left", Some(2.0))],
            Some("left@example.com"),
        );

        assert_eq!(set_aside.emails, vec!["left@example.com".to_string()]);
        assert!(
            set_aside.because.contains("no-return"),
            "{}",
            set_aside.because
        );
        assert!(
            !set_aside.because.contains("2%"),
            "quoting the figure would read as the reason, and it was not: {}",
            set_aside.because
        );
    }

    /// Every line answers the same four questions, and a cooling round is a
    /// round like any other.
    #[test]
    fn a_cooling_round_says_it_is_over_the_threshold_and_why_nothing_moved() {
        let line = round(
            at(86.0),
            Outcome::Cooling {
                why: "the last Switch was 4 minutes ago and this Group leaves at \
                      least 15 minutes between two, so nothing moves for another \
                      11 minutes."
                    .to_string(),
            },
        )
        .line(now());

        assert!(line.contains("cooling"), "{line}");
        assert!(line.contains("86% used"), "{line}");
        assert!(line.contains("threshold 80%"), "{line}");
        assert!(line.contains("too soon to move again"), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }
}
