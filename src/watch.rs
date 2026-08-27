//! The Watcher's round: what it read, what it decided, and why it said so.
//!
//! The five numbers that pace a round, which of them is anyone's to set, and which
//! outcomes stop at the figure while the refusals keep their sentence whole, are
//! ADR a-watcher-knob-is-arithmetic's.
//!
//! Nothing here reaches the network or the filesystem. What a round *does* is
//! [`crate::commands::watch`]'s; what a round *means* is here, where it can be argued
//! with in a unit test.

use chrono::{DateTime, Duration, SecondsFormat, Utc};

use crate::error::{EXIT_HELD, EXIT_NO_CANDIDATE, EXIT_NOTHING_TO_DO, EXIT_OK, Result};
use crate::live::{self, NotIdle};
use crate::lock::Lost;
use crate::probe::Installed;
use crate::registry::{Account, Checked, Settings};

/// How long the watcher waits between Refreshing the Account it is on.
///
/// Twenty-four reads an hour, inside the 28-30 Anthropic's usage endpoint allows per
/// Account (ADR a-figure-carries-its-age), leaving room for the `perch status
/// --refresh` somebody types while the loop is running.
pub const REFRESH_INTERVAL_MILLIS: u64 = 150_000;

/// The longest the loop will leave between two Refreshes, however long a failure lasts.
///
/// Eight ordinary intervals — three reads an hour. Bounded rather than doubling,
/// because the endpoint coming back does not announce itself.
pub const LONGEST_WAIT_MILLIS: u64 = REFRESH_INTERVAL_MILLIS * 8;

/// How long the loop rests after a round that found nowhere to go.
///
/// The ordinary interval Refreshes one Account and such a round read every
/// candidate, so that cadence spends each of their allowances on Accounts it
/// just refused. The Cooldown: none of them becomes a candidate in 150 seconds.
pub const NOWHERE_INTERVAL_MILLIS: u64 = COOLDOWN_MINUTES as u64 * 60_000;

/// How often that is, for the line that says what the loop is about to do.
///
/// Derived rather than written out, so the sentence and the constant cannot come to
/// disagree.
pub fn how_often() -> String {
    how_long(REFRESH_INTERVAL_MILLIS)
}

/// A wait, as the line that quotes one says it.
fn how_long(millis: u64) -> String {
    let seconds = millis / 1_000;
    format!("{}m{:02}s", seconds / 60, seconds % 60)
}

/// How long the loop waits before it asks again: the ordinary interval, until a Refresh
/// fails and then a growing multiple of it.
///
/// Counted apart from the [`cooldown`], or a failure that cleared would leave the
/// watcher waiting out a rest it never earned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backoff {
    /// Refreshes that have failed in a row, and nothing else: a count reset by the
    /// first one that works cannot outlive the failure.
    failures: u32,
}

impl Backoff {
    pub fn none() -> Backoff {
        Backoff::default()
    }

    /// A round that could not read: the failure is charged and the wait it lands on
    /// comes back, in one call.
    ///
    /// The only way to do either, so a hold cannot be reported without being paid for,
    /// or said against a wait it has not just earned.
    pub fn could_not_read(&mut self) -> u64 {
        self.failed();
        self.waiting_for()
    }

    /// A Refresh that could not be read.
    fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }

    /// A Refresh that was read. The first one clears the whole of the back-off rather
    /// than winding it down a step: pacing the loop on a failure that has stopped
    /// happening is pacing it on nothing.
    pub fn read(&mut self) {
        self.failures = 0;
    }

    /// How long to leave it before asking again, as things stand.
    fn waiting_for(&self) -> u64 {
        let doublings = self.failures.saturating_sub(1);
        // Saturating throughout: an overflow here would be a watcher that quietly
        // started hammering again after however many hours it took to wrap.
        let factor = 2u64.checked_pow(doublings).unwrap_or(u64::MAX);
        REFRESH_INTERVAL_MILLIS
            .saturating_mul(factor)
            .min(LONGEST_WAIT_MILLIS)
    }
}

/// How long an unchanged hold goes unsaid before it says it is still there.
///
/// An hour, so a hold lasting a working day is twenty-odd lines rather than a thousand
/// and a log opened at any point has recent evidence the watcher is awake
/// (ADR the-machine-runs-the-watcher).
pub const STILL_HOLDING_MILLIS: i64 = 3_600_000;

/// What to say about a hold, given what has already been said about it.
///
/// The proof of life is duration rather than repetition: a hold says itself in full
/// when it starts, says how long it has been going once an hour, and says what it cost
/// when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speak {
    /// Say the whole line. This hold is new, or it is not the one that was being held a
    /// moment ago.
    InFull,
    /// Say only that it is still there, and since when.
    StillHolding { since: DateTime<Utc> },
    /// Say nothing: the same hold, said recently enough.
    Nothing,
}

/// The hold a loop is currently in, and what has been said about it.
///
/// In memory and nowhere else, like the [`Backoff`] and the [`Recently`] beside it: a
/// second loop's log is not paced by this one's, and a Check says its one line and
/// leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holding {
    said: Option<Said>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    /// What is holding it, and when it said it would ask again — a pair, because a
    /// changed cadence is news: keyed on the reason alone, a log would claim the
    /// cadence a throttle was first said against while the [`Backoff`] doubled
    /// underneath it.
    said: HoldSaid,
    since: DateTime<Utc>,
    last_said: DateTime<Utc>,
    /// Whether anything has gone unsaid under this hold. What decides if there is
    /// anything worth saying when it ends: a hold that said every one of its rounds has
    /// already told the whole story.
    suppressed: bool,
}

/// What a held line claims: why, and when it will ask again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HoldSaid {
    why: String,
    retrying_in: Option<u64>,
}

impl Holding {
    /// A loop that is holding nothing, which is how one starts.
    pub fn nothing() -> Holding {
        Holding::default()
    }

    /// A round that held, and what to say about it.
    pub fn holding(&mut self, why: &str, retrying_in: Option<u64>, now: DateTime<Utc>) -> Speak {
        let saying = HoldSaid {
            why: why.to_string(),
            retrying_in,
        };
        match &mut self.said {
            Some(said) if said.said == saying => {
                if (now - said.last_said).num_milliseconds() >= STILL_HOLDING_MILLIS {
                    said.last_said = now;
                    // Suppressed, because this says the short form in place of the full
                    // line: unmarked, a hold whose only unsaid round was this hourly
                    // line would end in silence.
                    said.suppressed = true;
                    return Speak::StillHolding { since: said.since };
                }
                said.suppressed = true;
                Speak::Nothing
            }
            // A changed half starts its own line but not its own clock, so `since` and
            // the `suppressed` that reports the whole hold carry over wherever the
            // reason is the same.
            was => {
                let (since, suppressed) = match was {
                    Some(said) if said.said.why == saying.why => (said.since, said.suppressed),
                    _ => (now, false),
                };
                self.said = Some(Said {
                    said: saying,
                    since,
                    last_said: now,
                    suppressed,
                });
                Speak::InFull
            }
        }
    }

