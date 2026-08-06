//! Behaviour: forgetting an Account when a subscription is retired.
//!
//! Removal is the one command that destroys something, so most of these tests
//! are about what it refuses to destroy silently. Removing an Account nobody is
//! on is unremarkable — the entry goes, the Credential goes, the Alias comes
//! free. Removing the **active** Account is the case that needs care: Perch
//! names the Account it will leave active, lands on it before anything is
//! deleted, and asks first — so nobody ends up running as an Account Perch has
//! forgotten.

mod common;

use common::*;
use perch::commands::remove::RemoveArgs;
use perch::error::{EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
use perch::host::{FakeHost, Host};
use perch::probe::Identity;
use perch::registry::Account;

const FIRST_PROFILE: &str = "/Users/someone/.config/.perch/profiles/someone-example-com";
const SECOND_PROFILE: &str = "/Users/someone/.config/.perch/profiles/overflow-example-com";

/// The config directory every client reads — where a Switch, and the landing a
/// removal makes, has to write.
const DEFAULT_PROFILE: &str = "/Users/someone/.claude";

/// What the live store holds right now — the Credential every client reads.
fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

/// A Claude Code running against a Profile: the marker file it writes for the
/// session, and a process that has been there since before it (ADR 0022).
fn client_running_against(host: FakeHost, profile_dir: &str, pid: u32) -> FakeHost {
    let marker = format!(
        r#"{{"pid":{pid},"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    host.with_file(format!("{profile_dir}/sessions/{pid}.json"), &marker)
        .with_live_process(pid)
}

fn holds(host: &FakeHost, email: &str) -> bool {
    registry_of(host).account(email).is_some()
}

#[test]
fn removing_an_account_forgets_it_and_deletes_the_credential_perch_held() {
    let host = machine_with_two_accounts();
    assert!(
        credential_of(&host, SECOND_EMAIL).is_some(),
        "held to begin with"
    );

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("an Account nobody is on is removed without ceremony");
    assert!(!holds(&host, SECOND_EMAIL), "{printed}");
    assert_eq!(
        credential_of(&host, SECOND_EMAIL),
        None,
        "a retired subscription leaves no Credential behind: {printed}"
    );
    assert!(
        !host.path_exists(std::path::Path::new(SECOND_PROFILE)),
        "and no Profile either"
    );

    let (_, listed) = run_list(&host, false);
    assert!(
        !listed.contains(SECOND_EMAIL),
        "a removed Account stops appearing in the listing:\n{listed}"
    );
}

#[test]
fn the_alias_a_removed_account_answered_to_is_free_to_use_again() {
    let host = machine_with_two_accounts();
    set_alias(&host, "spare", SECOND_EMAIL).0.expect("named");

    let (result, printed) = run_remove(&host, "spare");

    result.expect("an Alias reaches the Account it names");
    assert!(printed.contains("`spare` is an Alias for"), "{printed}");
    assert_eq!(registry_of(&host).declared_alias("spare"), None);

    set_alias(&host, "spare", EMAIL)
        .0
        .expect("the name is free for another Account");
    assert_eq!(registry_of(&host).alias_of(EMAIL), Some("spare"));
}

#[test]
fn a_removed_account_is_no_longer_a_cycle_candidate() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 50.0)]);

    run_remove(&host, SECOND_EMAIL)
        .0
        .expect("the roomiest subscription is the one being retired");

    let (cycled, printed) = run_cycle(&host);

    cycled.expect("there is still somewhere to go");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(THIRD_EMAIL),
        "a Cycle cannot land on an Account Perch has forgotten: {printed}"
    );
}

#[test]
fn removing_an_account_leaves_every_other_account_and_the_live_credential_alone() {
    let host = three_accounts_in_one_group();
    let kept = credential_of(&host, THIRD_EMAIL);

    run_remove(&host, SECOND_EMAIL).0.expect("removed");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "removing an Account nobody is on switches nothing"
    );
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
    assert_eq!(credential_of(&host, THIRD_EMAIL), kept);
    assert!(
        host.http_calls().is_empty(),
        "and nothing was asked of Anthropic"
    );
}

