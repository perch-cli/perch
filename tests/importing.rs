//! `perch holdings import` — a whole machine, put back
//! (ADR the-holdings-go-out-sealed).
//!
//! The other half of the pair that makes "I can move to a new machine" true.
//! Everything here is about the two ways it could quietly fail somebody: by
//! restoring less than the whole, and by leaving a machine holding half of one.

mod common;

use chrono::Utc;
use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_HELD, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
use perch::host::prelude::*;
use perch::host::{FakeHost, Platform};
use perch::registry::{Active, Quarantine, Registry};

const PASSPHRASE: &str = "correct horse battery staple";
const AT: &str = "/Users/someone/perch-backup.age";

/// The file a machine worth backing up produced: three Accounts, two of them in
/// a Group carrying a policy, one named, one taken out of Cycling, and one
/// Quarantined.
fn an_export_of_a_whole_machine() -> String {
    let host = machine_with_three_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the name is free");
    disable_account(&host, THIRD_EMAIL)
        .0
        .expect("it stops being chosen");
    quarantine_for(&host, THIRD_EMAIL, Quarantine::RenewalRejected);
    config_set(&host, &["work", "watcher-threshold-percent", "65"])
        .0
        .expect("the Group takes its policy");

    let host = host.with_secrets(&[PASSPHRASE, PASSPHRASE]);
    run_export(&host, AT).0.expect("the export is written");
    host.file(AT).expect("a file was written")
}

/// A machine nobody has run Perch on, holding the file and somebody who can type
/// the passphrase it was written with.
fn a_new_machine_holding(sealed: &str) -> FakeHost {
    machine_with_claude_code()
        .with_file(AT, sealed)
        .with_secrets(&[PASSPHRASE])
}

/// The registry as it is on disk, or nothing at all — which is what "no
/// half-populated registry" is asserted against.
fn registry_on(host: &FakeHost) -> Option<Registry> {
    perch::registry::load(host).expect("whatever is there is readable")
}

/// The unlisted-Credential guard folds case and a `BTreeMap` lookup does not, so
/// a key spelled `SOMEONE@example.com` beside an account entry
/// `someone@example.com` is listed by the one and missed by the other.
#[test]
fn a_credential_keyed_in_another_case_is_placed_rather_than_silently_dropped() {
    let mut export =
        perch::export::unseal(&an_export_of_a_whole_machine(), PASSPHRASE).expect("it opens");
    // The same Account, spelled the other way — which is what a file written by
    // something other than `gather` looks like.
    let credential = export.credentials.remove(EMAIL).expect("it holds one");
    export
        .credentials
        .insert(EMAIL.to_uppercase(), credential.clone());
    if let Some(identity) = export.identity_files.remove(EMAIL) {
        export.identity_files.insert(EMAIL.to_uppercase(), identity);
    }
    let sealed = perch::export::seal(&export, PASSPHRASE).expect("it seals");
    let host = a_new_machine_holding(&sealed);

    let (outcome, said) = run_import(&host, AT);

    outcome.expect("the import restores");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(credential.as_str()),
        "an Account the file holds a Credential for gets it, whichever way the \
         key was spelled"
    );
    // The report asks the same question a third way, so a lookup by key here
    // announces an Account whose Credential has just been placed as "restored
    // without one" and sends its owner to `perch relogin` for nothing.
    assert!(
        !said.contains("held no Credential"),
        "and the report says so, rather than sending somebody to repair an \
         Account that is whole: {said}"
    );
}

/// `report` is the last fallible thing an Import does, and everything before it
/// has succeeded. Raised bare, the obvious next move meets
/// `refuse_a_machine_that_is_not_empty`, which advises a Purge.
#[test]
fn a_terminal_that_goes_away_after_the_import_lands_says_it_landed() {
    /// Writes until the Import's own report starts, and then is not there.
    struct GoesAwayReporting;

    impl std::io::Write for GoesAwayReporting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Imported") {
                true => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "the pipe closed",
                )),
                false => Ok(bytes.len()),
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let host = a_new_machine_holding(&an_export_of_a_whole_machine());

    let outcome =
        perch::commands::import::run(&host, std::path::Path::new(AT), &mut GoesAwayReporting);

    let refused = outcome.expect_err("the report could not be written");
    let said = refused.to_string();
    assert!(
        said.contains("nothing to run again"),
        "a machine that is restored is not told to import again: {said}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and it really is restored"
    );
    assert!(credential_of(&host, EMAIL).is_some(), "Credentials and all");
}

