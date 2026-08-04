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
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            active: None,
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
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

    /// Every Group any Account is in. Groups have no existence apart from the
    /// Accounts that claim them until `perch group` lands.
    pub fn group_names(&self) -> BTreeSet<&str> {
        self.accounts
            .iter()
            .filter_map(|account| account.group.as_deref())
            .collect()
    }

    /// The Alias an Account answers to, if it has been given one.
    pub fn alias_of(&self, email: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, target)| *target == email)
            .map(|(alias, _)| alias.as_str())
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

    let registry: Registry =
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

    Ok(Some(registry))
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
}
