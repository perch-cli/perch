//! Giving the machine back the state it had before Perch
//! (ADR the-holdings-go-out-sealed).
//!
//! Two things live here, and the second is all effect. **Refusing** a machine
//! something is running against is asked before anything is destroyed, because a
//! Purge deletes the Profiles a client would be holding files in. **Erasing** is
//! the act: every Credential out of its store, then Perch's home whole.
//!
//! That order is what makes a Purge that stopped part way re-runnable: a
//! Credential in the keychain lives outside the home that names it.

use std::path::PathBuf;

use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::live;
use crate::lock;
use crate::registry::{Account, Registry};

/// What a Purge took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purged {
    /// How many Accounts Perch is no longer holding.
    pub accounts: usize,
    /// How many of them had a Credential in a store to delete. Fewer than the
    /// Accounts is ordinary for a Quarantined one and news for any other, which
    /// is why the caller says it rather than this deciding.
    pub credentials: usize,
    /// Profiles emptied and deleted that no Account named. Counted rather than
    /// reported as nothing, because on a Registry that will not parse this is
    /// *every* Profile on the machine, and a Purge is what nothing undoes.
    pub unnamed: Unnamed,
    pub notes: Vec<String>,
}

/// The Profiles under Perch's home that the Registry does not name, and how many
/// of them had a Credential to delete.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Unnamed {
    pub profiles: usize,
    pub credentials: usize,
}

/// What a Purge that will not run leaves behind: everything, a Purge being all
/// or nothing.
const NOTHING_WAS_PURGED: live::Consequence = live::Consequence {
    nothing_happened: "Nothing was purged.",
    quit_it: "A Purge deletes those directories, and what is in them belongs to \
              whatever is holding them until it exits. Quit it and run this \
              again.",
};

/// Refuses while a client is running against a Profile a Purge would delete.
///
/// ADR a-profile-is-live-by-evidence's rule at its extreme: a Purge deletes those
/// directories rather than writing into them, and doubt counts as a client. Asked
/// of the same set [`forget_what_the_registry_does_not_name`] empties.
pub fn refuse_while_anything_is_running(host: &dyn Host, registry: &Registry) -> Result<()> {
    let mut places: Vec<live::Place> = registry
        .accounts
        .iter()
        .filter_map(|account| {
            account.profile_dir(host).ok().map(|dir| {
                live::Place::new(
                    account.provider(),
                    format!("the Profile of {}", account.key()),
                    dir,
                )
            })
        })
        .collect();

    // Named generically, because there is nothing to name them by: a login in
    // progress has no Account yet, and a Profile the Registry does not hold has
    // no address Perch can put to the user.
    places.extend(
        what_the_registry_does_not_name(host, registry)?
            .into_iter()
            .map(|profile| {
                live::Place::new(
                    profile.provider,
                    format!(
                        "{}, which no Account of Perch's names",
                        profile.dir.display()
                    ),
                    profile.dir,
                )
            }),
    );

    match live::ask(host, &places) {
        live::Answer::Idle(_) => Ok(()),
        live::Answer::NotIdle(not_idle) => Err(not_idle.refusal(&NOTHING_WAS_PURGED)),
    }
}

struct ManagedProfile {
    provider: crate::providers::provider::Id,
    dir: PathBuf,
}

/// Every directory under Perch's home that is or was a Profile: one under
/// `profiles/`, one under `pending/` that a login ran in.
///
/// A parent that is not there is ordinary; every other failure stops the Purge,
/// or an unlistable `pending/` reads as empty and `erase` takes the home whole.
fn everything_perch_holds(host: &dyn Host) -> Result<Vec<ManagedProfile>> {
    let mut found = Vec::new();
    for provider in crate::providers::provider::catalog() {
        for directory in ["profiles", "pending"] {
            let parent = provider.id().home(host)?.join(directory);
            match host.list_dir(&parent) {
                // Directories, as the name says: a `.DS_Store` beside them is not a
                // Profile, and counting one tells somebody agreeing to a Purge that
                // Perch holds a Profile it cannot name.
                Ok(entries) => found.extend(
                    entries
                        .into_iter()
                        .filter(|at| !host.is_file(at))
                        .map(|dir| ManagedProfile {
                            provider: provider.id(),
                            dir,
                        }),
                ),
                Err(crate::host::HostError::NotFound { .. }) => {}
                Err(err) => {
                    return Err(
                        PerchError::file_read(parent.clone(), err).with_note(&format!(
                            "Nothing was purged. Until Perch can list {}, it cannot say \
                     which Profiles are under it, and one that goes unlisted is \
                     a Credential left behind with nothing to name it by.",
                            parent.display(),
                        )),
                    );
                }
            }
        }
    }
    Ok(found)
}