    /// A round that did not hold, and how long the hold it ended had lasted — or `None`
    /// where there was nothing to end, or where nothing went unsaid under it, because
    /// every round of that hold is already in the log.
    pub fn released(&mut self, now: DateTime<Utc>) -> Option<Duration> {
        let said = self.said.take()?;
        match said.suppressed {
            true => Some(now - said.since),
            false => None,
        }
    }
}

/// A hold that is still what it was, said as how long rather than as what.
pub fn still_holding_line(since: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format!(
        "{}  {:<8}  still held, since {} ({}). Nothing has changed, and nothing \
         has been decided in that time.",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        "held",
        since.to_rfc3339_opts(SecondsFormat::Secs, true),
        for_how_long(now - since),
    )
}

/// A hold that is over, said before the line of the round that ended it.
pub fn released_line(held_for: Duration, now: DateTime<Utc>) -> String {
    format!(
        "{}  {:<8}  the hold is over after {}, and the watcher is deciding \
         again.",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        "resumed",
        for_how_long(held_for),
    )
}

/// A span, as the two lines above quote one. Minutes up to an hour and then hours and
/// minutes, because a hold is interesting at the scale it has lasted and never at the
/// second.
fn for_how_long(span: Duration) -> String {
    let minutes = span.num_minutes().max(0);
    match minutes {
        0..=59 => format!("{minutes}m"),
        _ => format!("{}h{:02}m", minutes / 60, minutes % 60),
    }
}

/// The least wall-clock the watcher leaves between two Switches.
///
/// A five-hour window moves slowly enough that fifteen minutes never misses a real
/// crossing, and often enough that a watcher which has just moved you is not about to
/// move you again.
pub const COOLDOWN_MINUTES: u32 = 15;

/// The rule a Scope gives the watcher: the two numbers saying when a move is wanted
/// and what is worth moving to, which are the pacing questions that are anyone's.
///
/// A copy taken at the top of a round, so a `perch config set` landing mid-round cannot
/// set a candidate aside by a figure the line does not quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// How full the Account you are on has to be before moving off it is wanted, as a
    /// percentage of its fullest Quota Window.
    pub threshold: u8,
    /// How far under the threshold a candidate has to sit before moving to it is worth
    /// doing, in percentage points. Relative to the threshold, so it tracks it.
    pub margin: u8,
}

impl Policy {
    pub fn of(config: &Settings) -> Policy {
        Policy {
            threshold: config.watcher_threshold_percent,
            margin: config.watcher_margin_percent,
        }
    }

    /// The Utilization a candidate has to be at or under to be worth moving to — the
    /// threshold less the margin.
    ///
    /// Saturating, so a threshold under the margin only moves to an Account with
    /// nothing used at all rather than never moving.
    pub fn ceiling(&self) -> u8 {
        self.threshold.saturating_sub(self.margin)
    }
}

/// [`COOLDOWN_MINUTES`] as the arithmetic reads it.
///
/// A function rather than a method on [`Policy`], because it takes nothing from one: a
/// `cooldown()` asked of a Policy would be two callers threading a value through to
/// reach a constant.
pub fn cooldown() -> Duration {
    Duration::minutes(i64::from(COOLDOWN_MINUTES))
}

/// The Quota Window an Account's fullness is judged by, and how full it is.
///
/// Always the fullest (ADR headroom-is-the-worst-window), and the same window the Cycle
/// ranks candidates on. The reset time is taken off, because "it comes back in twenty
/// minutes" is not a reason to stay on an Account that is full now.
#[derive(Debug, Clone, PartialEq)]
pub struct Fullest {
    pub window: String,
    pub used_percent: f64,
}

impl Fullest {
    /// What an Account's cached figure makes of it, or `None` where there is no figure.
    /// Never read as empty: "no figure" and "plenty of room" are opposite pieces of
    /// advice.
    pub fn of(account: &Account) -> Option<Fullest> {
        crate::cycle::fullest_window_of(account).map(|window| Fullest {
            window: window.window.clone(),
            used_percent: window.used_percent,
        })
    }

    /// Whether the Account is full enough that moving off it is wanted.
    ///
    /// At the threshold rather than past it: an 80 that waited for 81 would be a
    /// Setting that means something other than what it says.
    pub fn at_or_over(&self, threshold: u8) -> bool {
        self.used_percent >= f64::from(threshold)
    }

    /// The same question, answered as something the round can be given rather than as
    /// something it can be told.
    ///
    /// The figure comes back either way, so the number on the line and the number the
    /// decision was taken on cannot come to differ.
    pub fn crossed(self, threshold: u8) -> std::result::Result<Crossed, Fullest> {
        match self.at_or_over(threshold) {
            true => Ok(Crossed { fullest: self }),
            false => Err(self),
        }
    }

    /// Worded against the threshold it is being judged by, so the figure and the status
    /// word cannot disagree: `waiting  80% used` under an opening line declaring 80% is
    /// a self-contradiction.
    fn as_a_clause(&self, threshold: u8) -> String {
        format!(
            "{}% used, fullest {}",
            crate::utilization::percentage_against(self.used_percent, threshold),
            self.window
        )
    }
}

/// A figure read this round, at or over the Threshold: the Account is full enough that
/// moving off it is wanted.
///
/// Not a witness — it carries the figure (ADR an-ordering-is-a-type), which everything
/// after the crossing quotes.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossed {
    fullest: Fullest,
}

/// Why nothing may move yet, though the Threshold was crossed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cooling {
    pub why: String,
}

/// A crossing whose Cooldown is spent, so this round may Switch.
///
/// A witness borrowing the [`Crossed`] it was earned from. The one funnel producing
/// candidate addresses takes one, so reading a candidate inside a Cooldown does not
/// compile.
#[derive(Debug)]
pub struct Cooled<'a>(&'a Crossed);

impl Crossed {
    /// Whether the Cooldown between two Switches has run out.
    ///
    /// Asked before the candidates are read, which is the ordering this type exists to
    /// make unskippable.
    pub fn cooled(
        &self,
        recently: &Recently,
        now: DateTime<Utc>,
    ) -> std::result::Result<Cooled<'_>, Cooling> {
        match recently.resting(now) {
            Some(why) => Err(Cooling { why }),
            None => Ok(Cooled(self)),
        }
    }

    /// The figure that crossed, for the line the round prints.
    ///
    /// Read through here rather than off a public field so that the two ways a round
    /// reaches it are the same spelling.
    pub fn fullest(&self) -> &Fullest {
        &self.fullest
    }
}

impl Cooled<'_> {
    /// The figure the crossing was read on, which is [`Crossed::fullest`]'s.
    pub fn fullest(&self) -> &Fullest {
        self.0.fullest()
    }
}

