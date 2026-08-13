//! Behaviour: `perch watch` — the loop, the Refresh, and the decision log.
//!
//! The tests here drive a simulated Utilization trace through the real loop
//! against the fake Host: the figure the endpoint answers with moves from one
//! Refresh to the next, exactly as an Account being worked in does, and what the
//! loop does about it is asserted from the outside — what it printed, what went
//! out to the network, and which Credential ended up live.
//!
//! Four properties are what this command is for, and each has a test that
//! fails if it stops holding: only the Account you are on is Refreshed, every
//! decision is printed including the ones where nothing happens, nothing is
//! ever acted on but a figure that was just read, and two Accounts either side
//! of the threshold never trade places with each other (ADR 0013).

mod common;

use chrono::{DateTime, Duration, Utc};
use common::*;
use perch::anthropic::{PROFILE_URL, TOKEN_URL, USAGE_URL};
use perch::error::{EXIT_INVALID, EXIT_NOT_INTERCHANGEABLE};
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::{FakeHost, Host};
use perch::watch::REFRESH_INTERVAL_MILLIS;

/// The Credential of an Account whose access token ran out twenty minutes ago,
/// so reading it at all means Renewing it first.
const SPENT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1785843600000,"subscriptionType":"pro"}}"#;

/// The Default Profile's config directory, which is where a client running as
/// the active Account holds its session.
const DEFAULT_CONFIG_DIR: &str = "/Users/someone/.claude";

/// Perch's own lock over its registry — the one artifact a loop could leave
/// behind if it held anything across a wait.
const REGISTRY_LOCK: &str = "/Users/someone/.config/perch/.registry.lock";

/// A machine where the active Account's figure follows `trace` and the Account
/// it would Cycle to sits at `spare`, watched for as many rounds as the trace
/// is long.
fn watching(trace: &[f64], spare: f64) -> FakeHost {
    let host = answering(watched(), ACTIVE_TOKEN, EMAIL, trace);
    let host = answering(host, SPARE_TOKEN, SECOND_EMAIL, &[spare]);
    host.with_interrupt_after(trace.len() as u32)
}

/// The same, where both Accounts have a trace of their own — for the tests
/// about moving back and forth, where the Account that was the candidate
/// becomes the one being watched.
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

/// A machine where the Account being watched cannot be read: the usage
/// endpoint answers `refusals` in turn, the last of them for every round after
/// the trace runs out, and the Account it would move to is empty and waiting.
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

/// Every access token that asked the usage endpoint how full an Account is, in
/// the order they asked.
fn asked_by(host: &FakeHost) -> Vec<String> {
    host.sent_to(USAGE_URL)
        .iter()
        .map(|sent| sent.bearer().unwrap_or("nobody").to_string())
        .collect()
}

fn active(host: &FakeHost) -> Option<String> {
    registry_of(host).active
}

