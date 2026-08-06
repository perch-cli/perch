//! Behaviour: `perch relogin` — the way back from a Quarantine.
//!
//! The Account that comes back has to be the *same* Account: same Alias, same
//! Group, same standing with Cycling, same place in the listing. Removing and
//! re-adding would produce something that looks similar and is not, and would
//! hand the user the job of putting back settings they never changed. So most
//! of these tests are about what a repair leaves alone.
//!
//! The other half is the promise that repairing one Account never costs the
//! session being worked in. The login happens somewhere the active Account
//! cannot be reached from, and the one time the Default Profile is written is
//! when the Account being repaired is the one you are on — where leaving it
//! alone would mean repairing an Account that goes on being broken everywhere
//! it is used.

mod common;

use common::*;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};
use perch::registry::Quarantine;

/// What a repairing login produces for the second Account: the same person,
/// with a Credential that works.
const SECOND_REPAIRED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-second-repaired","refreshToken":"sk-ant-ort01-second-repaired","expiresAt":1785848400000,"scopes":["user:inference","user:profile"],"subscriptionType":"max"}}"#;

/// The same for the Account adoption started everybody on.
const REPAIRED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-repaired","refreshToken":"sk-ant-ort01-repaired","expiresAt":1785848400000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#;

/// Two Accounts, the second Quarantined, named and in a Group and disabled —
/// every setting a repair has to leave where it found it.
fn broken_second_account() -> FakeHost {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("joined");
    }
    set_alias(&host, "overflow", SECOND_EMAIL).0.expect("named");
    quarantine(&host, SECOND_EMAIL);
    host.with_login(login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE))
}

/// A Claude Code running against a config directory: the marker file it writes
/// for the session, and a process that has been there since before it.
fn client_running_against(host: FakeHost, config_dir: &str, pid: u32) -> FakeHost {
    let marker = format!(
        r#"{{"pid":{pid},"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    host.with_file(format!("{config_dir}/sessions/{pid}.json"), &marker)
        .with_live_process(pid)
}

fn is_enabled(host: &FakeHost, email: &str) -> bool {
    registry_of(host)
        .account(email)
        .expect("an Account Perch holds")
        .enabled
}

#[test]
fn a_repair_replaces_the_credential_and_clears_the_quarantine() {
    let host = broken_second_account();

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("a login is what a Quarantine is waiting for");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        None,
        "the Account is not broken any more, so nothing goes on saying it is: {printed}"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_REPAIRED),
        "and the Credential in its Profile is the one the login just produced"
    );
}

#[test]
fn a_repair_keeps_the_alias_the_group_the_cycling_state_and_the_place() {
    let host = broken_second_account();
    disable_account(&host, "overflow").0.expect("reserved");

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    let registry = registry_of(&host);
    let account = registry.account(SECOND_EMAIL).expect("still held");
    assert_eq!(
        registry.alias_of(SECOND_EMAIL),
        Some("overflow"),
        "{printed}"
    );
    assert_eq!(account.group.as_deref(), Some("work"));
    assert!(
        !is_enabled(&host, SECOND_EMAIL),
        "a login says nothing about whether Cycling may choose an Account, so it \
         does not quietly put one back in the pool"
    );
    assert_eq!(
        registry
            .accounts
            .iter()
            .map(|account| account.email().to_string())
            .collect::<Vec<_>>(),
        vec![EMAIL.to_string(), SECOND_EMAIL.to_string()],
        "and it is repaired where it stood rather than rebuilt at the end"
    );
}

#[test]
fn a_repaired_account_is_a_cycle_candidate_again() {
    let host = broken_second_account();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);

    run_cycle(&host)
        .0
        .expect_err("while it is Quarantined there is nowhere to go");

    run_relogin(&host, "overflow").0.expect("repaired");
    let (cycled, printed) = run_cycle(&host);

    cycled.expect("the Account with all the room works again");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

#[test]
fn the_account_you_are_working_in_is_untouched_by_repairing_another() {
    let host = broken_second_account();
    let before = host.file(IDENTITY_PATH).expect("Claude Code's own file");

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the live Credential is the one it was, so the session being worked in \
         does not notice: {printed}"
    );
    assert_eq!(host.file(IDENTITY_PATH).as_deref(), Some(before.as_str()));
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and its own Profile is untouched too"
    );
}

#[test]
fn a_login_that_was_abandoned_changes_nothing_at_all() {
    let host = broken_second_account().with_login(abandoned_login());

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("there is no Credential to repair it with");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(error.to_string().contains("Nothing changed"), "{error}");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected),
        "an Account that could not be repaired is still Quarantined, and for the \
         reason it always was"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the Credential it had is still the Credential it has: a failed repair \
         does not leave the Account emptier than it found it"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "and the Account being worked in is untouched on the failure path too"
    );
}

