//! Behavior: `perch watcher run` — the loop, the Refresh, and the decision log.
//!
//! A simulated Utilization trace driven through the real loop against the fake Host:
//! the figure the endpoint answers with moves from one Refresh to the next, and what
//! the loop does about it is asserted from the outside.
//!
//! Four properties, each with a test that fails if it stops holding: only the Account
//! you are on is Refreshed, every decision is printed, nothing is acted on but a figure
//! that was just read, and no Switch lands on an Account nearly as full as the one
//! being left (ADR a-watcher-knob-is-arithmetic).

// Every path compared here comes out of the fake's effect log, spelled as the
// code under test wrote it: filtering that log by prefix asks which effects
// landed under a directory, and never whether a path on a machine is inside one.
#![allow(
    clippy::disallowed_methods,
    reason = "the fake's effect log, filtered by the prefix it was written under"
)]

mod common;

use chrono::{DateTime, Duration, Utc};
use common::*;
use perch::anthropic::{PROFILE_URL, TOKEN_URL, USAGE_URL};
use perch::commands::add::AddArgs;
use perch::error::{EXIT_INVALID, EXIT_NOT_INTERCHANGEABLE};
use perch::host::FakeHost;
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::prelude::*;
use perch::watch::REFRESH_INTERVAL_MILLIS;

/// The Credential of an Account whose access token ran out twenty minutes ago, so
/// reading it at all means Renewing it first.
const SPENT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1785843600000,"subscriptionType":"pro"}}"#;

/// The Default Profile's config directory, which is where a client running as the
/// active Account holds its session.
const DEFAULT_CONFIG_DIR: &str = "/Users/someone/.claude";

/// Perch's own lock over its registry — the one artifact a loop could leave behind if
/// it held anything across a wait.
const REGISTRY_LOCK: &str = "/Users/someone/.config/perch/.registry.lock";

/// A machine where the active Account's figure follows `trace` and the Account it would
/// Cycle to sits at `spare`, watched for as many rounds as the trace is long.
fn watching(trace: &[f64], spare: f64) -> FakeHost {
    let host = answering(watched(), ACTIVE_TOKEN, EMAIL, trace);
    let host = answering(host, SPARE_TOKEN, SECOND_EMAIL, &[spare]);
    host.with_interrupt_after(trace.len() as u32)
}

/// The same, where both Accounts have a trace of their own — for the tests about moving
/// back and forth, where the Account that was the candidate becomes the one being
/// watched.
fn watching_both(here: &[f64], there: &[f64], rounds: u32) -> FakeHost {
    let host = answering(watched(), ACTIVE_TOKEN, EMAIL, here);
    let host = answering(host, SPARE_TOKEN, SECOND_EMAIL, there);
    host.with_interrupt_after(rounds)
}

/// When a decision was taken, off the front of the line it was printed on.
fn when(decision: &str) -> DateTime<Utc> {
    let stamp = decision.split_whitespace().next().expect("a line");
    DateTime::parse_from_rfc3339(stamp)
        .unwrap_or_else(|_| panic!("every decision line opens with its time: {decision}"))
        .with_timezone(&Utc)
}

#[test]
fn a_watcher_on_a_machine_with_no_login_holds_rather_than_exiting() {
    let host = machine_with_claude_code().with_interrupt_after(2);

    let (outcome, said) = run_watch(&host);

    outcome.expect("a machine with nothing to adopt is held on, not exited on");
    assert!(
        said.contains("No Claude Code login"),
        "and the held line says what is missing: {said}"
    );
    assert!(
        said.contains("Stopped."),
        "the loop ran until it was asked to stop, rather than ending itself: \
         {said}"
    );
}

#[test]
fn a_check_on_a_machine_with_no_login_still_reports_the_refusal() {
    let host = machine_with_claude_code();

    let (outcome, _) = run_watch_once(&host);

    let refused = outcome.expect_err("a scheduler has to be told");
    assert!(
        refused.to_string().contains("No Claude Code login"),
        "{refused}"
    );
}

/// A machine where the Account being watched cannot be read: the usage endpoint answers
/// `refusals` in turn, the last of them for every round after the trace runs out, and
/// the Account it would move to is empty and waiting.
fn unreadable(refusals: &[(u16, &str)], rounds: u32) -> FakeHost {
    let host = watched()
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_replies_to(USAGE_URL, ACTIVE_TOKEN, refusals);
    answering(host, SPARE_TOKEN, SECOND_EMAIL, &[5.0]).with_interrupt_after(rounds)
}

/// How long the loop waited after each round, in the order it waited.
fn waits(host: &FakeHost) -> Vec<u64> {
    host.effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(*millis),
            _ => None,
        })
        .collect()
}

/// Every access token that asked the usage endpoint how full an Account is, in the
/// order they asked.
fn asked_by(host: &FakeHost) -> Vec<String> {
    host.sent_to(USAGE_URL)
        .iter()
        .map(|sent| sent.bearer().unwrap_or("nobody").to_string())
        .collect()
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active().whose().map(str::to_string)
}

#[test]
fn a_client_that_starts_during_the_lock_wait_is_refused_and_the_loop_carries_on() {
    let now = watched().now();
    let outgoing = store_of(&watched(), EMAIL).config_dir;
    let host = watching(&[86.0, 40.0], 5.0)
        .with_dir_held_since("/Users/someone/.claude/.oauth_refresh.lock", now)
        .once_while_waiting(move |host| {
            // The holder gives the lock back — and in the same moment somebody starts
            // working against the Profile being switched away from.
            host.remove_dir_all(std::path::Path::new(
                "/Users/someone/.claude/.oauth_refresh.lock",
            ))
            .expect("the holder is done");
            host.set_file(
                format!("{}/sessions/7788.json", outgoing.display()),
                &a_client_marker(7788, now),
            );
            host.set_live_process(7788);
        });

    let (result, printed) = run_watch(&host);

    result.expect("a Switch the machine turned away does not end the watch");
    let decisions = decisions(&printed);
    assert!(
        decisions[0].contains("7788"),
        "the round says which client holds it: {printed}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing was switched"
    );
    assert!(
        decisions.len() > 1,
        "the loop went round again rather than stopping: {printed}"
    );
}

#[test]
fn every_round_prints_one_line_naming_the_figure_and_the_outcome() {
    let host = watching(&[40.0, 45.0, 52.0], 5.0);

    let (result, printed) = run_watch(&host);

    result.expect("a watcher that was stopped ended cleanly");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 3, "one line a round: {printed}");
    for (decision, used) in decisions.iter().zip(["40%", "45%", "52%"]) {
        assert!(decision.contains("waiting"), "{decision}");
        assert!(decision.contains(used), "the figure it read: {decision}");
        assert!(decision.contains("5-hour"), "and which window: {decision}");
    }
    assert!(
        printed.contains("Stopped."),
        "and it says it has stopped: {printed}"
    );
}

#[test]
fn the_threshold_is_said_once_for_the_whole_run_and_that_once_is_the_opening() {
    let host = watching(&[40.0, 45.0, 52.0], 5.0);

    let (result, printed) = run_watch(&host);

    result.expect("a watcher that was stopped ended cleanly");
    assert_eq!(
        printed.matches("80%").count(),
        1,
        "the whole run names the threshold once: {printed}"
    );
    let opening = printed.lines().next().expect("the opening line");
    assert!(
        opening.contains("reaches 80%"),
        "and the opening is where: {opening}"
    );
    for decision in decisions(&printed) {
        assert!(
            !decision.contains("threshold"),
            "no round re-derives what the opening declared: {decision}"
        );
    }
}

