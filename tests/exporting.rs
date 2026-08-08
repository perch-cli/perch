//! `perch export` — the whole machine in one `age` file (ADR 0014).
//!
//! The command that makes a dead machine, a mistaken `perch remove` and a new
//! laptop cost something less than a login for every subscription. Everything
//! here is about the two ways it could quietly fail somebody: by writing less
//! than the whole, and by writing something they cannot read back.

mod common;

use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_GENERAL, EXIT_INVALID, EXIT_NOT_FOUND};
use perch::export;
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host, PRIVATE_FILE_MODE};
use perch::registry::Quarantine;

const PASSPHRASE: &str = "correct horse battery staple";
const AT: &str = "/Users/someone/perch-backup.age";

/// A terminal with somebody at it who types the same passphrase twice, which is
/// the only way an Export is ever written.
fn typing_the_passphrase(host: FakeHost) -> FakeHost {
    host.with_secrets(&[PASSPHRASE, PASSPHRASE])
}

/// Three Accounts with everything a user gives them: two in a Group, one named,
/// one taken out of Cycling, and one broken.
fn a_machine_worth_backing_up() -> FakeHost {
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
    typing_the_passphrase(host)
}

/// What the file at a path holds, given the passphrase it was written with.
fn opened(host: &FakeHost, path: &str) -> export::Export {
    let sealed = host.file(path).expect("a file was written");
    export::unseal(&sealed, PASSPHRASE).expect("it opens with the passphrase it was sealed with")
}

/// The whole of what the file promises: every Account, every Credential, and
/// every name and rule the user gave them. Restoring the Credentials alone would
/// leave a new machine holding working Accounts stripped of all of it.
#[test]
fn an_export_holds_every_account_every_credential_and_everything_said_about_them() {
    let host = a_machine_worth_backing_up();

    let (outcome, _printed) = run_export(&host, AT);
    outcome.expect("the export is written");

    let export = opened(&host, AT);
    let registry = &export.registry;
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
            export.credentials.get(email).map(String::as_str),
            Some(credential),
            "{email}'s Credential is in the file"
        );
    }
}

/// A Quarantined Account is a broken login somebody still owns, and dropping it
/// from the Export would restore a machine that has quietly forgotten it. The
/// reason travels too: "this Account is broken" and "Anthropic would not renew
/// its Credential" are different pieces of news.
#[test]
fn a_quarantined_account_is_exported_as_quarantined_with_its_reason() {
    let host = a_machine_worth_backing_up();

    run_export(&host, AT).0.expect("the export is written");

    let export = opened(&host, AT);
    assert_eq!(
        export.registry.account(THIRD_EMAIL).unwrap().quarantine,
        Some(Quarantine::RenewalRejected),
    );
}

/// The file is `age`'s own, in `age`'s text encoding, so it can be opened by the
/// standard `age` command on a machine that has never heard of Perch. A backup
/// readable only by the tool that wrote it is a worse backup than one whose
/// format somebody else maintains.
#[test]
fn the_file_is_an_age_file_and_holds_nothing_readable_without_the_passphrase() {
    let host = a_machine_worth_backing_up();

    run_export(&host, AT).0.expect("the export is written");

    let sealed = host.file(AT).expect("a file was written");
    assert!(
        sealed.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"),
        "{}",
        &sealed[..sealed.len().min(80)],
    );
    for readable in [EMAIL, "sk-ant-ort01-test", "overflow", "work"] {
        assert!(
            !sealed.contains(readable),
            "`{readable}` should not be legible in the file"
        );
    }
    assert!(
        export::unseal(&sealed, "some other passphrase").is_err(),
        "and only the passphrase it was sealed with opens it"
    );
}

