//! The Cycle: which Account a Switch lands on when nobody named one.
//!
//! An Account's Headroom is its worst Quota Window and the Account whose worst
//! is best wins; a Strategy reorders what the measurement leaves standing rather
//! than getting round it (ADR headroom-is-the-worst-window). A Cycle never
//! leaves the Scope it started in — a Group, or the ungrouped Accounts once
//! somebody declares them interchangeable (ADR a-group-is-a-declaration).
//!
//! Nothing here reaches the network or the filesystem: ranking is on cached
//! figures and their ages (ADR a-figure-carries-its-age).

use chrono::{DateTime, Utc};

use crate::config::{Scope, Strategy};
use crate::error::{PerchError, Result};
use crate::name;
use crate::registry::{self, Account, CachedUtilization, Registry, WindowUtilization};
use crate::utilization;

/// Which Account a Scope prefers when more than one would serve.
///
/// Read from the Settings the Scope itself holds — there is nothing above it to
/// fall back to (ADR a-setting-names-its-scope). The ungrouped Accounts are not
/// a Group but are a Scope, so what they Cycle by is something a person says.
fn strategy(registry: &Registry, scope: &Scope) -> Strategy {
    registry.settings(scope).strategy
}

/// The weekly window a Fable request is metered under, as the reply's own scope
/// names it. What `prefer-fable` keys on, so a reply that stops naming it is a
/// preference matching nothing — which is said rather than silently ranked past
/// (ADR fable-is-spent-first).
pub const THE_FABLE_WINDOW: &str = "7-day-fable";

/// How a Scope measures its Accounts: Headroom alone, or Fable First — the
/// Accounts that can serve Fable now ahead of every one that cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// The worst Quota Window, whatever it meters.
    Worst,
    /// Two tiers. First the Accounts none of whose Fable-metering windows is
    /// full, ordered by the Fable weekly; then the rest, ordered by their
    /// fullest window that is not Fable's. A Strategy orders within a tier and
    /// never across one.
    FableFirst,
}

/// The Measure a Scope ranks by. `prefer-fable` matching no observed window
/// falls back to Headroom alone — the ranking [`fable_unmatched`] says it fell
/// back to — because tiers keyed on a window nobody reports order nothing.
pub fn measure_of(registry: &Registry, scope: &Scope) -> Measure {
    match registry.settings(scope).prefer_fable && !unmatched(registry, scope) {
        true => Measure::FableFirst,
        false => Measure::Worst,
    }
}

/// Whether `prefer-fable` would key on a window no observed Account reports.
/// Nothing observed at all is not a mismatch: there is no reply to disagree with.
fn unmatched(registry: &Registry, scope: &Scope) -> bool {
    let observed: Vec<_> = scope
        .accounts(registry)
        .iter()
        .filter_map(|account| account.observed_utilization())
        .collect();
    !observed.is_empty()
        && !observed.iter().any(|cached| {
            cached
                .windows
                .iter()
                .any(|window| window.window == THE_FABLE_WINDOW)
        })
}

/// The sentence a Scope preferring Fable is owed when the preference matches
/// nothing, or `None` where it matches. Loud rather than quiet, because a
/// Setting that silently stops meaning anything is the failure no refusal is
/// left to catch.
pub fn fable_unmatched(registry: &Registry, scope: &Scope) -> Option<String> {
    (registry.settings(scope).prefer_fable && unmatched(registry, scope)).then(|| {
        format!(
            "`prefer-fable` is on for {}, and no observed Account reports a \
             `{THE_FABLE_WINDOW}` window, so Accounts are ranked on Headroom \
             alone. Anthropic may have renamed the window; a newer Perch would \
             know the new name.",
            scope.mentioned(),
        )
    })
}

/// Where a bare `perch switch` may look, given the Account it would be leaving.
///
/// An ungrouped Account is the ordinary starting state rather than an edge case
/// — adoption leaves the first Account in no Group — so this is a refusal the
/// user is expected to meet, and it names both ways out of it.
pub fn scope_for(registry: &Registry, leaving: &Account) -> Result<Scope> {
    match &leaving.group {
        Some(group) => Ok(Scope::Group(group.clone())),
        None if registry.ungrouped.interchangeable => Ok(Scope::Ungrouped),
        None => Err(PerchError::NotInterchangeable(format!(
            "{} is in no Group, so nothing has declared which Accounts it is \
             interchangeable with.\n\
             Either put it in a Group with `perch group move {} <group>`, or \
             declare that every ungrouped Account is interchangeable with \
             `perch config set ungrouped interchangeable true`.",
            registry.named_for_the_user(leaving.email()),
            leaving.email(),
        ))),
    }
}

/// How full an Account is: by the Quota Window that is fullest.
///
/// One value rather than a number and a second question beside it: two
/// predicates that have to stay in agreement are a tally that stops adding up.
#[derive(Debug, Clone, PartialEq)]
pub enum Headroom<'a> {
    /// Every Quota Window has at least this much room left.
    Room {
        percent: f64,
        /// Borrowed from the Account it was measured on: most callers read only
        /// the percentage, and a listing computes one of these per row.
        fullest_window: &'a str,
        /// When the fullest window comes back, if the observation carried it.
        resets_at: Option<DateTime<Utc>>,
        observed_at: DateTime<Utc>,
    },
    /// A Quota Window is full, so the Account is blocked whatever its others
    /// say. It frees up when the last of its full windows resets.
    Exhausted { frees_at: Option<DateTime<Utc>> },
    /// No figure has ever been observed.
    Unobserved,
}

impl Headroom<'_> {
    /// What ranking sorts on, higher being better.
    ///
    /// Four tiers, because `soonest-reset` adds one on top of the three
    /// `by_room` holds and falls back to them where nothing has a reset. A reset
    /// time and a percentage meet only as tiers, never compared.
    fn ranking(&self, strategy: Strategy, now: DateTime<Utc>) -> (u8, f64) {
        match self.ranked_on_reset(strategy, now) {
            // Sooner is better, so the figure sorted on is the reset time
            // negated.
            Some(at) => (3, -(at.timestamp() as f64)),
            None => self.by_room(),
        }
    }

    /// The same ordering with the Strategy left out: how much is left, and
    /// nothing about when it comes back.
    ///
    /// Named on its own because one question wants it — whether moving would
    /// gain any room, which is not the question of which place is preferred.
    fn by_room(&self) -> (u8, f64) {
        match self {
            Headroom::Room { percent, .. } => (2, *percent),
            Headroom::Unobserved => (1, 0.0),
            Headroom::Exhausted { .. } => (0, 0.0),
        }
    }

    /// Whether this Account's fullest window has a reset that has not happened
    /// yet. One answer, because everything that has to agree about it asks here:
    /// the key that sorts the Accounts, the sentence saying why one won, and the
    /// sentence saying why staying put is already the best there is.
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
}

/// What reads a figure Perch has not got, said the same way wherever the absence
/// of one is the reason for the answer. Named against the Scope the Cycle looked
/// in, because a refresh reads the Accounts it is about to show and no others.
fn how_to_get_figures(scope: &Scope) -> String {
    format!(
        "`perch list {} --refresh` reads current figures.",
        scope.word()
    )
}

/// The Headroom a Cycle may rank its rivals against: this Account's, where the
/// cache carries a figure young enough to be one. `None` is a figure too old to
/// stand for the Account's state, or no figure at all, and rules nothing out.
pub fn trusted(account: &Account, now: DateTime<Utc>) -> Option<f64> {
    let cached = account.observed_utilization()?;
    if !crate::watch::figure_stands(cached, now) {
        return None;
    }
    match headroom_of(account) {
        Headroom::Room { percent, .. } => Some(percent),
        // Nothing left, which every rival carrying room beats — a trusted floor
        // rather than an absent one.
        Headroom::Exhausted { .. } => Some(0.0),
        Headroom::Unobserved => None,
    }
}

/// The most Headroom a cached figure could still be describing: a window whose
/// reset is still ahead caps an Account at what it was last seen with, and one
/// past its reset or silent about it caps nothing
/// (ADR a-choice-reads-what-it-ranks).
fn best_case(account: &Account, now: DateTime<Utc>) -> f64 {
    let Some(cached) = account.observed_utilization() else {
        return 100.0;
    };
    cached
        .windows
        .iter()
        .map(|window| match window.resets_at {
            Some(at) if at > now => 100.0 - window.used_percent,
            _ => 100.0,
        })
        .fold(100.0, f64::min)
}

