//! Behaviour: what Perch does with an Account whose Credential has stopped
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
        is_held(&host, EMAIL) && registry_of(&host).active.as_deref() == Some(EMAIL),
        "the Account is kept and stays active — a Quarantine is a state, not a \
         removal: {printed}"
    );
    assert!(
        printed.contains("Quarantined") && printed.contains("perch relogin"),
        "the reason and the repair are both said where it is found: {printed}"
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

    let (result, printed) = run_status_group_refresh(&host, false);

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

    let (result, printed) = run_status_group_refresh(&host, false);

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
    assert!(printed.contains("Quarantined"), "{printed}");
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
    let overflow = &document["accounts"][1];
    assert_eq!(overflow["quarantined"]["reason"], "rotation-lost");

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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
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

    run_status_group_refresh(&host, false)
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
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
