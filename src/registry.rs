//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned, and the version moves when the shape does
//! (ADR the-holdings-outlive-a-perch): a registry claiming more than this build
//! understands is refused rather than silently misread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::{Host, HostError};
use crate::lock;
use crate::name::{self, NameKind, UNGROUPED, means_ungrouped, same_name};
use crate::probe::Identity;

/// The version this build writes.
///
/// A registry claiming a higher one is refused rather than silently misread, and
/// the guard is only worth having if this moves whenever the shape does.
pub const CURRENT_VERSION: u32 = 5;

/// A version is a row of name rules, so the table's length is this number: a row
/// joining without this moving, or this moving without a row, fails the build
/// (ADR an-invariant-gets-a-door).
const _: () = assert!(CURRENT_VERSION as usize == crate::name::ROWS.len());

/// One Quota Window's Utilization, as observed at a point in time
/// (ADR a-figure-carries-its-age).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowUtilization {
    /// The window this figure describes, e.g. `5-hour`, `7-day`, `7-day-opus`.
    pub window: String,
    /// How full the window is, 0-100.
    pub used_percent: f64,
    /// When the window next resets, if the observation carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}

/// Cached Utilization for one Account. What every surface renders, and what
/// only a `--refresh` ever goes and fetches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedUtilization {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<WindowUtilization>,
}

/// Why an Account's Credential can no longer be used and cannot be recovered
/// from anything Perch holds (ADR a-broken-account-is-repaired).
///
/// Recorded rather than merely counted: every one of these is terminal, and
/// which one it is implies the repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quarantine {
    /// Anthropic turned the refresh token down — retired, revoked, or belonging
    /// to a login that has been ended elsewhere.
    RenewalRejected,
    /// Anthropic Rotated the refresh token and the new one could not be stored,
    /// so the old one is retired and the new one is gone.
    RotationLost,
    /// The Credential carries no refresh token, so the access token that ran out
    /// was the last thing it could offer.
    NoRefreshToken,
    /// Neither of the Profile's Credential Stores holds anything at all.
    NoCredential,
}

impl Quarantine {
    /// What happened, as the middle of a sentence about the Account: "{named}
    /// is Quarantined: {because}."
    pub fn because(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "Anthropic would not renew its Credential",
            Quarantine::RotationLost => {
                "Anthropic Rotated its refresh token and the new one could not be stored, \
                 so the one Perch holds is retired"
            }
            Quarantine::NoRefreshToken => {
                "the Credential Perch holds carries no refresh token, so it cannot be renewed"
            }
            Quarantine::NoCredential => "Perch holds no Credential for it",
        }
    }

    /// Whether getting here cost a request to Anthropic, which is what the
    /// Watcher's Back-off paces. A property of what happened rather than of why
    /// the Renewal was wanted: both reasons reach both halves of this.
    pub fn reached_anthropic(&self) -> bool {
        match self {
            Quarantine::RenewalRejected | Quarantine::RotationLost => true,
            Quarantine::NoRefreshToken | Quarantine::NoCredential => false,
        }
    }

    /// The reason as a script reads it, which is the spelling the registry
    /// records.
    pub fn as_str(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "renewal-rejected",
            Quarantine::RotationLost => "rotation-lost",
            Quarantine::NoRefreshToken => "no-refresh-token",
            Quarantine::NoCredential => "no-credential",
        }
    }

    /// The whole of what is said about a Quarantined Account where nothing around
    /// it says any of it: which Account, what happened, and how to end it.
    ///
    /// `detail` is whatever the failure underneath said. The reason is what
    /// happened; the detail is how.
    pub fn said_of(&self, named: &str, target: &str, detail: Option<&str>) -> String {
        let how = match detail {
            Some(detail) => format!(" ({detail})"),
            None => String::new(),
        };
        format!(
            "{named} is Quarantined: {}{how}. {}",
            self.because(),
            how_to_repair(target)
        )
    }

    /// What is true of this Account and no other: which one it is and what
    /// happened to it, without the repair.
    ///
    /// For a surface that has already said the state and says the repair once
    /// beneath all of them (ADR perch-says-what-it-did).
    pub fn shown_of(&self, named: &str) -> String {
        format!("{named}: {}.", self.because())
    }

    /// The refusal a command raises rather than acting on a Quarantined Account,
    /// as opposed to [`said_of`](Quarantine::said_of), which is how one is
    /// *shown*.
    ///
    /// `consequence` is the caller's, and is the only part that differs.
    pub fn refusal(self, named: &str, target: &str, consequence: &str) -> PerchError {
        PerchError::Quarantined {
            why: self,
            said: format!(
                "{named} is Quarantined: {}.\n{consequence} {}",
                self.because(),
                how_to_repair(target),
            ),
        }
    }

    /// The same as a script reads it. Absent reads as false wherever a script
    /// asks whether it is set, so one already branching on the fact carries why.
    ///
    /// `said` rather than `detail`: the registry records a `Quarantine` and not
    /// the failure behind one, so there is no "how" here to carry.
    pub fn document(quarantine: Option<Quarantine>) -> serde_json::Value {
        match quarantine {
            Some(why) => {
                serde_json::json!({"reason": why.as_str(), "said": why.because()})
            }
            None => serde_json::Value::Null,
        }
    }
}

/// How a Quarantine is asked about and how it is put right, said the same way
/// wherever an Account is shown as broken.
pub fn how_to_repair(target: &str) -> String {
    format!(
        "`perch relogin {target}` logs it in again in place, keeping its Alias, \
         its Group and whether Cycling may choose it."
    )
}

/// The same repair, for however many Accounts are in that state — said once,
/// because it is the same repair.
///
/// Named where there is exactly one to name: "logs *it* in again" over a set
/// tells somebody holding three broken Accounts to repair the first.
pub fn how_to_repair_them(targets: &[impl AsRef<str>]) -> Option<String> {
    match targets {
        [] => None,
        [one] => Some(how_to_repair(one.as_ref())),
        _ => Some(
            "`perch relogin <target>` logs one in again in place, keeping its \
             Alias, its Group and whether Cycling may choose it."
                .to_string(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    /// Who this Account is. Its email address is also its identifier.
    pub identity: Identity,
    /// The subscription the Credential reports — `pro`, `max`, and so on. It
    /// comes from the Credential rather than the Identity, which is why it is
    /// not part of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Whether the Account has been taken out of Cycling.
    ///
    /// Said only when true: the positive state has no name to write down — it is
    /// the absence of this one (ADR a-command-names-its-noun).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Why this Account's Credential can no longer be used, when it cannot.
    ///
    /// Left out of the file entirely rather than written as a null: the registry
    /// is something a person may open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<CachedUtilization>,
}

/// How Cycling orders the Accounts in a Group.
///
/// Both readings measure headroom the same way — the worst Quota Window an
/// Account has (ADR headroom-is-the-worst-window) — so neither is a way round an
/// exhausted Account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    /// Prefer the Account with the most room left.
    #[default]
    MostHeadroom,
    /// Prefer the Account whose window resets soonest, so perishable quota is
    /// spent rather than wasted.
    SoonestReset,
}

impl Strategy {
    /// Every Strategy there is, so the vocabulary `perch config` accepts and
    /// names in a refusal cannot fall behind the ones Cycling implements.
    pub const ALL: [Strategy; 2] = [Strategy::MostHeadroom, Strategy::SoonestReset];

    pub fn as_str(&self) -> &'static str {
        match self {
            Strategy::MostHeadroom => "most-headroom",
            Strategy::SoonestReset => "soonest-reset",
        }
    }
}

/// The watcher's threshold when nobody has said otherwise: high enough that an
/// unattended Switch means the Account really is running out.
pub const DEFAULT_WATCHER_THRESHOLD_PERCENT: u8 = 80;

/// The watcher's margin when nobody has said otherwise, in percentage points
/// under the threshold. Wide enough that a candidate barely emptier than the
/// Account being left is refused, which is what stops two Accounts walking
/// upward together.
pub const DEFAULT_WATCHER_MARGIN_PERCENT: u8 = 10;

/// The least a margin may be. Nothing is out of range rather than permissive: an
/// Account is left on `>=` the threshold and a candidate set aside on `>` the
/// ceiling, so at a margin of nothing one Account is both full enough to leave
/// and clear enough to arrive at.
pub const MIN_MARGIN_PERCENT: u8 = 1;

/// The most a Setting said as a share of something can be.
///
/// Not the bound on a Utilization figure, which `validate` checks separately:
/// that is what a *reading* may be and this is what a Setting may be *set to*.
/// The same number today, and two facts always.
pub const MAX_PERCENTAGE: u8 = 100;

/// What a percentage accepts, said once so a mistyped `perch config set` and a
/// hand-edited registry are refused in the same words. Built from the bound, so
/// the sentence and the number cannot disagree.
pub fn a_percentage() -> String {
    format!("a whole number between 0 and {MAX_PERCENTAGE}")
}

/// The same for a margin, whose floor is not zero. Built from the bounds for
/// [`a_percentage`]'s reason.
pub fn a_margin() -> String {
    format!("a whole number between {MIN_MARGIN_PERCENT} and {MAX_PERCENTAGE}")
}

/// Every Setting there is, all of them set: what one Scope holds
/// (ADR a-setting-names-its-scope).
///
/// There is nothing above a Scope for a value to fall back to, so a Setting
/// nobody has said anything about is the compiled-in default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub strategy: Strategy,
    /// Whether the watcher may Switch within this Scope unattended. Off unless
    /// the user says otherwise: nothing changes underneath somebody because they
    /// did not say it could (ADR a-group-is-a-declaration). Said about the Scope
    /// it grants and nowhere else.
    pub watcher_may_act: bool,
    /// The Utilization the watcher would act at, as a percentage.
    pub watcher_threshold_percent: u8,
    /// How far under the threshold a candidate has to sit before moving to it is
    /// worth doing, in percentage points. Separate from the threshold because
    /// the two are not one preference: how full is too full to stay on, and how
    /// empty is empty enough to move to, are answered by different appetites.
    pub watcher_margin_percent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            strategy: Strategy::default(),
            watcher_may_act: false,
            watcher_threshold_percent: DEFAULT_WATCHER_THRESHOLD_PERCENT,
            watcher_margin_percent: DEFAULT_WATCHER_MARGIN_PERCENT,
        }
    }
}

impl Settings {
    /// Refuses configuration that cannot mean what it says. Serde refuses a
    /// strategy Perch does not implement; what is left is the range.
    ///
    /// The refusal names the numbers that would have been accepted, because the
    /// script that mistyped one is the reader.
    pub fn validate(&self, scope: &Scope) -> Result<()> {
        if self.watcher_threshold_percent > MAX_PERCENTAGE {
            return Err(out_of_range(
                scope,
                "watcher-threshold-percent",
                self.watcher_threshold_percent,
                &a_percentage(),
            ));
        }
        if !(MIN_MARGIN_PERCENT..=MAX_PERCENTAGE).contains(&self.watcher_margin_percent) {
            return Err(out_of_range(
                scope,
                "watcher-margin-percent",
                self.watcher_margin_percent,
                &a_margin(),
            ));
        }
        Ok(())
    }
}

/// A number a setting cannot hold, refused with the ones it can.
fn out_of_range(
    scope: &Scope,
    key: &str,
    held: impl std::fmt::Display,
    accepted: &str,
) -> PerchError {
    PerchError::Invalid(format!(
        "{} has a `{key}` of {held}, and it takes {accepted}.",
        scope.described(),
    ))
}

/// The set of Accounts a Setting governs, and the set a Cycle may look within:
/// one Group, or the Accounts in no Group taken together.
///
/// One type for both, because they are one idea: a Cycle never leaves the Scope
/// it started in, and a Setting is said about exactly the Scope it governs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The Accounts in no Group, taken as one Scope. Not a Group and never one:
    /// a Group is a declaration somebody made, and this is the absence of one.
    Ungrouped,
    /// One Group, named as it was declared.
    Group(String),
}

impl Scope {
    /// The word that addresses this Scope on a command line, and the word it is
    /// recorded under wherever something is kept per Scope.
    ///
    /// A Group cannot be called `ungrouped` ([`name::validate`]), so the two can
    /// never collide.
    pub fn word(&self) -> &str {
        match self {
            Scope::Ungrouped => UNGROUPED,
            Scope::Group(name) => name,
        }
    }