#[test]
fn the_group_a_removed_account_was_in_stays_declared() {
    let host = machine_with_two_accounts();
    declare_group(&host, "retiring");
    move_to_group(&host, SECOND_EMAIL, "retiring")
        .0
        .expect("the Account joins the Group");

    run_remove(&host, SECOND_EMAIL).0.expect("removed");

    assert!(
        registry_of(&host).declared_group("retiring").is_some(),
        "a Group is something the user declared, not a summary of where the \
         Accounts happen to be"
    );
}

#[test]
fn removing_the_active_account_names_what_will_be_active_and_asks_first() {
    let host = machine_with_two_accounts().with_answers(&["y"]);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the removal was agreed to");
    assert!(
        printed.contains(SECOND_EMAIL),
        "the Account that will be left active is named before the question, not \
         discovered afterwards:\n{printed}"
    );
    assert!(!holds(&host, EMAIL), "{printed}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and Perch landed there rather than leaving the machine running as an \
         Account it no longer holds"
    );
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
}

/// The window between the two halves of removing the active Account: the
/// successor's Credential is live, and the Account being given up has not been
/// destroyed yet. A failure in there must leave the record agreeing with the
/// machine — because `active` is what the next Switch Captures *into*, and one
/// naming the Account whose Credential is no longer live would copy the
/// successor's over that Account's own good copy and destroy it (ADR 0006).
#[test]
fn a_removal_that_fails_after_landing_still_records_who_is_live() {
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        .with_undeletable_file(format!("{FIRST_PROFILE}/.credentials.json"), "read-only");

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect_err("the Credential could not be given up");
    assert!(holds(&host, EMAIL), "nothing was forgotten: {printed}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the landing happened, so the successor's Credential is the live one"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "and the record says so, rather than going on naming the Account whose \
         Credential a Switch would now overwrite"
    );
}

#[test]
fn declining_removes_nothing_at_all() {
    let host = machine_with_two_accounts().with_answers(&["n"]);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("answering no is an answer, not a failure");
    assert!(holds(&host, EMAIL), "{printed}");
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

#[test]
fn the_account_left_active_is_one_the_user_declared_interchangeable() {
    let host = machine_with_three_accounts().with_answers(&["y"]);
    declare_group(&host, "work");
    for email in [EMAIL, THIRD_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("removed");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(THIRD_EMAIL),
        "the Group is the one landing place the user endorsed in advance, so it \
         is preferred over the Account that merely comes first: {printed}"
    );
}

#[test]
fn a_disabled_account_is_not_what_perch_lands_on() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    disable_account(&host, SECOND_EMAIL).0.expect("reserved");

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the removal was agreed to");
    assert!(
        printed.contains("no active Account"),
        "an Account reserved for something else is never chosen for you, so \
         there is nowhere to land and the question says so:\n{printed}"
    );
    assert_eq!(registry_of(&host).active, None);
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "and Perch does not log anybody out: the live Credential is not its to \
         take away"
    );
}

#[test]
fn removing_the_only_account_is_confirmed_rather_than_leaving_nothing_active_silently() {
    let host = logged_in_machine().with_answers(&["y"]);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("holding no Accounts is a thing somebody may mean");
    assert!(
        printed.contains("no active Account") || printed.contains("no Accounts"),
        "{printed}"
    );
    let registry = registry_of(&host);
    assert!(registry.accounts.is_empty(), "{printed}");
    assert_eq!(registry.active, None);
    assert_eq!(credential_of(&host, EMAIL), None);
}

#[test]
fn the_only_account_is_not_removed_by_a_question_nobody_answered() {
    // End of input rather than a "no": a pipe that closed, and the removal is
    // the one thing that must not be read as agreement.
    let host = logged_in_machine();

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("nothing was asked of the machine that it refused");
    assert!(holds(&host, EMAIL), "{printed}");
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
}

#[test]
fn without_a_terminal_the_active_account_goes_only_when_asked_for_outright() {
    let host = machine_with_two_accounts().without_terminal();

    let (refused, _) = run_remove(&host, EMAIL);

    refused.expect_err("there is nobody to confirm with");
    assert!(holds(&host, EMAIL));

    let (result, printed) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("every capability is available non-interactively (ADR 0011)");
    assert!(!holds(&host, EMAIL), "{printed}");
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
}