/// A passphrase is the one wait in Perch with no bound on it, so it is where a
/// hold goes stale under a command behaving perfectly — and finding out at the
/// save is finding out after the rollback has deleted somebody else's Profile.
#[test]
fn an_import_whose_registry_went_stale_while_the_passphrase_was_typed_writes_nothing() {
    let host = a_new_machine_holding(&an_export_of_a_whole_machine())
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::registry::lock_spec(host).expect("home is known");
            host.remove_dir_all(&lock.dir).expect("it was abandoned");
            host.create_dir_exclusive(&lock.dir)
                .expect("the other `perch` takes it");
        });

    let (outcome, _) = run_import(&host, AT);

    let refused = outcome.expect_err("this Perch may no longer speak for the registry");
    assert_eq!(
        refused.exit_code(),
        EXIT_HELD,
        "a registry another `perch` has moved on is a run to repeat, not a fault: {refused}"
    );
    assert!(
        refused.to_string().contains("Nothing was imported"),
        "{refused}"
    );
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(
            credential_of(&host, email),
            None,
            "no Credential is written before the hold is re-checked, so there is \
             nothing for a rollback to take back out from under anybody"
        );
    }
}

#[test]
fn an_import_restores_every_account_credential_alias_group_and_rule() {
    let host = a_new_machine_holding(&an_export_of_a_whole_machine());

    let (outcome, printed) = run_import(&host, AT);
    outcome.expect("the import lands");

    let registry = registry_of(&host);
    assert_eq!(registry.accounts.len(), 3);
    assert_eq!(registry.alias_of(SECOND_EMAIL), Some("overflow"));
    assert_eq!(
        registry.account(EMAIL).unwrap().group.as_deref(),
        Some("work")
    );
    assert!(
        registry.account(THIRD_EMAIL).unwrap().disabled,
        "an Account taken out of Cycling comes back out of Cycling"
    );
    assert_eq!(
        registry.group("work").unwrap().watcher_threshold_percent,
        65,
        "a Group carries its policy, so a restore does not arrive with the defaults"
    );

    for (email, credential) in [
        (EMAIL, CREDENTIAL),
        (SECOND_EMAIL, SECOND_CREDENTIAL),
        (THIRD_EMAIL, THIRD_CREDENTIAL),
    ] {
        assert_eq!(
            credential_of(&host, email).as_deref(),
            Some(credential),
            "{email}'s Credential is in its Profile"
        );
    }
    assert!(printed.contains("3 Accounts"), "{printed}");
}

#[test]
fn a_quarantined_account_imports_as_quarantined_with_its_reason() {
    let host = a_new_machine_holding(&an_export_of_a_whole_machine());

    run_import(&host, AT).0.expect("the import lands");

    assert_eq!(
        quarantine_of(&host, THIRD_EMAIL),
        Some(Quarantine::RenewalRejected)
    );
}

/// The file records a Credential against an email address and nothing about the
/// store it came out of.
#[test]
fn a_credential_lands_in_the_store_this_machines_claude_code_would_use() {
    let sealed = an_export_of_a_whole_machine();
    let host = a_new_machine_holding(&sealed).with_platform(Platform::Other);

    run_import(&host, AT).0.expect("the import lands");

    let store = store_of(&host, EMAIL);
    assert_eq!(
        host.file(&store.credentials_file).as_deref(),
        Some(CREDENTIAL),
        "off macOS the Credential Store is a file inside the Profile"
    );
    assert_eq!(
        host.keychain_item(&store.keychain_service, &store.keychain_account),
        None,
        "and nothing was left in the store this platform does not use"
    );
}

