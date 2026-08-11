//! Behaviour: declaring which Accounts are interchangeable.
//!
//! A Group is a statement the user made — that these Accounts substitute for
//! one another — so these tests are mostly about that statement surviving:
//! being written down, being visible without reading a config file, and being
//! impossible to lose by accident.

mod common;

use common::*;
use perch::commands::add::AddArgs;
use perch::commands::group::GroupCommand;
use perch::error::{EXIT_CONFLICT, EXIT_GENERAL, EXIT_INVALID, EXIT_NOT_FOUND};
use perch::host::FakeHost;

fn remove_group(host: &FakeHost, name: &str) -> (perch::Result<()>, String) {
    run_group(
        host,
        GroupCommand::Remove {
            name: name.to_string(),
        },
    )
}

#[test]
fn a_declared_group_survives_a_restart() {
    let host = logged_in_machine();

    let (result, printed) = run_group(
        &host,
        GroupCommand::Add {
            name: "work".into(),
        },
    );
    assert!(result.is_ok(), "{:?}", result.err());
    assert!(printed.contains("work"), "{printed}");

    let config = registry_of(&host)
        .group("work")
        .expect("the Group is written down, not derived from the Accounts in it")
        .clone();
    assert_eq!(
        config.strategy, None,
        "a fresh Group Inherits every Setting"
    );
    assert!(
        config.is_empty(),
        "a fresh Group declares nothing of its own and Inherits Global — so a \
         threshold set at Global afterwards reaches it too (ADR 0002, amended)"
    );
    assert!(
        !registry_of(&host)
            .in_force(&perch::registry::Scope::Group("work".to_string()))
            .watcher_may_act,
        "unattended switching is off until the user says otherwise (ADR 0002)"
    );
}

#[test]
fn a_group_holding_no_accounts_is_removed() {
    let host = logged_in_machine();
    declare_group(&host, "work");

    let (result, printed) = remove_group(&host, "work");

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(printed.contains("work"), "{printed}");
    assert!(registry_of(&host).group("work").is_none());
}

#[test]
fn removing_a_group_that_still_holds_accounts_is_refused_and_names_them() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();

    let (result, _) = remove_group(&host, "work");

    let error = result.expect_err("the Accounts in it would be orphaned");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains(SECOND_EMAIL),
        "the Accounts standing in the way should be named:\n{error}"
    );
    assert!(
        registry_of(&host).group("work").is_some(),
        "and the Group is still there"
    );
}

#[test]
fn removing_a_group_perch_does_not_hold_is_refused_with_a_code_a_script_can_branch_on() {
    let host = logged_in_machine();

    let (result, _) = remove_group(&host, "work");

    let error = result.expect_err("there is no such Group");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert_ne!(error.exit_code(), EXIT_GENERAL);
    assert!(
        error.to_string().contains("work"),
        "the Group asked for should be named:\n{error}"
    );
}

#[test]
fn an_account_moves_between_groups_and_the_move_survives_a_restart() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    declare_group(&host, "overflow");
    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();

    let (result, printed) = move_to_group(&host, SECOND_EMAIL, "overflow");

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("work") && printed.contains("overflow"),
        "the move should say where the Account came from and where it went:\n{printed}"
    );
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("overflow"),
        "the Account is in one Group, not two"
    );
}

#[test]
fn an_account_is_moved_without_being_removed_and_re_added() {
    let host = machine_with_two_accounts();
    let before = registry_of(&host).account(SECOND_EMAIL).unwrap().clone();
    declare_group(&host, "work");

    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();

    let after = registry_of(&host).account(SECOND_EMAIL).unwrap().clone();
    assert_eq!(after.group.as_deref(), Some("work"));
    assert_eq!(
        after.identity, before.identity,
        "the Account is the same one, moved"
    );
    assert_eq!(after.plan, before.plan);
    assert_eq!(after.utilization, before.utilization);
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and its Profile was not rebuilt to change a Group: the stored \
         Credential is untouched"
    );
}

#[test]
fn an_account_can_be_moved_by_its_alias() {
    let host =
        logged_in_machine().with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            alias: Some("overflow".into()),
            ..AddArgs::default()
        },
    )
    .0
    .unwrap();
    declare_group(&host, "work");

    let (result, _) = move_to_group(&host, "overflow", "work");

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("work"),
        "no command should require typing an email address"
    );
}

