//! Behaviour: `perch config set` and `perch config get`.
//!
//! Perch has to be complete over SSH and in CI, so every capability is
//! available non-interactively (ADR 0011) — this is the one that changes how a
//! Group behaves. The tests are as much about the refusals as the settings: a
//! key or a value Perch does not understand is answered with the ones it does,
//! because a script that mistyped a setting must not go on believing it took.
//!
//! A Group's Strategy decides which Account a bare `perch switch` lands on, and
//! the global ungrouped-Cycling setting decides whether it may land anywhere at
//! all from an Account in no Group (ADR 0017). The watcher's fields govern
//! `perch watch` and nothing else, which is asserted here too: setting them
//! switches nothing on, because nothing acts on them until somebody runs the
//! loop (ADR 0013). What the loop then does with them is `watching.rs`.

mod common;

use chrono::Duration;
use common::*;
use perch::error::{EXIT_INVALID, EXIT_NOT_FOUND, EXIT_NOT_INTERCHANGEABLE};
use perch::host::{FakeHost, Host};
use perch::registry::Strategy;

/// Three Accounts in one Group where the two Strategies disagree: the Account
/// with the most room is not the one whose quota is about to be thrown away,
/// so which one a bare `perch switch` lands on says which Strategy was read.
fn where_the_strategies_disagree() -> FakeHost {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(
        &host,
        SECOND_EMAIL,
        vec![resetting(
            "5-hour",
            70.0,
            host.now() + Duration::minutes(20),
        )],
    );
    observed(
        &host,
        THIRD_EMAIL,
        vec![resetting("5-hour", 10.0, host.now() + Duration::hours(4))],
    );
    host
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active
}

fn group_config(host: &FakeHost, name: &str) -> perch::registry::GroupConfig {
    registry_of(host)
        .group(name)
        .expect("a Group Perch holds")
        .clone()
}

#[test]
fn a_groups_setting_is_set_from_a_script_and_read_back() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_set(&host, &["work", "strategy", "soonest-reset"]);

    result.expect("`strategy` is a Group's to carry");
    assert!(printed.contains("soonest-reset"), "{printed}");
    assert_eq!(group_config(&host, "work").strategy, Strategy::SoonestReset);

    let (result, printed) = config_get(&host, &["work", "strategy"]);

    result.expect("what was set can be read");
    assert_eq!(
        printed.trim(),
        "soonest-reset",
        "a value read back on its own is the value and nothing else, so a \
         script can use it without parsing prose"
    );
}

#[test]
fn a_global_setting_is_set_and_read_back_without_naming_a_group() {
    let host = machine_with_two_accounts();

    let (result, _) = config_set(&host, &["cycle-ungrouped", "true"]);

    result.expect("an ungrouped Account has no Group to carry this (ADR 0017)");
    assert!(registry_of(&host).global.cycle_ungrouped);

    let (result, printed) = config_get(&host, &["cycle-ungrouped"]);

    result.expect("what was set can be read");
    assert_eq!(printed.trim(), "true");
}

#[test]
fn every_setting_reads_back_in_the_form_that_would_set_it_again() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "watcher-threshold-percent", "90"])
        .0
        .expect("a percentage is a percentage");

    let (result, printed) = config_get(&host, &[]);

    result.expect("naming nothing asks about everything");
    assert!(
        printed.contains("cycle-ungrouped false"),
        "the setting that belongs to no Group is shown too: {printed}"
    );
    assert!(
        printed.contains("work strategy most-headroom"),
        "and every key a Group carries, set or not: {printed}"
    );
    assert!(
        printed.contains("work watcher-threshold-percent 90"),
        "{printed}"
    );
    for line in printed.lines().filter(|line| !line.trim().is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (result, _) = config_set(&host, &words);
        result.unwrap_or_else(|err| {
            panic!("`perch config set {line}` should set what `get` just said: {err}")
        });
    }
}

