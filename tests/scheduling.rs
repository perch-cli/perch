//! Behavior: `perch watcher check` — one check, and what it reports to whatever
//! scheduled it.
//!
//! What a Check owes cron is a decision line it can capture and an exit code it can
//! branch on — nothing else, and nothing left running.
//!
//! Two properties, each with tests that fail if it stops holding: the policy is the
//! loop's policy run once (ADR a-watcher-knob-is-arithmetic), and the cooldown that
//! paces it survives between invocations, because every one of them is a fresh process.

mod common;

use chrono::Duration;
use common::*;
use perch::anthropic::{PROFILE_URL, USAGE_URL};
use perch::error::{
    EXIT_HELD, EXIT_INVALID, EXIT_KEYCHAIN_UNAVAILABLE, EXIT_NO_CANDIDATE, EXIT_NOT_FOUND,
    EXIT_NOT_INTERCHANGEABLE, EXIT_NOTHING_TO_DO, EXIT_OK,
};
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{FakeHost, Refusing};

/// A machine where a check finds the active Account following `here` and the Account it
/// could move to following `there` — one figure per reading, the last of them for every
/// reading after the trace runs out. Nobody interrupts it, because nothing here waits.
fn checked(here: &[f64], there: &[f64]) -> FakeHost {
    let host = answering(watched(), ACTIVE_TOKEN, EMAIL, here);
    answering(host, SPARE_TOKEN, SECOND_EMAIL, there)
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active().whose().map(str::to_string)
}

/// Everything the check waited for, which is what a scheduled command has no business
/// doing any of.
fn waits(host: &FakeHost) -> Vec<u64> {
    host.effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(*millis),
            _ => None,
        })
        .collect()
}

/// The one line a check prints, which is the whole of what cron captures.
fn decision(printed: &str) -> String {
    let decisions = decisions(printed);
    assert_eq!(
        decisions.len(),
        1,
        "a check decides once and says so once: {printed}"
    );
    decisions.into_iter().next().expect("the one decision")
}

#[test]
fn one_check_switches_and_reports_it_in_the_exit_code() {
    let host = checked(&[86.0], &[5.0]);

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a check that switched is not a failure"),
        EXIT_OK
    );
    let decision = decision(&printed);
    assert!(decision.contains("switched"), "{decision}");
    assert!(decision.contains("86% used"), "what it read: {decision}");
    assert!(
        decision.contains(SECOND_EMAIL),
        "and where it went: {decision}"
    );
    assert!(
        !decision.contains("threshold") && !decision.contains("most room"),
        "and nothing a scheduler could have read off the guide (ADR perch-says-what-it-did): \
         {decision}"
    );
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
    assert!(
        waits(&host).is_empty(),
        "it exits rather than waiting for anything: scheduling is the \
         operating system's job"
    );
    assert!(
        !printed.contains("Ctrl-C"),
        "nothing here is stopped by hand: {printed}"
    );
}

#[test]
fn a_check_under_the_threshold_exits_fifteen_and_changes_nothing() {
    let host = checked(&[40.0], &[5.0]);

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("nothing to do is not a failure"),
        EXIT_NOTHING_TO_DO
    );
    let decision = decision(&printed);
    assert!(decision.contains("waiting"), "{decision}");
    assert!(decision.contains("40% used"), "{decision}");
    assert_eq!(
        decision, "2026-08-04T12:00:00Z  waiting   40% used, fullest 5-hour",
        "and the whole of it, because a round that did what it was asked to do \
         has nothing to explain"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        1,
        "and only the Account it is on was read: {printed}"
    );
}

#[test]
fn a_check_with_nowhere_to_go_exits_seventeen() {
    let host = checked(&[100.0], &[100.0]);

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a dead end is a decision rather than a failure"),
        EXIT_NO_CANDIDATE
    );
    let decision = decision(&printed);
    assert!(decision.contains("nowhere"), "{decision}");
    assert!(decision.contains("exhausted"), "and why: {decision}");
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_check_that_could_not_read_a_current_figure_exits_twenty() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_reply(USAGE_URL, 429, "{}");
    // A cached figure well over the threshold: acting on it would switch, and acting on
    // it is the whole of what the watcher refuses to do.
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch_once(&host);

    assert_eq!(result.expect("a held decision is not a failure"), EXIT_HELD);
    let decision = decision(&printed);
    assert!(decision.contains("held"), "{decision}");
    assert!(
        decision.contains("unread"),
        "the cached 95% is not quoted as the figure it decided on: {decision}"
    );
    assert!(decision.contains("rate-limiting"), "and why: {decision}");
    assert!(
        !decision.contains("Asking again"),
        "when to ask again belongs to whatever scheduled this: {decision}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing was switched on a figure Perch already had"
    );
}