/// Deletes every Credential Perch holds, and then everything Perch keeps.
///
/// A store that will not give its Credential up stops the Purge. Nothing is
/// undone — the Registry is still there, so running it again finishes it — and
/// the home goes last and whole, lock artifact and all.
pub fn erase(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &Registry,
    _fresh: &crate::wait::Fresh,
) -> Result<Purged> {
    // Resolved before anything is deleted, although it is not needed until the
    // end: every Profile is derived from it, so a machine that cannot say where
    // home is must not lose half its Credentials on the way to finding out.
    let home = holdings::perch_home(host)?;

    // Renewed around every store rather than trusted from the caller's last
    // check, because what happens here is unbounded: one Store per Account, and
    // a keychain delete can stop for a dialog while the hold goes stale.
    let mut credentials = 0;
    let mut notes = Vec::new();
    for account in &registry.accounts {
        perch.renew();
        let removed = forget_the_credential(host, account).map_err(incomplete_purge)?;
        credentials += usize::from(removed.removed);
        if let Some(note) = removed.note
            && !notes.contains(&note)
        {
            notes.push(note);
        }
    }
    perch.renew();
    let unnamed = forget_what_the_registry_does_not_name(host, registry, &mut notes)
        .map_err(incomplete_purge)?;

    // The last thing asked before the one deletion running this again cannot
    // finish, and not through `still_ours`: its sentence is that nothing was
    // done, and by this line every Credential is deleted.
    perch.renew();
    if !perch.still_held() {
        return Err(PerchError::Other(format!(
            "Another `perch` changed the Registry while this Purge was working. \
             Every Credential Perch held is deleted; {} was left where it is, \
             rather than taken with whatever the other `perch` put in it.\n\
             Run `perch holdings purge` again and it will finish.",
            home.display(),
        )));
    }

    host.remove_dir_all(&home).map_err(|err| {
        PerchError::Other(format!(
            "Every Credential Perch held is deleted, but {} could not be removed: \
             {err}\n\
             Run `perch holdings purge` again once it can be, and it will \
             finish.",
            home.display(),
        ))
    })?;

    Ok(Purged {
        accounts: registry.accounts.len(),
        credentials,
        unnamed,
        notes,
    })
}

/// Empties the Credential Store of every directory under Perch's home that has
/// one, whether or not the Registry names it: a Store is derived from its
/// directory, so a home taken whole destroys the only name reaching a keychain
/// item outside it. Counted apart from the Accounts — nobody believes in these —
/// and never as nothing, because a Registry that will not parse names none of them.
fn forget_what_the_registry_does_not_name(
    host: &dyn Host,
    registry: &Registry,
    notes: &mut Vec<String>,
) -> Result<Unnamed> {
    let mut counted = Unnamed::default();
    for profile in what_the_registry_does_not_name(host, registry)? {
        counted.profiles += 1;
        let removed = profile
            .provider
            .adapter()
            .forget_profile_credential(host, &profile.dir)?;
        counted.credentials += usize::from(removed.removed);
        if let Some(note) = removed.note
            && !notes.contains(&note)
        {
            notes.push(note);
        }
    }
    Ok(counted)
}

/// How many Profiles are under Perch's home, whatever the Registry says of them.
///
/// For the one question the Registry cannot answer: what a Purge is about to take
/// on a machine whose Registry will not parse. `Err` where the home cannot be
/// listed, which is a refusal the Purge itself raises a moment later.
pub fn profiles_held(host: &dyn Host) -> Result<usize> {
    Ok(everything_perch_holds(host)?.len())
}

/// Every directory Perch holds that no Account of its names.
///
/// One walk, because the refusal and the deletion have to be looking at the same
/// set: what `refuse_while_anything_is_running` declines to purge is exactly what
/// `forget_what_the_registry_does_not_name` empties.
fn what_the_registry_does_not_name(
    host: &dyn Host,
    registry: &Registry,
) -> Result<Vec<ManagedProfile>> {
    let recorded: Vec<PathBuf> = registry
        .accounts
        .iter()
        .filter_map(|account| account.profile_dir(host).ok())
        .collect();
    Ok(everything_perch_holds(host)?
        .into_iter()
        .filter(|profile| !recorded.contains(&profile.dir))
        .collect())
}

fn forget_the_credential(
    host: &dyn Host,
    account: &Account,
) -> Result<crate::providers::provider::CredentialRemoval> {
    if account.profile_dir(host).is_err() {
        return Ok(crate::providers::provider::CredentialRemoval::default());
    }
    account
        .provider()
        .adapter()
        .forget_credential(host, &account.profile(host)?)
}

