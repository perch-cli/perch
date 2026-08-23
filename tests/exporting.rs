//! `perch holdings export` — the whole machine in one `age` file
//! (ADR the-holdings-go-out-sealed).
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
use perch::host::prelude::*;
use perch::host::{FakeHost, PRIVATE_FILE_MODE};
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
            export.credentials.get(email).map(String::as_str),
            Some(credential),
            "{email}'s Credential is in the file"
        );
    }
}

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

/// Created at the mode a Credential is rather than tightened afterwards — a file
/// `chmod`ed after the fact was briefly readable
/// (ADR claude-code-chooses-the-store).
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

/// The reading taken of "nothing about the Export is written to standard
/// output": nothing the file *holds* goes there and neither does the passphrase.
/// The prompts and one line naming what was written do.
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
    // One line, asserted whole (ADR perch-says-what-it-did): how many and where.
    // What an Export carries is what an Export *is*, and the prompt says where
    // the passphrase is kept, so neither is said again after the write.
    assert_eq!(
        printed.trim_end().lines().last(),
        Some(format!("Exported 3 Accounts to {AT}.").as_str()),
        "{printed}"
    );
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

#[test]
fn a_store_that_will_not_say_what_it_holds_stops_the_export_rather_than_shrinking_it() {
    let host = a_machine_worth_backing_up();
    host.lock_keychain("User interaction is not allowed");

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("a store would not answer");
    assert!(refused.to_string().contains("partial restore"), "{refused}");
    assert_eq!(host.file(AT), None, "and nothing was written");
}

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

/// The refusal above is checked before the prompts and again after them, over a
/// window two questions long — and the write itself replaces rather than fails.
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

#[test]
fn a_machine_holding_no_accounts_is_told_so_rather_than_given_an_empty_file() {
    let host = typing_the_passphrase(machine_with_claude_code());

    let (outcome, _printed) = run_export(&host, AT);

    let refused = outcome.expect_err("there is nothing to export");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert_eq!(host.file(AT), None);
}

/// An Account added by another `perch` while somebody is typing is one the copy
/// being sealed does not hold, so the file presents itself as everything Perch
/// holds while being partial — and that is found out at the restore.
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

/// A backup of nothing and a backup that failed are indistinguishable once an
/// empty file has been written, on the one day it is read.
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
/// definition — and is the shortest form of the command.
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

/// A Renewal Rotates the live Credential and the copy in the Account's own
/// Profile catches up only when a Switch away Captures it
/// (ADR a-switch-is-written-down-first).
#[test]
fn the_active_accounts_credential_is_the_live_one_rather_than_the_copy_in_its_profile() {
    let host = machine_with_three_accounts();
    assert_eq!(
        registry_of(&host).active().whose(),
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

/// A login made outside Perch leaves `.claude.json` naming somebody Perch does
/// not hold, which is the evidence a Capture demands before it copies that
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

/// One of the things people give up on a machine they are decommissioning is
/// Claude Code, and nothing in an Export needs it: the one place a version is
/// asked for is an Identity that is not evidence against when it is absent.
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

/// A registry holding a **Landing** answers "who is active" with the Account
/// being *left*, and the live Credential during one may be either's — so one
/// refresh token would go into the file under two addresses.
#[test]
fn an_export_settles_a_landing_before_it_decides_whose_the_live_credential_is() {
    let host = machine_with_three_accounts();
    // What a Switch leaves when it dies after the Credential moved and before
    // the Identity was patched: the arriving Account's Credential is live, and
    // `.claude.json` still names the one being left.
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_CREDENTIAL);
    let host = typing_the_passphrase(host);

    run_export(&host, AT).0.expect("the export is written");

    let export = opened(&host, AT);
    assert_eq!(
        export.credentials.get(EMAIL).map(String::as_str),
        Some(CREDENTIAL),
        "the Account being left travels as its own Profile holds it, not as \
         whatever the interrupted Switch happened to leave live"
    );
    assert_eq!(
        export.credentials.get(SECOND_EMAIL).map(String::as_str),
        Some(SECOND_CREDENTIAL),
        "and the Account arriving travels as itself, once rather than nowhere"
    );
}

/// The two halves of the previous two tests at once. An Identity naming a
/// stranger is evidence against wherever it is readable, and whether `claude`
/// can say its version has nothing to do with whether it can be read.
#[test]
fn a_live_credential_belonging_to_somebody_else_is_left_out_with_claude_code_gone_too() {
    let host = machine_with_three_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.write_private_file(
        std::path::Path::new(IDENTITY_PATH),
        &SECOND_IDENTITY_FILE.replace(SECOND_EMAIL, "stranger@example.com"),
    )
    .expect("the identity file is written");
    host.remove_file(std::path::Path::new(CLAUDE_BIN))
        .expect("Claude Code is uninstalled");
    let host = typing_the_passphrase(host);

    run_export(&host, AT).0.expect("the export is written");

    assert_eq!(
        opened(&host, AT).credentials.get(EMAIL).map(String::as_str),
        Some(CREDENTIAL),
        "the copy in its own Profile is the honest answer for it"
    );
}

/// The one artifact that makes a Purge survivable, written where the Purge
/// deletes it. The offer inside `perch holdings purge` has refused this since
/// the review that found it; the command that writes the same file at the same
/// path had the guard nowhere, so a backup taken today is gone with the
/// Holdings it is the only copy of.
#[test]
fn an_export_inside_perchs_own_home_is_refused() {
    for typed in [
        "/Users/someone/.config/perch/backup.age",
        // The same directory, reached by a name sharing no component with it.
        "/Users/someone/backups/backup.age",
    ] {
        let host = typing_the_passphrase(machine_with_two_accounts()).with_link(
            perch::host::Link::Symbolic,
            "/Users/someone/.config/perch",
            "/Users/someone/backups",
        );

        let (outcome, printed) = run_export(&host, typed);

        let refused = outcome.expect_err("that is where a Purge deletes it");
        assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
        assert!(
            refused.to_string().contains("perch holdings purge"),
            "and says what would take it: {refused}"
        );
        assert!(
            host.file(typed).is_none(),
            "{typed}: nothing was written: {printed}"
        );
    }
}

/// The bytes land before the command says so, and the path an Export is never
/// written over is refused on a re-run — which reads as somebody else's file.
/// So a report that could not be printed has to say the Export is there.
#[test]
fn a_terminal_that_goes_away_reporting_still_says_the_export_was_written() {
    /// Writes until the report, and then is not there.
    struct GoesAwayReporting;

    impl std::io::Write for GoesAwayReporting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Exported") {
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

    let host = typing_the_passphrase(a_machine_worth_backing_up());

    let outcome =
        perch::commands::export::run(&host, std::path::Path::new(AT), &mut GoesAwayReporting);

    let refused = outcome.expect_err("the report could not be written");
    let said = refused.to_string();
    assert!(said.contains(AT), "the file that is there is named: {said}");
    assert!(
        said.contains("nothing to run again"),
        "and running it again is refused for the path being taken: {said}"
    );
    assert!(host.path_exists(std::path::Path::new(AT)), "which it is");
}