/// Prompted and confirmed, and never through the path that echoes: a passphrase
/// shown as it is typed is one in somebody's scrollback, and a mistyped one is a
/// file nobody discovers is unreadable until the machine it would have restored
/// is gone.
#[test]
fn the_passphrase_is_asked_for_twice_and_never_shown() {
    let host = a_machine_worth_backing_up();
    host.forget_effects();

    let (outcome, printed) = run_export(&host, AT);
    outcome.expect("the export is written");

    assert_eq!(
        host.effects()
            .iter()
            .filter(|effect| **effect == Effect::AskedInSecret)
            .count(),
        2,
        "prompted, and confirmed"
    );
    assert!(
        !host.effects().contains(&Effect::Asked),
        "nothing about an Export is asked at the prompt that shows what is typed"
    );
    assert!(printed.contains("Passphrase:"), "{printed}");
    assert!(printed.contains("Again:"), "{printed}");
}

#[test]
fn a_confirmation_that_does_not_match_writes_nothing() {
    let host =
        a_machine_worth_backing_up().with_secrets(&[PASSPHRASE, "correct hose battery staple"]);

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("the two do not match");
    assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
    assert!(refused.to_string().contains("do not match"), "{refused}");
    assert_eq!(host.file(AT), None, "nothing was written");
}

/// Required, not offered: an optional passphrase is one people skip, and the
/// failure is silent until it isn't. Typing nothing is the same skip.
#[test]
fn an_empty_passphrase_is_refused_rather_than_accepted_as_none() {
    for typed in [&["", ""], &["   ", "   "]] {
        let host = a_machine_worth_backing_up().with_secrets(typed);

        let refused = run_export(&host, AT)
            .0
            .expect_err("that is not a passphrase");
        assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
        assert_eq!(host.file(AT), None, "nothing was written");
    }
}

/// End of input is not a passphrase either — a pipe that closed must never read
/// as somebody having chosen one.
#[test]
fn end_of_input_at_the_prompt_writes_nothing() {
    let host = a_machine_worth_backing_up().with_secrets(&[]);

    run_export(&host, AT).0.expect_err("nobody typed anything");
    assert_eq!(host.file(AT), None);
}

/// There is deliberately no flag that answers ahead of time, so the refusal
/// names the terminal rather than a way round it.
#[test]
fn without_a_terminal_the_export_is_refused_and_says_what_is_needed() {
    let host = a_machine_worth_backing_up().without_terminal();

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("there is nobody to type a passphrase");
    assert_eq!(refused.exit_code(), EXIT_GENERAL, "{refused}");
    assert!(refused.to_string().contains("no terminal"), "{refused}");
    assert!(
        refused.to_string().contains("process table"),
        "a refusal that offered a flag would be offering a passphrase in argv: {refused}"
    );
    assert_eq!(host.file(AT), None, "nothing was written");
}

/// An Export reads what is stored. A Renewal may Rotate, and a backup that
/// retired the refresh token of every Account on the way to recording it would
/// break the machine it was taken from.
#[test]
fn nothing_is_renewed_or_rotated_by_an_export() {
    let host = a_machine_worth_backing_up();
    let before: Vec<Option<String>> = [EMAIL, SECOND_EMAIL, THIRD_EMAIL]
        .iter()
        .map(|email| credential_of(&host, email))
        .collect();

    run_export(&host, AT).0.expect("the export is written");

    assert!(
        host.http_calls().is_empty(),
        "an Export asks Anthropic nothing: {:?}",
        host.http_calls()
    );
    let after: Vec<Option<String>> = [EMAIL, SECOND_EMAIL, THIRD_EMAIL]
        .iter()
        .map(|email| credential_of(&host, email))
        .collect();
    assert_eq!(before, after, "every store holds what it held");
}

/// The file grants full access to every Account somebody owns. It is created at
/// the mode a Credential is, rather than tightened after the fact — a file
/// `chmod`ed afterwards is a file that was briefly readable (ADR 0020).
#[test]
fn the_file_is_created_readable_by_its_owner_alone() {
    let host = a_machine_worth_backing_up();
    host.forget_effects();

    run_export(&host, AT).0.expect("the export is written");

    assert_eq!(host.mode_of(AT), Some(PRIVATE_FILE_MODE));
    assert!(
        host.effects()
            .contains(&Effect::WrotePrivateFile(AT.into())),
        "created private rather than written and then narrowed"
    );
}