#[test]
fn removal_is_refused_while_the_accounts_profile_is_live() {
    let host = client_running_against(machine_with_two_accounts(), SECOND_PROFILE, 4242);

    let (result, _) = run_remove(&host, SECOND_EMAIL);

    let error = result.expect_err("something else is holding that Credential");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(holds(&host, SECOND_EMAIL), "and nothing was removed");
    assert!(credential_of(&host, SECOND_EMAIL).is_some());
}

#[test]
fn removing_the_active_account_is_refused_while_a_client_is_running_against_the_default_profile() {
    // The Account's own Profile is quiet; it is the Default Profile that a
    // client is holding, and that is where the Account Perch lands on has to be
    // written (ADR 0005).
    let host = client_running_against(machine_with_two_accounts(), DEFAULT_PROFILE, 5150)
        .with_answers(&["y"]);

    let (result, _) = run_remove(&host, EMAIL);

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(holds(&host, EMAIL), "and nothing was removed");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
}

#[test]
fn a_quarantined_account_is_not_what_perch_lands_on() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    quarantine(&host, SECOND_EMAIL);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the removal was agreed to");
    assert!(
        printed.contains("no active Account"),
        "landing on an Account whose Credential does not work would be no \
         landing at all:\n{printed}"
    );
    assert_eq!(registry_of(&host).active, None);
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "and the Credential of a broken Account was never made live"
    );
}

#[test]
fn an_account_that_shares_a_profile_keeps_the_credential_and_the_outcome_says_so() {
    // Two email addresses that slug to one directory share a Profile, and with
    // it a Credential Store. Rare, and the one case where deleting what the
    // Account was using would take an Account nobody asked to give up with it.
    let host = machine_holding_the_two_that_share_a_profile();
    let shared = credential_of(&host, "some-one@example.com");
    assert!(shared.is_some(), "the Profile they share holds one");

    let (result, printed) = run_remove(&host, "some-one@example.com");

    result.expect("the Account is still given up");
    assert!(!holds(&host, "some-one@example.com"), "{printed}");
    assert!(
        printed.contains("still there") && printed.contains("some.one@example.com"),
        "the outcome says what actually happened rather than claiming a \
         deletion that did not happen:\n{printed}"
    );
    assert_eq!(
        credential_of(&host, "some.one@example.com"),
        shared,
        "and the Account left behind still has something to switch to"
    );
}

/// A machine holding an ordinary active Account and two whose email addresses
/// derive one Profile between them.
fn machine_holding_the_two_that_share_a_profile() -> FakeHost {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    let mut registry = registry_of(&host);
    for email in ["some-one@example.com", "some.one@example.com"] {
        registry.upsert(Account {
            identity: Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            enabled: true,
            quarantine: None,
            group: None,
            utilization: None,
        });
    }
    common::save_registry(&host, &registry);

    let store = store_of(&host, "some-one@example.com");
    host.set_keychain_item(&store.keychain_service, &store.keychain_account, CREDENTIAL);
    host
}

#[test]
fn a_quarantined_account_can_be_retired_without_being_repaired_first() {
    let host = machine_with_two_accounts();
    quarantine(&host, SECOND_EMAIL);

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("a subscription that broke and is not coming back can still be given up");
    assert!(!holds(&host, SECOND_EMAIL), "{printed}");
}