/// Which of a Scope's Accounts a Cycle cannot rank without reading them first:
/// every candidate whose figure is too old to stand on and whose [`best_case`]
/// still beats `to_beat`. `None` rules nothing out, so nobody worth vouching
/// for means everybody worth reading.
pub fn worth_reading(
    registry: &Registry,
    scope: &Scope,
    leaving: Option<&str>,
    to_beat: Option<f64>,
    now: DateTime<Utc>,
) -> Vec<String> {
    // Under `soonest-reset` the order is a reset time rather than room, and
    // under Fable First it is the Fable weekly — either way a bound on room
    // excludes nothing and every stale candidate is read.
    let on_room = !matches!(strategy(registry, scope), Strategy::SoonestReset)
        && matches!(measure_of(registry, scope), Measure::Worst);
    let sharers = registry::Sharers::across(registry);
    scope
        .accounts(registry)
        .into_iter()
        .filter(|account| is_a_candidate(&sharers, account))
        .filter(|account| !leaving.is_some_and(|email| name::same_name(account.email(), email)))
        // A rival inside the same window that makes the Account being left
        // worth trusting is worth trusting too, and reading it buys nothing.
        .filter(|account| trusted(account, now).is_none())
        .filter(|account| !on_room || to_beat.is_none_or(|floor| best_case(account, now) > floor))
        .map(|account| account.email().to_string())
        .collect()
}

/// The Quota Window that decides how full an Account is: its fullest, or `None`
/// for one nothing has ever been observed of.
///
/// Public because the Watcher compares the Account it is on against a Threshold,
/// and two measures of fullness would act on one number and choose on another.
pub fn fullest_window_of(account: &Account) -> Option<&WindowUtilization> {
    account
        .observed_utilization()
        .and_then(|cached| fullest_of(&cached.windows, |_| true))
}

/// The Quota Window a Measure judges a *candidate's* fullness by — under Fable
/// First, the windows its tier ranks on, so a fall-through candidate is not
/// set aside for a Fable weekly its tier never reads.
pub fn measured_fullest_of(account: &Account, measure: Measure) -> Option<&WindowUtilization> {
    match measure {
        Measure::Worst => fullest_window_of(account),
        Measure::FableFirst => {
            let cached = account.observed_utilization()?;
            match serves_fable(cached) {
                true => fullest_of(&cached.windows, meters_fable),
                false => fullest_of(&cached.windows, |window| window.window != THE_FABLE_WINDOW),
            }
        }
    }
}

/// Which of [`Headroom`]'s three answers an Account is.
pub fn headroom_of(account: &Account) -> Headroom<'_> {
    let Some(cached) = account.observed_utilization() else {
        return Headroom::Unobserved;
    };
    headroom_over(cached, |_| true)
}

/// The measurement over the windows `counts` keeps: the worst of them decides.
fn headroom_over(
    cached: &CachedUtilization,
    counts: impl Fn(&WindowUtilization) -> bool + Copy,
) -> Headroom<'_> {
    // An observation carries at least one window, so only a narrowed subset can
    // be empty — and one is no figure rather than a full or empty one.
    let Some(fullest) = fullest_of(&cached.windows, counts) else {
        return Headroom::Unobserved;
    };
    if fullest.used_percent >= 100.0 {
        return Headroom::Exhausted {
            frees_at: frees_at(&cached.windows, counts),
        };
    }
    Headroom::Room {
        percent: 100.0 - fullest.used_percent,
        fullest_window: &fullest.window,
        resets_at: fullest.resets_at,
        observed_at: cached.observed_at,
    }
}

/// The fullest of the windows `counts` keeps, and on a tie the most perishable
/// of the equally full ones.
fn fullest_of(
    windows: &[WindowUtilization],
    counts: impl Fn(&WindowUtilization) -> bool,
) -> Option<&WindowUtilization> {
    /// Most perishable first. `is_none` leads so that a window saying nothing
    /// about its reset sorts behind every window that does, rather than ahead
    /// of them the way a bare `Option` orders.
    fn perishability(window: &WindowUtilization) -> (bool, Option<DateTime<Utc>>) {
        (window.resets_at.is_none(), window.resets_at)
    }

    windows
        .iter()
        .filter(|window| counts(window))
        .min_by(|left, right| {
            right
                .used_percent
                .total_cmp(&left.used_percent)
                .then_with(|| perishability(left).cmp(&perishability(right)))
        })
}

/// When an exhausted Account can be used again: the last of its full windows to
/// reset, or `None` where any of them does not say.
fn frees_at(
    windows: &[WindowUtilization],
    counts: impl Fn(&WindowUtilization) -> bool,
) -> Option<DateTime<Utc>> {
    let mut last = None;
    for window in windows
        .iter()
        .filter(|window| counts(window) && window.used_percent >= 100.0)
    {
        let resets_at = window.resets_at?;
        last = Some(last.map_or(resets_at, |last: DateTime<Utc>| last.max(resets_at)));
    }
    last
}

/// Whether this window meters a Fable request. Another model's weekly is the
/// one kind that does not; the five-hour, the seven-day and Fable's own weekly
/// all fill under one.
fn meters_fable(window: &WindowUtilization) -> bool {
    !window.window.starts_with("7-day-") || window.window == THE_FABLE_WINDOW
}

/// Whether the Account can serve a Fable request now: no window that meters one
/// is full.
fn serves_fable(cached: &CachedUtilization) -> bool {
    cached
        .windows
        .iter()
        .filter(|window| meters_fable(window))
        .all(|window| window.used_percent < 100.0)
}

/// An Account as a Scope's Measure reads it: the tier it stands in, and its
/// Headroom within that tier. Under [`Measure::Worst`] every Account is one
/// tier, so the pair *is* today's measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured<'a> {
    /// Higher first, before anything the Headroom says.
    tier: u8,
    headroom: Headroom<'a>,
}

/// Which tier the Measure puts an Account in, and what figure ranks it there.
pub fn measured_of(account: &Account, measure: Measure) -> Measured<'_> {
    match measure {
        Measure::Worst => Measured {
            tier: 0,
            headroom: headroom_of(account),
        },
        Measure::FableFirst => fable_first_of(account),
    }
}

/// "No figure" and "no Fable window" rank alike: in the first tier, below every
/// Account with known Fable room and above every full one — either read as good
/// or bad news would be a fact Perch invented.
fn fable_first_of(account: &Account) -> Measured<'_> {
    let unobserved = Measured {
        tier: 1,
        headroom: Headroom::Unobserved,
    };
    let Some(cached) = account.observed_utilization() else {
        return unobserved;
    };
    let Some(fable) = cached
        .windows
        .iter()
        .find(|window| window.window == THE_FABLE_WINDOW)
    else {
        return unobserved;
    };
    match serves_fable(cached) {
        // The Fable weekly orders the tier, not the worst of the three: while
        // Fable's is the only per-model window those coincide, and a Setting
        // whose only effect is its endgame reads as broken.
        true => Measured {
            tier: 1,
            headroom: Headroom::Room {
                percent: 100.0 - fable.used_percent,
                fullest_window: &fable.window,
                resets_at: fable.resets_at,
                observed_at: cached.observed_at,
            },
        },
        false => Measured {
            tier: 0,
            headroom: headroom_over(cached, |window| window.window != THE_FABLE_WINDOW),
        },
    }
}

impl Measured<'_> {
    /// [`Headroom::ranking`] under the tier, flattened for one comparison.
    fn ranking(&self, strategy: Strategy, now: DateTime<Utc>) -> (u8, u8, f64) {
        let (rank, figure) = self.headroom.ranking(strategy, now);
        (self.tier, rank, figure)
    }

    /// [`Headroom::by_room`] under the tier.
    fn by_room(&self) -> (u8, u8, f64) {
        let (rank, figure) = self.headroom.by_room();
        (self.tier, rank, figure)
    }

    fn is_exhausted(&self) -> bool {
        self.headroom.is_exhausted()
    }
}

