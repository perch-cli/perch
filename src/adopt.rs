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
use crate::login;
use crate::probe::{self, Findings, Store, Verdict};
use crate::profile;
use crate::registry::{self, Account, Registry};

/// Loads the registry, adopting the existing login the first time Perch runs.
///
/// Anything worth telling the user about adoption is written to `out` before
/// the command that asked for it renders anything.
///
/// For the commands that only *read* the registry, and for the two that spend a
/// browser login before they change anything. A command that is going to write
/// wants [`ensure_adopted_exclusively`] instead.
pub fn ensure_adopted(host: &dyn Host, out: &mut dyn Write) -> Result<Registry> {
    login::reap_abandoned(host);
    if let Some(registry) = registry::load(host)? {
        return Ok(registry);
    }
    // The adoption itself writes, so it is done with the other Perches shut out
    // — two of them adopting at once would each build the first Profile and one
    // would overwrite the other's registry wholesale.
    let _perch = registry::lock(host)?;
    load_or_adopt(host, out)
}

/// The registry, with every other Perch shut out of it until the returned hold
/// is dropped.
///
/// What a command that is going to change something asks for, and the reason
/// the hold comes back rather than staying inside: the span that has to be
/// exclusive is the whole load → change → save, not the save. A command holding
/// a copy read before somebody else's `perch switch` writes that copy back
/// afterwards and reverts them (see [`registry::lock`]).
pub fn ensure_adopted_exclusively<'a>(
    host: &'a dyn Host,
    out: &mut dyn Write,
) -> Result<(crate::lock::Held<'a>, Registry)> {
    login::reap_abandoned(host);
    let held = registry::lock(host)?;
    let registry = load_or_adopt(host, out)?;
    Ok((held, registry))
}

/// The registry, adopting the existing login if there is none. The lock is the
/// caller's to have taken — this is the half of adoption that writes.
fn load_or_adopt(host: &dyn Host, out: &mut dyn Write) -> Result<Registry> {
    match registry::load(host)? {
        Some(registry) => Ok(registry),
        None => adopt(host, out),
    }
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
    let dir = registry::profile_dir_for(host, &findings.identity.email)?;
    let store = profile::create(host, &dir, findings.credential.as_str())?;
    carry_the_identity_block(host, findings, &store)?;

    let mut registry = Registry::default();
    registry.upsert(Account {
        identity: findings.identity.clone(),
        plan: findings.credential.subscription_type.clone(),
        enabled: true,
        quarantine: None,
        group: None,
        utilization: None,
    });
    registry.active = Some(findings.identity.email.clone());

    registry::save(host, &registry)?;
    Ok(registry)
}

/// Keeps the `oauthAccount` block Claude Code wrote for the adopted Account in
/// that Account's own Profile.
///
/// A Profile holds an Account's Credential and how Claude Code describes it,
/// which is what `perch add` gives every Profile it creates. Adoption is the
/// only other way an Account arrives, and the file it would copy is right there
/// — the login being adopted is the one that file describes. Without it, the
/// Account everybody starts with is the one that comes back from a Switch
/// described only by the four fields Perch itself records.
fn carry_the_identity_block(host: &dyn Host, findings: &Findings, store: &Store) -> Result<()> {
    let contents = match host.read_file(&findings.store.identity_file) {
        Ok(contents) => contents,
        // The probe read an Identity out of this file moments ago, so this is a
        // file that has gone away underneath us rather than one that was never
        // there. Adoption still holds the Credential, which is the part that
        // cannot be reconstructed.
        Err(_) => return Ok(()),
    };
    let Some(block) = probe::oauth_account_block(&contents) else {
        return Ok(());
    };

    let kept = store.identity_file.clone();
    host.write_file(&kept, &probe::fresh_identity_file(block))
        .map_err(|err| PerchError::FileWrite {
            path: kept,
            source: std::io::Error::other(err.to_string()),
        })
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
