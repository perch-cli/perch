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
use crate::registry::{Account, Checked, Settings};

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
/// watcher makes, and a back-off paces questions nobody is answering. They are
/// counted separately because a failure that cleared would otherwise leave the
/// watcher waiting out a rest it never earned.
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

/// How long an unchanged hold goes unsaid before it says it is still there.
///
/// An hour, which is twenty-four ordinary rounds. Long enough that a hold
/// lasting a working day is twenty-odd lines rather than a thousand, and short
/// enough that a log opened at any point has recent evidence the watcher is
/// still awake (ADR 0040).
pub const STILL_HOLDING_MILLIS: i64 = 3_600_000;

/// What to say about a hold, given what has already been said about it.
///
/// ADR 0013 had every held round say which failure held it, and a hold whose
/// line said neither that nor when it would ask again "reads as a watcher that
/// has given up". That was written about a person watching a terminal, where the
/// repeated line *is* the proof of life. A Service writes to a log nobody reads
/// until something is wrong, and a permission hold repeats until somebody
/// changes a setting — possibly for weeks. At one line every two and a half
/// minutes, what five hundred and seventy-six identical lines a day bury is the
/// one line that matters.
///
/// So the proof of life moves from repetition to duration: a hold says itself in
/// full when it starts, says how long it has been going once an hour, and says
/// what it cost when it ends. "Held since 09:14" is better evidence of a stuck
/// watcher than the same sentence four hundred times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speak {
    /// Say the whole line. This hold is new, or it is not the one that was
    /// being held a moment ago.
    InFull,
    /// Say only that it is still there, and since when.
    StillHolding { since: DateTime<Utc> },
    /// Say nothing: the same hold, said recently enough.
    Nothing,
}

/// The hold a loop is currently in, and what has been said about it.
///
/// In memory and nowhere else, like the [`Backoff`] and the [`Recently`] beside
/// it: what has been said is about this loop's own output, and a second loop's
/// log is not paced by this one's. A [`Check`](crate::commands::watch) has no
/// use for it at all — one process says its one line and leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holding {
    said: Option<Said>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    /// What is holding it, and when it said it would ask again — both, because
    /// both are what ADR 0013 requires a held line to carry, and a hold that has
    /// changed either of them has changed.
    ///
    /// The interval is in here rather than left out as an implementation
    /// detail, and it is the whole reason this is a pair. Keyed on the reason
    /// alone, a throttled endpoint said "asking again in 2m30s" once and then
    /// went quiet while the [`Backoff`] doubled underneath it — so the log
    /// claimed a cadence of two and a half minutes for a watcher that had
    /// settled at twenty. A changed cadence is news, and it is said; a back-off
    /// that has saturated stops changing and stops being said, which is the
    /// quiet the coalescing was for.
    said: HoldSaid,
    since: DateTime<Utc>,
    last_said: DateTime<Utc>,
    /// Whether anything has gone unsaid under this hold. What decides if there
    /// is anything worth saying when it ends: a hold that said every one of its
    /// rounds has already told the whole story.
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
                    return Speak::StillHolding { since: said.since };
                }
                said.suppressed = true;
                Speak::Nothing
            }
            // A different reason is a different hold, and starts its own hour.
            // Not folded in with the one before it: a watcher that moved from a
            // throttled endpoint to a withdrawn permission has had two things
            // happen, and a reader who is shown one of them is being told the
            // wrong thing about their machine.
            // A hold that has changed either half starts its own line — but not
            // its own clock. `since` is carried over from the hold it replaces
            // when the *reason* is the same, because a throttled endpoint that
            // has been refusing for two hours has been refusing for two hours
            // however many times the back-off doubled in between, and an hourly
            // line that reset every doubling would be saying the wrong number
            // about the thing a reader is trying to judge.
            was => {
                // `suppressed` travels with `since` for the same reason: what
                // the line at the end of a hold reports is the whole hold, and
                // a back-off doubling in the middle of one does not un-suppress
                // the rounds that went unsaid before it.
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

    /// A round that did not hold, and how long the hold it ended had lasted —
    /// or `None` where there was nothing to end, or where nothing went unsaid
    /// under it.
    ///
    /// The way out is always said, which is the half of ADR 0013's rule that
    /// survives untouched: the ordinary decision line prints whatever this round
    /// decided, and this adds what the silence before it was. A hold that never
    /// suppressed a line needs no such sentence — every round of it is already
    /// in the log.
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

/// A span, as the two lines above quote one. Minutes up to an hour and then
/// hours and minutes, because a hold is interesting at the scale it has lasted
/// and never at the second.
fn for_how_long(span: Duration) -> String {
    let minutes = span.num_minutes().max(0);
    match minutes {
        0..=59 => format!("{minutes}m"),
        _ => format!("{}h{:02}m", minutes / 60, minutes % 60),
    }
}

/// The least wall-clock the watcher leaves between two Switches (ADR 0046).
///
/// A constant rather than a Setting, on the same test [`REFRESH_INTERVAL_MILLIS`]
/// is a constant by: a five-hour window moves slowly enough that fifteen minutes
/// never misses a real crossing, and often enough that a watcher which has just
/// moved you is not about to move you again. That is arithmetic about how fast a
/// Quota Window moves rather than anyone's taste, and what a person is left to
/// express — how promptly they are moved after a real crossing — has a right
/// answer rather than a preference.
pub const COOLDOWN_MINUTES: u32 = 15;

/// How far under the threshold a candidate has to sit before moving to it is
/// worth doing, in percentage points (ADR 0046).
///
/// Relative to the threshold rather than an absolute figure, so it tracks the
/// threshold as it changes: "10 points under" still means something once
/// somebody sets the threshold to 60, where a fixed 70 would quietly stop
/// meaning anything.
///
/// A constant because nobody wants the low end and the high end is already
/// reachable by moving the threshold. At a margin of nothing the two predicates
/// contradict each other — [`Fullest::at_or_over`] leaves on `>=` while
/// [`set_aside`] keeps a candidate on `>`, so an Account at exactly the
/// threshold would be full enough to leave and clear enough to arrive at.
pub const MARGIN_PERCENT: u8 = 10;

/// The rule a Group gives the watcher for Switching within it (ADR 0046).
///
/// One number, because only one of the watcher's four pacing questions turned
/// out to be anyone's: *when* a move is wanted. How often one may happen and how
/// much better the destination has to be are [`COOLDOWN_MINUTES`] and
/// [`MARGIN_PERCENT`], arithmetic in the same way [`REFRESH_INTERVAL_MILLIS`]
/// is, and whether the Account just left counts was a rule that could never
/// fire.
///
/// A copy taken at the top of a round rather than a borrow of the Group, so the
/// round decides against one threshold throughout: a `perch config set` landing
/// mid-round cannot make the figure a candidate is set aside by differ from the
/// one the decision line quotes it against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// How full the Account you are on has to be before moving off it is
    /// wanted, as a percentage of its fullest Quota Window.
    pub threshold: u8,
}

