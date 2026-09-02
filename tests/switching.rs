//! Behavior: what a Switch does, in what order, and what it refuses.
//!
//! The order of the effects is asserted rather than inferred, because it is the
//! visible contract with a Claude Code running at the same time
//! (ADR a-switch-is-written-down-first): the Capture reaches the outgoing
//! Profile before the live store is written, the Identity is patched last, and
//! all of it happens inside Claude Code's own locks, in Claude Code's own order.

// Every path compared here comes out of the fake's effect log, spelled as the
// code under test wrote it: filtering that log by prefix asks which effects
// landed under a directory, and never whether a path on a machine is inside one.
#![allow(
    clippy::disallowed_methods,
    reason = "the fake's effect log, filtered by the prefix it was written under"
)]

mod common;

use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use common::*;
use perch::commands::add::AddArgs;
use perch::error::{
    EXIT_CONFLICT, EXIT_KEYCHAIN_UNAVAILABLE, EXIT_NOT_FOUND, EXIT_NOTHING_TO_DO,
    EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE, EXIT_QUARANTINED,
};
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{FakeHost, Refusing};
use perch::probe;
use perch::registry::{Active, Quarantine};

const REFRESH_LOCK: &str = "/Users/someone/.claude/.oauth_refresh.lock";
const LEGACY_LOCK: &str = "/Users/someone/.claude.lock";
const CONFIG_LOCK: &str = "/Users/someone/.claude.json.lock";

const FIRST_PROFILE: &str = "/Users/someone/.config/perch/profiles/someone-example-com";
const SECOND_PROFILE: &str = "/Users/someone/.config/perch/profiles/overflow-example-com";

/// The keychain namespace of an Account's Profile, derived the way every
/// command derives it. The spelling of the directory decides the hash, and a
/// Windows build joins paths with the other separator — so a fixture spelling
/// the path by hand would name a namespace nothing reads.
fn profile_service(host: &FakeHost, email: &str) -> String {
    store_of(host, email).keychain_service
}

/// What the live store holds right now — the Credential every client reads.
fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

/// A Credential that Anthropic Rotated while the outgoing Account was active,
/// so the copy in that Account's Profile is behind the live one and a Capture
/// has something to save.
const ROTATED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-rotated","refreshToken":"sk-ant-ort01-rotated","expiresAt":1790000000000,"subscriptionType":"pro"}}"#;

/// A Switch that a session marker did not hold back, asserted on what it did
/// rather than on its not having been refused: the live Credential is Rotated
/// first, so a Perch that judged the Profile idle and skipped the Capture is not
/// mistaken for one that switched.
fn assert_the_switch_captured_and_landed(host: &FakeHost, why: &str) {
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);

    run_switch(host, SECOND_EMAIL).0.expect(why);

    assert_eq!(
        credential_of(host, EMAIL).as_deref(),
        Some(ROTATED),
        "the Capture ran: {why}"
    );
    assert_eq!(
        live_credential(host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the incoming Credential is the live one: {why}"
    );
    assert_eq!(registry_of(host).active().whose(), Some(SECOND_EMAIL));
}

fn stored_credential(host: &FakeHost, email: &str) -> Option<String> {
    host.keychain_item(&profile_service(host, email), LOGIN_NAME)
}

fn identity_file(host: &FakeHost) -> String {
    host.file(IDENTITY_PATH)
        .expect("the identity file is there")
}

/// The effects a Switch is judged on, in the order they reached the Host, said
/// the way the design talks about them. Everything else Perch touches — its own
/// Registry, the directories it makes sure exist — is left out, so what the
/// test asserts is the sequence a concurrently-running Claude Code would see.
fn trace(host: &FakeHost) -> Vec<String> {
    let live = DEFAULT_SERVICE.to_string();
    let first = profile_service(host, EMAIL);
    let second = profile_service(host, SECOND_EMAIL);

    let store = |service: &String| -> Option<String> {
        if *service == live {
            Some("the live store".to_string())
        } else if *service == first {
            Some(format!("{EMAIL}'s Profile"))
        } else if *service == second {
            Some(format!("{SECOND_EMAIL}'s Profile"))
        } else {
            None
        }
    };
    let lock = |path: &PathBuf| -> Option<String> {
        [REFRESH_LOCK, LEGACY_LOCK, CONFIG_LOCK]
            .iter()
            .find(|known| Path::new(known) == path)
            .map(|known| known.to_string())
    };

    host.effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Took(path) => lock(path).map(|lock| format!("took {lock}")),
            Effect::RemovedDir(path) => lock(path).map(|lock| format!("gave back {lock}")),
            Effect::KeychainGet { service, .. } => {
                store(service).map(|what| format!("read {what}"))
            }
            Effect::KeychainSet { service, .. } => {
                store(service).map(|what| format!("wrote {what}"))
            }
            Effect::Renamed { to, .. } if to == Path::new(IDENTITY_PATH) => {
                Some("patched the Identity".to_string())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_switch_asks_which_claude_code_is_installed_once() {
    let host = machine_with_two_accounts();
    // The fixture logs two Accounts in, and each login asks for itself.
    host.forget_effects();

    run_switch(&host, SECOND_EMAIL).0.expect("it switches");

    let asked = host
        .effects()
        .iter()
        .filter(|effect| matches!(effect, Effect::Exec { args, .. } if args == &["--version"]))
        .count();
    assert_eq!(asked, 1, "{:?}", host.effects());
}

#[test]
fn a_switch_that_cannot_place_the_default_profile_changes_nothing() {
    let host = machine_with_two_accounts().without_env("USER");

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    let refused = result.expect_err("there is no login name to derive a store from");
    assert!(
        refused.to_string().contains("USER"),
        "it names the assumption that failed: {refused}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and nothing moved: the Account that was active still is"
    );
    assert!(
        !printed.contains("Switched to"),
        "nor did it claim to have switched: {printed}"
    );
}

#[test]
fn switching_by_email_makes_that_account_the_one_every_client_reads() {
    let host = machine_with_two_accounts();

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the Switch runs");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the incoming Credential is the live one"
    );
    assert!(identity_file(&host).contains(SECOND_EMAIL));
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
    assert!(
        printed.contains(&format!("Switched to {SECOND_EMAIL}")),
        "{printed}"
    );
}

#[test]
fn switching_by_alias_says_which_account_the_name_reached() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL).0.unwrap();

    let (result, printed) = run_switch(&host, "overflow");

    result.expect("the Switch runs");
    assert!(
        printed.contains(&format!("`overflow` is an Alias for {SECOND_EMAIL}.")),
        "{printed}"
    );
    assert_eq!(live_credential(&host).as_deref(), Some(SECOND_CREDENTIAL));
}

#[test]
fn the_switch_captures_first_patches_the_identity_last_and_holds_claude_codes_locks() {
    let host = machine_with_two_accounts();
    host.forget_effects();

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    assert_eq!(
        trace(&host),
        vec![
            format!("took {REFRESH_LOCK}"),
            format!("took {LEGACY_LOCK}"),
            format!("took {CONFIG_LOCK}"),
            // Inside the locks, not before them: whether a client is running
            // against either Profile is a statement about a moment, and taking
            // the locks can take seconds.
            format!("read {SECOND_EMAIL}'s Profile"),
            "read the live store".to_string(),
            // Asked before it is written over: a Run against the Account being
            // left Rotates its own Profile's copy, so the live one is sometimes
            // the older of the two and Capturing it would retire the newer.
            format!("read {EMAIL}'s Profile"),
            format!("wrote {EMAIL}'s Profile"),
            format!("read {EMAIL}'s Profile"),
            "wrote the live store".to_string(),
            "read the live store".to_string(),
            "patched the Identity".to_string(),
            format!("gave back {CONFIG_LOCK}"),
            format!("gave back {LEGACY_LOCK}"),
            format!("gave back {REFRESH_LOCK}"),
        ],
        "the Capture must reach the outgoing Profile before the live store is \
         written, and the Identity must be patched last, all of it under the \
         locks in Claude Code's order"
    );
}