#[test]
fn only_the_account_you_are_on_is_refreshed() {
    let host = watching(&[40.0, 45.0, 52.0], 5.0);

    run_watch(&host).0.expect("it was stopped");

    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN; 3],
        "the other Account answers perfectly well and was never asked: its \
         figures are read at a decision, and no decision was taken"
    );
}

#[test]
fn a_round_asks_once_which_claude_code_is_installed_however_much_it_does() {
    // Over the threshold, so the round reads the candidates and Switches: the round
    // that used to ask three times.
    let host = watching(&[95.0], 5.0);

    run_watch(&host).0.expect("it was stopped");

    let probes = host
        .effects()
        .iter()
        .filter(|effect| {
            matches!(effect, Effect::Exec { program, args }
                if program == CLAUDE_BIN && args == &["--version".to_string()])
        })
        .count();
    assert_eq!(probes, 1, "{:?}", host.effects());
}

#[test]
fn the_refresh_interval_keeps_one_account_inside_its_hourly_allowance() {
    let host = watching(&[40.0, 45.0], 5.0);

    run_watch(&host).0.expect("it was stopped");

    let reads_an_hour = 3_600_000 / REFRESH_INTERVAL_MILLIS;
    assert!(
        (12..=28).contains(&reads_an_hour),
        "{reads_an_hour} reads an hour is either past the 28-30 the endpoint          allows or too rare to catch a crossing while it matters"
    );
    assert_eq!(
        waits(&host),
        vec![REFRESH_INTERVAL_MILLIS; 2],
        "and it is what the loop actually waits"
    );
}

#[test]
fn crossing_the_threshold_switches_and_the_line_says_what_it_switched_on() {
    let host = watching(&[40.0, 86.0], 5.0);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    let decisions = decisions(&printed);
    assert!(decisions[0].contains("waiting"), "{printed}");

    let switched = &decisions[1];
    assert!(switched.contains("switched"), "{switched}");
    assert!(switched.contains("86% used"), "what it read: {switched}");
    assert!(switched.contains(SECOND_EMAIL), "where it went: {switched}");
    assert!(
        !switched.contains("most room"),
        "and not why that Account won, which is Perch defending a ranking \
         nobody questioned (ADR perch-says-what-it-did): {switched}"
    );

    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SPARE),
        "and the Switch was a Switch: the incoming Credential is the live one"
    );
}

#[test]
fn acting_captures_the_outgoing_credential_before_it_writes_the_incoming_one() {
    let host = watching(&[86.0], 5.0);
    // A Rotation that happened while this Account was active: the live copy is ahead of
    // the one in its own Profile, and is the only good one there is.
    const ROTATED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-active","refreshToken":"sk-ant-ort01-rotated","expiresAt":1790000000000,"subscriptionType":"pro"}}"#;
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);

    run_watch(&host).0.expect("it was stopped");

    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(ROTATED),
        "the Account it left keeps the Rotation it earned while it was live"
    );
    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
}

#[test]
fn the_accounts_it_could_move_to_are_read_only_when_a_decision_needs_them() {
    let host = watching(&[40.0, 45.0, 86.0], 5.0);

    run_watch(&host).0.expect("it was stopped");

    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN, ACTIVE_TOKEN, ACTIVE_TOKEN, SPARE_TOKEN],
        "the candidate is asked once, at the crossing, and never before it"
    );
}

#[test]
fn a_reading_that_failed_holds_the_decision_rather_than_falling_back_to_the_cache() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_reply(USAGE_URL, 429, "{}")
        .with_interrupt_after(1);
    // A cached figure well over the threshold: acting on it would switch, and acting on
    // it is the whole of what this refuses to do.
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    let held = &decisions(&printed)[0];
    assert!(held.contains("held"), "{held}");
    assert!(
        held.contains("unread"),
        "the cached 95% is not quoted as the figure it decided on: {held}"
    );
    assert!(held.contains("rate-limiting"), "and why: {held}");
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "nothing was switched on a figure Perch already had"
    );
    assert_eq!(
        asked_by(&host).len(),
        1,
        "and no candidate was read for a decision that was never taken"
    );
}

#[test]
fn a_refresh_that_fails_across_a_threshold_crossing_never_switches() {
    let host = unreadable(&[(429, "{}")], 4);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 4, "it kept watching: {printed}");
    for decision in &decisions {
        assert!(decision.contains("held"), "{decision}");
        assert!(
            decision.contains("unread"),
            "and never quotes the cached 95% as the figure it decided on: \
             {decision}"
        );
    }
    assert_eq!(
        printed.matches("switched").count(),
        0,
        "the crossing is only in a figure nobody could confirm: {printed}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(ACTIVE),
        "and nothing was Captured or written for a Switch that never happened"
    );
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN; 4],
        "no candidate was read for a decision that was never taken"
    );
}

#[test]
fn a_refresh_that_keeps_failing_is_retried_less_and_less_often_and_then_no_less() {
    let host = unreadable(&[(429, "{}")], 5);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    assert_eq!(
        waits(&host),
        vec![150_000, 300_000, 600_000, 1_200_000, 1_200_000],
        "doubling from the ordinary interval, and bounded: {printed}"
    );
    for (decision, coming_back) in decisions(&printed)
        .iter()
        .zip(["2m30s", "5m00s", "10m00s", "20m00s", "20m00s"])
    {
        assert!(decision.contains("held"), "{decision}");
        assert!(
            decision.contains("rate-limiting"),
            "the line names the failure: {decision}"
        );
        assert!(
            decision.contains(coming_back),
            "and when it will ask again, which is the only thing about a hold \
             that changes: {decision}"
        );
    }
}

#[test]
fn a_failure_that_never_clears_never_asks_faster_than_an_ordinary_round() {
    let host = unreadable(&[(429, "{}")], 6);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    run_watch(&host)
        .0
        .expect("a held decision is not a failure");

    for wait in waits(&host) {
        let reads_an_hour = 3_600_000 / wait;
        assert!(
            wait >= REFRESH_INTERVAL_MILLIS && reads_an_hour <= 28,
            "{reads_an_hour} reads an hour is past what the endpoint allows, \
             and it is failing"
        );
    }
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN; 6],
        "and it is still only ever the Account it is on"
    );
}

#[test]
fn the_first_reading_that_works_puts_the_loop_back_to_its_ordinary_cadence() {
    let readable = usage(40.0);
    let host = unreadable(&[(429, "{}"), (429, "{}"), (200, &readable)], 4);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        waits(&host),
        vec![150_000, 300_000, 150_000, 150_000],
        "two failures, then the endpoint comes back: {printed}"
    );
    let decisions = decisions(&printed);
    assert!(decisions[1].contains("held"), "{printed}");
    assert!(
        decisions[2].contains("waiting") && decisions[2].contains("40% used"),
        "and the figure it decided on is the one it just read: {printed}"
    );
}

