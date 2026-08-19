//! `perch holdings import` — a whole machine, put back (ADR 0014).
//!
//! The other half of the pair that makes "I can move to a new machine" true.
//! Everything here is about the two ways it could quietly fail somebody: by
//! restoring less than the whole, and by leaving a machine holding half of one.

mod common;

use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
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

/// The guard above asks whether every Credential in the file is listed, and it
/// asks with `registry::same_name`, which folds case. Placement looked the same
/// keys up in a `BTreeMap`, which does not.
///
/// So an Export spelling a credential key `SOMEONE@example.com` beside an
/// account entry `someone@example.com` passed the check — the key *is* listed —
/// and then missed at placement, leaving that Account with no Profile and no
/// Credential while the Import reported success. That is exactly the partial
/// restore the guard exists to prevent, arriving through the one comparison it
/// did not share with the placement it guards.
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
    // The report asked the same question a third way, and got a third answer:
    // `without_a_credential` was the one caller still doing a lookup by key, so
    // an Account whose Credential had just been placed was announced as
    // "restored without one" and its owner sent to `perch relogin` for an
    // Account that was fine.
    assert!(
        !said.contains("held no Credential"),
        "and the report says so, rather than sending somebody to repair an \
         Account that is whole: {said}"
    );
}

/// **The terminal going away after the Import lands does not un-import it.**
///
/// `report` is the last fallible thing an Import does, and everything before it
/// has already succeeded: every Credential is placed and the registry is
/// written. A closed pty, a `SIGHUP`, a `head` that stopped reading — any of
/// them fails that `say`, and raised bare it read as an Import that had not
/// happened. The obvious next move then meets
/// `refuse_a_machine_that_is_not_empty`, whose advice is that `perch holdings
/// purge` "gives the machine back and is what makes room" — on a machine that
/// is already restored.
///
/// `commands::export` carries `landed` for this and `purge::still_standing`
/// closes the same gap; an Import was the one of the three that never got it.
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

/// The fourth command with an unbounded prompt under the registry lock, and
/// the one that skipped the guard the other three take.
///
/// A passphrase is the one wait in Perch with no bound on it, so it is the one
/// place a hold can go stale under a command that is behaving perfectly.
/// Another `perch` may then have claimed the abandoned lock and put an Account
/// down — and `import::place` writes every Credential the file holds before
/// `registry::save` ever asks. Finding out at the save means finding out after
/// the rollback has deleted the Profile and the keychain item that other Perch
/// just created, under a line reading "Nothing was imported."
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

/// The whole of what the pair promises: a new machine arrives with the setup the
/// old one had rather than a pile of nameless logins.
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

/// A broken login somebody still owns travels with the reason it broke, so the
/// machine it lands on says what happened rather than that Perch lost something.
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
/// store it came out of, so an Export taken on a Mac restores into files on
/// Linux — and the machine it came from is none the wiser either way.
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

/// Being active is a claim about which Credential is in this machine's Default
/// Profile, and an Import puts none there. Whatever Claude Code was logged in as
/// goes on running until the user Switches.
///
/// Asserted as the two data it is about, and no longer as a `perch switch` in
/// the report: nothing arrives active on any Import, so pointing at the Switch
/// was Perch pre-empting a disappointment on every run of the command (ADR
/// 0061). The guide is where an Import is described, and it says so.
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

/// The same rule, for the other claim a registry makes about right now.
///
/// `checks` is what a `perch watcher check` on the *other* machine did — when
/// it Switched — and the cooldown is measured from it. Carried across, an
/// Export taken this morning has the first check on the new machine reporting
/// `cooling` on the strength of something that happened somewhere else.
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

/// Every other command reads the registry through adoption, which takes the
/// existing Claude Code login over the first time Perch runs. An Import that did
/// that would make the machine non-empty on the way to refusing itself for being
/// non-empty.
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

/// Merging is where every hard case lives — the same Account on both sides one
/// Rotation apart, an Alias meaning different Accounts on two machines. That is
/// a real feature and it is not this one, so the refusal names the command that
/// makes room.
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

/// A wrong passphrase means the file never opened, and a file that never opened
/// cannot have restored anything.
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

/// Typing nothing is the same skip an optional passphrase would have been, and
/// end of input is a pipe that closed rather than somebody's answer.
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

/// The same rule the Export was written under: a passphrase passed as an
/// argument sits in the process table for anything on the machine to read, so
/// there is no flag and the refusal names the terminal rather than a way round
/// it.
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

/// A path that is a typo is answered by naming the path, rather than by advice
/// about a machine the user was never asking about.
#[test]
fn a_path_that_holds_nothing_is_said_rather_than_guessed_at() {
    let host = machine_with_claude_code().with_secrets(&[PASSPHRASE]);

    let (outcome, _printed) = run_import(&host, "/Users/someone/typo.age");

    let refused = outcome.expect_err("there is no file there");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert!(refused.to_string().contains("typo.age"), "{refused}");
}

