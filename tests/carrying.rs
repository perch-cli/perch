//! Behavior tests for the `.claude.json` keys a Run copies into the Profile it
//! is about to launch (ADR everything-but-the-account).
//!
//! The one file Reconcile cannot link, because it holds the Account as well as
//! the person. What every one of these is really asserting is which of those
//! two a key is.

// Every path compared here comes out of the fake's effect log, spelled as the
// code under test wrote it: filtering that log by prefix asks which effects
// landed under a directory, and never whether a path on a machine is inside one.
#![allow(
    clippy::disallowed_methods,
    reason = "the fake's effect log, filtered by the prefix it was written under"
)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use perch::carry;
use perch::host::FakeHost;
use perch::host::fake::Effect;
use perch::host::prelude::*;

/// Where the person's own `.claude.json` is: beside the Default Profile, as
/// Claude Code writes it.
const THE_PERSONS_FILE: &str = IDENTITY_PATH;

/// The repository the person is standing in, which the fake answers `getcwd`
/// with — and therefore the one entry of `projects` a Run is about.
const HERE: &str = "/Users/someone/work";

/// Another repository they have worked in, whose approvals are nothing to do
/// with this Run.
const ELSEWHERE: &str = "/Users/someone/elsewhere";

/// The `.claude.json` of somebody who has been using Claude Code for a while:
/// the Account it is logged in as, the state they have accumulated by using it,
/// the caches Anthropic filled for that one Account, and two repositories'
/// worth of trust and tool approvals.
fn a_file_in_use(email: &str, tips: u32) -> String {
    format!(
        r#"{{
  "numStartups": 41,
  "hasCompletedOnboarding": true,
  "lastOnboardingVersion": "2.1.221",
  "tipsHistory": {{
    "ide-hotkey": {tips}
  }},
  "seenNotifications": ["release-2.1"],
  "cachedUsageUtilization": {{
    "fiveHour": 41
  }},
  "modelAccessCache": {{
    "opus": true
  }},
  "overageCreditGrantCache": {{
    "granted": 0
  }},
  "orgModelDefaultCache": {{
    "default": "opus"
  }},
  "oauthAccount": {{
    "accountUuid": "account-uuid-1",
    "emailAddress": "{email}",
    "organizationUuid": "organization-uuid-1",
    "organizationName": "Acme",
    "organizationRole": "admin"
  }},
  "projects": {{
    "{HERE}": {{
      "hasTrustDialogAccepted": true,
      "allowedTools": ["Bash(git status)"],
      "lastCost": 1.25,
      "lastTotalInputTokens": 90210,
      "lastSessionId": "session-of-{email}"
    }},
    "{ELSEWHERE}": {{
      "hasTrustDialogAccepted": true,
      "allowedTools": ["Bash(rm -rf /)"]
    }}
  }}
}}"#
    )
}

/// Two Accounts declared interchangeable, the first one active and used, the
/// second one's Profile as fresh as `perch add` left it. The ordinary shape of
/// the problem: a long job on one Account and a quick question on the other.
fn machine() -> FakeHost {
    let host = machine_with_two_accounts().with_login(client_exiting(0));
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    host.set_file(THE_PERSONS_FILE, &a_file_in_use(EMAIL, 5));
    host.forget_effects();
    host
}

fn profile_of(host: &FakeHost, email: &str) -> PathBuf {
    perch::registry::profile_dir_for(host, email).expect("home is known")
}

/// The identity file of an Account's Profile, as it stands now.
fn identity_of(host: &FakeHost, email: &str) -> String {
    host.file(profile_of(host, email).join(".claude.json"))
        .expect("a Profile holds an identity file")
}

fn read(host: &FakeHost, email: &str) -> serde_json::Value {
    serde_json::from_str(&identity_of(host, email)).expect("the file is still JSON")
}

/// The clock moving on, so "most recently used" is a question with an answer.
fn used_at(host: &FakeHost, path: impl AsRef<Path>, hour: u32) {
    host.set_now(chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 8, 4, hour, 0, 0).unwrap());
    host.touch(path.as_ref()).expect("the file is there");
}

#[test]
fn onboarding_tips_and_notifications_cross_into_a_fresh_profile() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert_eq!(carried["hasCompletedOnboarding"], serde_json::json!(true));
    assert_eq!(
        carried["lastOnboardingVersion"],
        serde_json::json!("2.1.221")
    );
    assert_eq!(carried["tipsHistory"]["ide-hotkey"], serde_json::json!(5));
    assert_eq!(
        carried["seenNotifications"],
        serde_json::json!(["release-2.1"])
    );
}

