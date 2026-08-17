//! Behaviour: keeping an Account out of Cycling without giving it up.
//!
//! Disabling is the smallest thing that can be said about an Account: it leaves
//! the Cycling pool and nothing else changes. It stays listed, keeps its Alias
//! and its Group, keeps its stored Credential, and is still switched to the
//! moment you name it. So most of these tests are about what `perch disable`
//! does *not* touch, and about the exclusion being fully reversible.

mod common;

use common::*;
use perch::error::{EXIT_INVALID, EXIT_NO_CANDIDATE, EXIT_NOT_FOUND, EXIT_NOTHING_TO_DO};
use perch::host::FakeHost;

/// Two Accounts declared interchangeable, the first one active — the smallest
/// machine on which leaving the Cycling pool means anything.
fn two_accounts_in_one_group() -> FakeHost {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    host
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active().whose().map(str::to_string)
}

fn is_disabled(host: &FakeHost, email: &str) -> bool {
    registry_of(host)
        .account(email)
        .expect("an Account Perch holds")
        .disabled
}

#[test]
fn a_disabled_account_is_excluded_from_cycling() {
    let host = two_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);

    let (result, printed) = disable_account(&host, SECOND_EMAIL);

    result.expect("an Account can be taken out of Cycling");
    assert!(is_disabled(&host, SECOND_EMAIL), "{printed}");

    let (cycled, _) = run_cycle(&host);

    assert_eq!(
        cycled
            .expect_err("the Account with all the room is out of the pool")
            .exit_code(),
        EXIT_NOTHING_TO_DO
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "a Cycle lands nowhere rather than on an Account that was kept back for \
         something else"
    );
}

#[test]
fn a_disabled_account_stays_listed_and_is_shown_as_disabled() {
    let host = machine_with_two_accounts();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");

    let (result, printed) = run_list(&host, false);

    result.expect("the listing still renders");
    let row = printed
        .lines()
        .find(|line| line.contains(SECOND_EMAIL))
        .unwrap_or_else(|| panic!("a disabled Account is not a vanished one:\n{printed}"));
    assert!(row.contains("disabled"), "{printed}");

    let (_, document) = run_list(&host, true);
    assert!(
        document.contains("\"disabled\": true"),
        "and a script reads the same fact:\n{document}"
    );
}

#[test]
fn a_disabled_account_can_still_be_switched_to_by_name() {
    let host = machine_with_two_accounts();
    set_alias(&host, "spare", SECOND_EMAIL).0.expect("named");
    disable_account(&host, "spare")
        .0
        .expect("taken out of Cycling");

    let (result, printed) = run_switch(&host, "spare");

    result.expect("naming an Account is not Cycling to it");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{printed}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and it is a Switch like any other: {printed}"
    );
    assert!(
        is_disabled(&host, SECOND_EMAIL),
        "switching to it is not a way of putting it back in the pool"
    );
}

#[test]
fn enabling_returns_an_account_to_the_cycling_pool() {
    let host = two_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");

    let (result, printed) = enable_account(&host, SECOND_EMAIL);

    result.expect("the exclusion is fully reversible");
    assert!(!is_disabled(&host, SECOND_EMAIL), "{printed}");

    let (cycled, cycling) = run_cycle(&host);

    cycled.expect("there is somewhere to go again");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{cycling}");
}

