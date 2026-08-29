//! Behavior: forgetting an Account when a subscription is retired.
//!
//! Most of these are about what a removal refuses to destroy silently. Removing
//! an Account nobody is on is unremarkable — the entry goes, the Credential
//! goes, the Alias comes free. Removing the **active** Account is the case that
//! needs care: Perch names the Account it will leave active, lands on it before
//! anything is deleted, and asks first (ADR a-removal-lands-first).

mod common;

use common::*;
use perch::commands::remove::RemoveArgs;
use perch::error::{EXIT_HELD, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
use perch::host::prelude::*;
use perch::host::{FakeHost, Refusing};
use perch::probe::Identity;
use perch::registry::{Account, Active};

const FIRST_PROFILE: &str = "/Users/someone/.config/perch/profiles/someone-example-com";
const SECOND_PROFILE: &str = "/Users/someone/.config/perch/profiles/overflow-example-com";

/// The config directory every client reads — where a Switch, and the landing a
/// removal makes, has to write.
const DEFAULT_PROFILE: &str = "/Users/someone/.claude";

/// The lock Claude Code takes around a token refresh — the one a Switch, and so
/// the landing a removal makes, has to wait for.
const REFRESH_LOCK: &str = "/Users/someone/.claude/.oauth_refresh.lock";

/// What the live store holds right now — the Credential every client reads.
fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

fn holds(host: &FakeHost, email: &str) -> bool {
    registry_of(host).account(email).is_some()
}

/// A Remove deletes the Credential and saves the registry before it reports, so
/// a stdout that will not take the report is a change that landed under a
/// non-zero exit — and unnoted it sends a script back to give up an Account it
/// has already given up, which the second time is exit 12.
#[test]
fn a_remove_that_landed_says_so_when_only_the_report_could_not_be_written() {
    /// A stdout that takes everything until the report itself, which is the
    /// window this is about: the line naming the Target is written before
    /// anything is deleted, so a stdout refusing *that* has lost nothing.
    struct NowhereToWriteTheReport;

    impl std::io::Write for NowhereToWriteTheReport {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Removed ") {
                true => Err(std::io::Error::other("No space left on device")),
                false => Ok(bytes.len()),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let host = machine_with_two_accounts();

    let refused = perch::commands::remove::run(
        &host,
        RemoveArgs {
            target: SECOND_EMAIL.to_string(),
            yes: true,
        },
        &mut NowhereToWriteTheReport,
    )
    .expect_err("the report could not be written");

    assert!(
        refused.to_string().contains("Remove itself finished"),
        "the failure says which half of the command it was: {refused}"
    );
    assert!(
        !holds(&host, SECOND_EMAIL),
        "and it was the reporting half, so the Account is given up"
    );
    assert!(
        credential_of(&host, SECOND_EMAIL).is_none(),
        "and the Credential Perch held for it is deleted"
    );
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

    // The report is one line, asserted whole: every Remove that finds a
    // Credential deletes it, so saying so is the ordinary case announcing that
    // it was ordinary (ADR perch-says-what-it-did).
    assert_eq!(
        printed.trim_end().lines().last(),
        Some(format!("Removed {SECOND_EMAIL}.").as_str()),
        "{printed}"
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
        registry_of(&host).active().whose(),
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
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
    assert_eq!(credential_of(&host, THIRD_EMAIL), kept);
    assert!(
        host.http_calls().is_empty(),
        "and nothing was asked of Anthropic"
    );
}

#[test]
fn the_group_a_removed_account_was_in_stays_declared() {
    let host = machine_with_two_accounts();
    a_group_of(&host, "retiring", &[SECOND_EMAIL]);

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
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn a_removal_that_fails_after_landing_still_records_who_is_live() {
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        .with_a_path_refusing(
            format!("{FIRST_PROFILE}/.credentials.json"),
            Refusing::Delete,
            "read-only",
        );

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect_err("the Credential could not be given up");
    assert!(holds(&host, EMAIL), "nothing was forgotten: {printed}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the landing happened, so the successor's Credential is the live one"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "and the record says so, rather than going on naming the Account whose \
         Credential a Switch would now overwrite"
    );
}

#[test]
fn a_removal_that_emptied_one_store_and_not_the_other_does_not_say_nothing_happened() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    let store = store_of(&host, EMAIL);
    // Both stores hold a copy — the state a Profile is left in by a machine that
    // has been both — and only the file will not be given up.
    host.set_file(&store.credentials_file, CREDENTIAL);
    let host = host.with_a_path_refusing(&store.credentials_file, Refusing::Delete, "read-only");

    let (result, printed) = run_remove(&host, EMAIL);

    let refused = result.expect_err("one of the two stores would not empty");
    let said = refused.to_string();
    assert!(
        !said.contains("Nothing was removed"),
        "one of the two stores is already empty: {said}"
    );
    assert!(
        said.contains("may no longer work") && said.contains("perch remove"),
        "so the user is told what state the Account is in and how to finish: \
         {said}"
    );
    assert!(
        holds(&host, EMAIL),
        "and the Account is still in the registry, so running it again finishes \
         the job: {printed}"
    );
    // The other half of saying it plainly: whatever the sentence claims, the
    // registry has to agree with it. It records no Quarantine here, so the
    // sentence must not name one.
    assert!(
        !said.contains("Quarantined"),
        "a Quarantine is a state Perch records, and this path records none: {said}"
    );
    assert_eq!(
        registry_of(&host)
            .account(EMAIL)
            .expect("the Account is still held")
            .quarantine,
        None,
        "and `perch list` would render it as healthy, which is what the \
         sentence now says"
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
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn the_account_left_active_is_one_the_user_declared_interchangeable() {
    let host = machine_with_three_accounts().with_answers(&["y"]);
    a_group_of(&host, "work", &[EMAIL, THIRD_EMAIL]);

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("removed");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(THIRD_EMAIL),
        "the Group is the one landing place the user endorsed in advance, so it \
         is preferred over the Account that merely comes first: {printed}"
    );
}

#[test]
fn a_disabled_account_is_not_what_perch_lands_on() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the removal was agreed to");
    assert!(
        printed.contains("no active Account"),
        "an Account kept out of Cycling is never chosen for you, so \
         there is nowhere to land and the question says so:\n{printed}"
    );
    assert_eq!(*registry_of(&host).active(), Active::Nobody);
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
    assert_eq!(*registry.active(), Active::Nobody);
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

    let refused = refused.expect_err("there is nobody to confirm with");
    assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
    assert!(holds(&host, EMAIL));

    let (result, printed) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("every capability is available non-interactively (ADR perch-does-not-draw)");
    assert!(!holds(&host, EMAIL), "{printed}");
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn removal_is_refused_while_the_accounts_profile_is_live() {
    let host = machine_with_two_accounts();
    a_client_running_against(&host, SECOND_PROFILE, 4242);

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
    // written (ADR a-profile-is-live-by-evidence).
    let host = machine_with_two_accounts().with_answers(&["y"]);
    a_client_running_against(&host, DEFAULT_PROFILE, 5150);

    let (result, _) = run_remove(&host, EMAIL);

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(holds(&host, EMAIL), "and nothing was removed");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
}

#[test]
fn an_account_is_given_up_on_a_machine_that_no_longer_has_claude_code_on_it() {
    let host = machine_with_two_accounts();
    host.remove_file(std::path::Path::new(CLAUDE_BIN))
        .expect("Claude Code is uninstalled");

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("a removal needs no Claude Code");
    assert!(!holds(&host, SECOND_EMAIL), "{printed}");
    assert!(
        credential_of(&host, SECOND_EMAIL).is_none(),
        "and the Credential Perch held is gone: {printed}"
    );
}

#[test]
fn the_account_you_are_on_is_given_up_on_a_machine_with_no_claude_code_either() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    host.remove_file(std::path::Path::new(CLAUDE_BIN))
        .expect("Claude Code is uninstalled");

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("a removal needs no Claude Code, landing included");
    assert!(!holds(&host, EMAIL), "{printed}");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "and it landed on the successor rather than leaving nobody active: \n{printed}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "whose Credential is the live one: {printed}"
    );
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
    assert_eq!(*registry_of(&host).active(), Active::Nobody);
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

#[test]
fn the_active_account_never_lands_on_one_whose_credential_is_not_its_own() {
    let host = machine_holding_the_two_that_share_a_profile();
    let shared = credential_of(&host, "some-one@example.com");

    let (result, printed) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("the Account is still given up");
    assert!(!holds(&host, EMAIL), "{printed}");
    let registry = registry_of(&host);
    assert_eq!(
        registry.active().whose(),
        None,
        "neither sharer is a successor, so the machine lands nowhere:\n{printed}"
    );
    assert_eq!(
        credential_of(&host, "some-one@example.com"),
        shared,
        "and the Profile they share was not read to make one of them live"
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
            disabled: false,
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

#[test]
fn a_removal_that_found_no_credential_does_not_claim_to_have_deleted_one() {
    let host = machine_with_two_accounts();
    let store = store_of(&host, SECOND_EMAIL);
    // What the machine looks like when the item was filed under another name.
    host.forget_keychain_item(&store.keychain_service, LOGIN_NAME);

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("the Account is still forgotten");
    assert!(!holds(&host, SECOND_EMAIL), "{printed}");
    // Asserted whole, because the claim is the sentence: one of the two
    // outcomes that speaks at all, and what it must not do is claim a deletion
    // that never happened.
    assert!(
        printed.contains(&format!(
            "Removed {SECOND_EMAIL}. Neither of its Credential Stores held \
             anything to delete, and on macOS a keychain item is filed under \
             `$USER`, so one written under a different login name is still \
             there."
        )),
        "nothing was deleted, so nothing says it was, and the reason a \
         Credential might still be out there is named:\n{printed}"
    );
}

#[test]
fn a_removal_off_macos_explains_the_store_that_machine_actually_has() {
    let host = logged_in_machine_off_macos().with_answers(&["y"]);
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    host.remove_file(&store_of(&host, EMAIL).credentials_file)
        .expect("the Profile's file goes");

    let (result, printed) = run_remove(&host, EMAIL);

    result.expect("the Account is still forgotten");
    assert!(
        printed.contains("held anything to delete"),
        "the sentence this is about is printed at all:\n{printed}"
    );
    assert!(
        !printed.contains("$USER") && !printed.contains("keychain"),
        "and a machine with no keychain is not told about one:\n{printed}"
    );
}

#[test]
fn the_last_account_is_confirmed_without_claiming_it_is_the_one_running() {
    let host = machine_with_two_accounts().with_answers(&["y", "y"]);
    // Nothing to land on, so giving up the active Account leaves Perch holding
    // one Account and on nobody.
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");
    run_remove(&host, EMAIL)
        .0
        .expect("the active one is given up");
    assert_eq!(*registry_of(&host).active(), Active::Nobody);

    let (result, printed) = run_remove(&host, SECOND_EMAIL);

    result.expect("the last Account can be given up");
    assert!(
        !printed.contains("goes on running as"),
        "it does not describe a live state it cannot know:\n{printed}"
    );
    assert!(printed.contains("on no Account"), "{printed}");
}

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
        perch::holdings::lock(&host).is_ok(),
        "and the lock was given back rather than left behind"
    );
}

#[test]
fn an_answer_that_arrives_after_another_perch_took_the_lock_removes_nothing() {
    let credential_before = credential_of(&machine_with_two_accounts(), EMAIL);
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::holdings::lock_spec(host).expect("home is known");
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
    // The refusal has to land before the landing: making the successor live
    // replaces the Credential a running client is holding, and the `active` that
    // records it is in a registry this command may no longer write.
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "nothing was made live in the Default Profile either"
    );
}

#[test]
fn a_client_that_starts_while_the_question_is_answered_stops_the_removal() {
    let host = machine_with_two_accounts()
        .with_answers(&["y"])
        // The fake performs this the first time Perch waits, which is the
        // confirmation: the terminal has to take some time for there to be a
        // window at all.
        .with_a_terminal_that_takes(1_000)
        .once_while_waiting(|host| a_run_against(host, EMAIL, host.now()));

    let (result, printed) = run_remove(&host, EMAIL);

    let refused = result.expect_err("something started holding that Profile");
    assert_eq!(refused.exit_code(), EXIT_PROFILE_LIVE, "{refused}");
    assert!(
        holds(&host, EMAIL),
        "and nothing was removed, however the question was answered: {printed}"
    );
    assert!(
        credential_of(&host, EMAIL).is_some(),
        "the Credential the client is holding is still there"
    );
}

#[test]
fn a_lock_somebody_is_holding_stops_a_removal_as_held_rather_than_as_a_fault() {
    let host = machine_with_two_accounts();
    let now = host.now();
    // Held, and touched just now: somebody is alive behind it.
    let host = host
        .with_dir_held_since(REFRESH_LOCK, now)
        .with_answers(&["y"]);

    let (result, printed) = run_remove(&host, EMAIL);

    let refused = result.expect_err("the lock is Claude Code's");
    assert_eq!(
        refused.exit_code(),
        EXIT_HELD,
        "a lock that will be given back is not a permanent failure: {refused}"
    );
    assert!(
        refused.to_string().contains("is held by Claude Code"),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("Nothing was removed"),
        "and it still says what the machine is holding now: {refused}"
    );
    assert!(holds(&host, EMAIL), "nothing was removed: {printed}");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the Credential is still the live one"
    );
}

#[test]
fn a_removal_that_deleted_the_credential_but_could_not_be_recorded_says_so() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        REGISTRY_PATH,
        Refusing::Write,
        "read-only",
    );

    let (result, _) = run_remove_with(
        &host,
        RemoveArgs {
            target: SECOND_EMAIL.to_string(),
            yes: true,
        },
    );

    let said = result
        .expect_err("the record could not be written")
        .to_string();
    assert!(
        said.contains(&format!(
            "The Credential Perch held for {SECOND_EMAIL} is already deleted"
        )),
        "{said}"
    );
    assert!(
        said.contains("can no longer switch to"),
        "it says what the record it still holds is worth: {said}"
    );
}

