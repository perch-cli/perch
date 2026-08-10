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
use perch::commands::add::AddArgs;
use perch::error::{
    EXIT_CONFLICT, EXIT_KEYCHAIN_UNAVAILABLE, EXIT_NOT_FOUND, EXIT_NOTHING_TO_DO,
    EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE, EXIT_QUARANTINED,
};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};
use perch::probe;
use perch::registry::Quarantine;

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
/// rather than on its not having been refused.
///
/// Every one of the markers below used to be checked with a bare `expect` and
/// nothing else, which measures only "not refused" — a Perch that judged the
/// Profile idle and then skipped the Capture entirely passed all five. So the
/// live Credential is Rotated first, and what is asserted is the whole of what a
/// Switch owes: the Rotation went back into the outgoing Account's Profile, the
/// incoming Credential is the live one, and Perch records who is active.
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
    assert_eq!(registry_of(host).active.as_deref(), Some(SECOND_EMAIL));
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
            format!("took {REFRESH_LOCK}"),
            format!("took {LEGACY_LOCK}"),
            format!("took {CONFIG_LOCK}"),
            // Inside the locks, not before them: whether a client is running
            // against either Profile is a statement about a moment, and taking
            // the locks can take seconds.
            format!("read {SECOND_EMAIL}'s Profile"),
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

/// Perch is not the only thing that writes the Default Profile. Somebody who
/// runs `claude` and logs in directly leaves Perch's record of who is active
/// behind, and the live Credential is then a stranger's. Capturing it into the
/// Profile of the Account Perch *believes* is active would overwrite that
/// Account's own Credential with somebody else's — and a later Switch back
/// would make the stranger's Credential live under the Account's name, so
/// Claude Code would act as one person while displaying another.
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

/// The mirror, so the guard above cannot be satisfied by never Capturing at
/// all: an Identity that says nothing is no evidence against, and a Rotation
/// made under a Profile Claude Code has never written an Identity into must
/// still be kept.
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

/// A sessions directory that is there and will not be read is doubt, not
/// emptiness.
///
/// `sudo claude` leaves `sessions` owned by root inside a Profile the user owns,
/// and from then on listing it fails with a permission error rather than saying
/// the directory is absent. Reading that as "nothing is running" would let a
/// Switch Capture over the Credential a client is holding — the mid-task logout
/// ADR 0005 exists to prevent, arriving through the one answer the Host port
/// goes out of its way to keep distinct from an absent directory.
#[test]
fn a_sessions_directory_that_will_not_be_read_stops_the_switch_rather_than_reading_as_empty() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/77.json"), "{}")
        .with_unreadable_file(
            format!("{FIRST_PROFILE}/sessions"),
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// An absent one is the ordinary case and still means nothing is running: a
/// machine where no client has ever started has no such directory, and refusing
/// there would refuse every first Switch.
#[test]
fn a_profile_that_never_ran_a_client_has_no_sessions_directory_and_switches() {
    let host = machine_with_two_accounts();

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    result.expect("nowhere to look is not the same as something to worry about");
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
}

/// The Capture is the write, and it writes into the *outgoing* Account's own
/// Profile — so that Profile being Live is what stops a Switch (ADR 0006).
#[test]
fn switching_away_from_a_profile_a_client_is_running_against_is_refused() {
    let host = client_running_against(machine_with_two_accounts(), FIRST_PROFILE, 77);

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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// A live store holding bytes that are not a Credential is the state a
/// truncating keychain leaves (ADR 0008), and it used to wedge the machine.
/// Every Switch read the live Credential and parsed it — `already_landed` only
/// to ask whether one was there, the Capture only to copy the bytes back out —
/// so the assumption refusal came out of both, and the command that repairs the
/// store by writing a good Credential over the bad one was turned away on the
/// strength of the file it was about to replace. There was no way out through
/// Perch at all.
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

/// The same store, and a Switch onto somebody else: it lands too. The Account
/// being left keeps the Credential its own Profile already held, which is the
/// whole of what the Capture was protecting.
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "the outgoing Account's Profile is untouched: {printed}"
    );
}