    /// The Accounts this Scope holds.
    ///
    /// The same set whoever is asking. A second idea of which Accounts those are
    /// is how the figure on screen comes to describe a different set from the
    /// one that gets chosen.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Ungrouped => registry.ungrouped_accounts(),
            Scope::Group(name) => registry.accounts_in(name),
        }
    }

    /// The Scope as an adverbial phrase: "a Cycle {} prefers…". Said once here,
    /// because three spellings of "among the Accounts in no Group" is how two of
    /// them come to name the same set differently.
    pub fn within(&self) -> String {
        match self {
            Scope::Ungrouped => "among the Accounts in no Group".to_string(),
            Scope::Group(name) => format!("within Group `{name}`"),
        }
    }

    /// The Scope as the subject of a sentence about what it holds.
    pub fn described(&self) -> String {
        match self {
            Scope::Ungrouped => "The Ungrouped Scope".to_string(),
            Scope::Group(name) => format!("Group `{name}`"),
        }
    }

    /// The Scope in the middle of a sentence *about the Scope itself*: "a Setting
    /// {} carries", "`strategy` on {} is now …".
    ///
    /// [`Scope::described`] with the capital taken off: that one reads as a
    /// subject and this one does not. A Group is unaffected, being a name.
    pub fn mentioned(&self) -> String {
        match self {
            Scope::Ungrouped => "the Ungrouped Scope".to_string(),
            Scope::Group(_) => self.described(),
        }
    }

    /// The Scope as the middle of a sentence about the Accounts in it: "every
    /// Account in {}".
    ///
    /// Not [`Scope::described`], which is ungrammatical the moment "in" is said
    /// before it.
    pub fn place(&self) -> String {
        match self {
            Scope::Ungrouped => "no Group".to_string(),
            Scope::Group(_) => self.described(),
        }
    }
}

/// The last unasked Switch within a Scope, so the next round can be paced by it
/// (ADR a-watcher-knob-is-arithmetic). Written down rather than kept in memory
/// because a Watcher is a process its own Service restarts, and a Cooldown a
/// restart clears is no Cooldown. Per Scope: a Switch within `work` says nothing
/// about how soon `personal` may move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checked {
    /// When the Switch happened, which is what the cooldown counts from.
    pub switched_at: DateTime<Utc>,
}

/// What the Ungrouped Scope holds: the declaration that those Accounts are
/// interchangeable at all, and the Settings governing how they are Cycled.
///
/// The one Scope whose record is not a bare [`Settings`], because it is the one
/// that has to say it is a Scope at all. A Group **is** that declaration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UngroupedConfig {
    /// Whether the Accounts in no Group have been declared interchangeable —
    /// what a bare `perch switch` and the watcher both need first.
    ///
    /// Off unless the user says otherwise: being ungrouped is the absence of a
    /// declaration, not a weaker form of one.
    pub interchangeable: bool,
    /// The Settings this Scope holds, like every other Scope.
    pub settings: Settings,
}

/// Which Account is active — and, while a Switch is under way, that Perch cannot
/// yet say (ADR a-switch-is-written-down-first).
///
/// One field with three states rather than two, so a registry naming both a
/// settled active Account and a different in-flight one cannot be written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Active {
    /// Perch is on nobody: a machine that has never Switched, one a removal
    /// left with nowhere to land, or one a repair took off the Account it could
    /// not make live.
    #[default]
    Nobody,
    /// One Account is active, nothing is in flight, and the live Credential is
    /// theirs as far as anything Perch wrote is concerned.
    Settled(String),
    /// A **Landing**: a Switch that has been written down and not yet recorded.
    ///
    /// Written after the Capture and before the Credential moves, so a Perch that
    /// finds one knows the live Credential belongs to one of these two Accounts.
    /// Every Switch path settles one before it acts.
    Landing {
        /// The Account being left. `None` where Perch was on nobody, which is a
        /// Switch with no Capture to lose.
        leaving: Option<String>,
        /// The Account being switched to.
        arriving: String,
    },
}

impl Active {
    /// The Account to treat as active, which during a Landing is the one being
    /// left.
    ///
    /// Nothing has been recorded as having moved, and every path that could
    /// *lose* something by believing it settles the Landing first.
    pub fn whose(&self) -> Option<&str> {
        match self {
            Active::Nobody => None,
            Active::Settled(email) => Some(email),
            Active::Landing { leaving, .. } => leaving.as_deref(),
        }
    }

    /// Whether this address is the one a reader would call active, which during
    /// a Landing is the Account being *left*. Ungated, for a renderer that shows
    /// the Landing beside the answer rather than declining to answer.
    pub fn is_active(&self, email: &str) -> bool {
        self.whose().is_some_and(|active| same_name(active, email))
    }

    /// Whether this address is named here in any role, case-folded like every
    /// other way the registry is asked about a name.
    pub fn names(&self, email: &str) -> bool {
        match self {
            Active::Nobody => false,
            Active::Settled(held) => same_name(held, email),
            Active::Landing { leaving, arriving } => {
                same_name(arriving, email)
                    || leaving
                        .as_deref()
                        .is_some_and(|leaving| same_name(leaving, email))
            }
        }
    }

    /// The Switch that was in flight and never recorded, said out loud.
    ///
    /// Never changes an exit code: the next Switch resolves this by itself. Here
    /// rather than in a command, because `perch status` and `perch list` both
    /// say it.
    pub fn a_switch_in_flight(&self) -> Option<String> {
        let Active::Landing { leaving, arriving } = self else {
            return None;
        };
        let was_on = match leaving {
            Some(leaving) => format!("Perch was on {leaving}"),
            None => "Perch was on no Account".to_string(),
        };
        Some(format!(
            "A Switch was in flight and was not recorded — {was_on} and was \
             switching to {arriving}, so which Credential is live is not \
             settled. The next Switch resolves it, and says so if it cannot."
        ))
    }

    /// The Switch that was in flight and never recorded, as a script reads it,
    /// and `null` on every machine that is not mid-Landing.
    ///
    /// Beside whichever key already says who is active rather than folded into
    /// it: *which Account* and *whether Perch can say* are different questions.
    pub fn document(&self) -> serde_json::Value {
        match self {
            Active::Landing { leaving, arriving } => {
                serde_json::json!({"leaving": leaving, "arriving": arriving})
            }
            Active::Nobody | Active::Settled(_) => serde_json::Value::Null,
        }
    }

    /// Being on the Account a Switch was leaving, or on nobody where it was
    /// leaving nobody. What a Landing comes back to when nothing moved.
    pub fn settled_on(leaving: Option<String>) -> Active {
        match leaving {
            Some(leaving) => Active::Settled(leaving),
            None => Active::Nobody,
        }
    }

    /// Absent from the file rather than written as a word, which is what the
    /// registry of a machine that has never Switched has always looked like.
    fn is_nobody(&self) -> bool {
        matches!(self, Active::Nobody)
    }
}

/// No Landing is in flight, so the registry a reader is about to ask tells the
/// truth about who is active. A witness (ADR an-ordering-is-a-type), and the
/// negative of a Landing, so nothing is promoted. Two things earn it:
/// [`Registry::settle`] records what a walk settled a Landing on, and
/// [`nothing_in_flight`] finds there was none to settle.
pub struct Settled(());

/// The witness for a reader that has a Landing to *check* rather than one to
/// settle: a `perch watcher run` says what it is about to watch off a registry it
/// has not locked, and a Landing in flight is the state where it has nothing to
/// say yet, because [`Active::whose`] answers with the Account being *left*.
/// `None` is the whole of what it can answer about a Landing.
pub fn nothing_in_flight(registry: &Registry) -> Option<Settled> {
    match registry.active() {
        Active::Landing { .. } => None,
        Active::Nobody | Active::Settled(_) => Some(Settled(())),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    /// The active Account, or the Switch under way when this was last written.
    ///
    /// Private, and the only field here that is: assigning it directly is a
    /// Switch recorded without having been written down first. Reached through
    /// `begin_landing`, `settle` and `abandon_landing`.
    #[serde(default, skip_serializing_if = "Active::is_nobody")]
    active: Active,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Alias to Account email.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// The Groups the user has declared, with the Settings each one holds. A
    /// Group exists here even when it holds no Accounts: it is a statement
    /// somebody made, not a summary of where the Accounts happen to be.
    #[serde(default)]
    pub groups: BTreeMap<String, Settings>,
    /// What the Accounts in no Group hold, taken as one Scope. Not a Group and
    /// never one; here rather than under a reserved key in `groups` so that
    /// nothing can walk it as one.
    #[serde(default)]
    pub ungrouped: UngroupedConfig,
    /// The last unasked Switch in each Scope. Written by `perch watcher run` and
    /// `perch watcher check`, and absent from the file until one of them
    /// Switches. Spelled `checks` because a Watcher that only ran scheduled
    /// wrote it first, and a key is registry shape: renaming it is a migration
    /// rather than a rename.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, Checked>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            active: Active::Nobody,
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
            groups: BTreeMap::new(),
            ungrouped: UngroupedConfig::default(),
            checks: BTreeMap::new(),
        }
    }
}

impl Account {
    pub fn email(&self) -> &str {
        &self.identity.email
    }

    /// The Profile this Account's Credential lives in.
    ///
    /// Derived from the email address the registry already keys on rather than
    /// recorded beside it (ADR claude-code-chooses-the-store): two statements of
    /// one fact can disagree.
    pub fn profile_dir(&self, host: &dyn Host) -> Result<PathBuf> {
        holdings::profile_dir_for(host, self.email())
    }

    /// Where the installed Claude Code would keep this Account's configuration
    /// if it were pointed at its Profile.
    pub fn store(&self, host: &dyn Host) -> Result<crate::probe::Store> {
        crate::probe::store_for_profile(host, &self.profile_dir(host)?)
    }

    /// Whether this Account is Quarantined, for the places that only need the
    /// fact and not the reason.
    pub fn quarantined(&self) -> bool {
        self.quarantine.is_some()
    }

    /// The cached Utilization, if any figure has ever been observed. An empty
    /// set of windows is not an observation.
    pub fn observed_utilization(&self) -> Option<&CachedUtilization> {
        self.utilization
            .as_ref()
            .filter(|cached| !cached.windows.is_empty())
    }
}

