//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned from the first commit, because later specs add Groups, Aliases and
//! Quarantine to the same file and will have to migrate what is already there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{self, Host, HostError};
use crate::lock;
use crate::probe::{Identity, LockSpec};

/// The version this build writes. A registry from the future is refused rather
/// than silently misread.
///
/// Version 2 stopped recording where an Account's Credential is kept: a store
/// is derived from the Profile's path rather than written down, so a registry
/// can no longer disagree with the derivation (ADR 0020).
///
/// Version 3 records *why* an Account is Quarantined rather than only that it
/// is. A reason is not decoration: it is the difference between an Account the
/// user can act on and one that is broken for reasons nobody wrote down.
pub const CURRENT_VERSION: u32 = 3;

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
    /// A Quarantine a Perch that did not record reasons left behind. Only ever
    /// read, never written: a registry from before reasons still says the
    /// Account is broken rather than quietly reading as healthy.
    #[serde(rename = "unrecorded")]
    Unrecorded,
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
            Quarantine::Unrecorded => {
                "its Credential could no longer be used, and Perch did not record reasons \
                 when it found out"
            }
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
            Quarantine::Unrecorded => "unrecorded",
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
    /// Absent is the ordinary case, so it is left out of the file entirely: a
    /// registry an older Perch reads back still says every healthy Account is
    /// healthy.
    #[serde(
        default,
        alias = "quarantined",
        deserialize_with = "quarantine_as_recorded",
        skip_serializing_if = "Option::is_none"
    )]
    pub quarantine: Option<Quarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<CachedUtilization>,
}

fn enabled_by_default() -> bool {
    true
}

/// Reads a Quarantine however the registry that holds it spelled one.
///
/// Before version 3 this was a flag, so a registry Perch wrote earlier says
/// `false` where it now says nothing and `true` where it now names a reason.
/// Both still have to load: a registry that will not parse is worse than one
/// whose oldest entry cannot say why.
fn quarantine_as_recorded<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Quarantine>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Recorded {
        Flag(bool),
        Reason(Quarantine),
    }

    Ok(match Option::<Recorded>::deserialize(deserializer)? {
        None | Some(Recorded::Flag(false)) => None,
        Some(Recorded::Flag(true)) => Some(Quarantine::Unrecorded),
        Some(Recorded::Reason(reason)) => Some(reason),
    })
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

/// What a Group carries besides its Accounts: the rules that would govern
/// Cycling within it (ADR 0002).
///
/// v1 stores and validates these and reads none of them. They are here from the
/// first Group rather than added later, so a Group written today is a Group the
/// watcher can be pointed at without migrating anything.
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
}

impl Default for GroupConfig {
    fn default() -> Self {
        GroupConfig {
            strategy: Strategy::default(),
            watcher_may_act: false,
            watcher_threshold_percent: DEFAULT_WATCHER_THRESHOLD_PERCENT,
        }
    }
}

