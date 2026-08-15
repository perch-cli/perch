//! Behaviour: what `perch status --refresh` fetches, what it keeps, and what
//! it refuses to do to get a figure.
//!
//! This is the only command in v1 that spends network budget (ADR 0015), so the
//! tests are as much about what does not go out as about what comes back: no
//! read for an Account that is not being shown, no renewal of a Credential
//! something else is holding (ADR 0005), and no figure recorded against an
//! Account whose Credential it was not.

mod common;

use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use common::*;
use perch::anthropic::{self, BETA, PROFILE_URL, TOKEN_URL, USAGE_URL};
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::{FakeHost, Host};

const CONFIG_LOCK: &str = "/Users/someone/.claude.json.lock";

/// A Credential with an hour left on it at the clock every fixture here runs
/// at — one nothing needs to renew.
const FRESH: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-fresh","refreshToken":"sk-ant-ort01-fresh","expiresAt":1785848400000,"subscriptionType":"pro"}}"#;
const FRESH_TOKEN: &str = "sk-ant-oat01-fresh";

/// The same for the second Account, so a reply can be arranged for one and not
/// the other.
const SECOND_FRESH: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-second","refreshToken":"sk-ant-ort01-second","expiresAt":1785848400000,"subscriptionType":"max"}}"#;
const SECOND_TOKEN: &str = "sk-ant-oat01-second";

/// A Credential whose access token ran out twenty minutes ago.
const SPENT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1785843600000,"subscriptionType":"pro"}}"#;

/// What the token endpoint hands back: a fresh access token, and a Rotation.
const RENEWED: &str = r#"{"access_token":"sk-ant-oat01-renewed","refresh_token":"sk-ant-ort01-rotated","expires_in":28800}"#;
const RENEWED_TOKEN: &str = "sk-ant-oat01-renewed";

/// Every Quota Window an Account has: the five-hour one, the seven-day one, and
/// one per model.
const USAGE: &str = r#"{
  "five_hour": {"utilization": 42, "resets_at": "2026-08-04T14:30:00Z"},
  "seven_day": {"utilization": 18, "resets_at": "2026-08-09T00:00:00Z"},
  "seven_day_opus": {"utilization": 3, "resets_at": "2026-08-09T00:00:00Z"}
}"#;

fn profile_of(email: &str) -> String {
    format!(r#"{{"account": {{"email_address": "{email}"}}}}"#)
}

/// The keychain namespace of the second Account's Profile, derived the way
/// every command derives it. The spelling of the directory decides the hash,
/// and a Windows build joins paths with the other separator — so a fixture
/// spelling the path by hand would plant the item in a namespace nothing
/// reads.
fn second_service(host: &FakeHost) -> String {
    store_of(host, SECOND_EMAIL).keychain_service
}

/// A machine holding two Accounts, the active one carrying a Credential that is
/// good, with Anthropic answering about it.
fn ready() -> FakeHost {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE);
    host.forget_effects();
    host
}

/// Two Accounts declared interchangeable, so `--group` reads both of them.
fn two_accounts_in_a_group() -> FakeHost {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    host
}

fn cached_windows(host: &FakeHost, email: &str) -> Vec<perch::registry::WindowUtilization> {
    registry_of(host)
        .account(email)
        .expect("an Account Perch holds")
        .utilization
        .clone()
        .map(|cached| cached.windows)
        .unwrap_or_default()
}

fn at(hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 4, hour, minute, 0).unwrap()
}

/// A write of the Credential every client reads.
fn wrote_the_live_credential(effect: &Effect) -> bool {
    matches!(effect, Effect::KeychainSet { service, .. } if service == DEFAULT_SERVICE)
}

#[test]
fn refreshing_reads_current_utilization_and_shows_it_as_read_just_now() {
    let host = ready();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the read works");
    assert!(printed.contains("5-hour"), "{printed}");
    assert!(printed.contains("42%"), "{printed}");
    assert!(printed.contains("as of just now"), "{printed}");
}

#[test]
fn every_quota_window_is_kept_with_when_it_was_observed_and_when_it_resets() {
    let host = ready();

    run_status_refresh(&host, false).0.expect("the read works");

    let windows = cached_windows(&host, EMAIL);
    let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
    assert_eq!(
        named,
        vec!["5-hour", "7-day", "7-day-opus"],
        "the per-model weekly windows are recorded too"
    );
    assert_eq!(windows[0].used_percent, 42.0);
    assert_eq!(
        windows[0].resets_at,
        Some(at(14, 30)),
        "a figure carries when its window next resets"
    );
}