#[test]
fn disabling_the_last_candidate_in_a_group_is_allowed_and_leaves_no_candidate() {
    let host = two_accounts_in_one_group();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");

    let (result, printed) = disable_account(&host, EMAIL);

    result.expect(
        "emptying a Group of candidates is a thing somebody may mean, not \
         a state Perch refuses to be in",
    );
    assert!(is_disabled(&host, EMAIL), "{printed}");

    let (cycled, _) = run_cycle(&host);

    let error = cycled.expect_err("nobody in the Group is a candidate");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    assert!(
        error.to_string().contains("2 disabled"),
        "the answer says why there was nobody rather than just that there was \
         nobody: {error}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL), "and nothing moved");
}

#[test]
fn saying_it_twice_is_not_an_error_and_says_it_was_already_so() {
    let host = machine_with_two_accounts();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");

    let (again, printed) = disable_account(&host, SECOND_EMAIL);

    again.expect("the second one asks for a state the Account is already in");
    assert!(printed.contains("already"), "{printed}");
    assert!(is_disabled(&host, SECOND_EMAIL));

    enable_account(&host, SECOND_EMAIL).0.expect("returned");
    let (once_more, printed) = enable_account(&host, SECOND_EMAIL);

    once_more.expect("and the same in the other direction");
    assert!(printed.contains("already"), "{printed}");
    assert!(!is_disabled(&host, SECOND_EMAIL));
}

#[test]
fn keeping_an_account_out_of_cycling_leaves_everything_else_about_it_alone() {
    let host = two_accounts_in_one_group();
    set_alias(&host, "spare", SECOND_EMAIL).0.expect("named");
    let credential = credential_of(&host, SECOND_EMAIL);

    disable_account(&host, "spare")
        .0
        .expect("taken out of Cycling");

    let registry = registry_of(&host);
    let account = registry.account(SECOND_EMAIL).expect("still held");
    assert_eq!(
        account.group.as_deref(),
        Some("work"),
        "leaving the Cycling pool is not leaving the Group"
    );
    assert_eq!(registry.alias_of(SECOND_EMAIL), Some("spare"));
    assert_eq!(
        credential_of(&host, SECOND_EMAIL),
        credential,
        "its Credential is untouched, so putting it back needs no login"
    );
    assert!(host.http_calls().is_empty());
}

#[test]
fn the_account_you_are_on_can_be_kept_out_of_cycling_without_switching_away_from_it() {
    let host = two_accounts_in_one_group();

    let (result, printed) = disable_account(&host, EMAIL);

    result.expect("which Account is active and which Cycling may pick are separate facts");
    assert_eq!(active(&host).as_deref(), Some(EMAIL), "{printed}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "no Credential is rewritten to record a decision about Cycling"
    );
    assert!(is_disabled(&host, EMAIL));
}

#[test]
fn enabling_a_quarantined_account_does_not_claim_to_have_repaired_it() {
    let host = machine_with_two_accounts();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");
    quarantine(&host, SECOND_EMAIL);

    let (result, printed) = enable_account(&host, SECOND_EMAIL);

    result.expect("the two states are separate facts with separate fixes");
    assert!(
        printed.contains("Quarantined"),
        "an Account Cycling still cannot use should not be reported as one it \
         can: {printed}"
    );
    assert!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .expect("still held")
            .quarantined(),
        "and enabling it did not quietly clear it"
    );
}

#[test]
fn disabling_a_quarantined_account_promises_no_switch_that_would_not_work() {
    let host = machine_with_two_accounts();
    quarantine(&host, SECOND_EMAIL);

    let (result, printed) = disable_account(&host, SECOND_EMAIL);

    result.expect("an Account can leave the pool whatever its Credential is doing");
    assert!(
        !printed.contains("still switches to it"),
        "the promise disabling makes about naming an Account is exactly the one \
         Quarantine breaks, so it is not made here: {printed}"
    );
    assert!(printed.contains("Quarantined"), "{printed}");
}

#[test]
fn the_target_says_which_kind_of_name_matched_before_it_acts() {
    let host = machine_with_two_accounts();
    set_alias(&host, "spare", SECOND_EMAIL).0.expect("named");

    let (result, printed) = disable_account(&host, "spare");

    result.expect("an Alias reaches the Account it names");
    assert!(
        printed.contains("`spare` is an Alias for") && printed.contains(SECOND_EMAIL),
        "{printed}"
    );
}

#[test]
fn a_group_named_where_one_account_is_meant_is_refused_as_a_group() {
    let host = two_accounts_in_one_group();

    let (result, _) = disable_account(&host, "work");

    let error = result.expect_err("this acts on one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(
        error.to_string().contains("Group"),
        "the kind that did match is said, so the refusal reads as a \
         misunderstanding rather than a missing Account: {error}"
    );
    assert!(
        registry_of(&host)
            .accounts
            .iter()
            .all(|account| !account.disabled),
        "and nothing was disabled wholesale"
    );
}

#[test]
fn a_target_that_names_nothing_is_refused_with_what_it_nearly_matched() {
    let host = machine_with_two_accounts();

    let (result, _) = disable_account(&host, "overflow");

    let error = result.expect_err("nothing Perch holds is called that yet");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(error.to_string().contains(SECOND_EMAIL), "{error}");
}