#[test]
fn a_reply_perch_cannot_read_is_a_failed_refresh_rather_than_a_reading_of_zero() {
    let host = unreadable(
        &[(200, r#"{"five_hour": {}}"#), (200, "<html>nope</html>")],
        2,
    );
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "{printed}");
    for decision in &decisions {
        assert!(decision.contains("held"), "{decision}");
        assert!(
            !decision.contains("waiting") && !decision.contains("0% used"),
            "a partial reply is not an Account with nothing used: {decision}"
        );
    }
    // A window shaped like one that will not say how full it is is named, so the line
    // says which window drifted rather than only that something did.
    assert!(
        decisions[0].contains("five_hour") && decisions[0].contains("utilization"),
        "and the line says what could not be read: {}",
        decisions[0]
    );
    assert!(
        decisions[1].contains("not JSON"),
        "whichever way it was unreadable: {}",
        decisions[1]
    );
    assert_eq!(
        waits(&host),
        vec![150_000, 300_000],
        "and it backs off the same way, because it is the same failure"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_live_profiles_token_is_never_renewed_to_get_a_figure() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_interrupt_after(1);
    // The Account you are on, with an access token that has run out and a client
    // holding it.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = client_running_against(host, DEFAULT_CONFIG_DIR, 4242);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    let held = &decisions(&printed)[0];
    assert!(held.contains("held"), "{held}");
    assert!(held.contains("4242"), "and names what is running: {held}");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "nothing was renewed under a running session"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

/// A Back-off paces questions nobody is answering. A Renewal refused because a
/// client is holding the Profile asks nobody anything, so the loop that finds
/// the session gone must be able to read at once rather than in twenty minutes.
#[test]
fn a_hold_that_asked_anthropic_nothing_does_not_pace_the_loop_down() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_interrupt_after(4);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = client_running_against(host, DEFAULT_CONFIG_DIR, 4242);
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "nothing was renewed under a running session: {printed}"
    );
    assert_eq!(
        waits(&host),
        vec![150_000; 4],
        "so every round comes back on the ordinary beat: {printed}"
    );
}

/// The third way to that refusal, and the one a constant true at the site got
/// wrong: the live Credential says it has run out and carries no refresh token
/// to buy another with, which is a fact about a file. Nothing is asked of
/// Anthropic, so a doubling here is bought by a round that sent nothing — and
/// waited out after the `perch relogin` that fixes it.
#[test]
fn a_credential_with_nothing_left_to_ask_with_does_not_pace_the_loop_down() {
    // Expired and with no refresh token: a Renewal has nowhere to go.
    const NOTHING_LEFT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","expiresAt":1,"subscriptionType":"pro"}}"#;

    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0]).with_interrupt_after(4);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, NOTHING_LEFT);
    // Naming somebody else, so the Quarantine is not recorded against the
    // Account being watched and the round reports a reading that failed.
    host.write_private_file(
        std::path::Path::new("/Users/someone/.claude.json"),
        SECOND_IDENTITY_FILE,
    )
    .expect("the identity file is written");
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);
    host.forget_effects();

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    assert!(
        asked_by(&host).is_empty(),
        "nothing was asked of Anthropic: {printed}"
    );
    assert_eq!(
        waits(&host),
        vec![150_000; 4],
        "so every round comes back on the ordinary beat — a doubling here is \
         twenty minutes bought by four rounds that sent nothing, and waited \
         out after the relogin that fixes it: {printed}"
    );
}

/// The way out `Because` cannot answer for. The stored token says it has run
/// out, so a Renewal goes to the token endpoint — and Anthropic refuses the
/// refresh token. Why the Renewal was *wanted* is what `Because` records; what
/// paces the Back-off is whether a request went out, and here one did.
#[test]
fn a_renewal_anthropic_refused_paces_the_loop_down() {
    // The body OAuth gives a retired refresh token.
    const RETIRED: &str = r#"{"error":"invalid_grant"}"#;

    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply(TOKEN_URL, 401, RETIRED)
        .with_interrupt_after(4);
    // Expired, and with a refresh token to spend on a Renewal.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    // Naming somebody else, so the Quarantine is not recorded against the
    // Account and every round reaches the token endpoint again.
    host.write_private_file(
        std::path::Path::new("/Users/someone/.claude.json"),
        SECOND_IDENTITY_FILE,
    )
    .expect("the identity file is written");
    observed(&host, EMAIL, vec![window("5-hour", 95.0)]);
    host.forget_effects();

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    assert!(
        !host.sent_to(TOKEN_URL).is_empty(),
        "the round did ask Anthropic to renew: {printed}"
    );
    assert_ne!(
        waits(&host),
        vec![150_000; 4],
        "so the rounds that spent one apiece are paced rather than \
         hitting the token endpoint on the ordinary beat for ever: {printed}"
    );
}

/// The mirror, and the half a shared constant got wrong: the same refusal is
/// reached again *after* a reading has gone out and Anthropic has turned a live
/// token away. That round spent, so the Back-off paces it and the sentence says
/// what actually happened rather than naming an expiry that has not.
#[test]
fn a_hold_after_anthropic_refused_a_live_token_paces_the_loop_down() {
    let host = watched()
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_replies_to(
            USAGE_URL,
            ACTIVE_TOKEN,
            &[(401, r#"{"error":"unauthorized"}"#)],
        );
    let host = answering(host, SPARE_TOKEN, SECOND_EMAIL, &[5.0]).with_interrupt_after(4);
    let host = client_running_against(host, DEFAULT_CONFIG_DIR, 4242);
    host.forget_effects();

    let (result, printed) = run_watch(&host);

    result.expect("a held decision is not a failure");
    assert!(
        !asked_by(&host).is_empty(),
        "the round asked Anthropic and was refused: {printed}"
    );
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "and nothing was renewed under the running session: {printed}"
    );
    assert_eq!(
        waits(&host),
        vec![150_000, 300_000, 600_000, 1_200_000],
        "so the wait doubles, bounded — a flat beat here is a refused request \
         every interval for as long as that session runs: {printed}"
    );
    let held = decisions(&printed);
    assert!(
        held[0].contains("Anthropic would not accept its access token"),
        "and the line says what happened: {printed}"
    );
    assert!(
        !held[0].contains("has expired"),
        "rather than an expiry the Credential has months left before: {printed}"
    );
}

#[test]
fn nowhere_to_go_is_a_decision_and_the_loop_goes_on_watching() {
    let host = watching(&[100.0, 100.0], 100.0);

    let (result, printed) = run_watch(&host);

    result.expect("a dead end does not end the watch");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "it kept watching: {printed}");
    for decision in &decisions {
        assert!(decision.contains("nowhere"), "{decision}");
        assert!(decision.contains("exhausted"), "and why: {decision}");
        assert!(
            !decision.contains('\n'),
            "however long the reason is: {decision}"
        );
    }
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
}

#[test]
fn a_candidate_only_just_emptier_than_the_threshold_is_never_switched_to() {
    let host = watching(&[86.0, 88.0], 74.0);

    let (result, printed) = run_watch(&host);

    result.expect("a candidate not worth moving to does not end the watch");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "it kept watching: {printed}");
    for decision in &decisions {
        assert!(decision.contains("nowhere"), "{decision}");
        assert!(decision.contains("74%"), "how full it was: {decision}");
        assert!(
            decision.contains("70%"),
            "and the figure that was wanted, which is the threshold less the \
             margin: {decision}"
        );
    }
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing moved: {printed}"
    );
}

#[test]
fn a_destination_nearly_as_full_as_the_account_being_left_is_refused() {
    let host = watching_both(&[82.0, 79.0, 83.0, 81.0, 84.0, 80.0], &[78.0], 6);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        printed.matches("switched").count(),
        0,
        "nothing here is worth moving to, and doing it anyway is the walk \
         upward: {printed}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
    assert_eq!(
        decisions(&printed).len(),
        6,
        "and it said so every round: {printed}"
    );
}