#[test]
fn json_carries_an_observation_time_on_every_figure_it_just_read() {
    let host = ready();

    let (result, printed) = run_status_refresh(&host, true);

    result.expect("the read works");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    let windows = document["utilization"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 3);
    for window in windows {
        assert_eq!(window["observed_seconds_ago"], 0);
        assert!(
            window["observed_at"]
                .as_str()
                .is_some_and(|at| at.starts_with("2026-08-04T12:00:00")),
            "{window}"
        );
    }
    assert_eq!(document["refresh"]["accounts"][0]["email"], EMAIL);
    assert_eq!(document["refresh"]["accounts"][0]["outcome"], "observed");
    assert_eq!(document["refresh"]["kept"], true);
}

#[test]
fn a_read_carries_the_access_token_and_the_beta_the_endpoint_is_behind() {
    let host = ready();

    run_status_refresh(&host, false).0.expect("the read works");

    let read = host.sent_to(USAGE_URL);
    assert_eq!(read.len(), 1, "one Account shown, one read spent");
    assert_eq!(read[0].bearer(), Some(FRESH_TOKEN));
    assert_eq!(read[0].header("anthropic-beta"), Some(BETA));
}

#[test]
fn no_command_but_refresh_makes_a_network_call() {
    let host = ready();

    run_status(&host, false).0.expect("status renders");
    run_status(&host, true).0.expect("status renders as JSON");
    run_list(&host, false).0.expect("list renders");
    run_status_group(&host, false).0.expect("the group renders");

    assert!(
        host.http_calls().is_empty(),
        "only `--refresh` may spend the hourly allowance: {:?}",
        host.http_calls()
    );
}

#[test]
fn a_refresh_nobody_asked_for_is_not_reported_as_one_that_went_well() {
    let host = ready();

    let (result, printed) = run_status(&host, true);

    result.expect("status renders");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert!(document["refresh"].is_null());
}

#[test]
fn a_credential_that_has_run_out_is_renewed_and_the_rotation_is_written_back() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = host
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply_to(PROFILE_URL, RENEWED_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, RENEWED_TOKEN, 200, USAGE);
    host.forget_effects();

    let (result, _) = run_status_refresh(&host, false);

    result.expect("the read works once the Credential is renewed");
    let renewal = host.sent_to(TOKEN_URL);
    assert_eq!(renewal.len(), 1);
    let asked = renewal[0].body.clone().expect("a renewal is a POST");
    assert!(asked.contains("sk-ant-ort01-spent"), "{asked}");
    assert!(asked.contains(anthropic::CLIENT_ID), "{asked}");

    let stored = host
        .keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
        .expect("the Credential is still there");
    assert!(stored.contains(RENEWED_TOKEN), "{stored}");
    assert!(
        stored.contains("sk-ant-ort01-rotated"),
        "the Rotated refresh token replaces the retired one: {stored}"
    );
    assert!(!stored.contains("sk-ant-ort01-spent"), "{stored}");
    assert!(
        stored.contains(r#""subscriptionType":"pro""#),
        "what Claude Code recorded about the Account survives: {stored}"
    );
}

#[test]
fn the_rotation_is_written_back_inside_claude_codes_locks() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = host
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply_to(PROFILE_URL, RENEWED_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, RENEWED_TOKEN, 200, USAGE);
    host.forget_effects();

    run_status_refresh(&host, false).0.expect("the read works");

    let effects = host.effects();
    let lock = PathBuf::from(CONFIG_LOCK);
    let took = effects
        .iter()
        .position(|effect| *effect == Effect::Took(lock.clone()))
        .expect("Claude Code's own lock is taken");
    let wrote = effects
        .iter()
        .position(wrote_the_live_credential)
        .expect("the renewed Credential is written");
    let gave_back = effects
        .iter()
        .position(|effect| *effect == Effect::RemovedDir(lock.clone()))
        .expect("the lock is given back");

    assert!(
        took < wrote && wrote < gave_back,
        "a Rotation is written back under the lock, not beside it: {effects:?}"
    );
}