#[test]
fn the_credential_the_outgoing_account_rotated_to_is_captured_before_it_is_replaced() {
    let host = machine_with_two_accounts();
    // Claude Code Rotated while this Account was active, so the live copy is
    // several Rotations ahead of the one in its Profile.
    let rotated = CREDENTIAL.replace("sk-ant-ort01-test", "sk-ant-ort01-rotated");
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, &rotated);

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    assert_eq!(
        stored_credential(&host, EMAIL).as_deref(),
        Some(rotated.as_str()),
        "the Rotation would be lost if the Capture did not happen first"
    );
}

/// A Run points a client at the Account's own Profile, so a Rotation there
/// retires the refresh token the live copy still carries — leaving that Profile
/// holding the Account's only working Credential, which a Capture would take.
#[test]
fn a_capture_does_not_write_the_live_credential_over_a_newer_one_the_profile_holds() {
    let host = machine_with_two_accounts();
    // What a `perch run someone@example.com` leaves behind: the client Rotated
    // in the Account's own Profile, so that copy expires later than the live one.
    let rotated = CREDENTIAL
        .replace("sk-ant-ort01-test", "sk-ant-ort01-rotated")
        .replace("1785000000000", "1785999999999");
    host.set_keychain_item(&profile_service(&host, EMAIL), LOGIN_NAME, &rotated);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the Switch runs");
    assert_eq!(
        stored_credential(&host, EMAIL).as_deref(),
        Some(rotated.as_str()),
        "the Capture is declined: the live copy's refresh token is the retired one"
    );
    assert!(
        printed.contains("newer Credential"),
        "and the declined Capture is said rather than swallowed: {printed}"
    );
}

#[test]
fn a_live_credential_belonging_to_a_login_made_outside_perch_is_not_captured() {
    let host = machine_with_two_accounts();
    let held = stored_credential(&host, EMAIL).expect("the first Account has a Credential");

    // A login made outside Perch: a Credential Perch never filed, and the
    // Identity Claude Code writes beside it naming whose it is.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, THIRD_CREDENTIAL);
    host.set_file(IDENTITY_PATH, THIRD_IDENTITY_FILE);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the Switch runs");
    assert_eq!(
        stored_credential(&host, EMAIL).as_deref(),
        Some(held.as_str()),
        "{EMAIL}'s own Credential is untouched, not overwritten with {THIRD_EMAIL}'s"
    );
    assert!(
        printed.contains(THIRD_EMAIL) && printed.contains("not Captured"),
        "and the Switch says whose the live Credential was: {printed}"
    );
}

#[test]
fn a_rotation_is_not_lost_to_an_identity_perch_itself_failed_to_patch() {
    // Three, because the Switch that loses the Rotation is the one to a *third*
    // Account: with two, the repair path recognizes the Account it is on.
    let host = machine_with_three_accounts();
    host.now_refusing(IDENTITY_PATH, Refusing::Write, "read-only file");
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");
    host.no_longer_refusing(IDENTITY_PATH, Refusing::Write);
    assert!(
        identity_file(&host).contains(EMAIL),
        "the file still names the Account the Switch left: {}",
        identity_file(&host)
    );

    // The Account that is actually live Rotates, which is the whole reason a
    // Capture happens before anything is written.
    let rotated = SECOND_CREDENTIAL.replace("sk-ant-ort01-second", "sk-ant-ort01-rotated");
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, &rotated);

    let (result, printed) = run_switch(&host, THIRD_EMAIL);

    result.expect_err("nothing on the machine says whose Rotation that is");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(rotated.as_str()),
        "the Rotation is still live rather than written over: {printed}"
    );
    assert_ne!(
        stored_credential(&host, EMAIL).as_deref(),
        Some(rotated.as_str()),
        "and it was not filed under the Account the stale Identity named"
    );
}

#[test]
fn a_live_credential_with_no_identity_beside_it_is_captured_rather_than_left() {
    let host = machine_with_two_accounts();
    let rotated = CREDENTIAL.replace("sk-ant-ort01-test", "sk-ant-ort01-rotated");
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, &rotated);
    host.remove_file(Path::new(IDENTITY_PATH))
        .expect("the identity file was there to remove");

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    assert_eq!(
        stored_credential(&host, EMAIL).as_deref(),
        Some(rotated.as_str()),
        "a Rotation is lost if an absent Identity is read as evidence against"
    );
}

/// An identity file either side of its `oauthAccount` block — the whole of it
/// that does not belong to the Account.
fn around_the_block(text: &str) -> (String, String) {
    let block = probe::oauth_account_block(text).expect("there is a block");
    let (before, after) = text.split_once(block).expect("the block is in the file");
    (before.to_string(), after.to_string())
}

#[test]
fn a_switch_leaves_every_other_key_of_the_identity_file_byte_identical() {
    let host = machine_with_two_accounts();
    let before = identity_file(&host);

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    let after = identity_file(&host);
    assert_ne!(before, after, "the Account is named differently now");
    assert_eq!(
        around_the_block(&after),
        around_the_block(&before),
        "everything either side of the oauthAccount block — project history, \
         settings, the formatting of both — is byte for byte what it was"
    );
}

#[test]
fn shared_state_is_untouched_by_a_switch() {
    let host = machine_with_two_accounts()
        .with_file("/Users/someone/.claude/CLAUDE.md", "# memory\n")
        .with_file(
            "/Users/someone/.claude/settings.json",
            "{\"theme\":\"dark\"}",
        );

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    assert_eq!(
        host.file("/Users/someone/.claude/CLAUDE.md").as_deref(),
        Some("# memory\n")
    );
    assert_eq!(
        host.file("/Users/someone/.claude/settings.json").as_deref(),
        Some("{\"theme\":\"dark\"}")
    );
    assert!(
        !host.effects().iter().any(|effect| matches!(
            effect,
            Effect::WroteFile(path) | Effect::ReadFile(path)
                if path == Path::new("/Users/someone/.claude/CLAUDE.md")
                    || path == Path::new("/Users/someone/.claude/settings.json")
        )),
        "a Switch has no business reading or writing Shared State"
    );
}

#[test]
fn the_identity_written_is_the_block_the_incoming_account_logged_in_with() {
    let host = machine_with_two_accounts();

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch runs");

    let after = identity_file(&host);
    assert!(
        after.contains("\"organizationRole\": \"admin\""),
        "the block Claude Code wrote for that Account carries fields Perch does \
         not record, and they are kept: {after}"
    );
    assert!(
        after.contains("\"organizationName\": \"Overflow Ltd\""),
        "{after}"
    );
}

#[test]
fn switching_back_names_the_account_perch_adopted_with_every_field_it_had() {
    let host = machine_with_two_accounts();
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("the first Switch runs");

    let (result, _) = run_switch(&host, EMAIL);

    result.expect("the Switch back runs");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
    let after = identity_file(&host);
    assert!(
        after.contains(&format!("\"emailAddress\": \"{EMAIL}\"")),
        "{after}"
    );
    assert!(
        after.contains("\"accountUuid\": \"account-uuid-1\""),
        "{after}"
    );
    assert!(after.contains("\"organizationName\": \"Acme\""), "{after}");
    assert!(
        after.contains("\"organizationRole\": \"admin\""),
        "adoption kept no block of its own, so the Switch away had to keep one \
         for it — otherwise the Account comes back with only the fields Perch \
         happens to record: {after}"
    );
    serde_json::from_str::<serde_json::Value>(&after).expect("still a file Claude Code can read");
}

