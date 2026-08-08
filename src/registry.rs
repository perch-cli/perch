//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned, so that a registry written by a build that understands more than
//! this one is refused rather than silently misread. The version is a guard
//! against the future and not a migration story: nobody is running Perch yet,
//! so there is no past format to read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::lock;
use crate::probe::{Identity, LockSpec};

/// The version this build writes, and the only one there has ever been. A
/// registry from the future is refused rather than silently misread.
pub const CURRENT_VERSION: u32 = 1;

/// One Quota Window's Utilization, as observed at a point in time (ADR 0015).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
/// only `perch status --refresh` ever goes and fetches (ADR 0015).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedUtilization {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<WindowUtilization>,
}

/// Why an Account's Credential can no longer be used and cannot be recovered
/// from anything Perch holds.
///
/// Recorded rather than merely counted, because "this Account is broken" and
/// "this Account is broken because Anthropic retired its refresh token" are
/// different pieces of news: the first leaves the user guessing whether Perch
/// lost something, and the second says what happened and implies the repair.
/// Every one of these is terminal — none of them can be undone by trying again,
/// which is exactly what makes it a Quarantine rather than a failed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quarantine {
    /// Anthropic turned the refresh token down — retired, revoked, or belonging
    /// to a login that has been ended elsewhere.
    RenewalRejected,
    /// Anthropic Rotated the refresh token and the new one could not be stored,
    /// so the old one is retired and the new one is gone: ADR 0006's crash
    /// between two writes, arriving as a failed write instead of a crash.
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

    /// The whole of what is said about a Quarantined Account wherever one is
    /// shown as broken: which Account, what happened to it, and the one command
    /// that ends it.
    ///
    /// Said in one place because every surface owes the same three things. Two
    /// surfaces spelling this out separately would eventually offer two
    /// different repairs for one state.
    ///
    /// `detail` is whatever the failure underneath said, where there was one
    /// worth keeping — a keychain that would not take the Rotated Credential,
    /// say. The reason is what happened; the detail is how.
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

    /// The refusal a command raises rather than acting on a Quarantined
    /// Account, as opposed to [`said_of`](Quarantine::said_of), which is how one
    /// is *shown*.
    ///
    /// `consequence` is the caller's, and it is the only part that differs:
    /// what a Switch would have cost is not what a Run would have. Everything
    /// around it is shared, so the third command to refuse a Quarantine cannot
    /// come to offer a different repair from the first two.
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

    /// The same as a script reads it. Absent reads as false and present reads
    /// as true wherever a script asks whether it is set, so the fact a script
    /// already branches on branches the same way — and now carries why.
    pub fn document(quarantine: Option<Quarantine>) -> serde_json::Value {
        match quarantine {
            Some(why) => {
                serde_json::json!({"reason": why.as_str(), "detail": why.because()})
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// Who this Account is. Its email address is also its identifier.
    pub identity: Identity,
    /// The subscription the Credential reports — `pro`, `max`, and so on. It
    /// comes from the Credential rather than the Identity, which is why it is
    /// not part of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Whether the Account is a Cycle candidate. Later specs toggle this.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Why this Account's Credential can no longer be used, when it cannot.
    ///
    /// Absent is the ordinary case, and is left out of the file entirely rather
    /// than written as a null: the registry is something a person may open, and
    /// a healthy Account reads more clearly for saying nothing about its health.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<CachedUtilization>,
}

fn enabled_by_default() -> bool {
    true
}

/// How Cycling orders the Accounts in a Group.
///
/// Both readings measure headroom the same way — the worst Quota Window an
/// Account has (ADR 0012) — and differ only in what they do with it. The
/// measurement is fixed and the Strategy is a separate axis on top of it, so
/// neither reading is a way round an exhausted Account.
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

/// The least wall-clock between two unattended Switches, when nobody has said
/// otherwise (ADR 0013). A five-hour window moves slowly enough that fifteen
/// minutes never misses a real crossing, and often enough that a watcher which
/// has just moved you is not about to move you again.
pub const DEFAULT_WATCHER_COOLDOWN_MINUTES: u32 = 15;

/// How far below the threshold a candidate has to sit before it is worth moving
/// to, when nobody has said otherwise. This is what kills the ping-pong: at an
/// 80% threshold nothing is Switched to unless it is at 70% or better.
pub const DEFAULT_WATCHER_MARGIN_PERCENT: u8 = 10;

/// The longest cooldown that means anything. The longest Quota Window Anthropic
/// reports is seven days, so a cooldown past it is one that could never let a
/// second Switch happen inside any window it is pacing.
pub const MAX_WATCHER_COOLDOWN_MINUTES: u32 = 7 * 24 * 60;

/// What a percentage accepts, said once so that the refusal a mistyped `perch
/// config set` gets and the one a hand-edited registry gets are the same words.
pub const A_PERCENTAGE: &str = "a whole number between 0 and 100";

/// The same for a cooldown, which is a count of minutes rather than a share of
/// a window. Built from the bound rather than written out beside it, so the
/// sentence and the number it describes cannot come to disagree.
pub fn a_cooldown() -> String {
    format!("a whole number of minutes between 0 and {MAX_WATCHER_COOLDOWN_MINUTES} (seven days)")
}

/// What a Group carries besides its Accounts: the rules that govern Cycling
/// within it (ADR 0002), asked and unasked.
///
/// Four of these are the watcher's policy, and they are a Group's rather than
/// constants because they are preferences rather than arithmetic: how full is
/// too full, how often is too often, how much emptier is worth the move, and
/// whether coming straight back is allowed (ADR 0013). The interval the loop
/// Refreshes at is not among them — that one is derived from Anthropic's
/// allowance rather than from anyone's taste, and lives in
/// [`crate::watch::REFRESH_INTERVAL_MILLIS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupConfig {
    pub strategy: Strategy,
    /// Whether the watcher may Switch within this Group unattended. Off unless
    /// the user says otherwise: nothing changes underneath someone because they
    /// did not say it could (ADR 0002).
    pub watcher_may_act: bool,
    /// The Utilization the watcher would act at, as a percentage.
    pub watcher_threshold_percent: u8,
    /// The least wall-clock between two unattended Switches, in minutes.
    pub watcher_cooldown_minutes: u32,
    /// How far under the threshold a candidate has to be before moving to it is
    /// worth doing, in percentage points.
    pub watcher_margin_percent: u8,
    /// Whether the Account a Switch just left is barred from being Switched
    /// back to for one cooldown.
    pub watcher_no_return: bool,
}

impl Default for GroupConfig {
    fn default() -> Self {
        GroupConfig {
            strategy: Strategy::default(),
            watcher_may_act: false,
            watcher_threshold_percent: DEFAULT_WATCHER_THRESHOLD_PERCENT,
            watcher_cooldown_minutes: DEFAULT_WATCHER_COOLDOWN_MINUTES,
            watcher_margin_percent: DEFAULT_WATCHER_MARGIN_PERCENT,
            watcher_no_return: true,
        }
    }
}

impl GroupConfig {
    /// Refuses configuration that cannot mean what it says. Serde already
    /// refuses a strategy Perch does not implement, and a `true`/`false` that
    /// is neither; what is left is the ranges the numbers have to be in.
    ///
    /// Every refusal names the numbers that would have been accepted, because
    /// the script that mistyped one is the reader, and being told only that it
    /// was wrong leaves it to guess twice.
    pub fn validate(&self, group: &str) -> Result<()> {
        if self.watcher_threshold_percent > 100 {
            return Err(out_of_range(
                group,
                "watcher-threshold-percent",
                self.watcher_threshold_percent,
                A_PERCENTAGE,
            ));
        }
        if self.watcher_margin_percent > 100 {
            return Err(out_of_range(
                group,
                "watcher-margin-percent",
                self.watcher_margin_percent,
                A_PERCENTAGE,
            ));
        }
        if self.watcher_cooldown_minutes > MAX_WATCHER_COOLDOWN_MINUTES {
            return Err(out_of_range(
                group,
                "watcher-cooldown-minutes",
                self.watcher_cooldown_minutes,
                &a_cooldown(),
            ));
        }
        Ok(())
    }
}

/// A number a setting cannot hold, refused with the ones it can.
fn out_of_range(
    group: &str,
    key: &str,
    held: impl std::fmt::Display,
    accepted: &str,
) -> PerchError {
    PerchError::Invalid(format!(
        "Group `{group}` has a `{key}` of {held}, and it takes {accepted}."
    ))
}

/// The Switch a scheduled Check made within a Group, kept so the next one can
/// be paced by it (ADR 0013).
///
/// The one thing about the watcher that is written down, and only because
/// `perch watch --once` is a fresh process every time: the cooldown and the
/// no-return are measured from the last Switch, and a check that could not
/// remember one would be a check with no policy but the threshold. The loop
/// carries the same two facts in memory and records nothing, because a loop is
/// one process and a person watching it — two of them would otherwise pace each
/// other's decisions.
///
/// Per Group rather than per machine: a cooldown is a Group's setting, and a
/// Switch within `work` has nothing to say about how soon `personal` may move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    /// When the Switch happened, which is what the cooldown counts from.
    pub switched_at: DateTime<Utc>,
    /// The Account it Switched off, which no-return bars for one cooldown.
    pub switched_off: String,
}

