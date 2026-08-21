//! Behavior: what bare `perch switch` picks, and the three ways it honestly
//! picks nothing.
//!
//! Ranking reads the cache and nothing else (ADR a-figure-carries-its-age), so
//! every fixture here seeds figures rather than arranging replies.

mod common;

use chrono::Duration;
use common::*;
use perch::error::{
    EXIT_NO_CANDIDATE, EXIT_NOT_FOUND, EXIT_NOT_INTERCHANGEABLE, EXIT_NOTHING_TO_DO,
    EXIT_PROFILE_LIVE,
};
use perch::host::FakeHost;
use perch::host::fake::Effect;
use perch::host::prelude::*;

/// Declares the ungrouped Accounts interchangeable
/// (ADR a-group-is-a-declaration), the way a person declares it.
fn ungrouped_declared_interchangeable(host: &FakeHost) {
    config_set(host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("the ungrouped Accounts are declared interchangeable");
}

fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active().whose().map(str::to_string)
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
    assert!(
        printed.contains(&format!(
            "Switched to {THIRD_EMAIL}, the most room in Group `work`."
        )),
        "the landing line names the Account, what it was chosen on, and the \
         Group the Cycle stayed inside: {printed}"
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
    // by any window blocks you completely (ADR headroom-is-the-worst-window).
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

/// The specimen ADR perch-says-what-it-did is written around: three lines, each
/// of them something that happened. Counted as well as read, because "and
/// nothing else" is the claim, and a `contains` per line passes just as happily
/// on a rationale sitting between them.
#[test]
fn a_bare_switch_says_where_it_landed_and_what_it_bought_and_nothing_else() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(
        &host,
        SECOND_EMAIL,
        vec![window("5-hour", 12.0), window("7-day", 40.0)],
    );

    let (result, printed) = run_cycle(&host);

    result.expect("there is somewhere to go");
    let said: Vec<&str> = printed.lines().collect();
    assert_eq!(
        said.len(),
        3,
        "a landing line and one line per Quota Window is the whole of it: \
         {printed}"
    );
    assert_eq!(
        said[0],
        format!("Switched to {SECOND_EMAIL}, the most room in Group `work`."),
        "which names the Account and the Group the Cycle stayed inside: {printed}"
    );
    for (window, used) in [("5-hour", "12%"), ("7-day", "40%")] {
        assert!(
            said[1..]
                .iter()
                .any(|line| line.contains(window) && line.contains(used)),
            "and every window of the Account it landed on reaches the page: {printed}"
        );
    }
    assert_eq!(
        printed.matches("as of 4m ago").count(),
        2,
        "each carrying its own age: {printed}"
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
fn a_reset_that_has_already_gone_by_is_not_what_frees_up_soonest() {
    let host = three_accounts_in_one_group();
    // Read six hours ago, and its window came back five hours ago.
    observed(
        &host,
        EMAIL,
        vec![resetting("5-hour", 100.0, host.now() - Duration::hours(5))],
    );
    // The only one whose reset is still ahead of it, so the only one that can
    // answer "when does any of this come back?".
    observed(
        &host,
        SECOND_EMAIL,
        vec![resetting("7-day", 100.0, host.now() + Duration::hours(2))],
    );
    observed(
        &host,
        THIRD_EMAIL,
        vec![resetting("5-hour", 100.0, host.now() - Duration::hours(1))],
    );

    let (result, printed) = run_cycle(&host);

    let error = result.expect_err("every cached figure says exhausted");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    let said = error.to_string();
    assert!(
        said.contains(SECOND_EMAIL),
        "the one whose reset is still ahead is what frees up soonest: {said}"
    );
    assert!(
        !said.contains("any moment now"),
        "and nothing is announced as arriving any moment on the strength of a \
         window that came back hours ago: {said}"
    );
    assert!(
        said.contains("2 of them cache no reset time"),
        "the two elapsed ones say as little about the wait as no reset at all, \
         so the wait may be shorter than the one figure that can still speak: \
         {said}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL), "{printed}");
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
            Effect::Took(dir) if !dir.starts_with("/Users/someone/.config/perch")
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
         (ADR perch-does-not-draw)"
    );
}

/// The age is the whole of the promise Perch makes about a cached figure, and it
/// is made where the figure is rather than in a paragraph underneath it.
#[test]
fn the_figures_a_choice_was_made_on_are_dated_and_never_read_from_the_network() {
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
        !printed.contains("--refresh"),
        "and a Switch that worked does not send anybody to another command: \
         {printed}"
    );
    assert!(
        host.http_calls().is_empty(),
        "ranking reads the cache and never the network"
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
        printed.contains(&format!(
            "Switched to {SECOND_EMAIL}, nothing observed to rank on in Group `work`."
        )),
        "the landing line says it was made on no evidence rather than naming a \
         basis it did not have: {printed}"
    );
    assert!(
        printed.contains("never observed"),
        "and the figures under it say the same thing in figures: {printed}"
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
        !registry_of(&host).ungrouped.interchangeable,
        "being ungrouped is the absence of a declaration that Accounts are \
         interchangeable, not a weaker form of one"
    );
}

#[test]
fn a_group_with_nobody_left_to_cycle_to_says_so_rather_than_switching() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 100.0)]);
    for held_back in [SECOND_EMAIL, THIRD_EMAIL] {
        disable_account(&host, held_back)
            .0
            .expect("the Account leaves the pool");
    }

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("every other Account is out of the pool");
    assert_eq!(error.exit_code(), EXIT_NO_CANDIDATE);
    assert!(error.to_string().contains("work"), "{error}");

    // Two Accounts with full Headroom are sitting in that Group, and the fix is
    // `perch enable` rather than waiting for a quota reset.
    let said = error.to_string();
    assert!(
        said.contains("2 disabled"),
        "it says which way the others left the running: {said}"
    );
    assert!(
        said.contains("Cycling may choose"),
        "and does not claim the whole Group is exhausted: {said}"
    );
}

