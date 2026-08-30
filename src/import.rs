//! The exact inverse of an Export: one `age` file, put back on a machine
//! (ADR the-holdings-go-out-sealed).
//!
//! Two halves, the same way [`crate::export`] has two. **Reading** an Export is
//! arithmetic — given one, say whether this build understands it and what
//! registry it restores to. **Placing** is the effect: every Credential into the
//! Credential Store this machine's Claude Code would use, so an Export written
//! on macOS lands in files on Linux (ADR claude-code-chooses-the-store).
//!
//! An Import does not merge, and it is all or nothing.

use std::collections::BTreeMap;
use zeroize::Zeroizing;

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::export::Export;
use crate::holdings;
use crate::host::Host;
use crate::live;
use crate::login;
use crate::name;
use crate::probe::{self, Installed, Store};
use crate::profile;
use crate::registry::{self, Registry};
use crate::say;

/// Refuses to import onto a machine still holding anything, and names the one
/// command that makes room.
///
/// The whole registry rather than its Accounts: `Registry::forget` leaves a
/// Group declared, so the last Remove leaves Settings held nowhere else.
pub fn refuse_a_machine_that_is_not_empty(held: Option<&Registry>) -> Result<()> {
    let Some(registry) = held else {
        return Ok(());
    };
    let accounts = registry.accounts.len();
    let groups = registry.groups.len();
    let ungrouped_says_something = registry.ungrouped != crate::config::UngroupedConfig::default();
    if accounts == 0 && groups == 0 && !ungrouped_says_something {
        return Ok(());
    }

    let holding = match (accounts, groups) {
        (0, declared) => format!("no Account but {}", say::groups(declared)),
        (held, 0) => say::accounts(held),
        (held, declared) => format!("{} and {}", say::accounts(held), say::groups(declared)),
    };
    // Said only where a Group is part of what is held: an Import refused over
    // Accounts alone is refused for the sentence above it.
    let declarations = match groups {
        0 => "",
        _ => {
            " A Group and what it carries are declarations this machine holds \
              alone."
        }
    };
    Err(PerchError::Conflict(format!(
        "Perch already holds {holding}, and an Import does not merge onto a \
         machine that holds anything.{declarations}\n\
         Nothing was imported and the file was not opened. `perch holdings \
         purge` makes room, and offers to write an Export first."
    )))
}

/// The registry an Export restores to, or a refusal to guess at one.
///
/// Nothing arrives active and nothing arrives having just been checked: being
/// active is a claim about *this* machine's Default Profile, and
/// [`Registry::checks`] is a claim about a watcher that has not run here.
pub fn restored(export: &Export, path: &std::path::Path) -> Result<Registry> {
    // Both versions are read in `export::unseal`, off a shape that is only the
    // versions and before the document is read as an Export — so nothing reaches
    // here unchecked, and a second spelling is two guards to keep in step.

    // Named one at a time rather than as a struct update, because who is active
    // is not a field anybody may set: an Import lands on nobody, which is a
    // transition of its own (ADR a-switch-is-written-down-first).
    let mut restored = export.registry.clone();
    restored.settle(None);
    restored.checks = BTreeMap::new();

    // Cleared before it is judged rather than after, so what is asked about is
    // what will be written rather than what arrived.
    registry::readable(restored)
        .map_err(|refusal| refusal.with_note(&registry::the_file_to_edit(path)))
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
    /// Whether this Import wrote a Credential into that store. A Quarantined
    /// Account travels with none, so the Profile is made for its `.claude.json`
    /// alone — and forgetting a store this Import never wrote to would destroy a
    /// refresh token nothing here put there and nothing can recover.
    wrote_a_credential: bool,
    /// Whether this Import wrote the `.claude.json`. Recorded for the same
    /// reason and set the same way: the Account that *fails* is the last one
    /// recorded, and what it did not manage to write is not the undo's to take.
    wrote_the_identity_file: bool,
}

/// The Profiles an Import has touched, so they can be taken back out if
/// anything after them fails. Never leaves [`place`], which is what makes the
/// taking-back an obligation nothing outside can drop.
#[derive(Debug, Default)]
struct Placed {
    touched: Vec<Touched>,
}