/// An Account with what ranking made of it.
struct Ranked<'a> {
    account: &'a Account,
    measured: Measured<'a>,
}

/// Accounts this Cycle may not land on, whatever the ranking makes of them, and
/// the one sentence that says why. The Cycle holds no opinion about them: the
/// Watcher's Margin is policy about *when* a move is worth making
/// (ADR a-watcher-knob-is-arithmetic), and answering that here would put a
/// Watcher's clock inside every `perch switch`.
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

    /// `same_name`, and so is every other address comparison in this module:
    /// `upsert` matches an Account that way and stores the incoming spelling, so
    /// an Identity re-read under another capitalization otherwise leaves a
    /// set-aside Account quietly no longer set aside.
    fn holds(&self, email: &str) -> bool {
        self.emails.iter().any(|held| name::same_name(held, email))
    }
}

/// What the ranking rested on, which is the whole of what a Switch says about
/// having chosen for you (ADR perch-says-what-it-did) — the basis and not the
/// argument for it. A value rather than a sentence, because the landing line and
/// the Watcher's round put it in different places, and one decision must not
/// come to have two spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The worst Quota Window, and the Account whose worst is best. What a Cycle
    /// ranks on unless the Scope says otherwise, and what `soonest-reset` falls
    /// back to where nothing it could move to has a reset still to come.
    MostRoom,
    /// Of the Accounts with room, the one whose fullest window comes back
    /// soonest, so perishable quota is spent rather than wasted.
    SoonestReset,
    /// Of the Accounts that can serve Fable, the one with the most of its Fable
    /// weekly left — what a Scope preferring Fable ranks its first tier on.
    MostFable,
    /// Nothing has ever been observed of this Account, so it was compared with
    /// nothing.
    Unranked,
}

impl Basis {
    /// The clause a landing line carries after the Account it landed on:
    /// "Switched to overflow@example.com, {}." It names the Scope, because
    /// staying inside the Group is the claim worth making beside where a Cycle
    /// landed — and a Scope announced beforehand is announced to somebody who
    /// does not yet know where they are going.
    pub fn in_the(&self, scope: &Scope) -> String {
        let basis = match self {
            Basis::MostRoom => "the most room",
            Basis::SoonestReset => "the soonest reset",
            Basis::MostFable => "the most Fable left",
            Basis::Unranked => "nothing observed to rank on",
        };
        format!("{basis} in {}", scope.place())
    }
}

/// The Account a Cycle picked, with what it will say about having picked it.
#[derive(Debug)]
pub struct Choice {
    /// Owned, so the caller can go on to write the Registry it came from.
    pub account: Account,
    /// What this Account won on, for the landing line that names where it
    /// landed.
    pub basis: Basis,
}

/// Picks the Account to Switch to, or explains why none is worth switching to.
///
/// `leaving` is ranked like any other Account and never chosen: landing where
/// you already are rewrites Credentials for nothing. `set_aside` is the caller's
/// own reasons for not landing somewhere, obeyed without opinion ([`SetAside`]).
pub fn choose(
    registry: &Registry,
    scope: &Scope,
    leaving: Option<&str>,
    set_aside: &SetAside,
    now: DateTime<Utc>,
) -> Result<Choice> {
    let strategy = strategy(registry, scope);
    let measure = measure_of(registry, scope);
    let accounts = scope.accounts(registry);
    if accounts.is_empty() {
        return Err(PerchError::NoCandidate(format!(
            "{} holds no Accounts, so there is nowhere to Cycle to. Nothing was \
             changed.",
            scope.place(),
        )));
    }

    let sharers = registry::Sharers::across(registry);
    let mut ranked: Vec<Ranked> = accounts
        .iter()
        .filter(|account| is_a_candidate(&sharers, account))
        .map(|account| Ranked {
            account,
            measured: measured_of(account, measure),
        })
        .collect();
    if ranked.is_empty() {
        return Err(PerchError::NoCandidate(nobody_is_a_candidate(
            &sharers, scope, &accounts,
        )));
    }

    // Stable, so Accounts that rank identically stay in the order they were
    // added and the same command twice makes the same choice.
    ranked.sort_by(|left, right| {
        let (their_tier, theirs, them) = right.measured.ranking(strategy, now);
        let (our_tier, ours, us) = left.measured.ranking(strategy, now);
        (their_tier, theirs)
            .cmp(&(our_tier, ours))
            .then(them.total_cmp(&us))
    });

    if ranked.iter().all(|ranked| ranked.measured.is_exhausted()) {
        return Err(PerchError::NoCandidate(everyone_is_exhausted(
            registry, scope, &accounts, &ranked, now,
        )));
    }

    let is_leaving = |ranked: &Ranked| {
        leaving.is_some_and(|email| name::same_name(ranked.account.email(), email))
    };
    let here = ranked.iter().find(|ranked| is_leaving(ranked));
    let landable: Vec<&Ranked> = ranked
        .iter()
        .filter(|ranked| !is_leaving(ranked) && !ranked.measured.is_exhausted())
        .collect();
    let elsewhere: Vec<&Ranked> = landable
        .iter()
        .copied()
        .filter(|ranked| !set_aside.holds(ranked.account.email()))
        .collect();

    // The caller's policy is the only thing in the way, so its sentence is the
    // answer: "already the best Account in the Group" would be a claim about a
    // comparison nobody made.
    if elsewhere.is_empty() && !landable.is_empty() {
        return Err(PerchError::NoCandidate(set_aside.because.clone()));
    }

    // Which Accounts moving to would gain something, against the one being left.
    // The winner is the Strategy's pick from among those, rather than from among
    // every candidate.
    let worth_going: Vec<&Ranked> = match measured_against(here.map(|here| &here.measured)) {
        Some(here) => elsewhere
            .iter()
            .copied()
            .filter(|other| worth_leaving_for(&other.measured, here, strategy, now))
            .collect(),
        None => elsewhere,
    };

    if let Some(here) = here
        && let Headroom::Room { .. } = here.measured.headroom
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
            "{} is the only Account in {} that is not exhausted, and nothing \
             has been observed of it. {}",
            registry.named_for_the_user(alone.account.email()),
            scope.place(),
            how_to_get_figures(scope),
        )));
    };

    Ok(Choice {
        account: best.account.clone(),
        basis: chosen_basis(best, strategy, now),
    })
}

/// The Headroom a move is judged against, when there is one worth judging
/// against at all.
///
/// Staying put is right only where Perch can see that it is: an Account it has
/// never observed rules nothing out, and out of the box none has been observed.
fn measured_against<'h, 'a>(here: Option<&'h Measured<'a>>) -> Option<&'h Measured<'a>> {
    here.filter(|here| matches!(here.headroom, Headroom::Room { .. }))
}

/// Whether moving from `here` to `other` would gain anything, on the Strategy's
/// ordering or on the room alone.
///
/// One predicate, because [`choose`] and [`ranked`] both need it, and two
/// spellings of it are two orders over one Scope.
fn worth_leaving_for(
    other: &Measured,
    here: &Measured,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> bool {
    other.ranking(strategy, now) > here.ranking(strategy, now) || other.by_room() > here.by_room()
}

/// Every Account in a Scope, in the order a Cycle ranks them: the ones it could
/// land on first, best first, and the ones it would never choose after them.
/// [`choose`] needs only the winner and a listing needs the whole order, so the
/// two share this measurement rather than each sorting on its own idea of which
/// Account is better (ADR the-listing-owns-the-set).
pub fn ranked<'a>(registry: &'a Registry, scope: &Scope, now: DateTime<Utc>) -> Vec<&'a Account> {
    let strategy = strategy(registry, scope);
    let measure = measure_of(registry, scope);
    let accounts = scope.accounts(registry);
    // The Account a Cycle would be leaving, measured exactly as `choose`
    // measures it, and only where it is a candidate carrying a figure.
    let sharers = registry::Sharers::across(registry);
    let leaving = registry.active().whose();
    let here = leaving
        .and_then(|active| {
            accounts
                .iter()
                .find(|account| name::same_name(account.email(), active))
        })
        .filter(|account| is_a_candidate(&sharers, account))
        .map(|account| measured_of(account, measure));
    let here = measured_against(here.as_ref());
    // Measured once each rather than inside a comparator that runs O(n log n)
    // times: `place` walks every Quota Window an Account carries. Stable, so
    // Accounts that rank identically keep the order they were added.
    let mut placed: Vec<(&Account, Place)> = accounts
        .into_iter()
        .map(|account| {
            (
                account,
                place(&sharers, account, leaving, here, strategy, measure, now),
            )
        })
        .collect();
    placed
        .sort_by(|(_, (theirs, them)), (_, (ours, us))| ours.cmp(theirs).then(us.total_cmp(them)));
    placed.into_iter().map(|(account, _)| account).collect()
}

