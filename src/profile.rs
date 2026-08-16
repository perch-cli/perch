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

/// Makes a Profile's directory, if it is not already there.
///
/// Private from the moment it exists: off macOS the Credential is a file in
/// here, and a directory others may enter is a directory whose contents others
/// may open (ADR 0020). Created that way rather than tightened, because a
/// Profile is Perch's to make and there is no window to leave.
///
/// One copy of it, because both paths that bring a Profile into being — a login
/// stored here, and a Reconcile that needs somewhere to put a link — owe the
/// same mode, and two copies is how one of them comes to be created at the
/// umask.
pub fn make_dir(host: &dyn Host, dir: &Path) -> Result<()> {
    host.create_private_dir_all(dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))
}

/// Creates `dir` and stores `credential` where the Claude Code on this machine
/// would keep it, returning the store that now holds it.
pub fn create(host: &dyn Host, dir: &Path, credential: &str) -> Result<Store> {
    make_dir(host, dir)?;

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
///
/// There is a fourth thing a write owes that this cannot check, and a caller
/// must: **a Live Profile is not written into**. Something else is holding that
/// Credential, and replacing it is the mid-task logout ADR 0005 exists to
/// prevent. The question is [`crate::probe::live_clients`], and it has to be
/// asked of the Profile being written *under whatever lock the write is taken
/// under* — an answer from before a browser round trip or a lock wait is an
/// answer about a machine that has since had minutes to move on
/// ([`crate::switch::refuse_if_live_anywhere`] is the shape of asking it twice).
///
/// It is not enforced here because this function cannot tell the callers apart:
/// three of the eight paths write into a Profile something could be running
/// against and ask first, and five are bringing a Profile into being, where
/// there is nothing to be running yet and the question is vacuous. A parameter
/// saying which would be a caller asserting it rather than being held to it,
/// which is a comment with a type around it — so it is a comment.
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
                //
                // And it *fails* here, where the other direction only remarks.
                // A copy that loses to what was just written is untidy; a copy
                // that beats it means every later read hands back the
                // Credential this call replaced — so returning `Ok` would be
                // reporting a Capture that did not take effect, which is the
                // one thing a Capture must never do quietly. Reachable rather
                // than theoretical: off macOS the plaintext file is the
                // primary, and a Profile directory that is read-only fails the
                // write and the removal alike.
                supersede_or_fail(host, &primary, &fallback)?;
                Ok(())
            }
            // Both refused, and either may be sitting on a value that is
            // neither the Credential asked for nor the one it replaced —
            // `security`'s stdin buffer truncates mid-argument without saying
            // so (ADR 0008), which is the whole reason for the read-back. Left
            // there it is worse than nothing: a reader hands Claude Code bytes
            // it cannot parse, and every retry now fails a step earlier than
            // this one, with no way back but deleting the item by hand.
            //
            // Only a copy known to be bad is removed. A write that failed
            // outright left the store holding the Credential it had before,
            // which is the best one there is and not this function's to throw
            // away.
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
/// Best-effort, like [`supersede`], and for the same reason: this runs on a path
/// that is already failing, and the error worth reporting is the one that got
/// here. What it leaves behind is a Profile with no Credential — which is a
/// Quarantine, said in words, with `perch relogin` as the way out.
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

/// The same, in the direction where a copy left behind may not be untidiness
/// but a wrong answer: the store still holding the retired Credential is the
/// one [`credentials::read`] consults first, so a copy that survives there wins
/// over the one just written and the Profile goes on handing out what this call
/// was replacing.
///
/// *May*, and this is the whole of the check: a removal that failed because the
/// store will not answer at all — a locked keychain, which is the ordinary
/// reason the fallback was written in the first place — leaves a copy that
/// cannot win, because the read that would prefer it fails the same way. So the
/// store is asked, and only a store that still hands a Credential back is a
/// failure. Reported rather than remarked there, because the caller's next act
/// is to believe the write; a store that will not answer either way is the
/// remark it always was.
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

/// A write that did not end with the store holding the Credential, and what
/// that leaves behind.
///
/// The two ways it fails are not the same state on disk, and a caller cleaning
/// up has to tell them apart: a store that refused the write still holds
/// whatever it held before, and a store that took the write and read back as
/// something else is holding bytes nothing wrote on purpose.
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
        // The write was accepted and the store will not say what it holds, so
        // whether it is the Credential is exactly what is not known. Treated as
        // bad, because the cost of removing a good copy here is a Quarantine
        // that says how to repair it, and the cost of keeping a bad one is a
        // Profile nothing can use and nothing explains.
        holds_a_bad_copy: true,
    })?;

    if read_back.as_deref() != Some(credential) {
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

/// Forgets a Profile entirely: its stored Credential, wherever it is, and its
/// directory.
///
/// Best-effort by design. This runs on the failure path of `add`, where the
/// interesting error is the one that got us here, not the tidying up.
///
/// Best-effort about the *error*, though, and not about the order. The
/// directory goes only once every store has given its Credential up, because on
/// macOS a Credential Store is a keychain item outside the directory whose
/// service name is derived from it — so removing the directory first destroys
/// the only thing that could ever name the item again, exactly as
/// `purge::forget_what_the_registry_does_not_name` says. A keychain locked
/// while `perch add` was running would otherwise leave a live refresh token
/// under a hash of a path that no longer exists, for an Account the user
/// believes was never added, invisible to every later reap and Purge.
///
/// So a store that refuses keeps the directory, and the remark says why. What
/// is left behind is a Profile nothing names, which `perch holdings purge`
/// walks and which the next `perch add` can reap — untidy, and recoverable,
/// which is the side of the trade to be on.
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