/// A store that will not answer is not a store holding rubbish.
///
/// The two arrived at the same place: `read_credential` failed, the Capture was
/// declined, and the Switch went on to write the incoming Credential over
/// whatever was there. For bytes that are not a Credential that is right —
/// nothing is lost, and the write is the repair. For a `.credentials.json` left
/// owned by root by a `sudo claude`, it is not: the file very likely holds the
/// outgoing Account's own Credential, Rotated several times past the copy in
/// its Profile, and the store said nothing at all about what it holds. The
/// Switch destroyed the only good copy and reported success.
#[test]
fn a_live_store_that_will_not_answer_stops_the_switch_rather_than_being_written_over() {
    let host = two_accounts_off_macos();
    host.set_unreadable(CREDENTIALS_PATH, "Permission denied");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let error = result.expect_err("a Credential that cannot be read cannot be Captured");
    assert!(error.to_string().contains("Nothing was changed"), "{error}");
    assert!(
        error.to_string().contains(EMAIL),
        "and names the Account whose Credential it may be: {error}"
    );
    host.forget_unreadable(CREDENTIALS_PATH);
    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "the live store still holds what it held"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "and nothing moved"
    );
}

/// The same machine, and a store that answers with bytes nothing understands:
/// still a declined Capture and still a Switch that lands, because that is the
/// one the store cannot be repaired without.
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

/// The other direction is not the same danger and is not refused (ADR 0027).
///
/// A Switch only ever *reads* the incoming Account's Profile, to copy its
/// Credential into the Default Profile, and reading takes nothing away from the
/// session using it. Refusing this would make a Run and a Switch lock each other
/// out for no reason: the Account you are running in one terminal is exactly the
/// one you would want active in the others.
#[test]
fn switching_onto_a_profile_a_client_is_running_against_lands() {
    let host = client_running_against(machine_with_two_accounts(), SECOND_PROFILE, 4242);

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("reading a Credential out of a Live Profile is safe");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the incoming Account's Credential is the live one"
    );
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
}

/// A `~/.claude.json` managed by stow, chezmoi or yadm, switched over.
///
/// The link is followed and the file in the dotfiles repository is the one that
/// changes — but the half worth asserting is what *survives*: `numStartups`,
/// the project history, the MCP configuration. `patch_identity` reads the file
/// before it writes it, and a read that answered `NotFound` took the branch
/// written for "a Claude Code that has never run here", which composes a whole
/// new `.claude.json` from the Identity alone. Followed by a write that *does*
/// go through the link, that is somebody's dotfiles repository overwritten with
/// a two-key file, committed by whatever runs next.
///
/// Untestable until now: `FakeHost` followed a link on no read at all, so this
/// machine could not be built here.
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

/// A Profile whose `sessions` is a link into the Default Profile reports the
/// Default Profile's clients as its own.
///
/// That is the hazard ADR 0027 names and `reconcile::HELD_BACK` exists to
/// prevent, and nothing could stand on it: reading a linked `sessions` means
/// `list_dir` following the link, which the fake did not do. So the Switch is
/// refused here for a client that is running somewhere else entirely — one
/// Account made unreachable by a link in another Account's directory.
#[test]
fn a_profile_whose_sessions_is_a_link_reads_the_clients_at_the_other_end() {
    let host = client_running_against(machine_with_two_accounts(), "/Users/someone/.claude", 4242);
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
        &session_marker(9999, Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap()),
    );

    assert_the_switch_captured_and_landed(&host, "nothing is holding that Profile");
}

#[test]
fn a_marker_whose_pid_now_belongs_to_a_younger_process_is_not_a_live_profile() {
    // A dead session's marker plus a recycled PID: the process wearing the pid
    // today began after the session the marker records, so it cannot be the
    // client that wrote it (ADR 0022).
    let session_began = Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap();
    let host = machine_with_two_accounts()
        .with_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            &session_marker(4242, session_began),
        )
        .with_live_process_started_at(4242, Utc.with_ymd_and_hms(2026, 8, 4, 11, 0, 0).unwrap());

    assert_the_switch_captured_and_landed(&host, "that client died; the pid was recycled");
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