/// A client that starts during the lock wait is a refusal the loop survives.
///
/// `one_round` asks whether the outgoing Profile is Live before it spends a
/// Refresh on the candidates, and the Switch asks again under the locks —
/// because taking them can take seconds and a `claude` started in that gap is
/// one the first answer never saw (ADR 0005). The second ask is the one that
/// finds this, so the refusal comes back out of `perform` rather than from the
/// check above it.
///
/// Nothing moved, so it is news about the machine rather than a fault in it:
/// the round says so, and the loop goes on watching. `perch switch` meeting the
/// same race exits 16 and stops, because it has nothing else to do.
#[test]
fn a_client_that_starts_during_the_lock_wait_is_refused_and_the_loop_carries_on() {
    let now = watched().now();
    let outgoing = store_of(&watched(), EMAIL).config_dir;
    let host = watching(&[86.0, 40.0], 5.0)
        .with_dir_held_since("/Users/someone/.claude/.oauth_refresh.lock", now)
        .once_while_waiting(move |host| {
            // The holder gives the lock back — and in the same moment somebody
            // starts working against the Profile being switched away from.
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

/// The decision log is the whole of the evidence that the policy works, and
/// most of what a watcher decides is to do nothing. A line for the rounds that
/// switched and silence for the rest would be a log that cannot answer "was it
/// even awake".
#[test]
fn every_round_prints_one_line_naming_the_figure_the_threshold_and_the_outcome() {
    let host = watching(&[40.0, 45.0, 52.0], 5.0);

    let (result, printed) = run_watch(&host);

    result.expect("a watcher that was stopped ended cleanly");
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 3, "one line a round: {printed}");
    for (decision, used) in decisions.iter().zip(["40%", "45%", "52%"]) {
        assert!(decision.contains("waiting"), "{decision}");
        assert!(decision.contains(used), "the figure it read: {decision}");
        assert!(decision.contains("5-hour"), "and which window: {decision}");
        assert!(decision.contains("threshold 80%"), "{decision}");
        assert!(decision.contains("under it"), "and why: {decision}");
    }
    assert!(
        printed.contains("Stopped."),
        "and it says it has stopped: {printed}"
    );
}

/// One Account watched continuously fits inside the hourly allowance; a Group
/// of them does not, and would make the size of a Group a scaling limit.
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

/// The interval is the policy: the endpoint allows roughly 28-30 reads an hour
/// per Account, and the loop has to leave room inside that for the
/// `perch status --refresh` somebody types while it runs.
///
/// Bounded from below as well as above, because a number that only had to stay
/// under the allowance would be satisfied by reading once an hour — which
/// spends nothing and notices a five-hour window filling up long after it has.
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

/// The whole point of the thing: quota runs out mid-task, and Perch moves you
/// without being asked.
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
    assert!(switched.contains("threshold 80%"), "{switched}");
    assert!(switched.contains(SECOND_EMAIL), "where it went: {switched}");
    assert!(
        switched.contains("most room"),
        "and why that Account won: {switched}"
    );

    assert_eq!(active(&host).as_deref(), Some(SECOND_EMAIL));
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SPARE),
        "and the Switch was a Switch: the incoming Credential is the live one"
    );
}