/// The trust and tool approvals of the repository the person is standing in, so
/// a second Account in the same terminal does not re-approve them one dialog at
/// a time.
#[test]
fn the_project_entry_for_this_directory_crosses() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert_eq!(
        carried["projects"][HERE]["hasTrustDialogAccepted"],
        serde_json::json!(true)
    );
    assert_eq!(
        carried["projects"][HERE]["allowedTools"],
        serde_json::json!(["Bash(git status)"])
    );
}

#[test]
fn a_project_entry_for_another_directory_stays_behind() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert!(carried["projects"][ELSEWHERE].is_null(), "{carried}");
    assert!(
        !identity_of(&host, SECOND_EMAIL).contains("rm -rf"),
        "{}",
        identity_of(&host, SECOND_EMAIL)
    );
}

#[test]
fn the_directory_the_run_was_typed_in_is_the_one_that_crosses() {
    let host = machine().in_directory(ELSEWHERE);

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert!(!carried["projects"][ELSEWHERE].is_null(), "{carried}");
    assert!(carried["projects"][HERE].is_null(), "{carried}");
}

/// The whole reason the set is named rather than inverted: figures Anthropic
/// gave for one subscription would be shown under another Account's name
/// (ADR a-figure-names-its-account).
#[test]
fn nothing_keyed_to_the_other_account_crosses() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert_eq!(
        carried["oauthAccount"]["emailAddress"],
        serde_json::json!(SECOND_EMAIL),
        "the Profile is still its own Account"
    );
    for cache in [
        "cachedUsageUtilization",
        "modelAccessCache",
        "overageCreditGrantCache",
        "orgModelDefaultCache",
    ] {
        assert!(carried[cache].is_null(), "{cache} crossed: {carried}");
    }
}

#[test]
fn everything_outside_the_keys_that_cross_is_left_as_it_was() {
    let host = machine();
    let before = identity_of(&host, SECOND_EMAIL);

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let after = identity_of(&host, SECOND_EMAIL);
    for line in before.lines().filter(|line| !line.contains("projects")) {
        assert!(after.contains(line), "{line} was rewritten:\n{after}");
    }
    assert!(after.contains(r#""numStartups": 1"#), "{after}");
}

#[test]
fn nothing_crosses_between_accounts_that_share_no_group() {
    let host = machine_with_two_accounts().with_login(client_exiting(0));
    host.set_file(THE_PERSONS_FILE, &a_file_in_use(EMAIL, 5));
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    assert!(carried["hasCompletedOnboarding"].is_null(), "{carried}");
    assert!(carried["projects"][HERE].is_null(), "{carried}");
}

/// The active Account works in the Default Profile and a Run of it lands
/// somewhere else, so an Account's own state has to cross to its own Profile
/// whatever Group it is in.
#[test]
fn the_active_accounts_own_state_crosses_to_its_own_profile() {
    let host = machine_with_two_accounts().with_login(client_exiting(0));
    host.set_file(THE_PERSONS_FILE, &a_file_in_use(EMAIL, 5));
    host.forget_effects();

    run_run(&host, EMAIL).0.expect("the client ran");

    let carried = read(&host, EMAIL);
    assert_eq!(carried["hasCompletedOnboarding"], serde_json::json!(true));
    assert_eq!(
        carried["projects"][HERE]["hasTrustDialogAccepted"],
        serde_json::json!(true)
    );
}

#[test]
fn the_most_recently_used_profile_in_the_group_is_the_one_copied() {
    let host = machine_with_three_accounts().with_login(client_exiting(0));
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    let theirs = profile_of(&host, THIRD_EMAIL).join(".claude.json");
    host.set_file(THE_PERSONS_FILE, &a_file_in_use(EMAIL, 5));
    host.set_file(&theirs, &a_file_in_use(THIRD_EMAIL, 9));

    used_at(&host, theirs, 13);
    used_at(&host, THE_PERSONS_FILE, 14);
    host.forget_effects();
    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        read(&host, SECOND_EMAIL)["tipsHistory"]["ide-hotkey"],
        serde_json::json!(5),
        "the Default Profile was written last, so it is what is copied"
    );
}

#[test]
fn a_profile_used_after_the_default_one_is_what_is_copied() {
    let host = machine_with_three_accounts().with_login(client_exiting(0));
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    let theirs = profile_of(&host, THIRD_EMAIL).join(".claude.json");
    host.set_file(THE_PERSONS_FILE, &a_file_in_use(EMAIL, 5));
    host.set_file(&theirs, &a_file_in_use(THIRD_EMAIL, 9));

    used_at(&host, THE_PERSONS_FILE, 13);
    used_at(&host, theirs, 14);
    host.forget_effects();
    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        read(&host, SECOND_EMAIL)["tipsHistory"]["ide-hotkey"],
        serde_json::json!(9)
    );
}

