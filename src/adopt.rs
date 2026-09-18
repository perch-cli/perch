//! Initial discovery and installation of native Defaults (ADR a-login-perch-does-not-need).

use crate::providers::provider::{InstallMode, catalog};
use crate::registry::{Account, Registry};
use crate::{Host, PerchError, Result, holdings, registry, say};

pub fn ensure_adopted(host: &dyn Host) -> Result<Registry> {
    for provider in catalog() {
        provider.maintain(host);
    }
    if let Some(registry) = registry::load(host)? {
        return Ok(registry);
    }
    let mut held = holdings::lock(host)?;
    load_or_adopt(host, &mut held)
}

pub fn ensure_adopted_exclusively(host: &dyn Host) -> Result<(crate::lock::Held<'_>, Registry)> {
    for provider in catalog() {
        provider.maintain(host);
    }
    let mut held = holdings::lock(host)?;
    let registry = load_or_adopt(host, &mut held)?;
    Ok((held, registry))
}

fn load_or_adopt(host: &dyn Host, held: &mut crate::lock::Held<'_>) -> Result<Registry> {
    if let Some(registry) = registry::load(host)? {
        return Ok(registry);
    }
    let mut registry = Registry::default();
    let mut profiles = Vec::new();
    let mut notices = Vec::new();
    let result: Result<()> = (|| {
        for provider in catalog() {
            let configured = provider.configured(host)?;
            if !configured.enabled() {
                continue;
            }
            let installation = match configured.installation(host) {
                Ok(installation) => installation,
                Err(PerchError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            let Some(authenticated) = installation.discover(host)? else {
                continue;
            };
            let account = Account {
                storage_key: None,
                provider: provider.id(),
                provider_identity: authenticated.subject().clone(),
                identity: authenticated.identity().clone(),
                plan: authenticated.plan().clone(),
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            };
            let applied = provider.install(
                host,
                &account.profile(host)?,
                &authenticated,
                InstallMode::New,
            )?;
            registry.select_provider(provider.id());
            registry.settle(Some(account.key().to_string()));
            notices.push(format!(
                "Adopted the {} login as {}.",
                provider.name(),
                say::described(
                    &account.identity.email,
                    account.identity.organization_name.as_deref(),
                    account.plan.as_deref()
                ),
            ));
            registry.upsert(account);
            profiles.push(applied);
        }
        if !registry.accounts.is_empty() {
            registry::save(host, held, &mut registry)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut cleanup = crate::providers::provider::Cleanup::default();
        for applied in profiles.into_iter().rev() {
            cleanup.record(applied.rollback());
        }
        return Err(match cleanup.result() {
            Ok(()) => error,
            Err(cleanup) => error.with_note(&cleanup.to_string()),
        });
    }
    if !registry.accounts.is_empty() {
        for applied in profiles {
            applied.commit();
        }
        for notice in notices {
            host.note(&notice);
        }
        // Once, here: adoption is the one moment Perch knows the person is new.
        host.note(
            "`perch wizard` walks you through adding Accounts, Groups, Settings and the Watcher.",
        );
    }
    registry.select_provider(Default::default());
    Ok(registry)
}
