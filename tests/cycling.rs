//! Behaviour: what bare `perch switch` picks, and the three ways it honestly
//! picks nothing.
//!
//! This is the command someone types mid-task when quota just ran out, so the
//! tests are as much about what it refuses to do — prompt, leave the Group,
//! land somewhere useless — as about which Account it lands on. Ranking reads
//! the cache and nothing else (ADR 0015), so every fixture here seeds figures
//! rather than arranging replies.

mod common;

use chrono::Duration;
use common::*;
use perch::error::{
    EXIT_NO_CANDIDATE, EXIT_NOT_INTERCHANGEABLE, EXIT_NOTHING_TO_DO, EXIT_PROFILE_LIVE,
};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};

/// Turns on the global setting that says the ungrouped Accounts are
/// interchangeable (ADR 0017), the way a user turns it on.
fn ungrouped_declared_interchangeable(host: &FakeHost) {
    config_set(host, &["cycle-ungrouped", "true"])
        .0
        .expect("the ungrouped Accounts are declared interchangeable");
}

fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active
}

#[test]
fn a_bare_switch_lands_on_the_account_with_the_most_room_in_the_group() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 71.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 18.0)]);

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(active(&host).as_deref(), Some(THIRD_EMAIL), "{printed}");
    assert_eq!(live_credential(&host).as_deref(), Some(THIRD_CREDENTIAL));
    assert!(printed.contains("Group `work`"), "{printed}");
    assert!(
        printed.contains(&format!("Switched to {THIRD_EMAIL}")),
        "{printed}"
    );
}

#[test]
fn a_bare_switch_never_leaves_the_group_it_started_in() {
    let host = three_accounts_in_one_group();
    // The Account with by far the most room is one nobody declared
    // interchangeable with the others — a personal subscription, say.
    move_to_group(&host, THIRD_EMAIL, "none")
        .0
        .expect("it leaves the Group");
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 71.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 1.0)]);

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "a work subscription running dry must not land on a personal Account: {printed}"
    );
}

#[test]
fn naming_a_group_cycles_within_that_group() {
    let host = three_accounts_in_one_group();
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the active Account leaves the Group");
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 71.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 18.0)]);

    let (result, printed) = run_switch(&host, "work");

    result.expect("the Group names somewhere to go");
    assert!(printed.contains("`work` is a Group."), "{printed}");
    assert_eq!(active(&host).as_deref(), Some(THIRD_EMAIL), "{printed}");
}

#[test]
fn ranking_reads_each_accounts_worst_quota_window() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    // Nearly empty on the window you hit first, nearly dead on the weekly one.
    observed(
        &host,
        SECOND_EMAIL,
        vec![window("5-hour", 4.0), window("7-day", 95.0)],
    );
    // Middling on both, and therefore the better landing place: being blocked
    // by any window blocks you completely (ADR 0012).
    observed(
        &host,
        THIRD_EMAIL,
        vec![window("5-hour", 60.0), window("7-day", 55.0)],
    );

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "the five-hour window alone would have chosen the Account about to die \
         on its weekly limit: {printed}"
    );
}

#[test]
fn the_headroom_reported_is_true_of_every_one_of_the_accounts_windows() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(
        &host,
        SECOND_EMAIL,
        vec![window("5-hour", 12.0), window("7-day", 40.0)],
    );

    let (_, printed) = run_cycle(&host);

    assert!(
        printed.contains("60%"),
        "the number said is the worst window's headroom, not the best one's: {printed}"
    );
    assert!(
        printed.contains("7-day"),
        "and the window it came from is named: {printed}"
    );
}

#[test]
fn an_exhausted_account_is_never_chosen() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 90.0)]);
    observed(
        &host,
        SECOND_EMAIL,
        vec![window("5-hour", 2.0), window("7-day", 100.0)],
    );
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 80.0)]);

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "an Account with a full window is blocked whatever its others say: {printed}"
    );
}

#[test]
fn a_disabled_account_is_never_chosen() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 90.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 80.0)]);
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("the Account leaves the pool");

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(active(&host).as_deref(), Some(THIRD_EMAIL), "{printed}");
}

#[test]
fn a_quarantined_account_is_never_chosen() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 90.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 80.0)]);
    quarantine(&host, SECOND_EMAIL);

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert_eq!(active(&host).as_deref(), Some(THIRD_EMAIL), "{printed}");
}

