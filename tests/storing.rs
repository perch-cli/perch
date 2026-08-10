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

/// The file beside it, which is not a Credential and is not nothing either.
///
/// `.claude.json` holds MCP configuration, and an MCP server entry routinely
/// carries an API key in its `env` block — which is the rule `switch` already
/// writes the Default Profile's copy under. A Profile's own copy was created by
/// a plain write, so it arrived at the process umask; and because the rule for a
/// file that already exists is to carry its mode across, it then stayed there
/// for the life of the Profile while a Carry wrote into it on every Run.
#[test]
fn the_identity_file_in_a_profile_is_created_for_its_owner_alone_too() {
    let host = logged_in_machine_off_macos();

    run_status(&host, false).0.expect("the login is adopted");

    let store = store_of(&host, EMAIL);
    assert_eq!(
        host.mode_of(&store.identity_file),
        Some(0o600),
        "a file Perch is the first to create is created closed, not at the umask"
    );
}

#[test]
fn a_credential_file_others_could_read_is_tightened_and_reported_rather_than_refused() {
    let host = logged_in_machine_off_macos().with_file_mode(CREDENTIALS_PATH, 0o644);

    let (result, _) = run_status(&host, false);

    result.expect("a loose file is not a reason to refuse a working machine");
    assert_eq!(host.mode_of(CREDENTIALS_PATH), Some(0o600));
    // The note spells the path the way this platform joins it, so the
    // expectation derives the same spelling rather than writing one by hand.
    let displayed = perch::probe::default_store(&host)
        .expect("the store derives")
        .credentials_file
        .display()
        .to_string();
    let notes = host.notes();
    let tightened: Vec<&String> = notes
        .iter()
        .filter(|note| note.contains(&displayed))
        .collect();
    assert_eq!(tightened.len(), 1, "said once: {notes:?}");
}

/// The reasoning for tightening rather than refusing is that a tightened file
/// is a better outcome than an explained one. When the tightening does not
/// work, the user was getting neither: the remark sat inside the success arm,
/// so a `chmod` that failed produced no error, no note, and a world-readable
/// refresh token read on every command from then on.
///
/// A `.credentials.json` left owned by root by a `sudo claude`, or restored
/// from a backup, is how a machine arrives here.
#[test]
fn a_credential_file_that_cannot_be_tightened_is_still_said_out_loud() {
    let host = logged_in_machine_off_macos()
        .with_file_mode(CREDENTIALS_PATH, 0o644)
        .with_unwritable_file(CREDENTIALS_PATH, "Operation not permitted");

    let (result, _) = run_status(&host, false);

    result.expect("a loose file is not a reason to refuse a working machine");
    let displayed = perch::probe::default_store(&host)
        .expect("the store derives")
        .credentials_file
        .display()
        .to_string();
    let said = host
        .notes()
        .into_iter()
        .find(|note| note.contains(&displayed))
        .unwrap_or_else(|| panic!("nothing said it: {:?}", host.notes()));
    assert!(
        said.contains("could not be narrowed"),
        "and says the tightening did not happen: {said}"
    );
    assert!(
        said.contains("chmod 600"),
        "with the one thing that puts it right: {said}"
    );
    assert_eq!(
        host.mode_of(CREDENTIALS_PATH),
        Some(0o644),
        "the file really is still loose"
    );
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

/// The same removal, failing. A Profile directory that is read-only fails the
/// write *and* the unlink, so the Credential goes to the other store and the
/// one this platform reads first keeps the copy it had — which then beats it on
/// every later read, for ever. Reported as a failure rather than remarked,
/// because the caller's next act is to believe the Capture happened, and a
/// Capture that did not take effect is ADR 0006's silent poisoning by the back
/// door.
#[test]
fn a_superseded_copy_that_survives_in_the_store_read_first_is_a_failure() {
    let host = two_accounts_off_macos_with_a_keychain();
    // Derived rather than spelled out, because the refusal is asserted against
    // the path *as it renders* — and a Windows build joins with the other
    // separator, so a fixture writing the path by hand names something the
    // message never says.
    let live = perch::probe::default_store(&host)
        .expect("home is known")
        .credentials_file;
    // Readable, and neither writable nor removable: the file is still there and
    // still answers, so it still wins.
    let host = host
        .with_unwritable_file(&live, "Read-only file system (os error 30)")
        .with_undeletable_file(&live, "Read-only file system (os error 30)");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let refused = result.expect_err("the Capture did not take effect");
    let said = refused.to_string();
    assert!(
        said.contains(&live.display().to_string()),
        "which store is now lying: {said}"
    );
    assert!(
        said.contains("read first"),
        "and why that is the one that matters: {said}"
    );
    assert_eq!(
        host.file(&live).as_deref(),
        Some(CREDENTIAL),
        "the copy that survived is the one a read would still find, which is \
         exactly why this could not be reported as a success"
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

    // The store the write was aimed at is the Default Profile's. Both halves of
    // it took a value and read it back as something else, and a truncated
    // Credential left where Claude Code looks is worse than none: it parses as
    // nothing, so every retry fails a step earlier than this one did, and the
    // only way back is deleting the item by hand.
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME),
        None,
        "the copy the keychain mangled was taken back out"
    );
    assert_eq!(
        host.file(CREDENTIALS_PATH),
        None,
        "and so was the one the file mangled"
    );
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("did not read back intact")),
        "and the machine says what it removed and why: {:?}",
        host.notes()
    );
}

/// A store that refuses the write outright is a different state, and its
/// Credential is not this function's to throw away.
///
/// Nothing was written, so what it holds is what it held before — the last
/// Credential that worked. Removing that would turn a failed Switch into a
/// Quarantine, which is the one outcome worse than the failure being reported.
#[test]
fn a_store_that_would_not_take_the_write_at_all_keeps_the_credential_it_had() {
    let host = machine_with_two_accounts()
        .with_locked_keychain("the keychain is locked")
        .with_unwritable_file(CREDENTIALS_PATH, "Permission denied (os error 13)");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    result.expect_err("neither store would take it");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the Credential that was already live is untouched"
    );
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}
