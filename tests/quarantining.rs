//! Behavior: what Perch does with an Account whose Credential has stopped
//! working for good.
//!
//! ADR 0006 makes this the terminal state of the capture design rather than a
//! feature: a Rotation lost between two writes leaves an Account that no amount
//! of retrying will fix. The whole of the design is that such an Account is
//! *kept* — listed, named, and clearly broken, with the reason it broke — because
//! an Account that vanishes reads as data loss while a broken one reads as
//! something needing attention.
//!
//! So these tests are about three things: that the terminal failures are told
//! apart from the ones worth retrying, that the reason survives to every surface
//! a person or a script reads, and that nothing quietly switches to an Account
//! Perch knows will not work.

mod common;

use common::*;
use perch::anthropic::{PROFILE_URL, TOKEN_URL, USAGE_URL};
use perch::error::{EXIT_NOTHING_TO_DO, EXIT_QUARANTINED};
use perch::host::FakeHost;
use perch::registry::Quarantine;

/// A Credential whose access token ran out twenty minutes ago, so anything that
/// wants to ask Anthropic a question has to renew it first.
const SPENT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1785843600000,"subscriptionType":"pro"}}"#;

/// The same, from a login that never carried a refresh token: there is nothing
/// to buy a fresh access token with.
const SPENT_WITH_NOTHING_LEFT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","expiresAt":1785843600000,"subscriptionType":"pro"}}"#;

/// What the token endpoint hands back when it renews: a fresh access token, and
/// a Rotation that retires the refresh token Perch sent.
const RENEWED: &str = r#"{"access_token":"sk-ant-oat01-renewed","refresh_token":"sk-ant-ort01-rotated","expires_in":28800}"#;

/// The same renewal without a Rotation, which is the other half of what the
/// token endpoint does: a fresh access token, and the refresh token Perch sent
/// left exactly as live as it was.
const RENEWED_WITHOUT_ROTATION: &str =
    r#"{"access_token":"sk-ant-oat01-renewed","expires_in":28800}"#;

/// A Credential with an hour left on it at the clock these fixtures run at, so
/// nothing has to renew it to ask a question with it.
const FRESH: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-fresh","refreshToken":"sk-ant-ort01-fresh","expiresAt":1785848400000,"subscriptionType":"pro"}}"#;
const FRESH_TOKEN: &str = "sk-ant-oat01-fresh";

const USAGE: &str = r#"{"five_hour": {"utilization": 42}}"#;

/// The body OAuth gives a refresh token that has been retired, and the only
/// thing a refusal from the token endpoint may be read as a Quarantine. The
/// status it arrives under is deliberately not what decides that: a refusal
/// about Perch's own `client_id` wears the same statuses and is nobody's
/// Account's fault.
const RETIRED: &str = r#"{"error":"invalid_grant"}"#;

fn profile_naming(email: &str) -> String {
    format!(r#"{{"account": {{"email_address": "{email}"}}}}"#)
}

/// A machine whose active Account has to renew before it can be read, with
/// Anthropic answering the renewal however the test says.
fn about_to_renew(credential: &str) -> FakeHost {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, credential);
    host.forget_effects();
    host
}

fn is_held(host: &FakeHost, email: &str) -> bool {
    registry_of(host).account(email).is_some()
}

#[test]
fn a_credential_anthropic_will_not_renew_quarantines_its_account() {
    let host = about_to_renew(SPENT).with_reply(TOKEN_URL, 401, RETIRED);

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a refresh degrades the display rather than failing it (ADR 0018)");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::RenewalRejected),
        "a refresh token Anthropic turns down is one it has retired, and asking \
         again with it gets the same answer forever: {printed}"
    );
    assert!(
        is_held(&host, EMAIL) && registry_of(&host).active().whose() == Some(EMAIL),
        "the Account is kept and stays active — a Quarantine is a state, not a \
         removal: {printed}"
    );
    assert_eq!(
        printed.matches("perch relogin").count(),
        1,
        "the reason and the repair are said where it is found, and said once — \
         the refresh used to remark on the Quarantine it had just recorded and \
         then the Account's own line said the whole thing again: {printed}"
    );
    assert!(
        printed.contains("Anthropic would not renew its Credential"),
        "{printed}"
    );
}