#[test]
fn a_switch_never_writes_one_accounts_identity_into_anothers_profile() {
    // A Switch interrupted between writing the Credential and patching the
    // Identity, so Perch and Claude Code disagree about who is active. Anything
    // copying the live Identity into the Account being left copies the wrong one.
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Write,
        "read-only file",
    );
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");
    host.no_longer_refusing(IDENTITY_PATH, Refusing::Write);

    run_switch(&host, EMAIL).0.expect("the Switch back runs");
    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("and away again");
    assert!(
        identity_file(&host).contains(SECOND_EMAIL),
        "the Account whose Credential is live is the Account named: {printed}\n{}",
        identity_file(&host)
    );
    for (profile, email) in [(FIRST_PROFILE, EMAIL), (SECOND_PROFILE, SECOND_EMAIL)] {
        let kept = host
            .file(format!("{profile}/.claude.json"))
            .expect("each Profile describes its own Account");
        assert!(
            kept.contains(email),
            "{profile} describes somebody else: {kept}"
        );
    }
}

#[test]
fn the_switch_reports_where_it_landed_and_what_the_cache_says_about_it() {
    let host = machine_with_two_accounts();

    let (_, printed) = run_switch(&host, SECOND_EMAIL);

    assert!(
        printed.contains(&format!("Switched to {SECOND_EMAIL}.")),
        "an Account somebody named was not chosen, so nothing is said about \
         choosing it: {printed}"
    );
    assert!(
        !printed.contains("Captured"),
        "and the Capture every Switch makes is silent: {printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "though it did happen — the outgoing Credential is in its own Profile"
    );
    assert!(
        printed.contains("Utilization") && printed.contains("never observed"),
        "an Account nobody has read a figure for says so rather than nothing: {printed}"
    );
    assert!(
        host.http_calls().is_empty(),
        "Utilization is served from cache and a Switch never fetches (ADR a-figure-carries-its-age)"
    );
}

#[test]
fn a_sessions_directory_that_will_not_be_read_stops_the_switch_rather_than_reading_as_empty() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/77.json"), "{}")
        .with_a_path_refusing(
            format!("{FIRST_PROFILE}/sessions"),
            Refusing::Read,
            "Permission denied (os error 13)",
        );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("whether a client is running got no answer");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(
        error.to_string().contains("sessions"),
        "it names the directory that would not be read: {error}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "and nothing was written, because the doubt is resolved towards Live"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn a_profile_that_never_ran_a_client_has_no_sessions_directory_and_switches() {
    let host = machine_with_two_accounts();

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    result.expect("nowhere to look is not the same as something to worry about");
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn switching_away_from_a_profile_a_client_is_running_against_is_refused() {
    let host = machine_with_two_accounts();
    a_client_running_against(&host, FIRST_PROFILE, 77);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the outgoing Profile is Live");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("77"), "{error}");
    assert!(error.to_string().contains(EMAIL), "{error}");
    assert!(error.to_string().contains("Nothing was changed"), "{error}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was written"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn a_live_credential_perch_cannot_read_is_repaired_by_switching_rather_than_refused() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, "{ truncated");

    let (result, printed) = run_switch(&host, EMAIL);

    result.expect("the Switch that repairs the store is not refused by it");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "the store holds a Credential Claude Code can use again: {printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the rubbish was never filed under the Account's own Profile — \
         bytes nothing understands are not a Rotation to keep: {printed}"
    );
    assert!(
        printed.contains("could not be read"),
        "the declined Capture is said rather than swallowed: {printed}"
    );
}

#[test]
fn a_live_credential_perch_cannot_read_does_not_stop_a_switch_to_another_account() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, "{ truncated");

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("a Capture that could not be made is not a Switch that failed");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "{printed}"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "the outgoing Account's Profile is untouched: {printed}"
    );
}

#[test]
fn a_live_store_that_will_not_answer_stops_the_switch_rather_than_being_written_over() {
    let host = two_accounts_off_macos();
    host.now_refusing(CREDENTIALS_PATH, Refusing::Read, "Permission denied");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("a Credential that cannot be read cannot be Captured");
    assert!(
        error.to_string().contains("could not be Captured"),
        "it says which step stopped: {error}"
    );
    assert!(
        error.to_string().contains(EMAIL),
        "and names the Account whose Credential it may be: {error}"
    );
    host.no_longer_refusing(CREDENTIALS_PATH, Refusing::Read);
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "the live store still holds what it held"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and nothing moved"
    );
}

#[test]
fn a_live_store_that_answers_with_rubbish_is_still_switched_over() {
    let host = two_accounts_off_macos();
    host.set_file(CREDENTIALS_PATH, "{ truncated");

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("bytes nothing understands are not a Rotation to lose");
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(SECOND_CREDENTIAL),
        "{printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the rubbish was never filed under the Account's own Profile: {printed}"
    );
}

#[test]
fn switching_onto_a_profile_a_client_is_running_against_lands() {
    let host = machine_with_two_accounts();
    a_client_running_against(&host, SECOND_PROFILE, 4242);

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("reading a Credential out of a Live Profile is safe");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the incoming Account's Credential is the live one"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn a_switch_on_a_machine_whose_identity_file_is_a_managed_link_writes_through_it() {
    let host = machine_with_two_accounts();
    let repository = "/Users/someone/dotfiles/claude.json";
    let managed = host.file(IDENTITY_PATH).expect("Claude Code wrote one");
    host.set_file(repository, &managed);
    host.remove_file(Path::new(IDENTITY_PATH))
        .expect("the dotfile manager put a link here instead");
    let host = host.with_link(perch::host::Link::Symbolic, repository, IDENTITY_PATH);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("a managed dotfile is not a reason to refuse a Switch");
    let after = host
        .file(repository)
        .expect("the repository still holds it");
    assert!(
        after.contains(SECOND_EMAIL),
        "the file in the repository is the one that changed: {printed}"
    );
    assert!(
        after.contains("numStartups") && after.contains("projects"),
        "and everything that is the person rather than the Account survived — \
         which a file composed afresh from the Identity would not carry:\n{after}"
    );
    assert!(
        host.link_at(IDENTITY_PATH).is_some(),
        "the link is still a link, so what manages it goes on managing it"
    );
}

#[test]
fn a_profile_whose_sessions_is_a_link_reads_the_clients_at_the_other_end() {
    let host = machine_with_two_accounts();
    a_client_running_against(&host, "/Users/someone/.claude", 4242);
    let host = host.with_link(
        perch::host::Link::Symbolic,
        "/Users/someone/.claude/sessions",
        format!("{FIRST_PROFILE}/sessions"),
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("that Profile reads as Live through the link");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(
        error.to_string().contains("4242"),
        "naming a client that is running against the Default Profile: {error}"
    );
}

#[test]
fn a_marker_left_behind_by_a_client_that_died_is_not_a_live_profile() {
    let host = machine_with_two_accounts().with_file(
        format!("{FIRST_PROFILE}/sessions/9999.json"),
        &a_client_marker(9999, Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap()),
    );

    assert_the_switch_captured_and_landed(&host, "nothing is holding that Profile");
}

#[test]
fn a_marker_whose_pid_now_belongs_to_a_younger_process_is_not_a_live_profile() {
    // A dead session's marker plus a recycled PID: the process wearing the pid
    // today began after the session the marker records, so it cannot be the
    // client that wrote it (ADR a-profile-is-live-by-evidence).
    let session_began = Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap();
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            &a_client_marker(4242, session_began),
        )
        .with_live_process_started_at(4242, Utc.with_ymd_and_hms(2026, 8, 4, 11, 0, 0).unwrap());

    assert_the_switch_captured_and_landed(&host, "that client died; the pid was recycled");
}

