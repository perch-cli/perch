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

/// What a placement that cannot finish does with whatever landed before it
/// stopped.
pub enum IfItFails {
    /// Exactly what this placement made comes back out ([`Placed::take_back`]).
    TakeBack,
    /// Everything stays: the write went over what the Profile held before, so
    /// there is no old copy to put back, and taking the fresh one out would
    /// leave less than the caller started with. A directory this placement made
    /// and put nothing into still goes — nobody had it.
    KeepWhatLanded,
}

/// One Profile a placement has written into, and exactly what it made there:
/// the ledger bounds the undo — take back only what this write made, and only
/// if it made it. A Profile nothing records holds a live refresh token that
/// `reap_abandoned` never walks, so a caller whose record fails owes this a
/// [`Placed::take_back`].
#[derive(Debug)]
pub struct Placed {
    store: Store,
    /// Whether the Profile's directory was already on the machine: a
    /// `perch add` that died at the browser step, or a Purge that could not
    /// empty a store, leaves a directory the registry never named.
    was_already_there: bool,
    /// Whether this placement wrote a Credential into that store. A Quarantined
    /// Account travels with none, and forgetting a store this never wrote to
    /// would destroy a refresh token nothing here put there.
    wrote_a_credential: bool,
    /// Whether this placement wrote the `.claude.json`, set the same way and
    /// for the same reason.
    wrote_the_identity_file: bool,
}

/// Creates `dir` if it has to and puts a login into it: the Credential where
/// the Claude Code on this machine would keep it, and the `.claude.json`
/// beside it. Either may be absent — a Quarantined Account travels with no
/// Credential, and an adoption may find no identity block to carry.
pub fn place(
    host: &dyn Host,
    dir: &Path,
    credential: Option<&str>,
    identity_file: Option<&str>,
    if_it_fails: IfItFails,
) -> Result<Placed> {
    // Asked before the directory is made, which is the only moment the answer
    // is knowable.
    let was_already_there = host.path_exists(dir);
    make_dir(host, dir)?;
    let store = match probe::store_for_profile(host, dir) {
        Ok(store) => store,
        Err(error) => {
            // No store to speak of yet, so the only thing to take back is a
            // directory this just made — empty, whatever the policy says.
            if !was_already_there {
                let _ = host.remove_dir_all(dir);
            }
            return Err(error);
        }
    };

    let mut placed = Placed {
        store,
        was_already_there,
        wrote_a_credential: false,
        wrote_the_identity_file: false,
    };
    if let Some(credential) = credential {
        if let Err(error) = store_credential(host, &placed.store, credential) {
            // Asked rather than inferred from the `Err`: `store_credential`
            // refuses when the store read first will not give up the copy it
            // replaces, and the Credential is in the other one by then.
            placed.wrote_a_credential = landed(host, &placed.store, credential);
            placed.did_not_finish(host, if_it_fails);
            return Err(error);
        }
        placed.wrote_a_credential = true;
    }
    if let Some(contents) = identity_file {
        if let Err(error) = carry_identity_file(host, contents, &placed.store) {
            placed.did_not_finish(host, if_it_fails);
            return Err(error);
        }
        placed.wrote_the_identity_file = true;
    }
    Ok(placed)
}

impl Placed {
    /// The Store the Profile keeps its Credential in.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Takes back what this placement *made*, best-effort.
    ///
    /// Made rather than written into: a Profile directory nothing names
    /// outlives every command that would have named it — on macOS, the only
    /// name reaching a live Credential.
    pub fn take_back(&self, host: &dyn Host) {
        if !self.was_already_there {
            discard(host, &self.store);
            return;
        }
        // The directory stays and neither thing written into it does: a
        // `.claude.json` holds an API key in an MCP server's `env` block, so
        // taking it back prevents as much as taking the Credential back.
        if self.wrote_a_credential {
            for kept_in in credentials::stores_for(host, &self.store) {
                let _ = kept_in.forget(host);
            }
        }
        if self.wrote_the_identity_file {
            let _ = host.remove_file(&self.store.identity_file);
        }
        let taken_back = match (self.wrote_a_credential, self.wrote_the_identity_file) {
            (true, true) => "The Credential and the `.claude.json`",
            (true, false) => "The Credential",
            // Whatever the store holds belongs to whoever left the directory
            // behind.
            (false, true) => "The `.claude.json`",
            // Nothing of this placement's landed, so the directory is exactly
            // as its owner left it.
            (false, false) => return,
        };
        host.note(&format!(
            "{} was already on this machine, so it was left where it is rather \
             than removed with what was placed here. {taken_back} written into \
             it has been taken back out.",
            self.store.config_dir.display(),
        ));
    }

    /// What a placement that stopped does with its ledger, per [`IfItFails`].
    fn did_not_finish(&self, host: &dyn Host, if_it_fails: IfItFails) {
        match if_it_fails {
            IfItFails::TakeBack => self.take_back(host),
            IfItFails::KeepWhatLanded => {
                if !self.was_already_there
                    && !self.wrote_a_credential
                    && !self.wrote_the_identity_file
                {
                    discard(host, &self.store);
                }
            }
        }
    }
}

/// Whether a store holds the Credential this placement was writing into it.
///
/// A store that will not answer says nothing, and the undo leaves it: what it
/// might hold is a Credential this placement did not put there.
fn landed(host: &dyn Host, store: &Store, carried: &str) -> bool {
    credentials::read(host, store)
        .ok()
        .flatten()
        .is_some_and(|held| *held.credential == *carried)
}

/// Keeps the `.claude.json` in the Profile the Account settles into: the
/// Identity travels with the Credential it describes.
///
/// Through the same write `switch` patches the Default Profile's copy with,
/// which is what creates the file closed rather than at the process umask.
fn carry_identity_file(host: &dyn Host, contents: &str, store: &Store) -> Result<()> {
    crate::host::write_atomically(host, &store.identity_file, contents)
        .map_err(|err| PerchError::file_write(store.identity_file.clone(), err))
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
/// A lock is for a spell, so a store that will not say whether it still holds
/// the superseded copy is refused: whatever is behind the lock wins every read
/// after it opens. A store holding nothing under this name says so through one.
fn supersede_or_fail(
    host: &dyn Host,
    preferred: &CredentialStore,
    written: &CredentialStore,
) -> Result<()> {
    let Err(refused) = preferred.forget(host) else {
        return Ok(());
    };
    let said = match preferred.read(host) {
        // It says so itself, whatever it was holding before.
        Ok(None) => None,
        Err(_) => Some(format!(
            "The Credential was written to {}, but {} would not say whether it \
             still holds the one it replaces, and that is the store read first. \
             Open it and run this again.",
            written.describe(),
            preferred.describe(),
        )),
        Ok(Some(_)) => Some(format!(
            "The Credential was written to {}, but the copy it replaces is still \
             in {}, which is the store read first, so it is the one Claude Code \
             would go on using. Empty it and run this again.",
            written.describe(),
            preferred.describe(),
        )),
    };
    match said {
        Some(said) => Err(refused.with_note(&said)),
        None => {
            host.note(&format!(
                "A superseded copy of a Credential could not be removed from {}.",
                preferred.describe()
            ));
            Ok(())
        }
    }
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
