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

/// The Settings actually in force for a Group: Global's, with whatever that
/// Group Overrides laid over them (ADR 0002, amended). What a Cycle would
/// follow, which is what these tests are about — `overrides_of` is for the ones
/// about the layering itself.
fn group_config(host: &FakeHost, name: &str) -> perch::registry::Settings {
    registry_of(host).in_force(&perch::registry::Scope::Group(name.to_string()))
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
         reading and writing are one vocabulary — and the word count is what \
         says the value is this Group's Override rather than Global's default"
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
    assert_eq!(printed.trim(), "cycle-ungrouped true");
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
        printed.contains("strategy most-headroom"),
        "and every Setting at Global, which is where a Scope that says nothing \
         reads it from: {printed}"
    );
    assert!(
        printed.contains("work watcher-threshold-percent 90"),
        "and every Override there is, named by the Scope holding it: {printed}"
    );
    assert!(
        !printed.contains("work strategy"),
        "and nothing else of that Group's, because it Overrides nothing else — \
         a three-word line for an Inheritance would assert an Override that is \
         not there, and replaying this output would plant it: {printed}"
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
        "cycle-ungrouped false",
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
    // The distinction this has always protected, and which matters more now
    // that a Service exists rather than less: granting permission is not
    // starting anything. Somebody who typed this and walked away has a Group
    // that *may* be acted on and nothing acting on it.
    assert!(
        printed.contains("Nothing here starts one"),
        "a Group that may be acted on is not a Watcher that has been switched \
         on (ADR 0013, ADR 0040): {printed}"
    );
    assert!(
        printed.contains("perch service install"),
        "and all three ways of running one are named, because a sentence about \
         the loop alone leaves somebody with a Service no reason to read it: \
         {printed}"
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
        "watcher-may-act false",
        "a Group only ever changes underneath someone because they said it \
         could (ADR 0002) — and the two words say the `false` is Global's \
         default rather than something this Group declared"
    );
}

/// The rest of the watcher's policy: how often it may act, how much emptier the
/// Account it moves to has to be, and whether coming straight back is allowed
/// (ADR 0013). All three are the Group's, in the vocabulary the other settings
/// already use.
#[test]
fn the_watchers_cooldown_margin_and_no_return_are_the_groups_to_set() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_set(&host, &["work", "watcher-cooldown-minutes", "30"]);
    result.expect("a cooldown is a count of minutes");
    assert_eq!(group_config(&host, "work").watcher_cooldown_minutes, 30);
    assert!(
        printed.contains("at least"),
        "and what it now means: {printed}"
    );

    let (result, printed) = config_set(&host, &["work", "watcher-margin-percent", "25"]);
    result.expect("a margin is a percentage");
    assert_eq!(group_config(&host, "work").watcher_margin_percent, 25);
    assert!(
        printed.contains("55%"),
        "the margin is only meaningful beside the threshold, so the figure a \
         candidate now has to clear is worked out rather than left to the \
         reader: {printed}"
    );

    let (result, printed) = config_set(&host, &["work", "watcher-no-return", "false"]);
    result.expect("no-return is a yes or a no");
    assert!(!group_config(&host, "work").watcher_no_return);
    assert!(printed.contains("ping-pong"), "{printed}");

    for (key, value) in [
        ("watcher-cooldown-minutes", "30"),
        ("watcher-margin-percent", "25"),
        ("watcher-no-return", "false"),
    ] {
        let (result, printed) = config_get(&host, &["work", key]);
        result.expect("what was set can be read");
        assert_eq!(
            printed.trim(),
            format!("work {key} {value}"),
            "reading `{key}` back"
        );
    }
}

/// The defaults ADR 0013 names, as `perch config get` reports them. A default
/// is a promise, and this is where it is kept.
#[test]
fn a_group_starts_with_the_watcher_policy_the_adr_names() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_get(&host, &["work"]);

    result.expect("naming a Group asks about everything in force for it");
    for expected in [
        "watcher-threshold-percent 80",
        "watcher-cooldown-minutes 15",
        "watcher-margin-percent 10",
        "watcher-no-return true",
    ] {
        assert!(printed.contains(expected), "{expected} — got: {printed}");
    }
    assert!(
        !printed.contains("work watcher"),
        "and every one of them Inherited from Global, said by the line being \
         the two-word `set` that would restore it: {printed}"
    );
}