#[test]
fn a_credential_a_client_is_holding_is_never_renewed() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = client_running_against(host, "/Users/someone/.claude", 4242)
        .with_reply(TOKEN_URL, 200, RENEWED);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a Credential Perch may not touch is not a failed command");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "renewing a Credential a running client holds would log it out (ADR 0005)"
    );
    assert!(printed.contains("4242"), "{printed}");
    assert!(printed.contains("cached figure"), "{printed}");
}

/// A Profile with a Run against it is Live by Perch's own marker (ADR 0027),
/// and the rule that follows is the same one: the Credential in there belongs to
/// the client holding it until that client exits.
#[test]
fn a_credential_a_run_is_holding_is_never_renewed_either() {
    let host = two_accounts_in_a_group();
    // The active Account is fresh, so the only Credential anything here could
    // want to renew is the one the Run is holding.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    host.set_keychain_item(&second_service(&host), LOGIN_NAME, SPENT);
    a_run_against(&host, SECOND_EMAIL, host.now());
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        .with_reply(TOKEN_URL, 200, RENEWED);
    host.forget_effects();

    let (result, printed) = run_status_group_refresh(&host, false);

    result.expect("a Credential Perch may not touch is not a failed command");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "renewing under a Run would log that client out mid-task (ADR 0005)"
    );
    assert!(printed.contains(&THIS_PROCESS.to_string()), "{printed}");
    assert!(printed.contains("cached figure"), "{printed}");
}

/// The same, for the Account that is *active*, where the copy a refresh would
/// renew and the copy the Run is holding are two different files.
///
/// `perch run <the active account>` points a client at that Account's own
/// Profile, while the live copy sits in the Default Profile. A Rotation retires
/// the refresh token for the Account rather than for a file, so renewing the
/// live copy kills the one the Run is using just the same — and checking only
/// the directory being written would miss it entirely (ADR 0027).
#[test]
fn a_run_against_the_active_account_stops_its_live_credential_being_renewed() {
    let host = two_accounts_in_a_group();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    a_run_against(&host, EMAIL, host.now());
    let host = host.with_reply(TOKEN_URL, 200, RENEWED);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a Credential Perch may not touch is not a failed command");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "the Run holds a copy of this very Credential: {:?}",
        host.sent_to(TOKEN_URL)
    );
    assert!(printed.contains(&THIS_PROCESS.to_string()), "{printed}");
    assert!(printed.contains("cached figure"), "{printed}");
}

/// And the half of ADR 0005 that is not a refusal: an Account with a client
/// running has a fresh access token by that ADR's own reasoning, so its
/// Utilization is readable without renewing anything. A Run is not a blindfold.
#[test]
fn utilization_for_a_live_account_is_read_without_a_renewal() {
    let host = two_accounts_in_a_group();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    host.set_keychain_item(&second_service(&host), LOGIN_NAME, SECOND_FRESH);
    a_run_against(&host, SECOND_EMAIL, host.now());
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        .with_reply_to(PROFILE_URL, SECOND_TOKEN, 200, &profile_of(SECOND_EMAIL))
        .with_reply_to(USAGE_URL, SECOND_TOKEN, 200, USAGE);
    host.forget_effects();

    // Stated rather than assumed. Everything below is about an Account that is
    // Live, and a fixture that quietly stopped being Live would let all of it
    // pass while proving nothing.
    assert_eq!(
        perch::probe::live_clients(
            &host,
            &perch::registry::profile_dir_for(&host, SECOND_EMAIL).expect("home is known"),
            &perch::probe::Installed::unknown(CLAUDE_VERSION),
        )
        .expect("the marker corroborates"),
        vec![THIS_PROCESS],
        "the Account this is about has a Run against it"
    );

    run_status_group_refresh(&host, false)
        .0
        .expect("the reads work");

    assert_eq!(
        cached_windows(&host, SECOND_EMAIL).len(),
        3,
        "the Live Account's figures were read like anybody else's"
    );
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        2,
        "both Accounts were read, the Live one included"
    );
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "and nothing was renewed to get them"
    );
}

#[test]
fn a_throttled_read_falls_back_to_the_cached_figure_and_still_succeeds() {
    let host = ready();
    run_status_refresh(&host, false)
        .0
        .expect("the first read works");

    // The allowance is spent, and does not refill early (ADR 0015).
    host.reply(USAGE_URL, Some(FRESH_TOKEN), 429, "");
    host.set_now(at(12, 3));

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a throttle degrades the display rather than failing");
    assert!(printed.contains("42%"), "the cached figure still shows");
    assert!(printed.contains("as of 3m ago"), "{printed}");
    assert!(printed.contains("rate-limiting"), "{printed}");
}