#[test]
fn a_rotation_that_could_not_be_stored_quarantines_rather_than_reading_as_a_failed_write() {
    // The Credential is in the file store, which is about to become unwritable,
    // and the keychain will not take it either. Both halves of ADR 0006's
    // dangerous moment: Anthropic has retired the old refresh token and Perch
    // cannot keep the new one.
    let host = machine_with_two_accounts()
        .with_unwritable_file(CREDENTIALS_PATH, "no space left on device")
        .with_reply(TOKEN_URL, 200, RENEWED);
    host.forget_keychain_item(DEFAULT_SERVICE, LOGIN_NAME);
    host.set_file(CREDENTIALS_PATH, SPENT);
    host.lock_keychain("User interaction is not allowed");
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("one Account Perch cannot read is not a failed command");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::RotationLost),
        "the refresh token that was retired to get this one is gone, so a write \
         to try again would have nothing to try: {printed}"
    );
}

/// The other half of the same moment, and the one that must not read the same
/// way. A Renewal only sometimes Rotates: where Anthropic handed no new refresh
/// token over, the one Perch holds is untouched and still buys a token, so the
/// write that failed cost a cached access token and nothing else. Quarantining
/// there would take an Account out for good over a keychain somebody had
/// locked for the afternoon — and a `perch list <group> --refresh` would take
/// the whole Group out that way, each with a reason that is not true.
#[test]
fn a_renewal_that_rotated_nothing_is_a_failed_reading_rather_than_a_quarantine() {
    let host = machine_with_two_accounts()
        .with_unwritable_file(CREDENTIALS_PATH, "no space left on device")
        .with_reply(TOKEN_URL, 200, RENEWED_WITHOUT_ROTATION);
    host.forget_keychain_item(DEFAULT_SERVICE, LOGIN_NAME);
    host.set_file(CREDENTIALS_PATH, SPENT);
    host.lock_keychain("User interaction is not allowed");
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a refresh degrades the display rather than failing it (ADR 0018)");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "nothing was retired, so nothing is unrecoverable: {printed}"
    );
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(SPENT),
        "the store that refused the write is left holding what it held, which \
         is a Credential that still renews: {printed}"
    );
}

#[test]
fn a_credential_with_nothing_left_to_renew_with_quarantines() {
    let host = about_to_renew(SPENT_WITH_NOTHING_LEFT);

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command still answers");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::NoRefreshToken),
        "{printed}"
    );
    assert!(
        host.sent_to(TOKEN_URL).is_empty(),
        "and nothing was asked of Anthropic to find that out"
    );
}

#[test]
fn a_profile_that_holds_no_credential_at_all_quarantines_its_account() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("joined");
    }
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    // The second Account's Profile is empty — a Rotation that went missing
    // between two writes, or a store somebody cleared out.
    host.forget_keychain_item(&store_of(&host, SECOND_EMAIL).keychain_service, LOGIN_NAME);
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_naming(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE);

    let (result, printed) = run_list_in_refresh(&host, "work", false);

    result.expect("one Account Perch cannot read is not a failed command");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::NoCredential),
        "its own Profile is the only place its Credential ever lives, so an \
         empty one is an Account nothing Perch holds can recover: {printed}"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "and the Account that answered is untouched"
    );
}

#[test]
fn a_default_profile_that_holds_nothing_is_a_logout_rather_than_a_quarantine() {
    let host = machine_with_two_accounts();
    host.forget_keychain_item(DEFAULT_SERVICE, LOGIN_NAME);

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command still answers");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "somebody running `claude /logout` has not broken the Account — its own \
         Profile still holds a Credential to switch back to: {printed}"
    );
    assert!(printed.contains("logged out"), "{printed}");
}

#[test]
fn a_refusal_that_might_pass_is_not_a_quarantine() {
    let host = about_to_renew(SPENT).with_reply(TOKEN_URL, 429, "");

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command still answers");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "being rate-limited says nothing about whether the Credential works, and \
         an Account Quarantined for it would need a login it never needed: {printed}"
    );
}

/// The refusal that is about Perch rather than about the Account. A token
/// endpoint answers `invalid_client` with a 401, and `invalid_client` is a
/// statement about the `client_id` in the request — Perch's own, hard-coded,
/// and the same in every renewal it ever sends. Read as "this refresh token is
/// retired", one such answer walks a whole Group and Quarantines every Account
/// in it in a single pass, and only a browser login for each would clear it.
#[test]
fn a_renewal_refused_over_something_other_than_the_token_quarantines_nobody() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("joined");
    }
    // Both Accounts have to renew, and the endpoint turns both down without
    // ever saying the tokens are the problem.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    host.set_keychain_item(
        &store_of(&host, SECOND_EMAIL).keychain_service,
        LOGIN_NAME,
        SPENT,
    );
    let host = host.with_reply(TOKEN_URL, 401, r#"{"error":"invalid_client"}"#);

    let (result, printed) = run_list_in_refresh(&host, "work", false);

    result.expect("a refresh degrades the display rather than failing it");
    for email in [EMAIL, SECOND_EMAIL] {
        assert_eq!(
            quarantine_of(&host, email),
            None,
            "{email} was condemned for a refusal that was never about it: {printed}"
        );
    }
}

