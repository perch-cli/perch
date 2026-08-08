//! Behaviour: `perch watch --once` — one check, and what it reports to
//! whatever scheduled it.
//!
//! The loop is a person watching; this is a machine watching, and the
//! difference is entirely in how the answer is delivered. Scheduling is the
//! operating system's job (ADR 0013), so what `--once` owes cron is a decision
//! line it can capture and an exit code it can branch on — nothing else, and
//! nothing left running.
//!
//! Two properties are what this mode is for, and each has tests here that fail
//! if it stops holding: the policy is the loop's policy run once, and the
//! cooldown and no-return that pace it survive between invocations, because
//! every one of them is a fresh process.

mod common;

use chrono::Duration;
use common::*;
use perch::anthropic::{PROFILE_URL, USAGE_URL};
use perch::error::{
    EXIT_HELD, EXIT_INVALID, EXIT_NO_CANDIDATE, EXIT_NOT_FOUND, EXIT_NOT_INTERCHANGEABLE,
    EXIT_NOTHING_TO_DO, EXIT_OK,
};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};

/// A machine where a check finds the active Account following `here` and the
/// Account it could move to following `there` — one figure per reading, the
/// last of them for every reading after the trace runs out.
///
/// Nobody interrupts it, because nothing here waits: a check that had to be
/// stopped would be a loop.
fn checked(here: &[f64], there: &[f64]) -> FakeHost {
    let host = answering(watched(), ACTIVE_TOKEN, EMAIL, here);
    answering(host, SPARE_TOKEN, SECOND_EMAIL, there)
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active
}

/// Everything the check waited for, which is what a scheduled command has no
/// business doing any of.
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

/// The whole of what `--once` is: one check, one line, one exit code, and
/// nothing left running.
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
    assert!(decision.contains("threshold 80%"), "{decision}");
    assert!(
        decision.contains(SECOND_EMAIL),
        "and where it went: {decision}"
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

/// The ordinary answer, and the one a scheduler gets most of the time: the
/// Account is not full enough to want moving off, so there was nothing to do.
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
    assert!(decision.contains("under it"), "and why: {decision}");
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        1,
        "and only the Account it is on was read: {printed}"
    );
}

/// A Switch was wanted and every candidate was exhausted. Nothing to retry and
/// nothing to fix: the answer is to wait, and under cron waiting is the next
/// invocation.
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

/// The one code that is new, and the reason it had to be: a scheduler retrying
/// in five minutes needs to tell a stale figure from a Group with nowhere to
/// go, because only one of the two resolves itself.
#[test]
fn a_check_that_could_not_read_a_current_figure_exits_twenty() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_reply(USAGE_URL, 429, "{}");
    // A cached figure well over the threshold: acting on it would switch, and
    // acting on it is the whole of what the watcher refuses to do.
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

/// An Account whose Credential has stopped working is a figure that cannot be
/// read, so a check holds on it like any other — `19` says more, and it is not
/// in the table `--once` promises, so the Quarantine and the repair go on the
/// line instead. A scheduler could do nothing with the difference either way.
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

/// The permission the watcher needs is a Group's to give, and an ungrouped
/// Account has no Group to give it. A check behaves exactly as the loop does,
/// including saying both ways to fix it.
#[test]
fn a_check_on_an_ungrouped_account_exits_eighteen_and_names_both_fixes() {
    let host = checked(&[40.0], &[5.0]);
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the Account leaves the Group");
    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("ungrouped Accounts are interchangeable");

    let (result, _) = run_watch_once(&host);

    let refusal = result.expect_err("there is nothing here it may act on");
    assert_eq!(refusal.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    assert!(
        refusal.to_string().contains("perch group move"),
        "{refusal}"
    );
    assert!(refusal.to_string().contains("watcher-may-act"), "{refusal}");
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and it read nothing on the way to deciding it may do nothing"
    );
}

/// The other half of the same rule: a Group that has not said the watcher may
/// act on it is not acted on, however it is invoked.
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

/// The property that makes `--once` a watcher rather than a Switch on a timer:
/// every invocation is a fresh process, so the cooldown has to be written down
/// somewhere it survives one.
#[test]
fn the_cooldown_holds_between_one_check_and_the_next() {
    // The Account being watched fills up, the check moves off it, and the one
    // it moved to fills up in turn — the trace that would ping-pong if nothing
    // paced it across invocations.
    let host = checked(&[86.0, 20.0], &[5.0, 90.0]);

    let (first, _) = run_watch_once(&host);
    assert_eq!(first.expect("it switched"), EXIT_OK);
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));

    // Cron comes back five minutes later, well inside the Group's fifteen.
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

/// What one check leaves for the next: when it Switched, and off which Account
/// — the two things the cooldown and the no-return are measured from. Only a
/// Switch that happened writes one, because a check that changed nothing has
/// nothing to pace.
#[test]
fn a_check_records_when_it_switched_and_what_it_switched_off() {
    // Under the threshold, and then over it: two checks, of which only the
    // second has anything to record.
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
    assert_eq!(recorded.switched_off, EMAIL);
    assert_eq!(recorded.switched_at, switched_at + Duration::minutes(30));
}

/// A Switch that was turned away changed nothing and clears itself when
/// whatever is holding the Profile exits — so it is the same answer as a
/// cooldown to a scheduler: nothing to do now, and the line says what.
#[test]
fn a_switch_the_machine_turned_away_exits_fifteen_and_says_so() {
    let host = checked(&[86.0], &[5.0]);
    // A `perch run` against the Account being left: the Capture would write
    // into a Profile something else is holding (ADR 0027).
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

/// Permission is read on every check rather than once, and the Account it is
/// read about is the active one — so a machine that records nobody active has
/// no check to make. Saying which command makes one active matters here more
/// than elsewhere: whatever scheduled this is not watching the output.
#[test]
fn a_check_with_nobody_active_says_there_is_nothing_to_watch() {
    let host = checked(&[10.0], &[10.0]);
    let mut registry = registry_of(&host);
    registry.active = None;
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

/// A check reads a figure and writes it down, and the write can fail on its
/// own. What the round decides is still made on the figure it just read — that
/// one is current whatever happened to the record — and the user is told, once,
/// that the next command will show the older figure (ADR 0018).
#[test]
fn figures_that_were_read_but_could_not_be_kept_are_said_rather_than_swallowed() {
    let host = checked(&[95.0], &[10.0]).with_unwritable_file(REGISTRY_PATH, "read-only");

    let (result, printed) = run_watch_once(&host);

    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("could not write them to its own record")),
        "the degradation is said rather than swallowed: {:?}\n{printed}",
        host.notes()
    );
    let failed = result.expect_err("a record that will not take the Switch stops the check");
    // Rendered as the Host built it rather than as the constant spells it: the
    // registry is reached by joining, so the separators are the platform's.
    let registry = perch::registry::registry_path(&host).expect("home is known");
    assert!(
        failed.to_string().contains(&registry.display().to_string()),
        "and what could not be written is named: {failed}"
    );
}