/// Asserted as the two data it is about rather than as a `perch switch` in the
/// report: nothing arrives active on any Import, so pointing at the Switch is
/// Perch pre-empting a disappointment on every run (ADR perch-says-what-it-did).
#[test]
fn nothing_is_made_active_by_an_import() {
    let sealed = an_export_of_a_whole_machine();
    let host = logged_in_machine()
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    let live_before = host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME);

    let (outcome, printed) = run_import(&host, AT);
    outcome.expect("the import lands");

    assert_eq!(*registry_of(&host).active(), Active::Nobody, "{printed}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME),
        live_before,
        "the live Credential is not an Import's to replace: {printed}"
    );
}

/// `checks` is what a `perch watcher check` on the *other* machine did, and the
/// cooldown is measured from it — so carried across, an Export taken this
/// morning has a new machine's first check reporting `cooling`.
#[test]
fn no_watcher_has_run_here_yet_however_recently_one_ran_where_the_export_was_taken() {
    // A machine whose scheduled check Switched a moment ago, exported.
    let host = machine_with_three_accounts();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("it joins");
    let mut registry = registry_of(&host);
    registry.checks.insert(
        "work".to_string(),
        perch::registry::Checked {
            switched_at: host.now(),
        },
    );
    save_registry(&host, &registry);
    let host = host.with_secrets(&[PASSPHRASE, PASSPHRASE]);
    run_export(&host, AT).0.expect("the export is written");
    let sealed = host.file(AT).expect("a file was written");

    let onto = a_new_machine_holding(&sealed);
    run_import(&onto, AT).0.expect("the import lands");

    assert!(
        registry_of(&onto).checks.is_empty(),
        "a new machine's first check is its first check, not one paced by \
         another machine's: {:?}",
        registry_of(&onto).checks
    );
}

#[test]
fn an_import_onto_a_logged_in_machine_adopts_nothing() {
    let sealed = an_export_of_a_whole_machine();
    // A login belonging to nobody in the Export, so an adoption would show up as
    // a fourth Account rather than hiding inside one of the three.
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, THIRD_CREDENTIAL)
        .with_file(IDENTITY_PATH, THIRD_IDENTITY_FILE)
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);

    run_import(&host, AT).0.expect("the import lands");

    let registry = registry_of(&host);
    assert_eq!(
        registry.accounts.len(),
        3,
        "the three the file held, and nothing adopted alongside them"
    );
    assert_eq!(*registry.active(), Active::Nobody);
}

#[test]
fn a_machine_that_already_holds_an_account_is_refused_and_told_about_purge() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_two_accounts()
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    let before = registry_of(&host);

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("Perch already holds Accounts here");
    assert_eq!(refused.exit_code(), EXIT_CONFLICT, "{refused}");
    assert!(
        refused.to_string().contains("perch holdings purge"),
        "{refused}"
    );
    assert_eq!(
        registry_of(&host),
        before,
        "and the machine is exactly as it was"
    );
    assert!(
        !host
            .effects()
            .contains(&perch::host::fake::Effect::AskedInSecret),
        "nobody was asked to type a passphrase for a refusal"
    );
}

#[test]
fn a_wrong_passphrase_fails_before_anything_is_written() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_file(AT, &sealed)
        .with_secrets(&["correct hose battery staple"]);

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("that is not the passphrase");
    assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
    assert!(refused.to_string().contains("passphrase"), "{refused}");
    assert_eq!(registry_on(&host), None, "no registry was written");
    assert_eq!(credential_of(&host, EMAIL), None, "and no Credential");
    assert!(
        !host.path_exists(std::path::Path::new(
            "/Users/someone/.config/perch/profiles"
        )),
        "no Profile was made for a file that never opened"
    );
}

#[test]
fn an_empty_passphrase_and_end_of_input_both_restore_nothing() {
    let sealed = an_export_of_a_whole_machine();
    for typed in [&["   "][..], &[][..]] {
        let host = machine_with_claude_code()
            .with_file(AT, &sealed)
            .with_secrets(typed);

        run_import(&host, AT)
            .0
            .expect_err("that is not a passphrase");
        assert_eq!(registry_on(&host), None);
    }
}