/// A client rewrites this file wholesale on its way out, so anything Perch
/// spliced into it while one was running would be thrown away — or worse,
/// written over what the client had.
#[test]
fn nothing_is_written_while_a_client_is_running_against_that_profile() {
    let host = machine();
    let profile = profile_of(&host, SECOND_EMAIL);
    host.set_file(
        profile.join("sessions/4242.json"),
        &format!(
            r#"{{"pid":4242,"cwd":"{HERE}","startedAt":{}}}"#,
            host.now().timestamp_millis()
        ),
    );
    let host = host.with_live_process(4242);
    let before = identity_of(&host, SECOND_EMAIL);

    let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(identity_of(&host, SECOND_EMAIL), before);
    assert_eq!(
        outcome, 0,
        "and the Run still launched: a Carry that does nothing is not a refusal"
    );
}

/// The same precondition from the other end: a Run makes the Profile it launches
/// Live (ADR a-run-is-one-shot) *before* it Carries, because until the Marker
/// exists nothing on the machine knows the Run is happening — so the Carry has
/// to discount the Run's own claim.
#[test]
fn a_run_marks_the_profile_live_before_it_carries_and_carries_anyway() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        read(&host, SECOND_EMAIL)["hasCompletedOnboarding"],
        serde_json::json!(true),
        "the Run is not blocked from writing by its own marker"
    );

    let effects = host.effects();
    let carried = effects
        .iter()
        .rposition(|effect| matches!(effect, Effect::Renamed { to, .. } if to == &profile_of(&host, SECOND_EMAIL).join(".claude.json")))
        .expect("the Carry wrote the identity file");
    let marked = effects
        .iter()
        .position(|effect| matches!(effect, Effect::WroteFile(path) if path.starts_with(profile_of(&host, SECOND_EMAIL).join("sessions"))))
        .expect("the Run marked the Profile Live");
    assert!(
        marked < carried,
        "and it marked before it wrote, so nothing else may take the Profile \
         away while it is being prepared"
    );
}

/// A remark rather than a refusal, because what is lost is an onboarding
/// question — but said, because meeting it on every Run with nothing to explain
/// it is worse than the question.
#[test]
fn a_profile_that_cannot_be_written_is_remarked_on_and_the_run_happens_anyway() {
    let identity = {
        let host = machine();
        profile_of(&host, SECOND_EMAIL).join(".claude.json")
    };
    let host = machine().with_unwritable_file(&identity, "permission denied");

    let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(outcome, 0);
    let notes = host.notes();
    assert!(
        notes
            .iter()
            .any(|note| note.contains(".claude.json") && note.contains("launched anyway")),
        "{notes:?}"
    );
}

/// Nothing parses this file, so a value is found by running to the `,` or the
/// brace that ends it — and a source truncated partway through `true` has no
/// such byte. Reachable rather than theoretical: `carry` asks whether anything
/// is running against the *destination*, and the source is a Default Profile a
/// client rewrites wholesale on its way out.
#[test]
fn a_source_that_stops_mid_value_leaves_the_destination_alone() {
    let host = machine();
    let before = identity_of(&host, SECOND_EMAIL);

    let whole = a_file_in_use(EMAIL, 5);
    let cut = whole
        .find("\"hasCompletedOnboarding\": tru")
        .expect("the fixture has the key this is about");
    host.set_file(
        THE_PERSONS_FILE,
        &whole[..cut + "\"hasCompletedOnboarding\": tru".len()],
    );

    let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        outcome, 0,
        "a source nobody can read is not worth a refusal"
    );
    assert_eq!(
        identity_of(&host, SECOND_EMAIL),
        before,
        "nothing crossed, rather than a half-token crossing"
    );
    serde_json::from_str::<serde_json::Value>(&identity_of(&host, SECOND_EMAIL))
        .expect("and what the Profile holds is still JSON");
}

