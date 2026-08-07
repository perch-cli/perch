//! `perch import` — a whole machine, put back (ADR 0014).
//!
//! The other half of the pair that makes "I can move to a new machine" true.
//! Everything here is about the two ways it could quietly fail somebody: by
//! restoring less than the whole, and by leaving a machine holding half of one.

mod common;

use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_GENERAL, EXIT_INVALID, EXIT_NOT_FOUND};
use perch::host::{FakeHost, Host, Platform};
use perch::registry::{Quarantine, Registry};

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
        !registry.account(THIRD_EMAIL).unwrap().enabled,
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
#[test]
fn nothing_is_made_active_by_an_import() {
    let sealed = an_export_of_a_whole_machine();
    let host = logged_in_machine()
        .with_file(AT, &sealed)
        .with_secrets(&[PASSPHRASE]);
    let live_before = host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME);

    let (outcome, printed) = run_import(&host, AT);
    outcome.expect("the import lands");

    assert_eq!(registry_of(&host).active, None);
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME),
        live_before,
        "the live Credential is not an Import's to replace"
    );
    assert!(printed.contains("perch switch"), "{printed}");
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
    assert_eq!(registry.active, None);
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
    assert!(refused.to_string().contains("perch purge"), "{refused}");
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
    assert_eq!(refused.exit_code(), EXIT_GENERAL, "{refused}");
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

    let ahead_by_envelope = perch::export::Export {
        version: perch::export::CURRENT_VERSION + 1,
        ..opened.clone()
    };
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
    assert!(printed.contains("perch relogin"), "{printed}");
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
    assert!(printed.contains(AT), "what was read is said: {printed}");
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
        registry.active = None;
        (host.file(AT).expect("a file was written"), registry)
    };

    let onto = a_new_machine_holding(&exported.0);
    run_import(&onto, AT).0.expect("the import lands");

    assert_eq!(registry_of(&onto), exported.1);
}