#[test]
fn an_account_can_be_moved_out_of_every_group() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();

    let (result, _) = move_to_group(&host, SECOND_EMAIL, "none");

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        registry_of(&host).account(SECOND_EMAIL).unwrap().group,
        None,
        "an Account that joined a Group must be able to leave it again"
    );
    assert!(
        remove_group(&host, "work").0.is_ok(),
        "and the Group it left is then removable"
    );
}

#[test]
fn moving_into_a_group_perch_does_not_hold_is_refused_and_nothing_moves() {
    let host = machine_with_two_accounts();

    let (result, _) = move_to_group(&host, SECOND_EMAIL, "work");

    let error = result.expect_err("there is no such Group");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    let message = error.to_string();
    assert!(
        message.contains("work") && message.contains("group add"),
        "the user should be told the Group and how to declare it:\n{message}"
    );
    assert_eq!(
        registry_of(&host).account(SECOND_EMAIL).unwrap().group,
        None
    );
}

#[test]
fn moving_an_account_perch_does_not_hold_is_refused() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");

    let (result, _) = move_to_group(&host, "nobody@example.com", "work");

    let error = result.expect_err("there is no such Account");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("nobody@example.com"),
        "the target asked for should be named:\n{error}"
    );
}

#[test]
fn a_group_name_already_spoken_for_is_refused() {
    let host =
        logged_in_machine().with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            alias: Some("overflow".into()),
            ..AddArgs::default()
        },
    )
    .0
    .unwrap();

    let (result, _) = run_group(
        &host,
        GroupCommand::Add {
            name: "overflow".into(),
        },
    );

    let error = result.expect_err("a name cannot be both an Alias and a Group");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains(SECOND_EMAIL),
        "the Account the name already reaches should be named:\n{error}"
    );
}

#[test]
fn a_group_name_that_would_be_ambiguous_is_refused_with_its_own_code() {
    let host = logged_in_machine();

    for name in ["none", "someone@example.com", ""] {
        let (result, _) = run_group(
            &host,
            GroupCommand::Add {
                name: name.to_string(),
            },
        );
        let error = result.expect_err("`{name}` cannot name a Group");
        assert_eq!(
            error.exit_code(),
            EXIT_INVALID,
            "a name Perch will not accept is its own outcome, not a general failure: {error}"
        );
    }

    assert!(
        registry_of(&host).groups.is_empty(),
        "and none of them was created"
    );
}

#[test]
fn declaring_a_group_twice_is_refused_rather_than_resetting_its_configuration() {
    let host = logged_in_machine();
    declare_group(&host, "work");

    let (result, _) = run_group(
        &host,
        GroupCommand::Add {
            name: "work".into(),
        },
    );

    let error = result.expect_err("the Group is already there");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
}

#[test]
fn adding_an_account_to_a_group_declares_that_group() {
    let host = machine_with_two_accounts();
    let host = host.with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));

    run_add(&host, add_to_group("work")).0.unwrap();

    assert!(
        registry_of(&host).group("work").is_some(),
        "a Group named on `perch add` is a declaration too, and carries the same configuration"
    );
    let (_, printed) = run_group(&host, GroupCommand::List);
    assert!(printed.contains("work"), "{printed}");
}

#[test]
fn a_group_named_in_passing_joins_the_one_that_exists_however_it_is_capitalised() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    let host = host.with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));

    run_add(&host, add_to_group("WORK")).0.unwrap();

    let registry = registry_of(&host);
    assert_eq!(
        registry.groups.len(),
        1,
        "one Group, not two that differ only in case: {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        registry.account(THIRD_EMAIL).unwrap().group.as_deref(),
        Some("work"),
        "and the Account is in the Group that exists, not beside it"
    );
}

#[test]
fn group_list_shows_every_group_with_its_accounts_and_its_configuration() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    declare_group(&host, "spare");
    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();

    let (result, printed) = run_group(&host, GroupCommand::List);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("work") && printed.contains("spare"),
        "every Group, including the empty one:\n{printed}"
    );
    assert!(
        printed.contains(SECOND_EMAIL),
        "with the Accounts in it:\n{printed}"
    );
    assert!(
        printed.contains("most-headroom") && printed.contains("80"),
        "and the configuration governing Cycling, without reading a config file:\n{printed}"
    );
    assert!(
        printed.contains(EMAIL),
        "an Account in no Group should still be accounted for:\n{printed}"
    );
}

