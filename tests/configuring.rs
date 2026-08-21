//! Behavior: `perch config set` and `perch config get`.
//!
//! As much about the refusals as the Settings: a key or a value Perch does not
//! understand is answered with the ones it does, because a script that mistyped
//! a Setting must not go on believing it took (ADR perch-does-not-draw).
//!
//! The watcher's fields govern `perch watcher run` and nothing else, which is
//! asserted here too — setting them switches nothing on
//! (ADR a-watcher-knob-is-arithmetic). What the loop does with them is
//! `watching.rs`.

mod common;

use chrono::Duration;
use common::*;
use perch::error::{EXIT_INVALID, EXIT_NOT_FOUND, EXIT_NOT_INTERCHANGEABLE};
use perch::host::FakeHost;
use perch::host::prelude::*;
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
    registry_of(host).active().whose().map(str::to_string)
}

/// The Settings a Group holds, which are the whole of what a Cycle there
/// follows: there is nothing above a Scope for a value to have come from
/// (ADR a-setting-names-its-scope).
fn group_config(host: &FakeHost, name: &str) -> perch::registry::Settings {
    registry_of(host).settings(&perch::registry::Scope::Group(name.to_string()))
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
        "work strategy soonest-reset",
        "a value read back is the tail of the `set` that would restore it, so \
         reading and writing are one vocabulary"
    );
}

#[test]
fn the_declaration_the_ungrouped_accounts_carry_is_set_and_read_back() {
    let host = machine_with_two_accounts();

    let (result, _) = config_set(&host, &["ungrouped", "interchangeable", "true"]);

    result.expect(
        "`interchangeable` is the Ungrouped Scope's to carry (ADR a-group-is-a-declaration)",
    );
    assert!(registry_of(&host).ungrouped.interchangeable);

    let (result, printed) = config_get(&host, &["ungrouped", "interchangeable"]);

    result.expect("what was set can be read");
    assert_eq!(printed.trim(), "ungrouped interchangeable true");
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
        printed.contains("ungrouped interchangeable false"),
        "the declaration only the Accounts in no Group carry is shown against \
         them: {printed}"
    );
    assert!(
        printed.contains("ungrouped strategy most-headroom"),
        "every Scope's Config in full, each line naming its Scope: {printed}"
    );
    assert!(
        printed.contains("work watcher-threshold-percent 90"),
        "including the one that was set: {printed}"
    );
    assert!(
        printed.contains("work strategy most-headroom"),
        "and the ones nobody has said anything about, because a Scope holds \
         every Setting there is rather than falling back for them: {printed}"
    );
    assert!(
        !printed.contains("work interchangeable"),
        "and no line a Group could not take back: a Group is the declaration \
         that its Accounts are interchangeable: {printed}"
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
        printed.contains(&format!(
            "Switched to {SECOND_EMAIL}, the soonest reset in Group `work`."
        )),
        "and the landing line says what it chose on, in the terms it was judged \
         on, beside the Scope it stayed inside: {printed}"
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
        printed.contains(&format!(
            "Switched to {THIRD_EMAIL}, the most room in Group `work`."
        )),
        "and the landing line says the room it fell back to rather than \
         passing the choice off as the ranking that was asked for: {printed}"
    );
}

#[test]
fn the_soonest_resetting_strategy_still_measures_headroom_by_the_worst_window() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    // Resets soonest by a distance, and is exhausted on a window that is not
    // the one you hit first: how headroom is measured is a separate axis from
    // which Account a Cycle prefers (ADR headroom-is-the-worst-window).
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

    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("that is the declaration the refusal named");

    let (result, printed) = run_cycle(&host);

    result.expect("they have been declared interchangeable now");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{printed}");
}