fn incomplete_purge(error: PerchError) -> PerchError {
    error.with_note("The Purge did not finish. Some Credential Stores may already be empty; run again with `perch holdings purge` to finish.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_fixture as credentials;
    use crate::domain::Identity;
    use crate::host::prelude::*;
    use crate::host::{FakeHost, Platform};
    use crate::test_support::AccountStoreFixture as _;

    fn account(email: &str) -> Account {
        Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: email.into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        }
    }

    /// A machine holding two Accounts, each with a Credential in the store this
    /// platform keeps one in.
    fn holding_two(host: &FakeHost) -> Registry {
        let mut registry = Registry::default();
        for email in ["one@example.com", "two@example.com"] {
            registry.upsert(account(email));
            let store = registry.account(email).unwrap().store(host).unwrap();
            let [primary, _] = credentials::stores_for(host, &store);
            primary.write(host, "held").expect("the store takes it");
        }
        registry.settle(Some("one@example.com".into()));
        registry
    }

    /// The whole of what a Purge promises, on both kinds of machine: every
    /// Credential out of whichever store this platform keeps one in, and
    /// everything Perch keeps gone with it.
    #[test]
    fn every_credential_and_everything_perch_keeps_are_gone() {
        for platform in [Platform::MacOs, Platform::Other, Platform::Windows] {
            let host = FakeHost::new().with_platform(platform);
            let registry = holding_two(&host);

            let purged = erase(
                &host,
                &mut holdings::lock(&host).expect("the lock is free"),
                &registry,
                &crate::wait::Fresh::for_a_test(),
            )
            .expect("nothing refuses");

            assert_eq!(
                purged,
                Purged {
                    accounts: 2,
                    credentials: 2,
                    unnamed: Unnamed::default(),
                    notes: Vec::new(),
                },
                "{platform:?}"
            );
            for email in ["one@example.com", "two@example.com"] {
                let store = registry.account(email).unwrap().store(&host).unwrap();
                assert_eq!(
                    credentials::read(&host, &store).unwrap(),
                    None,
                    "{email} on {platform:?}"
                );
            }
            assert!(
                !host.path_exists(&holdings::perch_home(&host).unwrap()),
                "{platform:?}"
            );
        }
    }

    /// The keychain item is filed under `$USER`, and a delete that finds nothing
    /// reports success. Counted apart from the Accounts so the command can say
    /// what actually happened rather than what a Purge usually does.
    #[test]
    fn an_account_whose_stores_held_nothing_is_counted_apart_from_the_rest() {
        let host = FakeHost::new();
        let registry = holding_two(&host);
        let store = registry
            .account("two@example.com")
            .unwrap()
            .store(&host)
            .unwrap();
        host.forget_keychain_item(&store.keychain_service, &store.keychain_account);

        let purged = erase(
            &host,
            &mut holdings::lock(&host).expect("the lock is free"),
            &registry,
            &crate::wait::Fresh::for_a_test(),
        )
        .expect("nothing refuses");

        assert_eq!(purged.accounts, 2);
        assert_eq!(purged.credentials, 1);
        assert_eq!(purged.unnamed, Unnamed::default());
        assert_eq!(purged.notes.len(), 1);
    }

    /// Reporting a machine given back while a keychain goes on holding a working
    /// Credential is the one wrong answer here, so the store that will not give
    /// its Credential up stops the Purge — and the refusal says that running it
    /// again finishes it.
    #[test]
    fn a_store_that_will_not_give_its_credential_up_stops_the_purge() {
        let host = FakeHost::new();
        let registry = holding_two(&host);
        host.lock_keychain("User interaction is not allowed");

        let refused = erase(
            &host,
            &mut holdings::lock(&host).expect("the lock is free"),
            &registry,
            &crate::wait::Fresh::for_a_test(),
        )
        .expect_err("the keychain will not answer");

        assert!(refused.to_string().contains("run again"), "{refused}");
        assert!(
            !host
                .effects()
                .contains(&crate::host::fake::Effect::RemovedDir(
                    holdings::perch_home(&host).unwrap()
                )),
            "and the registry naming what is left was not taken with it"
        );
    }

    /// An address no Profile could be named after has no Credential Store to
    /// empty, and the Registry recording it is exactly what a Purge takes away.
    #[test]
    fn an_address_no_profile_could_be_named_after_does_not_stop_a_purge() {
        let host = FakeHost::new();
        let mut registry = holding_two(&host);
        registry.upsert(account("@"));

        let purged = erase(
            &host,
            &mut holdings::lock(&host).expect("the lock is free"),
            &registry,
            &crate::wait::Fresh::for_a_test(),
        )
        .expect("`@` names no directory and no store");

        assert_eq!(purged.accounts, 3);
        assert_eq!(purged.credentials, 2);
        assert!(!host.path_exists(&holdings::perch_home(&host).unwrap()));
    }

    /// A Purge deletes the Profiles a client would be holding files in, so it is
    /// refused for the same reason every other write into a Live Profile is —
    /// and refused before anything is destroyed, because it is all or nothing.
    #[test]
    fn a_profile_something_is_running_against_stops_the_purge_before_it_starts() {
        let host = FakeHost::new();
        let registry = holding_two(&host);
        let profile = registry
            .account("two@example.com")
            .unwrap()
            .profile_dir(&host)
            .unwrap();
        host.set_file(
            profile.join(format!("sessions/{}.json", crate::host::fake::THIS_PROCESS)),
            &serde_json::json!({"startedAt": host.now().timestamp_millis(), "writtenBy": "perch"})
                .to_string(),
        );

        let refused = refuse_while_anything_is_running(&host, &registry)
            .expect_err("something is holding that Profile");

        assert_eq!(refused.exit_code(), crate::error::EXIT_PROFILE_LIVE);
        assert!(refused.to_string().contains("two@example.com"), "{refused}");
        assert!(
            !refused.to_string().contains("one@example.com"),
            "and only the Profile that is Live is named: {refused}"
        );
    }
}