/// The one thing carried from one round to the next: when the watcher last Switched.
///
/// Where it is carried is the loop's and the check's one difference — in memory, or off
/// the registry ([`Recently::recorded`]) for a fresh process every time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recently {
    switched: Option<DateTime<Utc>>,
}

impl Recently {
    /// A loop that has just started, which owes nobody a wait.
    pub fn nothing() -> Recently {
        Recently::default()
    }

    /// What a scheduled check inherits from the one before it: when its Group last
    /// Switched, or nothing where it never has. Brought back to `now` where it is
    /// later, because the hold runs a Cooldown from the stamp: a clock reading 2035
    /// for the round that Switched would cool that Group until 2035, and only
    /// `perch group remove` — which destroys its Settings — clears a record.
    pub fn recorded(checked: Option<&Checked>, now: DateTime<Utc>) -> Recently {
        Recently {
            switched: checked.map(|checked| checked.switched_at.min(now)),
        }
    }

    pub fn switched(&mut self, at: DateTime<Utc>) {
        self.switched = Some(at);
    }

    /// Why nothing may move yet, or `None` when something may.
    pub fn resting(&self, now: DateTime<Utc>) -> Option<String> {
        let (since, left) = self.left_of_the_cooldown(now)?;
        // The rule is named rather than only described: this line is read out of a cron
        // mailbox by somebody deciding whether to wait, and "so nothing moves" without
        // the reason is a watcher that appears to have stopped.
        Some(format!(
            "the last Switch was {} ago and the cooldown leaves at \
             least {} between two, so nothing moves {}.",
            minutes(since),
            minutes(cooldown()),
            still_to_wait(left),
        ))
    }

    /// How long ago the Switch was and how much of the cooldown is left — both, because
    /// the sentence that quotes one quotes the other.
    ///
    /// The elapsed span is floored, so a stamp left in the future is not said as "-55
    /// minutes ago"; the span *left* is not, or the line would promise less.
    fn left_of_the_cooldown(&self, now: DateTime<Utc>) -> Option<(Duration, Duration)> {
        let switched = self.switched?;
        let elapsed = now - switched;
        let left = cooldown() - elapsed;
        (left > Duration::zero()).then_some((elapsed.max(Duration::zero()), left))
    }
}

/// A span as a count of minutes, for the sentences that quote one. Rounded down,
/// because "another 1 minute" said of fifty seconds is a promise the clock keeps and
/// "another 2" is one it does not. A span under a minute is said rather than counted.
fn minutes(span: Duration) -> String {
    match span.num_minutes() {
        0 => "under a minute".to_string(),
        1 => "1 minute".to_string(),
        count => format!("{count} minutes"),
    }
}

/// How much of the cooldown is left, as the tail of "so nothing moves …".
///
/// Its own phrasing because it is the one span in that sentence that can be under a
/// minute.
fn still_to_wait(left: Duration) -> String {
    match left.num_minutes() {
        0 => "for under a minute more".to_string(),
        _ => format!("for another {}", minutes(left)),
    }
}

/// A candidate as this round read it, ready to be judged against the margin.
///
/// The name travels beside the address because the sentence explaining why an Account
/// was passed over is read by the person who named it, and only the registry knows what
/// they called it.
#[derive(Debug, Clone, PartialEq)]
pub struct Considered {
    pub email: String,
    pub named: String,
    pub fullest: Option<Fullest>,
}

/// The Accounts a Switch may not land on this round, and the one sentence saying why.
/// Set aside before the Strategy ranks them rather than by second-guessing the winner,
/// and a candidate whose Refresh merely failed is judged on what was cached.
pub fn set_aside(
    policy: &Policy,
    scope: &crate::registry::Scope,
    considered: &[Considered],
) -> crate::cycle::SetAside {
    let mut emails = Vec::new();
    let mut clauses = Vec::new();
    for candidate in considered {
        let why = match &candidate.fullest {
            Some(fullest) if fullest.used_percent > f64::from(policy.ceiling()) => format!(
                "{} is at {}% used and nothing over {}% is worth moving to",
                candidate.named,
                crate::utilization::percentage_against(fullest.used_percent, policy.ceiling()),
                policy.ceiling(),
            ),
            Some(_) => continue,
            None => format!(
                "Perch has never observed how full {} is, and a Switch onto \
                 a figure it has not got is a Switch made blind",
                candidate.named,
            ),
        };
        emails.push(candidate.email.clone());
        clauses.push(why);
    }

    // A sentence built out of no clauses is one printed with a hole in it.
    if emails.is_empty() {
        return crate::cycle::SetAside::nothing();
    }
    crate::cycle::SetAside {
        because: format!(
            "Nothing {} is worth Switching to yet — {}. Nothing \
             was changed.",
            scope.within(),
            clauses.join("; "),
        ),
        emails,
    }
}

/// What a round decided.
///
/// One arm moved an Account and every other is a reason nothing did — which is what a
/// person reading the log needs, because each of them resolves itself in a different
/// way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The Account is not full enough to want moving off.
    Waiting,
    /// It was, and the cooldown has not run out since the last Switch. Nothing was read
    /// of the candidates.
    Cooling { why: String },
    /// It was, and this is where it went: the Account by name and nothing about the
    /// ranking (ADR perch-says-what-it-did).
    ///
    /// `unread` is what could not be re-read, and it stays: a candidate ranked on an
    /// old figure can make a Switch land somewhere worse than it left.
    Switched { to: String, unread: Vec<String> },
    /// It was, and there was nowhere worth going — every candidate exhausted, or none
    /// of them a candidate at all. Nothing to fix and nothing to retry: the answer is
    /// to wait, so the loop does — and `looking_again` is absent for a check, which
    /// exits, exactly as [`Outcome::Held`]'s `retrying_in` is.
    Nowhere {
        why: String,
        looking_again: Option<u64>,
    },
    /// The figures could not be read, so nothing was decided on the ones Perch already
    /// had. The only outcome carrying how long the loop then waits.
    ///
    /// Absent for a check, which is exiting: an interval it has no part in would be the
    /// one untrue thing on the line.
    Held {
        why: String,
        retrying_in: Option<u64>,
    },
    /// A Switch was wanted, attempted, and turned away without changing anything — most
    /// often a client running against the Profile the Capture would write into
    /// (ADR a-profile-is-live-by-evidence). Only where waiting is an answer: a failure
    /// that does not clear itself is reported as itself instead. `after_reading` paces
    /// the loop's next round, and `contended` decides the exit code.
    Refused {
        why: String,
        after_reading: bool,
        contended: bool,
    },
    /// The watch was taken over between the reading and the Switch. Its own outcome
    /// rather than a [`Outcome::Refused`], because a scheduler branching on "nothing
    /// to do" would record a round that was in fact displaced.
    HandedOver { why: String },
    /// This Watcher was asked to stop between the reading and the Switch. Told apart
    /// from being replaced because nothing replaced it: the word is what a day of
    /// these is skimmed by, and it is the difference between a machine somebody is
    /// still watching and one nobody is.
    Stopped { why: String },
}