/// The other round that has read every candidate: one that wanted a Switch,
/// attempted it, and was turned away. At the ordinary interval that repeats the
/// burst twenty-four times an hour per candidate, against the 28-30 Anthropic
/// allows — the same cost nowhere-to-go rests to avoid, for the same reason.
#[test]
fn a_switch_the_machine_turned_away_is_looked_at_again_at_the_cooldown() {
    let now = watched().now();
    let outgoing = store_of(&watched(), EMAIL).config_dir;
    let host = watching(&[86.0, 40.0], 5.0)
        .with_dir_held_since("/Users/someone/.claude/.oauth_refresh.lock", now)
        .once_while_waiting(move |host| {
            host.remove_dir_all(std::path::Path::new(
                "/Users/someone/.claude/.oauth_refresh.lock",
            ))
            .expect("the holder is done");
            host.set_file(
                format!("{}/sessions/7788.json", outgoing.display()),
                &a_client_marker(7788, now),
            );
            host.set_live_process(7788);
        });

    let (result, printed) = run_watch(&host);

    result.expect("a Switch the machine turned away does not end the watch");
    let waits: Vec<u64> = host
        .effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(millis),
            _ => None,
        })
        .collect();
    assert_eq!(
        waits.first().copied(),
        Some(perch::watch::NOWHERE_INTERVAL_MILLIS),
        "the round after the refusal rests for the Cooldown: {waits:?}\n{printed}"
    );
}

/// Nowhere to go is the one steady state a Threshold crossing can sit in for hours,
/// and the round that reaches it read every candidate. At the ordinary interval that is
/// twenty-four reads an hour *per candidate*, against the 28-30 Anthropic allows — so
/// the loop spends the allowance that `perch status --refresh` needs, on Accounts it
/// has already refused.
#[test]
fn a_dead_end_is_looked_at_again_at_the_cooldown_rather_than_at_the_interval() {
    // Every round over the Threshold, and the only candidate barely emptier — so
    // every one of them crosses, reads, and finds nowhere worth going.
    let host = watching_both(&[82.0, 83.0, 81.0, 84.0, 85.0, 86.0], &[78.0], 6);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    let waits: Vec<u64> = host
        .effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(millis),
            _ => None,
        })
        .collect();
    assert!(
        waits
            .iter()
            .all(|millis| *millis == perch::watch::NOWHERE_INTERVAL_MILLIS),
        "every round found nowhere to go, so every wait is the Cooldown: {waits:?}"
    );
    assert!(
        printed.contains("Looking again in 15m"),
        "and the line says the loop is resting longer than an interval: {printed}"
    );
}

/// A machine where the Account being watched fills up, the watcher moves off it, and
/// the Account it moved to fills up in turn — the trace that would move every round if
/// nothing paced it. The Account left behind is roomy again by then, so only the
/// cooldown stands between the two Switches.
fn filling_up_one_after_the_other() -> FakeHost {
    // Rounds are 2m30s apart, so a fifteen-minute cooldown is six of them: the Switch
    // lands on the second round and the earliest the next one may is the eighth.
    watching_both(&[40.0, 86.0, 20.0], &[5.0, 90.0], 8)
}

#[test]
fn two_switches_never_happen_closer_together_than_the_cooldown() {
    let host = filling_up_one_after_the_other();

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    let decisions = decisions(&printed);
    let switches: Vec<&String> = decisions
        .iter()
        .filter(|decision| decision.contains("switched"))
        .collect();
    assert_eq!(switches.len(), 2, "{printed}");
    assert!(
        when(switches[1]) - when(switches[0]) >= Duration::minutes(15),
        "{} then {}",
        switches[0],
        switches[1],
    );

    let cooling: Vec<&String> = decisions
        .iter()
        .filter(|decision| decision.contains("cooling"))
        .collect();
    assert_eq!(
        cooling.len(),
        5,
        "and every round in between said why it was not acting: {printed}"
    );
    for decision in cooling {
        assert!(decision.contains("90% used"), "what it read: {decision}");
        assert!(decision.contains("15 minutes"), "the cooldown: {decision}");
        assert!(
            decision.contains("so nothing moves"),
            "and when it lifts, because a refusal keeps its whole sentence \
             (ADR a-refusal-is-a-promise): {decision}"
        );
    }
}

#[test]
fn the_account_just_left_is_returned_to_once_the_cooldown_has_run_out() {
    let host = filling_up_one_after_the_other();

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        asked_by(&host),
        vec![
            // The Account being watched, then the candidate it moved to.
            ACTIVE_TOKEN,
            ACTIVE_TOKEN,
            SPARE_TOKEN,
            // Six rounds watching the Account it moved to, five of them held back by
            // the cooldown with nowhere read.
            SPARE_TOKEN,
            SPARE_TOKEN,
            SPARE_TOKEN,
            SPARE_TOKEN,
            SPARE_TOKEN,
            SPARE_TOKEN,
            // And the cooldown over, the Account it left is a candidate again.
            ACTIVE_TOKEN,
        ],
        "the Account it came off is not read while it may not be returned to: \
         {printed}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and it is returned to once the cooldown has run out: {printed}"
    );
}

#[test]
fn the_loop_carries_its_cooldown_in_memory_and_records_nothing() {
    let host = filling_up_one_after_the_other();

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        printed.matches("switched").count(),
        2,
        "the trace this is read off is one that Switched: {printed}"
    );
    assert!(
        registry_of(&host).checks.is_empty(),
        "two people watching in two terminals would otherwise pace each \
         other's decisions"
    );
}

#[test]
fn the_margin_refuses_a_barely_emptier_candidate_for_every_group() {
    let host = watching_both(&[86.0, 20.0], &[79.0, 86.0], 3);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        printed.matches("switched").count(),
        0,
        "79% is not 70% or better, and no Setting can say otherwise: {printed}"
    );
    assert!(
        printed.contains("nothing over 70% is worth moving to"),
        "and the round says which figure it was judged against: {printed}"
    );
}

#[test]
fn a_switch_the_machine_turned_away_is_said_and_the_loop_carries_on() {
    let host = watching(&[86.0, 88.0], 5.0);
    // A `perch run` against the Account being left: the Capture would write into a
    // Profile something else is holding (ADR a-profile-is-live-by-evidence).
    a_run_against(&host, EMAIL, host.now());

    let (result, printed) = run_watch(&host);

    result.expect("a refused Switch does not end the watch");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "{printed}");
    for decision in &decisions {
        assert!(decision.contains("refused"), "{decision}");
        assert!(decision.contains("86% used") || decision.contains("88% used"));
    }
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
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN; 2],
        "and no candidate was read: a `perch run` held open in another terminal \
         keeps that Profile Live for as long as somebody is working in it, so \
         reading every candidate each round spends an allowance that does not \
         refill early (ADR a-figure-carries-its-age) on a Switch that cannot happen — and throttles \
         the `perch status --refresh` the user types, at the moment the watcher \
         matters most"
    );
}

#[test]
fn a_profile_whose_sessions_will_not_be_read_stops_the_round_before_it_spends_anything() {
    let host = watching(&[86.0], 5.0);
    // A `sessions` that is a regular file is `ENOTDIR` on a real filesystem: doubt
    // about what is running, rather than the absence that means nothing is.
    host.set_file(
        format!("{}/sessions", store_of(&host, EMAIL).config_dir.display()),
        "",
    );

    let (result, _) = run_watch_once(&host);

    let error = result.expect_err("nothing about that Profile has been established");
    assert_eq!(error.exit_code(), perch::error::EXIT_PROBE_REFUSED);
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN],
        "and the candidates were never read: the round that cannot move must not \
         spend a Renewal apiece finding out"
    );
}