/// Not the ciphertext, not a Credential, not the passphrase.
///
/// The reading taken of "nothing about the export is written to standard
/// output": nothing the file *holds* goes there, and the passphrase never does
/// either. What does go there is the prompts and one line naming what was
/// written — because every other Perch command reports what it did, and a
/// `perch purge` offering to export first (#53) has to be able to say the export
/// happened.
#[test]
fn nothing_the_export_holds_reaches_standard_output() {
    let host = a_machine_worth_backing_up();

    let (outcome, printed) = run_export(&host, AT);
    outcome.expect("the export is written");

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
    assert!(printed.contains(AT), "what was written is said: {printed}");
}

/// The same on the way out of a failure, which is where a command is most
/// likely to say more than it meant to.
#[test]
fn a_write_that_fails_says_so_without_saying_what_it_was_writing() {
    let host = a_machine_worth_backing_up().with_unwritable_file(AT, "No space left on device");

    let (outcome, printed) = run_export(&host, AT);

    let refused = outcome.expect_err("the file cannot be written");
    let said = format!("{refused}{printed}");
    for secret in [PASSPHRASE, "sk-ant-ort01-test", "BEGIN AGE ENCRYPTED FILE"] {
        assert!(!said.contains(secret), "`{secret}` was said: {said}");
    }
    assert!(said.contains("No space left on device"), "{said}");
}

/// A locked keychain is not "this Account has no Credential". Recording it as
/// one would write a file that restores to a machine of logins that do not work,
/// and the user would find out on the day they needed it.
#[test]
fn a_store_that_will_not_say_what_it_holds_stops_the_export_rather_than_shrinking_it() {
    let host = a_machine_worth_backing_up();
    host.lock_keychain("User interaction is not allowed");

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("a store would not answer");
    assert!(refused.to_string().contains("partial restore"), "{refused}");
    assert_eq!(host.file(AT), None, "and nothing was written");
}

/// An Account whose stores hold nothing is still exported, because an Account
/// Perch has forgotten is worse news than one that needs logging in again.
#[test]
fn an_account_with_no_credential_is_exported_without_one_and_said_so() {
    let host = a_machine_worth_backing_up();
    let store = store_of(&host, THIRD_EMAIL);
    host.forget_keychain_item(&store.keychain_service, &store.keychain_account);

    let (outcome, printed) = run_export(&host, AT);
    outcome.expect("the export is written");

    let export = opened(&host, AT);
    assert_eq!(export.registry.accounts.len(), 3);
    assert!(!export.credentials.contains_key(THIRD_EMAIL));
    assert!(printed.contains(THIRD_EMAIL), "{printed}");
    assert!(printed.contains("perch relogin"), "{printed}");
}

/// Reading out of a Live Profile is allowed — only writing into one is refused —
/// so a Run in another terminal is never a reason a backup cannot be taken.
#[test]
fn an_account_something_is_running_against_is_exported_like_any_other() {
    let host = a_machine_worth_backing_up();
    a_run_against(&host, SECOND_EMAIL, host.now());

    run_export(&host, AT).0.expect("the export is written");

    assert_eq!(
        opened(&host, AT)
            .credentials
            .get(SECOND_EMAIL)
            .map(String::as_str),
        Some(SECOND_CREDENTIAL),
    );
}