#[test]
fn a_clock_that_stepped_forward_does_not_dismiss_the_marker_of_a_running_client() {
    let session_began = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            &a_client_marker(4242, session_began),
        )
        .with_live_process_started_at(4242, Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 2).unwrap());

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("that client is running");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
}

#[test]
fn a_marker_that_does_not_say_when_its_session_began_is_no_evidence_of_a_client() {
    // The marker parses but is not the shape Perch believes in. A Profile is
    // Live when something says so, not when nothing does.
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            r#"{"pid":4242,"cwd":"/Users/someone/work"}"#,
        )
        .with_live_process(4242);

    assert_the_switch_captured_and_landed(
        &host,
        "an uncorroborated marker does not hold a Profile",
    );
}

#[test]
fn a_marker_that_cannot_be_read_at_all_holds_the_profile_of_a_running_client() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/4242.json"), "")
        .with_a_path_refusing(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            Refusing::Read,
            "Permission denied",
        )
        .with_live_process(4242);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("nothing about that marker has been established");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(
        error.to_string().contains("4242.json"),
        "and it names the file to go and look at: {error}"
    );
}

#[test]
fn a_sessions_that_is_a_file_is_doubt_rather_than_an_idle_profile() {
    let host = machine_with_two_accounts().with_file(format!("{FIRST_PROFILE}/sessions"), "");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("nothing about that Profile has been established");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(
        error.to_string().contains("sessions"),
        "and it names the directory to go and look at: {error}"
    );
}

#[test]
fn an_unreadable_marker_whose_process_is_gone_holds_nothing() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/4242.json"), "")
        .with_a_path_refusing(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            Refusing::Read,
            "Permission denied",
        );

    assert_the_switch_captured_and_landed(&host, "no client is holding that Profile");
}

#[test]
fn an_unreadable_marker_whose_process_is_alive_names_the_file_rather_than_the_clock() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/4242.json"), "")
        .with_a_path_refusing(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            Refusing::Read,
            "Permission denied",
        )
        .with_live_process(4242);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the marker cannot be corroborated");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    let said = error.to_string();
    assert!(
        said.contains("could not be read") && said.contains("readable"),
        "the refusal names the file and what to do about it: {said}"
    );
    assert!(
        !said.contains("when that process began"),
        "and not the clock, which is the other doubt's diagnosis: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was written"
    );
}

#[test]
fn a_marker_that_is_not_json_is_no_evidence_of_a_client() {
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            "not a marker at all",
        )
        .with_live_process(4242);

    assert_the_switch_captured_and_landed(&host, "a marker Perch cannot read is not evidence");
}

#[test]
fn a_live_process_whose_start_cannot_be_read_is_a_refusal_naming_the_assumption() {
    // The marker is well-shaped and its process is alive, but the operating
    // system will not say when that process began — so the marker can be
    // neither corroborated nor dismissed, and guessing either way is wrong.
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            &a_client_marker(4242, Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap()),
        )
        .with_live_process_of_unknown_start(4242);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the marker cannot be corroborated");
    assert_eq!(error.exit_code(), EXIT_PROBE_REFUSED);
    assert!(
        error
            .to_string()
            .contains(probe::assumption::SESSION_MARKER),
        "{error}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was written"
    );
}

#[test]
fn a_marker_older_than_the_boot_stops_holding_the_profile_of_a_recycled_pid() {
    // The reported machine: a client wrote its marker, the machine rebooted,
    // and a system daemon took the pid at startup. macOS and Windows both
    // refuse the start of another user's process, so nothing corroborates it.
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/532.json"),
            &a_client_marker(532, Utc.with_ymd_and_hms(2026, 8, 3, 10, 42, 45).unwrap()),
        )
        .with_live_process_of_unknown_start(532)
        .with_booted_at(Utc.with_ymd_and_hms(2026, 8, 3, 11, 47, 34).unwrap());

    assert_the_switch_captured_and_landed(&host, "that session did not survive the reboot");
}

#[test]
fn a_switch_that_cannot_patch_the_identity_says_what_it_left_where() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Write,
        "read-only file",
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the Identity could not be patched");
    let message = error.to_string();
    assert!(
        message.contains(SECOND_EMAIL) && message.contains(EMAIL),
        "{message}"
    );
    assert!(
        message.contains("perch switch"),
        "it names the way out: {message}"
    );

    // The recoverable state: the incoming Credential is live, the outgoing one
    // was Captured before it was replaced, and Perch says who is active by the
    // only measure that matters — whose Credential a client would read.
    assert_eq!(live_credential(&host).as_deref(), Some(SECOND_CREDENTIAL));
    assert_eq!(stored_credential(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
    assert_eq!(
        host.file(perch::host::temp_beside(&host, Path::new(IDENTITY_PATH))),
        None,
        "a write that did not land leaves nothing beside the file Claude Code \
         reads"
    );
}

#[test]
fn patching_the_identity_leaves_it_as_narrow_as_it_was_found() {
    let host = machine_with_two_accounts().with_file_mode(IDENTITY_PATH, 0o600);

    run_switch(&host, SECOND_EMAIL).0.expect("it switches");

    assert!(identity_file(&host).contains(SECOND_EMAIL));
    assert_eq!(
        host.mode_of(IDENTITY_PATH),
        Some(0o600),
        "the file Perch replaced was narrow, so its replacement is too"
    );
}

#[test]
fn patching_the_identity_does_not_narrow_a_file_that_was_open() {
    let host = machine_with_two_accounts().with_file_mode(IDENTITY_PATH, 0o644);

    run_switch(&host, SECOND_EMAIL).0.expect("it switches");

    assert_eq!(host.mode_of(IDENTITY_PATH), Some(0o644));
}

#[test]
fn running_the_switch_again_finishes_a_job_that_stopped_half_way() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Write,
        "read-only file",
    );
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");

    // The file can be written again — the permission was fixed, the disk freed.
    host.no_longer_refusing(IDENTITY_PATH, Refusing::Write);
    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the repair runs rather than being refused as unnecessary");
    assert!(identity_file(&host).contains(SECOND_EMAIL), "{printed}");
}

#[test]
fn switching_to_the_account_that_is_already_active_changes_nothing() {
    let host = machine_with_two_accounts();

    let (result, _) = run_switch(&host, EMAIL);

    let error = result.expect_err("there is nothing to do");
    assert_eq!(error.exit_code(), EXIT_NOTHING_TO_DO);
    assert!(
        error.to_string().contains("already the active Account"),
        "{error}"
    );
    // Perch's own Registry lock is taken by every command; what must not be
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
fn switching_to_the_active_account_after_a_logout_puts_its_credential_back() {
    let host = machine_with_two_accounts();
    host.forget_keychain_item(DEFAULT_SERVICE, LOGIN_NAME);

    let (result, printed) = run_switch(&host, EMAIL);

    result.expect("the repair runs rather than being refused as unnecessary");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "the Account Perch says is active is the one a client now reads: {printed}"
    );
}