#[test]
fn a_throttle_is_named_as_one_in_json() {
    let host = ready();
    run_status_refresh(&host, false)
        .0
        .expect("the first read works");
    host.reply(USAGE_URL, Some(FRESH_TOKEN), 429, "");

    let (result, printed) = run_status_refresh(&host, true);

    result.expect("a throttle is not a failure");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["refresh"]["accounts"][0]["outcome"], "throttled");
    assert_eq!(
        document["utilization"]["windows"][0]["used_percent"], 42.0,
        "the figure that is shown is the one that was cached"
    );
}

#[test]
fn a_refresh_that_fails_for_one_account_still_reads_the_others() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("moved");
    move_to_group(&host, SECOND_EMAIL, "work").0.expect("moved");
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    host.set_keychain_item(&second_service(&host), LOGIN_NAME, SECOND_FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        // Turned away, so the Account is renewed and asked once more — and
        // turned away again, which is where it runs out of things to try.
        .with_reply_to(PROFILE_URL, SECOND_TOKEN, 401, "")
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply_to(PROFILE_URL, RENEWED_TOKEN, 401, "");
    host.forget_effects();

    let (result, printed) = run_status_group_refresh(&host, false);

    result.expect("one Account Perch cannot read is not a failed command");
    assert!(
        printed.contains("42%"),
        "the Account that could be read was read: {printed}"
    );
    assert!(
        printed.contains(SECOND_EMAIL) && printed.contains("would not accept"),
        "the Account that could not be read is named, with why: {printed}"
    );
    assert!(cached_windows(&host, SECOND_EMAIL).is_empty());
    assert!(
        !registry_of(&host)
            .account(SECOND_EMAIL)
            .expect("an Account Perch holds")
            .quarantine
            .is_some(),
        "and a token Anthropic issued and then refused is not the Account's \
         fault: the refresh token bought the renewal, so it is live"
    );
}

/// A Credential that never says when it expires is one `usable_at` takes at its
/// word for ever, so nothing ever concluded a Renewal was due — and a rejected
/// access token was then a dead end that every later command met again.
///
/// Reachable rather than hypothetical: `anthropic::renew` yields no expiry for
/// four different replies, and what is stored after one of those carries no
/// `expiresAt` at all.
#[test]
fn a_credential_that_will_not_say_when_it_expires_is_renewed_once_anthropic_refuses_it() {
    const NO_EXPIRY: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-undated","refreshToken":"sk-ant-ort01-undated","subscriptionType":"pro"}}"#;

    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, NO_EXPIRY);
    let host = host
        .with_reply_to(PROFILE_URL, "sk-ant-oat01-undated", 401, "")
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply_to(PROFILE_URL, RENEWED_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, RENEWED_TOKEN, 200, USAGE);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the refusal is answered by a Renewal rather than by giving up");
    let renewal = host.sent_to(TOKEN_URL);
    assert_eq!(renewal.len(), 1, "renewed once, off the rejection alone");
    assert!(
        renewal[0]
            .body
            .as_ref()
            .expect("a renewal is a POST")
            .contains("sk-ant-ort01-undated"),
        "with the refresh token it holds"
    );
    assert!(printed.contains("42%"), "and the figures are read: {printed}");
}

#[test]
fn a_refresh_reads_only_the_accounts_it_is_about_to_show() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    move_to_group(&host, EMAIL, "work").0.expect("moved");
    move_to_group(&host, SECOND_EMAIL, "work").0.expect("moved");
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    host.set_keychain_item(&second_service(&host), LOGIN_NAME, SECOND_FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        .with_reply_to(PROFILE_URL, SECOND_TOKEN, 200, &profile_of(SECOND_EMAIL))
        .with_reply_to(USAGE_URL, SECOND_TOKEN, 200, USAGE);
    host.forget_effects();

    run_status_refresh(&host, false).0.expect("the read works");
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        1,
        "`perch status --refresh` is about the Account you are on"
    );

    run_status_group_refresh(&host, false)
        .0
        .expect("the reads work");
    assert_eq!(
        host.sent_to(USAGE_URL).len(),
        3,
        "`--group` reads every Account it offers as a landing place"
    );
    assert_eq!(cached_windows(&host, SECOND_EMAIL).len(), 3);
}