/// A number the policy cannot hold is refused with the numbers it can, so the
/// script that mistyped one is not left to guess twice.
#[test]
fn a_watcher_number_out_of_range_is_refused_with_the_range_it_accepts() {
    let host = three_accounts_in_one_group();

    for (key, value, accepted) in [
        ("watcher-margin-percent", "101", "100"),
        ("watcher-margin-percent", "-5", "100"),
        ("watcher-margin-percent", "a tenth", "100"),
        ("watcher-cooldown-minutes", "10081", "10080"),
        ("watcher-cooldown-minutes", "-1", "10080"),
        ("watcher-cooldown-minutes", "quarter of an hour", "10080"),
    ] {
        let (result, _) = config_set(&host, &["work", key, value]);

        let error = result.expect_err("out of the range the key accepts");
        assert_eq!(error.exit_code(), EXIT_INVALID, "{error}");
        assert!(
            error.to_string().contains(accepted),
            "the numbers that would have been accepted are named: {error}"
        );
    }

    let config = group_config(&host, "work");
    assert_eq!(config.watcher_margin_percent, 10, "refused, so unchanged");
    assert_eq!(config.watcher_cooldown_minutes, 15, "refused, so unchanged");
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
    assert!(message.contains("watcher-cooldown-minutes"), "{message}");
    assert!(message.contains("watcher-margin-percent"), "{message}");
    assert!(message.contains("watcher-no-return"), "{message}");
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

/// The word somebody types when they mean Global, on all three of the forms
/// that take a Scope. Left to fall through it was answered with "No Group
/// called `global` … Declare it with `perch group add global`" — advice that
/// turns every later `perch config set global …` into a Group Override while
/// Global stays exactly as it was.
#[test]
fn naming_global_as_a_scope_says_how_global_is_addressed_rather_than_offering_a_group() {
    let host = three_accounts_in_one_group();

    for words in [
        vec!["global", "strategy", "soonest-reset"],
        vec!["Global", "watcher-may-act", "true"],
    ] {
        let (result, _) = config_set(&host, &words);

        let error = result.expect_err("Global is not addressed by naming it");
        let message = error.to_string();
        assert!(
            !message.contains("perch group add"),
            "declaring a Group by that name is the one repair that makes this \
             worse: {message}"
        );
        assert!(
            message.contains("perch config set <key> <value>"),
            "the form that does set Global is named: {message}"
        );
    }

    assert_eq!(
        registry_of(&host).global.settings.strategy,
        Strategy::MostHeadroom,
        "and nothing was written"
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
        message.contains("perch config set <scope> <key> <value>"),
        "the form that addresses a Scope is named: {message}"
    );
    assert!(
        message.contains("perch config set <key> <value>"),
        "and so is the form that addresses a setting belonging to none: {message}"
    );
}

/// One word to `get` is either a key belonging to no Group or a Group name.
/// When it is neither, the answer has to be the vocabulary rather than "not
/// found": a mistyped setting read back as an error with no alternatives is a
/// script that goes on believing it asked something meaningful.
#[test]
fn a_get_of_one_word_that_is_neither_a_key_nor_a_group_answers_with_both_vocabularies() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_get(&host, &["stratergy"]);

    let refusal = result.expect_err("`stratergy` is neither");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    let said = refusal.to_string();
    assert!(said.contains("`stratergy` is neither"), "{said}");
    assert!(said.contains("Settings:"), "it names the keys: {said}");
    assert!(
        said.contains("Groups Perch holds: work."),
        "and the Groups: {said}"
    );
}

/// The same refusal before any Group has been declared. Listing the Groups
/// Perch holds would be an empty list, which reads as though the answer were
/// missing rather than as though there were nothing to name.
#[test]
fn the_same_refusal_says_so_plainly_when_no_group_has_been_declared() {
    let host = machine_with_two_accounts();

    let (result, _) = config_get(&host, &["nonsense"]);

    let said = result.expect_err("`nonsense` is neither").to_string();
    assert!(said.contains("No Groups have been declared yet."), "{said}");
    assert!(
        !said.contains("Groups Perch holds:"),
        "there are none to hold: {said}"
    );
}

/// `get` and `set` do not take the same shapes — naming fewer words asks about
/// more rather than being short of a value — so being told the forms of `set`
/// after mis-addressing a `get` would name a form that does not exist.
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
        said.contains("reads Global and every Override there is"),
        "it names the bare form too: {said}"
    );
    assert!(
        !said.contains("perch config set"),
        "the forms of `set` are not the forms of `get`: {said}"
    );
}

