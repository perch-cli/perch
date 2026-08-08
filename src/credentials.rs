//! The Credential Store: the two places a Credential can be, and the composite
//! that reads across them (ADR 0020).
//!
//! Claude Code keeps a Credential in the operating system's keychain on macOS
//! and in a file of JSON everywhere else, and reads from both. Perch stores one
//! exactly where the installed Claude Code would, so it has both stores too,
//! and the two halves are decided by different things:
//!
//! - the **platform** says which store is primary, because a Profile that was
//!   created a moment ago holds no evidence about anything;
//! - the **evidence** says which store is believed, because one that already
//!   exists can be looked at — which is what stops a locked keychain reading as
//!   "no login" on a machine whose Claude Code is working off its file.
//!
//! Nothing here decides *what* to write. The read-back guard and the removal of
//! a superseded copy live in [`crate::profile`], with the rest of what it means
//! to put a Credential somewhere.

use std::path::PathBuf;

use crate::error::{PerchError, Result};
use crate::host::{self, Host, HostError, Platform};
use crate::keychain::KeychainError;
use crate::probe::Store;

/// One place a Credential can be kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialStore {
    /// The operating system's keychain, reached through `/usr/bin/security`
    /// (ADR 0008). The primary on macOS, and on every other platform a store
    /// that simply never holds anything.
    Keychain { service: String, account: String },
    /// A file of JSON that nobody but its owner can read.
    Plaintext { path: PathBuf },
}

/// The two Credential Stores of one config directory, the platform's primary
/// first.
pub fn stores_for(host: &dyn Host, config: &Store) -> [CredentialStore; 2] {
    let keychain = CredentialStore::Keychain {
        service: config.keychain_service.clone(),
        account: config.keychain_account.clone(),
    };
    let plaintext = CredentialStore::Plaintext {
        path: config.credentials_file.clone(),
    };

    match host.platform() {
        Platform::MacOs => [keychain, plaintext],
        Platform::Windows | Platform::Other => [plaintext, keychain],
    }
}

/// A Credential, and which store it came out of — so a complaint about bytes
/// Perch cannot make sense of names the store they are actually in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCredential {
    pub kept_in: CredentialStore,
    pub credential: String,
}

/// The Credential a config directory holds, from whichever of its two stores
/// holds one.
///
/// The primary is asked first and believed when it answers. Anything else —
/// an item that is not there, a keychain that will not open — sends the
/// question to the other store, and only if that holds nothing either is the
/// primary's answer the one reported. A locked keychain is therefore a failure
/// when it is the whole story and a detail when it is not.
pub fn read(host: &dyn Host, config: &Store) -> Result<Option<StoredCredential>> {
    let [primary, fallback] = stores_for(host, config);
    match primary.read(host) {
        Ok(Some(credential)) => Ok(Some(StoredCredential {
            kept_in: primary,
            credential,
        })),
        otherwise => match fallback.read(host) {
            Ok(Some(credential)) => Ok(Some(StoredCredential {
                kept_in: fallback,
                credential,
            })),
            // Neither holds one. What the primary said is what happened: on a
            // machine with no keychain that is "nothing here", and on one with
            // a locked keychain it is the lock.
            Ok(None) => otherwise.map(|_| None),
            // The fallback is there and would not say what it holds. That is
            // not "no Credential": "no Credential" is terminal — it Quarantines
            // an Account (ADR 0006) — and a file that is temporarily unreadable
            // must not be promoted to a permanent verdict. If the primary
            // already failed, its failure is the one to report, since it is the
            // store this machine is meant to be using.
            Err(fallback_failed) => match otherwise {
                Ok(None) => Err(fallback_failed),
                otherwise => otherwise.map(|_| None),
            },
        },
    }
}

impl CredentialStore {
    /// What this store is called, for a message about it. Never its contents.
    pub fn describe(&self) -> String {
        match self {
            CredentialStore::Keychain { service, .. } => format!("the keychain ({service})"),
            CredentialStore::Plaintext { path } => path.display().to_string(),
        }
    }

    /// The Credential here, or `None` when this store holds none.
    pub fn read(&self, host: &dyn Host) -> Result<Option<String>> {
        match self {
            CredentialStore::Keychain { service, account } => {
                match host.keychain_get(service, account) {
                    Ok(raw) => Ok(Some(raw)),
                    Err(KeychainError::NotFound { .. }) => Ok(None),
                    Err(_) if there_is_no_keychain_here(host) => Ok(None),
                    Err(other) => Err(PerchError::from(other)),
                }
            }
            CredentialStore::Plaintext { path } => match host.read_file(path) {
                Ok(contents) => {
                    tighten_if_loose(host, path);
                    Ok(Some(contents))
                }
                Err(HostError::NotFound { .. }) => Ok(None),
                Err(err) => Err(PerchError::file_read(path.clone(), err)),
            },
        }
    }