#[test]
fn figures_are_not_recorded_against_an_account_the_credential_is_not_for() {
    // Somebody ran `claude` and logged in directly, so the live Credential is
    // no longer the Account Perch has recorded as active.
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(SECOND_EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command still answers");
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "no read is spent on a figure that could not be recorded anywhere"
    );
    assert!(printed.contains(SECOND_EMAIL), "{printed}");
    assert!(printed.contains("never observed"), "{printed}");
    assert!(cached_windows(&host, EMAIL).is_empty());
}

/// An outage on the profile endpoint is not permission to skip the question it
/// answers.
///
/// ADR 0019 carves out one thing and one thing only: "drift in a reply Perch
/// reads for reassurance is no reason to stop reading Utilization at all". A 503
/// is not drift. It says nothing about who the live Credential belongs to, and
/// `/api/oauth/usage` going on answering while `/api/oauth/profile` does not is
/// exactly what an incident looks like.
///
/// Folded in with drift, that switched the ownership check off for the duration:
/// somebody who had logged in as a second Account directly would have had that
/// Account's figures cached under the one Perch records as active. Figures under
/// the wrong Account do not look wrong — they look like quota that Account never
/// spent, which is what a Cycle ranks on. The one kind of wrong answer this
/// design cannot afford is a plausible one.
#[test]
fn an_outage_on_the_profile_endpoint_records_nothing_rather_than_guessing() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 503, "upstream unavailable")
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("an endpoint having a bad afternoon is not a failed command");
    assert!(
        cached_windows(&host, EMAIL).is_empty(),
        "nothing is recorded against an Account nothing corroborated: {printed}"
    );
    assert!(
        host.sent_to(USAGE_URL).is_empty(),
        "and no read is spent on a figure that could not be recorded anywhere"
    );
}

/// Drift, on the other hand, genuinely is no evidence either way — and is the
/// case ADR 0019 wrote the carve-out for. A profile reply whose shape this build
/// does not recognise must not stop a Utilization read: Anthropic changing a
/// field name would otherwise take every figure in Perch with it.
#[test]
fn a_profile_reply_this_build_does_not_recognise_still_lets_the_figures_be_read() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, "<html>hello</html>")
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command answers");
    assert!(
        printed.contains("42%"),
        "the figures were read all the same: {printed}"
    );
    assert!(!cached_windows(&host, EMAIL).is_empty());
}

/// Whoever was holding Claude Code's lock while Perch waited for it may have
/// renewed the very Credential Perch was about to renew. So the Credential is
/// read again once the lock is held, and a Rotation that has already happened
/// is used rather than spent again — a second Renewal would hand back a token
/// the other holder's Rotation has already invalidated (ADR 0006).
#[test]
fn a_credential_renewed_while_perch_waited_for_the_lock_is_not_renewed_again() {
    let host = machine_with_two_accounts();
    let now = host.now();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = host
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_of(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        // Somebody else is holding Claude Code's lock, so Perch waits for it.
        .with_dir_held_since(CONFIG_LOCK, now)
        .once_while_waiting(|host| {
            // And while Perch waits, they renew it and give the lock back.
            host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
            host.remove_dir_all(std::path::Path::new(CONFIG_LOCK))
                .expect("they give the lock back");
        });
    host.forget_effects();

    let (result, _) = run_status_refresh(&host, false);

    result.expect("the read works off the Credential that is now there");
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "the re-read under the lock found a usable Credential, so nothing was \
         renewed a second time"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(FRESH),
        "and the other holder's Rotation was left exactly where it was"
    );
}

/// A refresh that failed outright, in the form a script parses. The reason has
/// to travel with it: an outcome of "failed" with no detail is one nothing can
/// be done about without running the command again by hand.
#[test]
fn a_refresh_that_failed_is_named_as_one_in_json_with_the_reason() {
    let host = ready();
    run_status_refresh(&host, false)
        .0
        .expect("the first read works");
    host.reply(USAGE_URL, Some(FRESH_TOKEN), 500, "the endpoint is unwell");

    let (result, printed) = run_status_refresh(&host, true);

    result.expect("a failed read still reports the cached figure");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["refresh"]["accounts"][0]["outcome"], "failed");
    assert!(
        document["refresh"]["accounts"][0]["detail"].is_string(),
        "a failure with no reason is one nobody can act on: {printed}"
    );
    assert_eq!(
        document["utilization"]["windows"][0]["used_percent"], 42.0,
        "and the figure shown is the one that was cached"
    );
}