#[test]
fn cycling_among_ungrouped_accounts_is_off_until_it_is_turned_on() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_get(&host, &["ungrouped", "interchangeable"]);

    result.expect("a setting nobody has touched still reads back");
    assert_eq!(
        printed.trim(),
        "ungrouped interchangeable false",
        "being ungrouped is the absence of a declaration that Accounts are \
         interchangeable, not a weaker form of one"
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
        printed.contains("perch watcher run"),
        "and what now may act is named: {printed}"
    );
    // Granting permission is not starting anything: somebody who typed this
    // and walked away has a Group that *may* be acted on and nothing acting.
    assert!(
        printed.contains("Nothing here starts one"),
        "a Group that may be acted on is not a Watcher that has been switched \
         on (ADR the-machine-runs-the-watcher): {printed}"
    );
    assert!(
        printed.contains("perch watcher install"),
        "and all three ways of running one are named, because a sentence about \
         the loop alone leaves somebody with a Service no reason to read it: \
         {printed}"
    );

    config_set(&host, &["work", "watcher-threshold-percent", "50"])
        .0
        .expect("50 is a percentage");
    assert_eq!(group_config(&host, "work").watcher_threshold_percent, 50);

    // Both Accounts are well past a 50% threshold and the watcher may act —
    // and nothing has switched, because nothing is running the loop.
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "permission is not a process: configuring a Group switches nothing \
         until `perch watcher run` is running"
    );
}

#[test]
fn the_watchers_may_act_field_is_off_until_it_is_asked_for() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_get(&host, &["work", "watcher-may-act"]);

    result.expect("a field nobody has touched still reads back");
    assert_eq!(
        printed.trim(),
        "work watcher-may-act false",
        "a Group only ever changes underneath someone because they said it \
         could, and the line says which Group that is"
    );
}

/// A default is a promise, and this is where it is kept.
#[test]
fn a_group_starts_with_the_watcher_policy_the_adr_names() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_get(&host, &["work"]);

    result.expect("naming a Group asks about every Setting it holds");
    assert!(
        printed.contains("work watcher-threshold-percent 80"),
        "the default, said as the `set` that would restore it: {printed}"
    );
}

#[test]
fn a_watcher_number_out_of_range_is_refused_with_the_range_it_accepts() {
    let host = three_accounts_in_one_group();

    for (key, value, accepted) in [
        ("watcher-threshold-percent", "101", "100"),
        ("watcher-threshold-percent", "-5", "100"),
        ("watcher-threshold-percent", "four fifths", "100"),
    ] {
        let (result, _) = config_set(&host, &["work", key, value]);

        let error = result.expect_err("out of the range the key accepts");
        assert_eq!(error.exit_code(), EXIT_INVALID, "{error}");
        assert!(
            error.to_string().contains(accepted),
            "the numbers that would have been accepted are named: {error}"
        );
    }

    assert_eq!(
        group_config(&host, "work").watcher_threshold_percent,
        80,
        "refused, so unchanged"
    );
}

/// The three the Watcher does not carry: refused in the same words any other
/// unknown key is, with no half-life in which they are quietly ignored.
#[test]
fn the_settings_the_watcher_shed_are_no_longer_keys_a_scope_carries() {
    let host = three_accounts_in_one_group();

    for (key, value) in [
        ("watcher-cooldown-minutes", "30"),
        ("watcher-margin-percent", "25"),
        ("watcher-no-return", "false"),
    ] {
        let (result, _) = config_set(&host, &["work", key, value]);
        let error = result.expect_err("not a Setting any more");
        assert_eq!(error.exit_code(), EXIT_INVALID, "{error}");
        assert!(error.to_string().contains(key), "{error}");

        let (result, _) = config_get(&host, &["work", key]);
        result.expect_err("and there is nothing to read back either");
    }

    let (result, printed) = config_get(&host, &["work"]);
    result.expect("the Settings that are left still read back");
    for gone in [
        "watcher-cooldown-minutes",
        "watcher-margin-percent",
        "watcher-no-return",
    ] {
        assert!(
            !printed.contains(gone),
            "and none of the three is on the listing either: {printed}"
        );
    }
}

#[test]
fn an_unknown_key_is_refused_and_names_the_ones_a_group_carries() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["work", "stratagem", "soonest-reset"]);

    let error = result.expect_err("Perch does not know that key");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(message.contains("stratagem"), "{message}");
    assert!(message.contains("Group `work`"), "{message}");
    assert!(message.contains("strategy"), "{message}");
    assert!(message.contains("watcher-may-act"), "{message}");
    assert!(message.contains("watcher-threshold-percent"), "{message}");
}

