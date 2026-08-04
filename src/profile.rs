//! Putting a Credential into a Profile of its own.
//!
//! Both ways an Account enters Perch end here: adoption copies the existing
//! login in (ADR 0009), and `add` copies in the login it just created. Neither
//! knows how a directory becomes a keychain namespace — that stays in
//! [`crate::probe`] — and both get the same read-back guard for free.

use std::path::Path;

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe;
use crate::registry::Profile;

/// Creates `dir` and stores `credential` in the keychain namespace its path
/// derives, returning the Profile that now holds it.
pub fn create(host: &dyn Host, dir: &Path, credential: &str) -> Result<Profile> {
    host.create_dir_all(dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    let store = probe::store_for_profile(host, dir)?;
    store_credential(
        host,
        &store.keychain_service,
        &store.keychain_account,
        credential,
    )?;

    Ok(Profile {
        dir: dir.to_path_buf(),
        keychain_service: store.keychain_service,
        keychain_account: store.keychain_account,
    })
}

/// Writes a Credential into a keychain namespace and reads it back before
/// trusting it.
///
/// `security`'s stdin buffer truncates mid-argument without saying so (ADR
/// 0008), and a Credential that was truncated on the way in is indistinguishable
/// from one that is simply wrong when it is next used — which would be at the
/// worst moment, some Switch later. Every write of a Credential goes through
/// here, so no path can forget the read-back.
pub fn store_credential(
    host: &dyn Host,
    service: &str,
    account: &str,
    credential: &str,
) -> Result<()> {
    host.keychain_set(service, account, credential)?;

    let stored = host.keychain_get(service, account)?;
    if stored != credential {
        return Err(PerchError::KeychainUnavailable(format!(
            "the Credential written to {service} did not read back intact"
        )));
    }
    Ok(())
}

/// Forgets a Profile entirely: its stored Credential and its directory.
///
/// Best-effort by design. This runs on the failure path of `add`, where the
/// interesting error is the one that got us here, not the tidying up.
pub fn discard(host: &dyn Host, profile: &Profile) {
    let _ = host.keychain_delete(&profile.keychain_service, &profile.keychain_account);
    let _ = host.remove_dir_all(&profile.dir);
}