impl Placed {
    /// Takes back what this Import *made*, best-effort.
    ///
    /// Made rather than written into: it is the *registry* an Import needs empty,
    /// and a Profile directory nothing names outlives every command that would
    /// have named it — on macOS, the only name reaching a live Credential.
    fn undo(&self, host: &dyn Host) {
        for touched in &self.touched {
            if !touched.was_already_there {
                profile::discard(host, &touched.store);
                continue;
            }
            // The directory stays and neither thing this Import wrote into it
            // does: a `.claude.json` holds an API key in an MCP server's `env`
            // block, so `profile::discard` prevents it as much as a Credential.
            if touched.wrote_a_credential {
                for kept_in in credentials::stores_for(host, &touched.store) {
                    let _ = kept_in.forget(host);
                }
            }
            if touched.wrote_the_identity_file {
                let _ = host.remove_file(&touched.store.identity_file);
            }
            let taken_back = match (touched.wrote_a_credential, touched.wrote_the_identity_file) {
                (true, true) => "the Credential and the `.claude.json`",
                (true, false) => "the Credential",
                // This Account travels with no Credential, so whatever is in that
                // store belongs to whoever left the directory behind.
                (false, true) => "the `.claude.json`",
                // The Account this Import stopped on: nothing of its own landed,
                // so the directory is exactly as its owner left it.
                (false, false) => continue,
            };
            host.note(&format!(
                "{} was already on this machine, so it was left where it is \
                 rather than removed with the Profiles this Import made. \
                 {taken_back} this Import wrote into it has been taken back out, \
                 and the Export still holds it.",
                touched.store.config_dir.display(),
            ));
        }
    }
}