#[test]
fn a_set_naming_a_key_but_no_scope_is_refused_and_names_the_scopes_there_are() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["watcher-threshold-percent", "70"]);

    let error = result.expect_err("there is no Scope for that to be about");
    assert_eq!(error.exit_code(), EXIT_INVALID, "{error}");
    let message = error.to_string();
    assert!(message.contains("names no Scope"), "{message}");
    assert!(message.contains("ungrouped"), "{message}");
    assert!(
        message.contains("Groups Perch holds: work."),
        "the Scopes there are to name: {message}"
    );
    assert_eq!(
        group_config(&host, "work").watcher_threshold_percent,
        80,
        "and nothing was written"
    );
}

#[test]
fn a_set_naming_neither_a_scope_nor_a_key_is_answered_about_the_scope() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["wrok", "strategy"]);

    let refusal = result.expect_err("there is no Scope called that");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    assert!(
        refusal.to_string().contains("work"),
        "the Group they probably meant is named, exactly as it is when they \
         remember the value: {refusal}"
    );

    let (result, _) = config_set(&host, &["global", "watcher-may-act"]);

    let refusal = result.expect_err("there is no Scope every other one falls back to");
    assert!(
        refusal
            .to_string()
            .contains("every Setting is said about the Scope it governs"),
        "and `global` still meets the refusal written for it: {refusal}"
    );
}

#[test]
fn naming_global_as_a_scope_says_there_is_no_such_scope_rather_than_offering_a_group() {
    let host = three_accounts_in_one_group();

    for words in [
        vec!["global", "strategy", "soonest-reset"],
        vec!["Global", "watcher-may-act", "true"],
    ] {
        let (result, _) = config_set(&host, &words);

        let error = result.expect_err("there is no Scope every other one falls back to");
        let message = error.to_string();
        assert!(
            !message.contains("perch group add"),
            "declaring a Group by that name is the one repair that makes this \
             worse: {message}"
        );
        assert!(
            message.contains("every Setting is said about the Scope it governs"),
            "the reason there is no such word is what makes the refusal useful: \
             {message}"
        );
    }

    assert_eq!(
        group_config(&host, "work").strategy,
        Strategy::MostHeadroom,
        "and nothing was written"
    );
}

#[test]
fn global_is_still_a_reserved_word_and_the_refusal_says_why() {
    let host = machine_with_two_accounts();

    let (result, _) = run_group(
        &host,
        perch::commands::group::GroupCommand::Add {
            name: "global".to_string(),
        },
    );

    let refusal = result.expect_err("`global` is how people say every Scope at once");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(said.contains("every Scope at once"), "{said}");
    assert!(
        said.contains("perch config set <scope> <key> <value>"),
        "and the form that does exist is named: {said}"
    );
    assert!(
        registry_of(&host).groups.is_empty(),
        "and no Group was declared"
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
fn a_key_named_with_no_value_is_refused_with_the_form_the_command_takes() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["strategy"]);

    let error = result.expect_err("nothing was said to set it to");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    let message = error.to_string();
    assert!(
        message.contains("perch config set <scope> <key> <value>"),
        "the one form there is, which has a subject in it: {message}"
    );
    assert!(
        message.contains("Groups Perch holds: work."),
        "and the Scopes it could be about: {message}"
    );
}

#[test]
fn a_get_of_one_word_that_is_no_scope_is_refused_the_way_a_typo_always_is() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_get(&host, &["wrok"]);

    let refusal = result.expect_err("there is no Scope called that");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    let said = refusal.to_string();
    assert!(
        said.contains("work"),
        "the Group they probably meant is named: {said}"
    );
}

#[test]
fn a_get_of_a_key_alone_says_a_setting_is_read_about_a_scope() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_get(&host, &["strategy"]);

    let refusal = result.expect_err("a Setting on its own is about nothing");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    let said = refusal.to_string();
    assert!(said.contains("rather than a Scope"), "{said}");
    assert!(
        said.contains("perch config get <scope> strategy"),
        "and the form that reads it is named: {said}"
    );
}