/// A marker Perch cannot see the *contents* of is a different thing from one
/// whose contents say nothing.
///
/// Root-owned after a `sudo claude`, or halfway through being written by a
/// client that is starting up right now. Nothing about it has been established,
/// so it resolves the way every other doubt in the probe resolves — towards
/// Live — rather than being read as an empty Profile a Switch may write under.
/// That is the mid-task logout ADR 0005 exists to prevent.
#[test]
fn a_marker_that_cannot_be_read_at_all_holds_the_profile_of_a_running_client() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/4242.json"), "")
        .with_unreadable_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
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

/// The same file with nothing running under it is litter, not doubt. A marker
/// nobody can read beside a process that is not there must not refuse every
/// Switch against this Profile for ever.
#[test]
fn an_unreadable_marker_whose_process_is_gone_holds_nothing() {
    let host = machine_with_two_accounts()
        .with_file(format!("{FIRST_PROFILE}/sessions/4242.json"), "")
        .with_unreadable_file(
            format!("{FIRST_PROFILE}/sessions/4242.json"),
            "Permission denied",
        );

    assert_the_switch_captured_and_landed(&host, "no client is holding that Profile");
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
        host.file(perch::host::temp_beside(&host, Path::new(IDENTITY_PATH))),
        None,
        "a write that did not land leaves nothing beside the file Claude Code \
         reads"
    );
}

/// `.claude.json` holds MCP configuration, and an MCP server entry routinely
/// carries an API key in its `env` block. The Identity is patched by writing a
/// replacement and moving it over the file, and a replacement created at the
/// process umask would hand every one of those secrets to every other user on
/// the machine — permanently, from the first Switch, with no race to lose.
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

/// The other half of the same rule: a file that was open stays as it was, since
/// Perch is patching one key of a file it does not own and its permissions are
/// not Perch's to decide either way.
#[test]
fn patching_the_identity_does_not_narrow_a_file_that_was_open() {
    let host = machine_with_two_accounts().with_file_mode(IDENTITY_PATH, 0o644);

    run_switch(&host, SECOND_EMAIL).0.expect("it switches");

    assert_eq!(host.mode_of(IDENTITY_PATH), Some(0o644));
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

/// `claude /logout` empties the live store and leaves `.claude.json` naming
/// whoever was there. Perch's record still says that Account is active, and it
/// is — as a claim about which Credential is live, which is now none.
///
/// Read off the Identity alone that looks like a Switch that already landed,
/// and the command turned away as unnecessary is precisely the one that would
/// put the Credential back. It is the interrupted Switch's half-state reached
/// from the other side, and it wants the same answer: run it.
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

/// Something at a lock path that is not a lock is said, rather than reported as
/// a Claude Code that will not let go.
///
/// A lock is a directory and `remove_dir_all` does not follow the last
/// component, so a plain file there fails with `ENOTDIR` every time. That
/// failure was discarded, which turned it into five attempts of no progress and
/// then "is held by Claude Code and was not given back. … quit it and run this
/// again" — about a path with no Claude Code behind it, where the advice can
/// never work. Every Switch, Run and Renewal failed that way until somebody
/// deleted the path by hand.
#[test]
fn something_at_a_lock_path_that_is_not_a_lock_is_named_rather_than_blamed_on_claude_code() {
    let host = machine_with_two_accounts().with_file(REFRESH_LOCK, "not a lock directory");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let refusal = result.expect_err("nothing can take that lock");
    let said = refusal.to_string();
    assert!(
        said.contains("is not a lock directory"),
        "it says what is wrong: {said}"
    );
    // The file name rather than the whole path: a `Path` renders with the
    // separator of whatever is running the test, so a fixture spelled with `/`
    // comes back mixed on Windows and an assertion on the whole string would be
    // testing the separator rather than the message.
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// A takeover on the last attempt gets the lock it just freed.
///
/// The takeover used to `continue`, which spent the attempt — so a holder that
/// died just before the final try was cleared and then reported as holding the
/// lock this very call had freed.
#[test]
fn a_lock_abandoned_on_the_last_attempt_is_taken_rather_than_reported_as_held() {
    let host = machine_with_two_accounts();
    let now = host.now();
    // The refresh lock goes stale at 60s, and each of the four waits advances
    // the fake clock by a second. Held since 56.5s ago, it reads as alive on
    // attempts one to four and as abandoned on the fifth — the last one there
    // is.
    let host = host.with_dir_held_since(REFRESH_LOCK, now - chrono::Duration::milliseconds(56_500));

    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("the lock was free by the time the last attempt asked");

    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
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

/// The window a liveness refusal asked before the locks leaves open.
///
/// Taking Claude Code's locks can take seconds — the wait is four of them. A
/// `claude` started inside that wait is one a check made beforehand never saw,
/// and the Switch would go on to replace the Credential that session is
/// holding: the mid-task logout ADR 0005 exists to prevent. So the question is
/// asked again once the locks are held and nothing can change the answer.
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
                &session_marker(7788, now),
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

/// Claude Code writes `.claude.json` the first time it is run — onboarding, a
/// theme, a project entry — and writes the `oauthAccount` block into it only
/// when somebody logs in through it. A machine restored from an Export has
/// never logged in through Claude Code, so "the file is there and the block is
/// not" is an ordinary state rather than drift.
///
/// Refusing it wedged the Switch permanently rather than failing it: the
/// Credential is already live and the registry already records the Account by
/// the time the Identity is patched, so every retry got exactly this far and
/// stopped in the same place. The note advising another run was advice that
/// could never work, and hand-editing `.claude.json` was the only way out.
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
        "with every other member of it untouched (ADR 0001): {identity}"
    );
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

/// A Capture needs somewhere to put what it finds, and a Perch that records no
/// active Account has nowhere. Saying so matters because it is the one case
/// where whatever was live is replaced rather than kept anywhere: it belonged
/// to no Account Perch holds, so no Profile is the right home for it.
#[test]
fn switching_with_no_active_account_recorded_says_there_was_nothing_to_capture() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.active = None;
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(SECOND_EMAIL));
}