impl Outcome {
    /// What a single check reports to whatever scheduled it.
    ///
    /// Fewer codes than outcomes: those leaving a scheduler the same thing to do share
    /// one, and which of them it was is on the decision line.
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Switched { .. } => EXIT_OK,
            // A lock somebody else holds, whichever end of the round met it: nothing
            // was changed and asking again is what resolves it, which is the one thing
            // `EXIT_NOTHING_TO_DO` would not tell a scheduler.
            Outcome::Refused {
                contended: true, ..
            } => EXIT_HELD,
            // Nothing to do *now*, three ways: the Account is not full enough, the
            // cooldown is not up, or a client was holding the Profile and will not be
            // holding it for long.
            Outcome::Waiting | Outcome::Cooling { .. } | Outcome::Refused { .. } => {
                EXIT_NOTHING_TO_DO
            }
            Outcome::Nowhere { .. } => EXIT_NO_CANDIDATE,
            // Both are the code for a contended lock: nothing was changed, and
            // asking again is what resolves it.
            Outcome::Held { .. } | Outcome::HandedOver { .. } | Outcome::Stopped { .. } => {
                EXIT_HELD
            }
        }
    }

    /// The word the line is read by, in a column so a day of them can be skimmed for
    /// the ones that did something.
    fn word(&self) -> &'static str {
        match self {
            Outcome::Waiting => "waiting",
            Outcome::Cooling { .. } => "cooling",
            Outcome::Switched { .. } => "switched",
            Outcome::Nowhere { .. } => "nowhere",
            Outcome::Held { .. } => "held",
            Outcome::Refused { .. } => "refused",
            // One lowercase token, as every other word is: two of them are eleven
            // cells in a field of eight, and a space where a reader counts columns.
            Outcome::HandedOver { .. } => "replaced",
            Outcome::Stopped { .. } => "stopped",
        }
    }
}

/// What a round makes of a liveness ask that did not come back Idle: an outcome it can
/// report, or a failure it has to raise.
///
/// Every variant answered by name, with no catch-all — a third way for the ask to fail
/// breaks the build here until the round says which of the two it is.
pub fn refused_or_raised(not_idle: NotIdle, installed: &Installed) -> Result<Outcome> {
    match not_idle {
        // Reported as the Switch would have reported it, because it is the same refusal
        // about the same Profile — and waiting is an answer, because the client exits
        // and the round after it moves.
        running @ NotIdle::Live(_) => Ok(Outcome::Refused {
            why: running
                .refusal(installed, &live::NOTHING_WAS_CHANGED)
                .to_string(),
            // Asked before the burst, so nothing has been spent on it.
            after_reading: false,
            contended: false,
        }),
        // This does not clear itself: a `sessions` directory nobody can read is a machine
        // somebody has to look at, so the loop stops rather than deciding.
        unsure @ NotIdle::Unsure(_) => Err(unsure.refusal(installed, &live::NOTHING_WAS_CHANGED)),
    }
}

/// One pass of the loop, whole: what was read, what it was read against, and what came
/// of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    /// How full the Account was, as this round read it. Absent when the read failed,
    /// which is the whole of why nothing was decided.
    ///
    /// Which Account is neither held here nor on the line: it is the active one, which
    /// the opening line names.
    pub fullest: Option<Fullest>,
    /// The Scope's `watcher-threshold-percent`. Not quoted on the line, and carried
    /// because it is what the figure beside it is worded against: see
    /// [`Fullest::as_a_clause`].
    pub threshold: u8,
    pub outcome: Outcome,
}

impl Round {
    /// How long the loop leaves it before the next round.
    ///
    /// Read off the round, so the wait the line promised and the wait taken are the
    /// same number. A round that read a figure is followed by the ordinary interval
    /// whatever it decided about it.
    pub fn waiting_for(&self) -> u64 {
        match &self.outcome {
            Outcome::Held {
                retrying_in: Some(millis),
                ..
            } => *millis,
            // Read off the round, as the hold's is: they agree today only
            // because `act` passes this same constant.
            Outcome::Nowhere {
                looking_again: Some(millis),
                ..
            } => *millis,
            Outcome::Nowhere {
                looking_again: None,
                ..
            } => NOWHERE_INTERVAL_MILLIS,
            // Turned away after every candidate was read, which is the burst
            // `NOWHERE_INTERVAL_MILLIS` exists to stop repeating: an hourly
            // allowance per Account, spent on a Switch that was refused.
            Outcome::Refused {
                after_reading: true,
                ..
            } => NOWHERE_INTERVAL_MILLIS,
            // Named one by one rather than caught by a wildcard, so an outcome added
            // later has to say what the loop does after it. A hold carrying no wait is
            // a check's, and a check exits rather than asking.
            Outcome::Held {
                retrying_in: None, ..
            }
            | Outcome::Waiting
            | Outcome::Cooling { .. }
            | Outcome::Switched { .. }
            | Outcome::Refused {
                after_reading: false,
                ..
            }
            // The loop leaves at the top of the next round rather than waiting this
            // out, so the number is only what the line would have promised.
            | Outcome::HandedOver { .. }
            | Outcome::Stopped { .. } => REFRESH_INTERVAL_MILLIS,
        }
    }

    /// What held this round, or `None` where it decided something.
    ///
    /// What the loop keys its coalescing on. Read off the outcome rather than kept
    /// beside it, so the reason that is compared is the reason that would have been
    /// printed.
    pub fn held_because(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Held { why, .. } => Some(why),
            Outcome::Waiting
            | Outcome::Cooling { .. }
            | Outcome::Switched { .. }
            | Outcome::Nowhere { .. }
            | Outcome::Refused { .. }
            | Outcome::HandedOver { .. }
            | Outcome::Stopped { .. } => None,
        }
    }

    /// The decision line: the stamp, the word it is read by, the figure it was decided
    /// on, and whatever this round has that the opening did not say.
    ///
    /// A whole RFC 3339 instant, because a scheduled Check appends to a file read cold
    /// days later and a Service runs the loop across midnight.
    pub fn line(&self, now: DateTime<Utc>) -> String {
        format!(
            "{}  {:<8}  {}{}",
            now.to_rfc3339_opts(SecondsFormat::Secs, true),
            self.outcome.word(),
            self.figure(),
            self.tail(),
        )
    }

    fn figure(&self) -> String {
        match &self.fullest {
            Some(fullest) => fullest.as_a_clause(self.threshold),
            // Said as a figure that was not read rather than left out: a line missing
            // the number reads as an oversight.
            None => "unread".to_string(),
        }
    }

    /// What this round has to add to the figure, and for most rounds nothing. A round
    /// that refused adds its whole sentence (ADR a-refusal-is-a-promise).
    ///
    /// The em dash is the mark of the second kind: prose follows it, and it is on a
    /// line only where something other than the ordinary happened.
    fn tail(&self) -> String {
        match &self.outcome {
            Outcome::Waiting => String::new(),
            Outcome::Switched { to, unread } => match unread.is_empty() {
                true => format!(" → {to}"),
                false => format!(" → {to}{}", explaining(&unread.join(" "))),
            },
            Outcome::Cooling { why } => explaining(why),
            // The wait is said, as a hold says its own: this is the one decision
            // the loop rests longer than an interval after.
            Outcome::Nowhere {
                why,
                looking_again: Some(millis),
            } => explaining(&format!(
                "nowhere to go: {why} Looking again in {}.",
                how_long(*millis),
            )),
            Outcome::Nowhere {
                why,
                looking_again: None,
            } => explaining(&format!("nowhere to go: {why}")),
            Outcome::Held {
                why,
                retrying_in: Some(millis),
            } => explaining(&format!(
                "nothing current to decide on, so nothing was decided: {why} \
                 Asking again in {}.",
                how_long(*millis),
            )),
            Outcome::Held {
                why,
                retrying_in: None,
            } => explaining(&format!(
                "nothing current to decide on, so nothing was decided: {why}"
            )),
            Outcome::Refused { why, .. } => {
                explaining(&format!("the Switch was turned away: {why}"))
            }
            Outcome::HandedOver { why } | Outcome::Stopped { why } => explaining(why),
        }
    }
}