#[test]
fn a_login_as_a_different_account_is_refused_and_takes_nothing_over() {
    let host =
        broken_second_account().with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("that login was somebody else");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains(THIRD_EMAIL) && error.to_string().contains("perch add"),
        "the refusal names who logged in and what to do about them: {error}"
    );

    let registry = registry_of(&host);
    assert_eq!(
        registry.alias_of(SECOND_EMAIL),
        Some("overflow"),
        "an Alias the user chose for one Account is not handed to another because \
         a browser was signed into somebody else"
    );
    assert!(registry.account(THIRD_EMAIL).is_none());
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL)
    );
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected)
    );
}

#[test]
fn repairing_the_account_you_are_on_makes_its_fresh_credential_the_live_one() {
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);

    let (result, printed) = run_relogin(&host, EMAIL);

    result.expect("the Account you are on can be repaired");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(REPAIRED),
        "a repair only its own Profile can see would leave the Account broken \
         everywhere it is actually used: {printed}"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(REPAIRED));
    assert_eq!(quarantine_of(&host, EMAIL), None);
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "and it is still the active Account: this was a repair, not a Switch"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "no other Account's Profile is written on the way"
    );
}

#[test]
fn a_healthy_account_may_be_logged_in_again() {
    let host = machine_with_two_accounts()
        .with_login(login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE));

    let (result, printed) = run_relogin(&host, SECOND_EMAIL);

    result.expect(
        "a Credential somebody suspects is going wrong should not have to break \
         first before it can be replaced",
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_REPAIRED)
    );
    assert_eq!(quarantine_of(&host, SECOND_EMAIL), None);
    assert!(
        !printed.contains("was Quarantined"),
        "and it does not claim to have repaired something that was not broken: {printed}"
    );
}

#[test]
fn a_profile_a_client_is_running_against_is_refused_before_a_login_is_spent() {
    let host = broken_second_account();
    let profile = store_of(&host, SECOND_EMAIL).config_dir;
    let marker = format!(
        r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    let host = host
        .with_file(profile.join("sessions/4242.json"), &marker)
        .with_live_process(4242);
    host.forget_effects();

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "a Profile Perch may not write to is one no browser round trip was going \
         to repair: {:?}",
        host.effects()
    );
}

#[test]
fn repairing_the_account_you_are_on_is_refused_while_a_client_holds_the_default_profile() {
    // The Default Profile is the one a repair of the active Account writes, and
    // it is the Credential a running session is holding.
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let host = client_running_against(host, "/Users/someone/.claude", 4242);
    host.forget_effects();

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "and no login was spent on a repair that could not land"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the session goes on holding exactly what it was holding"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::RenewalRejected)
    );
}

#[test]
fn a_repair_that_could_not_be_made_live_still_stands_and_says_what_is_left() {
    // Off macOS, so the Default Profile's Credential is the file below and the
    // login's own directory is somewhere else entirely: the write that fails is
    // the last one, not the login.
    let host = logged_in_machine_off_macos().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    run_list(&host, false)
        .0
        .expect("the first command adopts the login");
    quarantine(&host, EMAIL);

    // Neither store of the Default Profile will take it, so the repair lands in
    // the Account's own Profile and goes no further.
    let host = host.with_unwritable_file(CREDENTIALS_PATH, "no space left on device");
    host.lock_keychain("could not run /usr/bin/security: No such file or directory");

    let (result, printed) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the live Credential could not be replaced");
    assert!(
        error.to_string().contains("The repair itself stands"),
        "a partial outcome says which half happened: {error}"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "the Account has a working Credential in its own Profile, which is the \
         whole of what the Quarantine said it did not have: {printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "and that Credential is the repaired one"
    );
}

#[test]
fn a_group_named_where_one_account_is_meant_is_refused_as_a_group() {
    let host = broken_second_account();

    let (result, _) = run_relogin(&host, "work");

    let error = result.expect_err("this acts on one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(error.to_string().contains("Group"), "{error}");
}

/// Repairing the Account you are on lands the fresh Credential in the Default
/// Profile last, and that step can fail on its own. What is left is a fresh
/// Credential in the Account's own Profile and the broken one it replaced still
/// live — so `active` must stop naming that Account: the very next `perch
/// switch`, which is at least as natural a thing to reach for as running the
/// repair again, would Capture the broken copy over the fresh one and undo the
/// whole browser round trip (ADR 0006).
#[test]
fn a_repair_that_could_not_be_made_live_leaves_nothing_to_capture_into() {
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let host = host.with_unwritable_file(IDENTITY_PATH, "read-only file");

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the repair could not be made live");
    assert!(
        error.to_string().contains("no active Account"),
        "it says what it did about it: {error}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "the repair itself stands"
    );
    assert_eq!(
        registry_of(&host).active,
        None,
        "and Perch is on nobody, so nothing can Capture over it"
    );

    // The command a user would reach for next, which used to be the one that
    // destroyed the repair.
    host.writable_again(IDENTITY_PATH);
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("a Switch still works");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "the fresh Credential survives the Switch that follows"
    );
}
