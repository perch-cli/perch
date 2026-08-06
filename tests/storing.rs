//! Where a Credential ends up, and where it is looked for (ADR 0020).
//!
//! Perch keeps a Credential exactly where the installed Claude Code would, so
//! these run the real commands on a machine that is a Mac and on one that is
//! not, whichever kind of machine the suite itself is running on.

mod common;

use common::*;
use perch::commands::add::AddArgs;
use perch::error::EXIT_KEYCHAIN_UNAVAILABLE;
use perch::host::{FakeHost, Platform};
use perch::registry;

/// The Credential of an Account that has since Rotated several times: what a
/// copy left behind in the store Perch stopped writing to would be.
const STALE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-retired","refreshToken":"sk-ant-ort01-retired"}}"#;

#[test]
fn off_macos_a_credential_is_kept_in_a_file_and_not_in_a_keychain() {
    let host = logged_in_machine_off_macos();

    run_status(&host, false).0.expect("the login is adopted");

    let store = store_of(&host, EMAIL);
    assert_eq!(
        host.file(&store.credentials_file).as_deref(),
        Some(CREDENTIAL),
        "the Profile's Credential belongs in the store Claude Code reads here"
    );
    assert!(
        host.keychain_services().is_empty(),
        "nothing should have been written to a keychain this machine has not got"
    );
}

#[test]
fn a_file_holding_a_credential_is_created_for_its_owner_alone() {
    let host = logged_in_machine_off_macos();

    run_status(&host, false).0.expect("the login is adopted");

    let store = store_of(&host, EMAIL);
    assert_eq!(host.mode_of(&store.credentials_file), Some(0o600));
    assert_eq!(host.mode_of(&store.config_dir), Some(0o700));
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, perch::host::fake::Effect::MadePrivate(_))),
        "the mode is set at creation, not by a chmod that leaves a window open"
    );
}

#[test]
fn a_credential_file_others_could_read_is_tightened_and_reported_rather_than_refused() {
    let host = logged_in_machine_off_macos().with_file_mode(CREDENTIALS_PATH, 0o644);

    let (result, _) = run_status(&host, false);

    result.expect("a loose file is not a reason to refuse a working machine");
    assert_eq!(host.mode_of(CREDENTIALS_PATH), Some(0o600));
    let notes = host.notes();
    assert_eq!(notes.len(), 1, "said once: {notes:?}");
    // The note spells the path the way this platform joins it, so the
    // expectation derives the same spelling rather than writing one by hand.
    let displayed = perch::probe::default_store(&host)
        .expect("the store derives")
        .credentials_file
        .display()
        .to_string();
    assert!(notes[0].contains(&displayed), "{notes:?}");
}

#[test]
fn a_second_account_off_macos_gets_a_file_of_its_own() {
    let host = logged_in_machine_off_macos()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));

    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");

    let first = store_of(&host, EMAIL);
    let second = store_of(&host, SECOND_EMAIL);
    assert_ne!(
        first.credentials_file, second.credentials_file,
        "two Accounts must not share one store"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL)
    );
}

#[test]
fn a_switch_off_macos_moves_the_credential_between_files() {
    let host = two_accounts_off_macos();

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.unwrap_or_else(|err| panic!("the Switch should land: {err}\n{printed}"));
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the incoming Credential is live where Claude Code reads it"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the outgoing one was Captured into its own Profile"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

/// The bug this ADR exists to fix: a Mac whose keychain will not open, and a
/// Claude Code working perfectly off the file beside it.
#[test]
fn a_locked_keychain_does_not_hide_a_login_that_claude_code_is_using() {
    let host = machine_with_claude_code()
        .with_file(CREDENTIALS_PATH, CREDENTIAL)
        .with_file(IDENTITY_PATH, IDENTITY_FILE)
        .with_locked_keychain("User interaction is not allowed");

    let (result, printed) = run_status(&host, false);

    result.unwrap_or_else(|err| {
        panic!("a machine that is logged in must not read as logged out: {err}")
    });
    assert!(printed.contains(EMAIL), "{printed}");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "the adopted Credential is held wherever it could be written"
    );
}

