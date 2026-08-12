//! `perch export` — the whole machine in one `age` file (ADR 0014).
//!
//! The command that makes a dead machine, a mistaken `perch remove` and a new
//! laptop cost something less than a login for every subscription. Everything
//! here is about the two ways it could quietly fail somebody: by writing less
//! than the whole, and by writing something they cannot read back.

mod common;

use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND};
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
        Some(65),
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
    assert_eq!(
        refused.exit_code(),
        EXIT_INVALID,
        "a request Perch understood and refused on its own terms, which is what \
         a script has to be able to tell from a disk that filled up: {refused}"
    );
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

/// An Export of a machine Perch holds nothing on. There is a difference between
/// a backup of nothing and a backup that failed, and writing an empty file
/// would make the two indistinguishable on the day it is restored from.
#[test]
fn exporting_a_machine_with_no_accounts_refuses_rather_than_writing_an_empty_file() {
    // The state is reached the only way it can be: the last Account given up.
    // A machine that never had one is refused by adoption long before this.
    let host = typing_the_passphrase(logged_in_machine());
    run_remove_with(
        &host,
        perch::commands::remove::RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    )
    .0
    .expect("the last Account is given up");
    assert!(registry_of(&host).accounts.is_empty(), "as we start");

    let (result, _) = run_export(&host, AT);

    let refusal = result.expect_err("there is nothing to export");
    let said = refusal.to_string();
    assert!(
        said.contains("Perch holds no Accounts, so there is nothing to export."),
        "{said}"
    );
    assert!(said.contains("perch add"), "it names the way out: {said}");
    assert_eq!(host.file(AT), None, "and nothing was written");
}

/// A path with no directory in it names the current directory, which exists by
/// definition. Refusing it as "no such directory" would turn the shortest form
/// of the command into the one that never works.
#[test]
fn exporting_to_a_bare_filename_is_not_refused_for_want_of_a_directory() {
    let host = a_machine_worth_backing_up();

    let (result, printed) = run_export(&host, "perch-backup.age");

    result.expect("the current directory is somewhere that exists");
    assert!(printed.contains("Exported 3 Accounts"), "{printed}");
    assert!(
        host.file("perch-backup.age").is_some(),
        "the file is where it was asked for"
    );
}

/// More than one Account with no Credential anywhere. The sentence naming them
/// has to agree in number, because this is the line that tells somebody their
/// backup is less complete than they think.
#[test]
fn an_export_says_accounts_in_the_plural_when_several_have_no_credential() {
    let host = machine_with_three_accounts();
    for email in [SECOND_EMAIL, THIRD_EMAIL] {
        let store = store_of(&host, email);
        host.forget_keychain_item(&store.keychain_service, LOGIN_NAME);
        host.remove_file(&store.credentials_file).ok();
        quarantine_for(&host, email, Quarantine::RenewalRejected);
    }
    let host = typing_the_passphrase(host);

    let (result, printed) = run_export(&host, AT);

    result.expect("an Export is still written");
    assert!(
        printed.contains("carries the Accounts without one"),
        "two of them, so the plural: {printed}"
    );
    assert!(
        printed.contains(SECOND_EMAIL) && printed.contains(THIRD_EMAIL),
        "and it names which: {printed}"
    );
    assert!(printed.contains("perch relogin"), "{printed}");
}

/// A Credential that Anthropic Rotated while the Account was active, so the live
/// copy is ahead of the one in that Account's own Profile.
const ROTATED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-rotated","refreshToken":"sk-ant-ort01-rotated","expiresAt":1790000000000,"subscriptionType":"pro"}}"#;

/// The active Account's Credential lives in the Default Profile, and a Renewal
/// Rotates it there — the copy in its own Profile only catches up when a Switch
/// away Captures it (ADR 0006). Read from the Profile, the one Account the user
/// is actually working in travels as a refresh token Anthropic has already
/// retired, and `perch watch` Renews that Account every few minutes. A restore
/// would then bring back every Account but the one they used most, and they
/// would find out on the day they needed it.
#[test]
fn the_active_accounts_credential_is_the_live_one_rather_than_the_copy_in_its_profile() {
    let host = machine_with_three_accounts();
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "the fixture's premise"
    );
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    let host = typing_the_passphrase(host);

    run_export(&host, AT).0.expect("the export is written");

    let export = opened(&host, AT);
    assert_eq!(
        export.credentials.get(EMAIL).map(String::as_str),
        Some(ROTATED),
        "the Rotation the active Account is living on has to be what travels"
    );
    assert_eq!(
        export.credentials.get(SECOND_EMAIL).map(String::as_str),
        Some(SECOND_CREDENTIAL),
        "and every other Account still travels as its own Profile holds it"
    );
}

/// The live Credential is only the active Account's where the Default Profile
/// says it is theirs. A login made outside Perch leaves `.claude.json` naming
/// somebody Perch does not hold, and exporting those bytes under the active
/// Account's address would restore a machine where one Account answers as
/// another — the same evidence a Capture demands before it copies that
/// Credential anywhere.
#[test]
fn a_live_credential_belonging_to_somebody_else_is_not_exported_as_the_active_accounts() {
    let host = machine_with_three_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.write_private_file(
        std::path::Path::new(IDENTITY_PATH),
        &SECOND_IDENTITY_FILE.replace(SECOND_EMAIL, "stranger@example.com"),
    )
    .expect("the identity file is written");
    let host = typing_the_passphrase(host);

    run_export(&host, AT).0.expect("the export is written");

    assert_eq!(
        opened(&host, AT).credentials.get(EMAIL).map(String::as_str),
        Some(CREDENTIAL),
        "the copy in its own Profile is the honest answer for it"
    );
}

/// An Export is the command you run on a machine you are giving up, and one of
/// the things people give up on such a machine is Claude Code.
///
/// Nothing in an Export needs it. Every Credential is already in a store whose
/// path Perch derives on its own, and the one place a version was asked for is
/// reading the Default Profile's Identity — which is not evidence against the
/// active Account when it is absent or unreadable, and is no more so when there
/// is no Claude Code to ask. Propagated, it refused the whole command after the
/// passphrase had been typed twice, and took `perch purge` with it, because the
/// Export that offers to save you first is one that stops the Purge when it
/// fails.
#[test]
fn an_export_is_written_by_a_machine_that_no_longer_has_claude_code_on_it() {
    let host = machine_with_three_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.remove_file(std::path::Path::new(CLAUDE_BIN))
        .expect("Claude Code is uninstalled");
    let host = typing_the_passphrase(host);

    let (outcome, printed) = run_export(&host, AT);

    outcome.expect("an Export needs no Claude Code");
    let export = opened(&host, AT);
    assert_eq!(export.accounts(), 3, "{printed}");
    assert_eq!(
        export.credentials.get(EMAIL).map(String::as_str),
        Some(ROTATED),
        "and the active Account still travels as the live Credential it is on: \
         no Identity could be read, and an Identity nothing can read has never \
         been evidence against"
    );
}
