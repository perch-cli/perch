//! Adoption: the existing login becomes the first Profile
//! (ADR a-login-perch-does-not-need).
//!
//! On whichever command runs first, and once for the Holdings it begins — only a
//! Purge undoes it, and the next command after one adopts again.

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::login;
use crate::probe::{self, Findings, Store, Verdict};
use crate::profile;
use crate::registry::{self, Account, Registry};

/// Loads the registry, adopting the existing login the first time Perch runs.
///
/// For the commands that only *read* the registry, and for the two that spend a
/// browser login before they change anything. A command that is going to write
/// wants [`ensure_adopted_exclusively`].
pub fn ensure_adopted(host: &dyn Host) -> Result<Registry> {
    login::reap_abandoned(host);
    if let Some(registry) = registry::load(host)? {
        return Ok(registry);
    }
    // The adoption itself writes, so it is done with the other Perches shut out
    // — two of them adopting at once would each build the first Profile and one
    // would overwrite the other's registry wholesale.
    let mut perch = registry::lock(host)?;
    load_or_adopt(host, &mut perch)
}

/// The registry, with every other Perch shut out of it until the returned hold
/// is dropped.
///
/// The hold comes back rather than staying inside because the span that has to
/// be exclusive is the whole load → change → save (see [`registry::lock`]).
pub fn ensure_adopted_exclusively(host: &dyn Host) -> Result<(crate::lock::Held<'_>, Registry)> {
    login::reap_abandoned(host);
    let mut held = registry::lock(host)?;
    let registry = load_or_adopt(host, &mut held)?;
    Ok((held, registry))
}

/// The registry, adopting the existing login if there is none. The lock is the
/// caller's to have taken — this is the half of adoption that writes.
fn load_or_adopt(host: &dyn Host, perch: &mut crate::lock::Held<'_>) -> Result<Registry> {
    match registry::load(host)? {
        Some(registry) => Ok(registry),
        None => adopt(host, perch),
    }
}

fn adopt(host: &dyn Host, perch: &mut crate::lock::Held<'_>) -> Result<Registry> {
    let findings = match probe::probe(host, registry::the_default_profile(host)?)? {
        Verdict::Recognized(findings) => findings,
        Verdict::NoLogin { version, .. } => {
            // Nothing to adopt, and nothing worth writing: an empty Profile
            // would only be a second thing to explain later.
            return Err(PerchError::NotFound(format!(
                "No Claude Code login found (Claude Code {version}).\n\
                 Run `claude` and log in, then run Perch again."
            )));
        }
    };

    let registry = store_as_first_profile(host, perch, &findings)?;
    report(host, &findings);
    Ok(registry)
}

fn store_as_first_profile(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    findings: &Findings,
) -> Result<Registry> {
    let dir = registry::profile_dir_for(host, &findings.identity.email)?;
    let store = profile::create(host, &dir, findings.credential.as_str())?;

    // Undone if it fails, because a Profile nothing records is worse than none:
    // it holds a copy of the live Credential that no registry names and that
    // `reap_abandoned` never walks, since that only walks `pending/`.
    let made = (|| {
        carry_the_identity_block(host, findings, &store)?;

        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: findings.identity.clone(),
            plan: findings.credential.subscription_type.clone(),
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.settle(Some(findings.identity.email.clone()));

        registry::save(host, perch, &mut registry)?;
        Ok(registry)
    })();

    if made.is_err() {
        profile::discard(host, &store);
    }
    made
}

/// Keeps the `oauthAccount` block Claude Code wrote for the adopted Account in
/// that Account's own Profile.
///
/// Without it, the Account everybody starts with is the one a Switch describes
/// by the four fields Perch records rather than in Claude Code's own terms.
fn carry_the_identity_block(host: &dyn Host, findings: &Findings, store: &Store) -> Result<()> {
    let contents = match host.read_file(&findings.store.identity_file) {
        Ok(contents) => zeroize::Zeroizing::new(contents),
        // The probe read an Identity out of this file moments ago, so it has
        // gone away underneath us. Adoption still holds the Credential, which
        // is the part that cannot be reconstructed.
        Err(_) => return Ok(()),
    };
    let Some(block) = probe::oauth_account_block(&contents) else {
        return Ok(());
    };

    let kept = store.identity_file.clone();
    // The write `login::carry_identity_file` uses, for the reason written there:
    // a Profile's `.claude.json` is created closed rather than at the umask
    // (ADR claude-code-chooses-the-store).
    crate::host::write_atomically(host, &kept, &probe::fresh_identity_file(block))
        .map_err(|err| PerchError::file_write(kept, err))
}

/// Says what was adopted, so somebody can confirm Perch picked up the right
/// Account before trusting it with anything.
///
/// A remark rather than output: this is news about the machine, and two of the
/// callers that reach adoption render JSON on the stream it would land on.
fn report(host: &dyn Host, findings: &Findings) {
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

    host.note(&format!(
        "Adopted the Claude Code login as your first Profile: {description}"
    ));
    host.note(&format!(
        "It is now the active Account. Claude Code {}.",
        findings.version
    ));
}