#[test]
fn a_switch_takes_a_lock_a_process_died_holding_and_waits_for_one_still_held() {
    let host = machine_with_two_accounts();
    let long_ago = host.now() - chrono::Duration::seconds(120);
    let host = host
        .with_dir_held_since(REFRESH_LOCK, long_ago)
        .with_dir_held_since(LEGACY_LOCK, long_ago);

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("a lock nobody has touched for two minutes is nobody's");

    assert!(
        host.effects().iter().any(
            |effect| matches!(effect, Effect::RemovedDir(path) if path == Path::new(REFRESH_LOCK))
        ),
        "an abandoned lock is cleared rather than waited on forever"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Slept { .. })),
        "and it is not waited on at all"
    );
}

#[test]
fn an_abandoned_lock_that_would_not_be_cleared_is_waited_on_rather_than_declared_broken() {
    let host = machine_with_two_accounts();
    let long_ago = host.now() - chrono::Duration::seconds(120);
    let host = host
        .with_dir_held_since(REFRESH_LOCK, long_ago)
        .with_a_path_refusing(REFRESH_LOCK, Refusing::Delete, "Device or resource busy");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let refusal = result.expect_err("the lock never comes free");
    let said = refusal.to_string();
    assert!(
        !said.contains("is not a lock directory"),
        "a directory that would not go is not a path that is not a directory: \
         {said}"
    );
    assert!(
        host.effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Slept { .. })),
        "it falls through to the ordinary wait rather than ending the command \
         on the first attempt: {:?}",
        host.effects()
    );
}

#[test]
fn an_abandoned_lock_that_cannot_even_be_walked_is_still_waited_on_rather_than_declared_broken() {
    let host = machine_with_two_accounts();
    let long_ago = host.now() - chrono::Duration::seconds(120);
    // One arrangement rather than two: `with_unlistable_dir` says what it means
    // — `remove_dir_all` and the listing both fail EACCES — so one real state is
    // not described by turning two knobs that could be turned apart.
    let host = host
        .with_dir_held_since(REFRESH_LOCK, long_ago)
        .with_a_path_refusing(REFRESH_LOCK, Refusing::List, "Permission denied");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let refusal = result.expect_err("the lock never comes free");
    let said = refusal.to_string();
    assert!(
        !said.contains("is not a lock directory"),
        "a directory nothing can walk is still a directory: {said}"
    );
    assert!(
        host.effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Slept { .. })),
        "so it falls through to the ordinary wait: {:?}",
        host.effects()
    );
}

#[test]
fn something_at_a_lock_path_that_is_not_a_lock_is_named_rather_than_blamed_on_claude_code() {
    let host = machine_with_two_accounts().with_file(REFRESH_LOCK, "not a lock directory");
    // Past the staleness window, which is what makes it a wedge rather than a
    // lock somebody is holding: a fresh one is waited on like any other, and it
    // is only once nothing may still be behind it that the shape is the news.
    host.set_now(host.now() + chrono::Duration::milliseconds(120_000));

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let refusal = result.expect_err("nothing can take that lock");
    let said = refusal.to_string();
    assert!(
        said.contains("is not a lock directory"),
        "it says what is wrong: {said}"
    );
    // The file name rather than the whole path: a `Path` renders with the
    // separator of whatever is running the test, so an assertion on the whole
    // string would be testing the separator rather than the message.
    assert!(
        said.contains(
            Path::new(REFRESH_LOCK)
                .file_name()
                .and_then(|name| name.to_str())
                .expect("the lock is a named file")
        ),
        "and where: {said}"
    );
    assert!(
        !said.contains("quit it"),
        "and does not send somebody looking for a Claude Code to quit: {said}"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn a_lock_abandoned_on_the_last_attempt_is_taken_rather_than_reported_as_held() {
    let host = machine_with_two_accounts();
    let now = host.now();
    // The refresh lock goes stale at 60s and the waits before the last attempt
    // come to 3.175s, so a lock held since 58s ago reads as alive on the first
    // seven attempts and as abandoned on the eighth, which is the last there is.
    let host = host.with_dir_held_since(REFRESH_LOCK, now - chrono::Duration::milliseconds(58_000));

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("the lock was free by the time the last attempt asked");

    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn a_lock_somebody_is_holding_stops_the_switch_without_changing_anything() {
    let host = machine_with_two_accounts();
    let now = host.now();
    // Held, and touched just now: somebody is alive behind it. The fake clock
    // moves as Perch waits, so this test spends none of the time it describes.
    let host = host.with_dir_held_since(REFRESH_LOCK, now);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the lock is somebody else's");
    assert!(
        error.to_string().contains("is held by Claude Code"),
        "{error}"
    );
    assert!(error.to_string().contains("Nothing was changed"), "{error}");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
    assert!(
        host.effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Slept { .. })),
        "a lock somebody holds is waited on before it is given up on"
    );
}

#[test]
fn a_target_that_names_nothing_is_refused_before_anything_is_touched() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL).0.unwrap();

    let (result, _) = run_switch(&host, "overflw");

    let error = result.expect_err("nothing is called that");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("Did you mean `overflow`?"),
        "a mistyped name is far more common than an imagined one: {error}"
    );
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
}

#[test]
fn an_account_whose_credential_perch_no_longer_holds_is_quarantined_rather_than_dropped() {
    let host = machine_with_two_accounts();
    host.forget_keychain_item(&profile_service(&host, SECOND_EMAIL), LOGIN_NAME);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("there is no Credential to make live");
    assert_eq!(error.exit_code(), EXIT_QUARANTINED);
    assert!(error.to_string().contains("perch relogin"), "{error}");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));

    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::NoCredential),
        "what Perch found out is written down, so the next command says the \
         Account is broken rather than discovering it again"
    );
    assert!(
        registry_of(&host).account(SECOND_EMAIL).is_some(),
        "and the Account is still held: one that vanished would read as data loss"
    );
}

#[test]
fn a_locked_keychain_reads_as_locked_rather_than_as_a_missing_account() {
    let host = machine_with_two_accounts();
    host.lock_keychain("The user name or passphrase you entered is not correct");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the keychain cannot be consulted");
    assert_eq!(error.exit_code(), EXIT_KEYCHAIN_UNAVAILABLE);
}

#[test]
fn a_client_that_starts_during_the_lock_wait_still_stops_the_switch() {
    let host = machine_with_two_accounts();
    let now = host.now();
    let host = host
        .with_dir_held_since(REFRESH_LOCK, now)
        .once_while_waiting(move |host| {
            // The holder gives the lock back — and in the same moment somebody
            // starts working against the Profile being switched to.
            host.remove_dir_all(Path::new(REFRESH_LOCK))
                .expect("the holder is done");
            host.set_file(
                format!("{FIRST_PROFILE}/sessions/7788.json"),
                &a_client_marker(7788, now),
            );
            host.set_live_process(7788);
        });

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("that Credential belongs to the session holding it");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("7788"), "{error}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "and nothing was written"
    );
}