#[test]
fn group_list_says_so_when_nothing_has_been_declared() {
    let host = logged_in_machine();

    let (result, printed) = run_group(&host, GroupCommand::List);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("group add"),
        "an empty list should say how to declare a Group:\n{printed}"
    );
}

#[test]
fn a_stored_configuration_outside_its_range_is_refused_rather_than_read_as_something_else() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        r#"{"version":1,"accounts":[],"groups":{"work":{"strategy":"most-headroom","watcher_may_act":true,"watcher_threshold_percent":150}}}"#,
    );

    let (result, _) = run_group(&host, GroupCommand::List);

    let error = result.expect_err("150% is not a Utilization");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(
        message.contains("work") && message.contains("100"),
        "the Group and the range it broke should both be named:\n{message}"
    );
}

#[test]
fn a_stored_strategy_perch_does_not_know_is_refused() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        r#"{"version":1,"accounts":[],"groups":{"work":{"strategy":"whatever-is-cheapest"}}}"#,
    );

    let (result, _) = run_group(&host, GroupCommand::List);

    assert!(
        result.is_err(),
        "a strategy nothing implements must not be read as the default"
    );
}

#[test]
fn a_group_named_where_one_account_is_wanted_is_refused_by_saying_what_it_is() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");

    let (result, _) = move_to_group(&host, "work", "work");

    let error = result.expect_err("a Group names more than one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(error.to_string().contains("`work` is a Group."), "{error}");
    assert!(
        error.to_string().contains("one Account"),
        "being told what `work` is beats being told it is not an Account: {error}"
    );
}

#[test]
fn managing_groups_makes_no_network_call() {
    let host = machine_with_two_accounts();

    declare_group(&host, "work");
    move_to_group(&host, SECOND_EMAIL, "work").0.unwrap();
    run_group(&host, GroupCommand::List).0.unwrap();

    assert!(host.http_calls().is_empty());
}

/// Moving an Account out of no Group into no Group. It is not a failure — the
/// Account is where it was asked to be — but it must not be reported as a move
/// either, or a script reads a no-op as a change.
#[test]
fn moving_an_ungrouped_account_out_of_a_group_says_it_was_already_in_none() {
    let host = machine_with_two_accounts();

    let (result, printed) = run_group(
        &host,
        perch::commands::group::GroupCommand::Move {
            target: EMAIL.to_string(),
            group: "none".to_string(),
        },
    );

    result.expect("it is already where it was asked to be");
    assert!(
        printed.contains(&format!("{EMAIL} was already in no Group.")),
        "{printed}"
    );
    assert_eq!(registry_of(&host).account(EMAIL).expect("held").group, None);
}

/// A Group name nothing resembles, with Groups declared. The refusal names the
/// ones that exist and how to declare the one that does not — a near miss gets
/// the suggestion instead, on the grounds that it is a typo rather than an
/// intention.
#[test]
fn a_group_name_resembling_nothing_is_answered_with_the_groups_that_exist() {
    let host = three_accounts_in_one_group();

    let (result, _) = move_to_group(&host, SECOND_EMAIL, "zzzzzzzz");

    let said = result.expect_err("there is no such Group").to_string();
    assert!(said.contains("No Group called `zzzzzzzz`"), "{said}");
    assert!(said.contains("Groups Perch holds: work."), "{said}");
    assert!(
        said.contains("perch group add zzzzzzzz"),
        "and how to declare it: {said}"
    );
}

/// A Group whose watcher has been turned on reads as one that may act, with
/// the whole policy rather than the threshold alone: a summary naming only when
/// it acts would read as the whole of what it does (ADR 0013).
#[test]
fn a_group_listing_says_when_its_watcher_may_switch_unattended() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("the watcher is turned on");

    let (result, printed) = run_group(&host, perch::commands::group::GroupCommand::List);

    result.expect("a listing");
    assert!(printed.contains("may switch unattended at"), "{printed}");
    assert!(
        printed.contains("onto") && printed.contains("at most every"),
        "the whole policy, not the threshold alone: {printed}"
    );
    assert!(
        !printed.contains("off (would act"),
        "it is on, so it must not read as off: {printed}"
    );
}

/// `perch group list` writes nothing, so it does not wait on a writer either.
#[test]
fn listing_the_groups_reads_alongside_another_perch_rather_than_waiting_on_it() {
    let host = three_accounts_in_one_group();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, printed) = run_group(&host, perch::commands::group::GroupCommand::List);

    result.expect("a read does not wait on a writer");
    assert!(printed.contains("work"), "{printed}");
    drop(held);
}