impl Registry {
    /// The Account an address names, folded as a Profile is derived: `CAFÉ@…`
    /// and `café@…` share one Profile and `perch add` refuses the second, so
    /// asking in ASCII here would disagree with the directory on disk.
    pub fn account(&self, email: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| same_name(account.email(), email))
    }

    pub fn active_account(&self, _settled: &Settled) -> Option<&Account> {
        self.active.whose().and_then(|email| self.account(email))
    }

    /// Which Account is active, or the Switch that was in flight when this was
    /// last written.
    ///
    /// Reading is nobody's to get wrong. Writing is three named transitions.
    pub fn active(&self) -> &Active {
        &self.active
    }

    /// Writes down that a Switch is about to move the live Credential, naming
    /// both Accounts it could then belong to.
    ///
    /// Hands back what it replaced, because the Landing has to reach disk before
    /// it means anything — see [`Registry::abandon_landing`].
    pub fn begin_landing(&mut self, leaving: Option<String>, arriving: &str) -> Active {
        std::mem::replace(
            &mut self.active,
            Active::Landing {
                leaving,
                arriving: arriving.to_string(),
            },
        )
    }

    /// Puts back what [`Registry::begin_landing`] replaced, where the save that
    /// would have carried it did not happen.
    ///
    /// Not [`Registry::settle`]: this Landing never existed anywhere but in
    /// memory, and nothing has moved.
    pub fn abandon_landing(&mut self, before: Active) {
        self.active = before;
    }

    /// Records who is active now that a Switch is over. `None` is a machine on
    /// nobody, and what is passed is whose Credential the machine is holding.
    ///
    /// An address rather than an [`Active`], which is what makes "settled" true
    /// of what it leaves: handed the enum it would accept a Landing.
    pub fn settle(&mut self, on: Option<String>) -> Settled {
        self.active = Active::settled_on(on);
        Settled(())
    }

    /// Whether this address is the one the registry records as active.
    ///
    /// Case-folded, like every other way the registry is asked about a name:
    /// `upsert` stores the incoming spelling, so an Identity re-read with
    /// different capitalization would leave an exact `==` answering wrongly.
    pub fn is_active(&self, _settled: &Settled, email: &str) -> bool {
        self.active.is_active(email)
    }

    /// Every Group name in use. A Group an Account claims is always declared
    /// too — [`load`] sees to that — so this is the declared set.
    pub fn group_names(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(String::as_str)
    }

    /// The Settings of the Group declared under a name, whatever it was
    /// capitalized as.
    ///
    /// Through [`declared_group`](Self::declared_group), because that is how
    /// every other question about a Group name is answered here.
    pub fn group(&self, name: &str) -> Option<&Settings> {
        self.groups.get(self.declared_group(name)?)
    }

    /// Every Scope a Setting can be said at, in the order they are offered: the
    /// Ungrouped Accounts, then each Group as it was declared.
    pub fn scopes(&self) -> Vec<Scope> {
        let mut every = vec![Scope::Ungrouped];
        every.extend(
            self.group_names()
                .map(|name| Scope::Group(name.to_string())),
        );
        every
    }

    /// The Settings a Scope holds.
    ///
    /// A lookup rather than a cascade: there is no chain. A Group Perch does not
    /// hold is not a Scope, and answers with the compiled-in defaults.
    pub fn settings(&self, scope: &Scope) -> Settings {
        match scope {
            Scope::Ungrouped => self.ungrouped.settings,
            Scope::Group(name) => self.group(name).copied().unwrap_or_default(),
        }
    }

    /// The same, to write through. A Group Perch does not hold has nothing to
    /// write to: declaring one is `declare_group`'s.
    pub fn settings_mut(&mut self, scope: &Scope) -> Option<&mut Settings> {
        match scope {
            Scope::Ungrouped => Some(&mut self.ungrouped.settings),
            Scope::Group(name) => {
                let declared = self.declared_group(name)?.to_string();
                self.groups.get_mut(&declared)
            }
        }
    }

    /// The Scope an Account's Settings come from: its Group, or the Ungrouped
    /// Accounts. One place, because there is nothing to the rule but this match,
    /// which is exactly what gets written out again at a call site.
    pub fn scope_of(&self, account: &Account) -> Scope {
        match &account.group {
            Some(name) => Scope::Group(name.clone()),
            None => Scope::Ungrouped,
        }
    }

    /// The Accounts in a Group, in the order they were added.
    pub fn accounts_in(&self, group: &str) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| {
                account
                    .group
                    .as_deref()
                    .is_some_and(|held| same_name(held, group))
            })
            .collect()
    }

    /// The Accounts that are in no Group — the ordinary starting state, not an
    /// error.
    pub fn ungrouped_accounts(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.is_none())
            .collect()
    }

    /// The Group declared under a name, whatever it was capitalized as. Two
    /// names that differ only in case are one name here, so this is how a Group
    /// typed in passing is matched to the one that exists.
    pub fn declared_group(&self, name: &str) -> Option<&str> {
        self.groups
            .keys()
            .find(|declared| same_name(declared, name))
            .map(String::as_str)
    }

    /// Declares a Group, refusing a name that is not usable or already means
    /// something else.
    pub fn declare_group(&mut self, name: &str) -> Result<()> {
        self.refuse_a_name_nothing_may_answer_to(NameKind::Group, name, None)?;
        // At the compiled-in defaults, which is what every Setting means until
        // somebody says otherwise about this Group.
        self.groups.insert(name.to_string(), Settings::default());
        Ok(())
    }

    /// Refuses a name nothing may answer to: one that is not usable, one another
    /// name of the same kind holds, or one the other namespace half holds.
    ///
    /// Shape before collision. `instead_of` is the name this replaces, and
    /// renaming itself waives only the same-kind collision.
    pub fn refuse_a_name_nothing_may_answer_to(
        &self,
        kind: NameKind,
        name: &str,
        instead_of: Option<&str>,
    ) -> Result<()> {
        name::validate(kind, name)?;

        let renaming_itself = instead_of.is_some_and(|held| same_name(held, name));
        if !renaming_itself {
            // The same-kind collision. Asked here for a Group and inside
            // `refuse_taken_names` for an Alias, which is asymmetric: given an
            // Alias it checks both halves, and given a Group only the Alias one.
            if let (NameKind::Group, Some(declared)) = (kind, self.declared_group(name)) {
                return Err(PerchError::Conflict(format!(
                    "There is already a Group called `{declared}`."
                )));
            }
        }

        match kind {
            NameKind::Group => self.refuse_taken_names(None, Some(name)),
            NameKind::Alias => match renaming_itself {
                // An Account keeping its own Alias under another capitalization
                // cannot collide with the Alias it is giving up.
                true => self.refuse_a_group_of_this_name(name),
                false => self.refuse_taken_names(Some(name), None),
            },
        }
    }

    /// The half of [`Self::refuse_taken_names`] that still applies to an Alias
    /// renaming itself: the other side of the shared namespace.
    fn refuse_a_group_of_this_name(&self, name: &str) -> Result<()> {
        match self.declared_group(name) {
            Some(declared) => Err(PerchError::Conflict(format!(
                "`{declared}` is already a Group name, and a name cannot be both."
            ))),
            None => Ok(()),
        }
    }

    /// Renames a Group, keeping everything it carries.
    ///
    /// `held` is the name as this registry holds it. Three things move with it:
    /// the Settings, the Accounts that claim it, and what the last scheduled
    /// Check left — dropping that would let the watcher Switch again at once.
    pub fn rename_group(&mut self, held: &str, to: &str) -> Result<()> {
        // Resolved before the new name is judged, or
        // `perch group rename nosuchgroup work` exits as a name collision. And
        // through `declared_group`, as every question about a Group name is.
        let Some(declared) = self.declared_group(held).map(str::to_string) else {
            return Err(PerchError::NotFound(format!(
                "no Group is called `{held}`."
            )));
        };
        self.refuse_a_name_nothing_may_answer_to(NameKind::Group, to, Some(held))?;

        // `declared` is one of this map's own keys, `declared_group` having
        // just read it out — so the refusal above is the only one there is.
        let settings = self.groups.remove(&declared).unwrap_or_default();
        self.groups.insert(to.to_string(), settings);
        for account in &mut self.accounts {
            if account
                .group
                .as_deref()
                .is_some_and(|held| same_name(held, &declared))
            {
                account.group = Some(to.to_string());
            }
        }
        if let Some(checked) = self.checks.remove(&declared) {
            self.checks.insert(to.to_string(), checked);
        }
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name a
    /// Group in passing rather than to create one — `perch add --group`.
    ///
    /// Returns the spelling the Group was declared under: naming `Work` in
    /// passing joins `work`.
    pub fn ensure_group(&mut self, name: &str) -> Result<String> {
        if let Some(declared) = self.declared_group(name) {
            return Ok(declared.to_string());
        }
        self.declare_group(name)?;
        Ok(name.to_string())
    }

    /// Forgets a Group. The caller establishes nothing is left in it: dropping
    /// the Group is not a way to empty it.
    ///
    /// What a Watcher left goes with it, or a Group declared under the same name
    /// later would inherit a cooldown from a Group it never was.
    pub fn forget_group(&mut self, name: &str) {
        let Some(declared) = self.declared_group(name).map(str::to_string) else {
            return;
        };
        self.groups.remove(&declared);
        self.checks.remove(&declared);
    }

    /// The last unasked Switch within a Scope, if one has happened there.
    pub fn checked(&self, group: &str) -> Option<&Checked> {
        self.checks
            .iter()
            .find(|(declared, _)| same_name(declared, group))
            .map(|(_, checked)| checked)
    }

    /// Records an unasked Switch, for the next round to be paced by.
    ///
    /// Filed under the spelling the Group was declared under, so a round naming
    /// it in another case does not leave a second record pacing nothing.
    pub fn record_switch(&mut self, group: &str, at: DateTime<Utc>) {
        let under = self.declared_group(group).unwrap_or(group).to_string();
        self.checks.insert(under, Checked { switched_at: at });
    }

    pub fn account_mut(&mut self, email: &str) -> Option<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| same_name(account.email(), email))
    }

    /// The same, where the Account has to be there.
    ///
    /// The state cannot happen — [`validate`] refuses every registry that could
    /// produce it, on the way in and, since `save` validates too, on the way out
    /// — so a refusal naming what could not be found beats a panic.
    pub fn held(&self, email: &str) -> Result<&Account> {
        self.account(email).ok_or_else(|| no_such_account(email))
    }

    /// [`Self::held`], for the callers that go on to change what they find.
    pub fn held_mut(&mut self, email: &str) -> Result<&mut Account> {
        match self.account_mut(email) {
            Some(account) => Ok(account),
            None => Err(no_such_account(email)),
        }
    }

    /// The Alias an Account answers to, if it has been given one.
    pub fn alias_of(&self, email: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, target)| same_name(target, email))
            .map(|(alias, _)| alias.as_str())
    }

    /// Every Account's Alias at once, for a caller asking about more than one.
    ///
    /// [`Registry::alias_of`] scans, the map being keyed by Alias rather than by
    /// Account, so a listing asking it per row is a scan per row.
    pub fn aliases_by_account(&self) -> AliasOf<'_> {
        let mut held: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for (alias, email) in &self.aliases {
            // First wins, as `alias_of`'s scan does: `validate` refuses two
            // Aliases for one Account, and `aliases` walks in sorted order.
            held.entry(name::folded(email)).or_insert(alias.as_str());
        }
        AliasOf(held)
    }

    /// An Account as the user names it: by its Alias when it has one, so a
    /// message about it reads the way they would say it.
    pub fn named_for_the_user(&self, email: &str) -> String {
        match self.alias_of(email) {
            Some(alias) => format!("{email} (as `{alias}`)"),
            None => email.to_string(),
        }
    }

    /// The Alias held under a name, whatever it was capitalized as, and the
    /// Account it reaches.
    pub fn declared_alias(&self, name: &str) -> Option<(&str, &str)> {
        self.aliases
            .iter()
            .find(|(alias, _)| same_name(alias, name))
            .map(|(alias, email)| (alias.as_str(), email.as_str()))
    }

    /// Refuses an Alias and a Group name that would not both be free.
    ///
    /// The pair is checked against each other as well as against what is held: a
    /// command setting both at once could otherwise plant the collision it is
    /// meant to prevent. Two names differing only in case are one name.
    pub fn refuse_taken_names(&self, alias: Option<&str>, group: Option<&str>) -> Result<()> {
        if let (Some(alias), Some(group)) = (alias, group)
            && same_name(alias, group)
        {
            return Err(PerchError::Conflict(format!(
                "`{alias}` cannot be both an Alias and a Group name."
            )));
        }

        if let Some(alias) = alias {
            if let Some((held, target)) = self.declared_alias(alias) {
                return Err(PerchError::Conflict(format!(
                    "`{held}` already names {target}. Free it with `perch alias {held} --unset` first."
                )));
            }
            // The same lookup and sentence `refuse_a_group_of_this_name` is,
            // which an Alias renaming itself needs without the one above it.
            self.refuse_a_group_of_this_name(alias)?;
        }

        if let Some(group) = group
            && let Some((held, target)) = self.declared_alias(group)
        {
            return Err(PerchError::Conflict(format!(
                "`{held}` is already an Alias for {target}, and a name cannot be both."
            )));
        }

        Ok(())
    }

    /// Names an Account, refusing a name that is not usable or already means
    /// something else. Hands back the Alias it gave up, where it had one.
    ///
    /// An Account answers to one Alias, so naming one that already had a name
    /// replaces it rather than adding to it.
    pub fn name_account(&mut self, alias: &str, email: &str) -> Result<Option<String>> {
        let previous = self.alias_of(email).map(str::to_string);
        self.refuse_a_name_nothing_may_answer_to(NameKind::Alias, alias, previous.as_deref())?;

        // The address as the registry *holds* it, not as it was typed: the
        // lookup above folds case, and storing the typed spelling would point
        // the Alias at a string no `accounts` entry has.
        let held = self
            .account(email)
            .map_or_else(|| email.to_string(), |account| account.email().to_string());
        self.aliases.retain(|_, named| !same_name(named, email));
        self.aliases.insert(alias.to_string(), held);
        Ok(previous)
    }

    /// Frees a name, returning the name as it was held and the Account it used
    /// to reach.
    pub fn unset_alias(&mut self, alias: &str) -> Option<(String, String)> {
        let held = self.declared_alias(alias)?.0.to_string();
        let email = self.aliases.remove(&held)?;
        Some((held, email))
    }

    /// Records that an Account's Credential can no longer be used, and says
    /// whether that is news.
    ///
    /// The first reason stands: a Quarantined Account asked a second question
    /// fails a second way, and the reason worth keeping is how it broke.
    pub fn quarantine(&mut self, email: &str, why: Quarantine) -> bool {
        match self.account_mut(email) {
            Some(account) if account.quarantine.is_none() => {
                account.quarantine = Some(why);
                true
            }
            _ => false,
        }
    }

    /// Returns an Account to the pool of ones that work, reporting what it was
    /// Quarantined for. Only a login can do this: nothing else produces a
    /// Credential to replace the one that stopped working.
    pub fn release(&mut self, email: &str) -> Option<Quarantine> {
        self.account_mut(email)?.quarantine.take()
    }

    /// Forgets an Account: the entry, the Alias, and its place as the active one.
    ///
    /// Its Group is left declared, and the Credential is not this to delete —
    /// what a Profile holds is the caller's to take away first, while the Account
    /// can still be named.
    pub fn forget(&mut self, email: &str) {
        self.accounts
            .retain(|account| !same_name(account.email(), email));
        self.aliases.retain(|_, named| !same_name(named, email));
        // Either half of a Landing, and it comes back to whichever half is
        // still held: a Landing naming an Account Perch no longer holds is a
        // dangling pointer `load` refuses. Through `settle`, like every writer.
        if self.active.names(email) {
            let comes_back_to = self
                .active
                .whose()
                .filter(|whose| !same_name(whose, email))
                .map(str::to_string);
            self.settle(comes_back_to);
        }
    }

    pub fn upsert(&mut self, account: Account) {
        match self
            .accounts
            .iter_mut()
            .find(|existing| same_name(existing.email(), account.email()))
        {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }
}