/// The one argument is a path somebody typed, and what the command does with it
/// is replace whatever is there. A mistyped one would otherwise make a backup
/// command destroy the file it was pointed at — and an Export landing on an
/// older Export is the older backup gone.
#[test]
fn an_export_is_never_written_over_anything() {
    let host = a_machine_worth_backing_up().with_file(AT, "something the user wrote");

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("something is already there");
    assert_eq!(refused.exit_code(), EXIT_CONFLICT, "{refused}");
    assert!(refused.to_string().contains(AT), "{refused}");
    assert_eq!(
        host.file(AT).as_deref(),
        Some("something the user wrote"),
        "what was there is what is there"
    );
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "and nobody was asked to type a passphrase twice for a refusal"
    );
}

/// The refusal above is checked before the prompts and again after them. The
/// window in between is two questions long, and what closes it is a second
/// `perch export` aimed at the same path finishing while this one is still being
/// typed at — the write itself replaces rather than fails.
#[test]
fn a_file_that_arrives_while_the_passphrase_is_typed_is_not_written_over_either() {
    let host = a_machine_worth_backing_up();
    // The fake performs this the first time Perch waits, which is the first
    // prompt: the terminal has to take some time for there to be a window at
    // all.
    let host = host
        .with_a_terminal_that_takes(1_000)
        .once_while_waiting(|host| host.set_file(AT, "an Export somebody else just wrote"));

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("something arrived at the path");
    assert_eq!(refused.exit_code(), EXIT_CONFLICT, "{refused}");
    assert_eq!(
        host.file(AT).as_deref(),
        Some("an Export somebody else just wrote"),
        "the other Export is still there"
    );
}

/// The private write would create the directory, and every one above it, at
/// 0700. That is right for the directories Perch owns and presumptuous for a
/// path somebody typed — where a missing directory is a typo more often than an
/// instruction.
#[test]
fn a_path_whose_directory_is_not_there_is_refused_rather_than_having_one_made() {
    let host = a_machine_worth_backing_up();

    let (outcome, _printed) = run_export(&host, "/Users/someone/backups/2026/perch.age");

    let refused = outcome.expect_err("there is nowhere to write it");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert!(
        refused.to_string().contains("/Users/someone/backups/2026"),
        "{refused}"
    );
    assert!(
        !host.path_exists(std::path::Path::new("/Users/someone/backups")),
        "and no directory was made on the way to the refusal"
    );
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "nor was a passphrase asked for twice for a refusal"
    );
}

/// Nothing to back up is not a file with nothing in it: an empty Export restores
/// to exactly the machine somebody already has, and looks like a backup while
/// being one.
#[test]
fn a_machine_holding_no_accounts_is_told_so_rather_than_given_an_empty_file() {
    let host = typing_the_passphrase(machine_with_claude_code());

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("there is nothing to export");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert_eq!(host.file(AT), None);
}

/// The passphrase is asked for twice, and the registry was read before either
/// prompt. That is the same unbounded wait `perch purge` and `perch remove`
/// re-check their hold across, and `perch export` was the one that did not:
/// it re-asked whether the *path* was still free and never whether the registry
/// was still its own.
///
/// An Account added by another `perch` while somebody was typing is an Account
/// the copy being sealed does not hold — so the file would present itself as
/// everything Perch holds while being a partial one. That is a selective Export,
/// which is the failure the format exists to prevent, and it is found out at the
/// restore rather than here.
#[test]
fn an_export_whose_registry_went_stale_while_the_passphrase_was_typed_writes_nothing() {
    let host = typing_the_passphrase(a_machine_worth_backing_up())
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::registry::lock_spec(host).expect("home is known");
            host.remove_dir_all(&lock.dir).expect("it was abandoned");
            host.create_dir_exclusive(&lock.dir)
                .expect("the other `perch` takes it");
        });

    let (outcome, _) = run_export(&host, AT);

    let refused = outcome.expect_err("this Perch may no longer speak for the registry");
    assert!(
        refused.to_string().contains("Nothing was exported"),
        "{refused}"
    );
    assert_eq!(
        host.file(std::path::Path::new(AT)),
        None,
        "a partial Export is worse than none: it is only found out at the restore"
    );
}
