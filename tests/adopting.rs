//! Behavior: what happens the first time Perch runs on a machine.
//!
//! These drive the real command code against the fake Host and assert only on
//! things a user could observe — what was printed, how the command ended, and
//! what is in the keychain afterwards.

mod common;

use common::*;
use perch::error::{EXIT_KEYCHAIN_UNAVAILABLE, EXIT_NOT_FOUND, EXIT_PROBE_REFUSED};
use perch::host::prelude::*;
use perch::host::{Execution, FakeHost, Refusing};
use perch::registry;

#[test]
fn the_existing_login_is_adopted_as_the_first_profile() {
    let host = logged_in_machine();

    let (result, printed) = run_status(&host, false);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains(EMAIL),
        "adoption should report which Account it found:\n{printed}"
    );
    assert!(
        printed.contains("Acme"),
        "adoption should describe the Account it found:\n{printed}"
    );
}

#[test]
fn the_adopted_account_is_recorded_as_active() {
    let host = logged_in_machine();

    let (result, _) = run_status(&host, false);
    assert!(result.is_ok());

    let registry = registry::load(&host)
        .unwrap()
        .expect("a registry is written");
    assert_eq!(registry.active().whose(), Some(EMAIL));
    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(registry.accounts[0].plan.as_deref(), Some("pro"));
    assert_eq!(
        registry.accounts[0].identity.organization_name.as_deref(),
        Some("Acme")
    );
}

#[test]
fn the_credential_is_copied_into_the_profiles_own_namespace() {
    let host = logged_in_machine();

    run_status(&host, false).0.unwrap();

    let store = store_of(&host, EMAIL);

    assert_ne!(
        store.keychain_service, DEFAULT_SERVICE,
        "a Profile must have a namespace of its own"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the live Credential is left where Claude Code expects it"
    );
}

#[test]
fn the_profile_keeps_the_block_claude_code_wrote_for_the_adopted_account() {
    let host = logged_in_machine();

    run_status(&host, false).0.unwrap();

    let kept = host
        .file(store_of(&host, EMAIL).identity_file)
        .expect("the Profile holds how Claude Code describes this Account");
    let block = perch::probe::oauth_account_block(&kept).expect("a block");

    assert!(
        block.contains(&format!("\"emailAddress\": \"{EMAIL}\"")),
        "{block}"
    );
    assert!(
        block.contains("\"organizationRole\": \"admin\""),
        "verbatim, including the fields Perch does not record itself — \
         otherwise the Account everybody starts with comes back from a Switch \
         described by four fields: {block}"
    );
    assert!(
        !kept.contains("numStartups"),
        "and nothing else from the live file, which belongs to the person \
         rather than to the Account: {kept}"
    );
}

#[test]
fn a_machine_with_no_login_is_told_to_log_in_and_gets_no_profile() {
    let host = machine_with_claude_code();

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("there is nothing to adopt");
    let message = error.to_string();
    assert!(
        message.contains("log in"),
        "the user should be told what to do:\n{message}"
    );
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        registry::load(&host).unwrap().is_none(),
        "no registry should be written"
    );
    assert!(
        host.keychain_services().is_empty(),
        "no empty Profile should be created"
    );
}

#[test]
fn a_locked_keychain_is_reported_differently_from_a_missing_account() {
    let host = logged_in_machine().with_locked_keychain("User interaction is not allowed");

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("nothing can be read while the keychain is locked");
    assert_eq!(error.exit_code(), EXIT_KEYCHAIN_UNAVAILABLE);
    assert_ne!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error
            .to_string()
            .contains("User interaction is not allowed"),
        "the reason should survive: {error}"
    );
}

#[test]
fn an_unrecognized_credential_store_is_refused_and_names_the_assumption() {
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, r#"{"somethingElse":{}}"#)
        .with_file("/Users/someone/.claude.json", IDENTITY_FILE);

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("Perch should not act on a store it does not understand");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    let message = error.to_string();
    assert!(
        message.contains("claudeAiOauth"),
        "the failed assumption should be named:\n{message}"
    );
    assert!(
        message.contains(CLAUDE_VERSION),
        "the installed version should be quoted:\n{message}"
    );
}

#[test]
fn a_login_whose_identity_perch_cannot_read_is_refused_rather_than_guessed_at() {
    let host =
        machine_with_claude_code().with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL);

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("Perch cannot name an Account it cannot identify");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(error.to_string().contains("oauthAccount"));
}