#[test]
fn a_quarantined_account_is_not_asked_again() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SPENT);
    let host = host
        .with_reply(TOKEN_URL, 200, RENEWED)
        .with_reply(
            PROFILE_URL,
            200,
            r#"{"account":{"email_address":"someone@example.com"}}"#,
        )
        .with_reply(USAGE_URL, 200, USAGE);
    quarantine(&host, EMAIL);
    host.forget_effects();

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("the command still answers");
    assert!(
        host.http_calls().is_empty(),
        "an Account Perch has written off does not spend an allowance that does \
         not refill early (ADR 0015) to be written off again: {:?}",
        host.http_calls()
    );
    assert_eq!(
        printed.matches("perch relogin").count(),
        1,
        "and the Quarantine it was not asked about is still shown, once: {printed}"
    );
}

#[test]
fn the_reason_is_shown_by_list_and_by_status() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL).0.expect("named");
    quarantine_for(&host, SECOND_EMAIL, Quarantine::RotationLost);

    let (listed, printed) = run_list(&host, false);

    listed.expect("the listing still renders");
    assert!(
        printed.contains("quarantined"),
        "the state stays in the table: {printed}"
    );
    assert!(
        printed.contains("Rotated") && printed.contains("perch relogin overflow@example.com"),
        "and the reason is written out under it, with the one command that puts \
         it right: {printed}"
    );

    let (as_json, document) = run_list(&host, true);
    as_json.expect("the listing renders as JSON");
    let document: serde_json::Value = serde_json::from_str(&document).expect("valid JSON");
    let overflow = &document["sections"][0]["accounts"][1];
    assert_eq!(
        overflow["quarantined"]["reason"], "rotation-lost",
        "{document}"
    );

    // And about the Account you are on, which is where `perch status` answers.
    quarantine_for(&host, EMAIL, Quarantine::RenewalRejected);
    let (status, printed) = run_status(&host, false);

    status.expect("status still renders");
    assert!(
        printed.contains("Quarantine") && printed.contains("would not renew"),
        "{printed}"
    );

    let (_, document) = run_status(&host, true);
    let document: serde_json::Value = serde_json::from_str(&document).expect("valid JSON");
    assert_eq!(
        document["active"]["quarantined"]["reason"],
        "renewal-rejected"
    );
}

#[test]
fn switching_to_a_quarantined_account_by_name_is_refused_with_a_code_of_its_own() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL).0.expect("named");
    quarantine(&host, SECOND_EMAIL);
    host.forget_effects();

    let (result, _) = run_switch(&host, "overflow");

    let error = result.expect_err("making a Credential live that does not work costs the session");
    assert_eq!(
        error.exit_code(),
        EXIT_QUARANTINED,
        "no other refusal in Perch is answered by logging in again, so no other \
         exit code says this: {error}"
    );
    assert!(error.to_string().contains("perch relogin"), "{error}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "and the Account being worked in is exactly where it was"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn an_account_quarantined_by_a_refresh_leaves_the_cycling_pool_from_that_moment() {
    // The Account with all the room is the one about to be found broken, so
    // before the refresh it is exactly where a Cycle would land.
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("joined");
    }
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);
    // The Account being worked in answers for itself; the other one has to renew
    // first, and Anthropic will not renew it.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, FRESH);
    host.set_keychain_item(
        &store_of(&host, SECOND_EMAIL).keychain_service,
        LOGIN_NAME,
        SPENT,
    );
    let host = host
        .with_reply_to(PROFILE_URL, FRESH_TOKEN, 200, &profile_naming(EMAIL))
        .with_reply_to(USAGE_URL, FRESH_TOKEN, 200, USAGE)
        .with_reply(TOKEN_URL, 401, RETIRED);

    run_list_in_refresh(&host, "work", false)
        .0
        .expect("a refresh degrades the display rather than failing it");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected)
    );

    let (cycled, _) = run_cycle(&host);

    let error = cycled.expect_err("the only Account with room turned out not to work");
    assert_eq!(
        error.exit_code(),
        EXIT_NOTHING_TO_DO,
        "a Cycle lands nowhere rather than on an Account Perch has just watched \
         Anthropic turn down: {error}"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

/// The Default Profile is not Perch's alone to write. Somebody who runs
/// `claude` and logs in directly leaves Perch's record of who is active behind,
/// and when *their* login later dies a refresh reads their dead Credential —
/// and would condemn the Account Perch believes is active, whose own Profile
/// still holds a perfectly good copy. ADR 0019 says a figure is recorded only
/// against the Account it was read for, and a Quarantine is a recording too.
#[test]
fn a_credential_that_may_be_somebody_elses_quarantines_nobody() {
    let host = about_to_renew(SPENT).with_reply(TOKEN_URL, 401, RETIRED);
    // The evidence that the live Credential is not the active Account's: Claude
    // Code writes the Identity beside the Credential it belongs to, and this
    // one names somebody else.
    host.set_file(
        IDENTITY_PATH,
        &IDENTITY_FILE.replace(EMAIL, "stranger@example.com"),
    );

    let (result, printed) = run_status_refresh(&host, false);

    result.expect("a refresh degrades the display rather than failing it");
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "the Account whose own Profile holds a good Credential is not condemned \
         for a login made outside Perch: {printed}"
    );
    assert!(
        printed.contains("perch switch"),
        "and the way out is named: {printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "its own copy is still there to switch back to"
    );
}