impl Policy {
    pub fn of(config: &Settings) -> Policy {
        Policy {
            threshold: config.watcher_threshold_percent,
        }
    }

    /// The Utilization a candidate has to be at or under to be worth moving to
    /// — the threshold less [`MARGIN_PERCENT`].
    ///
    /// Named for the figure rather than for the rule, because the rule is the
    /// margin and the two would end up meaning each other.
    ///
    /// Saturating, so a threshold under the margin is a Group that will only
    /// move to an Account with nothing used at all rather than one that will
    /// never move: a threshold of 5 is a coherent thing to ask for, and refusing
    /// it here would be the margin vetoing the one Setting that is a preference.
    pub fn ceiling(&self) -> u8 {
        self.threshold.saturating_sub(MARGIN_PERCENT)
    }
}

/// [`COOLDOWN_MINUTES`] as the arithmetic reads it.
///
/// A function rather than a method on [`Policy`], because it takes nothing from
/// one: a `cooldown()` that never touched the Policy it was asked of would have
/// two callers threading a Policy through to reach a constant, which is the
/// dead parameter ADR 0046 has just finished removing four of.
pub fn cooldown() -> Duration {
    Duration::minutes(i64::from(COOLDOWN_MINUTES))
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

    /// Said beside the threshold it is being judged against, so the figure and
    /// the verdict on the same line cannot disagree.
    fn as_a_clause(&self, threshold: u8) -> String {
        format!(
            "{}% used, fullest {}",
            crate::utilization::percentage_against(self.used_percent, threshold),
            self.window
        )
    }
}

/// The one thing carried from one round to the next: when the watcher last
/// Switched.
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
    switched: Option<DateTime<Utc>>,
}

impl Recently {
    /// A loop that has just started, which owes nobody a wait.
    pub fn nothing() -> Recently {
        Recently::default()
    }