/// [`Registry::alias_of`] answered for every Account rather than for one.
pub struct AliasOf<'a>(std::collections::HashMap<String, &'a str>);

impl<'a> AliasOf<'a> {
    pub fn account(&self, email: &str) -> Option<&'a str> {
        self.0.get(name::folded(email).as_str()).copied()
    }
}

/// Which Profiles more than one Account derives, settled in one pass.
///
/// `is_a_candidate` asks this of every Account and is asked of every one, so
/// answering it by [`sharing_a_profile_with`]'s scan cost n² for one fact.
pub struct Sharers(std::collections::HashSet<String>);

impl Sharers {
    pub fn across(registry: &Registry) -> Sharers {
        // Counting is sound because `validate` refuses two Accounts under one
        // folded address, so nobody is counted as sharing with themselves.
        let mut once: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(registry.accounts.len());
        let mut twice = std::collections::HashSet::new();
        let mut slugged = String::new();
        for account in &registry.accounts {
            holdings::slug_into(&mut slugged, account.email());
            if !once.insert(slugged.clone()) {
                twice.insert(slugged.clone());
            }
        }
        Sharers(twice)
    }

    /// Whether this Account's Profile is one another Account derives too.
    pub fn hold(&self, email: &str) -> bool {
        self.0.contains(holdings::slug(email).as_str())
    }
}

/// The other Account a Profile belongs to as well, where there is one.
///
/// Three commands ask it — a Switch, a Renewal and a Remove — and each spelled
/// its own scan, two of them comparing addresses by bytes where the third folded
/// case.
pub fn sharing_a_profile_with<'a>(
    registry: &'a Registry,
    account: &Account,
) -> Option<&'a Account> {
    // Slugged once rather than once per comparison, and the other side into a
    // buffer this scan keeps: `is_a_candidate` asks this of every Account and
    // is itself asked of every one, so an allocation here is paid n² times.
    let mine = holdings::slug(account.email());
    let mut theirs = String::with_capacity(mine.len());
    registry.accounts.iter().find(|held| {
        !same_name(held.email(), account.email()) && {
            holdings::slug_into(&mut theirs, held.email());
            theirs == mine
        }
    })
}

/// Reads the registry, or `None` when Perch has never run here.
pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    let path = &holdings::registry_path(host)?;
    let contents = match host.read_file(path) {
        Ok(contents) => contents,
        Err(HostError::NotFound { .. }) => return Ok(None),
        Err(err) => {
            return Err(PerchError::Other(format!(
                "could not read {}: {err}",
                path.display()
            )));
        }
    };

    // The version first, off a shape that is only the version. A newer Perch is
    // exactly the thing that writes a value this build has no variant for, and
    // reading the document first fails on that with serde's own words.
    let claimed = crate::error::claimed_version(&contents);
    match claimed {
        Some(version) if version > u64::from(CURRENT_VERSION) => {
            return Err(crate::error::written_by_a_newer_perch(
                &path.display().to_string(),
                "registry",
                version,
                CURRENT_VERSION,
            ));
        }
        Some(version) if version < u64::from(crate::migration::EARLIEST_VERSION) => {
            return Err(no_perch_wrote(path, Some(version)));
        }
        None if crate::migration::says_no_version(&contents) => {
            return Err(no_perch_wrote(path, None));
        }
        _ => {}
    }

    // In memory here and written back by `migration::bring_forward`, because
    // every path that writes holds the lock before it reads. Decorated as every
    // other refusal here is: a step that names a field names no file otherwise.
    let forwarded = crate::migration::forward_from(&contents, claimed)
        .map_err(|refused| refused.with_note(&the_file_to_edit(path)))?;

    // Strictly, so a key nobody recognizes is a refusal naming it rather than a
    // value that quietly did nothing. Every type here is Perch's own — Claude
    // Code's `.claude.json` is read through `probe`'s lenient shapes instead.
    let registry: Registry = serde_json::from_str(forwarded.as_deref().unwrap_or(&contents))
        .map_err(|err| {
            PerchError::Malformed {
                path: path.display().to_string(),
                detail: err.to_string(),
            }
            .with_note(&the_file_to_edit(path))
        })?;

    let registry =
        readable(registry).map_err(|refusal| refusal.with_note(&the_file_to_edit(path)))?;

    Ok(Some(registry))
}

/// The refusal for a registry claiming a version no Perch has stamped, or none
/// (ADR a-registry-comes-forward).
///
/// Neither names a shape, and a document whose shape is unstated half-parses
/// rather than refusing.
fn no_perch_wrote(path: &Path, claimed: Option<u64>) -> PerchError {
    // Without the path, which `Malformed` has already said.
    let what = match claimed {
        Some(version) => format!(
            "it says it is registry version {version}, and no Perch has written \
             that."
        ),
        None => "it does not say which registry version it is, and every Perch \
                 has written one."
            .to_string(),
    };
    PerchError::Malformed {
        path: path.display().to_string(),
        detail: what,
    }
    .with_note(&format!(
        "The version says which shape the rest of the file is in, so Perch will \
         not guess at it. This build reads versions {} through {CURRENT_VERSION}.\n{}",
        crate::migration::EARLIEST_VERSION,
        the_file_to_edit(path),
    ))
}

/// Where to put right something only a hand edit could have put wrong.
///
/// Kept apart from [`validate`]'s rule because [`save`] is holding a registry
/// nobody hand-edited, and telling somebody to edit a value that is not in the
/// file yet is the one sentence that would make it worse.
pub fn the_file_to_edit(path: &Path) -> String {
    format!(
        "It is in {}, and every Perch command reads that file — including the \
         ones that would set it. Edit the value there.",
        path.display(),
    )
}

/// The refusal for an Account that was named and is not there.
///
/// Unreachable by construction, so it is worded as what it is rather than as
/// something to go and fix.
fn no_such_account(email: &str) -> PerchError {
    PerchError::Other(format!(
        "Perch was asked for {email}, which it does not hold, by something that \
         had already established it did.\n\
         {}\n\
         Nothing was changed.",
        crate::report::this_is_a_bug(),
    ))
}

/// Everything a registry has to be true of before any command acts on it.
///
/// Checked on the way in rather than where each value is read, because the thing
/// that reads them is a loop nobody is watching. Public because an Import writes
/// a registry without reading one, and what it accepts must not differ.
pub fn validate(registry: &Registry) -> Result<()> {
    // Every Scope, and every Scope is all of them: with no layer above, one
    // walk over the Scopes is the whole of the check.
    for scope in registry.scopes() {
        registry.settings(&scope).validate(&scope)?;
    }

    // The Group *names* an Account claims, the declared Groups, and the Aliases
    // with them: a hand-edited registry is exactly where a name nothing would
    // have accepted comes from, in any of the three.
    let claimed = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.as_deref());
    for name in claimed.chain(registry.groups.keys().map(String::as_str)) {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Group, name)?;
    }
    for name in registry.aliases.keys() {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Alias, name)?;
    }

    // What each Alias points *at*, which the loop above does not look at. A
    // dangling one is not a refusal downstream — it is the `expect` in every
    // command that resolves a Target.

    // Keyed rather than scanned: both questions below are asked of every Alias,
    // and two names are one name exactly where `name::folded` agrees.
    let held: std::collections::HashSet<String> = registry
        .accounts
        .iter()
        .map(|account| name::folded(account.email()))
        .collect();
    let mut named: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for (alias, email) in &registry.aliases {
        if !held.contains(name::folded(email).as_str()) {
            return Err(PerchError::Invalid(format!(
                "The registry gives the Alias `{alias}` to {email}, which is not \
                 an Account Perch holds.",
            )));
        }
        // One Account, one Alias. With two, `alias_of` returns whichever the map
        // yields first, so `perch list` shows one while `perch switch` answers to
        // both — the same undecided answer as two names differing only in case.
        if let Some(already) = named.insert(name::folded(email), alias) {
            return Err(PerchError::Invalid(format!(
                "The registry gives {email} both the Alias `{already}` and the \
                 Alias `{alias}`, and an Account answers to one Alias at a time \
                 — so which of them Perch shows it under is not decided by \
                 anything.",
            )));
        }
    }

    // The other pointer into the Accounts, and both ends of a Landing, because
    // resolving one reads the Credential of the other. Holding nothing is a
    // state and not a fault.
    match &registry.active {
        Active::Nobody => {}
        Active::Settled(active) => refuse_a_dangling_pointer(
            registry,
            active,
            &format!("says {active} is the active Account"),
        )?,
        Active::Landing { leaving, arriving } => {
            if let Some(leaving) = leaving {
                refuse_a_dangling_pointer(
                    registry,
                    leaving,
                    &format!("says a Switch away from {leaving} was under way"),
                )?;
            }
            refuse_a_dangling_pointer(
                registry,
                arriving,
                &format!("says a Switch to {arriving} was under way"),
            )?;
        }
    }

    // The third member of the namespace. `name::validate` keeps an Alias and a
    // Group name tellable from an address, no `@` being an identifier
    // character; the mirror rule, that an address looks like one, is this.
    for account in &registry.accounts {
        if !account.email().contains('@') {
            return Err(PerchError::Invalid(format!(
                "The registry holds an Account called `{}`, which is not an \
                 address an Alias or a Group name could be told from — and a \
                 Target that could be either has no single answer.",
                account.email(),
            )));
        }
        // Nothing here about a character a terminal would act on. An address is
        // Claude Code's rather than anybody's choice, so it is refused where it
        // enters and drawn through `Shown` (ADR nothing-drawn-is-obeyed).
    }

    // One entry per Account. `upsert` replaces the matching entry, so two for
    // one address is a hand edit — after which `account` acts on the first,
    // `perch list` renders two rows, and a Cycle counts it twice.
    if let Some((already, again)) = first_collision(registry.accounts.iter().map(Account::email)) {
        return Err(PerchError::Invalid(format!(
            "The registry holds two Accounts spelled `{already}` and `{again}`, \
             which are one Account — so which entry a command reads, and which \
             one it writes, is not decided by anything."
        )));
    }

    // What `checks` is keyed on, the one pointer into the Group namespace with no
    // rule of its own: `record_switch` keeps the name it was handed when it cannot
    // resolve one, and `forget_group` only clears what it can.
    for named in registry.checks.keys() {
        if same_name(named, UNGROUPED) || registry.declared_group(named).is_some() {
            continue;
        }
        return Err(PerchError::Invalid(format!(
            "The registry records a Check against `{named}`, which is neither a \
             Group Perch holds nor the Accounts in no Group — so the Cooldown it \
             carries paces nothing."
        )));
    }

    // `checked` answers with the first match in `BTreeMap` order and
    // `record_switch` writes under the declared spelling, so two keys that fold
    // to one pace the next Check off a record nothing is keeping.
    if let Some((already, name)) = first_collision(registry.checks.keys().map(String::as_str)) {
        return Err(PerchError::Invalid(format!(
            "The registry records a Check against `{already}` and one against \
             `{name}`, which are one Group — so which Cooldown paces the next \
             one is not decided by anything."
        )));
    }

    refuse_two_names_that_differ_only_in_case(NameKind::Group, registry.groups.keys())?;
    refuse_two_names_that_differ_only_in_case(NameKind::Alias, registry.aliases.keys())?;

    // The percentages a Cycle ranks on. A negative figure gives
    // `cycle::headroom_of` over 100% of headroom, so `used_percent >= threshold`
    // never fires; serde_json already refuses a literal too large for an `f64`.
    for account in &registry.accounts {
        for window in account
            .utilization
            .iter()
            .flat_map(|cached| &cached.windows)
        {
            if !(0.0..=100.0).contains(&window.used_percent) {
                return Err(PerchError::Invalid(format!(
                    "The registry says {} is {}% through its {} window, and a \
                     window is between 0 and 100 percent full — so what a Cycle \
                     would rank it on, and what the watcher would compare \
                     against a threshold, is not a figure at all.\n\
                     Deleting the Account's `utilization` lets a `perch status \
                     --refresh` read it again.",
                    account.email(),
                    window.used_percent,
                    window.window,
                )));
            }
        }
    }

    Ok(())
}