/// A count of one is said as one word rather than "1 words".
#[test]
fn a_single_word_is_counted_as_one_word() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_set(&host, &["strategy"]);

    let said = result.expect_err("`set` needs a value").to_string();
    assert!(said.contains("was given 1 word"), "{said}");
    assert!(!said.contains("1 words"), "{said}");
}

/// Two words to `set` name a key and a value — unless the first is a Group, in
/// which case what is missing is the value rather than the meaning. Being told
/// "no such key" there would send somebody looking for a spelling mistake that
/// is not the problem.
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

/// The same shape with a second word that is not a key at all.
///
/// The guard establishes that the *first* word is a Scope and then asserted the
/// second was a key without ever asking. So a mistyped key was answered with
/// "nothing to set it to" — pointing at a value the user had not got to yet, and
/// away from the mistake they had actually made. They add a value, run it again,
/// and only then find out.
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

/// One Scope inheriting is one subject, and the verb has to agree with it.
///
/// `listed_scopes` renders a single Scope as a bare singular, and a machine with
/// one Group is the ordinary case rather than the edge one — so the plural verb
/// was what most people saw: "Group `work` Inherit it".
#[test]
fn one_scope_inheriting_is_said_in_the_singular() {
    let host = three_accounts_in_one_group();
    // The Ungrouped Scope is always one of them, so it takes an Override of its
    // own to leave exactly one Scope following Global.
    config_set(&host, &["ungrouped", "watcher-threshold-percent", "70"])
        .0
        .expect("the Ungrouped Scope Overrides it");

    let (result, printed) = config_set(&host, &["watcher-threshold-percent", "60"]);

    result.expect("Global carries every Setting");
    assert!(
        printed.contains("Group `work` Inherits it, and goes on following Global"),
        "one Scope, one verb to agree with it: {printed}"
    );
}

/// `perch config get` writes nothing, so it does not wait on a writer.
///
/// The same rule `perch status` states for itself and `perch list` follows.
/// Both halves of `perch config` took the write lock, so reading a setting
/// while `perch watch` was between rounds — it takes that lock every round, and
/// `perch status --refresh` holds it across every network read — waited the
/// wait out and then failed with "another `perch` holds it", about a command
/// that only ever reads.
#[test]
fn getting_a_setting_reads_alongside_another_perch_rather_than_waiting_on_it() {
    let host = three_accounts_in_one_group();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, printed) = config_get(&host, &["work", "strategy"]);

    result.expect("a read does not wait on a writer");
    assert_eq!(printed.trim(), "strategy most-headroom", "{printed}");
    drop(held);
}

/// The other half of the same rule: `set` writes, so it does take the lock.
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

/// The round trip above is why a Group name cannot hold a space.
///
/// `perch config get` prints `<group> <key> <value>` and `perch config set`
/// reads it back by counting words, so a Group called `my work` would print a
/// four-word line that `set` answers with "how a `set` is addressed". Refused at
/// the one moment somebody can still choose another name, rather than printing
/// output that cannot be typed back in.
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

/// Global carries the defaults and a Group Overrides them (ADR 0002, amended).
///
/// The whole point of the layer: somebody running several Groups sets a
/// threshold once rather than once per Group, and changing their mind changes
/// it once.
#[test]
fn a_setting_at_global_reaches_every_group_that_has_not_said_otherwise() {
    let host = three_accounts_in_one_group();
    declare_group(&host, "personal");

    let (result, printed) = config_set(&host, &["watcher-threshold-percent", "60"]);

    result.expect("every Setting exists at Global now");
    assert_eq!(group_config(&host, "work").watcher_threshold_percent, 60);
    assert_eq!(
        group_config(&host, "personal").watcher_threshold_percent,
        60
    );
    assert!(
        printed.contains("Inherit it"),
        "and the Scopes it reached are named, because that is the thing that \\
         was bought: {printed}"
    );
}

/// An Override beats Global, and nothing beats an Override.
#[test]
fn a_group_overrides_the_one_setting_it_wants_different_and_inherits_the_rest() {
    let host = three_accounts_in_one_group();
    declare_group(&host, "personal");
    config_set(&host, &["work", "watcher-threshold-percent", "95"])
        .0
        .expect("a Group may Override what it wants different");

    config_set(&host, &["watcher-threshold-percent", "60"])
        .0
        .expect("and Global still moves");

    assert_eq!(
        group_config(&host, "work").watcher_threshold_percent,
        95,
        "the Scope that said otherwise goes on saying it"
    );
    assert_eq!(
        group_config(&host, "personal").watcher_threshold_percent,
        60
    );
    assert_eq!(
        group_config(&host, "work").watcher_cooldown_minutes,
        15,
        "and everything it did not Override still comes from Global"
    );
}

