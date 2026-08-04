//! Adoption: the existing login becomes the first Profile (ADR 0009).
//!
//! Anyone installing Perch is already logged into an Account. Perch copies that
//! Credential into a Profile of its own and records the Account as active,
//! rather than asking for a login it does not need. That leaves two copies of
//! one Credential — which is exactly where every Switch leaves things (ADR
//! 0006), so adoption starts the system in its steady state rather than adding
//! a case.

use std::io::Write;

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Findings, Verdict};
use crate::registry::{self, Account, Profile, Registry};

/// Loads the registry, adopting the existing login the first time Perch runs.
///
/// Anything worth telling the user about adoption is written to `out` before
/// the command that asked for it renders anything.
pub fn ensure_adopted(host: &dyn Host, out: &mut dyn Write) -> Result<Registry> {
    if let Some(registry) = registry::load(host)? {
        return Ok(registry);
    }
    adopt(host, out)
}

fn adopt(host: &dyn Host, out: &mut dyn Write) -> Result<Registry> {
    let findings = match probe::probe(host)? {
        Verdict::Recognised(findings) => findings,
        Verdict::NoLogin { version, .. } => {
            // Nothing to adopt, and nothing worth writing: an empty Profile
            // would only be a second thing to explain later.
            return Err(PerchError::NotFound(format!(
                "No Claude Code login found (Claude Code {version}).\n\
                 Run `claude` and log in, then run Perch again."
            )));
        }
    };

    let registry = store_as_first_profile(host, &findings)?;
    report(out, &findings)?;
    Ok(registry)
}

fn store_as_first_profile(host: &dyn Host, findings: &Findings) -> Result<Registry> {
    let dir = registry::profile_dir_for(host, &findings.identity.email);
    host.create_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    let store = probe::store_for_profile(host, &dir)?;
    host.keychain_set(
        &store.keychain_service,
        &store.keychain_account,
        findings.credential.as_str(),
    )?;

    // `security`'s stdin buffer truncates mid-argument without saying so
    // (ADR 0008), so the copy is read back before it is trusted.
    let stored = host.keychain_get(&store.keychain_service, &store.keychain_account)?;
    if stored != findings.credential.as_str() {
        return Err(PerchError::KeychainUnavailable(format!(
            "the Credential written to {} did not read back intact",
            store.keychain_service
        )));
    }

    let mut registry = Registry::default();
    registry.upsert(Account {
        email: findings.identity.email.clone(),
        account_uuid: findings.identity.account_uuid.clone(),
        organization: findings.identity.organization_name.clone(),
        organization_uuid: findings.identity.organization_uuid.clone(),
        plan: findings.credential.subscription_type.clone(),
        profile: Profile {
            dir,
            keychain_service: store.keychain_service,
            keychain_account: store.keychain_account,
        },
        enabled: true,
        quarantined: false,
        group: None,
        utilization: None,
    });
    registry.active = Some(findings.identity.email.clone());

    registry::save(host, &registry)?;
    Ok(registry)
}

/// Says what was adopted, so the user can confirm Perch picked up the right
/// Account before trusting it with anything.
fn report(out: &mut dyn Write, findings: &Findings) -> Result<()> {
    let mut description = findings.identity.email.clone();
    let details: Vec<String> = [
        findings.identity.organization_name.clone(),
        findings.credential.subscription_type.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !details.is_empty() {
        description.push_str(&format!(" ({})", details.join(", ")));
    }

    writeln!(
        out,
        "Adopted the Claude Code login as your first Profile: {description}"
    )
    .and_then(|_| {
        writeln!(
            out,
            "It is now the active Account. Claude Code {}.",
            findings.version
        )
    })
    .map_err(|err| PerchError::Other(err.to_string()))?;
    writeln!(out).map_err(|err| PerchError::Other(err.to_string()))?;
    Ok(())
}