#[test]
fn a_check_on_a_quarantined_account_holds_and_names_the_repair() {
    let host = checked(&[86.0], &[5.0]);
    quarantine(&host, EMAIL);

    let (result, printed) = run_watch_once(&host);

    assert_eq!(result.expect("a held decision is not a failure"), EXIT_HELD);
    let decision = decision(&printed);
    assert!(decision.contains("held"), "{decision}");
    assert!(
        decision.contains("perch relogin"),
        "and the line says what repairs it: {decision}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_check_on_an_ungrouped_account_exits_eighteen_and_names_both_declarations() {
    let host = checked(&[40.0], &[5.0]);
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the Account leaves the Group");

    let (result, _) = run_watch_once(&host);

    let refusal = result.expect_err("there is nowhere here it may act");
    assert_eq!(refusal.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    let said = refusal.to_string();
    assert!(said.contains("interchangeable"), "{said}");
    assert!(said.contains("watcher-may-act"), "{said}");
    assert!(said.contains("perch group move"), "{said}");
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and it read nothing on the way to deciding it may do nothing"
    );
}

#[test]
fn a_grant_said_about_a_group_leaves_the_ungrouped_accounts_alone() {
    let host = checked(&[99.0], &[1.0]);
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("a statement about the Group this person runs");
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the Account leaves the Group");
    move_to_group(&host, SECOND_EMAIL, "none")
        .0
        .expect("and so does the other");

    let (result, _) = run_watch_once(&host);

    let refusal = result.expect_err("that yes was not about these Accounts");
    assert_eq!(refusal.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    assert!(
        refusal.to_string().contains("interchangeable"),
        "the declaration that has to come first is named: {refusal}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing moved underneath them, at 99% used and an empty Account \
         beside it (ADR a-group-is-a-declaration)"
    );
    assert!(host.sent_to(USAGE_URL).is_empty());
}

#[test]
fn a_check_on_ungrouped_accounts_declared_interchangeable_still_needs_the_watcher_let_in() {
    let host = checked(&[99.0], &[1.0]);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none").0.expect("it leaves");
    }
    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("ungrouped Accounts are interchangeable");

    let (result, _) = run_watch_once(&host);

    let refusal = result.expect_err("nothing has let the watcher in");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal
            .to_string()
            .contains("perch config set ungrouped watcher-may-act true"),
        "and the Scope is addressed the way every other one is: {refusal}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_check_switches_among_ungrouped_accounts_once_both_declarations_are_made() {
    let host = checked(&[99.0], &[1.0]);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none").0.expect("it leaves");
    }
    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("they are interchangeable");
    config_set(&host, &["ungrouped", "watcher-may-act", "true"])
        .0
        .expect("and the watcher may act on them");

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a check that switched is not a failure"),
        EXIT_OK
    );
    assert!(decision(&printed).contains("switched"), "{printed}");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
}

#[test]
fn a_check_among_ungrouped_accounts_paces_the_next_one() {
    // The trace that would move every round if nothing paced it across invocations.
    let host = checked(&[99.0, 20.0], &[1.0, 90.0]);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none").0.expect("it leaves");
    }
    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .unwrap();
    config_set(&host, &["ungrouped", "watcher-may-act", "true"])
        .0
        .unwrap();

    run_watch_once(&host).0.expect("the first one switches");
    host.set_now(host.now() + Duration::minutes(5));
    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("cooling is a decision rather than a failure"),
        EXIT_NOTHING_TO_DO
    );
    assert!(decision(&printed).contains("cooling"), "{printed}");
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
}

#[test]
fn a_check_on_a_group_that_has_not_said_the_watcher_may_act_names_the_setting() {
    let host = checked(&[40.0], &[5.0]);
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");

    let (result, _) = run_watch_once(&host);

    let refusal = result.expect_err("nothing may be acted on");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(refusal.to_string().contains("watcher-may-act"), "{refusal}");
    assert!(host.sent_to(USAGE_URL).is_empty(), "and nothing was read");
}