/// Where one Account sorts, higher being better: whether a Cycle could land on
/// it at all, then whether moving there gains anything, then how it ranks. An
/// Account a Cycle has ruled out is not one to show at the top however good its
/// number looks. `leaving` is named rather than inferred from `here`, which is
/// `None` for an unobserved Account and would drop the rule for all of them.
type Place = ((u8, u8, u8, u8), f64);

fn place(
    sharers: &registry::Sharers,
    account: &Account,
    leaving: Option<&str>,
    here: Option<&Measured>,
    strategy: Strategy,
    measure: Measure,
    now: DateTime<Utc>,
) -> Place {
    let candidate = is_a_candidate(sharers, account);
    let measured = measured_of(account, measure);
    // Asked only of a candidate, which is the only set `choose` asks it of: it
    // drops the non-candidates before looking for the one being left.
    let staying = candidate && leaving.is_some_and(|email| name::same_name(account.email(), email));
    // Exhausted asked outright rather than left to `worth_leaving_for`, whose
    // `here` is `None` both for an unobserved active Account and for an
    // exhausted one — so `is_none_or` would pass every exhausted candidate.
    let worth = u8::from(
        !staying
            && !measured.is_exhausted()
            && here.is_none_or(|here| worth_leaving_for(&measured, here, strategy, now)),
    );
    let (tier, rank, figure) = measured.ranking(strategy, now);
    ((u8::from(candidate), worth, tier, rank), figure)
}

/// Whether a Cycle could land on this Account at all — never a Disabled or
/// Quarantined one, nor a sharer, whom every Switch refuses.
///
/// One predicate, because what a Cycle may choose, what a Scope has left to draw
/// on ([`crate::reserve`]) and what a Remove lands on are one set of Accounts.
pub fn is_a_candidate(sharers: &registry::Sharers, account: &Account) -> bool {
    !account.disabled && !account.quarantined() && !sharers.hold(account.email())
}

/// Whether anything has declared the Accounts in this Scope interchangeable.
/// Always true of a Group, which is that declaration. Asked in one place, so the
/// listing and the figures above it cannot end up disagreeing about whether the
/// ungrouped Accounts are a set.
pub fn may_cycle_within(registry: &Registry, scope: &Scope) -> bool {
    match scope {
        Scope::Group(_) => true,
        Scope::Ungrouped => registry.ungrouped.interchangeable,
    }
}

/// Whether the Watcher may act unasked within a Scope, and what is missing where
/// it may not. Two statements and never one: being interchangeable is a
/// declaration somebody makes and letting the Watcher act is a grant, and neither
/// implies the other. The arms are written in the order the two are said in, so a
/// reader answering only the first cannot answer them out of order.
pub enum MayAct {
    /// Both said.
    May,
    /// Nothing has declared the Accounts in this Scope interchangeable, so there
    /// is nowhere to Switch to whatever the grant says. Carries the grant, because
    /// a Scope missing both is told about both.
    Undeclared { granted: bool },
    /// Declared, and nobody has told the Watcher it may act here.
    Ungranted,
}

impl MayAct {
    /// Whether both were said, for a reader with nothing to say about which was not.
    pub fn may(&self) -> bool {
        matches!(self, MayAct::May)
    }
}

/// Asked in one place, so no two readers of it can come to disagree about whether
/// a Scope is being watched.
pub fn may_act_within(registry: &Registry, scope: &Scope) -> MayAct {
    let granted = registry.settings(scope).watcher_may_act;
    match (may_cycle_within(registry, scope), granted) {
        (false, granted) => MayAct::Undeclared { granted },
        (true, false) => MayAct::Ungranted,
        (true, true) => MayAct::May,
    }
}

/// The Accounts a Cycle may not choose, counted once each, or empty where every
/// Account is a candidate. Shared with the Reserve, which counts the same set —
/// and every way out of [`is_a_candidate`] is counted here, or a Scope held back
/// by only the missing one renders an empty parenthetical.
pub fn out_of_the_running(sharers: &registry::Sharers, accounts: &[&Account]) -> String {
    let quarantined = accounts.iter().filter(|a| a.quarantined()).count();
    let disabled = accounts
        .iter()
        .filter(|a| a.disabled && !a.quarantined())
        .count();
    let sharing = accounts
        .iter()
        .filter(|a| !a.disabled && !a.quarantined() && sharers.hold(a.email()))
        .count();
    let mut out = Vec::new();
    if disabled > 0 {
        out.push(format!("{disabled} disabled"));
    }
    if quarantined > 0 {
        out.push(format!("{quarantined} Quarantined"));
    }
    if sharing > 0 {
        out.push(format!("{sharing} sharing a Profile with another Account"));
    }
    out.join(", ")
}

/// How much of an Account is left to spend, in a column's worth of words.
///
/// The figure the ranking is made on, said so the order can be checked against
/// it. Never observed is said as itself and never as a number: "no figure" and
/// "plenty of room" are opposite pieces of advice.
pub fn headroom_phrase(account: &Account) -> String {
    match headroom_of(account) {
        Headroom::Room { percent, .. } => format!("{}%", utilization::percentage(percent)),
        Headroom::Exhausted { .. } => "exhausted".to_string(),
        Headroom::Unobserved => "never observed".to_string(),
    }
}

/// The same three answers as a script reads them.
///
/// Two keys rather than a bare number, because only one of the three answers is
/// a number and the other two are not nought. Unrounded, like every percentage
/// in a document: rounding is what a column does to fit.
pub fn headroom_document(account: &Account) -> serde_json::Value {
    let (state, percent) = match headroom_of(account) {
        Headroom::Room { percent, .. } => ("room", Some(percent)),
        Headroom::Exhausted { .. } => ("exhausted", None),
        Headroom::Unobserved => ("never-observed", None),
    };
    serde_json::json!({ "state": state, "percent": percent })
}

/// What the winner won on, in the terms it was actually judged on, which are not
/// always the terms the Strategy asked for: a Scope set to `soonest-reset` with
/// no reset in sight lands on the most room and says so. Why the Strategy could
/// not be followed is the argument, and the argument is what is cut.
fn chosen_basis(best: &Ranked, strategy: Strategy, now: DateTime<Utc>) -> Basis {
    let headroom = &best.measured.headroom;
    match (headroom, headroom.ranked_on_reset(strategy, now)) {
        (Headroom::Room { .. }, Some(_)) => Basis::SoonestReset,
        // Room in the first tier exists only under Fable First, where its
        // figure is the Fable weekly's.
        (Headroom::Room { .. }, None) if best.measured.tier == 1 => Basis::MostFable,
        (Headroom::Room { .. }, None) => Basis::MostRoom,
        // Never observed. An exhausted Account cannot get here: everything
        // exhausted is answered above, and an unknown outranks a full window.
        _ => Basis::Unranked,
    }
}