/// Where every Account really is a candidate, the sentence stays the plain one:
/// nothing is out of the running, so nothing is said about anything being.
#[test]
fn a_group_where_every_account_is_exhausted_says_only_that() {
    let host = three_accounts_in_one_group();
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        observed(&host, email, vec![window("5-hour", 100.0)]);
    }

    let (result, _) = run_cycle(&host);

    let said = result.expect_err("there is nowhere with room").to_string();
    assert!(said.contains("is exhausted"), "{said}");
    assert!(
        !said.contains("out of the running"),
        "nothing was set aside, so nothing is said about it: {said}"
    );
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

/// A Cycle is a Switch that chooses for you, so it inherits the rule about
/// Live Profiles whole: the Account it lands on is only ever read from, and an
/// Account you are already running in one terminal is exactly the one you would
/// want active in the others (ADR a-profile-is-live-by-evidence).
#[test]
fn a_cycle_lands_on_a_live_account_like_any_other() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 18.0)]);
    let profile = "/Users/someone/.config/perch/profiles/overflow-example-com";
    let marker = format!(
        r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    let host = host
        .with_file(format!("{profile}/sessions/4242.json"), &marker)
        .with_live_process(4242);

    run_cycle(&host).0.expect("a Run does not close an Account");

    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
    assert_eq!(live_credential(&host).as_deref(), Some(SECOND_CREDENTIAL));
}

/// The other direction is still refused: the Capture writes the live Credential
/// into the outgoing Account's own Profile, and a client running there is
/// holding that file.
#[test]
fn a_cycle_away_from_a_live_profile_is_refused() {
    let host = three_accounts_in_one_group();
    observed(&host, EMAIL, vec![window("5-hour", 96.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 18.0)]);
    let profile = "/Users/someone/.config/perch/profiles/someone-example-com";
    let marker = format!(
        r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    let host = host
        .with_file(format!("{profile}/sessions/4242.json"), &marker)
        .with_live_process(4242);

    let (result, _) = run_cycle(&host);

    let error = result.expect_err("the Capture would write under that client");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

/// A bare Cycle asks the Group of the Account it is leaving where it may look,
/// so a Perch that records nobody as active has no question to answer rather
/// than an empty one. The refusal names the way out, which is not a Perch
/// command: nothing here is repaired by adding an Account, only by there being
/// a login to adopt in the first place.
#[test]
fn a_bare_switch_with_nobody_active_says_there_is_no_group_to_cycle_within() {
    let host = three_accounts_in_one_group();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let written_before = credentials_written(&host);

    let (result, printed) = run_cycle(&host);

    let refused = result.expect_err("there is no Account to cycle away from");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert!(
        refused.to_string().contains("no Group to Cycle within"),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("log in"),
        "and it names the way out: {refused}"
    );
    assert_eq!(
        credentials_written(&host),
        written_before,
        "nothing is rewritten for a Cycle that never had a starting point: {printed}"
    );
}

/// How many Credentials have been written to the live store so far. Counted
/// rather than asserted absent, because the fixtures reach this point by adding
/// Accounts, and adding one writes.
fn credentials_written(host: &FakeHost) -> usize {
    host.effects()
        .iter()
        .filter(|effect| matches!(effect, Effect::KeychainSet { .. }))
        .count()
}

/// The Accounts in no Group are a Scope, so Cycling among them reads a Strategy
/// somebody can set rather than a constant compiled into Perch.
#[test]
fn cycling_among_ungrouped_accounts_reads_the_strategy_that_scope_holds() {
    let host = machine_with_three_accounts();
    ungrouped_declared_interchangeable(&host);
    // Where the two Strategies disagree: the emptiest Account is not the one
    // whose quota is about to be thrown away.
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

    let (result, printed) = run_cycle(&host);
    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(THIRD_EMAIL),
        "the compiled-in default is still the most room left: {printed}"
    );

    config_set(&host, &["ungrouped", "strategy", "soonest-reset"])
        .0
        .expect("`ungrouped` addresses the Scope, like every other one");
    run_switch(&host, EMAIL).0.expect("back to the full one");

    let (result, printed) = run_cycle(&host);
    result.expect("there is somewhere to go");
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "and the Scope Cycles by what it was told: {printed}"
    );
}
