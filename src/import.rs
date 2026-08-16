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

use std::collections::BTreeMap;

use crate::error::{PerchError, Result};
use crate::export::Export;
use crate::host::Host;
use crate::login;
use crate::probe::{self, Store};
use crate::profile;
use crate::registry::{self, Active, Registry};

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
        "Perch already holds {}, and an Import does not merge: the \
         same Account on both sides one Rotation apart has no answer to which \
         Credential is live, and an Alias can mean different Accounts on two \
         machines.\n\
         Nothing was imported and the file was not opened. `perch holdings \
         purge` gives the machine back and is what makes room — it offers to \
         write an Export first.",
        crate::commands::accounts(accounts),
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
///
/// Nothing arrives having just been checked either, and for the same reason.
/// [`Registry::checks`] is what a `perch watcher check` on the *other* machine
/// did — when it Switched — and the cooldown is measured from it. Carried
/// across, an Export taken this morning has the first check on the new machine
/// reporting `cooling` on the strength of something that happened somewhere
/// else. That is a claim about a watcher, and no watcher has run here yet.
pub fn restored(export: &Export, path: &std::path::Path) -> Result<Registry> {
    // The registry's own version is not checked here. `export::unseal` reads
    // both versions off a shape that is only the versions, *before* the document
    // is read as an Export, and it has to: a newer Perch is exactly the thing
    // that writes a value this build has no variant for, and reading the
    // document first fails on that with serde's own words. So nothing can reach
    // this function without having passed the identical check, and a second
    // spelling of it was two things to keep in step for one guard.

    // The check the registry gets on the way in off disk, and the same one: an
    // Import writes a registry without reading one first, so anything this
    // accepts and `load` does not is a machine with no working command left on
    // it — including the `perch holdings purge` that would make room to try
    // again. This was a narrower copy that walked Group configuration alone, so
    // an Export holding an Alias keyed by an email address, or a Group name
    // with a space in it, imported cleanly and wedged the machine on the next
    // command.
    //
    // The path named is where the registry is about to be written rather than
    // where it came from, because that is the file the refusal tells them to
    // edit — and the Export it came from is encrypted.
    registry::validate(&export.registry)
        .map_err(|refusal| refusal.with_note(&registry::the_file_to_edit(path)))?;

    Ok(Registry {
        active: Active::Nobody,
        checks: BTreeMap::new(),
        ..export.registry.clone()
    })
}

/// One Profile an Import has written into, and whether the Import is what
/// brought it into being.
#[derive(Debug)]
struct Touched {
    store: Store,
    /// Whether the Profile's directory was already on the machine. An Import
    /// runs on a machine holding no *Accounts*, which is not the same as a
    /// machine holding no Profiles: a `perch add` that died at the browser step,
    /// or a Purge that could not empty a store, leaves a directory the registry
    /// never named.
    was_already_there: bool,
}

/// The Profiles an Import has touched, so they can be taken back out if
/// anything after them fails.
#[derive(Debug, Default)]
pub struct Placed {
    touched: Vec<Touched>,
}