/// The Scope holds Accounts and none of them is a candidate. Which way each one
/// left the running is [`out_of_the_running`]'s to count.
fn nobody_is_a_candidate(
    sharers: &registry::Sharers,
    scope: &Scope,
    accounts: &[&Account],
) -> String {
    format!(
        "No Account in {} is a Cycle candidate ({}).",
        scope.place(),
        out_of_the_running(sharers, accounts),
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
    // `> now` for the reason `ranked_on_reset` gives: an elapsed reset is not a
    // fact about when this Account comes back, and `min_by_key` over one crowns
    // the stalest reading.
    let soonest = ranked
        .iter()
        .filter_map(|ranked| match ranked.measured.headroom {
            Headroom::Exhausted {
                frees_at: Some(at), ..
            } if at > now => Some((at, ranked.account)),
            _ => None,
        })
        .min_by_key(|(at, _)| *at);
    // Both mean "the wait could be shorter than that", and are told apart
    // because with no `soonest` at all they are the opposite advice.
    let mut elapsed = 0;
    let mut uncached = 0;
    for ranked in ranked {
        match ranked.measured.headroom {
            Headroom::Exhausted { frees_at: None } => uncached += 1,
            Headroom::Exhausted {
                frees_at: Some(at), ..
            } if at <= now => elapsed += 1,
            _ => {}
        }
    }
    let unsaid = elapsed + uncached;

    let mut waiting = match soonest {
        Some((at, account)) => format!(
            "{} frees up soonest, at {}.",
            registry.named_for_the_user(account.email()),
            utilization::reset_phrase(at, now),
        ),
        // Every cached reset has come and gone, so the figures are what is old
        // rather than the Accounts that are full. Saying nothing said would send
        // somebody off to wait for a reset that has already happened.
        None if elapsed > 0 => format!(
            "Every reset any of them cached has already passed, so they may be \
             free now. {}",
            how_to_get_figures(scope)
        ),
        // None of them carried a reset. Saying nothing would read as "never".
        None => format!(
            "No cached figure says when any of them frees up. {}",
            how_to_get_figures(scope)
        ),
    };
    // An Account whose full window carries no reset could come back first, so
    // leaving it out silently turns "the soonest Perch can vouch for" into
    // advice to wait longer than you have to.
    if unsaid > 0 && soonest.is_some() {
        // Not "cache no reset time": an elapsed one caches a reset and it is
        // simply behind us, which says as little about the next window as no
        // reset at all — which is why the two are counted together.
        let say = match unsaid {
            1 => "says",
            _ => "say",
        };
        waiting.push_str(&format!(
            " {unsaid} of them {say} nothing about when they come back, so the \
             wait may be shorter. {}",
            how_to_get_figures(scope)
        ));
    }

    // What the filter took out before any of this was measured. Without it the
    // refusal sends somebody off to wait for a quota reset about a Group whose
    // two Accounts with full Headroom are merely disabled.
    let set_aside = out_of_the_running(&registry::Sharers::across(registry), accounts);
    let (every, also) = match set_aside.is_empty() {
        true => (String::new(), String::new()),
        false => (
            " Cycling may choose".to_string(),
            format!(" The others are out of the running ({set_aside})."),
        ),
    };

    format!(
        "Every Account in {}{every} is exhausted.{also}\n{waiting}",
        scope.place(),
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
    let scope = scope.place();
    // Said of the comparison Perch actually made: under `soonest-reset` it has
    // only compared Accounts whose figures carry a reset time.
    match here
        .measured
        .headroom
        .ranked_on_reset(strategy, now)
        .is_some()
    {
        true => format!(
            "{named} already comes back soonest of the Accounts in {scope} with a known reset."
        ),
        false => format!("{named} is already the best Account in {scope}."),
    }
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

    #[test]
    fn an_account_that_is_both_disabled_and_quarantined_is_counted_once() {
        let mut broken = account("broken@example.com", vec![]);
        broken.disabled = true;
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let mut spare = account("spare@example.com", vec![]);
        spare.disabled = true;

        let said = nobody_is_a_candidate(
            &registry::Sharers::across(&holding(vec![broken.clone(), spare.clone()])),
            &Scope::Ungrouped,
            &[&broken, &spare],
        );

        assert!(said.contains("1 disabled"), "{said}");
        assert!(said.contains("1 Quarantined"), "{said}");
    }

    /// A sharer is the third way out of `is_a_candidate`, and the only one
    /// whose absence here rendered `(   )` — an empty parenthetical where the
    /// sentence promised the reason nothing can be Cycled to.
    #[test]
    fn an_account_sharing_a_profile_is_counted_out_of_the_running() {
        let one = account("some-one@example.com", vec![]);
        let other = account("some.one@example.com", vec![]);

        let said = nobody_is_a_candidate(
            &registry::Sharers::across(&holding(vec![one.clone(), other.clone()])),
            &Scope::Ungrouped,
            &[&one, &other],
        );

        assert!(
            said.contains("2 sharing a Profile with another Account"),
            "{said}"
        );
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
            disabled: false,
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

    /// The Account being left, as fresh as the floor allows: a figure Perch may
    /// rank rivals against rather than one it must read again.
    fn just_read(mut account: Account) -> Account {
        let cached = account.utilization.as_mut().expect("a figure to freshen");
        cached.observed_at = now() - chrono::Duration::seconds(1);
        account
    }

    #[test]
    fn a_figure_older_than_the_watchers_interval_is_not_one_to_rank_rivals_against() {
        let stale = account("a@example.com", vec![window("5-hour", 10.0)]);
        assert_eq!(
            trusted(&stale, now()),
            None,
            "four minutes is past the floor"
        );
        assert_eq!(trusted(&just_read(stale), now()), Some(90.0));
    }

    #[test]
    fn a_figure_stamped_in_the_future_is_not_fresh() {
        let mut ahead = account("a@example.com", vec![window("5-hour", 10.0)]);
        ahead.utilization.as_mut().expect("a figure").observed_at =
            now() + chrono::Duration::hours(1);
        assert_eq!(trusted(&ahead, now()), None);
    }

    #[test]
    fn an_exhausted_account_read_just_now_is_trusted_at_nothing_left() {
        let full = just_read(account("a@example.com", vec![window("5-hour", 100.0)]));
        assert_eq!(trusted(&full, now()), Some(0.0));
    }

    #[test]
    fn a_window_whose_reset_is_still_ahead_caps_the_account_at_what_it_was_seen_with() {
        let account = account("a@example.com", vec![resetting("7-day", 9.0, 100)]);
        assert_eq!(best_case(&account, now()), 91.0);
    }

    #[test]
    fn a_window_past_its_reset_or_silent_about_one_caps_nothing() {
        let gone = account("a@example.com", vec![resetting("5-hour", 11.0, -21)]);
        assert_eq!(best_case(&gone, now()), 100.0, "the window has come back");
        let silent = account("b@example.com", vec![window("5-hour", 11.0)]);
        assert_eq!(
            best_case(&silent, now()),
            100.0,
            "it never said when it does"
        );
        let never = account("c@example.com", vec![]);
        assert_eq!(best_case(&never, now()), 100.0, "nothing has been observed");
    }

    /// The worst window, exactly as Headroom is: a five-hour window that has
    /// come back caps nothing, and the weekly one beside it still caps the
    /// Account.
    #[test]
    fn an_accounts_best_case_is_its_worst_windows() {
        let account = account(
            "a@example.com",
            vec![resetting("5-hour", 11.0, -21), resetting("7-day", 9.0, 100)],
        );
        assert_eq!(best_case(&account, now()), 91.0);
    }

    /// The transcript this rule was written around: a rival unseen for
    /// twenty-one hours, and arithmetic enough to answer without reading it.
    #[test]
    fn a_rival_that_could_not_win_at_its_best_is_not_read() {
        let registry = holding(vec![
            just_read(account("here@example.com", vec![window("5-hour", 0.0)])),
            account(
                "rival@example.com",
                vec![resetting("5-hour", 11.0, -21), resetting("7-day", 9.0, 100)],
            ),
        ]);
        let scope = Scope::Group("work".to_string());
        let to_beat = trusted(registry.account("here@example.com").unwrap(), now());

        assert_eq!(to_beat, Some(100.0));
        assert!(
            worth_reading(&registry, &scope, Some("here@example.com"), to_beat, now()).is_empty(),
            "91% at its very best loses to a trusted 100%"
        );
    }

    #[test]
    fn a_rival_that_could_win_at_its_best_is_read() {
        let registry = holding(vec![
            just_read(account("here@example.com", vec![window("5-hour", 50.0)])),
            account("rival@example.com", vec![resetting("7-day", 9.0, 100)]),
        ]);
        let scope = Scope::Group("work".to_string());
        let to_beat = trusted(registry.account("here@example.com").unwrap(), now());

        assert_eq!(
            worth_reading(&registry, &scope, Some("here@example.com"), to_beat, now()),
            vec!["rival@example.com".to_string()],
            "91% at its best beats a trusted 50%, so the cache cannot settle it"
        );
    }

    #[test]
    fn nothing_worth_vouching_for_means_everybody_worth_reading() {
        let registry = holding(vec![
            account("here@example.com", vec![window("5-hour", 0.0)]),
            account("rival@example.com", vec![resetting("7-day", 99.0, 100)]),
        ]);
        let scope = Scope::Group("work".to_string());

        assert_eq!(
            worth_reading(&registry, &scope, Some("here@example.com"), None, now()),
            vec!["rival@example.com".to_string()],
            "a 1% Best Case still cannot be ruled out against nothing"
        );
    }

    #[test]
    fn a_rival_inside_the_floor_is_never_read() {
        let registry = holding(vec![
            just_read(account("here@example.com", vec![window("5-hour", 50.0)])),
            just_read(account("rival@example.com", vec![window("5-hour", 1.0)])),
        ]);
        let scope = Scope::Group("work".to_string());

        assert!(
            worth_reading(
                &registry,
                &scope,
                Some("here@example.com"),
                Some(50.0),
                now()
            )
            .is_empty(),
            "its figure is as young as the one it would be measured against"
        );
    }

    /// A bound on room excludes nobody from an order made on reset times, so the
    /// Strategy that ranks on one reads every stale candidate.
    #[test]
    fn soonest_reset_proves_nothing_from_a_best_case() {
        let registry = preferring(
            holding(vec![
                just_read(account("here@example.com", vec![window("5-hour", 0.0)])),
                account(
                    "rival@example.com",
                    vec![resetting("5-hour", 11.0, -21), resetting("7-day", 9.0, 100)],
                ),
            ]),
            crate::config::Strategy::SoonestReset,
        );
        let scope = Scope::Group("work".to_string());

        assert_eq!(
            worth_reading(
                &registry,
                &scope,
                Some("here@example.com"),
                Some(100.0),
                now()
            ),
            vec!["rival@example.com".to_string()],
        );
    }

    #[test]
    fn a_quarantined_or_disabled_account_is_never_worth_reading() {
        let mut out = account("out@example.com", vec![]);
        out.disabled = true;
        let registry = holding(vec![
            just_read(account("here@example.com", vec![window("5-hour", 50.0)])),
            out,
        ]);
        let scope = Scope::Group("work".to_string());

        assert!(
            worth_reading(
                &registry,
                &scope,
                Some("here@example.com"),
                Some(50.0),
                now()
            )
            .is_empty(),
            "a Cycle could not land on it, so its figure decides nothing"
        );
    }

    pub(crate) fn holding(accounts: Vec<Account>) -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.settle(accounts.first().map(|first| first.email().to_string()));
        for account in accounts {
            registry.upsert(account);
        }
        registry
    }

    /// The same Group, told to prefer the other of the two Strategies.
    pub(crate) fn preferring(
        mut registry: Registry,
        strategy: crate::config::Strategy,
    ) -> Registry {
        registry.groups.get_mut("work").expect("declared").strategy = strategy;
        registry
    }

    /// The same Group, told to spend Fable first.
    fn preferring_fable(mut registry: Registry) -> Registry {
        registry
            .groups
            .get_mut("work")
            .expect("declared")
            .prefer_fable = true;
        registry
    }

    fn cycle(registry: &Registry) -> Result<Choice> {
        setting_aside(registry, &SetAside::nothing())
    }

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    /// The fixture surprises: a Group is the declaration, so only the Ungrouped
    /// Scope can be `Undeclared` and the two Settings have to be set apart.
    fn standing(interchangeable: bool, granted: bool) -> (Registry, Scope) {
        let mut registry = holding(vec![account("one@example.com", vec![])]);
        registry.ungrouped.interchangeable = interchangeable;
        registry.ungrouped.settings.watcher_may_act = granted;
        (registry, Scope::Ungrouped)
    }

    #[test]
    fn a_scope_told_both_things_is_the_only_one_the_watcher_may_act_within() {
        let (registry, scope) = standing(true, true);
        assert!(may_act_within(&registry, &scope).may());
    }

    #[test]
    fn the_declaration_is_answered_before_the_grant_where_both_are_missing() {
        let (registry, scope) = standing(false, false);
        assert!(matches!(
            may_act_within(&registry, &scope),
            MayAct::Undeclared { granted: false }
        ));
    }

    #[test]
    fn a_grant_without_the_declaration_is_still_undeclared_and_says_so() {
        let (registry, scope) = standing(false, true);
        assert!(matches!(
            may_act_within(&registry, &scope),
            MayAct::Undeclared { granted: true }
        ));
    }

    #[test]
    fn the_declaration_without_the_grant_is_ungranted() {
        let (registry, scope) = standing(true, false);
        assert!(matches!(
            may_act_within(&registry, &scope),
            MayAct::Ungranted
        ));
    }

    #[test]
    fn a_group_carries_the_declaration_so_only_its_grant_is_ever_missing() {
        let registry = holding(vec![account("one@example.com", vec![])]);
        assert!(matches!(
            may_act_within(&registry, &work()),
            MayAct::Ungranted
        ));
    }

    fn ranked_emails(registry: &Registry) -> Vec<&str> {
        ranked(registry, &work(), now())
            .into_iter()
            .map(Account::email)
            .collect()
    }

    #[test]
    fn the_account_being_left_is_never_the_top_row_even_where_nothing_has_been_observed() {
        let registry = holding(vec![
            account("here@example.com", vec![]),
            account("there@example.com", vec![]),
        ]);

        assert_eq!(
            ranked_emails(&registry),
            ["there@example.com", "here@example.com"],
            "the Account being left sorts below the one a Cycle could land on"
        );
        assert_eq!(
            cycle(&registry)
                .expect("there is somewhere to go")
                .account
                .email(),
            "there@example.com",
            "and it is the one the choice makes, which is the whole point"
        );
    }

    #[test]
    fn an_exhausted_account_never_outranks_the_one_being_left() {
        let registry = holding(vec![
            account("here@example.com", vec![]),
            account("full@example.com", vec![window("5-hour", 100.0)]),
            account("fuller@example.com", vec![window("5-hour", 100.0)]),
        ]);

        assert_eq!(
            ranked_emails(&registry),
            ["here@example.com", "full@example.com", "fuller@example.com"],
            "nothing a Cycle would refuse to land on sorts above the Account it \
             would be leaving"
        );
        assert!(
            cycle(&registry).is_err(),
            "and the choice agrees there is nowhere to go, which is what makes \
             the order above the only honest one"
        );
    }

    /// The branch carrying `expect("something unexhausted is here or
    /// elsewhere")`, whose safety is spread over three earlier checks:
    /// everything-exhausted is caught above, and what is not `here` is in
    /// `elsewhere`, which was just found empty.
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
            said.contains("nothing has been observed of it"),
            "and why staying put is not a comparison Perch made: {said}"
        );
        assert!(
            said.contains("perch list work --refresh"),
            "and how to get a figure so it can be one, over exactly the Scope \
             the missing figures are in: {said}"
        );
    }

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

    #[test]
    fn an_account_no_cycle_would_choose_sorts_below_every_one_it_would() {
        let mut spared = account("spared@example.com", vec![window("5-hour", 0.0)]);
        spared.disabled = true;
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

    fn setting_aside(registry: &Registry, set_aside: &SetAside) -> Result<Choice> {
        choose(
            registry,
            &Scope::Group("work".to_string()),
            registry.active().whose(),
            set_aside,
            now(),
        )
    }

    #[test]
    fn headroom_is_the_room_left_in_the_fullest_window() {
        let held = account(
            "a@example.com",
            vec![window("5-hour", 4.0), window("7-day", 95.0)],
        );
        let headroom = headroom_of(&held);
        assert!(
            matches!(
                headroom,
                Headroom::Room {
                    percent: 5.0,
                    fullest_window,
                    ..
                } if fullest_window == "7-day"
            ),
            "the window with the least room decides, so the number is true of \
             every window: {headroom:?}"
        );
    }

    /// `Account::observed_utilization`'s filter is the whole guard on
    /// `headroom_of`'s `expect("an observation carries at least one window")`,
    /// and `observe::keep` stores what it is given — so the value that `expect`
    /// forbids is one Perch can write.
    #[test]
    fn an_observation_carrying_no_windows_reads_as_never_observed_rather_than_panicking() {
        let mut held = account("a@example.com", vec![window("5-hour", 4.0)]);
        held.utilization = Some(CachedUtilization {
            observed_at: now(),
            windows: Vec::new(),
        });

        assert_eq!(held.observed_utilization(), None);
        assert_eq!(
            headroom_of(&held),
            Headroom::Unobserved,
            "a figure with nothing in it is no figure, not a full window"
        );
    }

    #[test]
    fn a_full_window_exhausts_an_account_however_empty_its_others_are() {
        let held = account(
            "a@example.com",
            vec![window("5-hour", 0.0), window("7-day", 100.0)],
        );
        assert!(headroom_of(&held).is_exhausted());
    }

    #[test]
    fn an_exhausted_account_frees_up_when_its_last_full_window_resets() {
        let held = account(
            "a@example.com",
            vec![
                resetting("5-hour", 100.0, 1),
                resetting("7-day", 100.0, 50),
                // Not full, so its reset has no bearing on the wait.
                resetting("7-day-opus", 3.0, 100),
            ],
        );
        assert_eq!(
            headroom_of(&held),
            Headroom::Exhausted {
                frees_at: Some(now() + chrono::Duration::hours(50))
            }
        );
    }

    #[test]
    fn a_full_window_that_does_not_say_when_it_resets_leaves_the_wait_unknown() {
        let held = account(
            "a@example.com",
            vec![window("5-hour", 100.0), resetting("7-day", 100.0, 3)],
        );
        let headroom = headroom_of(&held);
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
    fn preferring_fable_ranks_the_first_tier_by_the_fable_weekly_not_the_worst_window() {
        let accounts = || {
            vec![
                account(
                    "here@example.com",
                    vec![window("5-hour", 90.0), window("7-day-fable", 90.0)],
                ),
                account(
                    "draining@example.com",
                    vec![window("5-hour", 70.0), window("7-day-fable", 40.0)],
                ),
                account(
                    "fuller@example.com",
                    vec![window("5-hour", 10.0), window("7-day-fable", 60.0)],
                ),
            ]
        };

        assert_eq!(
            cycle(&holding(accounts()))
                .expect("there is room")
                .account
                .email(),
            "fuller@example.com",
            "off, the worst window decides"
        );

        let choice = cycle(&preferring_fable(holding(accounts()))).expect("there is room");
        assert_eq!(
            choice.account.email(),
            "draining@example.com",
            "on, the most Fable weekly left decides"
        );
        assert_eq!(choice.basis, Basis::MostFable);
    }

    #[test]
    fn a_fable_spent_scope_falls_through_to_the_best_of_what_remains() {
        let accounts = || {
            vec![
                account(
                    "here@example.com",
                    vec![window("5-hour", 30.0), window("7-day-fable", 100.0)],
                ),
                account(
                    "resting@example.com",
                    vec![window("5-hour", 60.0), window("7-day-fable", 100.0)],
                ),
                account(
                    "spare@example.com",
                    vec![window("5-hour", 20.0), window("7-day-fable", 100.0)],
                ),
            ]
        };

        assert!(
            cycle(&holding(accounts())).is_err(),
            "off, a full Fable weekly exhausts every one of them"
        );

        let choice = cycle(&preferring_fable(holding(accounts())))
            .expect("a full Fable weekly alone does not exhaust the fall-through tier");
        assert_eq!(choice.account.email(), "spare@example.com");
        assert_eq!(choice.basis, Basis::MostRoom);
    }

    #[test]
    fn a_full_five_hour_window_keeps_an_account_out_of_the_fable_tier_however_empty_its_fable_is() {
        let registry = preferring_fable(holding(vec![
            account(
                "here@example.com",
                vec![window("5-hour", 90.0), window("7-day-fable", 90.0)],
            ),
            account(
                "blocked@example.com",
                vec![window("5-hour", 100.0), window("7-day-fable", 0.0)],
            ),
            account(
                "serving@example.com",
                vec![window("5-hour", 50.0), window("7-day-fable", 80.0)],
            ),
        ]));

        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "serving@example.com",
            "an Account that cannot serve a Fable request now is no Fable candidate"
        );
    }

    #[test]
    fn an_account_without_a_fable_window_ranks_below_known_fable_room_and_above_a_spent_one() {
        let registry = preferring_fable(holding(vec![
            account(
                "here@example.com",
                vec![window("5-hour", 100.0), window("7-day-fable", 100.0)],
            ),
            account(
                "spent@example.com",
                vec![window("5-hour", 10.0), window("7-day-fable", 100.0)],
            ),
            account("unsaid@example.com", vec![window("5-hour", 10.0)]),
            account(
                "known@example.com",
                vec![window("5-hour", 80.0), window("7-day-fable", 90.0)],
            ),
        ]));

        assert_eq!(
            ranked_emails(&registry),
            [
                "known@example.com",
                "unsaid@example.com",
                "spent@example.com",
                "here@example.com",
            ]
        );
    }

    #[test]
    fn a_strategy_orders_within_a_tier_and_never_across_one() {
        let registry = preferring(
            preferring_fable(holding(vec![
                account(
                    "here@example.com",
                    vec![
                        resetting("7-day-fable", 90.0, 40),
                        resetting("5-hour", 10.0, 2),
                    ],
                ),
                account(
                    "serving@example.com",
                    vec![
                        resetting("7-day-fable", 50.0, 90),
                        resetting("5-hour", 10.0, 3),
                    ],
                ),
                // The soonest reset in the Scope, and in the fall-through tier.
                account(
                    "spent@example.com",
                    vec![
                        resetting("7-day-fable", 100.0, 1),
                        resetting("5-hour", 5.0, 1),
                    ],
                ),
            ])),
            Strategy::SoonestReset,
        );

        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "serving@example.com",
            "soonest-reset orders the Fable tier and never promotes one out of \
             the fall-through"
        );
    }

    #[test]
    fn another_models_full_weekly_does_not_keep_an_account_out_of_the_fable_tier() {
        let registry = preferring_fable(holding(vec![
            account(
                "here@example.com",
                vec![window("5-hour", 90.0), window("7-day-fable", 90.0)],
            ),
            account(
                "opus-spent@example.com",
                vec![
                    window("5-hour", 10.0),
                    window("7-day-fable", 50.0),
                    window("7-day-opus", 100.0),
                ],
            ),
        ]));

        assert_eq!(
            cycle(&registry)
                .expect("a Fable request never spends from another model's weekly")
                .account
                .email(),
            "opus-spent@example.com",
        );
    }

    #[test]
    fn a_preference_matching_no_observed_window_is_said_and_ranks_on_headroom_alone() {
        let registry = preferring_fable(holding(vec![
            account("here@example.com", vec![window("5-hour", 50.0)]),
            account("roomy@example.com", vec![window("5-hour", 10.0)]),
        ]));

        let said =
            fable_unmatched(&registry, &work()).expect("observed, and nothing reports the window");
        assert!(said.contains(THE_FABLE_WINDOW), "{said}");
        assert_eq!(
            measure_of(&registry, &work()),
            Measure::Worst,
            "tiers keyed on a window nobody reports order nothing"
        );
        assert_eq!(
            cycle(&registry).expect("there is room").account.email(),
            "roomy@example.com",
        );

        let unobserved = preferring_fable(holding(vec![
            account("here@example.com", vec![]),
            account("new@example.com", vec![]),
        ]));
        assert_eq!(
            fable_unmatched(&unobserved, &work()),
            None,
            "nothing observed is not a mismatch"
        );
        assert_eq!(
            fable_unmatched(&holding(vec![account("here@example.com", vec![])]), &work()),
            None,
            "and neither is a Scope that never said `prefer-fable`"
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
            error
                .to_string()
                .contains("1 of them says nothing about when they come back"),
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
        assert_eq!(
            choice.basis,
            Basis::SoonestReset,
            "and what it says it chose on is what it chose on",
        );
    }

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

    #[test]
    fn windows_equally_full_are_broken_by_which_one_bites_first() {
        let registry = preferring(
            holding(vec![
                account("active@example.com", vec![resetting("5-hour", 40.0, 3)]),
                // Both at 40%, and only the `5-hour` says when it comes back —
                // in one hour, so it is the window this Account is constrained
                // by and the one to rank it on.
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
        // A `5-hour` read as carrying no reset would have fallen back to the
        // room it could see, which is what this basis says it did not do.
        assert_eq!(
            choice.basis,
            Basis::SoonestReset,
            "so the Account is chosen on that window's reset",
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
        // `ranked_on_reset` is the only way to this basis and it declines a
        // reset already gone, so the basis is the claim that what it ranked on
        // is still to come.
        assert_eq!(
            choice.basis,
            Basis::SoonestReset,
            "and the reset it is chosen on is one still to come",
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
        assert_eq!(
            choice.basis,
            Basis::MostRoom,
            "nothing may claim a reset it has not got",
        );
    }

    /// A figure Perch quotes carries its age and a reset it quotes has to be
    /// one still to come, so the verdict that needs nothing done quotes
    /// neither. What a reset already elapsed renders as is asserted where one
    /// still reaches a sentence, in `tests/cycling.rs`.
    #[test]
    fn staying_put_names_where_you_are_and_quotes_no_figure() {
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

        assert!(said.contains("here@example.com"), "which Account: {said}");
        assert!(said.contains("Group `work`"), "and against what: {said}");
        for quoted in ["%", "as of", "resets at", "has passed", "any moment now"] {
            assert!(
                !said.contains(quoted),
                "a verdict with nothing to do quotes no figure, and this says \
                 `{quoted}`: {said}"
            );
        }
    }

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

    /// `the_order_is_the_one_the_choice_would_make` cannot see this: under
    /// `most-headroom` with no reset times the Strategy's ranking and the room
    /// ranking are one ordering, and the veto collapses to the identity.
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
                 ranking the listing exists to make visible is not the one \
                 Perch uses — listed {listed:?}"
            );
        }
    }

    /// `Registry::upsert` matches with `same_name` and stores the *incoming*
    /// spelling, so an Identity Claude Code re-writes under another
    /// capitalization leaves `active` naming the entry the old way — the state
    /// this fixture puts the Registry in.
    #[test]
    fn the_listing_agrees_with_the_cycle_however_the_active_address_is_capitalized() {
        let registry = preferring(
            {
                let mut registry = holding(vec![
                    account("here@example.com", vec![resetting("5-hour", 40.0, 1)]),
                    account("worse@example.com", vec![resetting("5-hour", 95.0, 2)]),
                    account("roomiest@example.com", vec![window("5-hour", 5.0)]),
                ]);
                registry.settle(Some("HERE@EXAMPLE.COM".to_string()));
                registry
            },
            Strategy::SoonestReset,
        );

        assert_eq!(
            ranked_emails(&registry),
            [
                "roomiest@example.com",
                "here@example.com",
                "worse@example.com"
            ],
            "the Account being left sorts by the veto whatever case `active` \
             happens to spell it in",
        );
    }

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
        assert_eq!(
            choice.basis,
            Basis::MostRoom,
            "and the room it fell back to is what it says it chose on, rather \
             than the ranking that was asked for",
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

    /// The cache said exactly when each of them came back, so telling somebody
    /// nothing said is the opposite advice: they wait on a reset that has
    /// already happened.
    #[test]
    fn everyone_exhausted_on_resets_that_have_passed_says_they_may_be_free_now() {
        let registry = holding(vec![
            account("here@example.com", vec![resetting("5-hour", 100.0, -2)]),
            account("full@example.com", vec![resetting("5-hour", 100.0, -1)]),
        ]);

        let refusal = cycle(&registry)
            .expect_err("everything is exhausted")
            .to_string();

        assert!(
            !refusal.contains("No cached figure says when any"),
            "the cache said when each of them frees up: {refusal}"
        );
        assert!(
            refusal.contains("may be free now"),
            "and an elapsed reset is good news rather than a wait: {refusal}"
        );
    }

    #[test]
    fn a_scope_where_nobody_is_a_candidate_says_which_way_each_account_left_it() {
        let mut registry = holding(vec![
            account("here@example.com", vec![window("5-hour", 100.0)]),
            account("off@example.com", vec![window("5-hour", 1.0)]),
            account("broken@example.com", vec![window("5-hour", 1.0)]),
        ]);
        registry.account_mut("here@example.com").unwrap().disabled = true;
        registry.account_mut("off@example.com").unwrap().disabled = true;
        registry.quarantine("broken@example.com", Quarantine::RenewalRejected);

        let error = cycle(&registry).expect_err("nobody is a candidate");

        assert_eq!(error.exit_code(), crate::error::EXIT_NO_CANDIDATE);
        assert!(error.to_string().contains("2 disabled"), "{error}");
        assert!(error.to_string().contains("1 Quarantined"), "{error}");
    }
}

/// Properties the ranking holds for every arrangement of Accounts, not only the
/// ones somebody wrote a fixture for: a comparator sign flipped inside one
/// Strategy passes every example that does not use that Strategy. A fixed seed
/// printed in every failure, from a congruential generator rather than a crate
/// (ADR a-crate-must-not-cost-a-seam).
#[cfg(test)]
mod properties {
    use super::tests::*;
    use super::*;
    use crate::config::Strategy;
    use crate::registry::Quarantine;

    /// One case: some Accounts, in some shape, under one Strategy.
    struct Arrangement {
        registry: Registry,
        strategy: Strategy,
        described: String,
    }

    /// A deterministic stream of numbers: reproducible rather than random, which
    /// is the property that matters when a case fails.
    struct Cases(u64);

    impl Cases {
        fn next(&mut self, below: u64) -> u64 {
            // Numerical Recipes' constants. Any full-period generator will do.
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
                // Every shape the ranking distinguishes.
                let used = self.next(101) as f64;
                let windows = match self.next(4) {
                    0 => vec![],
                    1 => vec![window("5-hour", used)],
                    2 => vec![resetting("5-hour", used, 1 + self.next(48) as i64)],
                    _ => vec![window("5-hour", 100.0)],
                };
                let mut held = account(&email, windows);
                held.disabled = self.next(6) == 0;
                held.quarantine = (self.next(8) == 0).then_some(Quarantine::RenewalRejected);
                described.push_str(&format!(
                    "\n  {email}: {:?} disabled={} quarantined={}",
                    headroom_of(&held),
                    held.disabled,
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

    #[test]
    fn nothing_disabled_exhausted_or_quarantined_is_ever_chosen() {
        for arrangement in cases() {
            let Some(won) = chosen(&arrangement, None) else {
                continue;
            };
            assert!(!won.disabled, "{}", arrangement.described);
            assert!(!won.quarantined(), "{}", arrangement.described);
            assert!(
                !headroom_of(&won).is_exhausted(),
                "{}",
                arrangement.described
            );
        }
    }

    #[test]
    fn the_winner_ranks_at_least_as_high_as_every_candidate() {
        for arrangement in cases() {
            let Some(won) = chosen(&arrangement, None) else {
                continue;
            };
            let winning = headroom_of(&won).ranking(arrangement.strategy, now());
            for held in &arrangement.registry.accounts {
                if held.disabled || held.quarantined() || headroom_of(held).is_exhausted() {
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

    /// Asked with `leaving` set, which the two properties above do not do:
    /// without it the whole comparison against `here` — the veto and the choice
    /// it gates — goes unexercised.
    #[test]
    fn the_account_chosen_is_never_one_the_account_being_left_beats_outright() {
        for arrangement in cases() {
            let Some(leaving) = arrangement.registry.accounts.first() else {
                continue;
            };
            // Only where the Account being left is one a Cycle would consider:
            // the figure beside a broken Credential is not a standard anything
            // has to beat.
            if !is_a_candidate(&registry::Sharers::across(&arrangement.registry), leaving) {
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

    #[test]
    fn the_same_arrangement_always_chooses_the_same_account() {
        for arrangement in cases() {
            let once = chosen(&arrangement, None).map(|won| won.email().to_string());
            let twice = chosen(&arrangement, None).map(|won| won.email().to_string());
            assert_eq!(once, twice, "{}", arrangement.described);
        }
    }
}
