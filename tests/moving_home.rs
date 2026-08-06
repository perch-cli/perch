//! Behaviour: an installation that is still in the old place moves itself.
//!
//! Perch used to keep its state in `~/.perch`. A tool's state is not something
//! anybody reads by hand, so it lives under `~/.config` now — and the move
//! cannot be a rename, because a Profile's keychain namespace is derived from
//! the Profile's path (ADR 0001). Renaming the directory would leave every
//! macOS Credential filed under a service name nothing derives any more: every
//! Account Quarantined, with the Credentials still on the machine and out of
//! reach.

mod common;

use std::path::PathBuf;

use common::*;
use perch::host::{FakeHost, Host, Platform};

/// Everything the migration says, which is everything it said to stderr.
///
/// Not to the command's own writer, and this is the point rather than an
/// implementation detail: the command that happens to trigger the move may be a
/// `perch status --json` in somebody's shell prompt, and prose on stdout ahead
/// of the document is prose in the middle of whatever is parsing it.
fn said_about_the_move(host: &FakeHost) -> String {
    host.notes().join("\n")
}

/// Both homes are asked for rather than spelled out, because a path is not a
/// string: `PathBuf::join` uses the platform's own separator, and a fixture
/// written with forward slashes would quietly match nothing on Windows — which
/// is a test that passes by never arranging the thing it is about.
fn old_home(host: &FakeHost) -> PathBuf {
    perch::registry::perch_home_before_the_move(host).expect("home is known")
}

fn new_home(host: &FakeHost) -> PathBuf {
    perch::registry::perch_home(host).expect("home is known")
}

/// A machine holding two Accounts, with everything Perch holds put back where
/// the version before this one kept it.
fn an_installation_in_the_old_place(host: &FakeHost) {
    let (old, new) = (old_home(host), new_home(host));

    for path in host.paths_under(&new) {
        let under = path.strip_prefix(&new).expect("under the new home");
        let contents = host.file(&path).expect("a file");
        host.set_file(old.join(under), &contents);
    }
    // The keychain items go with them: a Profile's namespace is derived from
    // its path, so an item filed under the new path is not one the old
    // installation could ever have written.
    for email in [EMAIL, SECOND_EMAIL] {
        let now = store_of(host, email);
        let then = perch::probe::store_for_profile(
            host,
            &old.join("profiles").join(perch::registry::slug(email)),
        )
        .expect("USER is set");
        if let Some(held) = host.keychain_item(&now.keychain_service, LOGIN_NAME) {
            host.set_keychain_item(&then.keychain_service, LOGIN_NAME, &held);
        }
        host.forget_keychain_item(&now.keychain_service, LOGIN_NAME);
    }
    host.remove_dir_all(&new)
        .expect("nothing is in the new place yet");
}

#[test]
fn an_installation_in_the_old_place_is_carried_across_credentials_and_all() {
    let host = machine_with_two_accounts();
    let before = registry_of(&host);
    an_installation_in_the_old_place(&host);

    let (result, printed) = run_list(&host, false);

    result.expect("the listing runs");
    assert!(
        said_about_the_move(&host).contains(&new_home(&host).display().to_string()),
        "it says what it did:\n{}",
        said_about_the_move(&host)
    );
    assert!(
        !printed.contains("Moving"),
        "and says it beside the listing rather than in it:\n{printed}"
    );
    assert_eq!(
        registry_of(&host),
        before,
        "every Account, Alias, Group and figure came across unchanged"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and each Credential is readable from the Profile's new path, which is \
         a different keychain namespace from the one it was filed under"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL)
    );
    assert!(
        !host.path_exists(&old_home(&host)),
        "and nothing is left in the home directory"
    );
}

/// The point of moving: every command after the first says nothing about it.
#[test]
fn the_move_happens_once_and_is_not_mentioned_again() {
    let host = machine_with_two_accounts();
    an_installation_in_the_old_place(&host);

    run_list(&host, false)
        .0
        .expect("the first listing moves it");
    let after_the_move = host.notes().len();
    let (result, printed) = run_list(&host, false);

    result.expect("the second listing runs");
    assert!(!printed.contains("Moving"), "{printed}");
    assert_eq!(
        host.notes().len(),
        after_the_move,
        "and nothing further is said about it: {:?}",
        host.notes()
    );
}

