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

use crate::credentials::{self, Forgotten};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::live;
use crate::lock;
use crate::probe;
use crate::registry::{self, Account, Registry};

/// What a Purge took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Purged {
    /// How many Accounts Perch is no longer holding.
    pub accounts: usize,
    /// How many of them had a Credential in a store to delete. Fewer than the
    /// Accounts is ordinary for a Quarantined one and news for any other, which
    /// is why the caller says it rather than this deciding.
    pub credentials: usize,
    /// Profiles emptied and deleted that no Account named. Counted rather than
    /// reported as nothing, because on a registry that will not parse this is
    /// *every* Profile on the machine, and a Purge is what nothing undoes.
    pub unnamed: Unnamed,
}

/// The Profiles under Perch's home that the registry does not name, and how many
/// of them had a Credential to delete.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Unnamed {
    pub profiles: usize,
    pub credentials: usize,
}

/// Refuses while a client is running against a Profile a Purge would delete.
///
/// ADR a-profile-is-live-by-evidence's rule at its extreme: a Purge deletes those
/// directories rather than writing into them, and doubt counts as a client. Asked
/// of the same set [`forget_what_the_registry_does_not_name`] empties.
pub fn refuse_while_anything_is_running(host: &dyn Host, registry: &Registry) -> Result<()> {
    let mut running: Vec<String> = registry
        .accounts
        .iter()
        .filter(|account| {
            account
                .profile_dir(host)
                .is_ok_and(|dir| live::anything_running(host, &dir))
        })
        .map(|account| format!("the Profile of {}", account.email()))
        .collect();

    // Named generically, because there is nothing to name them by: a login in
    // progress has no Account yet, and a Profile the registry does not hold has
    // no address Perch can put to the user.
    running.extend(
        what_the_registry_does_not_name(host, registry)?
            .into_iter()
            .filter(|dir| live::anything_running(host, dir))
            .map(|dir| format!("{}, which no Account of Perch's names", dir.display())),
    );

    if running.is_empty() {
        return Ok(());
    }

    Err(PerchError::ProfileLive(format!(
        "A client is running against {}.\n\
         Nothing was purged. A Purge deletes those directories, and what is in \
         them belongs to whatever is holding them until it exits — quit it and \
         run this again.",
        running.join(", "),
    )))
}

/// Every directory under Perch's home that is or was a Profile: one under
/// `profiles/`, one under `pending/` that a login ran in.
///
/// A parent that is not there is ordinary; every other failure stops the Purge,
/// or an unlistable `pending/` reads as empty and `erase` takes the home whole.
fn everything_perch_holds(host: &dyn Host) -> Result<Vec<std::path::PathBuf>> {
    let mut found = Vec::new();
    for parent in [
        registry::profiles_dir(host),
        registry::pending_logins_dir(host),
    ]
    .into_iter()
    .flatten()
    {
        match host.list_dir(&parent) {
            // Directories, as the name says: a `.DS_Store` beside them is not a
            // Profile, and counting one tells somebody agreeing to a Purge that
            // Perch holds a Profile it cannot name.
            Ok(entries) => found.extend(entries.into_iter().filter(|at| !host.is_file(at))),
            Err(crate::host::HostError::NotFound { .. }) => {}
            Err(err) => {
                return Err(
                    PerchError::file_read(parent.clone(), err).with_note(&format!(
                        "Nothing was purged. Until Perch can list {}, it cannot say \
                     which Profiles are under it — and a Credential Store is \
                     named after the directory it belongs to, so a Profile that \
                     goes unlisted is a Credential that could be left behind \
                     with nothing left to name it by.",
                        parent.display(),
                    )),
                );
            }
        }
    }
    Ok(found)
}