#[test]
fn a_switch_finishes_against_a_claude_json_that_has_no_identity_block_yet() {
    let host = machine_with_two_accounts();
    host.set_file(
        Path::new(IDENTITY_PATH),
        r#"{"numStartups": 41, "theme": "dark"}"#,
    );

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("a file with no block is a file to write one into");
    let identity = host
        .file(Path::new(IDENTITY_PATH))
        .expect("the file is still there");
    assert!(
        identity.contains(SECOND_EMAIL),
        "and it comes to name the Account whose Credential is now live: {identity}"
    );
    assert!(
        identity.contains(r#""numStartups": 41"#) && identity.contains(r#""theme": "dark""#),
        "with every other member of it untouched (ADR everything-but-the-account): {identity}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

#[test]
fn switching_with_no_active_account_recorded_says_there_was_nothing_to_capture() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("an Account Perch holds is still somewhere to land");
    assert!(
        printed.contains("Perch held no active Account, so there was nothing to Capture."),
        "{printed}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the Switch itself still happened"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn switching_from_a_logged_out_claude_code_says_there_was_nothing_live_to_capture() {
    let host = machine_with_two_accounts();
    host.keychain_delete(DEFAULT_SERVICE, LOGIN_NAME)
        .expect("the login is given up");

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("a logged-out machine is still one that can be switched");
    assert!(
        printed.contains("There was no live Credential to Capture: Claude Code was logged out."),
        "{printed}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the incoming Credential is live"
    );
}

#[test]
fn a_switch_perch_cannot_write_down_moves_nothing_at_all() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        REGISTRY_PATH,
        Refusing::Write,
        "read-only",
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Landing could not be written")
        .to_string();
    assert!(
        said.contains("has written down that it is about to"),
        "it says why nothing moved: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "the outgoing Credential is still the live one: {said}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and Perch is on the Account it was on"
    );
}

#[test]
fn a_switch_that_moves_nothing_takes_its_landing_back() {
    let host = two_accounts_off_macos().with_a_path_refusing(
        CREDENTIALS_PATH,
        Refusing::Write,
        "read-only file",
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    result.expect_err("the Default Profile could not be written");
    assert_eq!(
        *registry_of(&host).active(),
        Active::Settled(EMAIL.to_string()),
        "nothing moved, so Perch is settled on the Account it was on rather \
         than in flight"
    );
}

/// A machine that is not a Mac, holding the same two Accounts. Off macOS a
/// Profile keeps its Credential in a file (ADR claude-code-chooses-the-store),
/// which is the only way to make one particular store refuse a write while the
/// others still work — a locked keychain locks all of them at once, and stops
/// the Switch a step earlier than these tests are about.
fn two_accounts_off_macos() -> FakeHost {
    let host = logged_in_machine_off_macos()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        perch::commands::add::AddArgs {
            no_group: true,
            ..Default::default()
        },
    )
    .0
    .expect("the second Account is added");
    host
}

#[test]
fn a_switch_that_cannot_capture_says_nothing_moved_and_moves_nothing() {
    let host = two_accounts_off_macos();
    let outgoing = store_of(&host, EMAIL).credentials_file;
    let host = host.with_a_path_refusing(&outgoing, Refusing::Write, "read-only file");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Capture could not be written")
        .to_string();
    assert!(
        said.contains("Nothing was switched."),
        "the first write failing means nothing happened: {said}"
    );
    assert!(
        said.contains(EMAIL) && said.contains("still the active Account"),
        "it names who is still active: {said}"
    );

    // And the machine agrees with the note in the one place that decides it.
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "the live Credential is the outgoing Account's, untouched"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn a_live_write_that_fails_with_nothing_active_names_the_account_that_did_not_land() {
    let host = two_accounts_off_macos();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);
    let host = host.with_a_path_refusing(CREDENTIALS_PATH, Refusing::Write, "read-only file");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the live store could not be written")
        .to_string();
    assert!(
        said.contains(&format!("{SECOND_EMAIL} was not made active")),
        "{said}"
    );
    assert!(
        !said.contains("Captured") && !said.contains("Profile is unchanged"),
        "with nothing active there is no Profile to say anything about: {said}"
    );
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "and the live store is as it was found"
    );
}

#[test]
fn a_switch_that_captured_but_could_not_go_live_says_nothing_was_lost() {
    let host = two_accounts_off_macos().with_a_path_refusing(
        CREDENTIALS_PATH,
        Refusing::Write,
        "read-only file",
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the live store could not be written")
        .to_string();
    assert!(
        said.contains("was Captured into its own Profile first")
            && said.contains("nothing has been lost"),
        "{said}"
    );
    assert!(
        said.contains(&format!("{SECOND_EMAIL} was not made active")),
        "and it says the Switch did not happen: {said}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "the machine never moved"
    );
}

#[test]
fn a_live_write_that_fails_with_nothing_captured_says_the_profile_is_unchanged() {
    let host = two_accounts_off_macos();
    host.remove_file(std::path::Path::new(CREDENTIALS_PATH))
        .expect("the login is given up");
    let host = host.with_a_path_refusing(CREDENTIALS_PATH, Refusing::Write, "read-only file");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the live store could not be written")
        .to_string();
    assert!(
        said.contains(&format!("{EMAIL}'s Profile is unchanged.")),
        "{said}"
    );
    assert!(
        !said.contains("Captured"),
        "nothing was Captured, so nothing may claim it was: {said}"
    );
}

#[test]
fn an_identity_that_cannot_be_patched_with_nothing_active_still_says_what_to_run() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Write,
        "read-only file",
    );
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Identity could not be patched")
        .to_string();
    assert!(
        said.contains("another Account"),
        "with nobody recorded, the file names an Account Perch cannot name: {said}"
    );
    assert!(
        said.contains(&format!("perch switch {SECOND_EMAIL}")),
        "it still names the way out: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the machine did move, which is what makes running it again safe"
    );
}

#[test]
fn an_identity_file_that_is_not_json_leaves_a_switch_that_says_how_to_finish_it() {
    let host = machine_with_two_accounts();
    host.set_file(IDENTITY_PATH, "not json at all");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Identity could not be patched")
        .to_string();
    assert!(
        said.contains(&format!("perch switch {SECOND_EMAIL}")),
        "{said}"
    );
    assert_eq!(live_credential(&host).as_deref(), Some(SECOND_CREDENTIAL));
}

#[test]
fn a_switch_onto_the_active_account_repairs_an_identity_naming_nobody() {
    let host = machine_with_two_accounts();
    host.set_file(
        IDENTITY_PATH,
        r#"{"oauthAccount": {"organization": "Acme"}}"#,
    );

    let (result, _) = run_switch(&host, EMAIL);

    result.expect("the Switch that repairs the file is not refused by it");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "the Account Perch records as active is the live one"
    );
    let identity = host.file(IDENTITY_PATH).expect("the file is still there");
    assert!(
        identity.contains(EMAIL),
        "and it now names somebody: {identity}"
    );
}

#[test]
fn an_identity_file_that_cannot_be_read_stops_the_switch_at_its_last_step() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Read,
        "permission denied",
    );
    // Derived rather than spelled: `~/.claude.json` is a join, so a Windows
    // build renders it `/Users/someone\.claude.json` and the constant the
    // fixture arranges the failure with is not the string the refusal prints.
    let identity = probe::default_store(&host)
        .expect("home is known")
        .identity_file;

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Identity could not be read")
        .to_string();
    assert!(
        said.contains(&identity.display().to_string()),
        "it names the file: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the Credential went live before the file was read"
    );
}