    /// Puts a Credential here, creating whatever has to exist for it.
    pub fn write(&self, host: &dyn Host, credential: &str) -> Result<()> {
        match self {
            CredentialStore::Keychain { service, account } => {
                host.keychain_set(service, account, credential)?;
                Ok(())
            }
            CredentialStore::Plaintext { path } => host
                .write_private_file(path, credential)
                .map_err(|err| PerchError::file_write(path.clone(), err)),
        }
    }

    /// Takes the Credential out of this store, and says whether there was one.
    ///
    /// A store that held nothing is not a failure: this is how a copy that has
    /// been superseded elsewhere is removed, and it is often already gone. But
    /// it is not the same as one that gave a Credential up, and the caller is
    /// sometimes about to tell the user which happened — `perch remove` says
    /// the Credential Perch held is deleted, and the keychain item's account
    /// name is `$USER`, which is not the same today as it was under `sudo -u`,
    /// after a login rename, or in a launchd context. There, the delete is a
    /// no-op that reads as success while the keychain goes on holding a working
    /// Credential for an Account the tool just said it destroyed.
    pub fn forget(&self, host: &dyn Host) -> Result<Forgotten> {
        match self {
            CredentialStore::Keychain { service, account } => {
                match host.keychain_delete(service, account) {
                    Ok(()) => Ok(Forgotten::Credential),
                    Err(KeychainError::NotFound { .. }) => Ok(Forgotten::Nothing),
                    Err(_) if there_is_no_keychain_here(host) => Ok(Forgotten::Nothing),
                    Err(other) => Err(PerchError::from(other)),
                }
            }
            CredentialStore::Plaintext { path } => {
                let held = host.path_exists(path);
                host.remove_file(path)
                    .map_err(|err| PerchError::file_write(path.clone(), err))?;
                Ok(if held {
                    Forgotten::Credential
                } else {
                    Forgotten::Nothing
                })
            }
        }
    }
}

/// What a store gave up when it was asked to forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forgotten {
    Credential,
    /// The store held nothing to take. Ordinary for the store that was not
    /// written; news when it is the only store there was.
    Nothing,
}

/// Whether this machine has a keychain at all.
///
/// Off macOS it has not, so a keychain that will not answer is a store holding
/// nothing rather than a failure: a missing `/usr/bin/security` is not news on
/// Linux, and reporting it would turn every logged-out machine into a broken
/// one (ADR 0020).
fn there_is_no_keychain_here(host: &dyn Host) -> bool {
    host.platform() != Platform::MacOs
}

