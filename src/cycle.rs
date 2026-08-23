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

use crate::error::{PerchError, Result};
use crate::registry::{
    self, Account, CachedUtilization, Registry, Scope, Strategy, WindowUtilization,
};
use crate::utilization;

/// Which Account a Scope prefers when more than one would serve.
///
/// Read from the Settings the Scope itself holds — there is nothing above it to
/// fall back to (ADR a-setting-names-its-scope). The ungrouped Accounts are not
/// a Group but are a Scope, so what they Cycle by is something a person says.
fn strategy(registry: &Registry, scope: &Scope) -> Strategy {
    registry.settings(scope).strategy
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
             interchangeable with. Nothing was changed.\n\
             Either put it in a Group with `perch group move {} <group>`, or \
             declare that every ungrouped Account is interchangeable with \
             `perch config set ungrouped interchangeable true`.",
            registry.named_for_the_user(leaving.email()),
            leaving.email(),
        ))),
    }
}

/// How full an Account is: by the Quota Window that is fullest.
#[derive(Debug, Clone, PartialEq)]
enum Headroom {
    /// Every Quota Window has at least this much room left.
    Room {
        percent: f64,
        fullest_window: String,
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

impl Headroom {
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

    /// The figure as a clause, for the two sentences that quote it: why an
    /// Account won, and why staying put is already the best you can do.
    ///
    /// Both make the same promise about the same number, so they make it in the
    /// same words, and the clause is the one the Strategy judged on.
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
        // The same predicate the ranking asked, so the number quoted as the
        // reason is the number that decided it.
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
            // Ranked on its room, because that is what there was to rank it
            // on. An elapsed reset is said as stale rather than as absent.
            (Strategy::SoonestReset, None) => match resets_at {
                // The clock time alone: `reset_phrase` renders a time already
                // gone as "any moment now", which is a contradiction two words
                // before "which has passed".
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

/// What reads a figure Perch has not got, said the same way wherever the absence
/// of one is the reason for the answer. Named against the Scope the Cycle looked
/// in, because a refresh reads the Accounts it is about to show and no others.
fn how_to_get_figures(scope: &Scope) -> String {
    format!(
        "`perch list {} --refresh` reads current figures.",
        scope.word()
    )
}

/// The Quota Window that decides how full an Account is: its fullest, or `None`
/// for one nothing has ever been observed of.
///
/// Public because the Watcher compares the Account it is on against a Threshold,
/// and two measures of fullness would act on one number and choose on another.
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

/// The fullest window, and on a tie the most perishable of the equally full
/// ones.
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
/// reset, or `None` where any of them does not say.
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
        self.emails
            .iter()
            .any(|held| registry::same_name(held, email))
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
            Basis::Unranked => "nothing observed to rank on",
        };
        format!("{basis} in {}", scope.place())
    }
}

/// The Account a Cycle picked, with what it will say about having picked it.
#[derive(Debug)]
pub struct Choice {
    /// Owned, so the caller can go on to write the registry it came from.
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
    let accounts = scope.accounts(registry);
    if accounts.is_empty() {
        return Err(PerchError::NoCandidate(format!(
            "{} holds no Accounts, so there is nowhere to Cycle to. Nothing was \
             changed.",
            scope.place(),
        )));
    }