/// Inherit is a state and not an absence: a cleared Override goes back to
/// following Global rather than copying whatever Global happened to say.
#[test]
fn clearing_an_override_makes_the_group_track_global_from_then_on() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "watcher-threshold-percent", "95"])
        .0
        .expect("it takes");

    let (result, printed) = config_unset(&host, &["work", "watcher-threshold-percent"]);

    result.expect("`unset` clears an Override");
    assert!(printed.contains("Inherited from Global"), "{printed}");
    assert_eq!(group_config(&host, "work").watcher_threshold_percent, 80);

    config_set(&host, &["watcher-threshold-percent", "60"])
        .0
        .expect("and Global moves");
    assert_eq!(
        group_config(&host, "work").watcher_threshold_percent,
        60,
        "Inheriting is following rather than copying once"
    );
}

/// Global's values are always set — there is nothing above Global to Inherit
/// from — so clearing there is not a state that exists, and is refused rather
/// than silently accepted.
#[test]
fn unset_at_global_is_refused_and_names_what_it_would_have_meant() {
    let host = three_accounts_in_one_group();

    let (result, _) = config_unset(&host, &["watcher-threshold-percent"]);

    let refusal = result.expect_err("Global has nothing above it");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(said.contains("cannot be unset at Global"), "{said}");
    assert!(
        said.contains("perch config set watcher-threshold-percent <value>"),
        "the thing that does change it is named: {said}"
    );
    assert!(
        said.contains("perch config unset <scope> watcher-threshold-percent"),
        "and so is the form that does exist: {said}"
    );
    assert_eq!(
        registry_of(&host).global.settings.watcher_threshold_percent,
        80,
        "and nothing was written"
    );
}

/// One word to `unset` names Global's key — unless it is a Scope, in which case
/// what is missing is the key rather than the meaning. `set` has said this since
/// it grew the mirror of the guard; `unset` reported a correctly spelled Group
/// name as a Setting Perch does not hold, which sends somebody looking for a
/// spelling mistake that is not the problem.
#[test]
fn an_unset_naming_only_a_scope_says_the_key_is_what_is_missing() {
    let host = three_accounts_in_one_group();

    for word in ["work", "ungrouped"] {
        let refusal = config_unset(&host, &[word])
            .0
            .expect_err("a Scope on its own clears nothing");
        assert_eq!(refusal.exit_code(), EXIT_INVALID);
        let said = refusal.to_string();
        assert!(
            !said.contains("is not a Setting Perch holds"),
            "`{word}` is spelled correctly and is not a key: {said}"
        );
        assert!(said.contains("but no key"), "{said}");
        assert!(
            said.contains("perch config unset <scope> <key>"),
            "and the form that does exist is named: {said}"
        );
    }
}

/// A script can tell an Override from an Inheritance without diffing against
/// Global: the layer a value came from is the number of words that would set it
/// again.
#[test]
fn reading_a_setting_back_says_which_scope_the_value_came_from() {
    let host = three_accounts_in_one_group();
    config_set(&host, &["work", "strategy", "soonest-reset"])
        .0
        .expect("it takes");

    let (_, overridden) = config_get(&host, &["work", "strategy"]);
    let (_, inherited) = config_get(&host, &["work", "watcher-no-return"]);

    assert_eq!(overridden.trim(), "work strategy soonest-reset");
    assert_eq!(inherited.trim(), "watcher-no-return true");
    assert_eq!(
        overridden.split_whitespace().count(),
        3,
        "three words is `perch config set work strategy soonest-reset`, which \\
         is what would restore it — so it is this Group's own"
    );
    assert_eq!(
        inherited.split_whitespace().count(),
        2,
        "and two is `perch config set watcher-no-return true`, which is Global's"
    );
}

/// An Override holding the value Global happens to hold is still an Override:
/// one tracks Global as it changes and the other does not, and the output says
/// which.
#[test]
fn an_override_equal_to_globals_value_still_reads_as_an_override() {
    let host = three_accounts_in_one_group();

    let (result, printed) = config_set(&host, &["work", "watcher-no-return", "true"]);

    result.expect("setting a Setting to what it already resolved to is not a failure");
    assert!(
        printed.contains("Override rather than Inherited"),
        "the difference is the whole of what changed, so it is said: {printed}"
    );
    let (_, read_back) = config_get(&host, &["work", "watcher-no-return"]);
    assert_eq!(read_back.trim(), "work watcher-no-return true");
}