#[test]
fn a_stored_credential_that_cannot_be_understood_stops_the_switch_before_it_writes() {
    let host = machine_with_two_accounts();
    host.set_keychain_item(
        &profile_service(&host, SECOND_EMAIL),
        LOGIN_NAME,
        "{\"claudeAiOauth\":{}}",
    );

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("the Credential is not one Perch understands")
        .to_string();
    assert!(
        said.contains("has no accessToken"),
        "it says which belief the stored Credential broke: {said}"
    );
    assert!(
        said.contains(CLAUDE_VERSION),
        "and against which Claude Code, which is what dates the belief: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was written: the Credential is read before the first write"
    );
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

#[test]
fn a_switch_typed_inside_a_run_lands_on_the_default_profile_rather_than_the_runs() {
    let inside_a_run =
        perch::holdings::profile_dir_for(&machine_with_three_accounts(), SECOND_EMAIL)
            .expect("home is known");
    let host = machine_with_three_accounts()
        .with_env("CLAUDE_CONFIG_DIR", &inside_a_run.to_string_lossy());

    run_switch(&host, THIRD_EMAIL).0.expect("the Switch lands");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(THIRD_CREDENTIAL),
        "the live Credential is the one every client falls back to"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the Profile the Run is working in still holds its own Account's Credential"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the outgoing Account was Captured back into its own Profile"
    );
}

#[test]
fn a_switch_typed_inside_a_login_lands_on_the_default_profile_rather_than_the_pending_one() {
    let host = machine_with_two_accounts();
    let pending = perch::holdings::pending_login_dir(&host, host.now()).expect("home is known");
    let host =
        machine_with_two_accounts().with_env("CLAUDE_CONFIG_DIR", &pending.to_string_lossy());

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch lands");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the live Credential is the one every client falls back to, not one \
         filed in a directory the login is about to delete"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and the outgoing Account was Captured back into its own Profile"
    );
    assert!(
        perch::credentials::read(
            &host,
            &probe::store_for_profile(&host, &pending).expect("USER is set")
        )
        .expect("the store could be consulted")
        .is_none(),
        "nothing was written into the pending login directory at all"
    );
}

#[test]
fn a_configuration_directory_that_is_not_a_profile_is_where_a_switch_lands() {
    let moved = Path::new("/Users/someone/elsewhere");
    let host = machine_with_two_accounts().with_env("CLAUDE_CONFIG_DIR", &moved.to_string_lossy());

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch lands");

    let store = probe::store_for_profile(&host, moved).expect("USER is set");
    assert_eq!(
        perch::credentials::read(&host, &store)
            .expect("the store could be consulted")
            .map(|held| held.credential.to_string())
            .as_deref(),
        Some(SECOND_CREDENTIAL),
        "the directory they moved their configuration to is the Default Profile"
    );
}

/// An address with a non-ASCII letter in it, and the same address spelled with
/// that letter in the other case. `É` and `é` are one letter over the whole of
/// Unicode and two unrelated bytes under ASCII folding.
const ACCENTED: &str = "café@example.com";
const ACCENTED_UPPER: &str = "CAFÉ@example.com";

fn accented_identity(email: &str) -> String {
    format!(
        r#"{{
  "numStartups": 3,
  "oauthAccount": {{
    "accountUuid": "account-uuid-accented",
    "emailAddress": "{email}",
    "organizationUuid": "organization-uuid-accented",
    "organizationName": "Café",
    "organizationRole": "admin"
  }},
  "projects": {{}}
}}"#
    )
}

/// A machine adopted as an Account whose address carries a non-ASCII letter,
/// holding a second Account to switch to.
fn a_machine_on_an_accented_account() -> FakeHost {
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(IDENTITY_PATH, &accented_identity(ACCENTED))
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");
    host
}

#[test]
fn a_rotation_is_captured_however_the_identity_file_cases_a_non_ascii_address() {
    let host = a_machine_on_an_accented_account();
    assert_eq!(registry_of(&host).active().whose(), Some(ACCENTED));
    // Claude Code renewed, Rotated, and rewrote its own file with the other
    // spelling of the same address.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.set_file(IDENTITY_PATH, &accented_identity(ACCENTED_UPPER));

    let (outcome, printed) = run_switch(&host, SECOND_EMAIL);

    outcome.expect("the Switch lands");
    assert_eq!(
        credential_of(&host, ACCENTED).as_deref(),
        Some(ROTATED),
        "the Rotation went back into its own Profile rather than being \
         thrown away as somebody else's: {printed}"
    );
    assert_eq!(live_credential(&host).as_deref(), Some(SECOND_CREDENTIAL));
}

#[test]
fn a_switch_onto_the_account_already_active_is_recognized_whatever_the_case() {
    let host = a_machine_on_an_accented_account();
    host.set_file(IDENTITY_PATH, &accented_identity(ACCENTED_UPPER));

    let (outcome, _printed) = run_switch(&host, ACCENTED);

    let error = outcome.expect_err("there is nothing to do");
    assert_eq!(error.exit_code(), EXIT_NOTHING_TO_DO);
    assert!(
        error.to_string().contains("already the active Account"),
        "{error}"
    );
}

#[test]
fn a_landing_left_behind_by_a_death_does_not_cost_the_outgoing_account_its_credential() {
    let host = machine_with_three_accounts();
    // What a Perch killed between step two and its own record leaves behind: the
    // Landing, the incoming Credential live, and `.claude.json` still naming the
    // Account that Switch was leaving.
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_CREDENTIAL);

    let (result, printed) = run_switch(&host, THIRD_EMAIL);

    result.expect("the Landing is settled and the Switch goes through");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "the Account the interrupted Switch left keeps its own Credential: \
         {printed}"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and so does the one it had landed on"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(THIRD_CREDENTIAL),
        "while the Switch that was asked for happened"
    );
    assert_eq!(
        *registry_of(&host).active(),
        Active::Settled(THIRD_EMAIL.to_string()),
        "and nothing is left in flight"
    );
}

#[test]
fn a_landing_is_settled_onto_whoever_the_live_credential_belongs_to() {
    struct Case {
        what: &'static str,
        live: Option<&'static str>,
        settles_on: &'static str,
        /// An Account this case is not about, given up to make the settling
        /// visible without disturbing it.
        and_removes: &'static str,
    }

    let cases = [
        Case {
            what: "the Account being switched to, so the Switch had finished",
            live: Some(SECOND_CREDENTIAL),
            settles_on: SECOND_EMAIL,
            and_removes: THIRD_EMAIL,
        },
        Case {
            what: "the Account being left, so the Switch had not moved anything",
            live: Some(CREDENTIAL),
            settles_on: EMAIL,
            and_removes: THIRD_EMAIL,
        },
        Case {
            what: "a third Account Perch holds, which is the fallback",
            live: Some(THIRD_CREDENTIAL),
            settles_on: THIRD_EMAIL,
            and_removes: SECOND_EMAIL,
        },
        Case {
            what: "nothing at all, which is nothing a later Capture could destroy",
            live: None,
            settles_on: EMAIL,
            and_removes: THIRD_EMAIL,
        },
    ];

    for case in cases {
        let host = machine_with_three_accounts();
        a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
        match case.live {
            Some(credential) => host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, credential),
            None => host
                .keychain_delete(DEFAULT_SERVICE, LOGIN_NAME)
                .expect("the login is given up"),
        }

        run_remove_with(
            &host,
            perch::commands::remove::RemoveArgs {
                target: case.and_removes.to_string(),
                yes: true,
            },
        )
        .0
        .unwrap_or_else(|error| panic!("{}: nobody is on it: {error}", case.what));

        assert_eq!(
            *registry_of(&host).active(),
            Active::Settled(case.settles_on.to_string()),
            "{}: the Landing is settled rather than left in flight",
            case.what
        );
    }
}