/// The configuration that belongs to no Group, because there is no Group for
/// it to belong to (ADR 0017).
///
/// An ungrouped Account has nothing carrying its settings, and modelling the
/// Accounts in no Group as a Group with a reserved name would make a Group mean
/// two contradictory things — a declaration the user made, and one Perch made
/// for them. So the one setting about those Accounts is global.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    /// Whether bare `perch switch` may Cycle among the Accounts in no Group.
    ///
    /// Off unless the user says otherwise, and deliberately so: being ungrouped
    /// is the absence of a declaration that Accounts are interchangeable, not a
    /// weaker form of one. Cycling freely here would move someone from their
    /// work subscription onto their personal one without their ever having said
    /// the two were substitutable.
    pub cycle_ungrouped: bool,
}

/// The word that addresses "no Group at all" on `perch group move`, and the
/// answer `perch add` accepts to the Group it offers. A Group cannot be called
/// this, because then one of the two meanings would be unreachable.
pub const NO_GROUP: &str = "none";

/// Whether a name is the one that means no Group at all.
pub fn means_no_group(name: &str) -> bool {
    same_name(name, NO_GROUP)
}

/// Which of the two things sharing the namespace is being named. A refusal
/// says which: being told `none` cannot be a name is less use than being told
/// what Perch was asked to call `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Alias,
    Group,
}

impl NameKind {
    /// The kind of name, plural, so a refusal states the rule that was broken
    /// rather than a remark about the one name that broke it.
    pub fn names(self) -> &'static str {
        match self {
            NameKind::Alias => "Alias names",
            NameKind::Group => "Group names",
        }
    }
}