#[test]
fn an_identity_file_perch_cannot_parse_is_refused_and_names_the_assumption() {
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file("/Users/someone/.claude.json", "{ this is not JSON");

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("Perch should not act on a file it cannot read the shape of");
    assert_eq!(
        error.exit_code(),
        EXIT_PROBE_REFUSED,
        "a file Claude Code owns, in a shape Perch does not know, is an \
         assumption failing: {error}"
    );
    let message = error.to_string();
    assert!(message.contains("oauthAccount"), "{message}");
    assert!(message.contains(CLAUDE_VERSION), "{message}");
}

#[test]
fn an_identity_file_that_cannot_be_read_is_not_reported_as_a_missing_account() {
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_a_path_refusing(
            "/Users/someone/.claude.json",
            Refusing::Read,
            "Permission denied (os error 13)",
        );

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("a file that cannot be read is not an answer");
    let message = error.to_string();
    assert!(
        message.contains("Permission denied"),
        "the real reason should survive: {message}"
    );
    assert!(
        !message.contains("names no account"),
        "an unreadable file is not the same as one naming nobody: {message}"
    );
}

#[test]
fn claude_code_being_absent_is_refused_rather_than_assumed_away() {
    let host = FakeHost::new();

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("there is nothing to probe");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(error.to_string().contains("Claude Code is installed"));
}

#[test]
fn a_version_perch_cannot_read_is_refused() {
    let host = FakeHost::new()
        .with_env("PATH", "/usr/bin")
        .with_file(common::CLAUDE_BIN, "")
        .with_exec(
            common::CLAUDE_BIN,
            &["--version"],
            Execution {
                status: 1,
                stdout: String::new(),
                stderr: "boom".into(),
            },
        );

    let (result, _) = run_status(&host, false);

    assert_eq!(
        result.unwrap_err().exit_code(),
        EXIT_PROBE_REFUSED,
        "a Claude Code that will not say what it is cannot be trusted"
    );
}

#[test]
fn adoption_happens_once() {
    let host = logged_in_machine();

    let (first, first_output) = run_status(&host, false);
    first.unwrap();
    let registry_after_adoption = host.file(REGISTRY_PATH).unwrap();

    let (second, second_output) = run_status(&host, false);
    second.unwrap();

    // On the remarks channel rather than in the command's own output: the two
    // commands that reach adoption first also render JSON on that stream.
    assert!(
        host.notes().iter().any(|note| note.contains("Adopted")),
        "{:?}",
        host.notes()
    );
    assert!(!first_output.contains("Adopted"), "{first_output}");
    assert!(
        !second_output.contains("Adopted"),
        "the second run should say nothing about adoption:\n{second_output}"
    );
    assert_eq!(host.file(REGISTRY_PATH).unwrap(), registry_after_adoption);
}

#[test]
fn adoption_makes_no_network_call() {
    let host = logged_in_machine();

    run_status(&host, false).0.unwrap();

    assert!(host.http_calls().is_empty());
}

#[test]
fn a_login_whose_address_names_no_directory_is_refused_rather_than_adopted() {
    let nameless = IDENTITY_FILE.replace(EMAIL, "@");
    let host = logged_in_machine().with_file(IDENTITY_PATH, &nameless);

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("no Profile can be named after that address");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(
        host.file(REGISTRY_PATH).is_none(),
        "and nothing was written on the way to finding out"
    );
}

/// A first run on a machine whose Registry cannot be written — a full disk, a
/// directory somebody made read-only. Adoption writes a Credential before it
/// writes the Registry that names it, and on macOS that copy is a keychain item
/// outside Perch's home entirely.
#[test]
fn an_adoption_that_could_not_be_recorded_leaves_no_credential_behind() {
    let host = logged_in_machine().with_a_path_refusing(
        REGISTRY_PATH,
        Refusing::Write,
        "no space left on device",
    );
    let dir = perch::holdings::profile_dir_for(&host, EMAIL).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &dir).expect("USER is set");

    let (result, _) = run_status(&host, false);

    result.expect_err("the registry could not be written");
    assert!(
        host.file(REGISTRY_PATH).is_none(),
        "nothing records the Account"
    );
    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "so nothing may be holding its Credential either — least of all a \
         keychain item outside Perch's home"
    );
    assert_eq!(host.file(&store.credentials_file), None);
    assert!(!host.path_exists(&dir), "and no Profile is left over");

    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "whatever Claude Code is logged in as is untouched: adoption failing is \
         not a reason to log somebody out"
    );
}