/// Something that is not an Export at all is told apart from a passphrase that
/// is wrong, because one of the two is worth typing again and the other is not.
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

/// A binary `age` file is refused as the encoding it is, not as a byte stream
/// that would not decode.
///
/// An Export is `age`'s *armored* form — its text encoding — which is what lets
/// it go through the Host port's ordinary private write. So the read is a read
/// of text, and plain `age -p` writes binary: the file failed UTF-8 decoding
/// before `unseal` saw it, and came back as "stream did not contain valid
/// UTF-8". The person who meets that is somebody who took their own backup with
/// `age -p`, reading it on the day the machine it would have restored is gone.
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

/// An Import that fails part way leaves nothing behind: a machine holding some
/// of an Export is the partial restore the file exists to prevent, wearing an
/// accident's clothes.
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

/// And it takes back what it *made*, which is not the same as what it wrote
/// into.
///
/// An Import refuses a machine holding an Account, and that was read one step
/// wider than it goes: it is the registry that has to be empty, and a Profile
/// directory nothing names outlives every command that would have named it — a
/// `perch add` that died at the browser step leaves one, and so does a Purge
/// that could not empty a store. The rollback deleted it along with its own,
/// and on macOS the keychain item outside it that only that directory's name
/// could still reach went with it: a live refresh token for a login nobody was
/// importing.
#[test]
fn a_rollback_leaves_a_profile_that_was_already_on_the_machine_where_it_is() {
    let sealed = an_export_of_a_whole_machine();
    let host = machine_with_claude_code()
        .with_platform(Platform::Other)
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    // A Profile from a `perch add` that never finished: a directory holding a
    // Credential, and a registry that has never named it. The first of the
    // three, so the Import has written into it by the time the second one
    // fails.
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
        host.notes()
            .iter()
            .any(|note| note.contains("already on this machine")),
        "and the one that stayed is said rather than left to be found: {:?}",
        host.notes()
    );
}

/// The other end of the same promise, and the worse half of it: every Credential
/// is already in a store by the time the registry is written, so a registry that
/// will not go down would otherwise leave a machine full of Profiles no command
/// could name.
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

/// Both of Perch's formats are versioned, and both guards are about the future:
/// the machine holding two builds, where the wrong answer is a file half-read
/// rather than refused.
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

/// An Account the Export carries no Credential for is still restored — one Perch
/// has forgotten is worse news than one that needs logging in again — and the
/// command says which, with the command that ends it.
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

/// The same with more than one, which nothing was asking.
///
/// The sentence agreed its noun with the whole list — "the Accounts were
/// restored without one" — and then closed with `how_to_repair(bare[0])`, which
/// says "`perch relogin a@example.com` logs *it* in again". So somebody who had
/// just restored three credential-less Accounts was told to repair the first.
/// The mirror of this in `perch holdings export` gets it right by naming none
/// of them.
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

/// Not the passphrase, and not a Credential. The prompts and one line naming
/// what was restored are what a command owes; nothing the file holds is.
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
    // One line, asserted whole (ADR 0043): how many and from where. That
    // nothing arrives active, and that an Import carries the whole registry,
    // are true of every Import — so the guide is where they are established
    // rather than here (ADR 0061).
    assert_eq!(
        printed.trim_end().lines().last(),
        Some(format!("Imported 3 Accounts from {AT}.").as_str()),
        "{printed}"
    );
}

/// The pair is the point: what a Purge gives back, an Import puts back. Asserted
/// end to end, because each half is only as good as the other one being able to
/// read what it wrote.
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

/// A Profile holds two things, and both of them travel.
///
/// The Credential is the one that cannot be reconstructed at all. The identity
/// file is the one that cannot be reconstructed *faithfully*: the `oauthAccount`
/// block Claude Code wrote carries fields beyond the four the registry records,
/// and a Switch prefers it verbatim over anything Perch composes. Restoring
/// without it puts every Account into the degraded state adoption goes out of
/// its way to keep the first one out of — and leaves each Profile with nothing
/// for a Run to Carry into, so the onboarding dialog comes back on every Run
/// (ADR 0003).
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

/// An Export written before its Profile ever had one still restores a Profile a
/// Run can Carry into: the Identity the registry records is enough to compose
/// the block Claude Code would have written.
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

/// A Profile something is running against is one nothing writes into, and an
/// Import was the one writer that never asked.
///
/// An Import runs on a machine holding no *Accounts*, which is not the same as
/// a machine holding no Profiles: a Purge that failed at its last step leaves
/// `profiles/` populated with no registry above it, and `Touched::was_already_there`
/// exists because of exactly that. Somebody who then opens a terminal against
/// one of those directories and runs `perch holdings import` had that session's
/// Credential replaced underneath it — the mid-task logout ADR 0005 refuses
/// everywhere else.
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