#[test]
fn an_account_the_watcher_finds_broken_is_recorded_rather_than_rediscovered() {
    let host = watching(&[86.0, 88.0], 5.0);
    // The Account it would move to has nothing in either store — a Rotation that went
    // missing, or a store somebody cleared out.
    host.forget_keychain_item(&store_of(&host, SECOND_EMAIL).keychain_service, LOGIN_NAME);

    let (result, printed) = run_watch(&host);

    result.expect("an Account that turns out to be broken does not end the watch");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(perch::registry::Quarantine::NoCredential),
        "written down where it was found: {printed}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing was switched onto it"
    );
    // The second round has the Quarantine to read, so it never gets as far as trying:
    // nowhere to go rather than a Switch refused.
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "{printed}");
    assert!(
        decisions[1].contains("nowhere"),
        "the round after knows better than to try again: {}",
        decisions[1]
    );
}

#[test]
fn a_quarantine_the_watcher_reports_names_the_account_the_way_the_user_does() {
    let host = watching(&[86.0, 88.0], 5.0);
    set_alias(&host, "spare", SECOND_EMAIL)
        .0
        .expect("the name is free");
    host.forget_keychain_item(&store_of(&host, SECOND_EMAIL).keychain_service, LOGIN_NAME);

    let (result, printed) = run_watch(&host);

    result.expect("an Account that turns out to be broken does not end the watch");
    assert!(
        printed.contains("(as `spare`)"),
        "the Alias is how the user would say it:\n{printed}"
    );
    assert!(
        printed.contains(&format!("perch relogin {SECOND_EMAIL}")),
        "and the repair is still a Target that can be typed:\n{printed}"
    );
}

/// An Account already Quarantined is not asked again — `observe` returns before
/// the first request — so the round spent nothing, and a Back-off paces questions
/// nobody is answering. Charged, the loop is asking once every twenty minutes
/// within eight minutes of the Quarantine, and a `perch relogin` that clears it
/// waits out the rest of that.
#[test]
fn a_round_that_asked_nobody_anything_does_not_pace_the_loop_down() {
    let host = watching(&[86.0, 88.0, 90.0, 92.0], 5.0);
    quarantine_for(&host, EMAIL, perch::registry::Quarantine::RenewalRejected);

    let (result, printed) = run_watch(&host);

    result.expect("a Quarantined Account is held on rather than exited on");
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "nothing was asked of Anthropic at all:\n{printed}"
    );
    let waits: Vec<u64> = host
        .effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(*millis),
            _ => None,
        })
        .collect();
    assert_eq!(
        waits,
        vec![150_000, 150_000, 150_000, 150_000],
        "so nothing earned a longer wait:\n{printed}"
    );
}

#[test]
fn stopping_leaves_no_lock_no_marker_and_no_half_written_state() {
    let host = watching(&[40.0, 45.0], 5.0);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert!(
        host.is_listening_for_interrupts(),
        "Ctrl-C is taken over from the default handler, so a Switch in flight \
         finishes before the loop ends"
    );
    assert!(
        !host.path_exists(std::path::Path::new(REGISTRY_LOCK)),
        "the registry lock is given back every round rather than held for the \
         life of the loop"
    );
    assert!(
        host.paths_under(DEFAULT_CONFIG_DIR)
            .iter()
            .all(|path| !path.to_string_lossy().contains("sessions")),
        "and the watcher is not a client: it writes no session marker"
    );
    assert!(printed.contains("Stopped."), "{printed}");
}

#[test]
fn the_decision_log_is_standard_output_and_no_file_is_written() {
    let host = watching(&[40.0, 45.0], 5.0);

    let (_, printed) = run_watch(&host);

    assert_eq!(decisions(&printed).len(), 2);

    // Named rather than filtered by what a logfile might be called: the property is
    // about which files are written, not what they are named.
    let written: Vec<_> = host
        .effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::WroteFile(path) | Effect::WrotePrivateFile(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    let registry = perch::registry::registry_path(&host).expect("home is known");
    assert!(
        written
            .iter()
            // The registry, and the copy beside it its atomic write goes through on the
            // way.
            .all(|path| path
                .to_string_lossy()
                .starts_with(&*registry.to_string_lossy())),
        "the registry is the only file a watcher writes, and it writes that \
         because a Switch happened: {written:?}"
    );
}

#[test]
fn an_account_in_no_group_is_not_watched_however_freely_it_may_be_cycled() {
    let host = watching(&[40.0], 5.0);
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the Account leaves the Group");
    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("ungrouped Accounts are interchangeable");

    let (result, printed) = run_watch(&host);

    // The loop holds rather than stopping (ADR the-machine-runs-the-watcher), and what
    // the grant governs is untouched: a holding watcher reads nothing and moves
    // nothing.
    result.expect("a machine that is not arranged for watching is held, not failed");
    assert!(
        printed.contains("held")
            && printed.contains("perch config set ungrouped watcher-may-act true"),
        "it says what is missing, addressing the Scope the way every other one \
         is addressed: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and it holds without reading anything: the question it is held by is \
         asked of the registry, not of Anthropic"
    );

    // A Check has no process left to hold with, so it exits `14` where the loop holds.
    let (once, _) = run_watch_once(&host);
    assert_eq!(
        once.expect_err("a Check still refuses").exit_code(),
        EXIT_INVALID
    );
}

#[test]
fn a_landing_the_watcher_cannot_settle_holds_the_loop_rather_than_stopping_it() {
    let host = watching(&[99.0], 1.0);
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    // A Rotation after the interruption: the corner nothing on the machine can account
    // for.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);

    let (result, printed) = run_watch(&host);

    result.expect("a Switch left in flight is held on, not stopped on");
    assert!(
        printed.contains("held") && printed.contains("perch relogin"),
        "it says what is holding it and the way through: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and it holds without reading anything: a Watcher that may not act \
         spends nothing finding out where it would have gone"
    );

    // A scheduler has to be told, which is the one difference between the two
    // arrangements.
    let (once, _) = run_watch_once(&host);
    assert_eq!(
        once.expect_err("a Check exits on it").exit_code(),
        perch::error::EXIT_CONFLICT
    );
}

#[test]
fn a_landing_in_flight_leaves_the_opening_line_naming_nobody() {
    let host = watching(&[42.0], 5.0);
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);

    let (result, printed) = run_watch(&host);

    result.expect("a Landing this Perch can settle is no reason to stop");
    let opening = printed.lines().next().expect("the opening line");
    assert!(
        opening.starts_with("Started."),
        "it says it started and leaves the reason to the first round: {opening}"
    );
    assert!(
        !opening.contains(EMAIL) && !opening.contains(SECOND_EMAIL),
        "and names neither of the two Accounts the Landing left in doubt: \
         {opening}"
    );

    // The round settles it and gets on with watching, which is what makes the quiet
    // opening a deferral rather than a refusal.
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "the Landing is settled onto the Account that was being left: {printed}"
    );
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 1, "{printed}");
    assert!(
        decisions[0].contains("waiting") && decisions[0].contains("42% used"),
        "and that line decides, because by then there is an Account to decide \
         about: {}",
        decisions[0]
    );
}

#[test]
fn a_grant_said_about_a_group_leaves_ungrouped_accounts_alone() {
    let host = watching(&[99.0], 1.0);
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("a statement about the Group this person runs");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none")
            .0
            .expect("the Account leaves the Group");
    }

    let (result, printed) = run_watch(&host);

    result.expect("held rather than failed");
    assert!(
        printed.contains("interchangeable"),
        "the declaration that is still missing is named: {printed}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and nothing moved underneath them, at 99% used with an empty Account \
         beside it (ADR a-group-is-a-declaration) — which is the whole of what the grant protects, \
         and is exactly as true of a watcher that holds as of one that exits"
    );
    assert!(host.sent_to(USAGE_URL).is_empty());

    let (once, _) = run_watch_once(&host);
    assert_eq!(
        once.expect_err("a Check still refuses").exit_code(),
        EXIT_NOT_INTERCHANGEABLE
    );
}