/// Refuses one of `active`'s pointers into the Accounts naming somebody Perch
/// does not hold. `said` is what the registry claims about that address, so
/// each of the three pointers a Landing can carry says which one it was.
fn refuse_a_dangling_pointer(registry: &Registry, email: &str, said: &str) -> Result<()> {
    if registry.account(email).is_some() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "The registry {said}, which is not an Account Perch holds."
    )))
}

/// Refuses a pair of names in one half of the namespace that only case tells
/// apart. Both halves, because the namespace is shared and one copy of the rule
/// is how the two cannot come to disagree about it.
fn refuse_two_names_that_differ_only_in_case<'a>(
    kind: NameKind,
    names: impl Iterator<Item = &'a String>,
) -> Result<()> {
    match first_collision(names.map(String::as_str)) {
        None => Ok(()),
        Some((already, name)) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{already}` and `{name}`, which differ only \
             in case — so which one a Target finds is not decided by anything.",
            kind.article(),
        ))),
    }
}

/// The first pair of names in a sequence that [`same_name`] cannot tell apart,
/// earlier one first. Keyed on [`name::folded`], which two names are one name
/// exactly where they agree on; the alternative is asking `same_name` of
/// everything already seen, which is a scan per name.
fn first_collision<'a>(names: impl Iterator<Item = &'a str>) -> Option<(&'a str, &'a str)> {
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for name in names {
        if let Some(already) = seen.insert(name::folded(name), name) {
            return Some((already, name));
        }
    }
    None
}

/// Refuses a name in the registry that nothing would have accepted.
///
/// Named rather than repaired: a value only a hand edit can produce is one only a
/// hand edit can take out. The cross-half collision is asked from the Group side
/// only, because every Group is walked either way.
fn refuse_a_name_nothing_would_have_accepted(
    registry: &Registry,
    kind: NameKind,
    name: &str,
) -> Result<()> {
    // `none` is not a case of its own here: `name::validate` already refuses it
    // for both kinds, in words true of a claim and a declaration alike — and
    // this loop walks *declared* Groups too.
    let refused = name::validate(kind, name)
        .err()
        .map(|refusal| refusal.to_string())
        .or_else(|| {
            (kind == NameKind::Group)
                .then(|| registry.declared_alias(name).map(|(held, _)| held))
                .flatten()
                .map(|alias| {
                    format!(
                        "`{alias}` is already an Alias, and Aliases and Group \
                         names share one namespace"
                    )
                })
        });

    match refused {
        None => Ok(()),
        Some(why) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{name}`, which is not a name Perch would \
             have accepted: {why}.",
            kind.article(),
        ))),
    }
}

/// A registry from outside this Perch, made readable: every claimed Group
/// declared, every `checks` key under its declared spelling, then validated. The
/// pair and never one — `validate` asks `declared_group` about the `checks` key,
/// so validating before normalizing refuses a shape the normalizer repairs. The
/// refusal is undecorated, because a `load` and an Import name different files.
pub fn readable(registry: Registry) -> Result<Registry> {
    let registry = with_every_claimed_group_declared(registry);
    validate(&registry)?;
    Ok(registry)
}

/// Declares any Group an Account claims but nothing declared.
///
/// One nothing declares falls out of `perch list`, which walks the declared
/// Groups and then the Accounts in none (ADR the-listing-owns-the-set). A claim
/// differing only in case joins rather than becoming a second key.
fn with_every_claimed_group_declared(mut registry: Registry) -> Registry {
    let claimed: Vec<String> = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.clone())
        .collect();
    for name in claimed {
        match registry.declared_group(&name) {
            Some(declared) if declared != name => {
                let declared = declared.to_string();
                for account in &mut registry.accounts {
                    if account.group.as_deref().is_some_and(|of| of == name) {
                        account.group = Some(declared.clone());
                    }
                }
            }
            Some(_) => {}
            None => {
                registry.groups.insert(name, Settings::default());
            }
        }
    }
    with_every_check_under_the_declared_spelling(registry)
}

/// The same for what `checks` is keyed on. `checked` and `validate` fold the
/// key and the two mutators remove it exactly, so a key differing only in case
/// outlives its Group and leaves `validate` refusing what `save` just built.
fn with_every_check_under_the_declared_spelling(mut registry: Registry) -> Registry {
    let keyed: Vec<String> = registry.checks.keys().cloned().collect();
    for name in keyed {
        // The Ungrouped Scope has no declaration to be brought to, so it is
        // brought to the constant `record_switch` writes. Without this the key is
        // the only one in the map that can outlive its own spelling.
        let declared = match means_ungrouped(&name) {
            true => Some(UNGROUPED.to_string()),
            false => registry.declared_group(&name).map(str::to_string),
        };
        let Some(declared) = declared else {
            continue;
        };
        if declared == name {
            continue;
        }
        if let Some(checked) = registry.checks.remove(&name) {
            // The later of the two, where both spellings carry a record: byte
            // order would otherwise decide, and the older one winning is a
            // Check free to Switch inside a Cooldown still running.
            let later = match registry.checks.get(&declared) {
                Some(held) if held.switched_at > checked.switched_at => held.clone(),
                _ => checked,
            };
            registry.checks.insert(declared, later);
        }
    }
    registry
}

/// Writes the registry, under the hold the caller took to read it.
///
/// The hold is a parameter because a registry is only ever written by the Perch
/// that read it, and is where [`validate`] is asked on the way out, so what this
/// writes and what [`load`] accepts cannot differ.
pub fn save(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &mut Registry) -> Result<()> {
    perch.renew();
    if !perch.still_held() {
        // A general failure rather than `Busy`, deliberately
        // (ADR a-refusal-is-a-promise): `Busy` promises nothing was changed, and
        // this save is reached as often after a Credential moved as before.
        return Err(PerchError::Other(
            "Another `perch` took the registry lock over while this command was \
             working, and has changed the registry since this one read it. \
             Nothing was written, because writing would have undone whatever it \
             did. Run this command again."
                .to_string(),
        ));
    }

    // What `load` will accept, asked before `load` has to refuse it: a command
    // writing a file every later command declined to read would leave a machine
    // with no working `perch` on it, and no `perch holdings purge` either.
    validate(registry).map_err(|invalid| {
        PerchError::Other(format!(
            "{invalid}\n\n{}\n\
             Nothing was written, and the registry on disk is as it was.",
            crate::report::this_is_a_bug(),
        ))
    })?;

    let path = holdings::registry_path(host)?;
    // In place, so the caller's field cannot disagree with the file — and because
    // cloning the Holdings to set one `u32` is a deep copy of every Account and
    // its figures per write, which a Watcher pays every round.
    registry.version = CURRENT_VERSION;
    // Pushed rather than `format!`ed onto, for the reason the line above is in
    // place: a second full copy of the Holdings is a copy a Watcher pays for
    // every round.
    let mut body = serde_json::to_string_pretty(&*registry)
        .map_err(|err| PerchError::Other(format!("could not serialize the registry: {err}")))?;
    body.push('\n');
    write(host, &path, &body)
}

/// Replaces the registry in one step, or not at all, and for its owner alone.
///
/// One step because every command reads this file first, and a crash mid-write
/// would leave it half written for good. Its owner alone because it holds no
/// Credential and everything else about every Account.
fn write(host: &dyn Host, path: &Path, contents: &str) -> Result<()> {
    host.write_private_file(path, contents)
        .map_err(|err| PerchError::file_write(path, err))
}

/// Why there is no active Account, in the terms the way out depends on: holding
/// nothing, a login is the way in; holding Accounts, Perch has merely been left
/// on nobody and naming one is what `perch switch` is for. `because` is what the
/// command wanted an active Account for. One function, because two commands meet
/// this state and only one of them told the difference.
pub fn no_active_account(registry: &Registry, because: &str) -> PerchError {
    if registry.accounts.is_empty() {
        return PerchError::NotFound(format!(
            "Perch holds no Accounts{because}. Run `claude` and log in, then run \
             Perch again."
        ));
    }
    PerchError::NotFound(format!(
        "Perch holds no active Account{because}. `perch switch <target>` makes \
         {} active.",
        match registry.accounts.len() {
            1 => "the one it holds".to_string(),
            held => format!("one of the {held} it holds"),
        }
    ))
}