#[test]
fn a_group_named_where_one_account_is_meant_is_refused_as_a_group() {
    let host = three_accounts_in_one_group();

    let (result, _) = run_remove(&host, "work");

    let error = result.expect_err("this acts on one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(error.to_string().contains("Group"), "{error}");
    assert_eq!(
        registry_of(&host).accounts.len(),
        3,
        "naming a Group is not a way to empty one"
    );
}

#[test]
fn a_target_that_names_nothing_is_refused_with_what_it_nearly_matched() {
    let host = machine_with_two_accounts();

    let (result, _) = run_remove(&host, "overflow");

    let error = result.expect_err("nothing Perch holds is called that");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(error.to_string().contains(SECOND_EMAIL), "{error}");
    assert_eq!(registry_of(&host).accounts.len(), 2);
}

/// A keychain item is filed under `$USER`. If that is not the name it was
/// written under — a login rename, `sudo -u`, a launchd context, or a shell
/// with `USER` unset — the delete finds nothing and reports success, while the
/// keychain goes on holding a working Credential. The plaintext copy and the
/// Profile directory do go, so there is nothing left for the user to notice by.
#[test]
fn a_removal_that_found_no_credential_does_not_claim_to_have_deleted_one() {
    let host = machine_with_two_accounts();
    let store = store_of(&host, SECOND_EMAIL);
    // What the machine looks like when the item was filed under another name.
    host.forget_keychain_item(&store.keychain_service, LOGIN_NAME);

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("the Account is still forgotten");
    assert!(!holds(&host, SECOND_EMAIL), "{printed}");
    assert!(
        !printed.contains("is deleted"),
        "nothing was deleted, so nothing says it was:\n{printed}"
    );
    assert!(
        printed.contains("$USER"),
        "and the reason a Credential might still be out there is named:\n{printed}"
    );
}

/// Removing the last Account while Perch is on nobody used to be confirmed with
/// a sentence about Claude Code "going on running as" that Account — which is
/// not true: it is not the active one, and the live store may hold somebody
/// else's Credential or none at all. Asking somebody to agree to a description
/// of a state that is not theirs is asking them to agree to nothing.
#[test]
fn the_last_account_is_confirmed_without_claiming_it_is_the_one_running() {
    let host = machine_with_two_accounts().with_answers(&["y", "y"]);
    // Nothing to land on, so giving up the active Account leaves Perch holding
    // one Account and on nobody.
    disable_account(&host, SECOND_EMAIL).0.expect("reserved");
    run_remove(&host, EMAIL)
        .0
        .expect("the active one is given up");
    assert_eq!(registry_of(&host).active, None);

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("the last Account can be given up");
    assert!(
        !printed.contains("goes on running as"),
        "it does not describe a live state it cannot know:\n{printed}"
    );
    assert!(printed.contains("on no Account"), "{printed}");
}

/// `perch remove` asks before it gives up the active Account, and a question
/// put to a person is the one wait in Perch with no bound on it. The registry
/// lock is held across it — the whole command is one load, one decision and one
/// save — so a question nobody answers for a while is where that lock can go
/// stale under a command that is otherwise behaving perfectly.
///
/// A lock that is still Perch's is one this command may go on using, however
/// long the answer took.
#[test]
fn a_question_somebody_takes_their_time_over_does_not_cost_the_lock() {
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        .with_a_terminal_that_takes(5 * 60_000);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the removal runs");
    assert!(printed.contains("Removed"), "{printed}");
    assert!(registry_of(&host).account(EMAIL).is_none());
    assert!(
        perch::registry::lock(&host).is_ok(),
        "and the lock was given back rather than left behind"
    );
}

/// The other direction, and the reason the hold is checked rather than assumed.
/// If the answer took so long that another `perch` found the lock abandoned and
/// took it over, this one is working from a registry that is however many
/// commands out of date — and everything past the question deletes Credentials.
/// Refused before the first irreversible thing rather than discovered at the
/// save, by which point the Credential is already gone.
#[test]
fn an_answer_that_arrives_after_another_perch_took_the_lock_removes_nothing() {
    let credential_before = credential_of(&machine_with_two_accounts(), EMAIL);
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::registry::lock_spec(host).expect("home is known");
            host.remove_dir_all(&lock.dir).expect("it was abandoned");
            host.create_dir_exclusive(&lock.dir)
                .expect("the other `perch` takes it");
        });

    let (result, printed) = run_remove(&host, EMAIL);

    let refused = result.expect_err("this Perch may no longer act on what it read");
    assert!(
        refused.to_string().contains("Nothing was removed"),
        "{refused}"
    );
    assert!(!printed.contains("Removed"), "{printed}");
    assert!(
        registry_of(&host).account(EMAIL).is_some(),
        "the Account is still held"
    );
    assert_eq!(
        credential_of(&host, EMAIL),
        credential_before,
        "and its Credential was never touched"
    );
    // The refusal has to land before the landing, not after it. Making the
    // successor live is the first irreversible step of a removal: it replaces
    // the Credential a running client is holding, and the `active` that records
    // it is in the registry this command may no longer write.
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "nothing was made live in the Default Profile either"
    );
}