    /// What a scheduled check inherits from the one before it: when its Group
    /// last Switched, or nothing where it never has.
    ///
    /// Only the stamp, though [`Checked`] also records the Account that was left
    /// — nothing paces itself on *which* Account any more (ADR 0046), and the
    /// record is kept against the day something does.
    pub fn recorded(checked: Option<&Checked>) -> Recently {
        Recently {
            switched: checked.map(|checked| checked.switched_at),
        }
    }

    pub fn switched(&mut self, at: DateTime<Utc>) {
        self.switched = Some(at);
    }

    /// Why nothing may move yet, or `None` when something may.
    ///
    /// The cooldown is the floor under how often the watcher acts, and it is
    /// checked before the candidates are read rather than after: a round that
    /// cannot act has no business spending an allowance on figures it will not
    /// use.
    pub fn resting(&self, now: DateTime<Utc>) -> Option<String> {
        let (since, left) = self.left_of_the_cooldown(now)?;
        // The rule is named rather than only described, because a check's line
        // is read out of a cron mailbox by somebody deciding whether to wait,
        // and "so nothing moves" without the reason is a watcher that appears
        // to have stopped.
        Some(format!(
            "the last Switch was {} ago and the cooldown leaves at \
             least {} between two, so nothing moves {}.",
            minutes(since),
            minutes(cooldown()),
            still_to_wait(left),
        ))
    }

    /// How long ago the Switch the cooldown is running from was, and how much of
    /// the cooldown is left — or `None` where none of it is, which is also the
    /// answer when nothing has been Switched yet.
    ///
    /// Both, because the sentence that quotes one quotes the other, and asking
    /// separately made the caller repeat a question this had already answered.
    ///
    /// The elapsed span is floored at nothing, so a Switch stamped in the
    /// future is treated as one that has just happened. `checks` is read off
    /// disk and a `--once` watcher is the only thing that writes it, so the
    /// stamp survives a clock the machine steps backwards — and unfloored, an
    /// hour of skew held the Group for its cooldown *plus* the hour and said
    /// "the last Switch was -55 minutes ago" while it did. `age_phrase` already
    /// refuses to make that claim about a reading; this is the same refusal
    /// about a Switch.
    ///
    /// Floored for the sentence alone. How much is *left* is arithmetic about
    /// what the loop will actually do, and the loop will in fact go on holding
    /// for the skew plus the cooldown — so computing it from the floored span
    /// promised a wait a fraction as long as the one being served. A cron
    /// `--once` after an NTP step backwards mailed "so nothing moves for another
    /// 15 minutes" every five minutes for an hour and a quarter, each one true
    /// of a clock nobody has. The module doc calls this line the whole of the
    /// evidence that the policy works; a number that is not the policy's is not
    /// evidence of it.
    fn left_of_the_cooldown(&self, now: DateTime<Utc>) -> Option<(Duration, Duration)> {
        let switched = self.switched?;
        let elapsed = now - switched;
        let left = cooldown() - elapsed;
        (left > Duration::zero()).then_some((elapsed.max(Duration::zero()), left))
    }
}

/// A span as a count of minutes, for the sentences that quote one. Rounded
/// down, because "another 1 minute" said of fifty seconds is a promise the
/// clock keeps and "another 2" is one it does not.
///
/// A span under a minute is said rather than counted: "0 minutes" is not a
/// length of time anybody can act on, and this line is read out of a cron
/// mailbox by somebody deciding whether to wait.
fn minutes(span: Duration) -> String {
    match span.num_minutes() {
        0 => "under a minute".to_string(),
        1 => "1 minute".to_string(),
        count => format!("{count} minutes"),
    }
}