#[test]
fn a_landing_is_resolved_with_claude_codes_locks_held() {
    let host = machine_with_three_accounts();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_CREDENTIAL);
    host.forget_effects();

    run_remove_with(
        &host,
        perch::commands::remove::RemoveArgs {
            target: THIRD_EMAIL.to_string(),
            yes: true,
        },
    )
    .0
    .expect("nobody is on it");

    let effects = host.effects();
    let read_the_live_store = effects
        .iter()
        .position(|effect| {
            matches!(effect, Effect::KeychainGet { service, .. } if service == DEFAULT_SERVICE)
        })
        .expect("the live Credential is what settles a Landing");
    for held in [REFRESH_LOCK, LEGACY_LOCK, CONFIG_LOCK] {
        let took = effects
            .iter()
            .position(|effect| matches!(effect, Effect::Took(path) if path == Path::new(held)))
            .unwrap_or_else(|| panic!("{held} is taken: {effects:?}"));
        assert!(
            took < read_the_live_store,
            "{held} is held before the live Credential is read, or a Claude Code \
             refresh lands on the one thing that can settle this Landing"
        );
    }
}

#[test]
fn a_live_store_that_will_not_answer_resolves_no_landing() {
    let host = two_accounts_off_macos();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.now_refusing(CREDENTIALS_PATH, Refusing::Read, "Permission denied");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let said = result
        .expect_err("nothing on the machine can say whether that Switch happened")
        .to_string();
    assert!(
        said.contains("was in flight and was not recorded"),
        "it says what could not be settled: {said}"
    );
    assert!(
        said.contains("Make that store readable and run this again."),
        "and what to do about it: {said}"
    );

    host.no_longer_refusing(CREDENTIALS_PATH, Refusing::Read);
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "the live store still holds what it held"
    );
    assert!(
        matches!(*registry_of(&host).active(), Active::Landing { .. }),
        "and the Landing is still there to be resolved once the store answers"
    );
}

#[test]
fn a_landing_that_left_nobody_behind_is_refused_without_naming_one() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);
    a_switch_died_mid_flight(&host, None, SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("nothing on the machine says whose that Credential is");
    let said = error.to_string();
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{said}");
    assert!(said.contains("on no Account before it"), "{said}");
    assert!(
        said.contains(&format!("perch relogin {SECOND_EMAIL}")),
        "and names the one way through: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(ROTATED),
        "with nothing written over: {said}"
    );
}

#[test]
fn a_landing_nothing_accounts_for_is_refused_naming_both_readings() {
    let host = machine_with_two_accounts();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    // A Rotation made after the interruption, by whichever of the two the
    // machine was actually acting as.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("Perch cannot tell whose that Credential is");
    let said = error.to_string();
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{said}");
    assert!(
        said.contains(EMAIL) && said.contains(SECOND_EMAIL),
        "{said}"
    );
    assert!(
        said.contains("perch relogin"),
        "it says the way through: {said}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(ROTATED),
        "and nothing was written over: {said}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "in either Profile"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "in either Profile"
    );
}

#[test]
fn repairing_an_interrupted_switch_never_writes_over_a_rotation_it_declined_to_save() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        IDENTITY_PATH,
        Refusing::Write,
        "read-only file",
    );
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "the incoming Account is live, so Perch records it as active"
    );
    // The user carries on working, and Claude Code Rotates.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.no_longer_refusing(IDENTITY_PATH, Refusing::Write);

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("Perch cannot tell whose that Credential is");
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{error}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(ROTATED),
        "the Rotation is still live rather than written over: {printed}"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and its Profile copy is untouched, so nothing was lost either way"
    );
    assert!(
        error.to_string().contains("perch relogin"),
        "and it says the way through: {error}"
    );
}

#[test]
fn a_keychain_dialog_somebody_walked_away_from_does_not_cost_perch_its_registry_hold() {
    let host = machine_with_two_accounts().with_a_keychain_that_asks_first(20_000);
    host.forget_effects();

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("the Switch lands");
    let touched = host
        .effects()
        .iter()
        .filter(|effect| {
            matches!(effect, Effect::Touched(path) if path.starts_with("/Users/someone/.config/perch/.registry.lock"))
        })
        .count();
    assert!(
        touched > 1,
        "the registry hold is renewed as the Switch goes rather than only at the \
         save afterwards, so the slow steps do not run it out: touched {touched} \
         time(s)\n{printed}"
    );
}

/// A live Credential matching neither the Account Perch is on nor the one its
/// Identity names — somebody logged into a second Account with `claude` itself,
/// and that session Rotated. Perch cannot tell whose Rotation it is holding, so
/// it refuses; the branch had no test, and its remedy named two commands that
/// both meet this same refusal again.
#[test]
fn a_live_credential_nothing_accounts_for_names_the_repair_that_clears_it() {
    let host = machine_with_two_accounts();

    // The Identity names the second Account, and the Credential beside it is
    // neither Account's stored copy.
    host.set_file(IDENTITY_PATH, SECOND_IDENTITY_FILE);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, THIRD_CREDENTIAL);
    host.forget_effects();

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("Perch cannot say whose Rotation that is");
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{error}");
    let said = error.to_string();
    assert!(
        said.contains(&format!("perch relogin {EMAIL}")),
        "the repair named is the one that lands in the Default Profile — the \
         Account Perch is on, which is the one a Capture files under: {said}"
    );
    assert!(
        !said.contains(&format!("perch switch {SECOND_EMAIL}")),
        "and not a Switch, which re-enters this same refusal: {said}"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(THIRD_CREDENTIAL),
        "nothing was written over: {printed}"
    );
}

/// The other side of the same rule. The Capture writes into the *outgoing*
/// Account's store, so a Profile it shares is one whose other Account's
/// Credential the Capture takes away — with nothing left to tell the two apart.
#[test]
fn a_switch_off_an_account_that_shares_a_profile_is_refused_before_the_capture() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    let mut registry = registry_of(&host);
    for email in ["some-one@example.com", "some.one@example.com"] {
        registry.upsert(perch::registry::Account {
            identity: probe::Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
    }
    // The one being switched *off*, so the Capture is what would write.
    registry.settle(Some("some-one@example.com".to_string()));
    common::save_registry(&host, &registry);

    let shared = store_of(&host, "some-one@example.com");
    host.set_keychain_item(
        &shared.keychain_service,
        &shared.keychain_account,
        SECOND_CREDENTIAL,
    );
    host.forget_effects();

    let (result, printed) = run_switch(&host, EMAIL);

    let error = result.expect_err("the Capture would destroy the other Account's Credential");
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{error}");
    assert!(error.to_string().contains("share one Profile"), "{error}");
    assert_eq!(
        host.keychain_item(&shared.keychain_service, &shared.keychain_account)
            .as_deref(),
        Some(SECOND_CREDENTIAL),
        "the shared store still holds what it held: {printed}"
    );
}

#[test]
fn a_switch_onto_an_account_that_shares_a_profile_is_refused() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    let mut registry = registry_of(&host);
    for email in ["some-one@example.com", "some.one@example.com"] {
        registry.upsert(perch::registry::Account {
            identity: probe::Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
    }
    common::save_registry(&host, &registry);
    let store = store_of(&host, "some-one@example.com");
    host.set_keychain_item(&store.keychain_service, &store.keychain_account, CREDENTIAL);
    host.forget_effects();

    let (result, printed) = run_switch(&host, "some.one@example.com");

    let error = result.expect_err("that Profile is not this Account's alone");
    assert_eq!(error.exit_code(), EXIT_CONFLICT, "{error}");
    let said = error.to_string();
    assert!(
        said.contains("some.one@example.com") && said.contains("some-one@example.com"),
        "both Accounts are named, because which two collided is the whole of \
         what a person needs to act: {said}"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and the machine is exactly as it was: {printed}"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "with the live Credential untouched"
    );
}
