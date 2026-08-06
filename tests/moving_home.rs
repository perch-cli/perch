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

use common::*;
use perch::host::{FakeHost, Host, Platform};

const OLD_HOME: &str = "/Users/someone/.perch";
const NEW_HOME: &str = "/Users/someone/.config/.perch";

/// A machine holding two Accounts, with everything Perch holds put back where
/// the version before this one kept it.
fn an_installation_in_the_old_place(host: &FakeHost) {
    for path in host.paths_under(NEW_HOME) {
        let moved = path
            .to_string_lossy()
            .replace(NEW_HOME, OLD_HOME)
            .to_string();
        let contents = host.file(&path).expect("a file");
        host.set_file(&moved, &contents);
    }
    // The keychain items go with them: a Profile's namespace is derived from
    // its path, so an item filed under the new path is not one the old
    // installation could ever have written.
    for email in [EMAIL, SECOND_EMAIL] {
        let now = store_of(host, email);
        let then = perch::probe::store_for_profile(
            host,
            std::path::Path::new(&format!(
                "{OLD_HOME}/profiles/{}",
                perch::registry::slug(email)
            )),
        )
        .expect("USER is set");
        if let Some(held) = host.keychain_item(&now.keychain_service, LOGIN_NAME) {
            host.set_keychain_item(&then.keychain_service, LOGIN_NAME, &held);
        }
        host.forget_keychain_item(&now.keychain_service, LOGIN_NAME);
    }
    host.remove_dir_all(std::path::Path::new(NEW_HOME))
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
        printed.contains(NEW_HOME),
        "it says what it did:\n{printed}"
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
        !host.path_exists(std::path::Path::new(OLD_HOME)),
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
    let (result, printed) = run_list(&host, false);

    result.expect("the second listing runs");
    assert!(!printed.contains("Moving"), "{printed}");
}

/// Off macOS the Credential is a file inside the Profile, so what has to travel
/// is the file — under a directory only its owner may enter, exactly as the
/// Profile it came from was.
#[test]
fn a_credential_kept_in_a_file_travels_as_a_private_file() {
    let host = logged_in_machine_off_macos();
    run_list(&host, false).0.expect("the login is adopted");
    an_installation_in_the_old_place(&host);

    run_list(&host, false).0.expect("it moves");

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
    assert!(host.path_exists(std::path::Path::new(
        "/Users/someone/elsewhere/registry.json"
    )));
}