/// Deletes every Credential Perch holds, and then everything Perch keeps.
///
/// A store that will not give its Credential up stops the Purge. Nothing is
/// undone — the registry is still there, so running it again finishes it — and
/// the home goes last and whole, lock artifact and all.
pub fn erase(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &Registry) -> Result<Purged> {
    // Resolved before anything is deleted, although it is not needed until the
    // end: every Profile is derived from it, so a machine that cannot say where
    // home is must not lose half its Credentials on the way to finding out.
    let home = registry::perch_home(host)?;

    // Renewed around every store rather than trusted from the caller's last
    // check, because what happens here is unbounded: one Store per Account, and
    // a keychain delete can stop for a dialog while the hold goes stale.
    let mut credentials = 0;
    for account in &registry.accounts {
        perch.renew();
        if forget_the_credential(host, account)? {
            credentials += 1;
        }
    }
    perch.renew();
    let unnamed = forget_what_the_registry_does_not_name(host, registry)?;

    // The last thing asked before the one deletion running this again cannot
    // finish, and not through `still_ours`: its sentence is that nothing was
    // done, and by this line every Credential is deleted.
    perch.renew();
    if !perch.still_held() {
        return Err(PerchError::Other(format!(
            "Another `perch` changed the registry while this Purge was working, \
             so this one is working from a copy that is out of date. Every \
             Credential Perch held is already deleted; what is left is {}, and \
             it has been left where it is rather than taken with whatever the \
             other `perch` put in it.\n\
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
    })
}

/// Empties the Credential Store of every directory under Perch's home that has
/// one, whether or not the registry names it: a Store is derived from its
/// directory, so a home taken whole destroys the only name reaching a keychain
/// item outside it. Counted apart from the Accounts — nobody believes in these —
/// and never as nothing, because a registry that will not parse names none of them.
fn forget_what_the_registry_does_not_name(host: &dyn Host, registry: &Registry) -> Result<Unnamed> {
    let mut counted = Unnamed::default();
    for dir in what_the_registry_does_not_name(host, registry)? {
        // The same answer `forget_the_credential` gives, and for its reason: a
        // store that cannot even be named is one whose Credential cannot be
        // deleted, and passing over it reports a machine given back.
        let store = probe::store_for_profile(host, &dir).map_err(|error| {
            error.with_note(&format!(
                "Perch's registry is untouched and every Credential already \
                 deleted is already gone. {} is still there, and until Perch \
                 can say which Credential Store it belongs to there is no way \
                 to tell whether one is being left behind.",
                dir.display(),
            ))
        })?;
        counted.profiles += 1;
        if empty_the_stores(host, &store)? {
            counted.credentials += 1;
        }
    }
    Ok(counted)
}

/// How many Profiles are under Perch's home, whatever the registry says of them.
///
/// For the one question the registry cannot answer: what a Purge is about to take
/// on a machine whose registry will not parse. `Err` where the home cannot be
/// listed, which is a refusal the Purge itself raises a moment later.
pub fn profiles_held(host: &dyn Host) -> Result<usize> {
    Ok(everything_perch_holds(host)?.len())
}

/// Every directory Perch holds that no Account of its names.
///
/// One walk, because the refusal and the deletion have to be looking at the same
/// set: what `refuse_while_anything_is_running` declines to purge is exactly what
/// `forget_what_the_registry_does_not_name` empties.
fn what_the_registry_does_not_name(host: &dyn Host, registry: &Registry) -> Result<Vec<PathBuf>> {
    let recorded: Vec<PathBuf> = registry
        .accounts
        .iter()
        .filter_map(|account| account.profile_dir(host).ok())
        .collect();
    Ok(everything_perch_holds(host)?
        .into_iter()
        .filter(|dir| !recorded.contains(dir))
        .collect())
}

/// Empties both of a Profile's Credential Stores, and says whether either held
/// anything.
///
/// One function for two passes over the same act, because the sentence a failure
/// carries is the whole of what a half-finished Purge can promise.
fn empty_the_stores(host: &dyn Host, store: &probe::Store) -> Result<bool> {
    let mut held = false;
    for kept_in in credentials::stores_for(host, store) {
        let forgotten = kept_in.forget(host).map_err(|error| {
            error.with_note(&format!(
                "Perch's registry is untouched and every Credential already \
                 deleted is already gone, so `perch holdings purge` can be run \
                 again once {} can be written to, and it will finish.",
                kept_in.describe(),
            ))
        })?;
        held |= forgotten == Forgotten::Credential;
    }
    Ok(held)
}

/// Takes an Account's Credential out of both of its stores, and says whether
/// either of them held one.
fn forget_the_credential(host: &dyn Host, account: &Account) -> Result<bool> {
    // An address no Profile could be named after has no Profile and no Store to
    // empty. One reaches the registry only by hand, and a Purge is taking it out
    // by hand, automated — so it must not be the command such an Account stops.
    let Ok(dir) = account.profile_dir(host) else {
        return Ok(false);
    };
    // Anything the store itself refuses to say is propagated rather than skipped
    // the same way: a store that cannot be named is one whose Credential cannot
    // be deleted, and passing over it reports a machine given back regardless.
    let store = probe::store_for_profile(host, &dir)?;
    empty_the_stores(host, &store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::prelude::*;
    use crate::host::{FakeHost, Platform};
    use crate::probe::Identity;

    fn account(email: &str) -> Account {
        Account {
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
                &mut registry::lock(&host).expect("the lock is free"),
                &registry,
            )
            .expect("nothing refuses");

            assert_eq!(
                purged,
                Purged {
                    accounts: 2,
                    credentials: 2,
                    unnamed: Unnamed::default()
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
                !host.path_exists(&registry::perch_home(&host).unwrap()),
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
            &mut registry::lock(&host).expect("the lock is free"),
            &registry,
        )
        .expect("nothing refuses");

        assert_eq!(
            purged,
            Purged {
                accounts: 2,
                credentials: 1,
                unnamed: Unnamed::default()
            }
        );
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
            &mut registry::lock(&host).expect("the lock is free"),
            &registry,
        )
        .expect_err("the keychain will not answer");

        assert!(refused.to_string().contains("run again"), "{refused}");
        assert!(
            !host
                .effects()
                .contains(&crate::host::fake::Effect::RemovedDir(
                    registry::perch_home(&host).unwrap()
                )),
            "and the registry naming what is left was not taken with it"
        );
    }

    /// An address no Profile could be named after has no Credential Store to
    /// empty, and the registry recording it is exactly what a Purge takes away.
    #[test]
    fn an_address_no_profile_could_be_named_after_does_not_stop_a_purge() {
        let host = FakeHost::new();
        let mut registry = holding_two(&host);
        registry.upsert(account("@"));

        let purged = erase(
            &host,
            &mut registry::lock(&host).expect("the lock is free"),
            &registry,
        )
        .expect("`@` names no directory and no store");

        assert_eq!(purged.accounts, 3);
        assert_eq!(purged.credentials, 2);
        assert!(!host.path_exists(&registry::perch_home(&host).unwrap()));
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
            probe::session_marker_at(&profile, crate::host::fake::THIS_PROCESS),
            &probe::session_marker(crate::host::fake::THIS_PROCESS, host.now()),
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