impl Placed {
    /// Takes back everything this Import *made*, leaving the machine as it was.
    ///
    /// Best-effort, like every other undo in Perch: the interesting failure is
    /// the one that got us here, not the tidying up.
    ///
    /// Made, and not merely written into. The reasoning this used to be blunt on
    /// — "an Import only ever runs on a machine holding no Accounts, so every
    /// Profile it removes is one it made moments ago" — reads the refusal one
    /// step wider than it goes: it is the *registry* that has to be empty, and a
    /// Profile directory nothing names outlives every command that would have
    /// named it. So an Import that failed at the store of an Account whose
    /// directory was already there deleted that directory, and on macOS the
    /// keychain item outside it that only its name could still reach — a live
    /// refresh token for a login nobody was importing.
    ///
    /// What is left instead is a Profile holding the Credential this Import put
    /// in it, which costs nothing that has not already been paid: the Export
    /// file still holds every Credential in it, the directory was already
    /// unnamed before any of this started, and `perch holdings purge` walks it
    /// and the next `perch add` reaps it.
    pub fn undo(&self, host: &dyn Host) {
        for touched in &self.touched {
            if touched.was_already_there {
                host.note(&format!(
                    "{} was already on this machine, so it was left where it is \
                     rather than removed with the Profiles this Import made. \
                     The Export still holds every Credential in it.",
                    touched.store.config_dir.display(),
                ));
                continue;
            }
            profile::discard(host, &touched.store);
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
    // Every Credential in the file belongs to an Account the file lists, or
    // this is not the whole restore it claims to be. `gather` cannot write such
    // an Export, so this is about a file written by something else — and the
    // failure it guards is the one ADR 0014 exists to prevent, arriving
    // quietly: an Import that reports success having restored less than the
    // file held.
    let unlisted: Vec<&str> = export
        .credentials
        .keys()
        .map(String::as_str)
        .filter(|email| export.registry.account(email).is_none())
        .collect();
    if !unlisted.is_empty() {
        return Err(PerchError::Malformed {
            path: "the Export".to_string(),
            detail: format!(
                "it holds a Credential for {}, which it does not list as an \
                 Account. Nothing was imported: a Credential with no Account \
                 to belong to would be restored into a Profile nothing names, \
                 or not at all, and neither is the whole file.",
                unlisted.join(", "),
            ),
        });
    }

    // Every Profile is named before any is made. Where one of them lands is
    // derivation and not a write, and an address no directory can be named after
    // is a refusal (see [`registry::profile_dir_for`]) — meeting it half way
    // through would mean undoing work that never had to be started.
    // And no two of them may land in one place. Two addresses can flatten to one
    // Profile name — `user+work@` and `user.work@` do — and `perch add` refuses
    // that collision where a login enters, because storing over it supersedes
    // the Credential already there and destroys a refresh token nothing can
    // recover. An Export written on a machine that never had both can still be
    // restored onto one where they collide, and this loop wrote the second over
    // the first and reported success. Named before anything is made, like every
    // other refusal here.
    for (at, account) in export.registry.accounts.iter().enumerate() {
        if let Some(clash) = export.registry.accounts[..at]
            .iter()
            .find(|earlier| registry::same_profile(earlier.email(), account.email()))
        {
            return Err(PerchError::Conflict(format!(
                "{} and {} share the Profile they would be kept in, so importing \
                 both would mean each one's Credential replacing the other's.\n\
                 Nothing was imported. This Export cannot be restored whole onto \
                 this machine — one of the two has to be removed on a machine \
                 that still holds it, and the Export taken again.",
                clash.email(),
                account.email(),
            )));
        }
    }

    let mut placements = Vec::new();
    for account in &export.registry.accounts {
        let store = account.store(host)?;
        if let Some(credential) = export.credentials.get(account.email()) {
            // The Profile's own `.claude.json` travels beside its Credential.
            // Where the Export carries one it is written verbatim, because the
            // `oauthAccount` block Claude Code wrote holds fields the registry
            // does not record and a Switch prefers it over anything Perch would
            // compose. Where it carries none, one is composed — a Profile
            // without this file Carries nothing, so every Run against it would
            // meet the onboarding dialog afresh (ADR 0003).
            let identity_file = export
                .identity_files
                .get(account.email())
                .cloned()
                .unwrap_or_else(|| {
                    probe::fresh_identity_file(&account.identity.oauth_account_block())
                });
            placements.push((
                account.email().to_string(),
                store,
                credential,
                identity_file,
            ));
        }
    }

    let mut placed = Placed::default();
    for (email, store, credential, identity_file) in placements {
        // Recorded before it is written rather than after it worked, because
        // what has to come back out is everything this made: a Profile
        // directory made for a Credential that then would not go into it is
        // exactly the orphan an Import promises not to leave.
        //
        // And asked before the directory is made, which is the only moment the
        // answer is still knowable — see [`Placed::undo`] for what turns on it.
        placed.touched.push(Touched {
            was_already_there: host.path_exists(&store.config_dir),
            store: store.clone(),
        });
        if let Err(error) = profile::make_dir(host, &store.config_dir)
            .and_then(|()| profile::store_credential(host, &store, credential))
            .and_then(|()| login::carry_identity_file(host, &identity_file, &store))
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
    use crate::registry::{Account, Quarantine, Settings};
    use std::collections::BTreeMap;

    /// Where the restored registry would be written. It is the file a refusal
    /// tells somebody to edit, so it is named rather than derived here.
    const REGISTRY: &str = "/Users/someone/.config/perch/registry.json";

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
        registry.active = Active::Settled("one@example.com".into());

        Export {
            version: CURRENT_VERSION,
            registry,
            credentials: BTreeMap::from([("one@example.com".to_string(), "held".to_string())]),
            identity_files: BTreeMap::new(),
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
        assert!(
            refused.to_string().contains("perch holdings purge"),
            "{refused}"
        );
        assert!(
            refused
                .to_string()
                .contains("Perch already holds 1 Account,"),
            "the count is rendered once, by the one function that says it in \
             words: {refused}"
        );

        held.upsert(account("another@example.com"));
        let refused =
            refuse_a_machine_that_is_not_empty(Some(&held)).expect_err("there are Accounts here");
        assert!(
            refused
                .to_string()
                .contains("Perch already holds 2 Accounts,"),
            "{refused}"
        );
    }

    /// Being active is a claim about which Credential is in *this* machine's
    /// Default Profile, and an Import puts none there.
    #[test]
    fn everything_the_registry_said_is_restored_except_which_account_was_active() {
        let export = an_export();

        let restored =
            restored(&export, std::path::Path::new(REGISTRY)).expect("this build understands it");

        assert_eq!(restored.active, Active::Nobody);
        assert_eq!(restored.accounts.len(), 2);
        assert_eq!(
            restored.account("two@example.com").unwrap().quarantine,
            Some(Quarantine::RenewalRejected),
            "a Quarantined Account arrives Quarantined, with its reason"
        );
    }

    /// The same check the registry gets off disk. A value that means nothing
    /// would otherwise sit in the file until the watcher next went round.
    #[test]
    fn group_configuration_that_could_not_mean_anything_is_refused_on_the_way_in() {
        let mut export = an_export();
        export.registry.groups.insert(
            "work".to_string(),
            Settings {
                watcher_threshold_percent: 101,
                ..Settings::default()
            },
        );

        let refused = restored(&export, std::path::Path::new(REGISTRY))
            .expect_err("101% is not a percentage");
        assert!(
            refused.to_string().contains("watcher-threshold-percent"),
            "{refused}"
        );
    }

    /// And the rest of that check, which an Import used to skip. Anything this
    /// accepts and `registry::load` refuses is a machine with no working
    /// command left on it — `perch holdings purge`, the one that would make
    /// room to try again, reads the registry too. So an Export is held to
    /// exactly what a load is.
    #[test]
    fn a_registry_no_later_command_could_read_is_refused_rather_than_restored() {
        let mut named_badly = an_export();
        named_badly
            .registry
            .groups
            .insert("my work".to_string(), Settings::default());
        let refused = restored(&named_badly, std::path::Path::new(REGISTRY))
            .expect_err("no later command could read that");
        assert!(
            refused.to_string().contains("has a space in it"),
            "{refused}"
        );

        let mut aliased_badly = an_export();
        aliased_badly.registry.aliases.insert(
            "one@example.com".to_string(),
            "other@example.com".to_string(),
        );
        let refused = restored(&aliased_badly, std::path::Path::new(REGISTRY))
            .expect_err("an Alias cannot be an email address");
        assert!(
            refused.to_string().contains("looks like an email address"),
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

            place(&host, &export).expect("the one Profile can be made");

            assert!(
                host.path_exists(&store.config_dir),
                "the one Profile the Export holds a Credential for was made"
            );
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

    /// A Credential for an Account the file does not list is a file that is not
    /// the whole restore it claims to be.
    ///
    /// `gather` cannot write one, so this is about a file written by something
    /// else — and dropping it silently is the failure ADR 0014 exists to
    /// prevent, arriving as a success message.
    #[test]
    fn a_credential_for_an_account_the_export_does_not_list_is_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut export = an_export();
        export
            .credentials
            .insert("nobody@example.com".to_string(), "held".to_string());

        let refused = place(&host, &export).expect_err("that Credential belongs to nothing");

        assert!(
            refused.to_string().contains("nobody@example.com"),
            "it names the Account that is missing: {refused}"
        );
    }

    /// Two addresses can flatten to one Profile name, and plus-addressing on a
    /// single inbox is exactly how somebody comes to hold several Accounts.
    /// `perch add` refuses that collision where a login enters, because storing
    /// over it supersedes the Credential already there and destroys a refresh
    /// token nothing can recover. An Import placed the second over the first and
    /// reported success.
    #[test]
    fn two_accounts_that_would_share_one_profile_are_refused_before_either_is_placed() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut export = an_export();
        export.registry.upsert(account("user+work@example.com"));
        export.registry.upsert(account("user.work@example.com"));
        for email in ["user+work@example.com", "user.work@example.com"] {
            export.credentials.insert(
                email.to_string(),
                r#"{"claudeAiOauth":{"refreshToken":"sk-ant-ort01-other"}}"#.to_string(),
            );
        }

        let refused = place(&host, &export).expect_err("both would land in one Profile");

        assert!(
            refused.to_string().contains("user+work@example.com")
                && refused.to_string().contains("user.work@example.com"),
            "it names both: {refused}"
        );
        assert!(
            host.effects().is_empty(),
            "and nothing was written on the way to finding out: {:?}",
            host.effects()
        );
    }
}
