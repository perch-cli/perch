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
use crate::registry::{self, Account, CachedUtilization, Registry, Strategy, WindowUtilization};
use crate::utilization;

/// Where a Cycle may look for a landing place.
///
/// Deliberately not [`crate::commands::list::Scope`], which is the same idea
/// for a listing and carries an `Everything` besides. A Cycle never leaves the
/// scope it started in (ADR 0002): a work subscription running dry must not
/// land on a personal Account. Sharing the type would make "every Account" a
/// thing a Cycle could be handed, and the rule that stops it would move from
/// the type into a runtime check somebody has to remember to write. Two small
/// enums that cannot express each other's mistakes are the cheaper pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The Accounts in one Group, named as the Group was declared.
    Group(String),
    /// The Accounts in no Group at all — a scope only because a global setting
    /// says those Accounts are interchangeable (ADR 0017).
    Ungrouped,
}

impl Scope {
    /// The Accounts this scope holds, ranked or not.
    ///
    /// Public because a Group is also something to *look* at rather than only
    /// something to Cycle within: what a Group has left to draw on
    /// ([`crate::reserve`]) is measured over exactly the Accounts a Cycle could
    /// land on, and a second idea of which Accounts those are is how the figure
    /// on screen comes to describe a different set from the one that gets
    /// chosen.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Group(name) => registry.accounts_in(name),
            Scope::Ungrouped => registry.ungrouped_accounts(),
        }
    }

    /// The Scope this one is at the Config layer: the same set of Accounts,
    /// said at the level a Setting means something (ADR 0002, amended).
    pub fn config(&self) -> registry::Scope {
        match self {
            Scope::Group(name) => registry::Scope::Group(name.clone()),
            Scope::Ungrouped => registry::Scope::Ungrouped,
        }
    }

    /// The word that names this scope wherever a Setting belonging to it is
    /// recorded against it — what the last scheduled Check did, for one.
    ///
    /// A Group cannot be called `ungrouped` ([`registry::validate_name`]), so
    /// the two never collide.
    pub fn key(&self) -> String {
        match self {
            Scope::Group(name) => name.clone(),
            Scope::Ungrouped => registry::UNGROUPED.to_string(),
        }
    }

    /// Which Account this scope prefers when more than one would serve.
    ///
    /// Read from the Config in force for the scope: the Scope's own Override
    /// where it holds one, and Global's otherwise (ADR 0002, amended). The
    /// Accounts in no Group are still not a Group — they hold no Strategy of
    /// their own until somebody sets one — but they are a Scope now, so what
    /// they Cycle by is finally something a person can say rather than a
    /// constant compiled into Perch (ADR 0017, amended).
    fn strategy(&self, registry: &Registry) -> Strategy {
        registry.in_force(&self.config()).strategy
    }

    /// The scope as an adverbial phrase: "Nothing {} is worth Switching to".
    ///
    /// The Config layer's, because it is the same phrase about the same set of
    /// Accounts ([`registry::Scope::within`]) — and not [`Scope::described`],
    /// which is the middle of "every Account in {}" and reads as "in no Group",
    /// true there and ungrammatical the moment anything else is said around it.
    pub fn within(&self) -> String {
        self.config().within()
    }

    /// What the Cycle is about to do, said before it does it.
    pub fn announcement(&self) -> String {
        match self {
            Scope::Group(name) => format!("Cycling within Group `{name}`."),
            Scope::Ungrouped => "Cycling among the Accounts in no Group.".to_string(),
        }
    }

    /// The scope as the middle of a sentence: "every Account in {}".
    pub fn described(&self) -> String {
        match self {
            Scope::Group(name) => crate::commands::list::group_heading(name),
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
    fn ranking(&self, strategy: Strategy, now: DateTime<Utc>) -> (u8, f64) {
        match self.ranked_on_reset(strategy, now) {
            // Sooner is better, so the figure sorted on is the reset time
            // negated.
            Some(at) => (3, -(at.timestamp() as f64)),
            None => self.by_room(),
        }
    }

    /// The same ordering with the Strategy left out of it: how much is left, and
    /// nothing about when it comes back.
    ///
    /// The bottom three tiers of [`ranking`], which is what they always were —
    /// `soonest-reset` only ever added a tier on top. Named on its own because
    /// one question genuinely wants it: whether moving would gain any room. A
    /// Strategy says which of the places worth going is preferred, and that is a
    /// different question from whether it is worth going anywhere.
    ///
    /// [`ranking`]: Headroom::ranking
    fn by_room(&self) -> (u8, f64) {
        match self {
            Headroom::Room { percent, .. } => (2, *percent),
            Headroom::Unobserved => (1, 0.0),
            Headroom::Exhausted { .. } => (0, 0.0),
        }
    }

    /// Whether this Account's fullest window has a reset that has not happened
    /// yet.
    ///
    /// A cached figure outlives the window it describes (ADR 0015), so a
    /// `resets_at` in the past is ordinary rather than strange — it means the
    /// window has already come back and the percentage beside it is stale. What
    /// made that a bug is the direction it sorted: the key is the reset time
    /// negated, so an earlier time ranks higher, and among Accounts whose reset
    /// had already passed the *stalest* figure won. A six-hour-old reading of
    /// an Account at 10% headroom beat a one-minute-old reading of one at 90%,
    /// and `chosen_because` announced it as resetting "any moment now" about a
    /// window that came back hours ago.
    ///
    /// An elapsed reset is no longer a fact about when this Account comes back,
    /// so it does not rank as one, and the Account falls to the headroom key
    /// beside every other Account the Strategy could not get a reset for.
    /// One answer, because everything that has to agree about it asks here:
    /// the key that sorts the Accounts, the sentence that says why one won, and
    /// the sentence that says why staying put is already the best there is.
    /// Three questions phrased three ways is how the ranking came to be fixed
    /// and the sentence beside it did not.
    fn ranked_on_reset(&self, strategy: Strategy, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match (self, strategy) {
            (
                Headroom::Room {
                    resets_at: Some(at),
                    ..
                },
                Strategy::SoonestReset,
            ) if *at > now => Some(*at),
            _ => None,
        }
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
        let percent = utilization::percentage(*percent);
        // Asked of the same predicate the ranking asked, because a number
        // quoted as the reason has to be the number that decided it. Reading
        // `resets_at` directly here is what let this quote a reset time as
        // "any moment now" about a window that came back hours ago — in the
        // same sentence as the clause explaining there was no reset to rank on.
        Some(match (strategy, self.ranked_on_reset(strategy, now)) {
            (Strategy::MostHeadroom, _) => format!(
                "{percent}% headroom, which is true of every one of its Quota \
                 Windows — {fullest_window} is its fullest, as of {age}"
            ),
            (Strategy::SoonestReset, Some(at)) => format!(
                "{percent}% headroom, and the window that leaves it least — \
                 {fullest_window} — resets at {}, as of {age}",
                utilization::reset_phrase(at, now),
            ),
            // Ranked on its room, because that is what there was to rank it on.
            // A reset that has already elapsed is said as one: the figure is
            // stale rather than absent, and "no cached figure says when it
            // comes back" would be untrue of an Account whose cache says
            // exactly that about a time now past.
            (Strategy::SoonestReset, None) => match resets_at {
                // The clock time alone, because the clause says which side of
                // now it falls on itself. `reset_phrase` renders a time already
                // gone as "any moment now" — which is the right thing to say
                // about a window somebody is waiting for, and reads as a
                // contradiction two words before "which has passed".
                Some(at) => format!(
                    "{percent}% headroom, and the window that leaves it least — \
                     {fullest_window} — was due back at {}, which has passed, so \
                     there was no reset still to come to rank it on, as of {age}",
                    utilization::clock_time(*at),
                ),
                None => format!(
                    "{percent}% headroom, which is true of every one of its Quota \
                     Windows — {fullest_window} is its fullest — and no cached \
                     figure says when that one comes back, as of {age}"
                ),
            },
        })
    }
}

/// What reads a figure Perch has not got, said the same way wherever the
/// absence of one is the reason for the answer.
const HOW_TO_GET_FIGURES: &str = "`perch status --group --refresh` reads current figures.";

/// The Quota Window that decides how full an Account is: its fullest (ADR
/// 0012), or `None` for one nothing has ever been observed of.
///
/// Public because the watcher compares the Account it is on against a
/// threshold, and that comparison has to be against the same figure the ranking
/// below is made on. Two measures of fullness would be a watcher that acts on
/// one number and chooses on another, and the day they disagreed would be the
/// day it switched off an Account that was fine onto one that was not.
pub fn fullest_window_of(account: &Account) -> Option<&WindowUtilization> {
    account.observed_utilization().and_then(fullest_window)
}

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

/// The fullest window, and on a tie the one that bites first.
///
/// Ties are ordinary rather than exotic: Anthropic answers in whole
/// percentages, and an Account nothing has been spent on is at nought in every
/// window. `max_by` hands back the *last* of several equal maxima, and
/// `windows_in` sorts `5-hour`, then `7-day`, then a window per model — so a tie
/// always resolved to the longest-period window, which is the one least likely
/// to say when it resets.
///
/// That fed the wrong `resets_at` into [`Headroom::Room`], whose doc says the
/// window deciding the headroom is the window whose reset decides how
/// perishable that headroom is. An Account whose `5-hour` quota is thrown away
/// in an hour, tied with a `7-day-sonnet` carrying no reset, ranked under
/// `soonest-reset` as an Account with no reset at all — behind one that resets
/// in three hours. So the tie is broken on perishability: the soonest reset
/// first, and a window that says when it comes back ahead of one that does not.
fn fullest_window(cached: &CachedUtilization) -> Option<&WindowUtilization> {
    /// Most perishable first. `is_none` leads so that a window saying nothing
    /// about its reset sorts behind every window that does, rather than ahead
    /// of them the way a bare `Option` orders.
    fn perishability(window: &WindowUtilization) -> (bool, Option<DateTime<Utc>>) {
        (window.resets_at.is_none(), window.resets_at)
    }

    cached.windows.iter().min_by(|left, right| {
        right
            .used_percent
            .total_cmp(&left.used_percent)
            .then_with(|| perishability(left).cmp(&perishability(right)))
    })
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

/// Accounts this Cycle may not land on, whatever the ranking makes of them, and
/// the one sentence that says why.
///
/// The Cycle has no opinion about them. The watcher's margin and its no-return
/// (ADR 0013) are policy about *when* a move is worth making, which is a
/// different question from which Account is best, and answering both here would
/// put a watcher's clock inside the ranking that every `perch switch` uses.
/// They arrive as a list so that the ranking never lands on one, and with a
/// sentence so that the refusal — when they turn out to be all of them — says
/// what set them aside rather than claiming the Group is empty.
///
/// A `perch switch` the user typed sets nothing aside: [`SetAside::nothing`].
#[derive(Debug, Clone, Default)]
pub struct SetAside {
    /// The Accounts, by email.
    pub emails: Vec<String>,
    /// Why, whole, ready to print in place of a refusal the Cycle would have
    /// written itself.
    pub because: String,
}

impl SetAside {
    /// Every Account is fair game, which is what asking for a Cycle yourself
    /// means.
    pub fn nothing() -> SetAside {
        SetAside::default()
    }

    fn holds(&self, email: &str) -> bool {
        self.emails.iter().any(|held| held == email)
    }
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
///
/// `set_aside` is the caller's own reasons for not landing somewhere, which the
/// ranking obeys without holding an opinion about (see [`SetAside`]).
pub fn choose(
    registry: &Registry,
    scope: &Scope,
    leaving: Option<&str>,
    set_aside: &SetAside,
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
        .filter(|account| is_a_candidate(account))
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
        let (theirs, them) = right.headroom.ranking(strategy, now);
        let (ours, us) = left.headroom.ranking(strategy, now);
        theirs.cmp(&ours).then(them.total_cmp(&us))
    });

    if ranked.iter().all(|ranked| ranked.headroom.is_exhausted()) {
        return Err(PerchError::NoCandidate(everyone_is_exhausted(
            registry, scope, &accounts, &ranked, now,
        )));
    }

    let here = ranked
        .iter()
        .find(|ranked| Some(ranked.account.email()) == leaving);
    let landable: Vec<&Ranked> = ranked
        .iter()
        .filter(|ranked| Some(ranked.account.email()) != leaving && !ranked.headroom.is_exhausted())
        .collect();
    let elsewhere: Vec<&Ranked> = landable
        .iter()
        .copied()
        .filter(|ranked| !set_aside.holds(ranked.account.email()))
        .collect();

    // There was somewhere to go and the caller's own policy is the only thing
    // in the way, so its sentence is the answer. Saying "already the best
    // Account in the Group" instead would be a claim about a comparison that
    // was never made, and the reasons below belong to Accounts nobody set
    // aside.
    if elsewhere.is_empty() && !landable.is_empty() {
        return Err(PerchError::NoCandidate(set_aside.because.clone()));
    }

    // Which Accounts moving to would actually gain something, against the one
    // being left.
    //
    // Both orderings, and the second is the one that matters. A Strategy says
    // which of the places worth going is preferred; it does not get to say that
    // nowhere is worth going. Asked on the Strategy's ranking alone, a
    // `soonest-reset` Group whose active Account resets in an hour stayed on it
    // at 95% full while an empty Account sat behind it resetting in four —
    // because tier three sorts on the reset time and nothing else, and the
    // Account you are on was being ranked in it. That is the failure ADR 0013
    // sets candidates aside to prevent, reached through the staying-put check
    // instead of through a post-hoc veto.
    //
    // Applied to the *choice* and not only to the veto, which is the half that
    // was missing. Whether to move and where to move were asked of two
    // different sets: one Account breaking the veto let the Strategy then pick
    // any other, including one the Account being left beat on both counts. A
    // `soonest-reset` Group active on 60% headroom resetting in an hour, beside
    // a 5% Account resetting in two and an unresetting 95% one, moved to the 5%
    // Account — the 95% one broke the veto, and the Strategy's top was
    // somewhere else entirely. One rule asked once: the Accounts worth going to
    // are the ones the veto counts, and the winner is the Strategy's pick from
    // among those.
    let worth_going: Vec<&Ranked> = match measured_against(here.map(|here| &here.headroom)) {
        Some(here) => elsewhere
            .iter()
            .copied()
            .filter(|other| worth_leaving_for(&other.headroom, here, strategy, now))
            .collect(),
        None => elsewhere,
    };

    if let Some(here) = here
        && let Headroom::Room { .. } = here.headroom
        && worth_going.is_empty()
    {
        return Err(PerchError::NothingToDo(already_the_best(
            registry, scope, here, strategy, now,
        )));
    }

    // Everything else is exhausted and the Account Perch is on is not, which
    // leaves nowhere better to go even though it was never ranked against
    // anything.
    let Some(best) = worth_going.first() else {
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

/// The Headroom a move is judged against, when there is one worth judging
/// against at all.
///
/// Staying put is the right answer only when Perch can see that it is: an
/// Account it has never observed is not evidence that moving would gain nothing,
/// and out of the box no Account has been observed at all. So a `here` with no
/// figure rules nothing out.
fn measured_against(here: Option<&Headroom>) -> Option<&Headroom> {
    here.filter(|here| matches!(here, Headroom::Room { .. }))
}

/// Whether moving from `here` to `other` would gain anything.
///
/// Both orderings, and the second is the one that matters. A Strategy says which
/// of the places worth going is preferred; it does not get to say that nowhere is
/// worth going. Asked on the Strategy's ranking alone, a `soonest-reset` Group
/// whose active Account resets in an hour stayed on it at 95% full while an empty
/// Account sat behind it resetting in four — because tier three sorts on the
/// reset time and nothing else, and the Account you are on was being ranked in
/// it. That is the failure ADR 0013 sets candidates aside to prevent, reached
/// through the staying-put check instead of through a post-hoc veto.
///
/// One predicate, because [`choose`] and [`ranked`] both need it and two
/// spellings of it are two orders — the listing that puts one Account at the top
/// and the `perch switch` that lands on another.
fn worth_leaving_for(
    other: &Headroom,
    here: &Headroom,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> bool {
    other.ranking(strategy, now) > here.ranking(strategy, now) || other.by_room() > here.by_room()
}

/// Every Account in a scope, in the order a Cycle ranks them: the ones it could
/// land on first, best first, and the ones it would never choose after them.
///
/// [`choose`] needs only the winner and refuses where there is none. A picker
/// needs the whole order, and needs it over Accounts a Cycle would not touch —
/// a Disabled Account is still shown, and where it sits is what says it is out
/// of the running. So the two share the measurement and the Strategy rather
/// than each sorting on its own idea of which Account is better, because two
/// orders would be a listing that put one Account at the top and a `perch
/// switch` that landed on another.
///
/// A Cycle never leaves the scope it started in (ADR 0002), so there is no
/// ranking over every Account Perch holds: this is per scope, and a listing
/// spanning several is those rankings one after another.
pub fn ranked<'a>(registry: &'a Registry, scope: &Scope, now: DateTime<Utc>) -> Vec<&'a Account> {
    let strategy = scope.strategy(registry);
    let accounts = scope.accounts(registry);
    // The Account a Cycle would be leaving, measured exactly as [`choose`]
    // measures it — the same `leaving` every caller passes it, and only when it
    // is a candidate carrying a figure.
    //
    // Without it, the two orders disagreed. `choose` gained the staying-put veto
    // and this did not, so under `soonest-reset` the top row of the listing was
    // an Account a bare `perch switch` would not land on, and the one it does
    // land on could be the bottom row. The picker exists to make the ranking
    // visible; showing a different one is worse than showing none.
    let here = registry
        .active
        .as_deref()
        .and_then(|active| accounts.iter().find(|account| account.email() == active))
        .filter(|account| is_a_candidate(account))
        .map(|account| headroom_of(account));
    let here = measured_against(here.as_ref());
    // Measured once each rather than inside the comparator, which is the shape
    // [`choose`] already uses. `place` computes two `Headroom`s — each of which
    // clones the fullest window's name — and asks `is_a_candidate` and
    // `worth_leaving_for`, and a comparator runs O(n log n) times: `perch tui`
    // calls this on every frame, four times a second, for as long as the picker
    // is open.
    //
    // Stable, so Accounts that rank identically stay in the order they were
    // added — the same tie-break the choice itself has.
    let mut placed: Vec<(&Account, Place)> = accounts
        .into_iter()
        .map(|account| (account, place(account, here, strategy, now)))
        .collect();
    placed
        .sort_by(|(_, (theirs, them)), (_, (ours, us))| ours.cmp(theirs).then(us.total_cmp(them)));
    placed.into_iter().map(|(account, _)| account).collect()
}

/// Where one Account sorts, higher being better: whether a Cycle could land on
/// it at all, and then how it ranks among the ones it could.
///
/// Candidacy comes first and outranks every figure. An exhausted Account is
/// still one a Cycle would consider tomorrow; a Disabled or Quarantined one is
/// not one it would consider at all, and sorting it by its headroom would put a
/// full-looking Account nobody can use above one they can.
///
/// Then whether moving there would gain anything at all, which outranks the
/// figures for the same reason candidacy does: an Account the Cycle has ruled out
/// is not one to show at the top however good its number looks. It is what makes
/// the highest row a Cycle would land on the highest row full stop — the Account
/// being left included, since moving to where you already are gains nothing.
type Place = ((u8, u8, u8), f64);

fn place(
    account: &Account,
    here: Option<&Headroom>,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> Place {
    let candidate = u8::from(is_a_candidate(account));
    let headroom = headroom_of(account);
    let worth = u8::from(here.is_none_or(|here| worth_leaving_for(&headroom, here, strategy, now)));
    let (tier, figure) = headroom.ranking(strategy, now);
    ((candidate, worth, tier), figure)
}

/// Whether a Cycle could land on this Account at all — which is a different
/// question from whether it has room, and is asked first everywhere.
///
/// One predicate rather than the same pair of conditions written wherever the
/// question comes up: what a Cycle may choose and what a Group has left to draw
/// on ([`crate::reserve`]) must be the same set of Accounts, or the figure on
/// screen describes a set the Switch does not use.
pub fn is_a_candidate(account: &Account) -> bool {
    account.enabled && !account.quarantined()
}

/// Whether anything has declared the Accounts in this scope interchangeable.
///
/// Always true of a Group, which is that declaration (ADR 0002). The Accounts in
/// no Group are a scope only because a global setting says so (ADR 0017), and
/// until it does, every surface has to decline the same things about them —
/// ranking them, and saying what they have left between them. Asked in one place
/// so the listing and the figures above it cannot end up disagreeing about
/// whether they are a set.
pub fn may_cycle_within(registry: &Registry, scope: &Scope) -> bool {
    match scope {
        Scope::Group(_) => true,
        Scope::Ungrouped => registry.global.cycle_ungrouped,
    }
}

/// The Accounts a Cycle may not choose, counted once each.
///
/// One that is both Disabled and Quarantined is still one Account, and a tally
/// that put it in both buckets could add up to more Accounts than the scope
/// holds — a reason that does not survive being checked teaches the reader to
/// stop checking. Empty where every Account is a candidate.
///
/// Shared because the refusal that nobody can be Cycled to and the Reserve that
/// says what is out of the running are the same count of the same Accounts, and
/// two copies of it is how one comes to say "2 disabled" where the other says
/// "1".
pub fn out_of_the_running(accounts: &[&Account]) -> String {
    let quarantined = accounts.iter().filter(|a| a.quarantined()).count();
    let disabled = accounts
        .iter()
        .filter(|a| !a.enabled && !a.quarantined())
        .count();
    let mut out = Vec::new();
    if disabled > 0 {
        out.push(format!("{disabled} disabled"));
    }
    if quarantined > 0 {
        out.push(format!("{quarantined} Quarantined"));
    }
    out.join(", ")
}

/// How much of an Account is left to spend, in a column's worth of words.
///
/// The figure the ranking is made on, said so the order can be checked against
/// it rather than taken on trust. Never observed is said as itself and never as
/// a number: "no figure" and "plenty of room" are opposite pieces of advice.
pub fn headroom_phrase(account: &Account) -> String {
    match headroom_of(account) {
        Headroom::Room { percent, .. } => format!("{}%", utilization::percentage(percent)),
        Headroom::Exhausted { .. } => "exhausted".to_string(),
        Headroom::Unobserved => "never observed".to_string(),
    }
}

/// The three answers there are to "how much has this Account left", for the
/// callers that have to tell them apart rather than print them.
///
/// One value rather than a number and a second question beside it. A caller that
/// asked "has it room?" and then "was it ever read?" would be holding two
/// predicates that have to stay in agreement, and the day they disagreed would
/// be a tally that did not add up to the Accounts on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HowMuchIsLeft {
    /// Every Quota Window has at least this much room left.
    Room(f64),
    /// A Quota Window is full, so the Account is blocked whatever its others
    /// say.
    Exhausted,
    /// No figure has ever been read. Never room: "no figure" and "plenty of
    /// room" are opposite pieces of advice.
    NeverObserved,
}

/// Which of those three an Account is, measured the one honest way (ADR 0012).
pub fn how_much_is_left(account: &Account) -> HowMuchIsLeft {
    match headroom_of(account) {
        Headroom::Room { percent, .. } => HowMuchIsLeft::Room(percent),
        Headroom::Exhausted { .. } => HowMuchIsLeft::Exhausted,
        Headroom::Unobserved => HowMuchIsLeft::NeverObserved,
    }
}

/// How much of an Account is left to spend, with the Quota Window the figure was
/// taken from and the age of the observation it came from (ADR 0015).
///
/// The long form of [`headroom_phrase`], for the surface that gives an Account a
/// block of its own rather than a column: naming the fullest window is what
/// makes "taken from its most constrained window" checkable against the rows
/// underneath rather than a claim in a doc comment.
pub fn headroom_in_full(account: &Account, now: DateTime<Utc>) -> String {
    match headroom_of(account) {
        Headroom::Room {
            percent,
            fullest_window,
            observed_at,
            ..
        } => format!(
            "{}%  ({fullest_window} is its fullest, as of {})",
            utilization::percentage(percent),
            utilization::age_phrase(observed_at, now)
        ),
        // The rows underneath say which window is full and when it comes back,
        // so this says the state and leaves the arithmetic to them.
        Headroom::Exhausted { .. } => match account.observed_utilization() {
            Some(cached) => format!(
                "exhausted  (as of {})",
                utilization::age_phrase(cached.observed_at, now)
            ),
            None => "exhausted".to_string(),
        },
        Headroom::Unobserved => "never observed".to_string(),
    }
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
        Strategy::SoonestReset if best.headroom.ranked_on_reset(strategy, now).is_some() => {
            format!("{named} resets soonest: {figure}.")
        }
        // Nothing that could be moved to has a reset still to come — an Account
        // that did would have outranked this one — so the Cycle fell back to
        // the room it could see rather than switching on nothing.
        //
        // Said as "no reset still to come" rather than as "no cached figure",
        // because the two are not the same absence and the sentence used to
        // claim the wrong one: a window whose reset elapsed is a figure the
        // cache holds and `ranked_on_reset` declines, so "no cached figure says
        // when any of them comes back" contradicted the clause beside it, which
        // had just quoted the time one of them came back at.
        Strategy::SoonestReset => format!(
            "{named} has the most room: {figure}. Nothing that could be moved to \
             has a reset still to come, so there was no reset time to prefer one \
             on — {HOW_TO_GET_FIGURES}"
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

/// The scope holds Accounts, but none of them is a candidate. Which way each
/// one left the running is [`out_of_the_running`]'s to count.
fn nobody_is_a_candidate(scope: &Scope, accounts: &[&Account]) -> String {
    format!(
        "No Account in {} is a Cycle candidate ({}), so there is nowhere to \
         Switch to. Nothing was changed.",
        scope.described(),
        out_of_the_running(accounts),
    )
}

/// Every candidate is blocked. Naming the one that comes back soonest is the
/// whole of the answer: it lets the user decide to wait rather than being moved
/// somewhere useless.
fn everyone_is_exhausted(
    registry: &Registry,
    scope: &Scope,
    accounts: &[&Account],
    ranked: &[Ranked],
    now: DateTime<Utc>,
) -> String {
    // `> now` for the reason `ranked_on_reset` gives for the half of this that
    // has Room: a cached figure outlives the window it describes (ADR 0015), so
    // an elapsed `resets_at` is not a fact about when this Account comes back —
    // it is a window that already came back, under a percentage that is stale.
    //
    // Taken as one, it sorted the wrong way twice over. `min_by_key` picked the
    // *earliest* reset, so among Accounts whose windows had all come back the
    // stalest reading won; and `reset_phrase` renders a past instant as "(any
    // moment now)". A six-hour-old figure would announce that an Account frees
    // up "soonest, at 07:00 (any moment now)" about a window that came back
    // five hours ago, while a fresh figure twenty minutes from resetting went
    // unmentioned.
    let soonest = ranked
        .iter()
        .filter_map(|ranked| match ranked.headroom {
            Headroom::Exhausted {
                frees_at: Some(at), ..
            } if at > now => Some((at, ranked.account)),
            _ => None,
        })
        .min_by_key(|(at, _)| *at);
    // An elapsed reset says as little about the wait as no reset at all, so it
    // is counted here rather than dropped: both mean "the wait could be shorter
    // than that", and for an elapsed one it very likely is.
    let unsaid = ranked
        .iter()
        .filter(|ranked| match ranked.headroom {
            Headroom::Exhausted { frees_at: None } => true,
            Headroom::Exhausted {
                frees_at: Some(at), ..
            } => at <= now,
            _ => false,
        })
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

    // What the filter took out before any of this was measured. Without it the
    // refusal says "every Account in Group `work` is exhausted" about a Group
    // holding two Accounts with full headroom that happen to be disabled, and
    // sends the user off to wait for a quota reset when the fix is `perch
    // enable`. Its sibling refusal counts them, and so does the Reserve; this
    // was the one that dropped them.
    let set_aside = out_of_the_running(accounts);
    let (every, also) = match set_aside.is_empty() {
        true => (String::new(), String::new()),
        false => (
            " Cycling may choose".to_string(),
            format!(" The others are out of the running ({set_aside})."),
        ),
    };

    format!(
        "Every Account in {}{every} is exhausted, so there is nowhere useful \
         to Switch. Nothing was changed.{also}\n{waiting}",
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
    let standing = if here.headroom.ranked_on_reset(strategy, now).is_some() {
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
pub(crate) mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::Quarantine;
    use chrono::TimeZone;

    pub(crate) fn now() -> DateTime<Utc> {
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

    pub(crate) fn account(email: &str, windows: Vec<WindowUtilization>) -> Account {
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

    pub(crate) fn window(name: &str, used_percent: f64) -> WindowUtilization {
        WindowUtilization {
            window: name.to_string(),
            used_percent,
            resets_at: None,
        }
    }

    pub(crate) fn resetting(name: &str, used_percent: f64, hours: i64) -> WindowUtilization {
        WindowUtilization {
            resets_at: Some(now() + chrono::Duration::hours(hours)),
            ..window(name, used_percent)
        }
    }

    pub(crate) fn holding(accounts: Vec<Account>) -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.active = accounts.first().map(|first| first.email().to_string());
        for account in accounts {
            registry.upsert(account);
        }
        registry
    }

    /// The same Group, told to prefer the other of the two Strategies.
    pub(crate) fn preferring(
        mut registry: Registry,
        strategy: crate::registry::Strategy,
    ) -> Registry {
        registry.groups.get_mut("work").expect("declared").strategy = Some(strategy);
        registry
    }

    fn cycle(registry: &Registry) -> Result<Choice> {
        setting_aside(registry, &SetAside::nothing())
    }

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    fn ranked_emails(registry: &Registry) -> Vec<&str> {
        ranked(registry, &work(), now())
            .into_iter()
            .map(Account::email)
            .collect()
    }

    /// The Account you are on is the only one left, and Perch has never read a
    /// figure for it.
    ///
    /// Not a Cycle with nowhere to land, which is what "everything is
    /// exhausted" says, and not one that stayed put because it compared well —
    /// nothing was compared. Perch cannot see that staying is right, so it says
    /// which Account and how to get a figure rather than switching onto an
    /// exhausted one to look busy.
    ///
    /// Worth pinning by example, because this is the branch carrying
    /// `expect("something unexhausted is here or elsewhere")`, and what makes
    /// that safe is spread over three earlier checks: everything-exhausted is
    /// caught above, so something is not exhausted; and if it is not `here` it
    /// is in `elsewhere`, which was just found empty.
    #[test]
    fn the_only_account_left_being_one_nothing_was_read_for_is_said_rather_than_switched_off() {
        let registry = holding(vec![
            account("here@example.com", vec![]),
            account("full@example.com", vec![window("5-hour", 100.0)]),
        ]);

        let refused = cycle(&registry).expect_err("there is nowhere better to go");

        assert_eq!(refused.exit_code(), crate::error::EXIT_NOTHING_TO_DO);
        let said = refused.to_string();
        assert!(said.contains("here@example.com"), "which Account: {said}");
        assert!(
            said.contains("never observed how full it is"),
            "and why staying put is not a comparison Perch made: {said}"
        );
        assert!(
            said.contains("perch status --group --refresh"),
            "and how to get a figure so it can be one: {said}"
        );
    }

    /// The order a picker shows is the order the choice makes, so the Account
    /// at the top is the one a bare `perch switch` would land on.
    #[test]
    fn the_order_is_the_one_the_choice_would_make() {
        let registry = holding(vec![
            account("tired@example.com", vec![window("5-hour", 90.0)]),
            account("fresh@example.com", vec![window("5-hour", 10.0)]),
            account("middling@example.com", vec![window("5-hour", 50.0)]),
        ]);

        assert_eq!(
            ranked_emails(&registry),
            [
                "fresh@example.com",
                "middling@example.com",
                "tired@example.com"
            ]
        );
        assert_eq!(
            cycle(&registry).expect("somewhere to go").account.email(),
            "fresh@example.com",
            "the top of the listing is where the Cycle goes",
        );
    }

    /// The Strategy is the Group's, and the listing obeys it: two surfaces
    /// disagreeing about which Account is better would be a listing that put
    /// one at the top and a Switch that landed on another.
    #[test]
    fn the_order_follows_the_groups_strategy() {
        let registry = holding(vec![
            account("roomy@example.com", vec![resetting("5-hour", 20.0, 4)]),
            account("soon@example.com", vec![resetting("5-hour", 60.0, 1)]),
        ]);

        assert_eq!(
            ranked_emails(&registry)[0],
            "roomy@example.com",
            "most headroom is the default"
        );
        assert_eq!(
            ranked_emails(&preferring(registry, Strategy::SoonestReset))[0],
            "soon@example.com",
        );
    }

    /// An Account a Cycle would never choose sorts below every one it would,
    /// however full its window says it is: sorting it on its headroom would put
    /// an Account nobody can use above one they can.
    #[test]
    fn an_account_no_cycle_would_choose_sorts_below_every_one_it_would() {
        let mut spared = account("spared@example.com", vec![window("5-hour", 0.0)]);
        spared.enabled = false;
        let mut broken = account("broken@example.com", vec![window("5-hour", 0.0)]);
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let registry = holding(vec![
            spared,
            broken,
            account("exhausted@example.com", vec![window("5-hour", 100.0)]),
            account("usable@example.com", vec![window("5-hour", 80.0)]),
        ]);

        assert_eq!(
            ranked_emails(&registry),
            [
                "usable@example.com",
                "exhausted@example.com",
                "spared@example.com",
                "broken@example.com",
            ]
        );
    }

    /// The figure the order was made on, said so the order can be checked
    /// against it rather than taken on trust.
    #[test]
    fn the_headroom_shown_is_the_room_in_the_fullest_window() {
        assert_eq!(
            headroom_phrase(&account(
                "a@example.com",
                vec![window("5-hour", 4.0), window("7-day", 95.0)]
            )),
            "5%"
        );
        assert_eq!(
            headroom_phrase(&account("a@example.com", vec![window("5-hour", 100.0)])),
            "exhausted"
        );
        assert_eq!(
            headroom_phrase(&account("a@example.com", vec![])),
            "never observed",
            "'no figure' and 'plenty of room' are opposite pieces of advice",
        );
    }

    /// The long form names the window the figure came from, so "taken from its
    /// most constrained window" can be checked against the rows underneath it
    /// rather than taken on trust — and every one of the three answers carries
    /// the age of what it was read from, or says it was never read at all.
    #[test]
    fn the_headroom_said_in_full_names_its_window_and_its_age() {
        let roomy = account(
            "a@example.com",
            vec![window("5-hour", 4.0), window("7-day", 95.0)],
        );
        assert_eq!(
            headroom_in_full(&roomy, now()),
            "5%  (7-day is its fullest, as of 4m ago)"
        );

        // The rows underneath say which window is full and when it comes back,
        // so this says the state and leaves the arithmetic to them.
        let full = account("a@example.com", vec![window("5-hour", 100.0)]);
        assert_eq!(headroom_in_full(&full, now()), "exhausted  (as of 4m ago)");

        assert_eq!(
            headroom_in_full(&account("a@example.com", vec![]), now()),
            "never observed",
            "'no figure' and 'plenty of room' are opposite pieces of advice",
        );
    }

    /// The three answers are one value rather than a number and a second
    /// question beside it, so a caller counting them cannot hold two predicates
    /// that disagree.
    #[test]
    fn how_much_is_left_tells_the_three_answers_apart_in_one_pass() {
        assert_eq!(
            how_much_is_left(&account("a@example.com", vec![window("5-hour", 40.0)])),
            HowMuchIsLeft::Room(60.0)
        );
        assert_eq!(
            how_much_is_left(&account("a@example.com", vec![window("5-hour", 100.0)])),
            HowMuchIsLeft::Exhausted
        );
        assert_eq!(
            how_much_is_left(&account("a@example.com", vec![])),
            HowMuchIsLeft::NeverObserved
        );
    }

    fn setting_aside(registry: &Registry, set_aside: &SetAside) -> Result<Choice> {
        choose(
            registry,
            &Scope::Group("work".to_string()),
            registry.active.as_deref(),
            set_aside,
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

    /// A Strategy says which of the places worth going is preferred. It does
    /// not get to say that nowhere is worth going — and asked on the Strategy's
    /// ranking alone it did, because the Account you are on is ranked in tier
    /// three beside the candidates and tier three sorts on the reset time and
    /// nothing else. So the Account that resets soonest was the Account you
    /// stayed on, however full it was and however empty the alternative.
    ///
    /// This is the failure ADR 0013 sets candidates aside to prevent, reached
    /// from the other end: the watcher wants off a 95%-full Account, the margin
    /// sets nothing aside, and the Cycle then reports there is nowhere to go.
    #[test]
    fn soonest_reset_never_pins_you_to_a_full_account_because_it_comes_back_first() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![resetting("5-hour", 95.0, 1)]),
                account("spare@example.com", vec![resetting("5-hour", 5.0, 4)]),
            ]),
            Strategy::SoonestReset,
        );

        let choice = cycle(&registry).expect("an Account with room is right there");

        assert_eq!(
            choice.account.email(),
            "spare@example.com",
            "5% full and four hours off beats 95% full and an hour off, whatever \
             the Group prefers among the places it could go"
        );
    }

    /// The other side of the same rule, which is what keeps the Strategy worth
    /// setting: where the room is the same, the Group that prefers perishable
    /// quota still moves onto the Account that resets first.
    #[test]
    fn soonest_reset_still_moves_off_an_account_that_holds_its_quota_longer() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![resetting("5-hour", 40.0, 6)]),
                account("perishing@example.com", vec![resetting("5-hour", 40.0, 1)]),
            ]),
            Strategy::SoonestReset,
        );

        let choice = cycle(&registry).expect("there is room in both");

        assert_eq!(
            choice.account.email(),
            "perishing@example.com",
            "the same room in both, so the one about to be thrown away is the \
             one to spend"
        );
    }

    /// A cached figure outlives the window it describes (ADR 0015), so a
    /// `resets_at` in the past is ordinary — it means the window has already
    /// come back and the percentage beside it is stale.
    ///
    /// The key is the reset time negated, so an earlier time ranked higher, and
    /// among Accounts whose reset had already passed the *stalest* figure won.
    /// `chosen_because` then announced it as resetting "any moment now" about a
    /// window that came back hours ago.
    /// Two windows equally full is the ordinary case, not the exotic one:
    /// Anthropic answers in whole percentages, and an Account nothing has been
    /// spent on is at nought everywhere. `max_by` hands back the last of several
    /// equal maxima and the windows arrive longest-period last, so the tie went
    /// to the window least likely to say when it resets — and `Headroom::Room`
    /// took its `resets_at` from there. An Account whose `5-hour` quota is
    /// thrown away in an hour then ranked as an Account with no reset at all.
    #[test]
    fn windows_equally_full_are_broken_by_which_one_bites_first() {
        let registry = preferring(
            holding(vec![
                account("active@example.com", vec![resetting("5-hour", 40.0, 3)]),
                // Its `5-hour` and its `7-day-sonnet` are both at 40%, and only
                // the first of them says when it comes back — in one hour. That
                // is the window this Account is constrained by, so it is the one
                // it should be ranked on.
                account(
                    "soonest@example.com",
                    vec![resetting("5-hour", 40.0, 1), window("7-day-sonnet", 40.0)],
                ),
            ]),
            Strategy::SoonestReset,
        );

        let soonest = registry
            .account("soonest@example.com")
            .expect("an Account Perch holds");
        assert_eq!(
            fullest_window_of(soonest).map(|window| window.window.as_str()),
            Some("5-hour"),
            "the window an Account is measured by is the one that runs out first"
        );

        let choice = cycle(&registry).expect("there is room");
        assert_eq!(
            choice.account.email(),
            "soonest@example.com",
            "so it is ranked on that window's reset rather than on no reset at all"
        );
        assert!(
            choice.because.contains("5-hour") && choice.because.contains("in 60m"),
            "and the reset it is announced on is that window's: {}",
            choice.because
        );
    }

    #[test]
    fn a_reset_that_has_already_happened_is_not_a_reason_to_prefer_an_account() {
        let registry = preferring(
            holding(vec![
                // Read six hours ago, and its window came back an hour after
                // that: the 10% left is a figure about a window that no longer
                // exists.
                account("stale@example.com", vec![resetting("5-hour", 90.0, -5)]),
                // Read just now, mostly empty, and comes back in three hours.
                account("fresh@example.com", vec![resetting("5-hour", 10.0, 3)]),
            ]),
            Strategy::SoonestReset,
        );

        let choice = cycle(&registry).expect("there is room");

        assert_eq!(
            choice.account.email(),
            "fresh@example.com",
            "an elapsed reset is not a claim about when an Account comes back"
        );
        assert!(
            choice.because.contains("in 3h"),
            "and the reset it is announced on is one still to come: {}",
            choice.because
        );
    }

    /// The same rule where nothing is left to fall back on but the percentages:
    /// with every reset elapsed, the Strategy has no reset to rank on at all and
    /// ranks on room, rather than crowning whichever figure is oldest.
    #[test]
    fn with_every_reset_elapsed_the_choice_falls_back_to_headroom() {
        let registry = preferring(
            holding(vec![
                // Active, and the one with less room. Listed first because
                // that is what makes it the Account being Cycled off.
                account("fuller@example.com", vec![resetting("5-hour", 80.0, -9)]),
                account("emptier@example.com", vec![resetting("5-hour", 20.0, -1)]),
            ]),
            Strategy::SoonestReset,
        );

        let choice = cycle(&registry).expect("there is room");

        assert_eq!(choice.account.email(), "emptier@example.com");
        assert!(
            !choice.because.contains("resets soonest"),
            "nothing may claim a reset it has not got: {}",
            choice.because
        );
        // The half that was missed when the ranking was fixed: the clause
        // beside the reason read `resets_at` for itself, so it announced a
        // window as coming back "any moment now" an hour after it had, in the
        // same sentence as the explanation that there was no reset to rank on.
        assert!(
            !choice.because.contains("resets at"),
            "and the clause quoting the figure may not either: {}",
            choice.because
        );
        assert!(
            choice.because.contains("has passed"),
            "it says the reading is stale rather than absent: {}",
            choice.because
        );
        // The parenthetical `reset_phrase` puts on a time already gone. Two
        // words from "which has passed", it is the same contradiction one
        // bracket further along, and the assertion above walks straight past it.
        assert!(
            !choice.because.contains("any moment now"),
            "a window that came back is not one coming back: {}",
            choice.because
        );
        // And the sentence that follows the clause may not call the figure
        // absent when the clause it follows has just quoted it.
        assert!(
            !choice.because.contains("No cached figure says when any"),
            "the cache said exactly when it came back: {}",
            choice.because
        );
    }

    /// The same disagreement on the staying-put path, which quotes the same
    /// clause about the Account you are already on.
    #[test]
    fn staying_put_on_an_elapsed_reset_does_not_quote_it_as_still_to_come() {
        let registry = preferring(
            holding(vec![
                // Active, the most room, and a reset that came back an hour ago.
                account("here@example.com", vec![resetting("5-hour", 20.0, -1)]),
                account("other@example.com", vec![window("5-hour", 60.0)]),
            ]),
            Strategy::SoonestReset,
        );

        let said = cycle(&registry)
            .expect_err("there is nowhere better to go")
            .to_string();

        assert!(
            !said.contains("resets at"),
            "an elapsed reset is not a reset still to come: {said}"
        );
        assert!(said.contains("has passed"), "{said}");
        assert!(
            !said.contains("any moment now"),
            "and the bracket after the time may not say it is still to come: {said}"
        );
    }

    /// One Account being worth moving to does not make every Account worth
    /// moving to.
    ///
    /// Whether to move and where to move were asked of two different sets. The
    /// veto compared each candidate against the Account being left on both
    /// orderings; the choice that followed took the Strategy's top of *all* the
    /// candidates. So a single Account breaking the veto — here the roomy one
    /// with no reset time — let the Strategy hand back one the Account being
    /// left beat on both counts.
    #[test]
    fn one_candidate_worth_moving_to_does_not_let_the_strategy_pick_a_worse_one() {
        let registry = preferring(
            holding(vec![
                // Active: 60% headroom, and the soonest reset of the three.
                account("here@example.com", vec![resetting("5-hour", 40.0, 1)]),
                // Ranks below `here` on the Strategy *and* on room, so moving
                // here would be a move onto less of everything.
                account("worse@example.com", vec![resetting("5-hour", 95.0, 2)]),
                // The most room by far, and the only reason not to stay put —
                // but no reset time, so the Strategy ranks it last.
                account("roomiest@example.com", vec![window("5-hour", 5.0)]),
            ]),
            Strategy::SoonestReset,
        );

        assert_eq!(
            cycle(&registry)
                .expect("there is room to move to")
                .account
                .email(),
            "roomiest@example.com",
            "the Accounts worth going to are the ones that broke the veto, and \
             the Strategy picks among those — not among every candidate there is"
        );
    }

    /// And the listing says the same thing, over the fixture that pulls the two
    /// apart.
    ///
    /// `the_order_is_the_one_the_choice_would_make` cannot see this: it uses
    /// `most-headroom` with no reset times, where the Strategy's ranking and the
    /// room ranking are the same ordering and the veto collapses to the identity.
    /// Under `soonest-reset` they differ, and the veto only lived in `choose` —
    /// so the picker showed `worse@` at the top of the Accounts to land on while
    /// a bare `perch switch` landed on `roomiest@`, the bottom row.
    #[test]
    fn the_top_of_the_listing_is_where_the_cycle_goes_under_either_strategy() {
        let accounts = || {
            vec![
                account("here@example.com", vec![resetting("5-hour", 40.0, 1)]),
                account("worse@example.com", vec![resetting("5-hour", 95.0, 2)]),
                account("roomiest@example.com", vec![window("5-hour", 5.0)]),
            ]
        };

        for strategy in [Strategy::SoonestReset, Strategy::MostHeadroom] {
            let registry = preferring(holding(accounts()), strategy);
            let chosen = cycle(&registry).expect("there is room to move to");

            let listed = ranked_emails(&registry);
            let top = listed
                .iter()
                .find(|email| **email != "here@example.com")
                .expect("somebody other than the Account being left is listed");

            assert_eq!(
                *top,
                chosen.account.email(),
                "{strategy:?}: the highest Account the listing offers to land on \
                 has to be the one a bare `perch switch` lands on, or the \
                 ranking the picker exists to make visible is not the one Perch \
                 uses — listed {listed:?}"
            );
        }
    }

    /// The Account being left sits above every Account a Cycle has ruled out, and
    /// below every Account worth moving to. It is not "the best Account" that
    /// puts it there — it is that moving to where you already are gains nothing,
    /// which is the same rule applied to itself.
    #[test]
    fn the_account_being_left_sorts_below_the_ones_worth_moving_to() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![resetting("5-hour", 40.0, 1)]),
                account("worse@example.com", vec![resetting("5-hour", 95.0, 2)]),
                account("roomiest@example.com", vec![window("5-hour", 5.0)]),
            ]),
            Strategy::SoonestReset,
        );

        assert_eq!(
            ranked_emails(&registry),
            [
                "roomiest@example.com",
                "here@example.com",
                "worse@example.com"
            ],
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

    /// A Strategy is entitled to rank whatever the caller left it. Setting an
    /// Account aside has to take it out of the ranking rather than veto the
    /// winner afterwards — under `soonest-reset` the fullest Account can be the
    /// one that wins, and vetoing it would report nowhere to go while a
    /// perfectly empty Account sat behind it.
    #[test]
    fn setting_an_account_aside_leaves_the_ranking_to_choose_from_what_is_left() {
        let registry = preferring(
            holding(vec![
                account("here@example.com", vec![resetting("5-hour", 96.0, 9)]),
                account("soonest@example.com", vec![resetting("5-hour", 74.0, 1)]),
                account("emptiest@example.com", vec![resetting("5-hour", 4.0, 8)]),
            ]),
            Strategy::SoonestReset,
        );

        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "soonest@example.com",
            "the Strategy prefers the quota about to be thrown away"
        );

        let set_aside = SetAside {
            emails: vec!["soonest@example.com".to_string()],
            because: "nothing over 70% is worth moving to.".to_string(),
        };
        assert_eq!(
            setting_aside(&registry, &set_aside)
                .expect("there is still somewhere to go")
                .account
                .email(),
            "emptiest@example.com",
            "and it still prefers among what it was left",
        );
    }

    /// The caller's reason is the answer when the caller's reason is the whole
    /// of it. Anything the Cycle said instead would be a claim about a
    /// comparison it was never allowed to make.
    #[test]
    fn setting_every_landing_place_aside_answers_with_the_reason_it_was_given() {
        let registry = holding(vec![
            account("here@example.com", vec![window("5-hour", 90.0)]),
            account("nearly@example.com", vec![window("5-hour", 74.0)]),
        ]);

        let refusal = setting_aside(
            &registry,
            &SetAside {
                emails: vec!["nearly@example.com".to_string()],
                because: "nearly@example.com is at 74% used and nothing over 70% \
                          is worth moving to."
                    .to_string(),
            },
        )
        .expect_err("everything that could be landed on was set aside");

        assert_eq!(refusal.exit_code(), crate::error::EXIT_NO_CANDIDATE);
        assert!(refusal.to_string().contains("74%"), "{refusal}");
        assert!(
            !refusal.to_string().contains("already the best"),
            "staying put was never compared against anything: {refusal}"
        );
    }

    /// A caller that sets aside an Account the ranking could not have chosen
    /// anyway has told the Cycle nothing, and must not take the Cycle's own
    /// answer away from it.
    #[test]
    fn setting_aside_an_exhausted_account_leaves_the_cycles_own_answer_intact() {
        let registry = holding(vec![
            account("here@example.com", vec![resetting("5-hour", 100.0, 3)]),
            account("full@example.com", vec![resetting("5-hour", 100.0, 1)]),
        ]);

        let refusal = setting_aside(
            &registry,
            &SetAside {
                emails: vec!["full@example.com".to_string()],
                because: "a reason about the margin".to_string(),
            },
        )
        .expect_err("everything is exhausted");

        assert!(
            refusal.to_string().contains("frees up soonest"),
            "when the wait is the answer, saying the wait is the answer: {refusal}"
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

/// Properties the ranking has to hold for every arrangement of Accounts, not
/// only for the ones somebody thought to write a fixture for.
///
/// The example-based tests above say what happens in the situations the design
/// is *about*. These say what must never happen in any situation at all — a
/// comparator sign flipped inside one Strategy would pass every example that
/// does not use that Strategy, and these would catch it wherever it was.
///
/// The cases come from a small congruential generator rather than a property
/// crate: a fixed seed, printed in every failure, so a failing case is one that
/// can be reproduced by running the same test rather than one that has to be
/// caught again (ADR 0025 — a crate where it does not cost a seam, and a
/// dev-dependency for twenty lines of arithmetic is not that).
#[cfg(test)]
mod properties {
    use super::tests::*;
    use super::*;
    use crate::registry::{Quarantine, Strategy};

    /// One case: some Accounts, in some shape, under one Strategy.
    struct Arrangement {
        registry: Registry,
        strategy: Strategy,
        described: String,
    }

    /// A deterministic stream of numbers. Not random — reproducible, which is
    /// the property that matters when a case fails.
    struct Cases(u64);

    impl Cases {
        fn next(&mut self, below: u64) -> u64 {
            // Numerical Recipes' constants: any full-period generator will do
            // here, and this one needs no dependency and no explanation.
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (self.0 >> 33) % below
        }

        fn arrangement(&mut self, index: usize) -> Arrangement {
            let count = 1 + self.next(5) as usize;
            let strategy = if self.next(2) == 0 {
                Strategy::MostHeadroom
            } else {
                Strategy::SoonestReset
            };

            let mut described = format!("case {index}, {strategy:?}");
            let mut accounts = Vec::new();
            for at in 0..count {
                let email = format!("a{at}@example.com");
                // Every shape the ranking distinguishes: no observation, an
                // observation with a reset time, one without, and a window
                // that is full.
                let used = self.next(101) as f64;
                let windows = match self.next(4) {
                    0 => vec![],
                    1 => vec![window("5-hour", used)],
                    2 => vec![resetting("5-hour", used, 1 + self.next(48) as i64)],
                    _ => vec![window("5-hour", 100.0)],
                };
                let mut held = account(&email, windows);
                held.enabled = self.next(6) > 0;
                held.quarantine = (self.next(8) == 0).then_some(Quarantine::RenewalRejected);
                described.push_str(&format!(
                    "\n  {email}: {:?} enabled={} quarantined={}",
                    headroom_of(&held),
                    held.enabled,
                    held.quarantined(),
                ));
                accounts.push(held);
            }

            Arrangement {
                registry: preferring(holding(accounts), strategy),
                strategy,
                described,
            }
        }
    }

    fn cases() -> impl Iterator<Item = Arrangement> {
        let mut generator = Cases(0x5eed_1234_5678_9abc);
        (0..400).map(move |index| generator.arrangement(index))
    }

    fn chosen(arrangement: &Arrangement, leaving: Option<&str>) -> Option<Account> {
        choose(
            &arrangement.registry,
            &Scope::Group("work".to_string()),
            leaving,
            &SetAside::nothing(),
            now(),
        )
        .ok()
        .map(|choice| choice.account)
    }

    /// Never being chosen for you is the whole of what disabled means, and a
    /// Quarantined Account's Credential does not work. Neither can be the
    /// answer however the figures fall.
    #[test]
    fn nothing_disabled_exhausted_or_quarantined_is_ever_chosen() {
        for arrangement in cases() {
            let Some(won) = chosen(&arrangement, None) else {
                continue;
            };
            assert!(won.enabled, "{}", arrangement.described);
            assert!(!won.quarantined(), "{}", arrangement.described);
            assert!(
                !headroom_of(&won).is_exhausted(),
                "{}",
                arrangement.described
            );
        }
    }

    /// The winner is a winner: no candidate the Cycle was allowed to choose
    /// ranks above it.
    #[test]
    fn the_winner_ranks_at_least_as_high_as_every_candidate() {
        for arrangement in cases() {
            let Some(won) = chosen(&arrangement, None) else {
                continue;
            };
            let winning = headroom_of(&won).ranking(arrangement.strategy, now());
            for held in &arrangement.registry.accounts {
                if !held.enabled || held.quarantined() || headroom_of(held).is_exhausted() {
                    continue;
                }
                let theirs = headroom_of(held).ranking(arrangement.strategy, now());
                assert!(
                    winning.0 > theirs.0 || (winning.0 == theirs.0 && winning.1 >= theirs.1),
                    "{} won at {winning:?}, beaten by {theirs:?}\n{}",
                    won.email(),
                    arrangement.described,
                );
            }
        }
    }

    /// Landing where you already are would rewrite Credentials for nothing.
    #[test]
    fn the_account_being_left_is_never_the_one_chosen() {
        for arrangement in cases() {
            let leaving = arrangement
                .registry
                .accounts
                .first()
                .map(|first| first.email().to_string());
            let Some(won) = chosen(&arrangement, leaving.as_deref()) else {
                continue;
            };
            assert_ne!(
                Some(won.email()),
                leaving.as_deref(),
                "{}",
                arrangement.described
            );
        }
    }

    /// Moving is supposed to gain something. An Account the one being left
    /// beats on the Strategy's ranking *and* on the room it can see is a move
    /// that made things worse, whatever the Strategy prefers.
    ///
    /// Asked with `leaving` set, which is what the two properties above do not
    /// do: both ask with no Account being left, so the whole comparison against
    /// `here` — the veto and the choice it gates — went unexercised. This is the
    /// arrangement that found it: a `soonest-reset` Group active on 60% headroom
    /// resetting in an hour, beside a 5% Account resetting in two and an
    /// unresetting 95% one, moved onto the 5%.
    #[test]
    fn the_account_chosen_is_never_one_the_account_being_left_beats_outright() {
        for arrangement in cases() {
            let Some(leaving) = arrangement.registry.accounts.first() else {
                continue;
            };
            // Only where the Account being left is one the Cycle would have
            // considered. Leaving a Disabled or Quarantined Account is the case
            // for moving somewhere worse rather than against it: the figure
            // beside a broken Credential is not a standard anything has to beat.
            if !is_a_candidate(leaving) {
                continue;
            }
            let here = headroom_of(leaving);
            let Headroom::Room { .. } = here else {
                continue;
            };
            let leaving = leaving.email().to_string();
            let Some(won) = chosen(&arrangement, Some(&leaving)) else {
                continue;
            };

            let theirs = headroom_of(&won);
            assert!(
                theirs.ranking(arrangement.strategy, now())
                    > here.ranking(arrangement.strategy, now())
                    || theirs.by_room() > here.by_room(),
                "left {leaving} at {:?}/{:?} for {} at {:?}/{:?}, which is worse on both\n{}",
                here.ranking(arrangement.strategy, now()),
                here.by_room(),
                won.email(),
                theirs.ranking(arrangement.strategy, now()),
                theirs.by_room(),
                arrangement.described,
            );
        }
    }

    /// The same question asked twice gets the same answer, or nothing anybody
    /// reads is stable enough to act on.
    #[test]
    fn the_same_arrangement_always_chooses_the_same_account() {
        for arrangement in cases() {
            let once = chosen(&arrangement, None).map(|won| won.email().to_string());
            let twice = chosen(&arrangement, None).map(|won| won.email().to_string());
            assert_eq!(once, twice, "{}", arrangement.described);
        }
    }
}