/// The column writers ask, and a sentence is not a column: an Account's plan
/// comes out of `subscriptionType` in a Credential file, and the very first
/// command anyone runs writes it into a remark on stderr, raw. `perch status`
/// drew the same value stripped, one screen away (ADR nothing-drawn-is-obeyed).
#[test]
fn a_remark_carries_no_more_than_a_column_would() {
    let loud = CREDENTIAL.replace(
        r#""subscriptionType":"pro""#,
        r#""subscriptionType":"pro\u001b[2K\u001b[31mQUOTA GONE""#,
    );
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, &loud)
        .with_file(IDENTITY_PATH, IDENTITY_FILE);

    run_list(&host, false).0.expect("the login is adopted");

    let said = host.notes().join("\n");
    assert!(
        !said.contains('\u{1b}'),
        "nothing in a remark is something a terminal acts on: {said:?}"
    );
    assert!(
        said.contains("pro[2K[31mQUOTA GONE"),
        "and everything in it is drawn: {said:?}"
    );
}

/// An `oauthAccount` block that is there and names nobody: different from a file
/// with no block at all, which is a Claude Code nobody has logged into, and
/// different again from one Perch cannot parse.
#[test]
fn an_identity_block_that_names_no_address_is_refused_and_names_the_assumption() {
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(
            IDENTITY_PATH,
            r#"{"numStartups": 1, "oauthAccount": {"accountUuid": "account-uuid-1"}, "projects": {}}"#,
        );

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("an Account with no address is not one Perch can hold");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    let message = error.to_string();
    assert!(
        message.contains("oauthAccount with no emailAddress"),
        "{message}"
    );
    assert!(
        message.contains(CLAUDE_VERSION),
        "it dates the belief against the Claude Code that broke it: {message}"
    );
    assert!(
        host.file(REGISTRY_PATH).is_none(),
        "and nothing was written on the way to finding out"
    );
}

/// An identity block naming one Account, with `organizationName` as given.
fn machine_whose_organization_is(named: &str) -> FakeHost {
    let host =
        machine_with_claude_code().with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL);
    host.set_file(
        IDENTITY_PATH,
        &format!(
            r#"{{"oauthAccount":{{"accountUuid":"account-uuid-1",
               "emailAddress":"someone@example.com",
               "organizationUuid":"organization-uuid-1",
               "organizationName":"{named}"}}}}"#
        ),
    );
    host
}

/// The boundary refuses an organization name for what a line cannot hold, and
/// `perch status` draws the rest. It is Anthropic's, so a refusal here is one
/// nobody can clear from Perch, and it takes `perch add` with it. What a
/// terminal would obey is drawn without it; what composes with the character
/// beside it is drawn whole.
#[test]
fn an_organization_name_is_refused_for_what_a_line_cannot_hold_and_otherwise_drawn() {
    for (held, drawn) in [
        ("Acme \\u2600\\ufe0f", "Acme \u{2600}\u{FE0F}"),
        ("Acme\\u202e", "Acme"),
        ("Acme\\u200b", "Acme"),
    ] {
        let host = machine_whose_organization_is(held);

        let (result, printed) = run_status(&host, false);

        result.unwrap_or_else(|refused| panic!("`{held}` is a name Anthropic may hold: {refused}"));
        let row = printed
            .lines()
            .find(|line| line.starts_with("Organization"))
            .unwrap_or_else(|| panic!("`perch status` says which organization: {printed}"));
        assert_eq!(
            row.strip_prefix("Organization")
                .expect("the row opens with its label")
                .trim_start(),
            drawn,
            "`{held}` is drawn as its owner spells it, less what a terminal obeys"
        );
    }

    for framing in ["Acme\\u0007", "Acme\\u000a"] {
        let host = machine_whose_organization_is(framing);
        let error = run_status(&host, false)
            .0
            .expect_err("a line cannot hold that");
        assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
        assert!(error.to_string().contains("a control character"), "{error}");
    }
}

/// An address is refused for anything a terminal will not draw as itself.
///
/// `registry::validate` refuses nothing about an address on the stated grounds
/// that this does, so a character let through here is one nothing refuses — and
/// an address is a Target somebody types, so two that draw alike have no answer.
#[test]
fn an_address_is_refused_for_anything_a_terminal_will_not_draw_as_itself() {
    for hidden in ["some\\u200bone@example.com", "some\\ufe00one@example.com"] {
        let host =
            machine_with_claude_code().with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL);
        host.set_file(
            IDENTITY_PATH,
            &format!(
                r#"{{"oauthAccount":{{"accountUuid":"account-uuid-1",
                   "emailAddress":"{hidden}",
                   "organizationUuid":"organization-uuid-1",
                   "organizationName":"Acme"}}}}"#
            ),
        );

        let error = run_status(&host, false)
            .0
            .expect_err("that is not an address a Target could name");
        assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
        assert!(
            error.to_string().contains("the account"),
            "and it says which value: {error}"
        );
    }
}