/// A refusal's sentence, attached to the figure it explains. One shape wherever it is
/// reached, so a log is one column of stamps and words with the same mark wherever
/// prose begins.
fn explaining(said: &str) -> String {
    format!(" — {}", one_line(said))
}

/// A hold that happened before there was a [`Round`] to hold, because another `perch`
/// was holding the registry.
///
/// Said in the same shape as every other line, with the figure it does not have said as
/// unread. `retrying_in` is [`Outcome::Held`]'s.
pub fn held_line(why: &str, retrying_in: Option<u64>, now: DateTime<Utc>) -> String {
    before_a_round(
        Outcome::Held {
            why: why.to_string(),
            retrying_in,
        },
        now,
    )
}

/// The outcome for a round that stopped being the one to act while it was reading,
/// which is the longest thing a round does.
///
/// One sentence for the Landing's reading, the Account's own and the candidates':
/// what a reader needs is that nothing was switched, true of all three.
pub fn nothing_was_switched(lost: Lost) -> Outcome {
    match lost {
        Lost::HandedOver => Outcome::HandedOver {
            why: "the watch was taken over while this round was reading, so \
                  nothing was switched: whoever holds it now is watching this \
                  machine."
                .to_string(),
        },
        Lost::Stopped => Outcome::Stopped {
            why: "this Watcher was asked to stop while the round was reading, \
                  so nothing was switched."
                .to_string(),
        },
    }
}

/// A stop that happened before there was a [`Round`] to stop, because the walk that
/// settles a Landing was still going.
pub fn stopped_line(lost: Lost, now: DateTime<Utc>) -> String {
    before_a_round(nothing_was_switched(lost), now)
}

/// The line for something that happened before a [`Round`] could be reached, as the
/// Round it would have been — so the sentence has one spelling. A figure that was not
/// read renders as `unread` whatever the threshold says, which is why a threshold this
/// round never saw can be any number.
fn before_a_round(outcome: Outcome, now: DateTime<Utc>) -> String {
    Round {
        fullest: None,
        threshold: 0,
        outcome,
    }
    .line(now)
}