#[test]
fn the_strategy_a_group_carries_changes_which_account_a_bare_switch_chooses() {
    let host = where_the_strategies_disagree();

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "out of the box a Cycle prefers the Account with the most room: {printed}"
    );
}

#[test]
fn the_soonest_resetting_account_is_chosen_when_the_group_says_to_prefer_it() {
    let host = where_the_strategies_disagree();
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("the Group carries its own Strategy");

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "quota that resets in 20 minutes is perishable, and spending it costs \
         nothing that would not have been lost anyway: {printed}"
    );
    assert!(
        printed.contains("resets soonest"),
        "the choice is explained in the terms it was judged on: {printed}"
    );
}

#[test]
fn the_soonest_resetting_strategy_falls_back_to_room_when_no_figure_says_when_anything_resets() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 90.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 50.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 5.0)]);
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("the Group carries its own Strategy");

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "a Strategy says which figure to prefer, not which figures to invent: \
         with nothing cached saying when anything comes back, the room Perch \
         can see is what is left to choose on: {printed}"
    );
    assert!(
        printed.contains("no reset time to prefer one on"),
        "and the fallback is said rather than passed off as the ranking that \
         was asked for: {printed}"
    );
}

#[test]
fn the_soonest_resetting_strategy_still_measures_headroom_by_the_worst_window() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    // Resets soonest by a distance, and is exhausted on a window that is not
    // the one you hit first. ADR 0012 fixes how headroom is measured; the
    // Strategy is a separate axis on top of it, not a way round it.
    observed(
        &host,
        SECOND_EMAIL,
        vec![
            resetting("5-hour", 4.0, host.now() + Duration::minutes(5)),
            resetting("7-day", 100.0, host.now() + Duration::hours(70)),
        ],
    );
    observed(
        &host,
        THIRD_EMAIL,
        vec![
            resetting("5-hour", 60.0, host.now() + Duration::hours(4)),
            resetting("7-day", 55.0, host.now() + Duration::hours(80)),
        ],
    );
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("the Group carries its own Strategy");

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "an Account with a full window is blocked whatever its others say, and \
         however soon they reset: {printed}"
    );
}

#[test]
fn turning_on_ungrouped_cycling_changes_what_a_bare_switch_does() {
    let host = machine_with_two_accounts();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("nobody has declared these interchangeable");
    assert_eq!(error.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    assert_eq!(active(&host).as_deref(), Some(EMAIL));

    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("that is the declaration the refusal named");

    let (result, printed) = run_cycle(&host);

    result.expect("they have been declared interchangeable now");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{printed}");
}

#[test]
fn cycling_among_ungrouped_accounts_is_off_until_it_is_turned_on() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_get(&host, &["cycle-ungrouped"]);

    result.expect("a setting nobody has touched still reads back");
    assert_eq!(
        printed.trim(),
        "false",
        "being ungrouped is the absence of a declaration that Accounts are \
         interchangeable, not a weaker form of one (ADR 0017)"
    );
}

#[test]
fn the_watchers_fields_are_stored_and_govern_a_loop_that_has_to_be_run() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 98.0)]);

    let (result, printed) = config_set(&host, &["work", "watcher-may-act", "true"]);

    result.expect("the field is the watcher's, and it is stored");
    assert!(group_config(&host, "work").watcher_may_act);
    assert!(
        printed.contains("perch watch"),
        "and what now may act is named: {printed}"
    );
    assert!(
        printed.contains("not a daemon"),
        "along with what it is not — a Group that may be acted on is not a \
         service that has been switched on (ADR 0013): {printed}"
    );

    config_set(&host, &["work", "watcher-threshold-percent", "50"])
        .0
        .expect("50 is a percentage");
    assert_eq!(group_config(&host, "work").watcher_threshold_percent, 50);

    // Both Accounts are well past a 50% threshold and the watcher has been
    // told it may act — and nothing has switched, because nothing is running
    // the loop that would.
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "permission is not a process: configuring a Group switches nothing \
         until `perch watch` is running (ADR 0013)"
    );
}