/// Everything the watcher does when it acts is a Switch, which means the
/// outgoing Credential is Captured into its own Profile first (ADR 0006).
/// Without that, a Rotation that happened while the Account was live is lost
/// the moment the watcher moves off it.
#[test]
fn acting_captures_the_outgoing_credential_before_it_writes_the_incoming_one() {
    let host = watching(&[86.0], 5.0);
    // A Rotation that happened while this Account was active: the live copy is
    // ahead of the one in its own Profile, and is the only good one there is.
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

/// A candidate's figures are read at the moment a decision is taken, not kept
/// warm. They are idle then by definition, so ADR 0005 permits Renewing them —
/// and reading them every round instead would spend every Account's allowance
/// to keep numbers fresh that are looked at once.
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

/// The watcher's one job is to be the thing with fresh numbers. A Switch made
/// on a cached figure is a Switch the user could have made themselves without
/// leaving a process running.
#[test]
fn a_reading_that_failed_holds_the_decision_rather_than_falling_back_to_the_cache() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_reply(USAGE_URL, 429, "{}")
        .with_interrupt_after(1);
    // A cached figure well over the threshold: acting on it would switch, and
    // acting on it is the whole of what this refuses to do.
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

/// The trace this whole rule is for: the Account fills past the threshold while
/// the endpoint is refusing to say so. Everything but the freshness of the
/// cached figure says to move — it is well over the threshold, and the Account
/// beside it is empty and waiting — and moving on it would be a Switch made on
/// evidence the user already had, which is `perch switch` and needs no loop.
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

/// A held decision costs nothing, but held every two and a half minutes for as
/// long as the loop is left running it costs an endpoint that is already
/// refusing twenty-four questions an hour. So the wait grows — and stops
/// growing, because the failure clears without announcing itself and the only
/// way the watcher finds out is by asking.
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

/// The back-off never spends more of the allowance than a loop that is working.
/// The endpoint with a 28-30 an hour budget is the one refusing, and a retry is
/// not the place to spend the room the interval left for the `perch status
/// --refresh` somebody types while the watcher runs.
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

/// A transient failure is recovered from at the ordinary cadence. The first
/// Refresh that works clears the whole of the back-off rather than winding it
/// down a step at a time, which would pace the watcher on something that has
/// stopped happening.
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

/// A reply that arrived is not the same as a figure that was read. Anthropic
/// answering with something Perch cannot make a Quota Window of is a Refresh
/// that failed — read as an Account with nothing used, it would be the one
/// reading that can never be over any threshold, so the watcher would go on
/// waiting through a crossing it was left running for.
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
    // A window shaped like one that will not say how full it is is named, so
    // the line says which window drifted rather than only that something did.
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

/// Running while Claude Code is working is the normal case for this command,
/// so the refusal that protects a running session has to hold under it: a
/// Renewal may Rotate, and a Rotation logs that session out mid-task (ADR 0005).
#[test]
fn a_live_profiles_token_is_never_renewed_to_get_a_figure() {
    let host = answering(watched(), SPARE_TOKEN, SECOND_EMAIL, &[5.0])
        .with_reply_to(PROFILE_URL, ACTIVE_TOKEN, 200, &profile_of(EMAIL))
        .with_interrupt_after(1);
    // The Account you are on, with an access token that has run out and a
    // client holding it.
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

/// Every candidate exhausted is an answer rather than a failure: the thing to
/// do about it is wait, which is what the loop is already doing. Stopping there
/// would end the watch at the exact moment it is most wanted.
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

/// The margin (ADR 0013). At an 80% threshold nothing is moved to unless it is
/// at 70% or better, so an Account that is only just emptier than the one you
/// are on is passed over — moving there would buy a few minutes and cost a
/// Capture, a Credential write, and the same decision again shortly.
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

/// The whole of what the margin is for. Two Accounts hovering either side of
/// the threshold, watched for as long as anyone would: without a margin this is
/// a Switch every round, each one a Capture and a Credential write, and a
/// client logged out at the end of it.
#[test]
fn two_accounts_hovering_either_side_of_the_threshold_do_not_ping_pong() {
    let host = watching_both(&[82.0, 79.0, 83.0, 81.0, 84.0, 80.0], &[78.0], 6);

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        printed.matches("switched").count(),
        0,
        "nothing here is worth moving to, and doing it anyway is the ping-pong: \
         {printed}"
    );
    assert_eq!(active(&host).as_deref(), Some(EMAIL));
    assert_eq!(
        decisions(&printed).len(),
        6,
        "and it said so every round: {printed}"
    );
}

/// A machine where the Account being watched fills up, the watcher moves off
/// it, and the Account it moved to fills up in turn — the trace that would
/// ping-pong if nothing paced it.
///
/// The Account left behind is roomy again by then, so the only thing standing
/// between the two Switches is the cooldown.
fn filling_up_one_after_the_other() -> FakeHost {
    // Rounds are 2m30s apart, so a fifteen-minute cooldown is six of them: the
    // Switch lands on the second round and the earliest the next one may is the
    // eighth.
    watching_both(&[40.0, 86.0, 20.0], &[5.0, 90.0], 8)
}

/// The cooldown (ADR 0013): a floor under how often the watcher acts, whatever
/// the figures do in between.
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
        assert!(decision.contains("threshold 80%"), "{decision}");
        assert!(decision.contains("15 minutes"), "the cooldown: {decision}");
    }
}

/// No return (ADR 0013): the Account a Switch just left is no candidate until
/// the cooldown has passed, and it is not read either.
///
/// What this trace shows is the pair working — nothing is read of the Account
/// just left while the cooldown holds, and it is landed on again the first
/// round after. It cannot separate the two rules, because the cooldown always
/// reaches the decision first; `Recently::barred` is pinned on its own by a
/// unit test in `src/watch.rs`.
#[test]
fn the_account_just_left_is_no_candidate_until_the_cooldown_has_passed() {
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
            // Six rounds watching the Account it moved to, five of them held
            // back by the cooldown with nowhere read.
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

/// The loop's cooldown is the loop's own: it lives in the running process and
/// is written down nowhere, so stopping `perch watch` and starting it again is
/// a person saying "go on then". A scheduled `--once` is the other case — the
/// sequence of invocations is the watcher there, and what paces it has to
/// outlive any one of them (ADR 0013).
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

/// A Group is free to say the watcher may act as often as the figures warrant,
/// and the settings that say so are its own.
#[test]
fn a_group_that_asks_for_no_cooldown_and_no_margin_may_move_every_round() {
    let host = watching_both(&[86.0, 20.0], &[79.0, 86.0], 3);
    for (key, value) in [
        ("watcher-cooldown-minutes", "0"),
        ("watcher-margin-percent", "0"),
        ("watcher-no-return", "false"),
    ] {
        config_set(&host, &["work", key, value])
            .0
            .expect("all three are the Group's to set");
    }

    let (result, printed) = run_watch(&host);

    result.expect("it was stopped");
    assert_eq!(
        printed.matches("switched").count(),
        2,
        "with nothing pacing it, it moves whenever the ranking says to: {printed}"
    );
    assert_eq!(
        active(&host).as_deref(),
        Some(EMAIL),
        "there and straight back: {printed}"
    );
}

/// A Switch that was turned away without changing anything is a decision that
/// did not land, not a machine to go and look at: whatever was holding the
/// Profile stops holding it, and the next round tries again.
#[test]
fn a_switch_the_machine_turned_away_is_said_and_the_loop_carries_on() {
    let host = watching(&[86.0, 88.0], 5.0);
    // A `perch run` against the Account being left: the Capture would write
    // into a Profile something else is holding (ADR 0027).
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
         refill early (ADR 0015) on a Switch that cannot happen — and throttles \
         the `perch status --refresh` the user types, at the moment the watcher \
         matters most"
    );
}

