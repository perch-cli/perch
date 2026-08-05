//! Behaviour: what a Switch does, in what order, and what it refuses.
//!
//! The order of the effects is asserted rather than inferred, because it is the
//! visible contract with a Claude Code running at the same time (ADR 0006): the
//! Capture has to reach the outgoing Profile before the live store is written,
//! the Identity has to be patched last, and all of it has to happen inside
//! Claude Code's own locks, taken in Claude Code's own order.

mod common;

use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use common::*;
use perch::error::{
    EXIT_INVALID, EXIT_KEYCHAIN_UNAVAILABLE, EXIT_NOT_FOUND, EXIT_NOTHING_TO_DO,
    EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE,
};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};
use perch::probe;

const REFRESH_LOCK: &str = "/Users/someone/.claude/.oauth_refresh.lock";
const LEGACY_LOCK: &str = "/Users/someone/.claude.lock";
const CONFIG_LOCK: &str = "/Users/someone/.claude.json.lock";

const FIRST_PROFILE: &str = "/Users/someone/.perch/profiles/someone-example-com";
const SECOND_PROFILE: &str = "/Users/someone/.perch/profiles/overflow-example-com";

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

fn stored_credential(host: &FakeHost, email: &str) -> Option<String> {
    host.keychain_item(&profile_service(host, email), LOGIN_NAME)
}

fn identity_file(host: &FakeHost) -> String {
    host.file(IDENTITY_PATH)
        .expect("the identity file is there")
}

/// The effects a Switch is judged on, in the order they reached the Host, said
/// the way the design talks about them. Everything else Perch touches — its own
/// registry, the directories it makes sure exist — is left out, so what the
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

/// A Claude Code running against a Profile: the marker file it writes for the
/// session — naming its process and when the session began — and a process that
/// has been there since before it.
fn client_running_against(host: FakeHost, profile_dir: &str, pid: u32) -> FakeHost {
    let marker = session_marker(pid, host.now());
    host.with_file(format!("{profile_dir}/sessions/{pid}.json"), &marker)
        .with_live_process(pid)
}

/// The marker Claude Code writes for a session that began at `began`, in the
/// shape the probe believes in.
fn session_marker(pid: u32, began: chrono::DateTime<chrono::Utc>) -> String {
    format!(
        r#"{{"pid":{pid},"cwd":"/Users/someone/work","startedAt":{}}}"#,
        began.timestamp_millis()
    )
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
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
            // Read before any lock is taken: a Profile nothing is running
            // against does not change underneath Perch.
            format!("read {SECOND_EMAIL}'s Profile"),
            format!("took {REFRESH_LOCK}"),
            format!("took {LEGACY_LOCK}"),
            format!("took {CONFIG_LOCK}"),
            "read the live store".to_string(),
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
    // The state that makes this possible to get wrong: a Switch interrupted
    // between writing the Credential and patching the Identity, so that Perch
    // and Claude Code disagree about who is active. Anything copying "the
    // Identity that is live" into "the Account being left" copies the wrong one
    // here, and nothing afterwards would ever correct it.
    let host = machine_with_two_accounts().with_unwritable_file(IDENTITY_PATH, "read-only file");
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");
    host.writable_again(IDENTITY_PATH);

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
        printed.contains(&format!("Captured {EMAIL}'s live Credential")),
        "{printed}"
    );
    assert!(
        printed.contains(&format!("Switched to {SECOND_EMAIL}")),
        "{printed}"
    );
    assert!(
        printed.contains("Utilization") && printed.contains("never observed"),
        "an Account nobody has read a figure for says so rather than nothing: {printed}"
    );
    assert!(
        host.http_calls().is_empty(),
        "Utilization is served from cache and a Switch never fetches (ADR 0015)"
    );
}

#[test]
fn switching_onto_a_profile_a_client_is_running_against_is_refused() {
    let host = client_running_against(machine_with_two_accounts(), SECOND_PROFILE, 4242);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the Profile is Live");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(error.to_string().contains("Nothing was changed"), "{error}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "nothing was written"
    );
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

#[test]
fn switching_away_from_a_profile_a_client_is_running_against_is_refused_too() {
    // The Capture writes into the outgoing Account's Profile, so that Profile
    // being Live is the same danger from the other end.
    let host = client_running_against(machine_with_two_accounts(), FIRST_PROFILE, 77);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the outgoing Profile is Live");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains(EMAIL), "{error}");
}