/// Refuses to act on an Account whose Credential no longer works, in the words of
/// whichever command was asked; `consequence` is what did not happen and why it
/// would have been worse than nothing. One function rather than one per command:
/// `perch run` and `perch switch` meet this state over the same Account and must
/// not describe it in two ways.
pub fn refuse_a_quarantined_account(
    registry: &Registry,
    email: &str,
    consequence: &str,
) -> Result<()> {
    let account = registry.held(email)?;
    match account.quarantine {
        None => Ok(()),
        Some(why) => Err(why.refusal(&registry.named_for_the_user(email), email, consequence)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Refusing;
    use crate::host::prelude::*;
    use chrono::TimeZone;

    /// The way out turns on what is held, and both commands that meet this state
    /// read the same sentence: a login is the answer only where there is nothing
    /// to switch to.
    #[test]
    fn what_to_do_about_no_active_account_depends_on_what_perch_holds() {
        let empty = Registry::default();
        let said = no_active_account(&empty, "").to_string();
        assert!(said.contains("no Accounts"), "{said}");
        assert!(said.contains("`claude`"), "{said}");

        let mut held = Registry::default();
        held.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let said = no_active_account(&held, ", so there is no Group to Cycle within").to_string();
        assert!(said.contains("no Group to Cycle within"), "{said}");
        assert!(said.contains("the one it holds"), "{said}");
        assert!(
            !said.contains("`claude`"),
            "a login repairs nothing here: {said}"
        );
    }

    #[test]
    fn an_account_is_found_however_its_address_is_capitalized() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "café@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the same Account, spelled the way somebody typed it");

        assert!(registry.account("CAFÉ@example.com").is_some());
        assert!(registry.account_mut("CAFÉ@EXAMPLE.COM").is_some());
        assert_eq!(registry.alias_of("Café@Example.com"), Some("work"));

        registry.settle(Some("CAFÉ@EXAMPLE.COM".into()));
        assert!(
            registry.is_active(&Settled(()), "café@example.com"),
            "and which Account is active is the same question, asked the same \
             way — a dozen call sites compared these by exact bytes, which is \
             the one place in Perch an address was not case-folded"
        );
        assert!(!registry.is_active(&Settled(()), "someone@example.com"));

        registry.forget("CAFÉ@example.com");
        assert!(registry.accounts.is_empty(), "and it is the one that goes");
        assert!(registry.aliases.is_empty(), "with the name it answered to");
    }

    /// Two Accounts, which is the fewest a Landing needs.
    fn holding_two() -> Registry {
        let mut registry = Registry::default();
        for email in ["one@example.com", "two@example.com"] {
            registry.upsert(Account {
                identity: Identity {
                    email: email.into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        registry
    }

    #[test]
    fn forgetting_the_arriving_half_of_a_landing_comes_back_to_the_one_being_left() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("two@example.com");

        assert_eq!(
            registry.active().whose(),
            Some("one@example.com"),
            "the Account still held is the one the registry is on"
        );
        assert!(
            registry.active().a_switch_in_flight().is_none(),
            "and the Landing is gone with the half that went"
        );
    }

    #[test]
    fn forgetting_the_leaving_half_of_a_landing_settles_on_nobody() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("one@example.com");

        assert_eq!(*registry.active(), Active::Nobody);
    }

    #[test]
    fn an_account_that_was_named_and_is_not_there_is_refused_rather_than_panicked_on() {
        let registry = Registry::default();

        let refused = registry
            .held("nobody@example.com")
            .expect_err("Perch does not hold it");

        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("bug in Perch"),
            "whose fault it is, because there is nothing for a person to do: {refused}"
        );
        assert_eq!(refused.exit_code(), crate::error::EXIT_GENERAL);

        // The mutable half answers the same way: a caller that goes on to change
        // what it finds needs the other borrow.
        let mut registry = Registry::default();
        assert_eq!(
            registry
                .held_mut("nobody@example.com")
                .expect_err("Perch does not hold it")
                .to_string(),
            refused.to_string()
        );
    }

    /// The `active` states here are written to the field rather than through the
    /// transitions, and this is the one place that is right: a dangling pointer
    /// is a state no transition can produce.
    #[test]
    fn an_active_pointer_naming_nothing_is_refused_like_a_dangling_alias() {
        let mut registry = Registry {
            active: Active::Settled("nobody@example.com".to_string()),
            ..Default::default()
        };

        let refused = validate(&registry).expect_err("it names an Account Perch does not hold");
        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );

        registry.active = Active::Nobody;
        validate(&registry).expect("holding nothing is a state rather than a fault");
    }

    #[test]
    fn both_ends_of_a_landing_are_refused_when_they_name_nothing() {
        let held = "someone@example.com";
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: held.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        for (what, active, names, says) in [
            (
                "the Account it was switching to",
                Active::Landing {
                    leaving: Some(held.to_string()),
                    arriving: "nobody@example.com".to_string(),
                },
                "nobody@example.com",
                "a Switch to nobody@example.com was under way",
            ),
            (
                "the Account it was leaving",
                Active::Landing {
                    leaving: Some("nobody@example.com".to_string()),
                    arriving: held.to_string(),
                },
                "nobody@example.com",
                "a Switch away from nobody@example.com was under way",
            ),
        ] {
            registry.active = active;

            let refused = validate(&registry).expect_err("it names an Account Perch does not hold");

            assert!(refused.to_string().contains(names), "{what}: {refused}");
            assert!(
                refused.to_string().contains(says),
                "{what}: and says which end of the Landing dangles: {refused}"
            );
        }

        // A Landing naming Accounts Perch does hold is a state, not a fault: it
        // is what every interrupted Switch leaves, and the next one resolves it.
        registry.active = Active::Landing {
            leaving: None,
            arriving: held.to_string(),
        };
        validate(&registry).expect("a Switch left in flight is a machine to load, not to refuse");
    }

    /// A table, because the rules are one fact with two spellings — asked of the
    /// one function all four callers go through.
    #[test]
    fn what_a_name_may_be_is_one_rule_for_both_halves_of_the_namespace() {
        /// An Account answering to `work`, and a Group called `personal`.
        fn held() -> Registry {
            let mut registry = Registry::default();
            registry.upsert(Account {
                identity: Identity {
                    email: "someone@example.com".into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
            registry
                .name_account("work", "someone@example.com")
                .expect("the name is free");
            registry.declare_group("personal").expect("so is this one");
            registry
        }

        let cases: &[(NameKind, &str, Option<&str>, Option<&str>)] = &[
            // kind, name, instead_of, the refusal it earns (None = accepted)
            (NameKind::Group, "spare", None, None),
            (NameKind::Alias, "spare", None, None),
            // Its own half.
            (NameKind::Group, "personal", None, Some("already a Group")),
            (NameKind::Alias, "work", None, Some("already names")),
            // Two names that differ only in case are one name.
            (NameKind::Group, "PERSONAL", None, Some("already a Group")),
            (NameKind::Alias, "WORK", None, Some("already names")),
            // The other half, which is what makes the namespace shared.
            (NameKind::Group, "work", None, Some("already an Alias")),
            (
                NameKind::Alias,
                "personal",
                None,
                Some("already a Group name"),
            ),
            // Renaming itself is not colliding with itself, recapitalization
            // included, and the same waiver on both halves.
            (NameKind::Group, "Personal", Some("personal"), None),
            (NameKind::Alias, "Work", Some("work"), None),
            // Shape before collision.
            (NameKind::Group, "", None, Some("cannot be empty")),
            (NameKind::Alias, "", None, Some("cannot be empty")),
            (
                NameKind::Alias,
                "has a space",
                None,
                Some("carries ` ` (U+0020)"),
            ),
            (
                NameKind::Group,
                "none",
                None,
                Some("addresses the Accounts in no Group"),
            ),
            // Not whitespace, and so not caught by the clause above — and the
            // one a terminal reads as an instruction rather than as a name.
            (
                NameKind::Group,
                "\u{1b}[31mred",
                None,
                Some("a control character (U+001B)"),
            ),
            (
                NameKind::Alias,
                "bell\u{7}",
                None,
                Some("a control character (U+0007)"),
            ),
            // `char::is_control` is `Cc` alone, so neither of these was caught
            // by the clause above: the first reverses the rest of the line it is
            // drawn on, and the second is a name drawn identically to `work`.
            (
                NameKind::Group,
                "\u{202e}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+202E)"),
            ),
            (
                NameKind::Alias,
                "wo\u{200b}rk",
                None,
                Some("a character a terminal does not draw as itself (U+200B)"),
            ),
            // The word joiner and the bidi isolates, which are the same harm
            // under two more blocks of the table.
            (
                NameKind::Group,
                "wo\u{2060}rk",
                None,
                Some("a character a terminal does not draw as itself (U+2060)"),
            ),
            (
                NameKind::Group,
                "\u{2066}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+2066)"),
            ),
        ];

        for (kind, name, instead_of, refusal) in cases {
            let asked = held().refuse_a_name_nothing_may_answer_to(*kind, name, *instead_of);
            match refusal {
                None => asked.unwrap_or_else(|err| {
                    panic!("{kind:?} `{name}` should be free: {err}");
                }),
                Some(said) => {
                    let refused = asked.expect_err(&format!("{kind:?} `{name}` is not free"));
                    assert!(
                        refused.to_string().contains(said),
                        "{kind:?} `{name}`: expected {said:?}, got {refused}"
                    );
                }
            }
        }
    }

    /// The registry here is built by hand because nothing reachable produces it:
    /// a Group cannot be declared under a name an Alias already holds. It is what
    /// a third way of making a name would walk into.
    #[test]
    fn recapitalizing_an_alias_still_cannot_walk_into_a_group() {
        let mut registry = Registry::default();
        registry
            .aliases
            .insert("work".to_string(), "someone@example.com".to_string());
        registry
            .groups
            .insert("Work".to_string(), Settings::default());

        let refused = registry
            .refuse_a_name_nothing_may_answer_to(NameKind::Alias, "Work", Some("work"))
            .expect_err("the shared namespace is still checked");

        assert!(
            refused.to_string().contains("already a Group name"),
            "{refused}"
        );
    }

    /// Nothing reachable produces one, so what is asserted is the guard rather
    /// than the property: what happens on the day a command gets it wrong, and
    /// that the file on disk is left alone.
    #[test]
    fn a_registry_load_would_not_read_is_one_save_declines_to_write() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let path = holdings::registry_path(&host).unwrap();
        save(&host, &mut perch, &mut Registry::default()).expect("an empty one is fine");
        let before = host.file(&path).expect("it was written");

        // A dangling Alias is not a refusal downstream — it is the `expect` in
        // every command that resolves a Target.
        let mut broken = Registry::default();
        broken
            .aliases
            .insert("work".to_string(), "nobody@example.com".to_string());

        let refused = save(&host, &mut perch, &mut broken).expect_err("load would not read it");

        let said = refused.to_string();
        assert!(said.contains("nobody@example.com"), "the rule: {said}");
        assert!(said.contains("bug in Perch"), "whose fault it is: {said}");
        assert!(
            !said.contains("Edit the value there"),
            "and not an instruction to edit a file it did not write: {said}"
        );
        assert_eq!(
            refused.exit_code(),
            crate::error::EXIT_GENERAL,
            "a script told 14 would read it as its own input being wrong"
        );
        assert_eq!(
            host.file(&path).as_deref(),
            Some(before.as_str()),
            "and the registry on disk is untouched, which is the whole of it"
        );
    }

    #[test]
    fn a_command_that_takes_its_time_keeps_the_lock_it_took() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");

        // Past the staleness window several times over: the shape of a
        // `perch remove` waiting on somebody who walked away.
        for _ in 0..4 {
            host.sleep(holdings::REGISTRY_STALE_MILLIS as u64 - 10_000);
            save(&host, &mut perch, &mut Registry::default())
                .expect("it is still Perch's to write");
        }

        assert!(perch.still_held());
        assert!(
            holdings::lock(&host).is_err(),
            "and no other Perch could have taken it in the meantime"
        );
    }

    #[test]
    fn a_registry_read_before_somebody_elses_command_is_not_written_over_theirs() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");

        // The stall, and another Perch finding the lock abandoned and taking it.
        host.sleep(holdings::REGISTRY_STALE_MILLIS as u64 + 1_000);
        let theirs = holdings::lock(&host).expect("an abandoned lock is taken over");
        save(&host, &mut { theirs }, &mut Registry::default()).expect("theirs is the live hold");
        let before = load(&host).expect("it reads").expect("they wrote one");

        let mut stale = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        let refused =
            save(&host, &mut perch, &mut stale).expect_err("this one may no longer write");

        assert!(
            refused.to_string().contains("Run this command again"),
            "{refused}"
        );
        assert_eq!(
            load(&host).expect("it reads").expect("a registry is there"),
            before,
            "what the other Perch wrote is what is on disk"
        );
    }

    #[test]
    fn a_check_against_a_group_nothing_declares_is_not_a_registry() {
        let mut registry = Registry::default();
        registry.checks.insert(
            "a-group-nobody-declared".to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );

        let refused = validate(&registry).expect_err("the Cooldown paces nothing");
        assert!(
            refused.to_string().contains("a-group-nobody-declared"),
            "it names the entry: {refused}"
        );

        // The Ungrouped Scope keeps one and is not a Group, which is why the
        // fallback that produces these entries exists at all.
        registry.checks.clear();
        registry.checks.insert(
            UNGROUPED.to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );
        validate(&registry).expect("the Accounts in no Group Cycle too");
    }

    /// `checked` answers with the first in `BTreeMap` order and `record_switch`
    /// writes under the declared spelling, so the record read is not the record
    /// kept — and a Cooldown read off a stale one Switches sooner than 15
    /// minutes.
    #[test]
    fn two_checks_against_one_group_are_not_a_registry() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        for spelling in ["Work", "work"] {
            registry
                .checks
                .insert(spelling.to_string(), Checked { switched_at: at });
        }

        let refused = validate(&registry).expect_err("one of the two paces nothing");
        let said = refused.to_string();
        assert!(
            said.contains("Work") && said.contains("work"),
            "it names both spellings: {said}"
        );
    }

    /// `checked` and `validate` fold the key and the two mutators remove it
    /// exactly, so a `checks` key that outlives its Group leaves `validate`
    /// refusing what `save` has just built — under a sentence saying the fault
    /// is Perch's, on a registry no command can then read.
    #[test]
    fn a_check_keyed_in_another_case_than_its_group_is_brought_to_one_on_the_way_in() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        registry
            .checks
            .insert("Work".to_string(), Checked { switched_at: at });

        let mut registry = readable(registry).expect("one Group, one Check");
        assert_eq!(
            registry.checks.keys().collect::<Vec<_>>(),
            vec!["work"],
            "the Check is filed under the spelling the Group was declared under"
        );

        registry.forget_group("work");
        validate(&registry).expect("and it goes when the Group it paces goes");
        assert!(registry.checks.is_empty(), "{:?}", registry.checks);
    }

    /// The order is the whole of the contract: `validate` asks `declared_group`
    /// about the `checks` key, so a registry judged before it is normalized is
    /// refused over the very Group the normalizer is about to declare.
    #[test]
    fn a_registry_is_normalized_before_it_is_validated_and_never_after() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        // A Group an Account claims and nothing declared, which is the shape
        // `with_every_claimed_group_declared` exists to repair.
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: None,
        });
        registry
            .checks
            .insert("work".to_string(), Checked { switched_at: at });

        validate(&registry).expect_err("judged as it arrived, the key names no declared Group");
        readable(registry).expect("through the door, the Group is declared before it is asked for");
    }

    /// The fold happens before `validate`, so the collision it refuses is one
    /// `load` never reaches — it merges instead, and merging by insertion order
    /// lets `"Work"` (0x57) write over `"work"` whichever is fresher.
    #[test]
    fn two_checks_folding_to_one_group_keep_the_later_switch() {
        let noon = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let january = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        registry
            .checks
            .insert("Work".to_string(), Checked { switched_at: noon });
        registry.checks.insert(
            "work".to_string(),
            Checked {
                switched_at: january,
            },
        );

        let registry = with_every_claimed_group_declared(registry);

        assert_eq!(
            registry.checked("work").map(|it| it.switched_at),
            Some(noon),
            "the older record winning is a Check free to Switch at once"
        );
    }

    #[test]
    fn an_alias_points_at_the_address_as_the_registry_holds_it() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: crate::probe::Identity {
                email: "café@example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the name is free and the Account is there");

        assert_eq!(
            registry.aliases.get("work").map(String::as_str),
            Some("café@example.com"),
            "the Alias names the Account, and the Account has one spelling"
        );
    }

    #[test]
    fn a_key_the_registry_does_not_know_is_refused_rather_than_ignored() {
        let path = "/Users/someone/.config/perch/registry.json";
        for (written, key) in [
            (
                format!(
                    "{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"groups\":\
                     {{\"work\":{{\"watcher_treshold_percent\":50}}}}}}"
                ),
                "watcher_treshold_percent",
            ),
            (
                format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"aliasses\":{{}}}}"),
                "aliasses",
            ),
            // A transposed `leaving` deserializes as `None`, which is not
            // nothing: it is the Landing saying Perch had been on nobody, so the
            // Capture that resumes it has no Profile to file into.
            (
                format!(
                    "{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"active\":\
                     {{\"landing\":{{\"leavign\":\"one@example.com\",\
                     \"arriving\":\"two@example.com\"}}}}}}"
                ),
                "leavign",
            ),
        ] {
            let host = crate::host::FakeHost::new().with_file(path, &written);

            let refused = load(&host).expect_err("a key Perch does not know is not a registry");

            let said = refused.to_string();
            assert!(
                said.contains(key),
                "it names the key it could not read: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the file to put it right in: {said}"
            );
        }
    }

    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.config/perch/registry.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_a_path_refusing(
                path,
                Refusing::Write,
                "No space left on device (os error 28)",
            );

        // Holding the Account its `active` names, so `validate` passes and the
        // unwritable file is what fails the save: refused at the first step, the
        // two assertions below are true of a write that was never attempted.
        let mut registry = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        registry.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let refused = save(&host, &mut perch, &mut registry).expect_err("the write cannot land");
        assert!(
            refused.to_string().contains("No space left on device"),
            "the file is what refused it, rather than the shape: {refused}"
        );

        assert_eq!(
            host.file(path).as_deref(),
            Some(before.as_str()),
            "a reader still sees the registry that was there"
        );
        assert_eq!(
            host.file(crate::host::temp_beside(&host, Path::new(path))),
            None,
            "and the half-written copy is not left beside it"
        );
    }

    #[test]
    fn the_registry_is_written_for_its_owner_alone() {
        let host = crate::host::FakeHost::new();

        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &mut Registry::default()).expect("it is written");

        let path = holdings::registry_path(&host).unwrap();
        assert_eq!(host.mode_of(&path), Some(crate::host::PRIVATE_FILE_MODE));
        assert_eq!(
            host.mode_of(holdings::perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE),
            "a directory others may enter is a directory whose contents others \
             may open"
        );
    }

    #[test]
    fn a_registry_round_trips_through_json() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: Some("Acme".into()),
                organization_uuid: None,
            },
            plan: Some("pro".into()),
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.settle(Some("someone@example.com".into()));

        let json = serde_json::to_string(&registry).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
        assert_eq!(
            back.active_account(&Settled(())).unwrap().plan.as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn what_a_check_recorded_survives_the_file_and_is_absent_until_one_switches() {
        let mut registry = Registry::default();
        assert!(
            !serde_json::to_string(&registry).unwrap().contains("checks"),
            "a machine nothing has been scheduled on records no checks"
        );

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_switch("work", at);
        let back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();

        let recorded = back.checked("work").expect("the Group it Switched within");
        assert_eq!(recorded.switched_at, at);
        assert_eq!(
            back.checked("personal"),
            None,
            "a cooldown is paced per Group, and a Switch within one says \
             nothing about how soon another may move"
        );
    }

    #[test]
    fn forgetting_a_group_forgets_what_a_check_recorded_against_it() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.record_switch("work", Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

        registry.forget_group("work");

        assert_eq!(registry.checked("work"), None);
    }

    #[test]
    fn the_version_is_recorded_so_an_older_build_can_refuse_the_file() {
        let json = serde_json::to_string(&Registry::default()).unwrap();
        assert!(json.contains(&format!("\"version\":{CURRENT_VERSION}")));
    }

    #[test]
    fn a_healthy_account_records_no_quarantine_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("quarantine"), "{json}");

        assert!(registry.quarantine("someone@example.com", Quarantine::RotationLost));
        assert!(
            !registry.quarantine("someone@example.com", Quarantine::NoCredential),
            "the reason kept is how it broke, not the last thing that could not \
             be done to it since"
        );
        let mut back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
        assert_eq!(
            back.release("someone@example.com"),
            Some(Quarantine::RotationLost),
            "the reason survives the round trip, and a login is what ends it"
        );
        assert!(!back.account("someone@example.com").unwrap().quarantined());
    }

    #[test]
    fn an_account_nobody_has_disabled_records_no_disable_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("disabled"), "{json}");

        registry
            .account_mut("someone@example.com")
            .unwrap()
            .disabled = true;
        let written = serde_json::to_string(&registry).unwrap();
        assert!(written.contains(r#""disabled":true"#), "{written}");

        let back: Registry = serde_json::from_str(&written).unwrap();
        assert!(
            back.account("someone@example.com").unwrap().disabled,
            "and it survives the round trip, because it is the half that was said"
        );
    }

    /// `enabled` is `disabled` spelled the other way round.
    #[test]
    fn a_registry_that_still_says_enabled_is_refused_rather_than_read() {
        let held: std::result::Result<Registry, _> = serde_json::from_str(
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"enabled":true}]}"#,
        );

        assert!(held.is_err(), "{held:?}");
    }

    #[test]
    fn nothing_about_where_a_credential_is_kept_is_written_down() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        for derived in ["keychain_service", "keychain_account", "profile", "dir"] {
            assert!(
                !json.contains(derived),
                "a registry that records `{derived}` can disagree with the \
                 derivation it restates: {json}"
            );
        }
    }

    #[test]
    fn a_group_carries_its_configuration_through_json() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.groups.get_mut("work").unwrap().strategy = Strategy::SoonestReset;

        let json = serde_json::to_string(&registry).unwrap();
        assert!(json.contains("soonest-reset"), "{json}");
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
    }

    #[test]
    fn cycling_among_ungrouped_accounts_is_off_until_it_is_asked_for() {
        assert!(!Registry::default().ungrouped.interchangeable);
        let says_nothing_about_it: Registry =
            serde_json::from_str(&format!("{{\"version\":{CURRENT_VERSION}}}"))
                .expect("a registry with no settings in it");
        assert!(
            !says_nothing_about_it.ungrouped.interchangeable,
            "a registry that says nothing about it reads as off, not as a \
             declaration nobody made (ADR a-group-is-a-declaration)"
        );
    }

    #[test]
    fn a_new_group_holds_the_defaults_and_leaves_the_watcher_alone() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        let work = Scope::Group("work".to_string());

        let settings = registry.settings(&work);
        assert!(!settings.watcher_may_act);
        assert_eq!(settings.strategy, Strategy::MostHeadroom);
        assert_eq!(
            settings.watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT
        );

        registry.ungrouped.settings.watcher_threshold_percent = 55;
        assert_eq!(
            registry.settings(&work).watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT,
            "a Setting said about one Scope is said about that Scope: there is \
             no layer for it to arrive at another by"
        );
    }

    #[test]
    fn a_group_declared_later_is_not_reached_by_a_grant_made_earlier() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
            .settings_mut(&Scope::Group("work".to_string()))
            .expect("declared")
            .watcher_may_act = true;
        registry.ungrouped.settings.watcher_may_act = true;

        registry.declare_group("personal").unwrap();

        assert!(
            !registry
                .settings(&Scope::Group("personal".to_string()))
                .watcher_may_act,
            "a Group nobody has said anything about is not one the watcher may \
             act on"
        );
    }

    /// Asserted as the number rather than as the constant, because a default is
    /// a promise made in the docs and a test reading the constant back cannot
    /// notice it change.
    #[test]
    fn the_watchers_policy_has_the_default_it_is_documented_with() {
        assert_eq!(Settings::default().watcher_threshold_percent, 80);
        assert_eq!(Settings::default().watcher_margin_percent, 10);
    }

    #[test]
    fn a_number_out_of_range_is_refused_with_the_range() {
        let cases: [(Settings, &str, &str); 3] = [
            (
                Settings {
                    watcher_threshold_percent: 101,
                    ..Settings::default()
                },
                "watcher-threshold-percent",
                "100",
            ),
            // Zero is the one a margin refuses and a percentage does not: at a
            // margin of nothing an Account is both full enough to leave and
            // clear enough to arrive at.
            (
                Settings {
                    watcher_margin_percent: 0,
                    ..Settings::default()
                },
                "watcher-margin-percent",
                "between 1 and 100",
            ),
            (
                Settings {
                    watcher_margin_percent: 101,
                    ..Settings::default()
                },
                "watcher-margin-percent",
                "between 1 and 100",
            ),
        ];

        let work = Scope::Group("work".to_string());
        for (config, key, accepted) in cases {
            let refusal = config.validate(&work).expect_err("out of range");
            let message = refusal.to_string();
            assert_eq!(refusal.exit_code(), crate::error::EXIT_INVALID, "{message}");
            assert!(message.contains("work"), "{message}");
            assert!(message.contains(key), "{message}");
            assert!(
                message.contains(accepted),
                "a refusal that does not say what would be accepted leaves the \
                 script to guess twice: {message}"
            );
        }

        assert!(Settings::default().validate(&work).is_ok());
        assert!(Settings::default().validate(&Scope::Ungrouped).is_ok());
    }

    #[test]
    fn naming_an_account_that_is_already_named_replaces_the_name() {
        let mut registry = Registry::default();
        registry
            .name_account("overflow", "someone@example.com")
            .expect("the name is free");
        registry
            .name_account("work", "someone@example.com")
            .expect("the Account renames itself");

        assert_eq!(registry.alias_of("someone@example.com"), Some("work"));
        assert!(!registry.aliases.contains_key("overflow"));
        assert_eq!(
            registry.unset_alias("Work"),
            Some(("work".to_string(), "someone@example.com".to_string())),
            "a name is freed however it is capitalized, and says how it was held"
        );
        assert!(registry.unset_alias("work").is_none());
    }

    #[test]
    fn a_group_is_declared_once() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        assert!(registry.declare_group("work").is_err());
        registry
            .ensure_group("work")
            .expect("naming it again in passing is not a conflict");
    }

    /// A script branching on why an Account is Quarantined branches on this
    /// string and nothing else, so every kind needs one and no two may share it.
    #[test]
    fn every_quarantine_has_its_own_machine_readable_name() {
        let every = [
            Quarantine::RenewalRejected,
            Quarantine::RotationLost,
            Quarantine::NoRefreshToken,
            Quarantine::NoCredential,
        ];

        let named: Vec<&str> = every.iter().map(Quarantine::as_str).collect();
        assert_eq!(
            named,
            [
                "renewal-rejected",
                "rotation-lost",
                "no-refresh-token",
                "no-credential"
            ]
        );

        let mut unique = named.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), named.len(), "two Quarantines share a name");

        for why in every {
            assert!(
                !why.because().is_empty(),
                "{why:?} says nothing about itself"
            );
        }
    }

    #[test]
    fn a_registry_that_cannot_be_read_is_a_failure_rather_than_an_empty_perch() {
        let absent = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        assert_eq!(
            load(&absent).expect("a machine Perch has never run on holds nothing"),
            None
        );

        let path = holdings::registry_path(&absent).unwrap();
        let unreadable = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_file(&path, "{}")
            .with_a_path_refusing(&path, Refusing::Read, "permission denied");

        let failed = load(&unreadable).expect_err("a registry that is there must be readable");
        let said = failed.to_string();
        assert!(said.contains("permission denied"), "{said}");
        assert!(
            said.contains(&path.display().to_string()),
            "and it names the file: {said}"
        );
    }

    #[test]
    fn a_groups_settings_are_found_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("Work").expect("a usable name");

        assert!(registry.group("work").is_some());
        assert!(registry.group("WORK").is_some());
        assert!(registry.group("play").is_none());
    }

    #[test]
    fn every_question_about_a_group_is_answered_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: None,
        });

        registry
            .settings_mut(&Scope::Group("WORK".to_string()))
            .expect("the Group is declared, whatever it was typed as")
            .watcher_threshold_percent = 65;
        assert_eq!(
            registry
                .settings(&Scope::Group("Work".to_string()))
                .watcher_threshold_percent,
            65,
            "what was written is what is read back"
        );
        assert_eq!(registry.accounts_in("WORK").len(), 1);

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_switch("WORK", at);
        assert!(
            registry.checked("work").is_some(),
            "one Cooldown record, under the spelling the Group was declared as"
        );

        registry.rename_group("WORK", "office").expect("it renames");
        assert!(registry.group("work").is_none());
        assert_eq!(registry.accounts_in("office").len(), 1);
        assert!(
            registry.checked("office").is_some(),
            "the Cooldown came too"
        );

        registry.forget_group("OFFICE");
        assert!(registry.group("office").is_none());
        assert!(registry.checked("office").is_none());
        // And forgetting one nothing declared is nothing to do.
        registry.forget_group("office");
    }

    #[test]
    fn renaming_a_group_nothing_declared_is_refused_rather_than_panicked_on() {
        let mut registry = Registry::default();

        let error = registry
            .rename_group("work", "office")
            .expect_err("there is no such Group");

        assert!(error.to_string().contains("work"), "{error}");
    }

    #[test]
    fn a_group_an_account_claims_is_declared_by_the_time_anything_reads_it() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"work"}],"groups":{}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        let declared: Vec<&str> = registry.group_names().collect();
        assert!(
            declared.contains(&"work"),
            "the Group the Account claims is in the declared set: {declared:?}"
        );
        assert_eq!(
            registry.group("work"),
            Some(&Settings::default()),
            "carrying what a freshly declared Group carries"
        );
        assert_eq!(registry.accounts_in("work").len(), 1);
    }

    /// The `checks` rule asks `declared_group`, so it has to be asked after the
    /// claim is declared and not before. Judged the other way round it refused
    /// every command on the file, `holdings purge` among them.
    #[test]
    fn a_check_against_a_group_only_an_account_claims_is_read_rather_than_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"work"}],"groups":{},"checks":{"work":{"switched_at":"2026-01-01T00:00:00Z"}}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        assert!(
            registry.checked("work").is_some(),
            "the Check is still there"
        );
    }

    /// The other half: normalizing first must not make the rule toothless.
    #[test]
    fn a_check_against_a_group_nobody_claims_is_still_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"}}],"groups":{},"checks":{"ghost":{"switched_at":"2026-01-01T00:00:00Z"}}}"#,
        );

        let refused = load(&host).expect_err("that Cooldown paces nothing");

        assert!(refused.to_string().contains("ghost"), "{refused}");
    }

    #[test]
    fn a_group_an_account_claims_in_another_case_joins_the_one_that_is_declared() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"Work"}],"groups":{"work":{"watcher_threshold_percent":65}}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        let declared: Vec<&str> = registry.group_names().collect();
        assert_eq!(declared.len(), 1, "one Group, not two: {declared:?}");
        assert_eq!(
            registry.accounts_in("work").len(),
            1,
            "and the Account is in it, rather than in a namesake beside it"
        );
        assert_eq!(
            registry.group("work").unwrap().watcher_threshold_percent,
            65,
            "the declared Group keeps the policy it was declared with"
        );
    }

    #[test]
    fn a_claim_declare_group_would_have_refused_is_named_rather_than_declared() {
        let claims = [
            (r#""none""#, "{}", "addresses the Accounts in no Group"),
            (r#""my work""#, "{}", "carries ` ` (U+0020)"),
            (r#""overflow""#, r#"{}"#, "already an Alias"),
        ];

        for (claimed, groups, expected) in claims {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            let aliases = if expected == "already an Alias" {
                r#","aliases":{"overflow":"someone@example.com"}"#
            } else {
                ""
            };
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}},"group":{claimed}}}],"groups":{groups}{aliases}}}"#
                ),
            );

            let refused = load(&host).expect_err("that is not a Group name");
            let said = refused.to_string();
            assert!(
                said.contains(expected),
                "`{claimed}` should be refused for `{expected}`: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal names the file, because every command reads it \
                 — including the ones that would set this: {said}"
            );
        }
    }

    #[test]
    fn a_declared_group_or_an_alias_nothing_would_have_accepted_is_named_too() {
        let holdings = [
            (2, r#""groups":{"my work":{}}"#, "carries ` ` (U+0020)"),
            (
                2,
                r#""groups":{"none":{}}"#,
                "addresses the Accounts in no Group",
            ),
            (
                2,
                r#""aliases":{"someone@example.com":"other@example.com"}"#,
                "carries `@` (U+0040)",
            ),
            (
                2,
                r#""groups":{"work":{}},"aliases":{"work":"someone@example.com"}"#,
                "share one namespace",
            ),
            // Version 3, where a control character is a name no Perch gave. A
            // version 2 Perch gave one for six of the eight days it was current,
            // so the step forward owes that document a rename rather than this.
            (
                3,
                r#""groups":{"\u001b[31mred":{}}"#,
                "a control character (U+001B)",
            ),
        ];

        for (version, held, expected) in holdings {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(r#"{{"version":{version},"accounts":[],{held}}}"#),
            );

            let refused = load(&host).expect_err("that is not a name Perch would have given");
            let said = refused.to_string();
            assert!(
                said.contains(expected),
                "`{held}` should be refused for `{expected}`: {said}"
            );
            // These registries hold no Accounts at all, so a refusal explaining
            // itself in terms of what an Account can be in says nothing.
            assert!(
                !said.contains("an Account cannot be in it"),
                "a declared Group is refused in words about the name rather \
                 than about Accounts this registry does not hold: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal names the file to edit: {said}"
            );
        }
    }

    /// Both files parse as JSON perfectly well — a Strategy this build has no
    /// variant for, and a missing `version`. Told "not valid JSON", somebody goes
    /// looking for a syntax error that is not there.
    #[test]
    fn a_registry_that_is_json_and_still_unreadable_is_not_called_bad_json() {
        let files = [
            r#"{"version":2,"accounts":[],"groups":{"work":{"strategy":"round-robin"}}}"#,
            r#"{"accounts":[],"groups":{}}"#,
        ];

        for contents in files {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            host.set_file(&path, contents);

            let refused = load(&host).expect_err("this build cannot read it");
            let said = refused.to_string();
            assert!(
                !said.contains("not valid JSON"),
                "the file parses as JSON perfectly well: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal still names it: {said}"
            );
        }
    }

    #[test]
    fn a_percentage_that_is_not_one_is_refused_rather_than_ranked_on() {
        for figure in ["-50", "150"] {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}},"utilization":{{"observed_at":"2025-01-01T00:00:00Z","windows":[{{"window":"5-hour","used_percent":{figure}}}]}}}}],"groups":{{}}}}"#
                ),
            );

            let refused = load(&host).expect_err("that is not a percentage");
            assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
            let said = refused.to_string();
            assert!(
                said.contains("5-hour") && said.contains("someone@example.com"),
                "`{figure}` should be refused naming the window and the Account: \
                 {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }

        // And the ends of the range are figures, not refusals: a window that has
        // just reset and one that is completely spent are both ordinary.
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"utilization":{"observed_at":"2025-01-01T00:00:00Z","windows":[{"window":"5-hour","used_percent":0},{"window":"7-day","used_percent":100}]}}],"groups":{}}"#,
        );
        load(&host).expect("0 and 100 are both percentages");
    }

    #[test]
    fn an_alias_for_an_account_perch_does_not_hold_is_refused_and_names_both() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"}}],"aliases":{"overflow":"gone@example.com"}}"#,
        );

        let refused = load(&host).expect_err("the Alias names nobody");

        assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
        let said = refused.to_string();
        assert!(said.contains("overflow"), "which Alias: {said}");
        assert!(
            said.contains("gone@example.com"),
            "and who it names: {said}"
        );
        assert!(
            said.contains("registry.json"),
            "and the file to edit: {said}"
        );
    }

    #[test]
    fn two_names_in_one_half_of_the_namespace_that_differ_only_in_case_are_refused() {
        let holdings = [
            r#""groups":{"work":{},"Work":{}}"#,
            // Two Accounts, one Alias each: giving both names to one Account is
            // a different refusal.
            r#""aliases":{"work":"someone@example.com","Work":"other@example.com"}"#,
        ];

        for held in holdings {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}}}},{{"identity":{{"email":"other@example.com"}}}}],{held}}}"#
                ),
            );

            let refused = load(&host).expect_err("which of the two a Target finds is undecided");
            let said = refused.to_string();
            assert!(said.contains("differ only in case"), "`{held}`: {said}");
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }
    }

    #[test]
    fn an_account_address_a_name_could_be_confused_with_is_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"work"}},{"identity":{"email":"real@example.com"}}],"groups":{"work":{}}}"#,
        );

        let refused = load(&host).expect_err("that Target has two answers");
        let said = refused.to_string();
        assert!(said.contains("Account called `work`"), "{said}");
        assert!(
            said.contains("could be told from"),
            "it says what is wrong with it: {said}"
        );
        assert!(
            said.contains("registry.json"),
            "and the file to edit: {said}"
        );
    }

    /// An address is Claude Code's rather than anybody's choice, so it is
    /// refused where it enters and drawn stripped. Refused here it would be
    /// refused at `load`, which takes every command with it — including
    /// `perch remove`, which is the only way such an Account could ever go.
    #[test]
    fn an_account_address_a_terminal_would_obey_reads_rather_than_bricking() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"bad\u001brow@example.com"}}]}"#,
        );

        let registry = load(&host)
            .expect("a registry a published Perch wrote is readable")
            .expect("it is there");
        assert_eq!(registry.accounts.len(), 1);
    }

    #[test]
    fn one_account_reached_twice_over_is_refused() {
        let cases = [
            (
                r#""aliases":{"spare":"someone@example.com","work":"someone@example.com"}"#,
                "one Alias at a time",
            ),
            (
                r#""aliases":{}"#,
                // Two entries for one address, spelled differently.
                "which are one Account",
            ),
        ];

        for (index, (held, expected)) in cases.iter().enumerate() {
            let accounts = if index == 0 {
                r#"[{"identity":{"email":"someone@example.com"}}]"#
            } else {
                r#"[{"identity":{"email":"someone@example.com"}},{"identity":{"email":"SOMEONE@example.com"}}]"#
            };
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = holdings::registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(r#"{{"version":2,"accounts":{accounts},{held}}}"#),
            );

            let refused = load(&host).expect_err("one Account reached twice over");
            let said = refused.to_string();
            assert!(said.contains(expected), "`{held}`: {said}");
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }
    }

    /// The margin reaches `load` by the same door, so a hand edit is refused where
    /// no `perch config set` could have written it.
    #[test]
    fn a_margin_of_nothing_in_the_file_is_refused_by_the_read() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[],"groups":{"work":{"watcher_margin_percent":0}}}"#,
        );

        let refused = load(&host).expect_err("nothing is not a margin");

        assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
        let said = refused.to_string();
        assert!(said.contains("watcher-margin-percent"), "{said}");
        assert!(said.contains("between 1 and 100"), "{said}");
    }

    #[test]
    fn a_number_out_of_range_in_the_file_is_refused_by_the_read_and_names_the_file() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = holdings::registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[],"groups":{"work":{"watcher_threshold_percent":101}}}"#,
        );

        let refused = load(&host).expect_err("101 is not a percentage");

        assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
        let said = refused.to_string();
        assert!(said.contains("work"), "which Group: {said}");
        assert!(
            said.contains("watcher-threshold-percent"),
            "which setting, spelled the way it is set: {said}"
        );
        assert!(
            said.contains(&path.display().to_string()),
            "and where to go and change it, because no command can: {said}"
        );
    }

    #[test]
    fn a_registry_this_build_writes_says_so() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let mut stale = Registry {
            version: 0,
            ..Registry::default()
        };

        save(&host, &mut perch, &mut stale).expect("it writes");

        assert_eq!(
            load(&host).expect("it reads").expect("it is there").version,
            CURRENT_VERSION
        );
    }
}