#[test]
fn a_store_that_will_not_take_a_credential_hands_it_to_the_other_one() {
    let host = machine_with_claude_code()
        .with_file(CREDENTIALS_PATH, CREDENTIAL)
        .with_file(IDENTITY_PATH, IDENTITY_FILE)
        .with_locked_keychain("User interaction is not allowed");

    run_status(&host, false).0.expect("the login is adopted");

    let store = store_of(&host, EMAIL);
    assert_eq!(
        host.file(&store.credentials_file).as_deref(),
        Some(CREDENTIAL),
        "a keychain that refuses the write must not cost the Account its Profile"
    );
    assert!(
        host.notes().iter().any(|note| note.contains("keychain")),
        "the user should be told a Credential went somewhere else: {:?}",
        host.notes()
    );
}

#[test]
fn a_credential_written_to_the_primary_store_leaves_no_copy_in_the_other() {
    let host = machine_with_two_accounts();
    // What a spell of locked keychain would have left in this Profile: a
    // Credential several Rotations behind, in the store Perch is not writing.
    let left_behind = store_of(&host, EMAIL).credentials_file;
    host.set_file(&left_behind, STALE);

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch lands");

    assert_eq!(
        host.file(&left_behind),
        None,
        "a superseded copy would be handed back to Claude Code the next time \
         the keychain would not open"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and what the Capture wrote is what the Profile now holds"
    );
}

/// The same rule the other way round, which matters more: the store that could
/// not be written is the one a reader consults first, so a copy left in it
/// would beat the Credential that was just stored.
///
/// Off macOS with a keychain, which is a machine that does not exist —
/// `/usr/bin/security` is a Mac binary. This test is about the composite
/// reader's rule and not about a platform, so it asks for the one arrangement
/// that lets both stores be reachable at once and says so; the fake refuses a
/// keychain off macOS by default, because a test that got one by accident would
/// pass on a scenario the platform cannot produce.
#[test]
fn a_credential_stored_in_the_second_choice_store_empties_the_first() {
    let host = two_accounts_off_macos_with_a_keychain();
    let live = CREDENTIALS_PATH;
    assert_eq!(host.file(live).as_deref(), Some(CREDENTIAL), "as we start");

    // The file this platform reads first can no longer be written — a full
    // disk, a mode Perch cannot undo — so the Switch has to use the other one.
    let host = host.with_unwritable_file(live, "No space left on device (os error 28)");
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("a Switch is not lost to one store refusing");

    assert_eq!(
        host.file(live),
        None,
        "the outgoing Account's Credential must not be left where Claude Code \
         looks before the store the incoming one went to"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the Account switched to is the one a read would find"
    );
}

#[test]
fn both_stores_being_unreachable_is_reported_as_the_primary_failing() {
    let host = machine_with_two_accounts();
    host.lock_keychain("User interaction is not allowed");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("there is nowhere to read a Credential from");
    assert_eq!(
        error.exit_code(),
        EXIT_KEYCHAIN_UNAVAILABLE,
        "the store this machine was supposed to be using is the one to name: {error}"
    );
}

#[test]
fn a_version_1_registry_is_read_by_dropping_where_it_said_a_credential_was() {
    let host =
        logged_in_machine().with_file(REGISTRY_PATH, REGISTRY_FROM_BEFORE_STORES_WERE_DERIVED);
    // The Credential is in the store the derivation names, which is not where
    // the old file says it is.
    let store = store_of(&host, EMAIL);
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, CREDENTIAL);

    set_alias(&host, "work", EMAIL).0.expect("a command runs");

    let written = host
        .file(REGISTRY_PATH)
        .expect("the registry was rewritten");
    assert!(
        written.contains(&format!("\"version\": {}", registry::CURRENT_VERSION)),
        "{written}"
    );
    for derived in ["keychain_service", "keychain_account", "\"profile\""] {
        assert!(!written.contains(derived), "{derived} survived:\n{written}");
    }
    assert_eq!(
        registry_of(&host)
            .account(EMAIL)
            .unwrap()
            .profile_dir(&host)
            .unwrap(),
        registry::profile_dir_for(&host, EMAIL).unwrap(),
        "the Profile is derived from the Account, not read back from the file"
    );
}