/// A Profile with no `.claude.json` of its own is one there is nowhere to splice
/// a key into, which is the first thing `carry` asks. The fixture is deliberately
/// not the ordinary one, whose Default Profile is full of things to copy from.
#[test]
fn a_profile_with_no_identity_file_of_its_own_is_never_given_one() {
    let host = machine();
    let destination = profile_of(&host, SECOND_EMAIL).join(".claude.json");
    host.remove_file(&destination)
        .expect("`perch add` left one there");
    host.forget_effects();

    let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(outcome, 0, "nothing to carry is not a refusal");
    assert_eq!(
        host.file(&destination),
        None,
        "and a file Perch invented would be one Claude Code never wrote: the \
         keys that cross are spliced into what is already there"
    );
    assert!(
        !host
            .notes()
            .iter()
            .any(|note| note.contains(".claude.json")),
        "nor is it worth remarking on: {:?}",
        host.notes()
    );
}

/// A file rewritten with the bytes it already held is a modification time that
/// lies about when the Profile was last used, which is what the next Run ranks
/// on.
#[test]
fn a_profile_that_already_holds_all_of_it_is_not_written_again() {
    let host = machine();
    run_run(&host, SECOND_EMAIL)
        .0
        .expect("the first client ran");
    host.forget_effects();

    run_run(&host, SECOND_EMAIL)
        .0
        .expect("the second client ran");

    let identity = profile_of(&host, SECOND_EMAIL).join(".claude.json");
    let writes: Vec<Effect> = host
        .effects()
        .into_iter()
        .filter(|effect| match effect {
            Effect::WroteFile(path) | Effect::Renamed { to: path, .. } => *path == identity,
            _ => false,
        })
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn the_keys_that_cross_are_named_in_one_place() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    for key in carry::PERSON_KEYS {
        assert!(!carried[key].is_null(), "{key} did not cross: {carried}");
    }
}

/// The entry holds Claude Code's per-directory figures beside the person's
/// decisions. Carrying it whole put the spend and the session of the Account
/// being left under the name of the one being launched.
#[test]
fn what_one_account_spent_in_this_directory_does_not_cross_with_it() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    let entry = &carried["projects"][HERE];
    assert_eq!(
        entry["hasTrustDialogAccepted"],
        serde_json::json!(true),
        "what the person decided about this directory still crosses: {carried}"
    );
    for figure in ["lastCost", "lastTotalInputTokens", "lastSessionId"] {
        assert!(
            entry[figure].is_null(),
            "{figure} is the other Account's to have spent: {carried}"
        );
    }
}

/// The counterpart to `the_keys_that_cross_are_named_in_one_place`, one level
/// down: the entry is a named set too.
#[test]
fn the_keys_of_a_project_entry_that_cross_are_named_in_one_place() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let carried = read(&host, SECOND_EMAIL);
    let entry = &carried["projects"][HERE];
    for key in ["hasTrustDialogAccepted", "allowedTools"] {
        assert!(
            carry::PROJECT_KEYS.contains(&key) && !entry[key].is_null(),
            "{key} is named and crossed: {carried}"
        );
    }
    for key in ["lastCost", "lastTotalInputTokens", "lastSessionId"] {
        assert!(
            !carry::PROJECT_KEYS.contains(&key),
            "{key} is not one of the names"
        );
    }
}

/// The Profile a Run is launching is never a source, however it is spelled.
///
/// `~/.claude.json` is the Default Profile's identity file wherever the config
/// directory is, so a dotfile manager linking it at a Profile's own hands one
/// file two names — and every Run writes this one, so it outranks every source.
#[test]
fn the_profile_being_launched_is_no_source_under_a_second_spelling() {
    let host = machine_with_three_accounts().with_login(client_exiting(0));
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }

    // The person's state, in the Profile of an Account that is neither active
    // nor the one being run: the only real source of what should cross.
    let theirs = profile_of(&host, THIRD_EMAIL).join(".claude.json");
    host.set_file(&theirs, &a_file_in_use(THIRD_EMAIL, 9));

    // `~/.claude.json` linked at the Profile this Run is about to launch.
    let destination = profile_of(&host, SECOND_EMAIL).join(".claude.json");
    host.set_file(&destination, &a_file_in_use(SECOND_EMAIL, 1));
    host.remove_file(Path::new(THE_PERSONS_FILE))
        .expect("the person's file is there to replace");
    host.link(
        perch::host::Link::Symbolic,
        &destination,
        Path::new(THE_PERSONS_FILE),
    )
    .expect("the link is made");

    // The destination is what every Run writes, so it is always the newest.
    used_at(&host, &theirs, 10);
    used_at(&host, &destination, 12);
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        read(&host, SECOND_EMAIL)["tipsHistory"]["ide-hotkey"],
        serde_json::json!(9),
        "the one real source is the Account that is neither active nor launched"
    );
}