/// The other way the same question fails, which was going unanswered.
///
/// `refuse_if_live` refuses a Live Profile with `ProfileLive`, but it *fails*
/// with `ProbeRefused` when the `sessions` directory is there and will not be
/// read — the root-owned one a `sudo claude` leaves. The round asked it inside
/// an `if let Err(ProfileLive)`, and a pattern that does not match drops the
/// `Err` on the floor: no branch, no warning. So the round carried on to spend a
/// Renewal on every candidate, ranked them, and met the identical refusal in the
/// Switch — having spent exactly the allowance the early ask exists to save.
#[test]
fn a_profile_whose_sessions_will_not_be_read_stops_the_round_before_it_spends_anything() {
    let host = watching(&[86.0], 5.0);
    // A `sessions` that is a regular file is `ENOTDIR` on a real filesystem:
    // doubt about what is running, rather than the absence that means nothing is.
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

/// A Switch the watcher discovers is impossible for good records why, the same
/// as the command would.
///
/// The two paths meet the same state — an Account whose Profile holds no
/// Credential at all — and only one of them was covered. A Quarantine the
/// watcher found and did not write down is one rediscovered on the next round,
/// and the round after that: the loop would go on choosing an Account it has
/// already watched fail, at a browser round trip each time, for as long as it
/// runs.
#[test]
fn an_account_the_watcher_finds_broken_is_recorded_rather_than_rediscovered() {
    let host = watching(&[86.0, 88.0], 5.0);
    // The Account it would move to has nothing in either store — a Rotation
    // that went missing, or a store somebody cleared out.
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
    // The second round has the Quarantine to read, so it never gets as far as
    // trying: nowhere to go rather than a Switch refused.
    let decisions = decisions(&printed);
    assert_eq!(decisions.len(), 2, "{printed}");
    assert!(
        decisions[1].contains("nowhere"),
        "the round after knows better than to try again: {}",
        decisions[1]
    );
}

/// And it names the Account the way the user does.
///
/// `perch watch` prints a decision line and no Accounts at all, so the
/// Quarantine note is the only sentence on the screen about the candidate that
/// was passed over — and it was the one place in Perch naming an Account by raw
/// address while `perch list`, `perch status` and every refusal called it
/// ``someone@example.com (as `spare`)``. An Alias is what somebody gave the
/// Account so they would never have to read its address.
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

/// Ctrl-C stops it and leaves nothing behind. That is a property of the loop
/// holding nothing across a wait, not of the handler: the registry lock is
/// taken and given back inside each round.
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

/// The decisions go to standard output and nowhere else. A rotated logfile is
/// what a daemon needs because nobody is watching; redirection is this user's
/// call.
#[test]
fn the_decision_log_is_standard_output_and_no_file_is_written() {
    let host = watching(&[40.0, 45.0], 5.0);

    let (_, printed) = run_watch(&host);

    assert_eq!(decisions(&printed).len(), 2);

    // Named rather than filtered by what a logfile might be called. Asked as
    // "nothing with `log` or `watch` in its name", a watcher writing
    // `~/.config/perch/decisions.jsonl` or `history.txt` passed — and ADR
    // 0013's property is about which files are written, not what they are
    // called.
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
            // The registry, and the copy beside it its atomic write goes
            // through on the way.
            .all(|path| path
                .to_string_lossy()
                .starts_with(&*registry.to_string_lossy())),
        "the registry is the only file a watcher writes, and it writes that \
         because a Switch happened: {written:?}"
    );
}