/// How much of the cooldown is left, as the tail of "so nothing moves …".
///
/// Its own phrasing because it is the one span in that sentence that can be
/// under a minute, and neither "for another 0 minutes" nor "for another under a
/// minute" is something to read at six in the morning.
fn still_to_wait(left: Duration) -> String {
    match left.num_minutes() {
        0 => "for under a minute more".to_string(),
        _ => format!("for another {}", minutes(left)),
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
/// The margin is what refuses a destination nearly as full as the Account being
/// left (ADR 0046): at an 80% threshold nothing is moved to unless it is at 70%
/// or better. Two Accounts do not trade places — usage climbs on the one you are
/// on and stands still on the ones you are not, so without the margin they walk
/// upward together and every move lands you somewhere with almost nothing left.
/// Setting the candidate aside turns that round into `Exhausted` instead, which
/// is the true answer: there is nowhere better to go.
///
/// It is applied by setting candidates aside rather than by second guessing the
/// winner, because the Strategy is entitled to rank whatever clears the bar — a
/// `soonest-reset` Group would otherwise be told there is nowhere to go while a
/// perfectly empty Account sat behind the fullest one.
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
    scope: &crate::cycle::Scope,
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

    // Nothing set aside is nothing to explain, and a sentence built out of no
    // clauses would be one waiting to be printed with a hole in it.
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
    /// It was, and the cooldown has not run out since the last Switch.
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
    /// because this one is about the machine rather than about the quota.
    ///
    /// Only where waiting is an answer: a client that will exit, or an Account
    /// this round found unusable and Quarantined, which the next round passes
    /// over. A failure that clears itself is what earns "nothing to do now",
    /// and a failure that does not — a keychain nobody can reach, a Profile
    /// that will not be written — is reported as itself instead.
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

    /// What held this round, or `None` where it decided something.
    ///
    /// What the loop keys its coalescing on (ADR 0040): two consecutive rounds
    /// held by the same thing are one hold, and the second of them is not worth
    /// a line. Read off the outcome rather than kept beside it, so the reason
    /// that is compared is the reason that would have been printed.
    pub fn held_because(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Held { why, .. } => Some(why),
            Outcome::Waiting
            | Outcome::Cooling { .. }
            | Outcome::Switched { .. }
            | Outcome::Nowhere { .. }
            | Outcome::Refused { .. } => None,
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
            Some(fullest) => fullest.as_a_clause(self.threshold),
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

/// A hold that happened before there was a [`Round`] to hold.
///
/// The loop cannot always get as far as reading the registry — another `perch`
/// may be holding it — and a round that never learned which Account it was
/// watching has no Account to name and no threshold to quote. Said in the same
/// shape as every other line so a log stays one column of timestamps and words,
/// with the fields it does not have said as unread rather than left blank,
/// which is how [`Round::figure`] already says a figure that was not read.
/// `retrying_in` is `None` where nothing here decides when the next reading is,
/// which is a `perch watch --once`: it is exiting, and when it comes back is
/// whatever scheduled it to say. The same distinction [`Outcome::Held`] already
/// carries, and for the same reason — promising an interval this process has no
/// part in would be the one thing on the line that was not true.
pub fn held_line(why: &str, retrying_in: Option<u64>, now: DateTime<Utc>) -> String {
    let asking_again = match retrying_in {
        Some(millis) => format!(" Asking again in {}.", how_long(millis)),
        None => String::new(),
    };
    format!(
        "{}  {:<8}  unread unread; threshold unread — {}",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        "held",
        one_line(&format!(
            "nothing current to decide on, so nothing was decided: \
             {why}{asking_again}",
        )),
    )
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

    /// A throttle, as the round that held for it words it.
    const THROTTLED: &str = "Anthropic is rate-limiting reads of this Account's usage.";
    /// A grant that has not been given, which is the hold that lasts for weeks.
    const UNGRANTED: &str = "`work` has not been told the watcher may act on it.";

    /// The whole of what the coalescing is for: a hold that repeats says itself
    /// once, and the rounds that follow it are silence rather than five hundred
    /// identical lines a day (ADR 0040).
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

    /// And the proof of life the repetition used to be: an hour in, it says how
    /// long rather than saying the same thing again. A held Watcher that said
    /// nothing at all for a week would be indistinguishable from a dead one.
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

    /// **The bug this was written with and fixed.** Keyed on the reason alone, a
    /// throttled endpoint said "asking again in 2m30s" once and then went silent
    /// while the back-off doubled underneath it — so the log claimed a cadence
    /// of two and a half minutes for a Watcher that had settled at twenty.
    ///
    /// A changed cadence is news. A back-off that has saturated stops changing,
    /// and stops being said, which is the quiet the coalescing was for.
    #[test]
    fn a_back_off_that_doubles_is_said_again_because_the_line_has_changed() {
        let mut holding = Holding::nothing();
        let mut backoff = Backoff::none();
        let (mut in_full, mut heartbeats) = (0, 0);

        // Forty rounds at the ordinary interval is a hundred minutes, so this
        // covers the back-off saturating *and* an hour passing afterwards.
        for round in 0..40 {
            backoff.failed();
            let at = now() + Duration::milliseconds(REFRESH_INTERVAL_MILLIS as i64 * round);
            match holding.holding(THROTTLED, Some(backoff.waiting_for()), at) {
                Speak::InFull => in_full += 1,
                Speak::StillHolding { .. } => heartbeats += 1,
                Speak::Nothing => {}
            }
        }

        // One line per distinct wait — 2m30, 5m, 10m, 20m — and then silence,
        // because the back-off is bounded and has stopped changing.
        assert_eq!(
            in_full, 4,
            "every cadence the loop settled on is on the record, and no cadence \
             is on it twice"
        );
        // And the hourly line goes on firing underneath it, which is what keeps
        // a saturated back-off from reading as a Watcher that died an hour ago.
        assert_eq!(heartbeats, 1);
    }

    /// A hold that changes reason is a different thing happening to the machine,
    /// and is said as one — a Watcher that moved from a throttled endpoint to a
    /// withdrawn grant has had two things happen.
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

    /// The way out is always said, and it is said as what the silence cost.
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

    /// A hold that said every one of its rounds has already told the whole
    /// story, so there is nothing for the way out to add.
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
    /// read by somebody who has to know what they are waiting for.
    #[test]
    fn a_cooling_round_names_the_rule_that_held_it() {
        let mut recently = Recently::nothing();
        recently.switched(now());

        let why = recently
            .resting(now() + Duration::minutes(4))
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

        assert!(recorded.resting(now() + Duration::minutes(4)).is_some());
        assert_eq!(
            recorded.resting(now() + Duration::minutes(15)),
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
        Policy::of(&Settings::default())
    }

    fn work() -> crate::cycle::Scope {
        crate::cycle::Scope::Group("work".to_string())
    }

    /// The Group's default, and the two constants read off it (ADR 0046).
    #[test]
    fn the_default_policy_moves_you_at_eighty_and_only_onto_seventy_or_better() {
        let policy = policy();
        assert_eq!(policy.threshold, 80);
        assert_eq!(policy.ceiling(), 70);
        assert_eq!(cooldown(), Duration::minutes(15));
    }

    /// A threshold under the margin is a Group that will only move onto an
    /// Account with nothing used, rather than one that has quietly stopped
    /// moving at all. A threshold of 5 is a coherent thing to ask for, and the
    /// margin is not entitled to veto it.
    #[test]
    fn a_threshold_under_the_margin_bars_everything_but_an_empty_account() {
        assert_eq!(Policy { threshold: 5 }.ceiling(), 0);
    }

    /// The cooldown is a floor under how often the watcher acts, and it is
    /// counted from the Switch rather than from the round.
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

        // The last half-minute of it. "another 0 minutes" is not a wait
        // anybody can act on, and this is a line read out of a cron mailbox by
        // somebody deciding whether to sit and wait for it.
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

    /// A Switch stamped ahead of the clock reading it.
    ///
    /// `checks` is written by a `perch watch --once` and read back by the next
    /// one, so the stamp outlives the clock that made it — an NTP step
    /// backwards, or a machine that was running fast, puts a Switch in the
    /// future. Unfloored, the elapsed span went negative and the line read "the
    /// last Switch was -55 minutes ago", which is not a length of time anybody
    /// reading a cron mailbox can act on.
    ///
    /// Only the sentence is floored. The hold still runs a cooldown from the
    /// stamp, so a stamp an hour ahead holds the Group for an hour and the
    /// cooldown — which is the direction to err in: a stamp Perch cannot trust
    /// is a reason to Switch less often rather than more, and the wait ends
    /// without anybody having to repair anything.
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

        // What the line promises has to be the wait it is actually serving. The
        // elapsed span is floored so the sentence never runs backwards; the span
        // *left* was floored with it, so it quoted the cooldown alone — 15
        // minutes, against the 75 the hold above proves. Every five minutes for
        // an hour and a quarter, a cron mailbox got the same wrong number.
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

    /// The margin, which is the whole of what stops the walk upward: at an 80%
    /// threshold nothing over 70% may be moved to, so the Account that is only
    /// just emptier than the one you are on is passed over.
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

    /// The Cycle ranks an Account nobody has ever read above an exhausted one,
    /// which is right for a Switch somebody asked for. Unasked it is a move onto
    /// an Account the watcher knows nothing about, and "no figure" is not
    /// evidence of room.
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