/// Switching away from a Claude Code that is logged out. Worth saying rather
/// than passing over in silence, because it is the one case where switching
/// *back* to that Account will need a login rather than just working: there was
/// no live Credential to put back into its Profile.
#[test]
fn switching_from_a_logged_out_claude_code_says_there_was_nothing_live_to_capture() {
    let host = machine_with_two_accounts();
    host.keychain_delete(DEFAULT_SERVICE, LOGIN_NAME)
        .expect("the login is given up");

    let (result, printed) = run_switch(&host, SECOND_EMAIL);

    result.expect("a logged-out machine is still one that can be switched");
    assert!(
        printed.contains("There was no live Credential to Capture — Claude Code was logged out."),
        "{printed}"
    );
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and the incoming Credential is live"
    );
}

/// The Switch is three writes to the machine and one to Perch's own record, and
/// only the last of them can fail without the machine having moved. When it
/// does, the note has to say that the Switch worked — otherwise the obvious
/// reading of the failure is that it did not, and the obvious next step is to
/// run it again against a machine that is already there.
#[test]
fn a_switch_that_worked_but_could_not_be_recorded_says_the_machine_did_move() {
    let host = machine_with_two_accounts().with_unwritable_file(REGISTRY_PATH, "read-only");

    let (result, _) = run_switch(&host, SECOND_EMAIL);

    let failed = result.expect_err("the record could not be written");
    let said = failed.to_string();
    assert!(
        said.contains("The Switch itself worked"),
        "the machine moved even though the record did not: {said}"
    );
    assert!(said.contains(SECOND_EMAIL), "{said}");
    assert_eq!(
        live_credential(&host).as_deref(),
        Some(SECOND_CREDENTIAL),
        "and that is what the note claims"
    );
}

/// A machine that is not a Mac, holding the same two Accounts. Off macOS a
/// Profile keeps its Credential in a file (ADR 0020), which is the only way to
/// make one particular store refuse a write while the others still work — a
/// locked keychain locks all of them at once, and stops the Switch a step
/// earlier than these tests are about.
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