/// The reason the migration says nothing on stdout. `perch status --json` is
/// advertised for scripts, and the first run after an upgrade is exactly when
/// the move happens — so it is exactly the run whose document would otherwise
/// arrive with prose in front of it.
#[test]
fn the_move_leaves_a_json_document_a_script_can_still_parse() {
    let host = machine_with_two_accounts();
    an_installation_in_the_old_place(&host);

    let (result, printed) = run_status(&host, true);

    result.expect("the status runs");
    serde_json::from_str::<serde_json::Value>(&printed)
        .unwrap_or_else(|err| panic!("the document is the whole of stdout: {err}\n{printed}"));
    assert!(
        said_about_the_move(&host).contains("Moving"),
        "and the move is still reported, on stderr: {:?}",
        host.notes()
    );
}

/// Off macOS the Credential is a file inside the Profile, so what has to travel
/// is the file — under a directory only its owner may enter, exactly as the
/// Profile it came from was.
#[test]
fn a_credential_kept_in_a_file_travels_as_a_private_file() {
    let host = logged_in_machine_off_macos();
    run_list(&host, false).0.expect("the login is adopted");
    an_installation_in_the_old_place(&host);

    let said_before = host.notes().len();
    run_list(&host, false).0.expect("it moves");
    assert!(host.notes().len() > said_before, "the move is reported");

    let store = store_of(&host, EMAIL);
    assert_eq!(
        host.file(&store.credentials_file).as_deref(),
        Some(CREDENTIAL)
    );
    assert_eq!(host.mode_of(&store.credentials_file), Some(0o600));
    assert_eq!(host.mode_of(&store.config_dir), Some(0o700));
    assert_eq!(host.platform(), Platform::Other, "as arranged");
}

/// `$PERCH_HOME` is somebody saying where their state lives. Moving it would be
/// answering a question they have already answered.
#[test]
fn an_installation_that_was_told_where_to_live_is_left_alone() {
    let host = machine_with_claude_code()
        .with_env("PERCH_HOME", "/Users/someone/elsewhere")
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(IDENTITY_PATH, IDENTITY_FILE);

    let (result, printed) = run_list(&host, false);

    result.expect("the listing runs");
    assert!(!printed.contains("Moving"), "{printed}");
    assert!(
        !said_about_the_move(&host).contains("Moving"),
        "{:?}",
        host.notes()
    );
    assert!(host.path_exists(&PathBuf::from("/Users/someone/elsewhere/registry.json")));
}

/// The migration's last act is to delete the old home, which makes it the one
/// reader that cannot afford to be lenient. A registry written by a newer Perch
/// carries fields this build does not know about; parsing it here and writing
/// back what survived would drop them, and then remove the only copy that still
/// had them. Refused instead, exactly as it is refused everywhere else.
#[test]
fn a_registry_from_a_newer_perch_is_refused_rather_than_migrated_and_deleted() {
    let host = machine_with_two_accounts();
    an_installation_in_the_old_place(&host);

    let old = old_home(&host).join("registry.json");
    let from_the_future = host
        .file(&old)
        .expect("the old registry is there")
        .replace("\"version\": 3", "\"version\": 99");
    host.set_file(&old, &from_the_future);

    let (result, _) = run_list(&host, false);

    let refused = result.expect_err("this build cannot understand it");
    assert!(refused.to_string().contains("newer Perch"), "{refused}");
    assert_eq!(
        host.file(&old).as_deref(),
        Some(from_the_future.as_str()),
        "and the only copy that has those fields is still where it was"
    );
}

/// An address with no alphanumeric character in it slugs to nothing, and
/// joining nothing onto a path gives back the path — so such an Account's
/// Profile *is* `profiles/`, the directory holding every other Account's. The
/// migration reads from one of those directories and deletes the home it is in,
/// so it goes through the same guard every other derivation does rather than
/// joining a slug itself.
#[test]
fn an_account_that_names_no_profile_stops_the_move_rather_than_taking_it() {
    let host = machine_with_two_accounts();
    an_installation_in_the_old_place(&host);

    let old = old_home(&host).join("registry.json");
    let degenerate = host
        .file(&old)
        .expect("the old registry is there")
        .replace(SECOND_EMAIL, "@");
    host.set_file(&old, &degenerate);

    let (result, _) = run_list(&host, false);

    let refused = result.expect_err("no Profile can be named after that address");
    assert!(
        refused
            .to_string()
            .contains("no character a Profile directory can be named after"),
        "{refused}"
    );
    assert!(
        host.path_exists(&old_home(&host)),
        "and both homes are left standing, so the registry can be put right by hand"
    );
}