#[test]
fn a_landing_perch_cannot_write_down_removes_nothing_and_moves_nothing() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        REGISTRY_PATH,
        Refusing::Write,
        "read-only",
    );

    let (result, _) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    let said = result
        .expect_err("the Landing could not be written")
        .to_string();
    assert!(
        said.contains("has written down that it is about to"),
        "it says why nothing moved: {said}"
    );
    assert!(said.contains("Nothing was removed"), "{said}");
    assert!(holds(&host, EMAIL), "and the Account is still held");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "with its own Credential still the live one"
    );
}

#[test]
fn removing_the_active_account_with_several_left_counts_them_rather_than_naming_one() {
    let host = machine_with_three_accounts();
    for email in [SECOND_EMAIL, THIRD_EMAIL] {
        disable_account(&host, email)
            .0
            .expect("it stops being a candidate");
    }

    let (result, printed) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("it is removed");
    assert!(
        printed.contains("Perch holds no active Account now"),
        "{printed}"
    );
    assert!(
        printed.contains("one of the 2 it still holds"),
        "with two left there is no single one to name: {printed}"
    );
}

#[test]
fn removing_the_active_account_with_one_left_names_it_as_the_one() {
    let host = machine_with_two_accounts();
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("it stops being a candidate");

    let (result, printed) = run_remove_with(
        &host,
        RemoveArgs {
            target: EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("it is removed");
    assert!(printed.contains("the one it still holds"), "{printed}");
    assert!(!printed.contains("one of the 1"), "{printed}");
}

#[test]
fn a_profile_directory_that_will_not_go_is_a_note_rather_than_a_failure() {
    let host = machine_with_two_accounts();
    // Derived the way the note derives it rather than spelled by hand: a Windows
    // build joins paths with the other separator, so a fixture holding the
    // forward-slash spelling would assert on a path nothing ever prints.
    let profile = store_of(&host, SECOND_EMAIL).config_dir;
    let host = host.with_a_path_refusing(&profile, Refusing::Delete, "in use");

    let (result, _) = run_remove_with(
        &host,
        RemoveArgs {
            target: SECOND_EMAIL.to_string(),
            yes: true,
        },
    );

    result.expect("a directory that will not go is not a failed removal");
    assert!(!holds(&host, SECOND_EMAIL), "the Account is forgotten");
    let notes = host.notes().join("\n");
    assert!(
        notes.contains(&profile.display().to_string())
            && notes.contains("deleting it by hand is safe"),
        "{notes}"
    );
}