#[test]
fn a_watcher_acts_among_ungrouped_accounts_once_both_declarations_are_made() {
    let host = watching(&[99.0], 1.0);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none")
            .0
            .expect("the Account leaves the Group");
    }
    config_set(&host, &["ungrouped", "interchangeable", "true"])
        .0
        .expect("they are interchangeable");
    config_set(&host, &["ungrouped", "watcher-may-act", "true"])
        .0
        .expect("and the watcher may act on them");

    let (result, printed) = run_watch(&host);

    result.expect("a watcher that switched is not a failure");
    assert!(printed.contains("switched"), "{printed}");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

#[test]
fn a_group_that_has_not_said_the_watcher_may_act_is_not_watched() {
    let host = watching(&[40.0], 5.0);
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");

    let (result, printed) = run_watch(&host);

    result.expect("held rather than failed");
    assert!(printed.contains("watcher-may-act"), "{printed}");
    assert!(host.sent_to(USAGE_URL).is_empty(), "and nothing was read");

    let (once, _) = run_watch_once(&host);
    assert_eq!(
        once.expect_err("a Check still refuses").exit_code(),
        EXIT_INVALID
    );
}

#[test]
fn a_switch_onto_an_ungrouped_account_holds_the_loop_that_was_already_running() {
    let host = watching(&[40.0, 45.0], 5.0).once_while_waiting(|host| {
        move_to_group(host, SECOND_EMAIL, "none")
            .0
            .expect("the other Account leaves the Group");
        run_switch(host, SECOND_EMAIL)
            .0
            .expect("somebody Switches onto it between two rounds");
    });

    let (result, printed) = run_watch(&host);

    result.expect("the grant is gone, so the loop holds rather than failing");
    assert!(printed.contains("perch group move"), "{printed}");
    assert!(printed.contains("watcher-may-act"), "{printed}");
    assert_eq!(
        decisions(&printed).len(),
        2,
        "the round before it, and the held round that says the grant is gone: \
         {printed}"
    );
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN.to_string()],
        "and it read nothing for the round it may not decide: a withdrawn grant \
         is a question for the registry, and holding costs no allowance"
    );
}

#[test]
fn a_group_that_takes_the_permission_back_holds_the_watcher_it_had_given_it() {
    let host = watching(&[40.0, 45.0], 5.0).once_while_waiting(|host| {
        config_set(host, &["work", "watcher-may-act", "false"])
            .0
            .expect("the Group takes the permission back between two rounds");
    });

    let (result, printed) = run_watch(&host);

    result.expect("nothing may be acted on any more, so the loop holds");
    assert!(printed.contains("watcher-may-act"), "{printed}");
    assert_eq!(
        decisions(&printed).len(),
        2,
        "the round before it, and the held round that says the grant is gone: \
         {printed}"
    );
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN.to_string()],
        "and it read nothing for the round it may not decide"
    );
}

#[test]
fn the_run_fixture_marks_the_profile_of_the_account_it_names() {
    let host = watched();
    a_run_against(&host, EMAIL, host.now());

    let profile = perch::registry::profile_dir_for(&host, EMAIL).expect("home is known");
    assert!(
        host.path_exists(&perch::probe::session_marker_at(&profile, THIS_PROCESS)),
        "the marker is in the Account's own Profile"
    );
}

#[test]
fn a_switch_that_changed_something_and_then_failed_stops_the_loop() {
    let host = watching(&[86.0, 40.0], 5.0);
    // The Credential is written and the Identity beside it cannot be, which is
    // ADR a-switch-is-written-down-first's crash between two writes arriving as a
    // failed write.
    let host = host.with_unwritable_file(IDENTITY_PATH, "read-only file system");

    let (result, printed) = run_watch(&host);

    let stopped = result.expect_err("the machine is part way through a Switch");
    // The file rather than the path it is at: a Windows Perch joins the two with the
    // other separator, so the spelling of the path is the platform's and only the name
    // is Perch's to promise.
    assert!(
        stopped.to_string().contains(".claude.json"),
        "and it says what could not be written: {stopped}"
    );
    assert_eq!(
        decisions(&printed).len(),
        0,
        "the round never reached a decision, so it printed none: {printed}"
    );
    assert!(
        !printed.contains("Stopped."),
        "and it did not claim to have stopped cleanly: {printed}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(SECOND_EMAIL),
        "which Account is active is a fact about which Credential is live, so \
         it is recorded as what happened rather than as what was asked for \
         and never a Landing nobody wrote down"
    );
}

#[test]
fn another_perch_holding_the_registry_holds_the_round_rather_than_ending_the_watcher() {
    let host = watching(&[40.0, 45.0], 5.0).once_while_waiting(|host| {
        // What another `perch` holding the lock looks like from here.
        host.create_dir_exclusive(std::path::Path::new(REGISTRY_LOCK))
            .expect("the other `perch` takes it");
    });

    let (result, printed) = run_watch(&host);

    result.expect("a lock somebody else holds is not a fault");
    let held = decisions(&printed)
        .into_iter()
        .find(|line| line.contains("held"))
        .unwrap_or_else(|| panic!("the round is held and said so: {printed}"));
    assert!(
        held.contains("registry") || held.contains("lock"),
        "and says what was holding it: {held}"
    );
    assert!(
        held.contains("Asking again in"),
        "a hold that did not say when it comes back reads as having given up: {held}"
    );
}

#[test]
fn a_200_carrying_no_quota_window_holds_the_round_rather_than_reading_as_empty() {
    let host = unreadable(&[(200, "{}")], 1);

    let (result, printed) = run_watch(&host);

    result.expect("an answer with nothing in it is not a fault");
    let held = decisions(&printed)
        .into_iter()
        .find(|line| line.contains("held"))
        .unwrap_or_else(|| panic!("the round is held and said so: {printed}"));
    assert!(
        held.contains("named no Quota Window"),
        "it says what was wrong with the answer: {held}"
    );
    assert!(
        held.contains("nothing current to decide on"),
        "and that this is why nothing was decided: {held}"
    );
    assert!(
        held.contains("Asking again in"),
        "a hold that did not say when it comes back reads as having given up: {held}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and nothing was switched on the strength of it"
    );
}

#[test]
fn nowhere_to_go_says_which_candidates_could_not_be_read() {
    let host = watched()
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_replies_to(USAGE_URL, ACTIVE_TOKEN, &[(200, &usage(95.0))])
        .with_reply_to(PROFILE_URL, SPARE_TOKEN, 200, &profile_of(SECOND_EMAIL))
        .with_replies_to(USAGE_URL, SPARE_TOKEN, &[(500, "the endpoint is unwell")])
        .with_interrupt_after(1);

    let (result, printed) = run_watch(&host);

    result.expect("nowhere to go is an answer, not a fault");
    let decided = decisions(&printed).join("\n");
    assert!(
        decided.contains(SECOND_EMAIL),
        "the candidate that could not be read is named: {decided}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "and the loop stayed where it was"
    );
}

#[test]
fn a_check_another_perch_holds_the_registry_against_says_so_where_cron_is_reading() {
    let host = watching(&[86.0], 5.0);
    let _held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a held round is an outcome rather than a failure"),
        perch::error::EXIT_HELD,
        "and the exit code a scheduler branches on is unchanged: {printed}"
    );
    assert!(
        printed.contains("held"),
        "the decision line is on standard output: {printed}"
    );
    assert!(
        printed.contains("nothing was decided"),
        "and says what came of the round: {printed}"
    );
    assert!(
        !printed.contains("Asking again in"),
        "without promising an interval a check has no part in — when it comes \
         back is whatever scheduled it: {printed}"
    );
}