/// The Accounts in no Group are a Scope now (ADR 0017, amended), so Cycling
/// among them reads a Strategy somebody can set rather than one compiled into
/// Perch — and the Scope is addressed the way every other one is.
#[test]
fn the_ungrouped_accounts_are_a_scope_that_can_be_addressed_and_overridden() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_set(&host, &["ungrouped", "strategy", "soonest-reset"]);

    result.expect("`ungrouped` addresses the Accounts in no Group");
    assert!(printed.contains("soonest-reset"), "{printed}");
    assert_eq!(
        registry_of(&host)
            .in_force(&perch::registry::Scope::Ungrouped)
            .strategy,
        Strategy::SoonestReset,
    );

    let (_, read_back) = config_get(&host, &["ungrouped", "strategy"]);
    assert_eq!(read_back.trim(), "ungrouped strategy soonest-reset");
}

/// And where it Overrides nothing, it reads Global — which is what closes the
/// hole this Scope exists to close: before, that Strategy was settable by no
/// key, no Group and no command.
#[test]
fn the_ungrouped_scope_reads_globals_strategy_where_it_says_nothing() {
    let host = machine_with_two_accounts();

    config_set(&host, &["strategy", "soonest-reset"])
        .0
        .expect("Global carries every Setting now");

    assert_eq!(
        registry_of(&host)
            .in_force(&perch::registry::Scope::Ungrouped)
            .strategy,
        Strategy::SoonestReset,
    );
    let (_, read_back) = config_get(&host, &["ungrouped", "strategy"]);
    assert_eq!(
        read_back.trim(),
        "strategy soonest-reset",
        "two words, because Global is what would set it again"
    );
}

/// A Group cannot be called `ungrouped`, because the Scope answers to that name
/// first and the Group would be one no `perch config set` could reach.
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

/// The Ungrouped Scope's Overrides survive a round trip through `get`, like
/// every other Scope's.
#[test]
fn the_ungrouped_scopes_overrides_read_back_in_the_form_that_would_set_them() {
    let host = machine_with_two_accounts();
    config_set(&host, &["ungrouped", "watcher-cooldown-minutes", "45"])
        .0
        .expect("it takes");

    let (result, printed) = config_get(&host, &[]);

    result.expect("naming nothing asks about everything");
    assert!(
        printed.contains("ungrouped watcher-cooldown-minutes 45"),
        "{printed}"
    );
}

/// `perch config get <scope>` on the Ungrouped Scope shows the Setting that
/// governs whether it is Cycled within at all, because that is where it takes
/// effect even though it lives at Global (ADR 0017).
#[test]
fn the_ungrouped_page_shows_the_setting_that_gates_it() {
    let host = machine_with_two_accounts();

    let (result, printed) = config_get(&host, &["ungrouped"]);

    result.expect("`ungrouped` is a Scope");
    assert!(printed.contains("cycle-ungrouped false"), "{printed}");
    assert!(printed.contains("strategy most-headroom"), "{printed}");
}

/// The one key a Scope's page carries without carrying a form of it, asked for
/// both ways round.
///
/// `cycle-ungrouped` is Global's and has no per-Scope form (ADR 0017), but
/// `scope_lines` prints it against the Ungrouped Scope because that is where it
/// takes effect. Two commands disagreed with that page.
///
/// `set` names a Scope and a key with no value, and the guard that says "you are
/// missing the value" checked the key against *Global's* vocabulary — which
/// accepts `cycle-ungrouped`. So it advised the three-word form, and the
/// three-word form refuses, which is the second round trip the guard exists to
/// save.
///
/// `get` printed the line and then refused to read it back by name.
#[test]
fn the_setting_a_scope_shows_and_cannot_override_is_answered_the_way_it_is_shown() {
    let host = three_accounts_in_one_group();

    let refusal = config_set(&host, &["work", "cycle-ungrouped"])
        .0
        .expect_err("a Group cannot carry it at all");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(
        said.contains("per-Scope form"),
        "the refusal is that there is no per-Scope form, said on the first run \
         rather than after a value is added: {said}"
    );
    assert!(
        !said.contains("nothing to set it to"),
        "and it does not send somebody off to type a value that will be \
         refused for a different reason: {said}"
    );

    let (result, printed) = config_get(&host, &["ungrouped", "cycle-ungrouped"]);
    result.expect("the Ungrouped page shows this line, so it can be asked for");
    assert!(
        printed.contains("cycle-ungrouped") && printed.contains("false"),
        "and it reads back what the page prints: {printed}"
    );
}
