//! Putting a Credential into a Profile of its own.
//!
//! Both ways an Account enters Perch end here: adoption copies the existing
//! login in (ADR a-login-perch-does-not-need), and `add` copies in the login it
//! just created. Neither knows where a directory keeps its Credential — that is
//! [`crate::probe`] and [`crate::credentials`] — and both get the same
//! read-back guard for free.

use std::path::Path;

use crate::credentials::{self, CredentialStore};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Store};

/// Makes a Profile's directory, if it is not already there.
///
/// One copy of it, because both paths that bring a Profile into being — a login
/// stored here, and a Reconcile that needs somewhere to put a link — owe the
/// same mode (ADR claude-code-chooses-the-store).
pub fn make_dir(host: &dyn Host, dir: &Path) -> Result<()> {
    host.create_private_dir_all(dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))
}

/// Creates `dir` and stores `credential` where the Claude Code on this machine
/// would keep it, returning the store that now holds it.
pub fn create(host: &dyn Host, dir: &Path, credential: &str) -> Result<Store> {
    // Asked before the directory is made, which is the only moment the answer
    // is knowable.
    let made_here = !host.path_exists(dir);
    make_dir(host, dir)?;

    let store = probe::store_for_profile(host, dir)?;
    if let Err(error) = store_credential(host, &store, credential) {
        // Here rather than at the callers, all three of which open their undo
        // with the Store this hands back — so this failure is outside every one
        // of them, and what it leaves nothing walks. Only a directory this made.
        if made_here {
            discard(host, &store);
        }
        return Err(error);
    }
    Ok(store)
}

/// Writes a Credential into a Profile's Store and reads it back before trusting
/// it. Every write of a Credential goes through here.
///
/// **A Live Profile is not written into**, and the caller checks that under
/// whatever lock it writes (ADR a-profile-is-live-by-evidence).
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
                // so a copy surviving there would win over what was just
                // written. Safe now and not before: this one has been read back.
                supersede_or_fail(host, &primary, &fallback)?;
                Ok(())
            }
            // Either store may now be sitting on a value that is neither the
            // Credential asked for nor the one it replaced, which is worse than
            // nothing: a reader hands Claude Code bytes it cannot parse.
            Err(fallback_failed) => {
                discard_a_bad_copy(host, &primary, &primary_failed);
                discard_a_bad_copy(host, &fallback, &fallback_failed);
                // The primary's failure is the one to report: it is the store
                // this machine was supposed to be using.
                Err(primary_failed.error)
            }
        },
    }
}

/// Takes out a copy the store accepted and then read back as something else.
///
/// Only a copy known to be bad: a write that failed outright left whatever the
/// store held before, which is the best Credential there is. Best-effort, and
/// what it leaves is a Profile with no Credential — a Quarantine, said in words.
fn discard_a_bad_copy(host: &dyn Host, store: &CredentialStore, why: &NotKept) {
    if !why.holds_a_bad_copy {
        return;
    }
    host.note(&format!(
        "{} was left holding a Credential that did not read back intact, so it \
         was removed rather than left for Claude Code to find.",
        store.describe()
    ));
    if store.forget(host).is_err() {
        host.note(&format!(
            "That copy could not be removed from {}.",
            store.describe()
        ));
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

/// The same, in the direction where a copy left behind is a wrong answer.
///
/// Only a store that still hands a Credential back is a failure — one that will
/// not answer leaves a copy that cannot win, because the read that would prefer
/// it fails the same way.
fn supersede_or_fail(
    host: &dyn Host,
    preferred: &CredentialStore,
    written: &CredentialStore,
) -> Result<()> {
    let Err(refused) = preferred.forget(host) else {
        return Ok(());
    };
    if !matches!(preferred.read(host), Ok(Some(_))) {
        host.note(&format!(
            "A superseded copy of a Credential could not be removed from {}.",
            preferred.describe()
        ));
        return Ok(());
    }
    Err(refused.with_note(&format!(
        "The Credential was written to {}, but the copy it replaces is still in \
         {} — which is the store read first, so it is the one Claude Code would \
         go on using. Empty it and run this again.",
        written.describe(),
        preferred.describe(),
    )))
}

/// A write that did not end with the store holding the Credential.
///
/// The two are not the same state on disk: a store that refused the write still
/// holds what it held before, and one that took it and read back as something
/// else is holding bytes nothing wrote on purpose.
struct NotKept {
    error: PerchError,
    holds_a_bad_copy: bool,
}

fn write_and_read_back(
    host: &dyn Host,
    kept_in: &CredentialStore,
    credential: &str,
) -> std::result::Result<(), NotKept> {
    kept_in.write(host, credential).map_err(|error| NotKept {
        error,
        holds_a_bad_copy: false,
    })?;

    let read_back = kept_in.read(host).map_err(|error| NotKept {
        error,
        // Accepted, and the store will not say what it holds. Treated as bad:
        // removing a good copy costs a Quarantine that says how to repair it,
        // and keeping a bad one costs a Profile nothing explains.
        holds_a_bad_copy: true,
    })?;

    if read_back.as_ref().map(|held| held.as_str()) != Some(credential) {
        // Reported as a failure of the store it happened in, so the exit code a
        // script branches on still says which half of the machine to look at.
        let error = match kept_in {
            CredentialStore::Keychain { .. } => PerchError::KeychainUnavailable(format!(
                "the Credential written to {} did not read back intact",
                kept_in.describe()
            )),
            CredentialStore::Plaintext { path } => {
                PerchError::file_write(path.clone(), "the Credential did not read back intact")
            }
        };
        return Err(NotKept {
            error,
            holds_a_bad_copy: true,
        });
    }
    Ok(())
}

/// Forgets a Profile: its Credential, wherever it is, and its directory.
///
/// Best-effort about the *error*, not the order: the directory goes last because
/// it is the only thing that can still name the store, so a store that refuses
/// keeps it, and the remark says why.
pub fn discard(host: &dyn Host, store: &Store) {
    let mut still_holding = Vec::new();
    for kept_in in credentials::stores_for(host, store) {
        if kept_in.forget(host).is_err() {
            still_holding.push(kept_in.describe());
        }
    }

    if !still_holding.is_empty() {
        host.note(&format!(
            "{} would not give up the Credential it holds for {}, so that \
             directory was left where it is: it is the only thing that can \
             still name the store. `perch holdings purge` empties it, and the \
             next `perch add` reaps it.",
            still_holding.join(" and "),
            store.config_dir.display(),
        ));
        return;
    }

    let _ = host.remove_dir_all(&store.config_dir);
}