    let mut ranked: Vec<Ranked> = accounts
        .iter()
        .filter(|account| is_a_candidate(registry, account))
        .map(|account| Ranked {
            account,
            headroom: headroom_of(account),
        })
        .collect();
    if ranked.is_empty() {
        return Err(PerchError::NoCandidate(nobody_is_a_candidate(
            registry, scope, &accounts,
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

    let is_leaving = |ranked: &Ranked| {
        leaving.is_some_and(|email| registry::same_name(ranked.account.email(), email))
    };
    let here = ranked.iter().find(|ranked| is_leaving(ranked));
    let landable: Vec<&Ranked> = ranked
        .iter()
        .filter(|ranked| !is_leaving(ranked) && !ranked.headroom.is_exhausted())
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
             never observed how full it is. Nothing was changed — {}",
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
fn measured_against(here: Option<&Headroom>) -> Option<&Headroom> {
    here.filter(|here| matches!(here, Headroom::Room { .. }))
}

/// Whether moving from `here` to `other` would gain anything, on the Strategy's
/// ordering or on the room alone.
///
/// One predicate, because [`choose`] and [`ranked`] both need it, and two
/// spellings of it are two orders over one Scope.
fn worth_leaving_for(
    other: &Headroom,
    here: &Headroom,
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
    let accounts = scope.accounts(registry);
    // The Account a Cycle would be leaving, measured exactly as `choose`
    // measures it, and only where it is a candidate carrying a figure.
    let leaving = registry.active().whose();
    let here = leaving
        .and_then(|active| {
            accounts
                .iter()
                .find(|account| registry::same_name(account.email(), active))
        })
        .filter(|account| is_a_candidate(registry, account))
        .map(|account| headroom_of(account));
    let here = measured_against(here.as_ref());
    // Measured once each rather than inside a comparator that runs O(n log n)
    // times, since `place` clones a window name per `Headroom` it computes.
    // Stable, so Accounts that rank identically keep the order they were added.
    let mut placed: Vec<(&Account, Place)> = accounts
        .into_iter()
        .map(|account| {
            (
                account,
                place(registry, account, leaving, here, strategy, now),
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
type Place = ((u8, u8, u8), f64);

fn place(
    registry: &Registry,
    account: &Account,
    leaving: Option<&str>,
    here: Option<&Headroom>,
    strategy: Strategy,
    now: DateTime<Utc>,
) -> Place {
    let candidate = is_a_candidate(registry, account);
    let headroom = headroom_of(account);
    // Asked only of a candidate, which is the only set `choose` asks it of: it
    // drops the non-candidates before looking for the one being left.
    let staying =
        candidate && leaving.is_some_and(|email| registry::same_name(account.email(), email));
    // Exhausted asked outright rather than left to `worth_leaving_for`, whose
    // `here` is `None` both for an unobserved active Account and for an
    // exhausted one — so `is_none_or` would pass every exhausted candidate.
    let worth = u8::from(
        !staying
            && !headroom.is_exhausted()
            && here.is_none_or(|here| worth_leaving_for(&headroom, here, strategy, now)),
    );
    let (tier, figure) = headroom.ranking(strategy, now);
    ((u8::from(candidate), worth, tier), figure)
}

/// Whether a Cycle could land on this Account at all — never a Disabled or
/// Quarantined one, nor a sharer, whom every Switch refuses.
///
/// One predicate, because what a Cycle may choose, what a Scope has left to draw
/// on ([`crate::reserve`]) and what a Remove lands on are one set of Accounts.
pub fn is_a_candidate(registry: &Registry, account: &Account) -> bool {
    !account.disabled
        && !account.quarantined()
        && registry::sharing_a_profile_with(registry, account).is_none()
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

/// The Accounts a Cycle may not choose, counted once each, or empty where every
/// Account is a candidate. Shared with the Reserve, which counts the same set —
/// and every way out of [`is_a_candidate`] is counted here, or a Scope held back
/// by only the missing one renders an empty parenthetical.
pub fn out_of_the_running(registry: &Registry, accounts: &[&Account]) -> String {
    let quarantined = accounts.iter().filter(|a| a.quarantined()).count();
    let disabled = accounts
        .iter()
        .filter(|a| a.disabled && !a.quarantined())
        .count();
    let sharing = accounts
        .iter()
        .filter(|a| {
            !a.disabled
                && !a.quarantined()
                && registry::sharing_a_profile_with(registry, a).is_some()
        })
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

/// The three answers there are to "how much has this Account left", for the
/// callers that have to tell them apart rather than print them.
///
/// One value rather than a number and a second question beside it: two
/// predicates that have to stay in agreement are a tally that stops adding up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HowMuchIsLeft {
    /// Every Quota Window has at least this much room left.
    Room(f64),
    /// A Quota Window is full, so the Account is blocked whatever its others
    /// say.
    Exhausted,
    /// No figure has ever been read.
    NeverObserved,
}

/// Which of those three an Account is.
pub fn how_much_is_left(account: &Account) -> HowMuchIsLeft {
    match headroom_of(account) {
        Headroom::Room { percent, .. } => HowMuchIsLeft::Room(percent),
        Headroom::Exhausted { .. } => HowMuchIsLeft::Exhausted,
        Headroom::Unobserved => HowMuchIsLeft::NeverObserved,
    }
}

/// The same three answers as a script reads them.
///
/// Two keys rather than a bare number, because only one of the three answers is
/// a number and the other two are not nought. Unrounded, like every percentage
/// in a document: rounding is what a column does to fit.
pub fn headroom_document(account: &Account) -> serde_json::Value {
    let (state, percent) = match how_much_is_left(account) {
        HowMuchIsLeft::Room(percent) => ("room", Some(percent)),
        HowMuchIsLeft::Exhausted => ("exhausted", None),
        HowMuchIsLeft::NeverObserved => ("never-observed", None),
    };
    serde_json::json!({ "state": state, "percent": percent })
}

/// How much of an Account is left to spend, with the Quota Window the figure was
/// taken from and the age of the observation it came from.
///
/// The long form of [`headroom_phrase`]: naming the fullest window makes the
/// claim checkable against the rows underneath.
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

/// What the winner won on, in the terms it was actually judged on, which are not
/// always the terms the Strategy asked for: a Scope set to `soonest-reset` with
/// no reset in sight lands on the most room and says so. Why the Strategy could
/// not be followed is the argument, and the argument is what is cut.
fn chosen_basis(best: &Ranked, strategy: Strategy, now: DateTime<Utc>) -> Basis {
    match (&best.headroom, best.headroom.ranked_on_reset(strategy, now)) {
        (Headroom::Room { .. }, Some(_)) => Basis::SoonestReset,
        (Headroom::Room { .. }, None) => Basis::MostRoom,
        // Never observed. An exhausted Account cannot get here: everything
        // exhausted is answered above, and an unknown outranks a full window.
        _ => Basis::Unranked,
    }
}

/// The Scope holds Accounts and none of them is a candidate. Which way each one
/// left the running is [`out_of_the_running`]'s to count.
fn nobody_is_a_candidate(registry: &Registry, scope: &Scope, accounts: &[&Account]) -> String {
    format!(
        "No Account in {} is a Cycle candidate ({}), so there is nowhere to \
         Switch to. Nothing was changed.",
        scope.place(),
        out_of_the_running(registry, accounts),
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
        .filter_map(|ranked| match ranked.headroom {
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
        match ranked.headroom {
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
             free now — {}",
            how_to_get_figures(scope)
        ),
        // None of them carried a reset. Saying nothing would read as "never".
        None => format!(
            "No cached figure says when any of them frees up — {}",
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
             wait could be shorter than that — {}",
            how_to_get_figures(scope)
        ));
    }

    // What the filter took out before any of this was measured. Without it the
    // refusal sends somebody off to wait for a quota reset about a Group whose
    // two Accounts with full Headroom are merely disabled.
    let set_aside = out_of_the_running(registry, accounts);
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
    let how_to_get_figures = how_to_get_figures(scope);
    let scope = scope.place();
    // Said of the comparison Perch actually made: under `soonest-reset` it has
    // only compared Accounts whose figures carry a reset time.
    let standing = if here.headroom.ranked_on_reset(strategy, now).is_some() {
        format!(
            "{named} already comes back soonest of the Accounts in {scope} whose figures say when they do"
        )
    } else {
        format!("{named} is already the best Account in {scope}")
    };
    format!(
        "{standing}, with {}. Nothing was changed — {how_to_get_figures}",
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

    #[test]
    fn an_account_that_is_both_disabled_and_quarantined_is_counted_once() {
        let mut broken = account("broken@example.com", vec![]);
        broken.disabled = true;
        broken.quarantine = Some(Quarantine::RenewalRejected);
        let mut spare = account("spare@example.com", vec![]);
        spare.disabled = true;

        let said = nobody_is_a_candidate(
            &holding(vec![broken.clone(), spare.clone()]),
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
            &holding(vec![one.clone(), other.clone()]),
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
        strategy: crate::registry::Strategy,
    ) -> Registry {
        registry.groups.get_mut("work").expect("declared").strategy = strategy;
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
            said.contains("never observed how full it is"),
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
            registry.active().whose(),
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

        // A Switch says the basis and leaves the figures to the lines under it,
        // so the clause is quoted only by the refusal that says staying put is
        // best — asserted here, over the same two figures.
        let staying = preferring(
            holding(vec![
                account("emptier@example.com", vec![resetting("5-hour", 20.0, -1)]),
                account("fuller@example.com", vec![resetting("5-hour", 80.0, -9)]),
            ]),
            Strategy::SoonestReset,
        );
        let refusal = cycle(&staying)
            .expect_err("there is nowhere emptier to go")
            .to_string();

        assert!(
            !refusal.contains("resets at"),
            "the clause quoting the figure may not claim a reset either: {refusal}"
        );
        assert!(
            refusal.contains("has passed"),
            "it says the reading is stale rather than absent: {refusal}"
        );
        // The parenthetical `reset_phrase` puts on a time already gone, which
        // the assertion above walks straight past.
        assert!(
            !refusal.contains("any moment now"),
            "a window that came back is not one coming back: {refusal}"
        );
        // Nor may the sentence after the clause call the figure absent.
        assert!(
            !refusal.contains("No cached figure says when any"),
            "the cache said exactly when it came back: {refusal}"
        );
    }

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
    /// this fixture puts the registry in.
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
    use crate::registry::{Quarantine, Strategy};

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
            if !is_a_candidate(&arrangement.registry, leaving) {
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
