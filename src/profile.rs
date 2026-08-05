//! Putting a Credential into a Profile of its own.
//!
//! Both ways an Account enters Perch end here: adoption copies the existing
//! login in (ADR 0009), and `add` copies in the login it just created. Neither
//! knows where a directory keeps its Credential — that is [`crate::probe`] and
//! [`crate::credentials`] — and both get the same read-back guard for free.

use std::path::Path;

use crate::credentials::{self, CredentialStore};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Store};

/// Creates `dir` and stores `credential` where the Claude Code on this machine
/// would keep it, returning the store that now holds it.
pub fn create(host: &dyn Host, dir: &Path, credential: &str) -> Result<Store> {
    // Private from the moment it exists: off macOS the Credential is a file in
    // here, and a directory others may enter is a directory whose contents
    // others may open (ADR 0020). Created that way rather than tightened,
    // because a Profile is Perch's to make and there is no window to leave.
    host.create_private_dir_all(dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    let store = probe::store_for_profile(host, dir)?;
    store_credential(host, &store, credential)?;
    Ok(store)
}

/// Writes a Credential into a Profile's Credential Store and reads it back
/// before trusting it.
///
/// Every write of a Credential goes through here, so no path can forget any of
/// the three things a write owes (ADR 0020):
///
/// - it goes to the platform's primary store, and to the other one only when
///   that fails, because a store Perch cannot write to is worse than a store it
///   would rather not use;
/// - it is read back, because `security`'s stdin buffer truncates mid-argument
///   without saying so (ADR 0008) and a truncated Credential is
///   indistinguishable from a wrong one at the worst possible moment, some
///   Switch later;
/// - the copy in the store that was *not* written is removed, or the composite
///   reader would hand a retired refresh token back to Claude Code the next
///   time it consulted that one — ADR 0006's silent poisoning, arriving by the
///   back door.
///
/// ADR 0020 states that last one of a write to the primary store, which is the
/// case that happens. It is done in both directions because what the Credential
/// Store *is* — one store holding a Profile's Credential at a time — has to
/// hold after the rarer write too, and that direction is the more dangerous of
/// the two: see [`supersede`].
pub fn store_credential(host: &dyn Host, store: &Store, credential: &str) -> Result<()> {
    let [primary, fallback] = credentials::stores_for(host, store);

    match write_and_read_back(host, &primary, credential) {
        Ok(()) => {
            supersede(host, &fallback);
            Ok(())
        }
        Err(primary_failed) => match write_and_read_back(host, &fallback, credential) {
            Ok(()) => {
                host.note(&format!(
                    "{} could not be written to, so a Credential was stored in {} instead.",
                    primary.describe(),
                    fallback.describe()
                ));
                // The store that was not written is the one a reader prefers,
                // so a copy left in it is worse here than the other way round:
                // it would win over the Credential just written. Safe to do
                // now and not before, because this one has been read back.
                supersede(host, &primary);
                Ok(())
            }
            // Both refused. The primary's failure is the one to report: it is
            // the store this machine was supposed to be using.
            Err(_) => Err(primary_failed),
        },
    }
}

/// Takes the Credential this write has replaced out of the other store.
///
/// Best-effort: the Credential is somewhere Claude Code reads by the time this
/// runs, and a copy that could not be removed is worth a remark rather than
/// failing a Switch that has already happened.
fn supersede(host: &dyn Host, other: &CredentialStore) {
    if other.forget(host).is_err() {
        host.note(&format!(
            "A superseded copy of a Credential could not be removed from {}.",
            other.describe()
        ));
    }
}

fn write_and_read_back(host: &dyn Host, kept_in: &CredentialStore, credential: &str) -> Result<()> {
    kept_in.write(host, credential)?;

    if kept_in.read(host)?.as_deref() != Some(credential) {
        // Reported as a failure of the store it happened in, so the exit code a
        // script branches on still says which half of the machine to look at.
        return Err(match kept_in {
            CredentialStore::Keychain { .. } => PerchError::KeychainUnavailable(format!(
                "the Credential written to {} did not read back intact",
                kept_in.describe()
            )),
            CredentialStore::Plaintext { path } => PerchError::FileWrite {
                path: path.clone(),
                source: std::io::Error::other("the Credential did not read back intact"),
            },
        });
    }
    Ok(())
}

/// Forgets a Profile entirely: its stored Credential, wherever it is, and its
/// directory.
///
/// Best-effort by design. This runs on the failure path of `add`, where the
/// interesting error is the one that got us here, not the tidying up.
pub fn discard(host: &dyn Host, store: &Store) {
    for kept_in in credentials::stores_for(host, store) {
        let _ = kept_in.forget(host);
    }
    let _ = host.remove_dir_all(&store.config_dir);
}
