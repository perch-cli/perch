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

use crate::registry::{Account, GroupConfig};

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

/// How often that is, for the line that says what the loop is about to do.
///
/// Derived rather than written out, so the sentence and the constant cannot
/// come to disagree — and in one form rather than a special case per shape,
/// because a branch this constant never reaches is a branch nothing tests.
pub fn how_often() -> String {
    let seconds = REFRESH_INTERVAL_MILLIS / 1_000;
    format!("{}m{:02}s", seconds / 60, seconds % 60)
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

/// The one thing the loop carries from one round to the next: when it last
/// Switched, and what it Switched off.
///
/// In memory and nowhere else, which is the whole of why `perch watch` still
/// "writes no file of its own". A cooldown is about the loop somebody is
/// running, not about the machine: two watchers would be two people watching,
/// and a cooldown recorded in the registry would have one of them pacing the
/// other's decisions. Stopping the loop and starting it again is a person
/// saying "go on then", and it starts with nothing to wait for.
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
        Some(format!(
            "the last Switch was {} ago and this Group leaves at least {} \
             between two, so nothing moves for another {}.",
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
    Held { why: String },
    /// A Switch was wanted, was attempted, and was turned away without
    /// changing anything — a client running against the Profile the Capture
    /// would write into, most often (ADR 0027). Distinct from a dead end,
    /// because this one is about the machine rather than about the quota, and
    /// it clears when whatever was running stops.
    Refused { why: String },
}

impl Outcome {
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
            Outcome::Held { why } => {
                format!("nothing current to decide on, so nothing was decided: {why}")
            }
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