impl GroupConfig {
    /// Refuses configuration that cannot mean what it says. Serde already
    /// refuses a strategy Perch does not implement; what is left is the range
    /// a percentage has to be in.
    pub fn validate(&self, group: &str) -> Result<()> {
        if self.watcher_threshold_percent > 100 {
            return Err(PerchError::Invalid(format!(
                "Group `{group}` has a watcher threshold of {}, but a Utilization threshold is a percentage between 0 and 100.",
                self.watcher_threshold_percent
            )));
        }
        Ok(())
    }
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
    name.eq_ignore_ascii_case(NO_GROUP)
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
            .find(|declared| declared.eq_ignore_ascii_case(name))
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
    pub fn forget_group(&mut self, name: &str) {
        self.groups.remove(name);
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
            .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
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
            && alias.eq_ignore_ascii_case(group)
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

/// `$PERCH_HOME`, or `~/.perch` — an error when neither is knowable, because
/// a machine that cannot say where home is gets a refusal rather than a
/// registry written into the filesystem root.
pub fn perch_home(host: &dyn Host) -> Result<PathBuf> {
    if let Some(overridden) = host.env_var("PERCH_HOME") {
        return Ok(PathBuf::from(overridden));
    }
    let home = host
        .home_dir()
        .map_err(|err| PerchError::Other(err.to_string()))?;
    Ok(home.join(".perch"))
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
pub fn lock(host: &dyn Host) -> Result<lock::Held<'_>> {
    lock::take_all(host, vec![lock_spec(host)?])
}

/// Reads the registry, or `None` when Perch has never run here.
pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    let path = registry_path(host)?;
    let contents = match host.read_file(&path) {
        Ok(contents) => contents,
        Err(HostError::NotFound { .. }) => return Ok(None),
        Err(err) => {
            return Err(PerchError::Other(format!(
                "could not read {}: {err}",
                path.display()
            )));
        }
    };

    let mut registry: Registry =
        serde_json::from_str(&contents).map_err(|err| PerchError::Malformed {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;

    if registry.version > CURRENT_VERSION {
        return Err(PerchError::Other(format!(
            "{} was written by a newer Perch (registry version {}, this build understands {CURRENT_VERSION}). Upgrade Perch.",
            path.display(),
            registry.version
        )));
    }

    // A version 1 registry recorded each Account's Profile directory and
    // keychain namespace. All three are derived now, and serde has already
    // dropped them on the way in; the version follows so the next save says
    // what the file actually holds (ADR 0020).
    registry.version = CURRENT_VERSION;

    adopt_groups_only_the_accounts_record(&mut registry);

    // Group configuration is checked on the way in rather than where it is
    // read, because in v1 nothing reads it: a value that means nothing would
    // otherwise sit in the file until the watcher shipped and then surprise
    // someone by acting on it.
    for (name, config) in &registry.groups {
        config.validate(name)?;
    }

    Ok(Some(registry))
}

/// Declares every Group an Account is in but that nothing declared.
///
/// A Perch that predates `perch group` recorded a Group only on the Accounts
/// that joined it. Left alone, such a Group would be one the user has plainly
/// got — their Accounts are in it — that `perch group list` cannot show and
/// `perch group remove` says does not exist. It picks up the default
/// configuration, which is what it would have been created with.
fn adopt_groups_only_the_accounts_record(registry: &mut Registry) {
    let claimed: Vec<String> = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.clone())
        .collect();
    for name in claimed {
        registry.groups.entry(name).or_default();
    }
}

pub fn save(host: &dyn Host, registry: &Registry) -> Result<()> {
    let path = registry_path(host)?;
    let body = serde_json::to_string_pretty(registry)
        .map_err(|err| PerchError::Other(format!("could not serialise the registry: {err}")))?;
    write(host, &path, &format!("{body}\n"))
}

/// Replaces the registry in one step, or not at all.
///
/// The same care `.claude.json` gets, and for a sharper reason: this file is
/// the whole of Perch's state, and every command reads it before it does
/// anything. A truncate-then-write leaves a window in which a reader — `perch
/// status`, which is advertised for shell prompts and may run several times a
/// minute — sees half a file and reports it as malformed; and a crash inside
/// that window leaves it half-written for good, with no command able to run
/// until somebody edits it by hand.
fn write(host: &dyn Host, path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        host.create_dir_all(parent).map_err(|err| {
            PerchError::Other(format!("could not create {}: {err}", parent.display()))
        })?;
    }
    host::write_atomically(host, path, contents).map_err(|err| PerchError::FileWrite {
        path: path.to_path_buf(),
        source: std::io::Error::other(err.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The registry is the whole of Perch's state and every command reads it
    /// first, so a write that stops half way must not be visible: a reader
    /// would call the file malformed, and a crash inside that window would
    /// leave it malformed for good.
    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.perch/registry.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_unwritable_file(path, "No space left on device (os error 28)");

        let registry = Registry {
            active: Some("someone@example.com".into()),
            ..Registry::default()
        };
        save(&host, &registry).expect_err("the write cannot land");

        assert_eq!(
            host.file(path).as_deref(),
            Some(before.as_str()),
            "a reader still sees the registry that was there"
        );
        assert_eq!(
            host.file(format!("{path}.perch-tmp")),
            None,
            "and the half-written copy is not left beside it"
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

    #[test]
    fn the_version_is_recorded_so_later_specs_can_migrate() {
        let json = serde_json::to_string(&Registry::default()).unwrap();
        assert!(json.contains(&format!("\"version\":{CURRENT_VERSION}")));
    }

    #[test]
    fn a_quarantine_a_registry_recorded_as_a_flag_still_reads_as_broken() {
        let earlier = r#"{
          "version": 2,
          "accounts": [
            {"identity": {"email": "broken@example.com"}, "quarantined": true},
            {"identity": {"email": "fine@example.com"}, "quarantined": false}
          ]
        }"#;

        let registry: Registry =
            serde_json::from_str(earlier).expect("a registry Perch wrote before reasons existed");

        assert_eq!(
            registry.account("broken@example.com").unwrap().quarantine,
            Some(Quarantine::Unrecorded),
            "an Account a flag said was broken is still broken, and says as much \
             about why as the flag did"
        );
        assert!(!registry.account("fine@example.com").unwrap().quarantined());
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
        let before_the_setting_existed: Registry =
            serde_json::from_str(r#"{"version":2}"#).expect("a registry Perch wrote earlier");
        assert!(
            !before_the_setting_existed.global.cycle_ungrouped,
            "a registry written before the setting existed reads as off, not as \
             a declaration nobody made (ADR 0017)"
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

    #[test]
    fn a_threshold_that_is_not_a_percentage_is_refused() {
        let config = GroupConfig {
            watcher_threshold_percent: 101,
            ..GroupConfig::default()
        };
        let message = config.validate("work").unwrap_err().to_string();
        assert!(
            message.contains("work") && message.contains("100"),
            "{message}"
        );
        assert!(GroupConfig::default().validate("work").is_ok());
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
}