#[test]
fn the_watchers_may_act_field_is_off_until_it_is_asked_for() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_get(&host, &["work", "watcher-may-act"]);

    result.expect("a field nobody has touched still reads back");
    assert_eq!(
        printed.trim(),
        "false",
        "a Group only ever changes underneath someone because they said it \
         could (ADR 0002)"
    );
}

#[test]
fn an_unknown_key_is_refused_and_names_the_ones_a_group_carries() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["work", "stratagem", "soonest-reset"]);

    let error = result.expect_err("Perch does not know that key");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(message.contains("stratagem"), "{message}");
    assert!(message.contains("strategy"), "{message}");
    assert!(message.contains("watcher-may-act"), "{message}");
    assert!(message.contains("watcher-threshold-percent"), "{message}");
}

#[test]
fn an_unknown_global_key_is_refused_and_names_the_ones_that_belong_to_no_group() {
    let host = machine_with_two_accounts();

    let (result, _) = config_set(&host, &["cycle-everything", "true"]);

    let error = result.expect_err("Perch does not know that key");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(
        error.to_string().contains("cycle-ungrouped"),
        "the key that does exist is named, because two words without a Group \
         can only be addressing a global setting: {error}"
    );
}

#[test]
fn an_invalid_value_is_refused_and_names_the_valid_ones() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["work", "strategy", "whichever-is-cheapest"]);

    let error = result.expect_err("Perch implements two Strategies");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(message.contains("most-headroom"), "{message}");
    assert!(message.contains("soonest-reset"), "{message}");
    assert_eq!(
        group_config(&host, "work").strategy,
        Strategy::MostHeadroom,
        "a refused value leaves the setting as it was"
    );
}

#[test]
fn a_value_that_is_not_a_yes_or_a_no_is_refused_and_names_both() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["work", "watcher-may-act", "maybe"]);

    let error = result.expect_err("it may act or it may not");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(message.contains("true"), "{message}");
    assert!(message.contains("false"), "{message}");
    assert!(
        !group_config(&host, "work").watcher_may_act,
        "a refused value leaves the setting as it was"
    );
}

#[test]
fn a_watcher_threshold_that_is_not_a_percentage_is_refused() {
    let host = three_accounts_in_one_group();

    for value in ["101", "-5", "half"] {
        let (result, _) = config_set(&host, &["work", "watcher-threshold-percent", value]);

        let error = result.expect_err("a Utilization threshold is a percentage");
        assert_eq!(error.exit_code(), EXIT_INVALID, "{error}");
        assert!(error.to_string().contains("100"), "{error}");
    }
    assert_eq!(
        group_config(&host, "work").watcher_threshold_percent,
        80,
        "a refused value leaves the setting as it was"
    );
}

#[test]
fn a_setting_on_a_group_that_does_not_exist_is_refused_the_way_a_typo_always_is() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["wrok", "strategy", "soonest-reset"]);

    let error = result.expect_err("there is no Group called that");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("work"),
        "the Group they probably meant is named: {error}"
    );
}

#[test]
fn setting_a_value_a_group_already_has_is_not_a_failure() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("the first one takes");

    let (result, printed) = config_set(&host, &["work", "strategy", "soonest-reset"]);

    result.expect("a script that runs twice has not done anything wrong");
    assert!(printed.contains("already"), "{printed}");
    assert_eq!(group_config(&host, "work").strategy, Strategy::SoonestReset);
}

#[test]
fn a_key_named_with_no_value_is_refused_with_both_forms_of_the_command() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["strategy"]);

    let error = result.expect_err("nothing was said to set it to");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(
        message.contains("perch config set <group> <key> <value>"),
        "the form that addresses a Group is named: {message}"
    );
    assert!(
        message.contains("perch config set <key> <value>"),
        "and so is the form that addresses a setting belonging to none: {message}"
    );
}