/// The half of a Landing that is *arriving* is asked about with the copy in its
/// own Profile — and that copy is the one a Rotation of the live one retires.
///
/// A Switch writes the Landing down, copies the arriving Account's Credential
/// into the Default Profile, and is killed before it records anything. That
/// Credential is now live, and a Claude Code Renewal against it may Rotate —
/// which retires the copy still sitting in the arriving Account's own Profile.
/// A Refresh then reads that Profile, is turned down, and used to write a
/// Quarantine that only a browser login clears, against an Account whose
/// working Credential was live on the same machine the whole time.
///
/// `holding` reads *both* halves out of their own Profiles during a Landing —
/// nothing is settled, so neither is the active one — so the `its_own_profile`
/// arm of the ADR 0019 guard let the terminal verdict straight through.
#[test]
fn a_switch_in_flight_is_not_evidence_that_the_account_arriving_is_broken() {
    let host = machine_with_two_accounts();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    // The copy in the arriving Account's own Profile, overtaken by a Rotation
    // of the live one: it has to renew before it can be read, and the refresh
    // token it renews with is the retired one.
    let second = store_of(&host, SECOND_EMAIL);
    host.set_keychain_item(&second.keychain_service, LOGIN_NAME, SPENT);
    let host = host.with_reply(TOKEN_URL, 401, RETIRED);
    host.forget_effects();

    let (result, printed) = run_list_refresh(&host, false);

    result.expect("a refresh degrades the display rather than failing it (ADR 0018)");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        None,
        "nothing terminal is recorded against an Account whose working \
         Credential may be the live one: {printed}"
    );
    assert!(
        printed.contains("in flight"),
        "and the Landing is named as the reason nothing was recorded: {printed}"
    );
}

/// A Quarantine reports the machine-readable reason and the failure underneath
/// as two keys, because they are two facts.
///
/// `detail` means the same thing everywhere Perch says it — "whatever the
/// failure underneath said" — and the refresh report put the *reason* there
/// instead. So a script read `outcome: "quarantined"` beside `detail:
/// "renewal-rejected"`, which is the outcome again in a second spelling, while
/// the one thing the key promises was sitting unread in the value the human line
/// prints.
#[test]
fn a_quarantine_in_json_carries_the_reason_and_the_failure_underneath_apart() {
    let host = about_to_renew(SPENT).with_reply(TOKEN_URL, 401, RETIRED);

    let (result, printed) = run_status_refresh(&host, true);

    result.expect("a refresh degrades the display rather than failing it");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    let attempt = &document["refresh"]["accounts"][0];
    assert_eq!(attempt["outcome"], "quarantined");
    assert_eq!(
        attempt["reason"], "renewal-rejected",
        "the reason a script branches on: {printed}"
    );
    assert_ne!(
        attempt["detail"], attempt["reason"],
        "and `detail` is the failure underneath rather than the reason again: \
         {printed}"
    );
}