/// Both maps an Export keys on an address, so a rule stated about one is stated
/// about the other: an Account holds a Credential and a `.claude.json`, and a
/// file arriving under an address nothing lists is the same failure either way.
fn every_map(export: &Export) -> [(&'static str, &BTreeMap<String, String>); 2] {
    [
        ("a Credential", &export.credentials),
        ("a `.claude.json`", &export.identity_files),
    ]
}

/// What an Import leaves behind when it will not write: nothing at all, an
/// Import being whole or not having happened.
const NOTHING_WAS_IMPORTED: live::Consequence = live::Consequence {
    nothing_happened: "Nothing was imported.",
    quit_it: "That Credential would be replaced underneath the session holding \
              it. Close it and run this again.",
};

/// Puts every Credential the Export holds into the Profile of the Account it
/// belongs to, wherever this machine keeps one, then runs `save` — the caller's
/// registry write — and takes everything back out where that refuses, so an
/// Import that does not finish cannot leave a Credential behind. Through
/// [`profile::store_credential`] so an Import gets the read-back guard.
pub fn place(
    host: &dyn Host,
    export: &Export,
    installed: &Installed,
    _fresh: &crate::wait::Fresh,
    save: impl FnOnce() -> Result<()>,
) -> Result<()> {
    // Keyed rather than asked of the registry per key: two names are one name
    // exactly where `name::folded` agrees, which is what `account` scans for.
    let listed: std::collections::HashSet<String> = export
        .registry
        .accounts
        .iter()
        .map(|account| name::folded(account.email()))
        .collect();

    // Every Credential in the file belongs to an Account the file lists, or this
    // is not the whole restore it claims to be. `gather` cannot write such an
    // Export, so this is about a file written by something else.
    for (what, keys) in every_map(export) {
        let unlisted: Vec<&str> = keys
            .keys()
            .map(String::as_str)
            .filter(|email| !listed.contains(name::folded(email).as_str()))
            .collect();
        if !unlisted.is_empty() {
            return Err(PerchError::Malformed {
                path: "the Export".to_string(),
                detail: format!(
                    "it holds {what} for {}, which it does not list as an \
                     Account. Nothing was imported: a file with no Account to \
                     belong to would be restored into a Profile nothing names, \
                     or not at all, and neither is the whole file.",
                    unlisted.join(", "),
                ),
            });
        }
    }

    // Two keys in either map that fold to one address, refused for the reason
    // above and by the same fold: `credential_for` answers with the first match,
    // so only one of the two is ever placed, under a report saying it was whole.
    for (what, keys) in every_map(export) {
        let mut held: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for key in keys.keys().map(String::as_str) {
            if let Some(clash) = held.insert(name::folded(key), key) {
                return Err(PerchError::Malformed {
                    path: "the Export".to_string(),
                    detail: format!(
                        "it holds {what} under both {clash} and {key}, which are \
                         one address. Nothing was imported: only one of the two \
                         would ever be restored, and an Import that quietly kept \
                         one and dropped the other is not the whole file.",
                    ),
                });
            }
        }
    }

    // Every Profile is named before any is made. Where one lands is derivation
    // rather than a write, and an address no directory can be named after is a
    // refusal — met half way through, it means undoing work never started.

    // Said here rather than left to `profile_dir_for`, whose refusal names a
    // registry an Import requires to be empty — so it points at a file the
    // Account it is about is never in.
    for account in &export.registry.accounts {
        if holdings::slug(account.email()).is_empty() {
            return Err(PerchError::Invalid(format!(
                "The Export holds an Account recorded as `{}`, which has no \
                 character a Profile directory can be named after, so Perch \
                 cannot say where its Credential would be kept.\n\
                 Nothing was imported. That Account has to be removed on a \
                 machine that still holds it, and the Export taken again.",
                account.email(),
            )));
        }
    }

    // And no two may land in one place: `user+work@` and `user.work@` flatten to
    // one Profile name, and storing over it supersedes the Credential already
    // there, which `perch add` refuses where a login enters.
    let mut landing: std::collections::HashMap<String, &registry::Account> =
        std::collections::HashMap::new();
    for account in &export.registry.accounts {
        if let Some(clash) = landing.insert(holdings::slug(account.email()), account) {
            return Err(PerchError::Conflict(format!(
                "{} and {} share the Profile they would be kept in, so importing \
                 both would mean each one's Credential replacing the other's.\n\
                 Nothing was imported. One of the two has to be removed on a \
                 machine that still holds it, and the Export taken again.",
                clash.email(),
                account.email(),
            )));
        }
    }

    let mut placements = Vec::new();
    for account in &export.registry.accounts {
        let store = account.store(host)?;
        // Keyed the way the guard above asked the question: `credential_for`
        // folds case and a `BTreeMap` lookup does not, so a key spelled
        // `ONE@example.com` is listed by the one and missed by the other.
        let credential = export.credential_for(account.email());
        let carried = export.identity_file_for(account.email());
        // Either one is a Profile to make. A Quarantined Account travels with no
        // Credential and with the `.claude.json` naming it, and dropping that
        // makes the next Export smaller than the one that fed this Import.
        if credential.is_none() && carried.is_none() {
            continue;
        }
        // Verbatim where the Export carries one, because Claude Code's
        // `oauthAccount` block holds fields the registry does not record and a
        // Switch prefers it (ADR everything-but-the-account).

        // `Zeroizing` because `Export::drop` wipes `identity_files` and this
        // clones out from under it: a `.claude.json` routinely carries an API
        // key in an MCP server's `env` block. Both arms, so one type does.
        let identity_file = Zeroizing::new(carried.cloned().unwrap_or_else(|| {
            probe::fresh_identity_file(&account.identity.oauth_account_block())
        }));
        placements.push((
            account.email().to_string(),
            store,
            credential,
            identity_file,
        ));
    }

    // A Profile something is running against is one nothing writes into, which
    // `profile::store_credential` names as the obligation it cannot check for
    // itself. Asked over every placement before the first of them is written.
    let places: Vec<live::Place> = placements
        .iter()
        .map(|(email, store, _, _)| {
            live::Place::new(
                format!("{email}'s Profile at {}", store.config_dir.display()),
                &store.config_dir,
            )
        })
        .collect();
    if let live::Answer::NotIdle(not_idle) = live::ask(host, &places) {
        return Err(not_idle.refusal(installed, &NOTHING_WAS_IMPORTED));
    }

    let mut placed = Placed::default();
    for (email, store, credential, identity_file) in placements {
        // Recorded before it is written, because what has to come back out is
        // everything this made — and asked before the directory is made, which
        // is the only moment the answer is still knowable.
        let at = placed.touched.len();
        placed.touched.push(Touched {
            was_already_there: host.path_exists(&store.config_dir),
            wrote_a_credential: false,
            wrote_the_identity_file: false,
            store: store.clone(),
        });
        let a_credential_traveled = credential.is_some();
        // A Quarantined Account travels with no Credential and with the
        // `.claude.json` that names it, so the Profile is made for the file
        // alone: dropped, it is a re-Export smaller than the one that made it.
        let stored = profile::make_dir(host, &store.config_dir).and_then(|()| match credential {
            Some(credential) => profile::store_credential(host, &store, credential),
            None => Ok(()),
        });
        // Asked rather than inferred from the `Ok`: `store_credential` refuses
        // when the store read first will not give up the copy it replaces, and
        // the Credential is in the other one by then.
        placed.touched[at].wrote_a_credential = a_credential_traveled
            && (stored.is_ok() || credential.is_some_and(|carried| landed(host, &store, carried)));
        let landed = stored.and_then(|()| login::carry_identity_file(host, &identity_file, &store));
        placed.touched[at].wrote_the_identity_file = landed.is_ok();
        if let Err(error) = landed {
            placed.undo(host);
            // Said as "every Profile this had made" rather than as a count: the
            // count is nothing when the first Account is the one that fails, and
            // "the 0 already imported" is not a sentence.
            return Err(error.with_note(&format!(
                "Nothing was imported. {email}'s Credential could not be stored, \
                 and every Profile this had already made has been taken back out \
                 again: a machine holding some of an Export is the partial \
                 restore this file exists to prevent."
            )));
        }
    }
    // The save is this Import's to run: held outside, the taking-back was an
    // ordering one caller remembered in prose.
    if let Err(error) = save() {
        placed.undo(host);
        return Err(error.with_note(
            "Nothing was imported. The Credentials this had already restored \
             have been taken back out again, and the file can be imported \
             again.",
        ));
    }
    Ok(())
}

/// Whether a store holds the Credential this Import was writing into it.
///
/// A store that will not answer says nothing, and the undo leaves it: what it
/// might hold is a Credential this Import did not put there.
fn landed(host: &dyn Host, store: &Store, carried: &str) -> bool {
    credentials::read(host, store)
        .ok()
        .flatten()
        .is_some_and(|held| *held.credential == *carried)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::export::CURRENT_VERSION;
    use crate::host::Refusing;
    use crate::host::prelude::*;
    use crate::probe::Identity;
    use crate::registry::Active;
    use crate::registry::{Account, Quarantine};
    use std::collections::BTreeMap;

    /// What the refusal counts, on each of the three ways a machine can be
    /// occupied. A Group with no Account in it is the one the guard used to
    /// miss, and "0 Accounts" is the wrong thing to say to somebody whose
    /// Groups are what is about to be replaced.
    #[test]
    fn the_refusal_names_whichever_of_the_two_a_machine_is_holding() {
        let empty = Registry::default();
        refuse_a_machine_that_is_not_empty(None).expect("a machine Perch never ran on");
        refuse_a_machine_that_is_not_empty(Some(&empty)).expect("what a Purge leaves");

        let mut accounts_only = Registry::default();
        accounts_only.upsert(crate::cycle::tests::account("one@example.com", vec![]));
        let said = refuse_a_machine_that_is_not_empty(Some(&accounts_only))
            .expect_err("it holds an Account")
            .to_string();
        assert!(said.contains("1 Account"), "{said}");
        assert!(!said.contains("Group"), "and nothing about Groups: {said}");

        let mut groups_only = Registry::default();
        groups_only
            .declare_group("work")
            .expect("the Group is declared");
        let said = refuse_a_machine_that_is_not_empty(Some(&groups_only))
            .expect_err("a Group is a declaration held nowhere else")
            .to_string();
        assert!(said.contains("no Account but 1 Group"), "{said}");

        let mut both = groups_only.clone();
        both.upsert(crate::cycle::tests::account("one@example.com", vec![]));
        let said = refuse_a_machine_that_is_not_empty(Some(&both))
            .expect_err("it holds both")
            .to_string();
        assert!(said.contains("1 Account and 1 Group"), "{said}");
    }

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
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        }
    }

    /// The save that succeeds, shared so a test whose `place` refuses before
    /// it names the same step every succeeding test runs.
    fn saves() -> Result<()> {
        Ok(())
    }

    fn an_export() -> Export {
        let mut registry = Registry::default();
        registry.upsert(account("one@example.com"));
        registry.upsert(Account {
            quarantine: Some(Quarantine::RenewalRejected),
            ..account("two@example.com")
        });
        registry.settle(Some("one@example.com".into()));

        Export {
            version: CURRENT_VERSION,
            registry,
            credentials: BTreeMap::from([("one@example.com".to_string(), "held".to_string())]),
            identity_files: BTreeMap::new(),
        }
    }

    /// `gather` records a `.claude.json` for every Account whether or not a
    /// Credential could be read, and `refuse_a_file_for_an_account_it_does_not
    /// _list` validates its key — so one dropped here is a file that traveled,
    /// was checked, and then went nowhere.
    #[test]
    fn a_claude_json_for_an_account_carrying_no_credential_is_placed_all_the_same() {
        let host = crate::host::FakeHost::new();
        let mut export = an_export();
        export.identity_files.insert(
            "two@example.com".to_string(),
            r#"{"oauthAccount":{"emailAddress":"two@example.com"},"projects":{}}"#.to_string(),
        );
        let store = export
            .registry
            .account("two@example.com")
            .unwrap()
            .store(&host)
            .unwrap();

        place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            saves,
        )
        .expect("the Profiles can be made");

        assert_eq!(
            host.read_file(&store.identity_file).ok().as_deref(),
            Some(r#"{"oauthAccount":{"emailAddress":"two@example.com"},"projects":{}}"#),
            "the Quarantined Account's file came back as it went out"
        );
        assert!(
            crate::credentials::read(&host, &store).unwrap().is_none(),
            "and no Credential was invented to carry it"
        );
    }

    #[test]
    fn a_save_that_refuses_takes_every_credential_back_out() {
        let host = crate::host::FakeHost::new();
        let export = an_export();
        let store = export
            .registry
            .account("one@example.com")
            .unwrap()
            .store(&host)
            .unwrap();

        let refused = place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            || Err(PerchError::Other("the disk filled".to_string())),
        )
        .expect_err("the registry could not be written");

        assert!(refused.to_string().contains("taken back out"), "{refused}");
        assert!(
            !host.path_exists(&store.config_dir),
            "the Profile this Import made is gone with it"
        );
    }

    #[test]
    fn the_save_runs_once_and_only_after_every_credential_landed() {
        let host = crate::host::FakeHost::new();
        let export = an_export();
        let store = export
            .registry
            .account("one@example.com")
            .unwrap()
            .store(&host)
            .unwrap();

        let mut saved = 0;
        place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            || {
                saved += 1;
                // The Credential is already in its store when the save runs, so the
                // registry never records an Account whose Credential is not there.
                assert!(
                    crate::credentials::read(&host, &store)
                        .expect("the store answers")
                        .is_some(),
                    "the Credential lands before the registry says it did"
                );
                Ok(())
            },
        )
        .expect("the ordinary Import");

        assert_eq!(saved, 1);
        assert!(
            crate::credentials::read(&host, &store)
                .expect("the store answers")
                .is_some(),
            "and it stays"
        );
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

        assert_eq!(*restored.active(), Active::Nobody);
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
            refused.to_string().contains("carries ` ` (U+0020)"),
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
            refused.to_string().contains("carries `@` (U+0040)"),
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

            place(
                &host,
                &export,
                &Installed::unknown("2.1.221"),
                &crate::wait::Fresh::for_a_test(),
                saves,
            )
            .expect("the one Profile can be made");

            assert!(
                host.path_exists(&store.config_dir),
                "the one Profile the Export holds a Credential for was made"
            );
            assert_eq!(
                crate::credentials::read(&host, &store)
                    .unwrap()
                    .map(|held| held.credential.to_string()),
                Some("held".to_string()),
                "{platform:?}"
            );
            assert!(
                !host.path_exists(&holdings::profile_dir_for(&host, "two@example.com").unwrap()),
                "an Account the Export holds no Credential for gets no Profile"
            );
        }
    }

    /// A machine holding some of an Export is the partial restore the file
    /// exists to prevent, so what was placed before the failure comes back out.
    #[test]
    fn a_credential_that_cannot_be_stored_takes_back_the_ones_that_were() {
        let host = crate::host::FakeHost::new().with_platform(crate::host::Platform::Other);
        let second = holdings::profile_dir_for(&host, "two@example.com")
            .unwrap()
            .join(".credentials.json");
        let host = host.with_a_path_refusing(&second, Refusing::Write, "No space left on device");
        let mut export = an_export();
        export
            .credentials
            .insert("two@example.com".to_string(), "also held".to_string());

        let refused = place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            saves,
        )
        .expect_err("the second store will not take it");

        assert!(refused.to_string().contains("partial restore"), "{refused}");
        let first = holdings::profile_dir_for(&host, "one@example.com").unwrap();
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

        let refused = place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            saves,
        )
        .expect_err("`@` names no directory");

        let said = refused.to_string();
        assert!(said.contains('@'), "{said}");
        // An Import needs an empty registry, so the one `profile_dir_for` would
        // send somebody to edit is the one file the Account is not in.
        assert!(
            said.contains("Export"),
            "the refusal names the file the Account is actually in: {said}"
        );
        assert!(
            !said.contains("registry.json"),
            "and not one this machine is required not to have: {said}"
        );
        assert!(
            !host.path_exists(&holdings::profile_dir_for(&host, "one@example.com").unwrap()),
            "and the Profile of the Account listed before it was never made"
        );
    }

    /// `gather` cannot write one, so this is about a file written by something
    /// else — and dropping it silently is a success message over a restore that
    /// was not whole.
    #[test]
    fn a_file_for_an_account_the_export_does_not_list_is_refused() {
        for what in ["a Credential", "a `.claude.json`"] {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let mut export = an_export();
            let map = match what {
                "a Credential" => &mut export.credentials,
                _ => &mut export.identity_files,
            };
            map.insert("nobody@example.com".to_string(), "held".to_string());

            let refused = place(
                &host,
                &export,
                &Installed::unknown("2.1.221"),
                &crate::wait::Fresh::for_a_test(),
                saves,
            )
            .expect_err("that file belongs to nothing");

            let said = refused.to_string();
            assert!(
                said.contains("nobody@example.com"),
                "it names the Account that is missing: {said}"
            );
            assert!(
                said.contains(what),
                "and says which of the two maps holds it: {said}"
            );
        }
    }

    /// The `unlisted` guard folds case, so both keys name a listed Account and
    /// both pass it, and `credential_for` folds case too and answers with the
    /// *first* — so the second is never placed, mentioned or counted. A
    /// hand-written Export (`age -a -p`) is where two spellings of one address
    /// come from.
    #[test]
    fn an_export_holding_one_address_under_two_spellings_is_refused() {
        for (what, mut export) in [
            ("a Credential", an_export()),
            ("a `.claude.json`", an_export()),
        ] {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let map = match what {
                "a Credential" => &mut export.credentials,
                _ => &mut export.identity_files,
            };
            map.insert("one@example.com".to_string(), "held".to_string());
            map.insert("ONE@example.com".to_string(), "also held".to_string());

            let refused = place(
                &host,
                &export,
                &Installed::unknown("2.1.221"),
                &crate::wait::Fresh::for_a_test(),
                saves,
            )
            .expect_err("only one of the two would land");

            let said = refused.to_string();
            assert!(
                said.contains("ONE@example.com") && said.contains("one@example.com"),
                "it names both spellings: {said}"
            );
            assert!(
                said.contains(what),
                "and says which of the two maps holds them: {said}"
            );
            assert!(
                host.effects().is_empty(),
                "with nothing written on the way to finding out: {:?}",
                host.effects()
            );
        }
    }

    /// Two addresses can flatten to one Profile name, and plus-addressing on a
    /// single inbox is how somebody comes to hold several Accounts. `perch add`
    /// refuses that collision where a login enters, because storing over it
    /// destroys a refresh token nothing can recover.
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

        let refused = place(
            &host,
            &export,
            &Installed::unknown("2.1.221"),
            &crate::wait::Fresh::for_a_test(),
            saves,
        )
        .expect_err("both would land in one Profile");

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