/// `cycle-ungrouped` lets a bare `perch switch` Cycle among the Accounts in no
/// Group. It grants the watcher nothing on its own: permission to Switch when
/// you ask and permission to Switch while nobody is looking are different
/// grants, and the second one has to be given here too.
#[test]
fn an_account_in_no_group_is_not_watched_however_freely_it_may_be_cycled() {
    let host = watching(&[40.0], 5.0);
    move_to_group(&host, EMAIL, "none")
        .0
        .expect("the Account leaves the Group");
    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("ungrouped Accounts are interchangeable");

    let (result, _) = run_watch(&host);

    let refusal = result.expect_err("there is nothing here it may act on");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal
            .to_string()
            .contains("perch config set ungrouped watcher-may-act true"),
        "the Scope is addressed the way every other one is: {refusal}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and it exits rather than idling forever having decided nothing"
    );
}

/// **ADR 0017, amended.** `watcher-may-act` deliberately does not Inherit into
/// the Ungrouped Scope: it is gated behind `cycle-ungrouped`, so the watcher
/// acts on ungrouped Accounts only where both are on.
///
/// A Global "yes" is somebody saying "Cycle my work Groups unattended", and
/// Inheriting it straight through would authorise moving them onto their
/// personal subscription — the failure Groups exist to prevent, arriving by a
/// route nobody typed. This is the case most likely to be "fixed" by somebody
/// tidying the layering into uniformity, which is why it names the ADR.
#[test]
fn a_watcher_turned_on_at_global_leaves_ungrouped_accounts_alone() {
    let host = watching(&[99.0], 1.0);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none")
            .0
            .expect("the Account leaves the Group");
    }
    config_set(&host, &["watcher-may-act", "true"])
        .0
        .expect("a statement about the Groups this person runs");

    let (result, _) = run_watch(&host);

    let refusal = result.expect_err("that yes was not about these Accounts");
    assert_eq!(refusal.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    assert!(
        refusal.to_string().contains("cycle-ungrouped"),
        "the declaration that is still missing is named: {refusal}"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "and nothing moved underneath them, at 99% used with an empty Account \
         beside it (ADR 0017)"
    );
    assert!(host.sent_to(USAGE_URL).is_empty());
}

/// With both declarations made, the Ungrouped Scope is watched like any other —
/// which is the point of its being a Scope rather than a fallthrough.
#[test]
fn a_watcher_acts_among_ungrouped_accounts_once_both_declarations_are_made() {
    let host = watching(&[99.0], 1.0);
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "none")
            .0
            .expect("the Account leaves the Group");
    }
    config_set(&host, &["cycle-ungrouped", "true"])
        .0
        .expect("they are interchangeable");
    config_set(&host, &["ungrouped", "watcher-may-act", "true"])
        .0
        .expect("and the watcher may act on them");

    let (result, printed) = run_watch(&host);

    result.expect("a watcher that switched is not a failure");
    assert!(printed.contains("switched"), "{printed}");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

/// A Group only ever changes underneath you because you said it could, and the
/// saying is off by default.
#[test]
fn a_group_that_has_not_said_the_watcher_may_act_is_not_watched() {
    let host = watching(&[40.0], 5.0);
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");

    let (result, _) = run_watch(&host);

    let refusal = result.expect_err("nothing may be acted on");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(refusal.to_string().contains("watcher-may-act"), "{refusal}");
    assert!(host.sent_to(USAGE_URL).is_empty(), "and nothing was read");
}

/// Permission is asked for every round rather than only at the first, because
/// it can stop being given while the loop is sleeping: a `perch switch` typed
/// in another terminal can leave an ungrouped Account active, and a watcher
/// still watching it would be watching an Account nothing carries permission to
/// act on. It stops on the message it would have refused to start on.
#[test]
fn a_switch_onto_an_ungrouped_account_stops_the_loop_that_was_already_running() {
    let host = watching(&[40.0, 45.0], 5.0).once_while_waiting(|host| {
        move_to_group(host, SECOND_EMAIL, "none")
            .0
            .expect("the other Account leaves the Group");
        run_switch(host, SECOND_EMAIL)
            .0
            .expect("somebody Switches onto it between two rounds");
    });

    let (result, printed) = run_watch(&host);

    let refusal = result.expect_err("the Group that said it may act is gone");
    assert_eq!(refusal.exit_code(), EXIT_NOT_INTERCHANGEABLE);
    assert!(
        refusal.to_string().contains("perch group move"),
        "{refusal}"
    );
    assert!(refusal.to_string().contains("watcher-may-act"), "{refusal}");
    assert_eq!(
        decisions(&printed).len(),
        1,
        "the round before it, and no round after: {printed}"
    );
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN.to_string()],
        "and it stopped before spending a read on a decision it may not take"
    );
}