/// Refuses a name that could not be told from something else.
///
/// Aliases and Group names share one namespace and are both valid Targets for
/// `switch` and `run`, so a name has to be distinguishable from the other
/// things a Target can be: an email address, and the word that means no Group
/// at all.
/// Whether two names the user chose are the same name.
///
/// Case-insensitively, because nobody remembers which way they capitalised a
/// Group months ago — and over the whole of Unicode, because
/// [`validate_name`] accepts the whole of Unicode. Comparing only ASCII would
/// make `café` and `CAFÉ` two different Groups while making `work` and `Work`
/// one, which is the ambiguity the rule exists to prevent, kept for exactly
/// the users whose language has accents in it.
pub fn same_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

pub fn validate_name(kind: NameKind, name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(PerchError::Invalid(format!(
            "{} cannot be empty.",
            kind.names()
        )));
    }
    if name != name.trim() {
        return Err(PerchError::Invalid(format!(
            "`{name}` starts or ends with a space, which would make it impossible to type reliably."
        )));
    }
    if means_no_group(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` means no Group at all on `perch group move`, so it cannot also name one."
        )));
    }
    if name.contains('@') {
        return Err(PerchError::Invalid(format!(
            "`{name}` looks like an email address. {} have to be tellable from one, because a Target that could be either has no single answer.",
            kind.names()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    /// The email address of the active Account, if there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Alias to Account email. Empty until aliases land.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// The Groups the user has declared, with what each one carries. A Group
    /// exists here even when it holds no Accounts: it is a statement the user
    /// made, not a summary of where the Accounts happen to be.
    #[serde(default)]
    pub groups: BTreeMap<String, GroupConfig>,
    /// The configuration that hangs off no Group.
    #[serde(default)]
    pub global: GlobalConfig,
    /// What the last scheduled Check did in each Group. Written by
    /// `perch watch --once` and by nothing else, and absent from the file until
    /// one of them Switches.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, Checked>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            active: None,
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
            groups: BTreeMap::new(),
            global: GlobalConfig::default(),
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
    /// recorded beside it: two statements of one fact can disagree, and this is
    /// the fact every Credential Store is derived from in turn (ADR 0020).
    pub fn profile_dir(&self, host: &dyn Host) -> Result<PathBuf> {
        profile_dir_for(host, self.email())
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
    pub fn account(&self, email: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| account.email() == email)
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.active.as_deref().and_then(|email| self.account(email))
    }

    /// Every Group name in use. A Group an Account claims is always declared
    /// too — [`load`] sees to that — so this is the declared set.
    pub fn group_names(&self) -> BTreeSet<&str> {
        self.groups.keys().map(String::as_str).collect()
    }

    pub fn group(&self, name: &str) -> Option<&GroupConfig> {
        self.groups.get(name)
    }

    /// The Accounts in a Group, in the order they were added.
    pub fn accounts_in(&self, group: &str) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.as_deref() == Some(group))
            .collect()
    }

    /// The Accounts that are in no Group — the ordinary starting state, not an
    /// error (ADR 0017).
    pub fn ungrouped_accounts(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.is_none())
            .collect()
    }

    /// The Group declared under a name, whatever it was capitalised as. Two
    /// names that differ only in case are one name here (see
    /// [`refuse_taken_names`](Self::refuse_taken_names)), so this is how a
    /// Group typed in passing is matched to the one that exists.
    pub fn declared_group(&self, name: &str) -> Option<&str> {
        self.groups
            .keys()
            .find(|declared| same_name(declared, name))
            .map(String::as_str)
    }

    /// Declares a Group, refusing a name that is not usable or already means
    /// something else.
    pub fn declare_group(&mut self, name: &str) -> Result<()> {
        validate_name(NameKind::Group, name)?;
        if let Some(declared) = self.declared_group(name) {
            return Err(PerchError::Conflict(format!(
                "There is already a Group called `{declared}`."
            )));
        }
        self.refuse_taken_names(None, Some(name))?;
        self.groups.insert(name.to_string(), GroupConfig::default());
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name
    /// a Group in passing rather than to create one — `perch add --group`.
    ///
    /// Returns the name to record, which is the spelling the Group was
    /// declared under: naming `Work` in passing joins `work` rather than
    /// leaving the Account in a Group nothing else can see.
    pub fn ensure_group(&mut self, name: &str) -> Result<String> {
        if let Some(declared) = self.declared_group(name) {
            return Ok(declared.to_string());
        }
        self.declare_group(name)?;
        Ok(name.to_string())
    }

    /// Forgets a Group. The caller establishes that nothing is left in it:
    /// dropping the Group is not a way to empty it.
    ///
    /// What a scheduled Check left behind goes with it. A Group that is gone
    /// paces nothing, and a record kept past it would be a cooldown a Group
    /// declared under the same name later inherited from a Group it never was.
    pub fn forget_group(&mut self, name: &str) {
        self.groups.remove(name);
        self.checks.remove(name);
    }

    /// What the last scheduled Check did within a Group, if one has Switched
    /// there.
    pub fn checked(&self, group: &str) -> Option<&Checked> {
        self.checks.get(group)
    }

    /// Records a Switch a Check made, for the next one to be paced by.
    pub fn record_check(&mut self, group: &str, switched_off: &str, at: DateTime<Utc>) {
        self.checks.insert(
            group.to_string(),
            Checked {
                switched_at: at,
                switched_off: switched_off.to_string(),
            },
        );
    }

    pub fn account_mut(&mut self, email: &str) -> Option<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| account.email() == email)
    }

    /// The Alias an Account answers to, if it has been given one.
    pub fn alias_of(&self, email: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, target)| *target == email)
            .map(|(alias, _)| alias.as_str())
    }

    /// An Account as the user names it: by its Alias when it has one, so a
    /// message about it reads the way they would say it.
    pub fn named_for_the_user(&self, email: &str) -> String {
        match self.alias_of(email) {
            Some(alias) => format!("{email} (as `{alias}`)"),
            None => email.to_string(),
        }
    }

    /// The Alias held under a name, whatever it was capitalised as, and the
    /// Account it reaches.
    pub fn declared_alias(&self, name: &str) -> Option<(&str, &str)> {
        self.aliases
            .iter()
            .find(|(alias, _)| same_name(alias, name))
            .map(|(alias, email)| (alias.as_str(), email.as_str()))
    }

    /// Refuses an Alias and a Group name that would not both be free.
    ///
    /// Aliases and Group names share one namespace, so neither can shadow the
    /// other and the single Target on `switch` and `run` always has one
    /// answer. The pair is checked together as well as against what is already
    /// held: a command that sets both at once could otherwise plant the
    /// collision it is meant to prevent.
    ///
    /// Two names that differ only in case are the same name. Nobody remembers
    /// which way they capitalised a Group months ago, so `work` and `Work`
    /// reaching different Accounts is the ambiguity this exists to prevent
    /// even though a lookup could tell them apart.
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
            if let Some(declared) = self.declared_group(alias) {
                return Err(PerchError::Conflict(format!(
                    "`{declared}` is already a Group name, and a name cannot be both."
                )));
            }
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

    /// Names an Account, having established the name is free.
    ///
    /// An Account answers to one Alias, so naming one that already had a name
    /// replaces it: a name the user has moved on from should not go on
    /// reaching the Account behind their back.
    pub fn set_alias(&mut self, alias: &str, email: &str) {
        self.aliases.retain(|_, named| named != email);
        self.aliases.insert(alias.to_string(), email.to_string());
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
    /// An Account is never dropped for this: it stays listed, keeps its Alias,
    /// its Group and its place, and goes on being named. An Account that
    /// vanishes reads as data loss, and a broken one reads as something needing
    /// attention — which is what it is.
    ///
    /// The first reason stands. A Quarantined Account asked a second question
    /// fails a second way, and the reason worth keeping is the one that says how
    /// it broke rather than the last thing that could not be done to it since.
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

    /// Forgets an Account: the entry, the Alias that reached it, and its place
    /// as the active one.
    ///
    /// Its Group is left declared. A Group is something the user said rather
    /// than a summary of where the Accounts happen to be, so emptying one is not
    /// a reason to withdraw the statement — and `perch group remove` is how it
    /// is withdrawn.
    ///
    /// The Credential is not this to delete: what a Profile holds is the
    /// caller's to take away before the row that names it goes, so a store that
    /// will not give it up is met while the Account can still be named.
    pub fn forget(&mut self, email: &str) {
        self.accounts.retain(|account| account.email() != email);
        self.aliases.retain(|_, named| named != email);
        if self.active.as_deref() == Some(email) {
            self.active = None;
        }
    }

    pub fn upsert(&mut self, account: Account) {
        match self
            .accounts
            .iter_mut()
            .find(|existing| existing.email() == account.email())
        {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }
}

/// `$PERCH_HOME`, or `~/.config/perch` — an error when neither is knowable,
/// because a machine that cannot say where home is gets a refusal rather than a
/// registry written into the filesystem root.
///
/// Under `~/.config` rather than directly in home. A tool that keeps its state
/// in the home directory adds a line to what somebody sees every time they list
/// it, and Perch's state is not something anybody reads by hand.
///
/// The same path on every platform, Windows included, rather than
/// `%APPDATA%`: one rule is easier to document, to support and to keep in the
/// Host port, which exposes a home directory and nothing else. It is a
/// preference rather than a constraint — nothing in the design breaks under a
/// platform-specific path — and `$PERCH_HOME` is there for anybody who wants
/// one.
///
/// `~/.config` is created if it is not there — at 0700, along with everything
/// below it, since what goes under it here is Credentials.
pub fn perch_home(host: &dyn Host) -> Result<PathBuf> {
    if let Some(overridden) = host.env_var("PERCH_HOME") {
        return Ok(PathBuf::from(overridden));
    }
    Ok(home_dir(host)?.join(".config").join("perch"))
}

fn home_dir(host: &dyn Host) -> Result<PathBuf> {
    host.home_dir()
        .map_err(|err| PerchError::Other(err.to_string()))
}

pub fn registry_path(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("registry.json"))
}

pub fn profiles_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("profiles"))
}

/// The Profile directory for an Account. The email is slugged because the path
/// is hashed into a keychain service name and has to be stable and printable.
///
/// An address with no alphanumeric character in it slugs to nothing, and
/// joining nothing onto a path gives back the path: the Profile of such an
/// Account would *be* `profiles/`, the directory holding every other Account's.
/// `perch remove` deletes a Profile directory whole, so that Account is one
/// removal away from taking every Credential Perch holds with it. Refused here,
/// at the one place every store and every keychain namespace is derived from,
/// rather than trusted to whatever wrote the address.
pub fn profile_dir_for(host: &dyn Host, email: &str) -> Result<PathBuf> {
    let profiles = profiles_dir(host)?;
    let slugged = slug(email);
    let dir = profiles.join(&slugged);

    // Two ways of asking the same question, because the answer is the whole
    // machine: an empty slug, and — for whatever a future slug might let
    // through — a path that is not one directory below the one it was joined to.
    if slugged.is_empty() || dir.parent() != Some(profiles.as_path()) {
        return Err(PerchError::Invalid(format!(
            "`{email}` has no character a Profile directory can be named after, \
             so Perch cannot say where its Credential would be kept.\n\
             An Account recorded under that address has to be removed from \
             {} by hand.",
            registry_path(host)?.display(),
        )));
    }
    Ok(dir)
}

/// Where a login lives while Perch is running it.
///
/// A Profile is named after the Account it holds, and which Account that is
/// only becomes knowable once the login has finished — so the login happens
/// here and its Credential is moved into a Profile afterwards. Nothing outlives
/// the command: this directory is removed whether the login worked or not.
pub fn pending_login_dir(host: &dyn Host, started_at: DateTime<Utc>) -> Result<PathBuf> {
    Ok(pending_logins_dir(host)?.join(format!("login-{}", started_at.timestamp_millis())))
}

/// Where every pending login lives, so the ones nobody came back from can be
/// found again.
pub fn pending_logins_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("pending"))
}

/// When the login that made this directory started, as its name records.
///
/// The name is `login-<millis>`, written by [`pending_login_dir`], and it is
/// the only account of the directory's age that nothing later moves.
pub fn pending_login_started_at(dir: &Path) -> Option<DateTime<Utc>> {
    let millis: i64 = dir
        .file_name()?
        .to_str()?
        .strip_prefix("login-")?
        .parse()
        .ok()?;
    DateTime::from_timestamp_millis(millis)
}

/// Whether two Accounts derive the same Profile directory. The derivation is
/// `profiles_dir` joined with the slugged email, so sharing a slug is sharing
/// a Profile — kept here beside the derivation so the two cannot drift apart.
pub fn same_profile(one: &str, other: &str) -> bool {
    slug(one) == slug(other)
}

pub fn slug(email: &str) -> String {
    let slugged: String = email
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slugged.trim_matches('-').to_string()
}

/// How long a Perch that died holding the registry lock keeps it.
///
/// Longer than the Claude Code locks a Switch takes, because it is the outer
/// lock: everything one of those waits for happens inside this one. Short
/// enough that a machine whose Perch was killed mid-command is usable again
/// within a minute rather than needing the directory removed by hand.
const REGISTRY_STALE_MILLIS: i64 = 90_000;
const REGISTRY_UPDATE_MILLIS: i64 = 5_000;

/// The lock one Perch takes so that no other Perch is changing the registry at
/// the same time.
///
/// A directory, taken with the same `mkdir`-or-fail primitive the Claude Code
/// locks use, for the same reason: the call both asks and answers, with nothing
/// in between. It excludes only other Perches — Claude Code has no interest in
/// this file — which is exactly the set that needs excluding.
pub fn lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch registry lock",
        held_by: "the other `perch`",
        dir: perch_home(host)?.join(".registry.lock"),
        stale_millis: REGISTRY_STALE_MILLIS,
        update_millis: REGISTRY_UPDATE_MILLIS,
        lost_means: "Another `perch` has been changing the registry since this \
                     command read it, so what this one holds in memory is behind \
                     what is on disk. Nothing of it will be written over theirs.",
    })
}

/// Shuts every other Perch out of the registry until the returned hold is
/// dropped.
///
/// Every command is a load, some changes made in memory, and a save of the
/// whole thing — so two of them overlapping does not merge, it discards. The
/// hold is taken for the span of the command rather than for the span of the
/// write, because it is the *read* that goes stale: a `perch add` that saved
/// its copy after a `perch switch` had landed would put `active` back to
/// whatever it was before the Switch, and the next Capture would then write the
/// live Credential into the wrong Account's Profile (ADR 0006).
///
/// Never held across a browser login. That is minutes of somebody else's time,
/// and the commands that spend it take this afterwards, against a registry read
/// fresh.
///
/// Perch's home is created here, and privately, because on a fresh machine this
/// is the first thing to need it: the lock artifact lives inside it, so a
/// `create_dir_all` on the way to `mkdir` would otherwise be what brings the
/// directory into being — at whatever the umask happens to be, for a directory
/// that is about to hold Credentials.
pub fn lock(host: &dyn Host) -> Result<lock::Held<'_>> {
    let home = perch_home(host)?;
    host.create_private_dir_all(&home)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", home.display())))?;
    lock::take_all(host, vec![lock_spec(host)?])
}

/// Reads the registry, or `None` when Perch has never run here.
/// The `version` a document claims, read on its own.
///
/// Deliberately not a parse of the whole thing: what this exists to answer is
/// "was this written by something newer than me", and a document from something
/// newer is exactly the document this build cannot deserialize. A shape holding
/// one number deserializes out of any JSON object that carries it, whatever
/// else the object holds and whatever the rest of it means.
///
/// `None` is "it does not say", which is not a claim about a newer Perch: the
/// caller goes on to read the document properly and reports what it finds
/// there.
fn version_of(contents: &str) -> Option<u32> {
    #[derive(serde::Deserialize)]
    struct Versioned {
        version: Option<u32>,
    }

    serde_json::from_str::<Versioned>(contents).ok()?.version
}

pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    let path = &registry_path(host)?;
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

    // The version first, off a shape that is only the version, and before the
    // whole document is read as a Registry.
    //
    // This is the order the guard needs to be any use. A newer Perch is exactly
    // the thing that writes a value this build has no variant for — a Strategy
    // it added, a Quarantine reason — and reading the document first fails on
    // that with serde's own words. The user is then told `registry.json` is not
    // valid JSON, about a file that is perfectly valid JSON, which is precisely
    // the misdiagnosis the version field exists to prevent.
    if let Some(version) = version_of(&contents)
        && version > CURRENT_VERSION
    {
        return Err(crate::error::written_by_a_newer_perch(
            &path.display().to_string(),
            "registry",
            version,
            CURRENT_VERSION,
        ));
    }

    let registry: Registry =
        serde_json::from_str(&contents).map_err(|err| PerchError::Malformed {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;

    // Group configuration is checked on the way in rather than where it is
    // read, because the thing that reads it is a loop nobody is watching: a
    // value that means nothing would otherwise sit in the file until the
    // watcher next went round and surprise somebody by acting on it.
    for (name, config) in &registry.groups {
        config.validate(name)?;
    }

    Ok(Some(registry))
}

/// Writes the registry, under the hold the caller took to read it.
///
/// The hold is asked for rather than assumed, and it is the reason this
/// signature is the shape it is: the invariant the whole module turns on is
/// that a registry is only ever written by the Perch that read it, and a
/// parameter is the only way to say that once rather than in every caller.
///
/// It is also the one place every command reliably passes through after however
/// long its work took, which makes it where the hold is renewed. A hold that
/// was lost — Perch stalled past the staleness window and another `perch` took
/// the lock over and ran a whole command under it — is a hold whose registry is
/// behind the one on disk, and writing it back would revert that command
/// wholesale. So it is refused, and the caller says what that cost.
pub fn save(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &Registry) -> Result<()> {
    perch.renew();
    if !perch.still_held() {
        return Err(PerchError::Other(
            "Another `perch` took the registry lock over while this command was \
             working, and has changed the registry since this one read it. \
             Nothing was written, because writing would have undone whatever it \
             did. Run this command again."
                .to_string(),
        ));
    }

    let path = registry_path(host)?;
    let body = serde_json::to_string_pretty(registry)
        .map_err(|err| PerchError::Other(format!("could not serialise the registry: {err}")))?;
    write(host, &path, &format!("{body}\n"))
}

/// Replaces the registry in one step, or not at all, and for its owner alone.
///
/// **In one step**, with the same care `.claude.json` gets and for a sharper
/// reason: this file is the whole of Perch's state, and every command reads it
/// before it does anything. A truncate-then-write leaves a window in which a
/// reader — `perch status`, which is advertised for shell prompts and may run
/// several times a minute — sees half a file and reports it as malformed; and a
/// crash inside that window leaves it half-written for good, with no command
/// able to run until somebody edits it by hand.
///
/// **For its owner alone**, because the registry holds no Credential but holds
/// everything else: every Account's email address, organization, plan, Alias,
/// Group and Quarantine reason, and the Utilization history behind them. That
/// is a full picture of somebody's Anthropic relationships, and the Profile
/// directories it sits beside are already 0700 (ADR 0020) — this file was the
/// gap. A `~/.config/perch` that already exists keeps the mode it has, as `mkdir -p`
/// does everywhere else in Perch, but the file is replaced on every save and so
/// comes back narrow from the first one.
fn write(host: &dyn Host, path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        host.create_private_dir_all(parent).map_err(|err| {
            PerchError::Other(format!("could not create {}: {err}", parent.display()))
        })?;
    }
    host.write_private_file(path, contents)
        .map_err(|err| PerchError::file_write(path, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn an_email_slugs_to_a_stable_directory_name() {
        assert_eq!(slug("Someone@Example.com"), "someone-example-com");
    }

    /// `join("")` gives back the path it was joined to, so an address with
    /// nothing nameable in it would put an Account's Profile *at* the
    /// directory holding every other Account's — and `perch remove` deletes a
    /// Profile directory whole.
    #[test]
    fn an_address_that_names_no_directory_is_refused_rather_than_naming_them_all() {
        let host = crate::host::FakeHost::new();
        let profiles = profiles_dir(&host).unwrap();

        for degenerate in ["@", "-", "...", "@.-@"] {
            assert_eq!(slug(degenerate), "", "the case this is about: {degenerate}");
            let refused =
                profile_dir_for(&host, degenerate).expect_err("no Profile can be named after this");
            assert!(refused.to_string().contains(degenerate), "{refused}");
        }

        assert_eq!(
            profile_dir_for(&host, "someone@example.com").unwrap(),
            profiles.join("someone-example-com"),
            "and an ordinary address is unaffected"
        );
    }

    /// Every command is a load, some changes made in memory and a save of the
    /// whole thing, so two of them overlapping does not merge — it discards.
    #[test]
    fn one_perch_changes_the_registry_at_a_time() {
        let host = crate::host::FakeHost::new();

        let held = lock(&host).expect("the first Perch takes it");
        let refused = match lock(&host) {
            Err(refused) => refused,
            Ok(_) => panic!("the second Perch must wait, then give up"),
        };
        assert!(
            refused.to_string().contains("the Perch registry lock"),
            "{refused}"
        );

        drop(held);
        lock(&host).expect("a lock given back can be taken again");
    }

    /// The hold spans the whole command, and a command can stall for as long as
    /// somebody takes to answer a `[y/N]`. Renewed at every write, so the
    /// ordinary long command keeps the lock it took rather than letting it
    /// expire silently underneath itself.
    #[test]
    fn a_command_that_takes_its_time_keeps_the_lock_it_took() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // Comfortably past the staleness window, several times over: exactly
        // the shape of a `perch remove` waiting on somebody who walked away.
        for _ in 0..4 {
            host.sleep(REGISTRY_STALE_MILLIS as u64 - 10_000);
            save(&host, &mut perch, &Registry::default()).expect("it is still Perch's to write");
        }

        assert!(perch.still_held());
        assert!(
            lock(&host).is_err(),
            "and no other Perch could have taken it in the meantime"
        );
    }

    /// The other direction, and the reason the hold is checked rather than
    /// assumed: a Perch that did stall past the window has had its lock taken
    /// over, and the registry it read before that is however many commands out
    /// of date. Writing it back would revert every one of them wholesale — so
    /// the write is refused, and the user is told to run the command again.
    #[test]
    fn a_registry_read_before_somebody_elses_command_is_not_written_over_theirs() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // The stall, and another Perch finding the lock abandoned and taking it.
        host.sleep(REGISTRY_STALE_MILLIS as u64 + 1_000);
        let theirs = lock(&host).expect("an abandoned lock is taken over");
        save(&host, &mut { theirs }, &Registry::default()).expect("theirs is the live hold");
        let before = load(&host).expect("it reads").expect("they wrote one");

        let stale = Registry {
            active: Some("someone@example.com".into()),
            ..Registry::default()
        };
        let refused = save(&host, &mut perch, &stale).expect_err("this one may no longer write");

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

    /// Perch's home holds Profile directories full of Credentials, and on a
    /// fresh machine the *lock* is what brings it into being — before any
    /// registry has been written into it. Created privately there rather than
    /// at whatever the umask happens to be, or the narrow modes below it guard
    /// files inside a directory anybody may walk into.
    #[test]
    fn the_home_the_lock_creates_is_the_owners_alone() {
        let host = crate::host::FakeHost::new();

        let _perch = lock(&host).expect("the registry lock is free");

        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE)
        );
    }

    /// The registry is the whole of Perch's state and every command reads it
    /// first, so a write that stops half way must not be visible: a reader
    /// would call the file malformed, and a crash inside that window would
    /// leave it malformed for good.
    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.config/perch/registry.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_unwritable_file(path, "No space left on device (os error 28)");

        let registry = Registry {
            active: Some("someone@example.com".into()),
            ..Registry::default()
        };
        let mut perch = lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &registry).expect_err("the write cannot land");

        assert_eq!(
            host.file(path).as_deref(),
            Some(before.as_str()),
            "a reader still sees the registry that was there"
        );
        assert_eq!(
            host.file(crate::host::temp_beside(Path::new(path))),
            None,
            "and the half-written copy is not left beside it"
        );
    }

    /// No Credential, but every Account's address, organization, plan, Alias,
    /// Group, Quarantine reason and Utilization history — a full picture of
    /// somebody's Anthropic relationships, beside Profile directories that are
    /// already 0700.
    #[test]
    fn the_registry_is_written_for_its_owner_alone() {
        let host = crate::host::FakeHost::new();

        let mut perch = lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &Registry::default()).expect("it is written");

        let path = registry_path(&host).unwrap();
        assert_eq!(host.mode_of(&path), Some(crate::host::PRIVATE_FILE_MODE));
        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
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
            enabled: true,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.active = Some("someone@example.com".into());

        let json = serde_json::to_string(&registry).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
        assert_eq!(back.active_account().unwrap().plan.as_deref(), Some("pro"));
    }

    /// What a scheduled Check leaves for the next one, and nothing where none
    /// has Switched: a key in every registry on every machine would be a
    /// promise the watcher had run, which for nearly all of them it has not.
    #[test]
    fn what_a_check_recorded_survives_the_file_and_is_absent_until_one_switches() {
        let mut registry = Registry::default();
        assert!(
            !serde_json::to_string(&registry).unwrap().contains("checks"),
            "a machine nothing has been scheduled on records no checks"
        );

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_check("work", "someone@example.com", at);
        let back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();

        let recorded = back.checked("work").expect("the Group it Switched within");
        assert_eq!(recorded.switched_at, at);
        assert_eq!(recorded.switched_off, "someone@example.com");
        assert_eq!(
            back.checked("personal"),
            None,
            "a cooldown is a Group's, and a Switch within one says nothing \
             about how soon another may move"
        );
    }

    /// A Group that is gone paces nothing, and a record kept past it would be a
    /// cooldown inherited by a Group declared under the same name later.
    #[test]
    fn forgetting_a_group_forgets_what_a_check_recorded_against_it() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.record_check(
            "work",
            "someone@example.com",
            Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
        );

        registry.forget_group("work");

        assert_eq!(registry.checked("work"), None);
    }

    /// Not so that anything can be migrated — nothing is running this yet, so
    /// there is nothing to migrate from. It is there so a build that
    /// understands less than the file it is handed says so.
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
            enabled: true,
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
            enabled: true,
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
        assert!(!Registry::default().global.cycle_ungrouped);
        let says_nothing_about_it: Registry =
            serde_json::from_str(r#"{"version":1}"#).expect("a registry with no settings in it");
        assert!(
            !says_nothing_about_it.global.cycle_ungrouped,
            "a registry that says nothing about it reads as off, not as a \
             declaration nobody made (ADR 0017)"
        );
    }

    #[test]
    fn a_new_group_leaves_the_watcher_alone_until_asked() {
        let config = GroupConfig::default();
        assert!(!config.watcher_may_act);
        assert_eq!(config.strategy, Strategy::MostHeadroom);
        assert_eq!(
            config.watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT
        );
    }

    /// The three the watcher's policy grew (ADR 0013). Asserted as the numbers
    /// rather than as the constants, because a default is a promise made in a
    /// README and a test that reads the constant back cannot notice it change.
    #[test]
    fn the_watchers_policy_has_the_defaults_it_is_documented_with() {
        let config = GroupConfig::default();
        assert_eq!(config.watcher_threshold_percent, 80);
        assert_eq!(config.watcher_cooldown_minutes, 15);
        assert_eq!(config.watcher_margin_percent, 10);
        assert!(
            config.watcher_no_return,
            "coming straight back to the Account just left is the ping-pong the \
             policy exists to stop, so it is barred unless somebody says otherwise"
        );
    }

    /// A number a setting cannot hold is refused with the ones it can — from a
    /// hand-edited registry as much as from a mistyped command, because both
    /// readers have the same next question.
    #[test]
    fn a_number_out_of_range_is_refused_with_the_range() {
        let cases: [(GroupConfig, &str, &str); 3] = [
            (
                GroupConfig {
                    watcher_threshold_percent: 101,
                    ..GroupConfig::default()
                },
                "watcher-threshold-percent",
                "100",
            ),
            (
                GroupConfig {
                    watcher_margin_percent: 101,
                    ..GroupConfig::default()
                },
                "watcher-margin-percent",
                "100",
            ),
            (
                GroupConfig {
                    watcher_cooldown_minutes: MAX_WATCHER_COOLDOWN_MINUTES + 1,
                    ..GroupConfig::default()
                },
                "watcher-cooldown-minutes",
                "10080",
            ),
        ];

        for (config, key, accepted) in cases {
            let refusal = config.validate("work").expect_err("out of range");
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

        assert!(GroupConfig::default().validate("work").is_ok());
    }

    /// A margin at or over the threshold is not out of range — it is a Group
    /// that will only move to an Account with nothing used at all, which is a
    /// coherent thing to ask for even if few would. Refusing it would make the
    /// order two `set`s are typed in matter.
    #[test]
    fn a_margin_larger_than_the_threshold_is_a_strict_policy_rather_than_a_refusal() {
        let config = GroupConfig {
            watcher_threshold_percent: 50,
            watcher_margin_percent: 90,
            ..GroupConfig::default()
        };
        assert!(config.validate("work").is_ok());
    }

    #[test]
    fn a_name_that_would_be_ambiguous_is_refused_whichever_half_it_is_for() {
        for kind in [NameKind::Alias, NameKind::Group] {
            for name in [
                "",
                " ",
                "none",
                "None",
                " work",
                "work ",
                "someone@example.com",
            ] {
                assert!(
                    validate_name(kind, name).is_err(),
                    "`{name}` should not be usable as a {kind:?} name"
                );
            }
            for name in ["work", "Overflow Ltd", "personal-2"] {
                assert!(validate_name(kind, name).is_ok(), "`{name}` should be fine");
            }
        }
    }

    #[test]
    fn naming_an_account_that_is_already_named_replaces_the_name() {
        let mut registry = Registry::default();
        registry.set_alias("overflow", "someone@example.com");
        registry.set_alias("work", "someone@example.com");

        assert_eq!(registry.alias_of("someone@example.com"), Some("work"));
        assert!(!registry.aliases.contains_key("overflow"));
        assert_eq!(
            registry.unset_alias("Work"),
            Some(("work".to_string(), "someone@example.com".to_string())),
            "a name is freed however it is capitalised, and says how it was held"
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

    /// The machine-readable name of a Quarantine, which is what `--json` puts in
    /// front of a script (`reason`, beside the prose `detail`). Every kind needs
    /// one, and no two may share it: a script branching on why an Account is
    /// Quarantined branches on this string and nothing else.
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

    /// `PERCH_HOME` moves everything Perch keeps, which is what lets a test —
    /// or a second Perch on one machine — run without touching the real one.
    /// It passes through verbatim: no `.config/perch` is appended to it.
    #[test]
    fn perch_home_is_taken_from_the_environment_verbatim_when_it_is_set() {
        let host = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("PERCH_HOME", "/tmp/somewhere-else");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else")
        );
        assert_eq!(
            registry_path(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else/registry.json"),
            "and everything under it moves with it"
        );
    }

    #[test]
    fn without_the_override_perch_keeps_its_registry_under_the_config_directory() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/Users/someone/.config/perch")
        );
    }

    /// A registry that is not there is a Perch that holds nothing, and a
    /// registry that is there and will not be read is a failure. Saying "no
    /// Accounts" for the second would be a Perch that quietly forgot everything
    /// the moment a permission went wrong.
    #[test]
    fn a_registry_that_cannot_be_read_is_a_failure_rather_than_an_empty_perch() {
        let absent = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        assert_eq!(
            load(&absent).expect("a machine Perch has never run on holds nothing"),
            None
        );

        let path = registry_path(&absent).unwrap();
        let unreadable = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_file(&path, "{}")
            .with_unreadable_file(&path, "permission denied");

        let failed = load(&unreadable).expect_err("a registry that is there must be readable");
        let said = failed.to_string();
        assert!(said.contains("permission denied"), "{said}");
        assert!(
            said.contains(&path.display().to_string()),
            "and it names the file: {said}"
        );
    }
}
