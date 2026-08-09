//! Giving the machine back the state it had before Perch (ADR 0014).
//!
//! The exact inverse of an Import, and the other half of what makes "I can move
//! to a new machine" true: every Profile, every Credential Perch holds, and
//! Perch's own registry, gone in one act.
//!
//! Two things live here, and the second one is all effect. **Refusing** a
//! machine something is running against is a question asked before anything is
//! destroyed, because a Purge deletes the Profiles a client would be holding
//! files in. **Erasing** is the act: every Credential out of the store it lives
//! in, and then Perch's home directory whole.
//!
//! That order is the whole of what makes a Purge that stopped part way
//! re-runnable. A Credential in the operating system's keychain lives *outside*
//! Perch's home, so a Purge that removed the home first would leave items behind
//! with nothing left recording which they were. Taking the Credentials first
//! means the registry naming them is the last thing to go — so a Purge that
//! failed anywhere before that can simply be run again, and finds every
//! Credential it already deleted already gone.
//!
//! What it does not touch is the Credential in the Default Profile. That is
//! Claude Code's own login rather than a copy Perch holds, and a Purge that
//! logged the user out of the tool they are using would be doing more than
//! giving the machine back.

use crate::credentials::{self, Forgotten};
use crate::error::{PerchError, Result};
use crate::host::Host;
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
}

/// Refuses while a client is running against a Profile a Purge would delete.
///
/// The same rule every other write obeys, at its extreme: writing into a Live
/// Profile is refused because something else is holding those files (ADR 0005),
/// and a Purge does not write into one — it deletes it. Asked of every Account
/// before anything is destroyed, because a Purge is all or nothing and a refusal
/// discovered half way through is the partial state the command exists to avoid.
///
/// Doubt counts as a client here. A marker that can be neither corroborated nor
/// dismissed is a Profile that may be in use, and the cost of the two mistakes is
/// not the same: waiting costs a command run again once the client is quit, and
/// not waiting costs whatever that client had open.
pub fn refuse_while_anything_is_running(host: &dyn Host, registry: &Registry) -> Result<()> {
    let running: Vec<&str> = registry
        .accounts
        .iter()
        .filter(|account| {
            account
                .profile_dir(host)
                .is_ok_and(|dir| probe::anything_running(host, &dir))
        })
        .map(Account::email)
        .collect();
    if running.is_empty() {
        return Ok(());
    }

    Err(PerchError::ProfileLive(format!(
        "A client is running against the Profile of {}.\n\
         Nothing was purged. A Purge deletes those Profiles, and what is in them \
         belongs to whatever is holding them until it exits — quit it and run \
         this again.",
        running.join(", "),
    )))
}

/// Deletes every Credential Perch holds, and then everything Perch keeps.
///
/// A store that will not give its Credential up stops the Purge rather than
/// being shrugged off: reporting a machine given back while a keychain goes on
/// holding a working Credential is the one wrong answer here. Nothing is undone
/// on the way out — every step of a Purge is a step the user asked for, and the
/// registry is still there, so running it again finishes it.
///
/// The home directory goes last and goes whole, which takes every Profile, the
/// registry, any login nobody came back from, and the lock this is running
/// under. Losing the lock artifact costs nothing: what it excludes is another
/// Perch changing a registry that no longer exists.
pub fn erase(host: &dyn Host, registry: &Registry) -> Result<Purged> {
    // Resolved before anything is deleted, although it is not needed until the
    // end: every Profile is derived from it, so a machine that cannot say where
    // home is must not have half its Credentials taken on the way to finding
    // that out.
    let home = registry::perch_home(host)?;

    let mut credentials = 0;
    for account in &registry.accounts {
        if forget_the_credential(host, account)? {
            credentials += 1;
        }
    }
    forget_what_the_registry_does_not_name(host)?;

    host.remove_dir_all(&home).map_err(|err| {
        PerchError::Other(format!(
            "Every Credential Perch held is deleted, but {} could not be removed: \
             {err}\n\
             Run `perch purge` again once it can be, and it will finish.",
            home.display(),
        ))
    })?;

    Ok(Purged {
        accounts: registry.accounts.len(),
        credentials,
    })
}