/// A passphrase passed as an argument sits in the process table for anything on
/// the machine to read, so there is no flag to name instead of the terminal.
#[test]
fn without_a_terminal_the_import_is_refused_and_says_what_is_needed() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_file(AT, &sealed)
        .without_terminal();

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("there is nobody to type a passphrase");
    assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
    assert!(refused.to_string().contains("no terminal"), "{refused}");
    assert!(refused.to_string().contains("process table"), "{refused}");
    assert_eq!(registry_on(&host), None);
}

#[test]
fn a_path_that_holds_nothing_is_said_rather_than_guessed_at() {
    let host = machine_with_claude_code().with_secrets(&[PASSPHRASE]);

    let (outcome, _printed) = run_import(&host, "/Users/someone/typo.age");

    let refused = outcome.expect_err("there is no file there");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert!(refused.to_string().contains("typo.age"), "{refused}");
}

#[test]
fn a_file_that_is_not_an_export_is_refused_as_one_rather_than_as_a_bad_passphrase() {
    let host = machine_with_claude_code()
        .with_file(AT, "notes I keep in this directory")
        .with_secrets(&[PASSPHRASE]);

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("that is not an `age` file");
    assert!(refused.to_string().contains("`age` file"), "{refused}");
    assert_eq!(registry_on(&host), None);
}

/// An Export is `age`'s *armored* form, so the read is a read of text and plain
/// `age -p` writes binary — which fails UTF-8 decoding before `unseal` sees it.
/// Whoever meets it took their own backup with `age -p`.
#[test]
fn a_binary_age_file_is_refused_as_one_that_has_to_be_armored() {
    let host = machine_with_claude_code()
        .with_a_file_that_is_not_text(AT)
        .with_secrets(&[PASSPHRASE]);

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("an Export is the armored form");
    let said = refused.to_string();
    assert!(
        !said.contains("UTF-8"),
        "the encoding of the bytes is not what the reader did wrong: {said}"
    );
    assert!(
        said.contains("armored"),
        "what an Export is, is said: {said}"
    );
    assert!(
        said.contains("age -a -p"),
        "and so is the way to make one from what they have: {said}"
    );
    assert_eq!(registry_on(&host), None);
}

#[test]
fn an_import_that_fails_part_way_takes_back_what_it_had_already_placed() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_platform(Platform::Other)
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    // The Credential Store of one of the three, which off macOS is a file. Which
    // one it is does not matter: what is under test is that the others do not
    // survive it.
    let refuses = store_of(&host, SECOND_EMAIL).credentials_file;
    let host = host.with_unwritable_file(&refuses, "No space left on device");

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("one Credential cannot be stored");
    assert!(refused.to_string().contains("partial restore"), "{refused}");
    assert_eq!(registry_on(&host), None, "no half-populated registry");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(credential_of(&host, email), None, "{email} left nothing");
        assert!(
            !host.path_exists(&store_of(&host, email).config_dir),
            "{email}'s Profile went with it"
        );
    }
}

/// It is the *registry* an Import needs empty, and a Profile directory nothing
/// names outlives every command that would have named it — so on macOS deleting
/// one takes the only name reaching a live Credential beside it.
#[test]
fn a_rollback_leaves_a_profile_that_was_already_on_the_machine_where_it_is() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_platform(Platform::Other)
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    // A Profile from a `perch add` that never finished: a directory holding a
    // Credential and a registry that never named it. The first of the three, so
    // the Import has written into it by the time the second one fails.
    let orphan = store_of(&host, EMAIL);
    let host = host.with_file(&orphan.credentials_file, CREDENTIAL);
    let made = store_of(&host, SECOND_EMAIL);
    let host = host.with_unwritable_file(&made.credentials_file, "No space left on device");

    let (outcome, _printed) = run_import(&host, AT);

    outcome.expect_err("one Credential cannot be stored");
    assert!(
        host.path_exists(&orphan.config_dir),
        "the directory this Import did not make is still there, and on macOS it \
         is the only thing that can still name the store beside it"
    );
    assert!(
        !host.path_exists(&made.config_dir),
        "while the one it did make went back out"
    );
    assert!(
        !host.path_exists(&orphan.credentials_file),
        "and the Credential this Import wrote into it came back out with the \
         rest: a live Credential in a Profile the registry does not name is \
         what discarding a Profile exists to prevent"
    );
    assert!(
        !host.path_exists(&orphan.identity_file),
        "and so did the `.claude.json`, which came out of the same Export and \
         routinely carries an API key in an MCP server's `env` block"
    );
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("already on this machine")),
        "and the one that stayed is said rather than left to be found: {:?}",
        host.notes()
    );
}