#[test]
fn the_wait_a_ctrl_c_lands_in_costs_the_clock_nothing() {
    let host = watching(&[40.0, 45.0], 5.0);
    let opened_at = host.now();

    let (result, printed) = run_watch(&host);
    result.expect("it was stopped");

    let rounds = waits(&host).len() as i64;
    let interrupted_wait = waits(&host)
        .last()
        .copied()
        .expect("it waited at least once");
    assert!(
        interrupted_wait > 0,
        "the last wait was entered with an interval, which is what makes this \
         worth asserting: {:?}",
        waits(&host)
    );

    // Every wait but the last one, which is the one the interrupt landed in.
    let spent = (host.now() - opened_at).num_milliseconds();
    let waited: i64 = waits(&host).iter().map(|millis| *millis as i64).sum();
    assert_eq!(
        spent,
        waited - interrupted_wait as i64,
        "{rounds} rounds' waits, less the one that was interrupted:\n{printed}"
    );
}

#[test]
fn a_check_that_finds_a_watcher_already_running_holds_rather_than_deciding() {
    let host = watching(&[40.0], 5.0);
    let _watching_alone = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody holds it yet");

    let (result, printed) = run_watch_once(&host);

    assert_eq!(
        result.expect("a lock somebody else holds is not a failure"),
        perch::error::EXIT_HELD
    );
    assert!(
        printed.contains("held"),
        "and it says so on the same one line every other outcome uses: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "a Check that may not decide spends nothing finding out"
    );
}

#[test]
fn a_loop_that_finds_the_watch_held_says_so_and_comes_back_rather_than_exiting() {
    let host = watching(&[40.0], 5.0);
    let _watching_alone = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody holds it yet");

    let (result, printed) = run_watch(&host);

    result.expect("a Watcher that cannot take the watch holds rather than failing");
    assert!(
        printed.contains("held"),
        "it says what is holding it and when it will ask again: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and reads nothing while it waits for the watch"
    );
    assert!(
        printed.contains("Stopped."),
        "and a Ctrl-C while it is waiting still ends it cleanly: {printed}"
    );
}

/// The lock only becomes takeable once it is stale, and `lock::abandoned` is
/// consulted on an attempt alone — so how long a killed Watcher leaves this
/// machine unwatched is how often this asks. A doubling wait spends the staleness
/// window and then three more of them.
#[test]
fn a_loop_waiting_out_the_watch_asks_at_one_interval_rather_than_a_doubling_one() {
    let host = watching(&[40.0], 5.0).with_interrupt_after(4);
    let _watching_alone = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody holds it yet");

    let (result, printed) = run_watch(&host);

    result.expect("a Watcher that cannot take the watch holds rather than failing");
    // The waits themselves, because the log coalesces an unchanged hold into one
    // line and the doubling would be invisible there for the first hour.
    let waits: Vec<u64> = host
        .effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Waited { millis } => Some(*millis),
            _ => None,
        })
        .collect();
    assert_eq!(
        waits,
        vec![150_000, 150_000, 150_000, 150_000],
        "nothing here spent a request, so nothing earned a longer wait:\n{printed}"
    );
}

#[test]
fn a_loop_says_the_watch_is_still_held_on_every_round_it_takes() {
    let host = watching(&[40.0, 41.0, 42.0], 5.0);
    let lock = perch::registry::watcher_lock_spec(&host)
        .expect("home is known")
        .dir;

    let (result, _printed) = run_watch(&host);
    result.expect("nothing here fails");

    let renewals = host
        .effects()
        .iter()
        .filter(|effect| matches!(effect, Effect::Touched(path) if path == &lock))
        .count();
    // Three rounds and two renewals, which is what "once a round" comes to: the first
    // opens inside the update interval of the take that wrote the artifact.
    assert_eq!(
        renewals, 2,
        "every round but the one the take itself covers says the watch is \
         still held"
    );

    // And coming out of a wait the touch is the *first* thing that happens, ahead of
    // the round's own work — which is the half that bounds the gap, because the reads a
    // round makes are what it can spend minutes on.
    let effects = host.effects();
    let after_a_wait = effects
        .iter()
        .position(|effect| matches!(effect, Effect::Waited { .. }))
        .expect("the loop waits between rounds");
    let next = effects[after_a_wait + 1..]
        .iter()
        .find(|effect| {
            matches!(effect, Effect::Touched(path) if path == &lock)
                || matches!(effect, Effect::Http { .. })
        })
        .expect("the round after a wait does something");
    assert!(
        matches!(next, Effect::Touched(_)),
        "a renewal taken only after the reads is one already old by the time \
         the loop waits on it, so the wait and the round add up against the \
         staleness window instead of each standing alone: {next:?}"
    );
}

#[test]
fn a_loop_whose_watch_was_taken_over_stops_rather_than_deciding_beside_it() {
    let lock = "/Users/someone/.config/perch/.watch.lock";
    let host = watching(&[40.0, 41.0, 42.0], 5.0).once_while_waiting(move |host| {
        // What a takeover leaves behind: whoever decided this hold had gone quiet
        // cleared the artifact before making their own, and one that is not there is
        // not this Watcher's hold.
        let _ = host.remove_dir_all(std::path::Path::new(lock));
    });

    let (result, printed) = run_watch(&host);

    result.expect("a watch taken over is not this Watcher's failure");
    assert!(
        printed.contains("another Watcher has taken the watch over"),
        "it says why it is leaving rather than reporting an ordinary stop: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).len() < 3,
        "and it leaves rather than taking the rounds that were left"
    );
}

/// The burst is renewed either side of it and bounded by nothing in between, so it can
/// outlast the watch — and a Switch made after that is the second Watcher deciding
/// beside the first, which is the whole of what the lock is for.
#[test]
fn a_watch_taken_over_while_the_candidates_were_read_switches_nothing() {
    let lock = "/Users/someone/.config/perch/.watch.lock";
    // A keychain that stops to ask, and nobody at the machine to answer: the one
    // unbounded wait inside a round, and long enough here to carry it past the
    // staleness window that lets another Watcher judge this hold abandoned.
    let host = watching(&[86.0], 5.0)
        .with_a_keychain_that_asks_first(
            perch::watch::LONGEST_WAIT_MILLIS + perch::watch::REFRESH_INTERVAL_MILLIS + 1_000,
        )
        .once_while_waiting(move |host| {
            let _ = host.remove_dir_all(std::path::Path::new(lock));
        });
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    let code = result.expect("a watch taken over is not this Watcher's failure");
    assert_eq!(
        code,
        perch::error::EXIT_HELD,
        "a round another Watcher displaced is a contended lock rather than \
         nothing to do — a scheduler branching on 15 would record it as a \
         round that found no work: {printed}"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::KeychainSet { .. })),
        "nothing was switched, so no Credential moved: {printed}"
    );
    assert!(
        printed.contains("taken over"),
        "and the round says why it decided nothing: {printed}"
    );
}

#[test]
fn a_check_renews_the_watch_across_the_round_rather_than_only_holding_it() {
    let lock = "/Users/someone/.config/perch/.watch.lock";
    // A round that crosses the threshold behind a keychain that stops to ask. The
    // fake's clock only moves when something slow happens, and what has to be shown is
    // a round running longer than the interval keeping the hold up.
    let host = watching(&[86.0], 5.0).with_a_keychain_that_asks_first(70_000);
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    result.expect("the check reads and decides");
    let effects = host.effects();
    let switched = effects
        .iter()
        .position(|effect| matches!(effect, Effect::KeychainSet { .. }))
        .expect("the Switch writes the incoming Credential");
    let renewed_after = effects
        .iter()
        .skip(switched)
        .any(|effect| matches!(effect, Effect::Touched(path) if path.starts_with(lock)));

    assert!(
        renewed_after,
        "a Check never renewed the watch anywhere in the process, so its whole \
         round ran on the hold it started with\n{effects:#?}\n{printed}"
    );
}