/// A message from anywhere else, as one line.
///
/// The Cycle's refusals are written for a terminal and run to two or three lines. A
/// reason that wraps onto its own line stops being attached to the decision it explains
/// the moment two rounds are read together.
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
            fullest,
            threshold: 80,
            outcome,
        }
    }

    /// A throttle, as the round that held for it words it.
    const THROTTLED: &str = "Anthropic is rate-limiting reads of this Account's usage.";
    /// A grant that has not been given, which is the hold that lasts for weeks.
    const UNGRANTED: &str = "`work` has not been told the watcher may act on it.";

    #[test]
    fn a_hold_that_has_not_changed_is_said_once_rather_than_every_round() {
        let mut holding = Holding::nothing();
        let start = now();

        assert_eq!(
            holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), start),
            Speak::InFull,
            "the first one is said in full"
        );
        for round in 1..24 {
            let later = start + Duration::milliseconds(REFRESH_INTERVAL_MILLIS as i64 * round);
            assert_eq!(
                holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), later),
                Speak::Nothing,
                "round {round} repeats a hold already said"
            );
        }
    }

    #[test]
    fn an_hour_of_the_same_hold_says_how_long_it_has_been_holding() {
        let mut holding = Holding::nothing();
        let start = now();
        holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), start);

        let an_hour_on = start + Duration::milliseconds(STILL_HOLDING_MILLIS);
        assert_eq!(
            holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), an_hour_on),
            Speak::StillHolding { since: start },
            "it dates the hold from when it started rather than from the last line"
        );

        // And then goes quiet again for another hour rather than repeating.
        let a_minute_later = an_hour_on + Duration::minutes(1);
        assert_eq!(
            holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), a_minute_later),
            Speak::Nothing,
        );
    }

    #[test]
    fn a_back_off_that_doubles_is_said_again_because_the_line_has_changed() {
        let mut holding = Holding::nothing();
        let mut backoff = Backoff::none();
        let (mut in_full, mut heartbeats) = (0, 0);

        // Forty rounds at the ordinary interval is a hundred minutes, so this covers
        // the back-off saturating *and* an hour passing afterwards.
        for round in 0..40 {
            backoff.failed();
            let at = now() + Duration::milliseconds(REFRESH_INTERVAL_MILLIS as i64 * round);
            match holding.holding(THROTTLED, Some(backoff.waiting_for()), at) {
                Speak::InFull => in_full += 1,
                Speak::StillHolding { .. } => heartbeats += 1,
                Speak::Nothing => {}
            }
        }

        // One line per distinct wait — 2m30, 5m, 10m, 20m — and then silence, because
        // the back-off is bounded and has stopped changing.
        assert_eq!(
            in_full, 4,
            "every cadence the loop settled on is on the record, and no cadence \
             is on it twice"
        );
        // And the hourly line goes on firing underneath it, so a saturated back-off
        // does not read as a Watcher that died an hour ago.
        assert_eq!(heartbeats, 1);
    }

    #[test]
    fn a_hold_that_changes_its_reason_starts_its_own_line_and_its_own_clock() {
        let mut holding = Holding::nothing();
        let start = now();
        holding.holding(THROTTLED, Some(REFRESH_INTERVAL_MILLIS), start);

        let later = start + Duration::minutes(10);
        assert_eq!(
            holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), later),
            Speak::InFull,
        );

        let an_hour_on = later + Duration::milliseconds(STILL_HOLDING_MILLIS);
        assert_eq!(
            holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), an_hour_on),
            Speak::StillHolding { since: later },
            "the new hold is dated from when it started, not from the one it \
             replaced"
        );
    }

    #[test]
    fn a_hold_that_suppressed_anything_says_how_long_it_lasted_when_it_ends() {
        let mut holding = Holding::nothing();
        let start = now();
        holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), start);
        holding.holding(
            UNGRANTED,
            Some(REFRESH_INTERVAL_MILLIS),
            start + Duration::minutes(5),
        );

        let over = start + Duration::minutes(90);
        assert_eq!(holding.released(over), Some(Duration::minutes(90)));
        assert!(
            released_line(Duration::minutes(90), over).contains("1h30m"),
            "{}",
            released_line(Duration::minutes(90), over)
        );
        assert_eq!(
            holding.released(over),
            None,
            "and a hold that is already over ends once"
        );
    }

    #[test]
    fn a_hold_that_suppressed_nothing_adds_no_line_when_it_ends() {
        let mut holding = Holding::nothing();
        holding.holding(UNGRANTED, Some(REFRESH_INTERVAL_MILLIS), now());

        assert_eq!(holding.released(now() + Duration::minutes(2)), None);
    }

    fn at(used_percent: f64) -> Option<Fullest> {
        Some(Fullest {
            window: "5-hour".to_string(),
            used_percent,
        })
    }

    #[test]
    fn every_decision_opens_with_the_stamp_the_word_and_the_figure() {
        for decision in one_of_each_outcome() {
            let line = decision.line(now());

            assert!(line.starts_with("2026-08-04T12:00:00Z"), "{line}");
            assert!(line.contains(decision.outcome.word()), "{line}");
            assert!(!line.trim_end().is_empty(), "{line}");
            assert!(
                !line.contains('\n'),
                "a reason on a second line stops being attached to the decision \
                 it explains: {line}"
            );
        }
    }

    #[test]
    fn no_round_line_quotes_the_threshold() {
        for decision in one_of_each_outcome() {
            let line = decision.line(now());

            assert!(
                !line.contains("threshold"),
                "the header said it once, and this line is not the header: {line}"
            );
        }
        assert!(
            !held_line("Another `perch` holds the registry.", None, now()).contains("threshold"),
            "including the hold that happened before there was a round to hold"
        );
    }

    #[test]
    fn the_two_predictable_outcomes_are_data_and_stop_at_the_figure() {
        assert_eq!(
            round(at(42.0), Outcome::Waiting).line(now()),
            "2026-08-04T12:00:00Z  waiting   42% used, fullest 5-hour",
        );
        assert_eq!(
            round(
                at(86.0),
                Outcome::Switched {
                    to: "overflow@example.com".to_string(),
                    unread: Vec::new(),
                },
            )
            .line(now()),
            "2026-08-04T12:00:00Z  switched  86% used, fullest 5-hour → \
             overflow@example.com",
            "and a Switch still names where it went, which is the one thing \
             about it nobody could have predicted",
        );
    }

    #[test]
    fn a_switch_that_could_not_read_a_candidate_says_so_and_still_names_where_it_went() {
        let line = round(
            at(86.0),
            Outcome::Switched {
                to: "overflow@example.com".to_string(),
                unread: vec![
                    "spare@example.com could not be read, so its figure is the one Perch had."
                        .to_string(),
                ],
            },
        )
        .line(now());

        assert!(line.contains("→ overflow@example.com"), "{line}");
        assert!(
            line.contains("spare@example.com could not be read"),
            "{line}"
        );
        assert!(!line.contains('\n'), "{line}");
    }

    #[test]
    fn every_refusal_still_says_what_is_holding_it() {
        let cooling = round(
            at(86.0),
            Outcome::Cooling {
                why: "the last Switch was 4 minutes ago and the cooldown leaves \
                      at least 15 minutes between two, so nothing moves for \
                      another 11 minutes."
                    .to_string(),
            },
        )
        .line(now());
        assert!(cooling.contains("so nothing moves for another 11 minutes"));

        let nowhere = round(
            at(86.0),
            Outcome::Nowhere {
                why: "every Account in Group `work` is exhausted.".to_string(),
                looking_again: Some(NOWHERE_INTERVAL_MILLIS),
            },
        )
        .line(now());
        assert!(nowhere.contains("nowhere to go"), "{nowhere}");
        assert!(nowhere.contains("is exhausted"), "{nowhere}");

        let refused = round(
            at(86.0),
            Outcome::Refused {
                why: "a client is running against that Profile.".to_string(),
                after_reading: false,
                contended: false,
            },
        )
        .line(now());
        assert!(refused.contains("turned away"), "{refused}");
        assert!(refused.contains("a client is running"), "{refused}");
    }

    /// One of each, for the claims that hold whatever the round decided.
    fn one_of_each_outcome() -> Vec<Round> {
        vec![
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
                    to: "overflow@example.com".to_string(),
                    unread: Vec::new(),
                },
            ),
            round(
                at(86.0),
                Outcome::Nowhere {
                    why: "every Account in Group `work` is exhausted.".to_string(),
                    looking_again: Some(NOWHERE_INTERVAL_MILLIS),
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
                    after_reading: false,
                    contended: false,
                },
            ),
            round(
                at(86.0),
                Outcome::HandedOver {
                    why: "another Watcher has taken the watch over.".to_string(),
                },
            ),
            round(
                at(86.0),
                Outcome::Stopped {
                    why: "this Watcher was asked to stop.".to_string(),
                },
            ),
        ]
    }

    /// A check exits rather than asking again, so its `nowhere` round promises
    /// no interval — and the wait is read off the round, so the arm answering
    /// for that one has to be the loop's own constant.
    #[test]
    fn a_nowhere_round_that_promised_no_interval_still_waits_the_loops_own() {
        let promised = Outcome::Nowhere {
            why: String::new(),
            looking_again: Some(1_234),
        };
        let silent = Outcome::Nowhere {
            why: String::new(),
            looking_again: None,
        };

        assert_eq!(round(at(86.0), promised).waiting_for(), 1_234);
        assert_eq!(
            round(at(86.0), silent).waiting_for(),
            NOWHERE_INTERVAL_MILLIS
        );
    }

    /// The column a day of these is skimmed by. Every word is one lowercase token
    /// in eight cells; two of them push the figure three columns right of every
    /// other round's, and a space in the word breaks anything reading the second.
    #[test]
    fn every_status_word_is_one_token_that_fits_its_column() {
        for decision in one_of_each_outcome() {
            let word = decision.outcome.word();
            assert!(
                !word.contains(char::is_whitespace) && word.len() <= 8,
                "`{word}` does not fit the column every other word does"
            );
        }
    }

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
        assert!(!line.contains('%'), "{line}");
        assert!(line.contains("rate-limiting"), "{line}");
    }

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

    /// The same rule on the other outcome that names a cadence: a Check exits
    /// after printing, so its scheduler's interval decides when anything looks
    /// again — and a line saying otherwise is the one untrue thing on it.
    #[test]
    fn a_check_that_found_nowhere_to_go_promises_nothing_about_looking_again() {
        let line = round(
            at(100.0),
            Outcome::Nowhere {
                why: "Every Account in Group `work` is exhausted.".to_string(),
                looking_again: None,
            },
        )
        .line(now());

        assert!(line.contains("nowhere to go"), "{line}");
        assert!(line.contains("is exhausted"), "{line}");
        assert!(!line.contains("Looking again"), "{line}");
        assert!(!line.contains("m00s"), "no interval at all: {line}");
    }

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

    #[test]
    fn every_outcome_a_check_can_have_reports_a_code_from_the_table() {
        let outcomes = [
            Outcome::Waiting,
            Outcome::Cooling { why: String::new() },
            Outcome::Switched {
                to: String::new(),
                unread: Vec::new(),
            },
            Outcome::Nowhere {
                why: String::new(),
                looking_again: None,
            },
            Outcome::Held {
                why: String::new(),
                retrying_in: None,
            },
            Outcome::Refused {
                why: String::new(),
                after_reading: false,
                contended: false,
            },
            Outcome::HandedOver { why: String::new() },
            Outcome::Stopped { why: String::new() },
        ];

        for outcome in &outcomes {
            let code = outcome.exit_code();
            assert!(
                [EXIT_OK, EXIT_NOTHING_TO_DO, EXIT_NO_CANDIDATE, EXIT_HELD].contains(&code),
                "{outcome:?} exits {code}, which is not in the table a Check \
                 reports on"
            );
        }

        // The distinctions a scheduler acts on: a Switch happened, one could not be
        // decided on, and one was decided against for want of anywhere to go.
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

    #[test]
    fn a_cooling_round_names_the_rule_that_held_it() {
        let mut recently = Recently::nothing();
        recently.switched(now());

        let why = recently
            .resting(now() + Duration::minutes(4))
            .expect("four minutes into a fifteen minute cooldown");

        assert!(why.contains("cooldown"), "{why}");
    }

    #[test]
    fn a_check_is_paced_by_what_the_one_before_it_recorded() {
        let recorded = Recently::recorded(Some(&Checked { switched_at: now() }), now());

        assert!(recorded.resting(now() + Duration::minutes(4)).is_some());
        assert_eq!(
            recorded.resting(now() + Duration::minutes(15)),
            None,
            "one cooldown, counted from the Switch that was recorded rather \
             than from the process that read it"
        );
        assert_eq!(
            Recently::recorded(None, now()),
            Recently::nothing(),
            "a Group nothing has Switched within owes nobody a wait"
        );
    }

    /// The record is written from whatever clock the machine had at the time, and
    /// nothing but `perch group remove` — which destroys the Group's Settings —
    /// clears one.
    #[test]
    fn a_check_recorded_under_a_clock_years_fast_cools_its_group_for_a_cooldown() {
        let skewed = Checked {
            switched_at: now() + Duration::days(3650),
        };

        let recorded = Recently::recorded(Some(&skewed), now());

        assert!(
            recorded.resting(now() + Duration::minutes(4)).is_some(),
            "it still paces the next Check"
        );
        assert_eq!(
            recorded.resting(now() + Duration::minutes(15)),
            None,
            "but a Cooldown from now rather than from 2035"
        );
    }

    #[test]
    fn a_round_that_read_a_figure_is_followed_by_the_ordinary_interval() {
        for outcome in [
            Outcome::Waiting,
            Outcome::Cooling { why: String::new() },
            Outcome::Switched {
                to: String::new(),
                unread: Vec::new(),
            },
            Outcome::Refused {
                why: String::new(),
                after_reading: false,
                contended: false,
            },
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

        // The one decision that read *every* candidate, so keeping the ordinary
        // cadence spends each of their allowances on Accounts it just refused.
        assert_eq!(
            round(
                at(86.0),
                Outcome::Nowhere {
                    why: String::new(),
                    looking_again: Some(NOWHERE_INTERVAL_MILLIS),
                }
            )
            .waiting_for(),
            NOWHERE_INTERVAL_MILLIS,
            "nowhere to go rests for the Cooldown rather than an interval"
        );

        // And the other one: a Switch turned away *after* the burst has spent
        // every candidate's allowance costs exactly what nowhere-to-go costs.
        assert_eq!(
            round(
                at(86.0),
                Outcome::Refused {
                    why: String::new(),
                    after_reading: true,
                    contended: false,
                }
            )
            .waiting_for(),
            NOWHERE_INTERVAL_MILLIS,
            "a refusal that read the candidates rests for the Cooldown too"
        );

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

    #[test]
    fn the_first_failure_is_retried_at_the_ordinary_interval() {
        assert_eq!(after_failing(0), REFRESH_INTERVAL_MILLIS);
        assert_eq!(after_failing(1), REFRESH_INTERVAL_MILLIS);
    }

    #[test]
    fn the_wait_doubles_with_every_failure_and_stops_at_the_longest() {
        let waits: Vec<u64> = (1..=6).map(after_failing).collect();

        assert_eq!(
            waits,
            vec![150_000, 300_000, 600_000, 1_200_000, 1_200_000, 1_200_000],
        );
        assert_eq!(LONGEST_WAIT_MILLIS, 1_200_000, "twenty minutes");
    }

    #[test]
    fn a_failure_that_never_clears_never_comes_back_round_to_the_interval() {
        assert_eq!(after_failing(200), LONGEST_WAIT_MILLIS);
    }

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

    #[test]
    fn a_multi_line_reason_arrives_as_one_line() {
        let line = round(
            at(90.0),
            Outcome::Nowhere {
                why: "Every Account in Group `work` is exhausted, so there is \
                      nowhere useful to Switch. Nothing was changed.\n\
                      overflow@example.com frees up soonest, at 15:00."
                    .to_string(),
                looking_again: Some(NOWHERE_INTERVAL_MILLIS),
            },
        )
        .line(now());

        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("frees up soonest"), "{line}");
    }

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
        Policy::of(&Settings::default())
    }

    fn work() -> crate::registry::Scope {
        crate::registry::Scope::Group("work".to_string())
    }

    #[test]
    fn the_default_policy_moves_you_at_eighty_and_only_onto_seventy_or_better() {
        let policy = policy();
        assert_eq!(policy.threshold, 80);
        assert_eq!(policy.ceiling(), 70);
        assert_eq!(cooldown(), Duration::minutes(15));
    }

    #[test]
    fn a_threshold_under_the_margin_bars_everything_but_an_empty_account() {
        assert_eq!(
            Policy {
                threshold: 5,
                margin: 10
            }
            .ceiling(),
            0
        );
    }

    #[test]
    fn a_scope_that_set_its_own_margin_gets_the_ceiling_it_asked_for() {
        assert_eq!(
            Policy {
                threshold: 80,
                margin: 40
            }
            .ceiling(),
            40,
            "the margin is the one knob that moves a ceiling without moving \
             when you are moved off"
        );
    }

    #[test]
    fn nothing_moves_again_until_the_cooldown_has_run_out() {
        let mut recently = Recently::nothing();
        assert_eq!(
            recently.resting(now()),
            None,
            "a loop that has just started owes nobody a wait"
        );

        recently.switched(now());

        let waiting = recently
            .resting(now() + Duration::minutes(4))
            .expect("four minutes into a fifteen minute cooldown");
        assert!(waiting.contains("4 minutes ago"), "{waiting}");
        assert!(waiting.contains("15 minutes"), "the cooldown: {waiting}");
        assert!(
            waiting.contains("another 11"),
            "and what is left: {waiting}"
        );

        // The last half-minute of it: "another 0 minutes" is not a wait anybody can act
        // on.
        let nearly = recently
            .resting(now() + Duration::seconds(14 * 60 + 30))
            .expect("thirty seconds of the cooldown are left");
        assert!(
            nearly.contains("under a minute more"),
            "the tail end is said rather than counted to nothing: {nearly}"
        );
        assert!(!nearly.contains("0 minutes"), "{nearly}");

        assert_eq!(
            recently.resting(now() + Duration::minutes(15)),
            None,
            "a cooldown of fifteen minutes is over after fifteen minutes, not \
             after sixteen"
        );
    }

    #[test]
    fn a_switch_stamped_in_the_future_is_not_said_to_have_happened_backwards() {
        let mut recently = Recently::nothing();
        recently.switched(now() + Duration::minutes(60));

        let waiting = recently
            .resting(now())
            .expect("a Switch it thinks has happened is one to wait on");
        assert!(
            !waiting.contains('-'),
            "no span in the sentence runs backwards: {waiting}"
        );
        assert!(
            waiting.contains("under a minute ago"),
            "a Switch it cannot have been long since is said as one: {waiting}"
        );

        assert_eq!(
            recently.resting(now() + Duration::minutes(75)),
            None,
            "and the hold still ends, a cooldown after the stamp"
        );

        // The elapsed span is floored so the sentence never runs backwards; the span
        // *left* is not, or the line would promise the cooldown alone — fifteen
        // minutes, against the seventy-five the hold above proves.
        assert!(
            waiting.contains("another 75 minutes"),
            "the wait it promises is the wait it is serving: {waiting}"
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

    #[test]
    fn a_candidate_that_is_barely_emptier_than_the_threshold_is_set_aside() {
        let set_aside = set_aside(
            &policy(),
            &work(),
            &[
                considered("just-under", Some(74.0)),
                considered("at-the-bar", Some(70.0)),
                considered("roomy", Some(5.0)),
            ],
        );

        assert_eq!(set_aside.emails, vec!["just-under@example.com".to_string()]);
        assert!(set_aside.because.contains("74%"), "{}", set_aside.because);
        assert!(
            set_aside.because.contains("70%"),
            "and the figure it was judged against: {}",
            set_aside.because
        );
    }

    #[test]
    fn a_candidate_no_figure_was_ever_read_of_is_set_aside_rather_than_read_as_empty() {
        let set_aside = set_aside(&policy(), &work(), &[considered("unseen", None)]);

        assert_eq!(set_aside.emails, vec!["unseen@example.com".to_string()]);
        assert!(
            set_aside.because.contains("never observed"),
            "{}",
            set_aside.because
        );
    }

    #[test]
    fn a_figure_under_the_threshold_earns_no_crossing_and_comes_back_whole() {
        let under = Fullest {
            window: "5-hour".to_string(),
            used_percent: 42.0,
        };

        let handed_back = under
            .clone()
            .crossed(80)
            .expect_err("42% is not full enough to want moving off");

        assert_eq!(handed_back, under, "the figure the line will quote");
    }

    #[test]
    fn a_figure_at_the_threshold_is_a_crossing_carrying_the_figure_it_crossed_on() {
        let crossed = Fullest {
            window: "5-hour".to_string(),
            used_percent: 80.0,
        }
        .crossed(80)
        .expect("at the threshold is at or over it");

        assert_eq!(crossed.fullest().used_percent, 80.0);
        assert_eq!(crossed.fullest().window, "5-hour");
    }

    #[test]
    fn a_crossing_inside_the_cooldown_is_not_cooled_and_says_which_rule_held_it() {
        let crossed = at(86.0).unwrap().crossed(80).unwrap();
        let mut recently = Recently::nothing();
        recently.switched(now());

        let cooling = crossed
            .cooled(&recently, now() + Duration::minutes(4))
            .expect_err("four minutes into a fifteen minute cooldown");

        assert!(cooling.why.contains("cooldown"), "{}", cooling.why);
        assert!(cooling.why.contains("another 11"), "{}", cooling.why);
    }

    #[test]
    fn a_crossing_with_the_cooldown_spent_is_cooled_and_still_knows_the_figure() {
        let crossed = at(86.0).unwrap().crossed(80).unwrap();
        let mut recently = Recently::nothing();
        recently.switched(now());

        let cooled = crossed
            .cooled(&recently, now() + Duration::minutes(15))
            .expect("a cooldown of fifteen minutes is over after fifteen");

        assert_eq!(cooled.fullest().used_percent, 86.0);

        // A loop that has just started owes nobody a wait.
        assert!(crossed.cooled(&Recently::nothing(), now()).is_ok());
    }

    #[test]
    fn every_way_the_liveness_ask_fails_is_a_refused_round_or_a_raise() {
        let installed = Installed::unknown("1.2.3");

        let refused = refused_or_raised(
            NotIdle::Live(vec![live::Client {
                pid: 4242,
                whose: "someone@example.com's Profile".to_string(),
            }]),
            &installed,
        )
        .expect("a client that will exit is a round that decided, not a failure");
        assert!(
            matches!(&refused, Outcome::Refused { why, .. } if why.contains("pid 4242")),
            "{refused:?}",
        );
        assert_eq!(refused.exit_code(), EXIT_NOTHING_TO_DO);

        let unreadable = refused_or_raised(
            NotIdle::Unsure(live::Unsure::Unlistable {
                dir: std::path::PathBuf::from("/home/someone/.claude/sessions"),
                why: crate::host::HostError::Other("permission denied".to_string()),
            }),
            &installed,
        )
        .expect_err("a directory nobody can read does not clear itself");
        assert_eq!(unreadable.exit_code(), crate::error::EXIT_PROBE_REFUSED);
    }

    #[test]
    fn a_cooling_round_says_what_it_read_and_when_the_cooldown_lifts() {
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
        assert!(line.contains("at least 15 minutes between two"), "{line}");
        assert!(line.contains("another 11 minutes"), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }
}