/// A Profile is made for the `.claude.json` alone where the Export carries no
/// Credential, so a rollback that forgets that store destroys a refresh token
/// this Import never wrote and nothing can recover.
#[test]
fn a_rollback_leaves_a_credential_this_import_never_wrote() {
    // An Export where the third Account is Quarantined and its store was already
    // empty when it was written, so it travels with a `.claude.json` and nothing
    // else.
    let source = machine_with_three_accounts();
    quarantine_for(&source, THIRD_EMAIL, Quarantine::RenewalRejected);
    let store = store_of(&source, THIRD_EMAIL);
    source.forget_keychain_item(&store.keychain_service, &store.keychain_account);
    let source = source.with_secrets(&[PASSPHRASE, PASSPHRASE]);
    run_export(&source, AT).0.expect("the export is written");
    let sealed = source.file(AT).expect("a file was written");

    let host = machine_with_claude_code()
        .with_platform(Platform::Other)
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE])
        .with_unwritable_file(REGISTRY_PATH, "No space left on device");
    // A directory the registry never named, holding somebody's live Credential —
    // the leftover a `perch add` that died at the browser step leaves.
    let landing = store_of(&host, THIRD_EMAIL);
    let host = host.with_file(&landing.credentials_file, THIRD_CREDENTIAL);

    let (outcome, _printed) = run_import(&host, AT);

    outcome.expect_err("the registry cannot be written");
    assert_eq!(
        host.file(&landing.credentials_file).as_deref(),
        Some(THIRD_CREDENTIAL),
        "the Export carried no Credential for this Account, so the one in its \
         store is not this Import's to take back"
    );
    assert!(
        !host.path_exists(&landing.identity_file),
        "and the `.claude.json` this Import did write came back out"
    );
}

/// Every Credential is already in a store by the time the registry is written,
/// so a registry that will not go down leaves Profiles no command could name.
#[test]
fn a_registry_that_cannot_be_written_takes_every_profile_back_out_with_it() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE])
        .with_unwritable_file(REGISTRY_PATH, "No space left on device");

    let (outcome, _printed) = run_import(&host, AT);

    let refused = outcome.expect_err("the registry cannot be written");
    assert!(
        refused.to_string().contains("taken back out again"),
        "{refused}"
    );
    assert_eq!(registry_on(&host), None, "no half-populated registry");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(credential_of(&host, email), None, "{email} left nothing");
        assert!(
            !host.path_exists(&store_of(&host, email).config_dir),
            "{email}'s Profile is not left orphaned"
        );
    }
}

#[test]
fn an_export_or_a_registry_from_a_newer_perch_is_refused_rather_than_guessed_at() {
    let opened = perch::export::unseal(&an_export_of_a_whole_machine(), PASSPHRASE)
        .expect("it opens with the passphrase it was sealed with");

    // Stamped on a clone rather than built with `..opened`: an `Export` wipes
    // itself when it is dropped, and a type with a `Drop` cannot have its fields
    // moved out.
    let mut ahead_by_envelope = opened.clone();
    ahead_by_envelope.version = perch::export::CURRENT_VERSION + 1;
    let mut ahead_by_registry = opened;
    ahead_by_registry.registry.version = perch::registry::CURRENT_VERSION + 1;

    for ahead in [ahead_by_envelope, ahead_by_registry] {
        let sealed = perch::export::seal(&ahead, PASSPHRASE).expect("it seals");
        let host = machine_with_claude_code()
            .with_file(AT, &sealed)
            .with_secrets(&[PASSPHRASE]);

        let refused = run_import(&host, AT)
            .0
            .expect_err("this build does not understand it");
        assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
        assert_eq!(registry_on(&host), None);
        assert_eq!(credential_of(&host, EMAIL), None);
    }
}