/// The Capture is the first write a Switch makes, and the one thing that must
/// never be got wrong: it is the only good copy of the outgoing Account's live
/// Credential (ADR 0006). When it fails, nothing has moved — and the note has
/// to say so plainly, because the reasonable fear on reading a failed Switch is
/// that the Credential was lost somewhere between the two Profiles.
#[test]
fn a_switch_that_cannot_capture_says_nothing_moved_and_moves_nothing() {
    let host = two_accounts_off_macos();
    let outgoing = store_of(&host, EMAIL).credentials_file;
    let host = host.with_unwritable_file(&outgoing, "read-only file");

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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// The live write failing with no active Account recorded. There is no
/// outgoing Profile to reassure anybody about and nothing was Captured, so the
/// note is the one fact that holds: the live Credential was not replaced. It
/// still has to name the Account that did not become active, or the failure
/// says nothing about what was being attempted.
#[test]
fn a_live_write_that_fails_with_nothing_active_names_the_account_that_did_not_land() {
    let host = two_accounts_off_macos();
    let mut registry = registry_of(&host);
    registry.active = None;
    save_registry(&host, &registry);
    let host = host.with_unwritable_file(CREDENTIALS_PATH, "read-only file");

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

/// Between the Capture and the live write, the outgoing Credential is safe in
/// its own Profile and the incoming one is not yet live. The note has to say
/// both halves: nothing was lost, and the Switch did not happen.
#[test]
fn a_switch_that_captured_but_could_not_go_live_says_nothing_was_lost() {
    let host = two_accounts_off_macos().with_unwritable_file(CREDENTIALS_PATH, "read-only file");

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
        registry_of(&host).active.as_deref(),
        Some(EMAIL),
        "the machine never moved"
    );
}

/// The same step, from a logged-out Claude Code: there was no live Credential
/// to Capture, so the reassurance above would be a claim about a write that
/// never happened. What is true instead is that the outgoing Profile is as it
/// was found.
#[test]
fn a_live_write_that_fails_with_nothing_captured_says_the_profile_is_unchanged() {
    let host = two_accounts_off_macos();
    host.remove_file(std::path::Path::new(CREDENTIALS_PATH))
        .expect("the login is given up");
    let host = host.with_unwritable_file(CREDENTIALS_PATH, "read-only file");

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

/// The last step of a Switch, failing with no outgoing Account recorded. The
/// machine has moved — the incoming Credential is live — but `.claude.json`
/// still names whoever it named, and Perch does not know who that was.
#[test]
fn an_identity_that_cannot_be_patched_with_nothing_active_still_says_what_to_run() {
    let host = machine_with_two_accounts().with_unwritable_file(IDENTITY_PATH, "read-only file");
    let mut registry = registry_of(&host);
    registry.active = None;
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

/// A `.claude.json` that is not JSON at all. The Switch has already made the
/// incoming Credential live by the time the file is read, so this is the same
/// half-finished state — and it must be reported as one rather than as a parse
/// error with no advice attached.
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

/// A `.claude.json` that cannot be read at all is different from one that is
/// absent: absent is a Claude Code that has never run here and is written
/// fresh, unreadable is a file that is there and says nothing.
#[test]
fn an_identity_file_that_cannot_be_read_stops_the_switch_at_its_last_step() {
    let host = machine_with_two_accounts().with_unreadable_file(IDENTITY_PATH, "permission denied");
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

/// A Credential Perch holds that is no longer in a shape Perch understands.
/// Quarantining is for a Credential that is *missing*; this one is present and
/// unreadable, which is a refusal that names the Account and changes nothing.
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
    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
}

/// A client launched by a Run hands `CLAUDE_CONFIG_DIR` on to everything it
/// starts, so a `perch switch` typed inside one arrives with a Profile named in
/// its environment. It is not the Default Profile, and taking it for one wrote a
/// third Account's Credential into the Profile the Run is working in — which
/// superseded the copy that Profile held, logged that session out mid-task, and
/// left the registry naming an Account the machine was not on.
///
/// The Run-side half of the same rule is
/// `shared_state_is_read_from_the_default_profile_even_inside_another_run`.
#[test]
fn a_switch_typed_inside_a_run_lands_on_the_default_profile_rather_than_the_runs() {
    let inside_a_run =
        perch::registry::profile_dir_for(&machine_with_three_accounts(), SECOND_EMAIL)
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

/// The other half, as the Run path already states it: a configuration directory
/// somebody deliberately moved is where the live Credential is, and a Switch
/// writes it there. Only a Profile is disqualified, because only a Profile is
/// somewhere Perch itself pointed the variable.
#[test]
fn a_configuration_directory_that_is_not_a_profile_is_where_a_switch_lands() {
    let moved = Path::new("/Users/someone/elsewhere");
    let host = machine_with_two_accounts().with_env("CLAUDE_CONFIG_DIR", &moved.to_string_lossy());

    run_switch(&host, SECOND_EMAIL).0.expect("the Switch lands");

    let store = probe::store_for_profile(&host, moved).expect("USER is set");
    assert_eq!(
        perch::credentials::read(&host, &store)
            .expect("the store could be consulted")
            .map(|held| held.credential)
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

/// Whose Credential the live store holds is decided over the whole of Unicode,
/// not over the ASCII letters of an address.
///
/// Claude Code writes `.claude.json` itself, and nothing makes it agree with the
/// registry about the case of a letter outside ASCII. Compared with ASCII
/// folding, `CAFÉ@example.com` and `café@example.com` are two different people:
/// the Capture decided the live Credential was somebody else's and skipped it,
/// so every Switch away from that Account silently destroyed whatever Rotation
/// had happened while it was active — the poisoning ADR 0006 exists to prevent,
/// on the one Account whose address is not plain ASCII.
#[test]
fn a_rotation_is_captured_however_the_identity_file_cases_a_non_ascii_address() {
    let host = a_machine_on_an_accented_account();
    assert_eq!(registry_of(&host).active.as_deref(), Some(ACCENTED));
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

/// The mirror: a Switch onto the Account the machine is already acting as is
/// recognised as having landed, however the identity file cases the address. It
/// was not, so `perch switch café@example.com` re-ran the whole Switch — with
/// the Capture above declining, which is the destructive half.
#[test]
fn a_switch_onto_the_account_already_active_is_recognised_whatever_the_case() {
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

/// The repair for an interrupted Switch must not destroy a Rotation that
/// happened while the machine was in that half-state.
///
/// After a Switch that stopped before patching `.claude.json`, the incoming
/// Account is live and recorded as active while Claude Code's own file still
/// names the one it was leaving. So on the re-run that Account is both the
/// incoming and the outgoing one, and the Identity — which is exactly what is
/// stale — said the live Credential belonged to somebody else. The Capture was
/// declined on that reading and the write that followed put the Account's older
/// Profile copy over its own Rotation: the only good refresh token, gone, on the
/// command the previous failure told the user to run.
#[test]
fn repairing_an_interrupted_switch_never_writes_over_a_rotation_it_declined_to_save() {
    let host = machine_with_two_accounts().with_unwritable_file(IDENTITY_PATH, "read-only file");
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect_err("the Identity could not be patched");
    assert_eq!(
        registry_of(&host).active.as_deref(),
        Some(SECOND_EMAIL),
        "the incoming Account is live, so Perch records it as active"
    );
    // The user carries on working, and Claude Code Rotates.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ROTATED);
    host.writable_again(IDENTITY_PATH);

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

/// Everything Perch is holding has to survive the slow steps of a Switch, not
/// only Claude Code's locks.
///
/// `perform` renews those between each of its three steps and says why — "a
/// keychain that stops to ask the user for permission stretches either without
/// warning". All three ran under Perch's own registry hold, which was not touched
/// until the `registry::save` afterwards and goes stale in ninety seconds. Let
/// run out, it is `record_active` that refuses: the live Credential belongs to the
/// incoming Account while the registry still names the outgoing one, which sends
/// the *next* Switch to Capture the live Credential into the outgoing Account's
/// Profile and destroy its only copy (ADR 0006). Re-running the Switch does not
/// repair it — `already_there` answers before `record_active` is reached.
///
/// What renewing buys is the accumulation: several steps each comfortably inside
/// the window that together run past it. A single write that outlasts the whole
/// ninety seconds on its own is not something anything at this layer can hold
/// through, so what is asserted is that the hold is kept up as the Switch goes.
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
