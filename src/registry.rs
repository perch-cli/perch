//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned from the first commit, because later specs add Groups, Aliases and
//! Quarantine to the same file and will have to migrate what is already there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::probe::Identity;

/// The version this build writes. A registry from the future is refused rather
/// than silently misread.
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

/// Cached Utilization for one Account. Never fetched by `status` (ADR 0015).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedUtilization {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<WindowUtilization>,
}

/// Where an Account's Credential lives: a directory Claude Code would accept as
/// its whole configuration, and the keychain namespace derived from its path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub dir: PathBuf,
    pub keychain_service: String,
    pub keychain_account: String,
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
    pub profile: Profile,
    /// Whether the Account is a Cycle candidate. Later specs toggle this.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// An Account whose Credential can no longer be recovered.
    #[serde(default)]
    pub quarantined: bool,
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
/// Account has (ADR 0012) — and differ only in what they do with it. Stored and
/// validated here; nothing consumes it until ranking lands.
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

/// The word that addresses "no Group at all" on `perch group move`, and the
/// answer `perch add` accepts to the Group it offers. A Group cannot be called
/// this, because then one of the two meanings would be unreachable.
pub const NO_GROUP: &str = "none";

/// Whether a name is the one that means no Group at all.
pub fn means_no_group(name: &str) -> bool {
    name.eq_ignore_ascii_case(NO_GROUP)
}

/// Refuses a Group name that could not be told from something else.
///
/// A Group name shares one namespace with Aliases and is a valid target for
/// `switch` and `run`, so it has to be distinguishable from the other things a
/// target can be: an email address, and the word that means no Group at all.
pub fn validate_group_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(PerchError::Invalid("A Group needs a name.".to_string()));
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
            "`{name}` looks like an email address, and a target that could be either a Group or an Account has no single answer."
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
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            active: None,
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
            groups: BTreeMap::new(),
        }
    }
}

impl Account {
    pub fn email(&self) -> &str {
        &self.identity.email
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

    /// Declares a Group, refusing a name that is not usable or already means
    /// something else.
    pub fn declare_group(&mut self, name: &str) -> Result<()> {
        validate_group_name(name)?;
        if self.groups.contains_key(name) {
            return Err(PerchError::Conflict(format!(
                "There is already a Group called `{name}`."
            )));
        }
        self.refuse_taken_names(None, Some(name))?;
        self.groups.insert(name.to_string(), GroupConfig::default());
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name
    /// a Group in passing rather than to create one — `perch add --group`.
    pub fn ensure_group(&mut self, name: &str) -> Result<()> {
        if self.groups.contains_key(name) {
            return Ok(());
        }
        self.declare_group(name)
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

    /// Refuses an Alias and a Group name that would not both be free.
    ///
    /// Aliases and Group names share one namespace, so neither can shadow the
    /// other and the single target argument on `switch` and `run` always has
    /// one answer. The pair is checked together as well as against what is
    /// already held: a command that sets both at once could otherwise plant
    /// the collision it is meant to prevent.
    pub fn refuse_taken_names(&self, alias: Option<&str>, group: Option<&str>) -> Result<()> {
        if let (Some(alias), Some(group)) = (alias, group)
            && alias == group
        {
            return Err(PerchError::Conflict(format!(
                "`{alias}` cannot be both an Alias and a Group name."
            )));
        }

        if let Some(alias) = alias {
            if let Some(target) = self.aliases.get(alias) {
                return Err(PerchError::Conflict(format!(
                    "`{alias}` already names {target}."
                )));
            }
            if self.group_names().contains(alias) {
                return Err(PerchError::Conflict(format!(
                    "`{alias}` is already a Group name, and a name cannot be both."
                )));
            }
        }

        if let Some(group) = group
            && let Some(target) = self.aliases.get(group)
        {
            return Err(PerchError::Conflict(format!(
                "`{group}` is already an Alias for {target}, and a name cannot be both."
            )));
        }

        Ok(())
    }

    /// Names an Account, having established the name is free.
    pub fn set_alias(&mut self, alias: &str, email: &str) {
        self.aliases.insert(alias.to_string(), email.to_string());
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

/// `$PERCH_HOME`, or `~/.perch`.
pub fn perch_home(host: &dyn Host) -> PathBuf {
    host.env_var("PERCH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| host.home_dir().join(".perch"))
}

pub fn registry_path(host: &dyn Host) -> PathBuf {
    perch_home(host).join("registry.json")
}

pub fn profiles_dir(host: &dyn Host) -> PathBuf {
    perch_home(host).join("profiles")
}

/// The Profile directory for an Account. The email is slugged because the path
/// is hashed into a keychain service name and has to be stable and printable.
pub fn profile_dir_for(host: &dyn Host, email: &str) -> PathBuf {
    profiles_dir(host).join(slug(email))
}

/// Where a login lives while Perch is running it.
///
/// A Profile is named after the Account it holds, and which Account that is
/// only becomes knowable once the login has finished — so the login happens
/// here and its Credential is moved into a Profile afterwards. Nothing outlives
/// the command: this directory is removed whether the login worked or not.
pub fn pending_login_dir(host: &dyn Host, started_at: DateTime<Utc>) -> PathBuf {
    perch_home(host)
        .join("pending")
        .join(format!("login-{}", started_at.timestamp_millis()))
}

pub fn slug(email: &str) -> String {
    let slugged: String = email
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slugged.trim_matches('-').to_string()
}

/// Reads the registry, or `None` when Perch has never run here.
pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    let path = registry_path(host);
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
    let path = registry_path(host);
    let body = serde_json::to_string_pretty(registry)
        .map_err(|err| PerchError::Other(format!("could not serialise the registry: {err}")))?;
    write(host, &path, &format!("{body}\n"))
}

fn write(host: &dyn Host, path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        host.create_dir_all(parent).map_err(|err| {
            PerchError::Other(format!("could not create {}: {err}", parent.display()))
        })?;
    }
    host.write_file(path, contents)
        .map_err(|err| PerchError::FileWrite {
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
            profile: Profile {
                dir: PathBuf::from("/Users/someone/.perch/profiles/someone-example-com"),
                keychain_service: "Claude Code-credentials-abcd1234".into(),
                keychain_account: "someone".into(),
            },
            enabled: true,
            quarantined: false,
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
        assert!(json.contains("\"version\":1"));
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
    fn a_group_name_that_would_be_ambiguous_is_refused() {
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
                validate_group_name(name).is_err(),
                "`{name}` should not be usable as a Group name"
            );
        }
        for name in ["work", "Overflow Ltd", "personal-2"] {
            assert!(validate_group_name(name).is_ok(), "`{name}` should be fine");
        }
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