#[test]
fn an_account_the_export_held_no_credential_for_is_restored_and_said_so() {
    let host = machine_with_three_accounts();
    let store = store_of(&host, THIRD_EMAIL);
    host.forget_keychain_item(&store.keychain_service, &store.keychain_account);
    let host = host.with_secrets(&[PASSPHRASE, PASSPHRASE]);
    run_export(&host, AT).0.expect("the export is written");
    let sealed = host.file(AT).expect("a file was written");

    let onto = a_new_machine_holding(&sealed);
    let (outcome, printed) = run_import(&onto, AT);
    outcome.expect("the import lands");

    assert_eq!(registry_of(&onto).accounts.len(), 3);
    assert_eq!(credential_of(&onto, THIRD_EMAIL), None);
    assert!(printed.contains(THIRD_EMAIL), "{printed}");
    assert!(
        printed.contains(&format!("perch relogin {THIRD_EMAIL}")),
        "with one Account to name, the repair names it: {printed}"
    );
}

/// A sentence agreeing its noun with the whole list and then closing with
/// `how_to_repair(bare[0])` tells somebody who restored three credential-less
/// Accounts to repair the first. The Export's mirror names none of them.
#[test]
fn an_import_that_restored_several_without_credentials_does_not_name_one_of_them() {
    let host = machine_with_three_accounts();
    for email in [SECOND_EMAIL, THIRD_EMAIL] {
        let store = store_of(&host, email);
        host.forget_keychain_item(&store.keychain_service, &store.keychain_account);
    }
    let host = host.with_secrets(&[PASSPHRASE, PASSPHRASE]);
    run_export(&host, AT).0.expect("the export is written");
    let sealed = host.file(AT).expect("a file was written");

    let onto = a_new_machine_holding(&sealed);
    let (outcome, printed) = run_import(&onto, AT);
    outcome.expect("the import lands");

    assert!(
        printed.contains(SECOND_EMAIL) && printed.contains(THIRD_EMAIL),
        "both are named as having arrived without one: {printed}"
    );
    assert!(
        printed.contains("perch relogin <target>"),
        "and the repair is offered over either, rather than over whichever \
         happened to be first: {printed}"
    );
    for email in [SECOND_EMAIL, THIRD_EMAIL] {
        assert!(
            !printed.contains(&format!("perch relogin {email}")),
            "{email} is not singled out: {printed}"
        );
    }
}

#[test]
fn nothing_the_export_holds_reaches_standard_output() {
    let host = a_new_machine_holding(&an_export_of_a_whole_machine());

    let (outcome, printed) = run_import(&host, AT);
    outcome.expect("the import lands");

    for secret in [
        PASSPHRASE,
        "sk-ant-ort01-test",
        "sk-ant-oat01-test",
        "BEGIN AGE ENCRYPTED FILE",
    ] {
        assert!(
            !printed.contains(secret),
            "`{secret}` was printed: {printed}"
        );
    }
    // One line, asserted whole: how many, and from where. That nothing arrives
    // active and that an Import carries the whole registry are true of every
    // Import, so the guide establishes them rather than this line.
    assert_eq!(
        printed.trim_end().lines().last(),
        Some(format!("Imported 3 Accounts from {AT}.").as_str()),
        "{printed}"
    );
}

/// Asserted end to end, because each half is only as good as the other being
/// able to read what it wrote.
#[test]
fn what_an_export_wrote_is_what_an_import_reads_back() {
    let from = machine_with_three_accounts();
    declare_group(&from, "work");
    move_to_group(&from, EMAIL, "work")
        .0
        .expect("the Account joins the Group");
    let exported = {
        let host = from.with_secrets(&[PASSPHRASE, PASSPHRASE]);
        run_export(&host, AT).0.expect("the export is written");
        let mut registry = registry_of(&host);
        // The one thing that deliberately does not travel.
        registry.settle(None);
        (host.file(AT).expect("a file was written"), registry)
    };

    let onto = a_new_machine_holding(&exported.0);
    run_import(&onto, AT).0.expect("the import lands");

    assert_eq!(registry_of(&onto), exported.1);
}