/// The other grant, withdrawn the same way: a Group can be told the watcher may
/// no longer act on it while a watcher is acting on it, and permission taken
/// back is a loop to be stopped rather than one to be left running.
#[test]
fn a_group_that_takes_the_permission_back_stops_the_watcher_it_had_given_it() {
    let host = watching(&[40.0, 45.0], 5.0).once_while_waiting(|host| {
        config_set(host, &["work", "watcher-may-act", "false"])
            .0
            .expect("the Group takes the permission back between two rounds");
    });

    let (result, printed) = run_watch(&host);

    let refusal = result.expect_err("nothing may be acted on any more");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(refusal.to_string().contains("watcher-may-act"), "{refusal}");
    assert_eq!(
        decisions(&printed).len(),
        1,
        "the round before it, and no round after: {printed}"
    );
    assert_eq!(
        asked_by(&host),
        vec![ACTIVE_TOKEN.to_string()],
        "and it stopped before spending a read on a decision it may not take"
    );
}

/// The marker a Run writes names this process, and the watcher is this process
/// — so a fixture that leaves one behind would make the watcher look like a
/// client of itself. Asserted so the fixture above cannot quietly stop meaning
/// what it says.
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

/// The other half of that rule: a Switch that made the incoming Credential
/// live and *then* failed has left the machine part way through, and the loop
/// stops on it. A watcher that carried on watching would be deciding what to do
/// next about a machine nobody has looked at yet.
#[test]
fn a_switch_that_changed_something_and_then_failed_stops_the_loop() {
    let host = watching(&[86.0, 40.0], 5.0);
    // The Credential is written and the Identity beside it cannot be, which is
    // ADR 0006's crash between two writes arriving as a failed write.
    let host = host.with_unwritable_file(IDENTITY_PATH, "read-only file system");

    let (result, printed) = run_watch(&host);

    let stopped = result.expect_err("the machine is part way through a Switch");
    // The file rather than the path it is at: a Windows Perch joins the two
    // with the other separator, so the spelling of the path is the platform's
    // and only the name is Perch's to promise.
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
         (ADR 0006)"
    );
}

/// The loop takes the registry lock every round, waits about four seconds for
/// it, and used to propagate the refusal — which ended `perch watch`.
///
/// That made a `perch status --refresh` in another terminal able to stop the
/// watcher. It holds the exclusive lock across every Renewal and every read it
/// makes, comfortably more than four seconds over a Group, and the machine then
/// went unwatched until somebody noticed the process was gone. ADR 0013 stops
/// the loop for a Switch that changed something and then failed; contention
/// over a lock changed nothing at all.
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

/// A 200 carrying no Quota Window at all. The status says the endpoint was
/// happy, so the temptation is to treat the body as a reading — but an answer
/// that says nothing about how full an Account is says nothing about whether to
/// leave it. The round holds rather than reading an empty body as an empty
/// Account and switching off a subscription that was never measured.
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

/// The Account being watched is over its threshold and the one it would move to
/// cannot be read. There is nowhere to go, which is a decision rather than a
/// failure — but the line has to carry why the candidate was not considered, or
/// it reads as though every Account were genuinely full.
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

/// A check that could not take the registry says so on standard output, like
/// every other outcome it has.
///
/// The whole of what `--once` promises is that "the line goes to standard
/// output for cron to capture, and what was decided goes into the exit code" —
/// and the one outcome most likely to recur was the one with no line. Another
/// `perch` holding the registry is ordinary: `perch status --refresh` holds it
/// across every Renewal and every read it makes, comfortably longer than
/// `lock::take` waits. Raised rather than said, it went to standard error by way
/// of `main`, so a cron job capturing standard output got a line for every
/// outcome except that one.
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