#[test]
fn a_get_of_too_many_words_is_answered_with_the_forms_get_takes() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_get(&host, &["work", "strategy", "extra"]);

    let refusal = result.expect_err("`get` takes at most two words");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(said.contains("was given 3 words"), "{said}");
    assert!(said.contains("perch config get <scope> <key>"), "{said}");
    assert!(
        said.contains("reads every Scope there is"),
        "it names the bare form too: {said}"
    );
    assert!(
        !said.contains("perch config set"),
        "the forms of `set` are not the forms of `get`: {said}"
    );
}

#[test]
fn a_single_word_is_counted_as_one_word() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["strategy"]);

    let said = result.expect_err("`set` needs a value").to_string();
    assert!(said.contains("was given 1 word"), "{said}");
    assert!(!said.contains("1 words"), "{said}");
}

#[test]
fn a_set_naming_a_group_and_a_key_says_the_value_is_what_is_missing() {
    let host = three_accounts_in_one_group();
    let before = registry_of(&host).groups;

    let (result, _) = config_set(&host, &["work", "strategy"]);

    let refusal = result.expect_err("nothing was given to set it to");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(
        said.contains("names Group `work` and a key, but nothing to set it to"),
        "{said}"
    );
    assert!(
        said.contains("perch config set <scope> <key> <value>"),
        "{said}"
    );
    assert_eq!(
        registry_of(&host).groups,
        before,
        "and a refused `set` changes nothing"
    );
}

#[test]
fn a_set_naming_a_group_and_a_word_that_is_no_key_says_what_is_wrong_with_the_word() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["work", "stratgy"]);

    let refusal = result.expect_err("`stratgy` is not a Setting");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(
        said.contains("`stratgy` is not a Setting"),
        "the word that is wrong is the one named: {said}"
    );
    assert!(
        !said.contains("nothing to set it to"),
        "and it does not claim the value is what is missing: {said}"
    );
}

#[test]
fn getting_a_setting_reads_alongside_another_perch_rather_than_waiting_on_it() {
    let host = three_accounts_in_one_group();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, printed) = config_get(&host, &["work", "strategy"]);

    result.expect("a read does not wait on a writer");
    assert_eq!(printed.trim(), "work strategy most-headroom", "{printed}");
    drop(held);
}

#[test]
fn setting_one_waits_for_the_other_perch_because_it_writes() {
    let host = three_accounts_in_one_group();
    let _held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, _) = config_set(&host, &["work", "strategy", "soonest-reset"]);

    let refused = result.expect_err("a writer waits on a writer");
    assert!(
        refused.to_string().contains("the Perch registry lock"),
        "{refused}"
    );
}

#[test]
fn a_group_name_with_a_space_in_it_is_refused_rather_than_breaking_the_round_trip() {
    let host = machine_with_two_accounts();

    let (result, _) = run_group(
        &host,
        perch::commands::group::GroupCommand::Add {
            name: "my work".to_string(),
        },
    );

    let refusal = result.expect_err("a name no line of `config get` could name");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal.to_string().contains("has a space in it"),
        "{refusal}"
    );
    assert!(
        registry_of(&host).groups.is_empty(),
        "and no Group was declared"
    );
}

#[test]
fn a_setting_said_about_one_scope_reaches_no_other() {
    let host = three_accounts_in_one_group();
    declare_group(&host, "personal");

    let (result, printed) = config_set(&host, &["work", "watcher-threshold-percent", "60"]);

    result.expect("every Setting is a Scope's");
    assert!(printed.contains("Group `work`"), "{printed}");
    assert_eq!(group_config(&host, "work").watcher_threshold_percent, 60);
    assert_eq!(
        group_config(&host, "personal").watcher_threshold_percent,
        80,
        "the Group nobody said anything about is at the compiled-in default"
    );
    assert_eq!(
        registry_of(&host)
            .settings(&perch::registry::Scope::Ungrouped)
            .watcher_threshold_percent,
        80
    );
}

#[test]
fn a_group_declared_after_a_grant_is_not_covered_by_it() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("the Group may be acted on");

    declare_group(&host, "personal");

    assert!(
        !group_config(&host, "personal").watcher_may_act,
        "a Group that did not exist when the grant was said cannot have been \
         included in it"
    );
}