/// The identity file is the half of a Profile that cannot be reconstructed
/// *faithfully*: Claude Code's `oauthAccount` block carries fields beyond the
/// registry's four, and a Run Carries from it (ADR everything-but-the-account).
#[test]
fn an_imported_profile_holds_the_identity_file_its_account_had() {
    let from = machine_with_two_accounts();
    let sealed = {
        let host = from.with_secrets(&[PASSPHRASE, PASSPHRASE]);
        run_export(&host, AT).0.expect("the export is written");
        host.file(AT).expect("a file was written")
    };

    let onto = a_new_machine_holding(&sealed);
    run_import(&onto, AT).0.expect("the import lands");

    for email in [EMAIL, SECOND_EMAIL] {
        let held = onto
            .file(store_of(&onto, email).identity_file)
            .unwrap_or_else(|| panic!("{email}'s Profile holds an identity file"));
        assert!(
            held.contains(email),
            "and it is that Account's own block: {held}"
        );
    }
}

/// The Identity the registry records is enough to compose the block Claude Code
/// would have written.
#[test]
fn an_account_whose_export_carried_no_identity_file_still_gets_one() {
    let from = machine_with_two_accounts();
    let unreadable = store_of(&from, SECOND_EMAIL).identity_file;
    let from = from.with_unreadable_file(unreadable, "Permission denied (os error 13)");
    let sealed = {
        let host = from.with_secrets(&[PASSPHRASE, PASSPHRASE]);
        run_export(&host, AT).0.expect("the export is written");
        host.file(AT).expect("a file was written")
    };

    let onto = a_new_machine_holding(&sealed);
    run_import(&onto, AT).0.expect("the import lands");

    let held = onto
        .file(store_of(&onto, SECOND_EMAIL).identity_file)
        .expect("one was composed rather than left absent");
    assert!(held.contains(SECOND_EMAIL), "{held}");
}

/// An Import runs on a machine holding no *Accounts*, which is not a machine
/// holding no Profiles — so somebody with a terminal open against one of those
/// directories meets the mid-task logout
/// ADR a-profile-is-live-by-evidence refuses everywhere else.
#[test]
fn an_import_into_a_profile_a_client_is_holding_writes_nothing() {
    let sealed = an_export_of_a_whole_machine();
    let host = a_new_machine_holding(&sealed);
    // What a Purge that could not finish leaves: a Profile directory with a
    // client in it and no registry naming it.
    let profile = perch::registry::profile_dir_for(&host, EMAIL).expect("home is known");
    let host = client_running_against(host, &profile.to_string_lossy(), 4242);

    let (outcome, _) = run_import(&host, AT);

    let error = outcome.expect_err("that session is holding the Credential");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains(EMAIL), "{error}");
    assert!(
        registry_on(&host).is_none(),
        "and nothing was imported: an Import is whole or it did not happen"
    );
    assert_eq!(
        credential_of(&host, EMAIL),
        None,
        "the Credential that session is using was not written over"
    );
}

/// `load` normalizes before it validates, "so `load` and `save` judge one
/// shape". An Import writes a registry without reading one, so it has to judge
/// that same shape — and it validated what arrived instead, over two fields it
/// clears on the way in. A file every command on the machine would go on to read
/// was refused, and the refusal named a Check the Import was never going to keep.
#[test]
fn an_export_is_judged_by_the_shape_that_will_be_written_rather_than_the_one_that_arrived() {
    let mut export =
        perch::export::unseal(&an_export_of_a_whole_machine(), PASSPHRASE).expect("it opens");
    // A Check against a Group nothing declares, which is what a hand-edited
    // registry — or one whose Group was removed beside it — carries.
    export
        .registry
        .record_check("nobody-declared-this", Utc::now());
    let sealed = perch::export::seal(&export, PASSPHRASE).expect("it seals");
    let host = a_new_machine_holding(&sealed);

    let (outcome, said) = run_import(&host, AT);

    outcome.unwrap_or_else(|refused| {
        panic!("a Check the Import discards is no reason to refuse the file: {refused}")
    });
    assert!(
        registry_on(&host).is_some_and(|registry| registry.checks.is_empty()),
        "and nothing arrives having just been checked: {said}"
    );
}
