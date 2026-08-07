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

use chrono::{DateTime, SecondsFormat, Utc};

use crate::registry::Account;

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

/// What a round decided.
///
/// Five outcomes, and four of them change nothing. That is the ordinary shape
/// of watching something: the decisions where nothing happens are the ones that
/// have to be printed most carefully, because they are the evidence that the
/// watcher is awake and has an opinion.
///
/// They are also four different reasons for nothing happening, and the
/// difference is the whole of what a person reading the log needs: waiting
/// resolves itself, nowhere resolves itself by a reset, held resolves itself by
/// the network coming back, and refused resolves itself when whatever is
/// running stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The Account is not full enough to want moving off.
    Waiting,
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
}