/// Narrows a credential file anybody could read, and says so.
///
/// Not a refusal. Perch did not necessarily write this file — Claude Code did,
/// or a backup restored it — and refusing would strand a machine whose Claude
/// Code is working perfectly. A tightened file is a better outcome than an
/// explained one, and [`Host::note`](crate::host::Host::note) says it once
/// however many times the file is read.
fn tighten_if_loose(host: &dyn Host, path: &std::path::Path) {
    // A mode that cannot be read is not evidence of a loose file, and this is a
    // remark made in passing on the way to reading a Credential: nothing here
    // is worth failing the command the user actually asked for.
    let Ok(Some(mode)) = host.file_mode(path) else {
        return;
    };
    if host::is_private(mode) {
        return;
    }
    // Said either way. The reasoning above — that a tightened file is a better
    // outcome than an explained one — assumes the tightening works, and when it
    // does not the user was getting neither: no error, no remark, and a
    // world-readable refresh token read on every command from then on. That is
    // the one outcome worse than an explanation, because nothing is ever going
    // to mention it again.
    match host.make_private(path) {
        Ok(()) => host.note(&format!(
            "{} held a Credential that others could read ({mode:04o}). \
             Its permissions have been narrowed to you alone.",
            path.display()
        )),
        Err(err) => host.note(&format!(
            "{} holds a Credential that others could read ({mode:04o}), and its \
             permissions could not be narrowed: {err}. \
             Anyone who can read that file can act as this Account until it is \
             `chmod 600`.",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;
    use crate::probe;

    const CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test"}}"#;

    fn profile_store(host: &FakeHost) -> Store {
        probe::store_for_profile(
            host,
            std::path::Path::new("/Users/someone/.config/perch/profiles/a"),
        )
        .expect("USER is set")
    }

    #[test]
    fn macos_writes_to_the_keychain_and_everything_else_to_a_file() {
        let mac = FakeHost::new();
        let [primary, fallback] = stores_for(&mac, &profile_store(&mac));
        assert!(matches!(primary, CredentialStore::Keychain { .. }));
        assert!(matches!(fallback, CredentialStore::Plaintext { .. }));

        let linux = FakeHost::new().with_platform(Platform::Other);
        let [primary, fallback] = stores_for(&linux, &profile_store(&linux));
        assert!(matches!(primary, CredentialStore::Plaintext { .. }));
        assert!(matches!(fallback, CredentialStore::Keychain { .. }));
    }

    #[test]
    fn the_plaintext_store_of_a_profile_is_inside_it() {
        let host = FakeHost::new();
        assert_eq!(
            profile_store(&host).credentials_file,
            std::path::Path::new("/Users/someone/.config/perch/profiles/a/.credentials.json")
        );
    }

    #[test]
    fn a_credential_is_read_from_the_store_that_holds_it() {
        let host = FakeHost::new();
        let store = profile_store(&host);
        assert!(read(&host, &store).unwrap().is_none(), "neither holds one");

        // Only the fallback has it: what a machine that was logged in by a
        // Claude Code working off its plaintext store looks like.
        host.set_file(&store.credentials_file, CREDENTIAL);
        let held = read(&host, &store)
            .unwrap()
            .expect("the fallback holds one");
        assert_eq!(held.credential, CREDENTIAL);
        assert!(matches!(held.kept_in, CredentialStore::Plaintext { .. }));

        // The primary answers, so the primary is believed.
        host.set_keychain_item(&store.keychain_service, &store.keychain_account, "primary");
        let held = read(&host, &store).unwrap().expect("the primary holds one");
        assert_eq!(held.credential, "primary");
        assert!(matches!(held.kept_in, CredentialStore::Keychain { .. }));
    }

    #[test]
    fn a_locked_keychain_is_a_failure_only_when_nothing_else_holds_a_credential() {
        let host = FakeHost::new();
        let store = profile_store(&host);
        host.lock_keychain("User interaction is not allowed");

        let error = read(&host, &store).unwrap_err();
        assert!(
            matches!(error, PerchError::KeychainUnavailable(_)),
            "with nothing anywhere, the lock is the whole story: {error}"
        );

        host.set_file(&store.credentials_file, CREDENTIAL);
        assert_eq!(
            read(&host, &store).unwrap().map(|held| held.credential),
            Some(CREDENTIAL.to_string()),
            "a locked keychain must not hide the Credential Claude Code is using"
        );
    }

    #[test]
    fn a_keychain_that_is_not_there_at_all_is_a_store_holding_nothing() {
        let host = FakeHost::new().with_platform(Platform::Other);
        let store = profile_store(&host);
        host.lock_keychain("could not run /usr/bin/security: No such file or directory");

        assert!(
            read(&host, &store).unwrap().is_none(),
            "a logged-out Linux machine is logged out, not broken"
        );
    }

    #[test]
    fn a_credential_file_is_created_for_its_owner_alone() {
        let host = FakeHost::new().with_platform(Platform::Other);
        let store = profile_store(&host);
        let [primary, _] = stores_for(&host, &store);

        primary.write(&host, CREDENTIAL).expect("it is written");

        assert_eq!(
            host.file(&store.credentials_file).as_deref(),
            Some(CREDENTIAL)
        );
        assert_eq!(host.mode_of(&store.credentials_file), Some(0o600));
        assert_eq!(host.mode_of(&store.config_dir), Some(0o700));
    }

    #[test]
    fn a_credential_file_others_could_read_is_tightened_and_said_once() {
        let host = FakeHost::new()
            .with_platform(Platform::Other)
            .with_file_mode(
                "/Users/someone/.config/perch/profiles/a/.credentials.json",
                0o644,
            );
        let store = profile_store(&host);
        host.set_file(&store.credentials_file, CREDENTIAL);

        for _ in 0..3 {
            assert_eq!(
                read(&host, &store).unwrap().map(|held| held.credential),
                Some(CREDENTIAL.to_string()),
                "a loose file is tightened rather than refused"
            );
        }

        assert_eq!(host.mode_of(&store.credentials_file), Some(0o600));
        assert_eq!(host.notes().len(), 1, "{:?}", host.notes());
        assert!(host.notes()[0].contains("0644"), "{:?}", host.notes());
    }

    /// "Neither store holds one" is terminal — it Quarantines an Account — so a
    /// store that is there and would not say what it holds must not answer as
    /// one holding nothing. The wrong answer here turns a mode that can be
    /// fixed in a second into a state only a browser login ends.
    #[test]
    fn a_store_that_will_not_say_what_it_holds_is_not_a_store_holding_nothing() {
        let host = FakeHost::new();
        let store = profile_store(&host);
        let host = host.with_unreadable_file(&store.credentials_file, "Permission denied");
        // On this platform the file is the fallback, so it is the one asked
        // second — and the keychain, asked first, simply has nothing.
        host.set_file(&store.credentials_file, CREDENTIAL);

        let error = read(&host, &store).unwrap_err();
        assert!(error.to_string().contains("Permission denied"), "{error}");
    }

    #[test]
    fn forgetting_a_store_that_holds_nothing_is_not_a_failure() {
        let host = FakeHost::new();
        let store = profile_store(&host);
        for kept in stores_for(&host, &store) {
            kept.forget(&host).expect("nothing to forget is fine");
        }
    }
}