/// Empties the Credential Store of every directory under Perch's home that has
/// one, whether or not the registry names it.
///
/// The registry is not the whole account of what Perch is holding, and the
/// difference is not exotic. A login abandoned at the browser step leaves a
/// working Credential in `pending/login-<millis>/` — Ctrl-C there is the
/// documented flow rather than an accident — and nothing reaps one under thirty
/// minutes old. A Profile whose Credential Store would not empty is kept where
/// it is by [`profile::discard`], because the directory is the only thing that
/// can still name that store. Neither is in `registry.accounts`.
///
/// [`profile::discard`]: crate::profile::discard
///
/// What makes that fatal rather than untidy is where a Credential lives. On
/// macOS it is a keychain item outside Perch's home, and its service name is
/// derived from the directory — so `remove_dir_all` destroys the only thing
/// that could ever name it again. The item stays, live, holding a refresh
/// token, while the Purge reports the machine given back.
///
/// Deletions are counted for nothing and reported as nothing: what these
/// directories hold are Credentials for Accounts the user does not believe they
/// have, and a Purge that announced two more than it was asked about would be
/// answering a question nobody put.
fn forget_what_the_registry_does_not_name(host: &dyn Host) -> Result<()> {
    let mut left_over = Vec::new();
    for parent in [
        registry::profiles_dir(host)?,
        registry::pending_logins_dir(host)?,
    ] {
        // Absent is the ordinary case for both: no login has been abandoned
        // here, or every Profile the registry names is every Profile there is.
        if let Ok(entries) = host.list_dir(&parent) {
            left_over.extend(entries);
        }
    }

    for dir in left_over {
        // The same answer `forget_the_credential` gives, and for its reason: a
        // store that cannot even be named is one whose Credential cannot be
        // deleted, and passing over it would report a machine given back while
        // a keychain went on holding a working login.
        //
        // These two disagreed. A store is unnameable when the platform will not
        // say who the user is, which is not a fact about one directory — so a
        // Purge failed on the first Account in the registry and shrugged the
        // identical directory off here, and the walk is only reached at all
        // once the registry is empty. Perch was refusing to purge a machine
        // holding Accounts and reporting success on the same machine holding
        // only their leftovers.
        let store = probe::store_for_profile(host, &dir).map_err(|error| {
            error.with_note(&format!(
                "Perch's registry is untouched and every Credential already \
                 deleted is already gone. {} is still there, and until Perch \
                 can say which Credential Store it belongs to there is no way \
                 to tell whether one is being left behind.",
                dir.display(),
            ))
        })?;
        for kept_in in credentials::stores_for(host, &store) {
            kept_in.forget(host).map_err(|error| {
                error.with_note(&format!(
                    "Perch's registry is untouched and every Credential already \
                     deleted is already gone, so `perch purge` can be run again once \
                     {} can be written to, and it will finish.",
                    kept_in.describe(),
                ))
            })?;
        }
    }
    Ok(())
}

/// Takes an Account's Credential out of both of its stores, and says whether
/// either of them held one.
fn forget_the_credential(host: &dyn Host, account: &Account) -> Result<bool> {
    // An address no Profile could ever have been named after has no Profile and
    // no Credential Store to empty. `perch add` and adoption both refuse such an
    // address where it is derived, so the only way one reaches the registry is
    // by hand — and [`registry::profile_dir_for`] says it has to be taken out of
    // the registry by hand again. A Purge is exactly that, automated, so it must
    // not be the one command such an Account can stop.
    let Ok(dir) = account.profile_dir(host) else {
        return Ok(false);
    };
    // Anything the store itself refuses to say is propagated rather than skipped
    // the same way: a store that cannot even be named is one whose Credential
    // cannot be deleted, and passing over it would report a machine given back
    // while a keychain went on holding a working login.
    let store = probe::store_for_profile(host, &dir)?;

    let mut held = false;
    for kept_in in credentials::stores_for(host, &store) {
        let forgotten = kept_in.forget(host).map_err(|error| {
            error.with_note(&format!(
                "Perch's registry is untouched and every Credential already \
                 deleted is already gone, so `perch purge` can be run again once \
                 {} can be written to, and it will finish.",
                kept_in.describe(),
            ))
        })?;
        held |= forgotten == Forgotten::Credential;
    }
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            enabled: true,
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
        registry.active = Some("one@example.com".into());
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

            let purged = erase(&host, &registry).expect("nothing refuses");

            assert_eq!(
                purged,
                Purged {
                    accounts: 2,
                    credentials: 2
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

        let purged = erase(&host, &registry).expect("nothing refuses");

        assert_eq!(
            purged,
            Purged {
                accounts: 2,
                credentials: 1
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

        let refused = erase(&host, &registry).expect_err("the keychain will not answer");

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

        let purged = erase(&host, &registry).expect("`@` names no directory and no store");

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