#[test]
fn every_account_exhausted_picks_nothing_and_names_the_one_that_frees_up_soonest() {
    let host = three_accounts_in_one_group();
    observed(
        &host,
        EMAIL,
        vec![resetting("5-hour", 100.0, host.now() + Duration::hours(3))],
    );
    observed(
        &host,
        SECOND_EMAIL,
        vec![resetting("7-day", 100.0, host.now() + Duration::hours(1))],
    );
    observed(
        &host,
        THIRD_EMAIL,
        vec![resetting("5-hour", 100.0, host.now() + Duration::hours(9))],
    );

    let (result, printed) = run_cycle(&host);

    let error = result.expect_err("there is nowhere useful to go");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    assert!(
        error.to_string().contains(SECOND_EMAIL),
        "the Account that frees up soonest is named: {error}"
    );
    assert!(
        error.to_string().contains("13:00"),
        "and when it does: {error}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was switched: {printed}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn an_account_frees_up_when_its_last_full_window_resets_rather_than_its_first() {
    let host = three_accounts_in_one_group();
    // Full on both: the five-hour window comes back first, but the weekly one
    // still blocks it until Saturday.
    observed(
        &host,
        EMAIL,
        vec![
            resetting("5-hour", 100.0, host.now() + Duration::hours(1)),
            resetting("7-day", 100.0, host.now() + Duration::hours(50)),
        ],
    );
    observed(
        &host,
        SECOND_EMAIL,
        vec![resetting("5-hour", 100.0, host.now() + Duration::hours(6))],
    );
    observed(
        &host,
        THIRD_EMAIL,
        vec![resetting("5-hour", 100.0, host.now() + Duration::hours(9))],
    );

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("there is nowhere useful to go");
    assert!(
        error.to_string().contains(SECOND_EMAIL),
        "the Account whose five-hour window resets in an hour is still blocked \
         by its weekly one, so it is not the one that frees up soonest: {error}"
    );
}

#[test]
fn being_already_on_the_best_account_rewrites_no_credentials() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 10.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 71.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 44.0)]);
    host.forget_effects();

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("there is nothing to do");
    assert_eq!(error.exit_code(), EXIT_NOTHING_TO_DO);
    assert!(error.to_string().contains(EMAIL), "{error}");
    // Perch's own registry lock is taken by every command; what must not be
    // taken is one of Claude Code's, because taking those is the Switch.
    assert!(
        !host.effects().iter().any(|effect| matches!(
            effect,
            Effect::Took(dir) if !dir.starts_with("/Users/someone/.perch")
        )),
        "none of Claude Code's locks is taken, and no Credential is rewritten, \
         for a Switch that would change nothing"
    );
}

#[test]
fn a_bare_switch_never_prompts() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 71.0)]);
    observed(&host, THIRD_EMAIL, vec![window("5-hour", 18.0)]);

    let (result, _) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Asked)),
        "the command exists for the moment the user wants no interaction \
         (ADR 0011)"
    );
}

#[test]
fn the_choice_is_reported_as_made_on_cached_figures_that_may_be_stale() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 18.0)]);

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    assert!(
        printed.contains("as of 4m ago"),
        "the figure it ranked on carries its age: {printed}"
    );
    assert!(
        printed.contains("fuller"),
        "landing somewhere fuller than the cache implied is named as staleness \
         before it happens: {printed}"
    );
    assert!(
        printed.contains("--refresh"),
        "and the way to a current figure is named: {printed}"
    );
    assert!(
        host.http_calls().is_empty(),
        "ranking reads the cache and never the network (ADR 0015)"
    );
}

#[test]
fn an_account_nobody_has_read_a_figure_for_is_not_read_as_room() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 60.0)]);
    // Nothing observed for either of the others.

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("nothing known is better than where we are");
    assert_eq!(
        error.exit_code(),
        EXIT_NOTHING_TO_DO,
        "no figure and plenty of room are opposite pieces of advice: {error}"
    );
}

#[test]
fn an_unobserved_account_is_still_somewhere_to_go_when_the_current_one_is_spent() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 100.0)]);

    let (result, printed) = run_cycle(&host);

    result.expect(
        "out of the box nothing has been observed, and the command \
                   the whole tool exists for still has to work",
    );
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{printed}");
    assert!(
        printed.contains("never observed"),
        "the choice says it was made on no evidence: {printed}"
    );
}

#[test]
fn from_an_ungrouped_account_a_bare_switch_switches_nowhere_and_names_both_fixes() {
    let host = machine_with_two_accounts();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("nobody declared these interchangeable");
    assert_eq!(error.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    let message = error.to_string();
    assert!(message.contains("perch group move"), "{message}");
    assert!(message.contains("perch config"), "{message}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was switched"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn with_the_setting_on_a_bare_switch_cycles_among_the_ungrouped_accounts() {
    let host = machine_with_two_accounts();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    ungrouped_declared_interchangeable(&host);

    let (result, printed) = run_cycle(&host);

    result.expect("they have been declared interchangeable now");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL), "{printed}");
}

#[test]
fn the_setting_that_lets_ungrouped_accounts_cycle_is_off_until_it_is_turned_on() {
    let host = machine_with_two_accounts();

    assert!(
        !registry_of(&host).global.cycle_ungrouped,
        "being ungrouped is the absence of a declaration that Accounts are \
         interchangeable, not a weaker form of one (ADR 0017)"
    );
}

#[test]
fn a_group_with_nobody_left_to_cycle_to_says_so_rather_than_switching() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 100.0)]);
    for reserved in [SECOND_EMAIL, THIRD_EMAIL] {
        disable_account(&host, reserved)
            .0
            .expect("the Account leaves the pool");
    }

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("every other Account is out of the pool");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    assert!(error.to_string().contains("work"), "{error}");
}

#[test]
fn naming_a_group_that_holds_no_accounts_switches_nowhere() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");

    let (result, _) = run_switch(&host, "work");

    let error = result.expect_err("there is nobody in it");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    assert!(error.to_string().contains("holds no Accounts"), "{error}");
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_cycle_onto_a_live_profile_is_refused_like_any_other_switch() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 18.0)]);
    let profile = "/Users/someone/.perch/profiles/overflow-example-com";
    let marker = format!(
        r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    let host = host
        .with_file(format!("{profile}/sessions/4242.json"), &marker)
        .with_live_process(4242);

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("something else is holding that Credential");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
}
