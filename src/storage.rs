//! The configuration manifest and provider runtime records (ADR a-fresh-provider-layout).

use crate::providers::provider::{Id, catalog};
use crate::{Host, PerchError, Result, config, holdings, registry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FORMAT: &str = "perch-config";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderSettings {
    pub enabled: bool,
    pub cli_path: Option<PathBuf>,
    #[serde(deserialize_with = "crate::json::unique_map")]
    pub options: BTreeMap<String, Value>,
}
impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cli_path: None,
            options: BTreeMap::new(),
        }
    }
}
pub fn provider_defaults() -> BTreeMap<Id, ProviderSettings> {
    catalog()
        .iter()
        .map(|p| (p.id(), ProviderSettings::default()))
        .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    global: Global,
    #[serde(default = "provider_defaults")]
    #[serde(deserialize_with = "crate::json::unique_map")]
    providers: BTreeMap<Id, ProviderSettings>,
    #[serde(default)]
    scope_defaults: config::PolicyDefaults,
    #[serde(default)]
    #[serde(deserialize_with = "crate::json::unique_map")]
    groups: BTreeMap<String, config::ScopeSettings>,
    #[serde(default)]
    ungrouped: Ungrouped,
    #[serde(default)]
    #[serde(deserialize_with = "crate::json::unique_map")]
    accounts: BTreeMap<String, DirectoryAccount>,
    #[serde(default)]
    next_group_id: u64,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Global {
    run: Run,
    watcher: Watcher,
}
#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Run {
    default_provider: Id,
    fallback: Fallback,
}
impl Default for Run {
    fn default() -> Self {
        Self {
            default_provider: Id::default(),
            fallback: Fallback::Installed,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Fallback {
    Installed,
    Disabled,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Watcher {
    paused: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Ungrouped {
    interchangeable: bool,
    cycle: config::CycleOverrides,
    watcher: config::WatcherOverrides,
    #[serde(deserialize_with = "crate::json::unique_map")]
    providers: BTreeMap<Id, config::ProviderScope>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryAccount {
    provider: Id,
    identity: crate::domain::Identity,
    position: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_identity: Option<crate::providers::provider::AccountIdentity>,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    group: Option<String>,
    enabled: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    version: u32,
    active: registry::Active,
    #[serde(deserialize_with = "crate::json::unique_map")]
    checks: BTreeMap<String, registry::Checked>,
    #[serde(deserialize_with = "crate::json::unique_map")]
    accounts: BTreeMap<String, Health>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    quarantine: Option<registry::Quarantine>,
    utilization: Option<registry::CachedUtilization>,
}

fn read<T: serde::de::DeserializeOwned>(host: &dyn Host, path: &Path) -> Result<Option<T>> {
    match host.read_file(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| PerchError::Malformed {
                path: path.display().to_string(),
                detail: error.to_string(),
            }),
        Err(crate::host::HostError::NotFound { .. }) => Ok(None),
        Err(error) => Err(PerchError::file_read(path, error)),
    }
}
fn manifest(host: &dyn Host) -> Result<Option<Manifest>> {
    let home = holdings::perch_home(host)?;
    let path = home.join("config.json");
    let text = match host.read_file(&path) {
        Ok(text) => Some(text),
        Err(crate::host::HostError::NotFound { .. }) => None,
        Err(error) => return Err(PerchError::file_read(&path, error)),
    };
    let Some(text) = text else {
        if ["registry.json", "providers.json", "metadata.pending.json"]
            .iter()
            .any(|name| host.path_exists(&home.join(name)))
        {
            return Err(unsupported());
        }
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&text).map_err(|error| PerchError::Malformed {
        path: path.display().to_string(),
        detail: error.to_string(),
    })?;
    if let Some(version) = value.get("version").and_then(Value::as_u64)
        && version > u64::from(registry::CURRENT_VERSION)
    {
        return Err(crate::error::written_by_a_newer_perch(
            "This configuration",
            "Registry",
            version,
            registry::CURRENT_VERSION,
        ));
    }
    if value.get("format").and_then(Value::as_str) != Some(FORMAT)
        || value.get("version").and_then(Value::as_u64)
            != Some(u64::from(registry::CURRENT_VERSION))
    {
        return Err(unsupported());
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| PerchError::Malformed {
            path: path.display().to_string(),
            detail: error.to_string(),
        })
}
fn unsupported() -> PerchError {
    PerchError::Invalid("This Perch configuration uses an unsupported layout. Move the old configuration directory aside and start with a fresh installation; this build does not migrate it.".into())
}
pub fn provider_settings(host: &dyn Host, id: Id) -> Result<ProviderSettings> {
    Ok(manifest(host)?
        .and_then(|mut config| config.providers.remove(&id))
        .unwrap_or_default())
}

pub fn load(host: &dyn Host) -> Result<Option<registry::Registry>> {
    let Some(config) = manifest(host)? else {
        return Ok(None);
    };
    let mut result = registry::Registry {
        version: config.version,
        run_provider: config.global.run.default_provider,
        run_fallback: matches!(config.global.run.fallback, Fallback::Installed),
        watcher_paused: config.global.watcher.paused,
        provider_settings: config.providers,
        scope_defaults: config.scope_defaults,
        groups: config.groups,
        next_group_id: config.next_group_id,
        ungrouped: config::UngroupedConfig {
            interchangeable: config.ungrouped.interchangeable,
            settings: config::ScopeSettings {
                cycle: config.ungrouped.cycle,
                watcher: config.ungrouped.watcher,
                providers: config.ungrouped.providers,
                ..Default::default()
            },
        },
        ..Default::default()
    };
    let mut accounts: Vec<_> = config.accounts.into_iter().collect();
    accounts.sort_by_key(|(_, account)| account.position);
    if accounts
        .windows(2)
        .any(|pair| pair[0].1.position == pair[1].1.position)
    {
        return Err(PerchError::Invalid(
            "Two Accounts occupy the same position in the configuration".into(),
        ));
    }
    for (id, account) in accounts {
        if let Some(alias) = account.alias
            && result.aliases.insert(alias.clone(), id.clone()).is_some()
        {
            return Err(PerchError::Invalid(format!(
                "The Alias `{alias}` names more than one Account"
            )));
        }
        let account = registry::Account {
            storage_key: Some(id.clone()),
            provider: account.provider,
            provider_identity: account.provider_identity,
            identity: account.identity,
            plan: account.plan,
            group: account.group,
            disabled: !account.enabled,
            quarantine: None,
            utilization: None,
        };
        if account.key() != id {
            return Err(PerchError::Invalid(
                "An Account identity disagrees with its directory key".into(),
            ));
        }
        result.accounts.push(account);
    }
    result = registry::readable(result)?;
    for provider in catalog() {
        let id = provider.id();
        let Some(runtime) = read::<Runtime>(host, &id.home(host)?.join("state.json"))? else {
            continue;
        };
        if runtime.version != registry::CURRENT_VERSION {
            return Err(unsupported());
        }
        let active = prune_active(&result, id, runtime.active)?;
        let checks = runtime
            .checks
            .into_iter()
            .filter_map(|(scope_id, value)| {
                let name = if scope_id == "ungrouped" {
                    Some(crate::name::UNGROUPED.to_string())
                } else {
                    result
                        .groups
                        .keys()
                        .find(|name| result.scope_id(name) == scope_id)
                        .cloned()
                };
                name.map(|name| (name, value))
            })
            .collect();
        result
            .runtime
            .insert(id, registry::ProviderState::from_parts(active, checks));
        for (key, health) in runtime.accounts {
            if let Some(account) = result.account_mut(&key)
                && account.provider() == id
            {
                account.quarantine = health.quarantine;
                account.utilization = health.utilization;
            }
        }
    }
    registry::readable(result).map(Some).map_err(|error| {
        error.with_note(&registry::the_file_to_edit(
            &holdings::registry_path(host).unwrap_or_default(),
        ))
    })
}

fn prune_active(
    registry: &registry::Registry,
    provider: Id,
    active: registry::Active,
) -> Result<registry::Active> {
    let referenced: Vec<&str> = match &active {
        registry::Active::Nobody => vec![],
        registry::Active::Settled(id) => vec![id],
        registry::Active::Landing { leaving, arriving } => leaving
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(arriving.as_str()))
            .collect(),
    };
    for key in referenced {
        if registry
            .account(key)
            .is_some_and(|account| account.provider() != provider)
        {
            return Err(PerchError::Invalid(format!(
                "{}'s Default names an Account from another provider",
                provider.word()
            )));
        }
    }
    let held = |id: &str| {
        registry
            .account(id)
            .is_some_and(|account| account.provider() == provider)
    };
    Ok(match active {
        registry::Active::Settled(id) if !held(&id) => registry::Active::Nobody,
        registry::Active::Landing { leaving, arriving } if !held(&arriving) => {
            registry::Active::settled_on(leaving.filter(|id| held(id)))
        }
        registry::Active::Landing { leaving, arriving } => registry::Active::Landing {
            leaving: leaving.filter(|id| held(id)),
            arriving,
        },
        other => other,
    })
}

pub fn save(
    host: &dyn Host,
    held: &mut crate::lock::Held<'_>,
    registry: &registry::Registry,
) -> Result<()> {
    let manifest = Manifest {
        format: FORMAT.into(),
        version: registry::CURRENT_VERSION,
        global: Global {
            run: Run {
                default_provider: registry.run_provider,
                fallback: if registry.run_fallback {
                    Fallback::Installed
                } else {
                    Fallback::Disabled
                },
            },
            watcher: Watcher {
                paused: registry.watcher_paused,
            },
        },
        providers: registry.provider_settings.clone(),
        scope_defaults: registry.scope_defaults.clone(),
        groups: registry.groups.clone(),
        next_group_id: registry.next_group_id,
        ungrouped: Ungrouped {
            interchangeable: registry.ungrouped.interchangeable,
            cycle: registry.ungrouped.settings.cycle.clone(),
            watcher: registry.ungrouped.settings.watcher.clone(),
            providers: registry.ungrouped.settings.providers.clone(),
        },
        accounts: registry
            .accounts
            .iter()
            .enumerate()
            .map(|(position, account)| {
                (
                    account.key().to_string(),
                    DirectoryAccount {
                        position,
                        provider: account.provider(),
                        identity: account.identity.clone(),
                        provider_identity: account.provider_identity.clone(),
                        plan: account.plan.clone(),
                        alias: registry.alias_of(account.key()).map(str::to_string),
                        group: account.group.clone(),
                        enabled: !account.disabled,
                    },
                )
            })
            .collect(),
    };
    // Runtime records use stable references and remain valid on either side of a manifest rename.
    for provider in catalog() {
        let id = provider.id();
        let runtime = Runtime {
            version: registry::CURRENT_VERSION,
            active: registry.active_for(id).clone(),
            checks: registry
                .state_for(id)
                .checks
                .iter()
                .map(|(name, checked)| (registry.scope_id(name), checked.clone()))
                .collect(),
            accounts: registry
                .accounts
                .iter()
                .filter(|account| account.provider() == id)
                .map(|account| {
                    (
                        account.key().to_string(),
                        Health {
                            quarantine: account.quarantine,
                            utilization: account.utilization.clone(),
                        },
                    )
                })
                .collect(),
        };
        write_changed(host, held, &id.home(host)?.join("state.json"), &runtime)?;
    }
    write_changed(host, held, &holdings::registry_path(host)?, &manifest)
}

fn write_changed(
    host: &dyn Host,
    held: &mut crate::lock::Held<'_>,
    path: &Path,
    value: &impl Serialize,
) -> Result<()> {
    let value =
        serde_json::to_value(value).map_err(|error| PerchError::Other(error.to_string()))?;
    if read::<Value>(host, path)?.as_ref() == Some(&value) {
        return Ok(());
    }
    held.renew();
    if !held.still_held() {
        return Err(PerchError::Other(
            "The configuration lock was lost before writing".into(),
        ));
    }
    let body = serde_json::to_string_pretty(&value)
        .map_err(|error| PerchError::Other(error.to_string()))?;
    host.write_private_file(path, &body)
        .map_err(|error| PerchError::file_write(path, error))
}
