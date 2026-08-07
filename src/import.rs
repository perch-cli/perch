//! The exact inverse of an Export: one `age` file, put back on a machine (ADR
//! 0014).
//!
//! Two halves, the same way [`crate::export`] has two. **Reading** an Export is
//! arithmetic — given one, say whether this build understands it and what
//! registry it restores to — and needs no machine to be tested against.
//! **Placing** is the effect: every Credential into the Credential Store this
//! machine's Claude Code would use, so an Export written on macOS lands in files
//! on Linux and the other way round without either side knowing about the
//! other's store (ADR 0020).
//!
//! An Import refuses a machine that already holds an Account, and it does not
//! merge. Merging is where every hard case lives — the same Account on both
//! sides one Rotation apart, with no way to tell which Credential is live; an
//! Alias meaning different Accounts on two machines; a Group that exists in both
//! with different members. That is a real feature and it is not this one.
//! Refusing keeps an Import the exact inverse of a Purge, and that pair is what
//! makes "I can move to a new machine" true.
//!
//! It is also all-or-nothing. A machine holding some of an Export is the partial
//! restore the file exists to prevent, so anything that fails part way takes
//! back what it had already placed.

use crate::error::{PerchError, Result};
use crate::export::Export;
use crate::host::Host;
use crate::probe::Store;
use crate::profile;
use crate::registry::{self, Registry};

/// Refuses to import onto a machine that is already holding Accounts, and names
/// the one command that makes room.
///
/// `None` is a machine Perch has never run on, which is the case this command is
/// written for. A registry that is there and holds nothing — what a Purge leaves
/// — is just as empty, and is imported into just as happily.
pub fn refuse_a_machine_that_is_not_empty(held: Option<&Registry>) -> Result<()> {
    let accounts = held.map_or(0, |registry| registry.accounts.len());
    if accounts == 0 {
        return Ok(());
    }

    Err(PerchError::Conflict(format!(
        "Perch already holds {accounts} {}, and an Import does not merge: the \
         same Account on both sides one Rotation apart has no answer to which \
         Credential is live, and an Alias can mean different Accounts on two \
         machines.\n\
         Nothing was imported and the file was not opened. `perch purge` gives \
         the machine back and is what makes room — it offers to write an Export \
         first.",
        if accounts == 1 { "Account" } else { "Accounts" },
    )))
}

/// The registry an Export restores to, or a refusal to guess at one.
///
/// The Export's own envelope was checked when it was unsealed; this is the
/// registry that travelled inside it, which carries its own version and answers
/// the same question about its own shape. A machine holding two builds — the
/// newer one writing the file, the older one restoring it — is the case both
/// guards exist for.
///
/// Nothing arrives active. Being active is a claim about which Credential is in
/// this machine's Default Profile, and no Import puts one there: the Account
/// that was active where the Export was taken says nothing about what this
/// machine is running as. So the user Switches afterwards, and until they do
/// whatever Claude Code was logged in as goes on running.
pub fn restored(export: &Export) -> Result<Registry> {
    if export.registry.version > registry::CURRENT_VERSION {
        return Err(crate::error::written_by_a_newer_perch(
            "The registry inside this Export",
            "registry",
            export.registry.version,
            registry::CURRENT_VERSION,
        ));
    }

    // The same check the registry gets on the way in off disk, for the same
    // reason: a value that means nothing would otherwise sit in the file until
    // the watcher next went round and surprise somebody by acting on it.
    for (name, config) in &export.registry.groups {
        config.validate(name)?;
    }

    Ok(Registry {
        active: None,
        ..export.registry.clone()
    })
}

/// The Profiles an Import has touched, so they can be taken back out if
/// anything after them fails.
#[derive(Debug, Default)]
pub struct Placed {
    touched: Vec<Store>,
}

impl Placed {
    /// How many Profiles it has made.
    pub fn profiles_made(&self) -> usize {
        self.touched.len()
    }

    /// Takes back everything this Import placed, leaving the machine as it was.
    ///
    /// Best-effort, like every other undo in Perch: the interesting failure is
    /// the one that got us here, not the tidying up. Safe to be that blunt
    /// because an Import only ever runs on a machine holding no Accounts, so
    /// every Profile it removes is one it made moments ago.
    pub fn undo(&self, host: &dyn Host) {
        for store in &self.touched {
            profile::discard(host, store);
        }
    }
}