/// Two Accounts on a machine that is not a Mac.
/// The same, on the machine that does not exist: not a Mac, and with a
/// keychain that answers anyway. Only for the tests that are about the
/// composite reader rather than about a platform.
fn two_accounts_off_macos_with_a_keychain() -> FakeHost {
    let host = logged_in_machine_off_macos()
        .with_keychain_off_macos()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");
    host
}

fn two_accounts_off_macos() -> FakeHost {
    let host = logged_in_machine_off_macos()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");
    host
}

/// A registry from the Perch that recorded each Account's keychain namespace
/// and Profile directory, with values that were never right on this machine —
/// so a build that still read them would find nothing.
const REGISTRY_FROM_BEFORE_STORES_WERE_DERIVED: &str = r#"{
  "version": 1,
  "active": "someone@example.com",
  "accounts": [
    {
      "identity": {
        "email": "someone@example.com",
        "account_uuid": "account-uuid-1",
        "organization_name": "Acme",
        "organization_uuid": null
      },
      "plan": "pro",
      "profile": {
        "dir": "/Users/someone/.perch/profiles/somewhere-else",
        "keychain_service": "Claude Code-credentials-deadbeef",
        "keychain_account": "someone"
      },
      "enabled": true,
      "quarantined": false
    }
  ],
  "aliases": {}
}"#;

#[test]
fn the_platform_decides_which_store_is_written_first() {
    for (platform, holds) in [(Platform::MacOs, false), (Platform::Other, true)] {
        let host = machine_with_claude_code()
            .with_platform(platform)
            .with_file(IDENTITY_PATH, IDENTITY_FILE);
        let default = perch::probe::default_store(&host).expect("USER is set");
        let [primary, _] = perch::credentials::stores_for(&host, &default);
        primary.write(&host, CREDENTIAL).expect("logged in");

        run_status(&host, false).0.expect("the login is adopted");

        let store = store_of(&host, EMAIL);
        assert_eq!(
            host.file(&store.credentials_file).is_some(),
            holds,
            "on {platform:?} a Credential belongs in the store Claude Code writes to"
        );
        assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    }
}

/// The read-back guard, which is the whole reason a Credential is not simply
/// written and believed.
///
/// `security -i` truncates mid-argument when a command line overruns its 4096
/// byte stdin buffer, and says nothing about it (ADR 0008). A truncated
/// Credential is indistinguishable from a wrong one at the worst possible
/// moment — some Switch later, with the good copy already replaced. So what a
/// store says it kept is read back, and a store that kept something else is
/// treated exactly like one that refused the write outright.
#[test]
fn a_store_that_kept_less_than_it_was_given_is_treated_as_one_that_refused() {
    let host = machine_with_two_accounts().with_keychain_truncating_after(40);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the other store takes what this one would not keep");
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the Credential is whole, in the store that could hold it whole: \n{printed}"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME),
        None,
        "and the truncated copy is gone from the store a reader consults \
         first, where it would have won"
    );
    assert!(
        host.notes().iter().any(|note| note.contains("instead")),
        "{:?}",
        host.notes()
    );
}

/// And where neither store keeps what it was given, the Switch stops at the
/// write rather than recording a landing that did not happen.
#[test]
fn a_switch_neither_store_would_keep_intact_stops_at_the_write() {
    let host = machine_with_two_accounts()
        .with_keychain_truncating_after(40)
        .with_file_corrupting_writes(CREDENTIALS_PATH);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("what was stored is not what was written");
    assert_eq!(error.exit_code(), EXIT_KEYCHAIN_UNAVAILABLE);
    assert!(
        error.to_string().contains("did not read back intact"),
        "{error}"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "the Account being left is still the active one"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the Capture that ran before the write still stands"
    );
}