#[test]
fn a_marker_left_behind_by_a_client_that_died_is_not_a_live_profile() {
    let host = machine_with_two_accounts().with_file(
        format!("{SECOND_PROFILE}/sessions/9999.json"),
        &session_marker(9999, Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap()),
    );

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("nothing is holding that Profile");
}

#[test]
fn a_marker_whose_pid_now_belongs_to_a_younger_process_is_not_a_live_profile() {
    // A dead session's marker plus a recycled PID: the process wearing the pid
    // today began after the session the marker records, so it cannot be the
    // client that wrote it (ADR 0022).
    let session_began = Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap();
    let host = machine_with_two_accounts()
        .with_file(
            format!("{SECOND_PROFILE}/sessions/4242.json"),
            &session_marker(4242, session_began),
        )
        .with_live_process_started_at(4242, Utc.with_ymd_and_hms(2026, 8, 4, 11, 0, 0).unwrap());

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("that client died; the pid was recycled");
}

#[test]
fn a_marker_that_does_not_say_when_its_session_began_is_no_evidence_of_a_client() {
    // The marker parses but is not the shape Perch believes in. A Profile is
    // Live when something says so, not when nothing does.
    let host = machine_with_two_accounts()
        .with_file(
            format!("{SECOND_PROFILE}/sessions/4242.json"),
            r#"{"pid":4242,"cwd":"/Users/someone/work"}"#,
        )
        .with_live_process(4242);

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("an uncorroborated marker does not hold a Profile");
}

#[test]
fn a_marker_that_is_not_json_is_no_evidence_of_a_client() {
    let host = machine_with_two_accounts()
        .with_file(
            format!("{SECOND_PROFILE}/sessions/4242.json"),
            "not a marker at all",
        )
        .with_live_process(4242);

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("a marker Perch cannot read is not evidence");
}

#[test]
fn a_live_process_whose_start_cannot_be_read_is_a_refusal_naming_the_assumption() {
    // The marker is well-shaped and its process is alive, but the operating
    // system will not say when that process began — so the marker can be
    // neither corroborated nor dismissed, and guessing either way is wrong.
    let host = machine_with_two_accounts()
        .with_file(
            format!("{SECOND_PROFILE}/sessions/4242.json"),
            &session_marker(4242, Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap()),
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
fn a_switch_that_cannot_patch_the_identity_says_what_it_left_where() {
    let host = machine_with_two_accounts().with_unwritable_file(IDENTITY_PATH, "read-only file");

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
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
    assert_eq!(
        host.file(format!("{IDENTITY_PATH}.perch-tmp")),
        None,
        "a write that did not land leaves nothing beside the file Claude Code \
         reads"
    );
}

#[test]
fn running_the_switch_again_finishes_a_job_that_stopped_half_way() {
    let host = machine_with_two_accounts().with_unwritable_file(IDENTITY_PATH, "read-only file");
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");

    // The file can be written again — the permission was fixed, the disk freed.
    host.writable_again(IDENTITY_PATH);
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
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Took(_))),
        "no lock is taken, and no Credential is rewritten, for a Switch that \
         would change nothing"
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
fn a_lock_somebody_is_holding_stops_the_switch_without_changing_anything() {
    let host = machine_with_two_accounts();
    let now = host.now();
    // Held, and touched just now: somebody is alive behind it. The fake clock
    // moves as Perch waits, so this test spends none of the time it describes.
    let host = host.with_dir_held_since(REFRESH_LOCK, now);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the lock is somebody else's");
    assert!(
        error.to_string().contains("Claude Code is holding"),
        "{error}"
    );
    assert!(error.to_string().contains("Nothing was changed"), "{error}");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
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
fn a_group_is_not_a_switch_target_in_this_form() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");

    let (result, _) = run_switch(&host, "work");

    let error = result.expect_err("a Group names more than one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(error.to_string().contains("`work` is a Group."), "{error}");
}

#[test]
fn an_account_whose_credential_perch_no_longer_holds_is_refused_by_name() {
    let host = machine_with_two_accounts();
    host.forget_keychain_item(&profile_service(&host, SECOND_EMAIL), LOGIN_NAME);

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("there is no Credential to make live");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(error.to_string().contains("perch relogin"), "{error}");
    assert_eq!(live_credential(&host).as_deref(), Some(CREDENTIAL));
}

#[test]
fn a_locked_keychain_reads_as_locked_rather_than_as_a_missing_account() {
    let host = machine_with_two_accounts();
    host.lock_keychain("The user name or passphrase you entered is not correct");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("the keychain cannot be consulted");
    assert_eq!(error.exit_code(), EXIT_KEYCHAIN_UNAVAILABLE);
}