/// Puts every Credential the Export holds into the Profile of the Account it
/// belongs to.
///
/// Where that is is this machine's answer rather than the Export's: the file
/// records a Credential against an email address and nothing about the store it
/// came out of, so one written on macOS lands in a file on Linux and the other
/// way round (ADR 0020). Each one goes through [`profile::store_credential`], so
/// an Import gets the read-back guard every other write of a Credential gets.
///
/// An Account the Export holds no Credential for gets no Profile. That is how a
/// Quarantined Account travels, reason and all, and the Account is still
/// restored — one Perch has forgotten is worse news than one that needs logging
/// in again. Nothing is Quarantined here for it: the commands that need a
/// Credential discover there is none and record why, which is the one place that
/// decision is made.
pub fn place(host: &dyn Host, export: &Export) -> Result<Placed> {
    // Every Profile is named before any is made. Where one of them lands is
    // derivation and not a write, and an address no directory can be named after
    // is a refusal (see [`registry::profile_dir_for`]) — meeting it half way
    // through would mean undoing work that never had to be started.
    let mut placements = Vec::new();
    for account in &export.registry.accounts {
        let store = account.store(host)?;
        if let Some(credential) = export.credentials.get(account.email()) {
            placements.push((account.email().to_string(), store, credential));
        }
    }

    let mut placed = Placed::default();
    for (email, store, credential) in placements {
        // Recorded before it is written rather than after it worked, because
        // what has to come back out is everything this touched: a Profile
        // directory made for a Credential that then would not go into it is
        // exactly the orphan an Import promises not to leave.
        placed.touched.push(store.clone());
        if let Err(error) = profile::make_dir(host, &store.config_dir)
            .and_then(|()| profile::store_credential(host, &store, credential))
        {
            placed.undo(host);
            // Said as "every Profile this had made" rather than as a count,
            // because the count is nothing at all when the first Account is the
            // one that fails, and "the 0 already imported" is not a sentence.
            return Err(error.with_note(&format!(
                "Nothing was imported. {email}'s Credential could not be stored, \
                 and every Profile this had already made has been taken back out \
                 again: a machine holding some of an Export is the partial \
                 restore this file exists to prevent."
            )));
        }
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::CURRENT_VERSION;
    use crate::probe::Identity;
    use crate::registry::{Account, GroupConfig, Quarantine};
    use std::collections::BTreeMap;

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

    fn an_export() -> Export {
        let mut registry = Registry::default();
        registry.upsert(account("one@example.com"));
        registry.upsert(Account {
            quarantine: Some(Quarantine::RenewalRejected),
            ..account("two@example.com")
        });
        registry.active = Some("one@example.com".into());

        Export {
            version: CURRENT_VERSION,
            registry,
            credentials: BTreeMap::from([("one@example.com".to_string(), "held".to_string())]),
        }
    }

    /// A machine that has never run Perch and one a Purge emptied are the same
    /// machine as far as an Import is concerned.
    #[test]
    fn an_import_lands_on_an_empty_machine_however_it_came_to_be_empty() {
        refuse_a_machine_that_is_not_empty(None).expect("Perch has never run here");
        refuse_a_machine_that_is_not_empty(Some(&Registry::default()))
            .expect("a Purge leaves a registry holding nothing");
    }

    /// Merging is where every hard case lives, so the refusal names the command
    /// that makes room rather than leaving somebody to find it.
    #[test]
    fn a_machine_holding_an_account_is_refused_and_told_how_to_make_room() {
        let mut held = Registry::default();
        held.upsert(account("someone@example.com"));

        let refused =
            refuse_a_machine_that_is_not_empty(Some(&held)).expect_err("there is an Account here");
        assert_eq!(
            refused.exit_code(),
            crate::error::EXIT_CONFLICT,
            "{refused}"
        );
        assert!(refused.to_string().contains("perch purge"), "{refused}");
    }

    /// Being active is a claim about which Credential is in *this* machine's
    /// Default Profile, and an Import puts none there.
    #[test]
    fn everything_the_registry_said_is_restored_except_which_account_was_active() {
        let export = an_export();

        let restored = restored(&export).expect("this build understands it");

        assert_eq!(restored.active, None);
        assert_eq!(restored.accounts.len(), 2);
        assert_eq!(
            restored.account("two@example.com").unwrap().quarantine,
            Some(Quarantine::RenewalRejected),
            "a Quarantined Account arrives Quarantined, with its reason"
        );
    }

    /// The Export's envelope carries a version and so does the registry inside
    /// it. Both are guards against the future: the machine holding two builds,
    /// where the wrong answer is a file half-read rather than refused.
    #[test]
    fn a_registry_from_a_newer_perch_is_refused_rather_than_guessed_at() {
        let mut export = an_export();
        export.registry.version = registry::CURRENT_VERSION + 1;

        let refused = restored(&export).expect_err("this build does not understand it");
        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
    }

    /// The same check the registry gets off disk. A value that means nothing
    /// would otherwise sit in the file until the watcher next went round.
    #[test]
    fn group_configuration_that_could_not_mean_anything_is_refused_on_the_way_in() {
        let mut export = an_export();
        export.registry.groups.insert(
            "work".to_string(),
            GroupConfig {
                watcher_threshold_percent: 101,
                ..GroupConfig::default()
            },
        );

        let refused = restored(&export).expect_err("101% is not a percentage");
        assert!(
            refused.to_string().contains("watcher-threshold-percent"),
            "{refused}"
        );
    }

    /// The file records a Credential against an email address and nothing about
    /// the store it came out of, so where it lands is this machine's answer.
    #[test]
    fn a_credential_lands_in_the_store_this_machine_would_use() {
        for platform in [
            crate::host::Platform::MacOs,
            crate::host::Platform::Other,
            crate::host::Platform::Windows,
        ] {
            let host = crate::host::FakeHost::new().with_platform(platform);
            let export = an_export();
            let store = export
                .registry
                .account("one@example.com")
                .unwrap()
                .store(&host)
                .unwrap();

            let placed = place(&host, &export).expect("the one Profile can be made");

            assert_eq!(placed.profiles_made(), 1);
            assert_eq!(
                crate::credentials::read(&host, &store)
                    .unwrap()
                    .map(|held| held.credential),
                Some("held".to_string()),
                "{platform:?}"
            );
            assert!(
                !host.path_exists(&registry::profile_dir_for(&host, "two@example.com").unwrap()),
                "an Account the Export holds no Credential for gets no Profile"
            );
        }
    }

    /// A machine holding some of an Export is the partial restore the file
    /// exists to prevent, so what was placed before the failure comes back out.
    #[test]
    fn a_credential_that_cannot_be_stored_takes_back_the_ones_that_were() {
        let host = crate::host::FakeHost::new().with_platform(crate::host::Platform::Other);
        let second = registry::profile_dir_for(&host, "two@example.com")
            .unwrap()
            .join(".credentials.json");
        let host = host.with_unwritable_file(&second, "No space left on device");
        let mut export = an_export();
        export
            .credentials
            .insert("two@example.com".to_string(), "also held".to_string());

        let refused = place(&host, &export).expect_err("the second store will not take it");

        assert!(refused.to_string().contains("partial restore"), "{refused}");
        let first = registry::profile_dir_for(&host, "one@example.com").unwrap();
        assert!(
            !host.path_exists(&first),
            "the Profile made for the first Account is gone with it"
        );
    }

    /// Refused where the Profile directories are named rather than half way
    /// through making them, so undoing work that never had to start is not a
    /// case at all.
    #[test]
    fn an_address_no_profile_can_be_named_after_is_refused_before_anything_is_made() {
        let host = crate::host::FakeHost::new();
        let mut export = an_export();
        export.registry.upsert(account("@"));

        let refused = place(&host, &export).expect_err("`@` names no directory");

        assert!(refused.to_string().contains('@'), "{refused}");
        assert!(
            !host.path_exists(&registry::profile_dir_for(&host, "one@example.com").unwrap()),
            "and the Profile of the Account listed before it was never made"
        );
    }
}