#[test]
fn a_claude_code_holding_its_own_lock_is_a_refusal_rather_than_an_unread_figure() {
    // One round: the loop's wait advances the clock past the lock's staleness window,
    // so a second round would find it abandoned and take it over.
    let host = watching(&[86.0], 5.0);
    // Held now rather than long ago, so it is somebody's rather than abandoned.
    let holding_since = host.now();
    let host = host.with_dir_held_since("/Users/someone/.claude.json.lock", holding_since);

    let (result, printed) = run_watch(&host);

    result.expect("a lock somebody is holding does not end the watch");
    let decisions = decisions(&printed);
    assert!(!decisions.is_empty(), "{printed}");
    for decision in &decisions {
        assert!(
            decision.contains("refused"),
            "the round decided and was turned away, rather than reading nothing: {decision}"
        );
        assert!(
            decision.contains("86% used") || decision.contains("88% used"),
            "and it quotes the figure it read: {decision}"
        );
    }
    assert!(
        !printed.contains("20m00s"),
        "the Back-off paces questions nobody answers, and this one was \
         answered: {printed}"
    );
}

#[test]
fn a_check_held_settling_a_landing_says_so_on_standard_output() {
    let host = watching(&[86.0], 5.0);
    // A Switch that died between writing the Landing down and moving anything, so the
    // next round has one to settle.
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    // And a Claude Code holding the lock that settling it needs.
    let holding_since = host.now();
    let host = host.with_dir_held_since("/Users/someone/.claude.json.lock", holding_since);

    let (result, printed) = run_watch_once(&host);

    let code = result.expect("a lock somebody is holding is not a failed check");
    assert_eq!(code, perch::error::EXIT_HELD, "{printed}");
    assert!(
        printed.contains("lock"),
        "the line goes to standard output, and says what was holding it: {printed:?}"
    );
}

/// The same again, inside the round rather than between two. A `SIGKILL` here
/// runs no `Drop`, so both locks are left on disk: every other `perch` waits out
/// the registry's window, and the next login's Service waits out the watch's.
#[test]
fn a_watcher_asked_to_stop_reads_nothing_and_pays_nothing() {
    // A third Account, so there is a second candidate for the burst to reach —
    // and its Credential is one that has not run out, or the round stops at a
    // Renewal rather than at the reads this is about.
    const THIRD: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-third","refreshToken":"sk-ant-ort01-third","expiresAt":1790000000000,"subscriptionType":"pro"}}"#;
    let host = watched().with_login(login_producing(THIRD, THIRD_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the third Account is added");
    move_to_group(&host, THIRD_EMAIL, "work")
        .0
        .expect("it joins the Group");

    let host = answering(host, ACTIVE_TOKEN, EMAIL, &[86.0]);
    let host = answering(host, SPARE_TOKEN, SECOND_EMAIL, &[5.0]);
    let host = answering(host, THIRD_TOKEN, THIRD_EMAIL, &[5.0]).with_interrupt_after(0);
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    result.expect("being asked to stop is not this Watcher's failure");
    let asked = asked_by(&host);
    assert!(
        asked.is_empty(),
        "the reads stop where the stop arrived, including before the first: a \
         round given one address asks the question once, and an ask made only \
         after the first read is one that reading never makes: {asked:?}\n{printed}"
    );
    assert!(
        printed.contains("asked to stop"),
        "and the round says so rather than reporting a reading that failed, \
         which is what would pace a Back-off off a request nobody sent: {printed}"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::KeychainSet { .. })),
        "nothing was switched: {printed}"
    );
}

/// A round is bounded by the network rather than by the clock, so the wait at the
/// bottom of the loop is far too late to answer a stop: a service manager allows
/// thirty seconds and then sends a signal nothing can handle.
#[test]
fn a_watcher_asked_to_stop_while_the_candidates_were_read_switches_nothing() {
    let host = watching(&[86.0], 5.0).with_interrupt_after(0);
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    result.expect("being asked to stop is not this Watcher's failure");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::KeychainSet { .. })),
        "nothing was switched, so no Credential moved: {printed}"
    );
    assert!(
        printed.contains("asked to stop"),
        "and the round says why it decided nothing: {printed}"
    );
    assert!(
        printed.contains("stopped") && !printed.contains("replaced"),
        "under the word for what happened: nothing replaced this Watcher, and a \
         day of these is skimmed by that column: {printed}"
    );
}

/// A turn is up to six requests bounded at thirty seconds each, so a stop that
/// arrives after the first of them is one the ask at the top of the turn is far
/// too early to answer. Here it arrives after the ownership check and before the
/// read the round is for — and past that read is a Renewal, which retires the
/// only refresh token this Account has.
#[test]
fn a_stop_between_two_requests_of_one_turn_sends_no_more_of_them() {
    let host = watched()
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        // What the round would find past the stop: a token Anthropic will not
        // take, which is the one refusal that goes on to buy a new one.
        .with_reply_to(USAGE_URL, ACTIVE_TOKEN, 401, "{}")
        .with_interrupt_after_requests(1);
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    result.expect("being asked to stop is not this Watcher's failure");
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "the read the turn was for went out after the stop: {printed}"
    );
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "and so did the Renewal past it, which Rotates the only refresh token \
         this Account has: {printed}"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::KeychainSet { .. })),
        "no Rotation was written: {printed}"
    );
    let account = registry_of(&host)
        .account(EMAIL)
        .cloned()
        .expect("the Account is still held");
    assert!(
        account.quarantine.is_none() && account.utilization.is_none(),
        "and nothing was recorded against the Account, which learned nothing: \
         {account:?}\n{printed}"
    );
    assert!(
        printed.contains("asked to stop"),
        "and the round carries the loss rather than a reading that failed: \
         {printed}"
    );
}

/// The burst takes no wait at all, so what happens between two of its reads has
/// nowhere else to arrive. A second Watcher taking the watch over there leaves
/// this one ranking candidates it is no longer the one to choose between.
#[test]
fn a_watch_handed_over_between_two_reads_of_the_burst_switches_nothing() {
    let lock = "/Users/someone/.config/perch/.watch.lock";
    // Twenty seconds a request, so the hold comes up for its once-a-minute
    // renewal inside the candidate's turn: at thirty, a turn of two requests
    // puts every renewal at a turn's edge.
    let host = watching(&[86.0], 5.0)
        .with_a_network_that_answers_slowly(20_000)
        // Three requests in: the Account being watched answered both of its own,
        // and the candidate has just been asked whose its token is.
        .once_after_requests(3, move |host| {
            let _ = host.remove_dir_all(std::path::Path::new(lock));
        });
    host.forget_effects();

    let (result, printed) = run_watch_once(&host);

    let code = result.expect("a watch taken over is not this Watcher's failure");
    assert_eq!(code, perch::error::EXIT_HELD, "{printed}");
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        1,
        "the burst read on past the takeover: only the Account being watched \
         was asked before it: {printed}"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::KeychainSet { .. })),
        "and nothing was switched, so no Credential moved: {printed}"
    );
    assert!(
        printed.contains("taken over"),
        "and the round says why it decided nothing: {printed}"
    );
}