#[test]
fn every_line_names_the_scope_it_is_about() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("it takes");

    let (_, said) = config_get(&host, &["work", "strategy"]);
    let (_, untouched) = config_get(&host, &["work", "watcher-may-act"]);

    assert_eq!(said.trim(), "work strategy soonest-reset");
    assert_eq!(
        untouched.trim(),
        "work watcher-may-act false",
        "a Setting nobody has said anything about is still this Group's, at the \
         compiled-in default"
    );
    for line in [said.trim(), untouched.trim()] {
        assert_eq!(
            line.split_whitespace().count(),
            3,
            "every line is the whole of the `set` that would restore it: {line}"
        );
    }
}

#[test]
fn the_ungrouped_accounts_are_a_scope_that_can_be_addressed() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_set(&host, &["ungrouped", "strategy", "soonest-reset"]);

    result.expect("`ungrouped` addresses the Accounts in no Group");
    assert!(printed.contains("soonest-reset"), "{printed}");
    assert_eq!(
        registry_of(&host)
            .settings(&perch::registry::Scope::Ungrouped)
            .strategy,
        Strategy::SoonestReset,
    );

    let (_, read_back) = config_get(&host, &["ungrouped", "strategy"]);
    assert_eq!(read_back.trim(), "ungrouped strategy soonest-reset");
}

#[test]
fn a_group_cannot_take_the_name_that_addresses_the_ungrouped_scope() {
    let host = machine_with_two_accounts();

    let (result, _) = run_group(
        &host,
        perch::commands::group::GroupCommand::Add {
            name: "Ungrouped".to_string(),
        },
    );

    let refusal = result.expect_err("that name is taken by a Scope");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal.to_string().contains("perch config"),
        "and it says what already answers to it: {refusal}"
    );
}

#[test]
fn the_ungrouped_scopes_settings_read_back_in_the_form_that_would_set_them() {
    let host = machine_with_two_accounts();
    config_set(&host, &["ungrouped", "watcher-threshold-percent", "45"])
        .0
        .expect("it takes");

    let (result, printed) = config_get(&host, &[]);

    result.expect("naming nothing asks about everything");
    assert!(
        printed.contains("ungrouped watcher-threshold-percent 45"),
        "{printed}"
    );
}

#[test]
fn the_ungrouped_page_shows_the_declaration_it_carries() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_get(&host, &["ungrouped"]);

    result.expect("`ungrouped` is a Scope");
    assert!(
        printed.contains("ungrouped interchangeable false"),
        "{printed}"
    );
    assert!(
        printed.contains("ungrouped strategy most-headroom"),
        "{printed}"
    );
}

#[test]
fn a_group_neither_shows_nor_takes_the_declaration_that_is_a_group() {
    let host = three_accounts_in_one_group();

    let refusal = config_set(&host, &["work", "interchangeable", "true"])
        .0
        .expect_err("a Group is that declaration rather than holding one");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(said.contains("only they carry it"), "{said}");
    assert!(
        said.contains("perch config set ungrouped interchangeable <value>"),
        "and where it is said instead: {said}"
    );

    let (result, printed) = config_get(&host, &["work"]);
    result.expect("the Group's page still reads back");
    assert!(
        !printed.contains("interchangeable"),
        "and carries no line it would refuse to take back: {printed}"
    );

    let (result, printed) = config_get(&host, &["work", "interchangeable"]);
    result.expect_err("nor answers for it by name");
    assert!(printed.is_empty(), "{printed}");
}

/// A Group hides this: "Group `work`" is a name, and a name is spelled the same
/// wherever it appears, so every sentence here reads correctly until somebody
/// has no Group.
#[test]
fn the_ungrouped_scope_is_named_mid_sentence_the_way_a_sentence_names_it() {
    let host = machine_with_two_accounts();

    let (set, said) = config_set(&host, &["ungrouped", "strategy", "soonest-reset"]);
    set.expect("the Ungrouped Scope carries a Strategy");
    assert!(
        said.contains("on the Ungrouped Scope"),
        "a capital mid-sentence reads as a different noun: {said}"
    );

    let (refused, _) = config_get(&host, &["ungrouped", "no-such-key"]);
    let why = refused.expect_err("there is no such Setting").to_string();
    assert!(
        why.contains("Setting the Ungrouped Scope carries"),
        "and so does the refusal that names it: {why}"
    );
}