#[test]
fn the_cooldown_holds_between_one_check_and_the_next() {
    // The trace that would move every round if nothing paced it across invocations.
    let host = checked(&[86.0, 20.0], &[5.0, 90.0]);

    let (first, _) = run_watch_once(&host);
    assert_eq!(first.expect("it switched"), EXIT_OK);
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));

    // Cron comes back five minutes later, well inside the fifteen.
    host.set_now(host.now() + Duration::minutes(5));
    let (second, printed) = run_watch_once(&host);

    assert_eq!(
        second.expect("too soon to move is a decision, not a failure"),
        EXIT_NOTHING_TO_DO,
        "there was nothing to do now, which is what 15 says: {printed}"
    );
    let decision = decision(&printed);
    assert!(decision.contains("cooling"), "{decision}");
    assert!(decision.contains("90% used"), "what it read: {decision}");
    assert!(
        decision.contains("cooldown"),
        "and the rule that held it: {decision}"
    );
    assert!(decision.contains("15 minutes"), "{decision}");
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "and nothing moved: {printed}"
    );

    // And once the cooldown has run out, the same figures move it.
    host.set_now(host.now() + Duration::minutes(11));
    let (third, printed) = run_watch_once(&host);

    assert_eq!(third.expect("it switched"), EXIT_OK);
    assert!(printed.contains("switched"), "{printed}");
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_check_records_when_it_switched() {
    // Under the threshold, and then over it: two checks, of which only the second has
    // anything to record.
    let host = checked(&[40.0, 86.0], &[5.0]);
    let switched_at = host.now();

    run_watch_once(&host).0.expect("nothing to do");
    assert!(
        registry_of(&host).checks.is_empty(),
        "a check that changed nothing paces nothing"
    );

    host.set_now(switched_at + Duration::minutes(30));
    run_watch_once(&host).0.expect("it switched");

    let checks = registry_of(&host).checks;
    let recorded = checks.get("work").expect("the Group it Switched within");
    assert_eq!(recorded.switched_at, switched_at + Duration::minutes(30));
}

#[test]
fn a_switch_the_machine_turned_away_exits_fifteen_and_says_so() {
    let host = checked(&[86.0], &[5.0]);
    // A `perch run` against the Account being left: the Capture would write into a
    // Profile something else is holding (ADR a-profile-is-live-by-evidence).
    a_run_against(&host, EMAIL, host.now());

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a refused Switch is a decision rather than a failure"),
        EXIT_NOTHING_TO_DO
    );
    let decision = decision(&printed);
    assert!(decision.contains("refused"), "{decision}");
    assert!(decision.contains("86% used"), "{decision}");
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing was changed"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(ACTIVE),
        "least of all the Profile the client is holding"
    );
}

#[test]
fn a_check_stopped_by_something_that_will_not_clear_itself_exits_on_it() {
    // The Capture writes the live Credential back into the outgoing Account's Profile,
    // and neither of that Profile's two stores will take it (ADR
    // claude-code-chooses-the-store).
    let host = checked(&[86.0], &[5.0]);
    let plaintext = store_of(&host, EMAIL).credentials_file;
    let host = host
        .with_keychain_truncating_after(20)
        .with_a_path_refusing(&plaintext, Refusing::Write, "no space left on device");

    let (result, printed) = run_watch_once(&host);

    let failed = result.expect_err("a store that will not hold a Credential is a failure");
    assert_eq!(
        failed.exit_code(),
        EXIT_KEYCHAIN_UNAVAILABLE,
        "the code the failure earned, not the one that means come back in five \
         minutes: {failed}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL), "and nothing moved");
    assert!(
        !printed.contains("refused"),
        "a decision line would say the check had decided something: {printed}"
    );
}

#[test]
fn a_check_with_nobody_active_says_there_is_nothing_to_watch() {
    let host = checked(&[10.0], &[10.0]);
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let (result, printed) = run_watch_once(&host);

    let refused = result.expect_err("there is no Account to watch");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    assert!(
        refused.to_string().contains("nothing to watch"),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("perch switch"),
        "and it names what makes one active: {refused}"
    );
    assert_eq!(printed, "", "nothing is decided, so nothing is logged");
}

#[test]
fn figures_that_were_read_but_could_not_be_kept_are_said_rather_than_swallowed() {
    let host =
        checked(&[95.0], &[10.0]).with_a_path_refusing(REGISTRY_PATH, Refusing::Write, "read-only");

    let (result, printed) = run_watch_once(&host);

    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("could not write them to its own record")),
        "the degradation is said rather than swallowed: {:?}\n{printed}",
        host.notes()
    );
    let failed = result.expect_err("a record that will not take the Switch stops the check");
    // Rendered as the Host built it rather than as the constant spells it: the registry
    // is reached by joining, so the separators are the platform's.
    let registry = perch::holdings::registry_path(&host).expect("home is known");
    assert!(
        failed.to_string().contains(&registry.display().to_string()),
        "and what could not be written is named: {failed}"
    );
}

#[test]
fn a_check_that_switched_and_then_failed_still_paces_the_next_one() {
    let host = checked(&[86.0], &[5.0]);
    // The Credential is written and the Identity patch is what fails, so the machine is
    // part way through a Switch rather than untouched.
    let host = host.with_a_path_refusing(IDENTITY_PATH, Refusing::Write, "read-only file");
    let switched_at = host.now();

    let (result, _) = run_watch_once(&host);

    result.expect_err("the Switch went live and then could not be finished");
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "the Credential moved, so which Account is active moved with it"
    );

    let checks = registry_of(&host).checks;
    let recorded = checks
        .get("work")
        .expect("a check that moved records that it moved");
    assert_eq!(recorded.switched_at, switched_at);
}
