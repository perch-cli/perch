//! Contract tests for where a Credential lives (ADR 0020), asserted against
//! the Claude Code that is actually installed and against a real filesystem.
//!
//! These are the cross-platform half of the contract suite; the macOS-specific
//! beliefs are in `contract.rs`. Nothing here reads, writes or prints a real
//! login: every one of them works in a config directory of its own, with a
//! Credential that is not one.

use std::path::{Path, PathBuf};
use std::process::Command;

use perch::host::{self, Host, RealHost};
use perch::probe;

/// A Credential in the shape Claude Code stores, holding nothing that is worth
/// anything to anybody.
const NOT_A_CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-not-a-token","refreshToken":"sk-ant-ort01-not-a-token","expiresAt":4102444800000,"scopes":["user:inference"],"subscriptionType":"pro"}}"#;

/// [`probe::assumption::CREDENTIAL_LOCATION`], for the half of it that holds
/// off macOS, and the only load-bearing belief there: the plaintext store sits
/// inside the config directory Claude Code was given (ADR 0007, ADR 0020).
/// Everything that makes a Profile a private place for a Credential rests on it
/// — get it wrong and every Profile shares one file, which is every Account
/// sharing one login.
///
/// Asserted the way it is relied on. Perch says where the store of a config
/// directory is, a Credential is put exactly there, and the installed Claude
/// Code is asked who it is: it answers out of that file, having answered
/// "nobody" for the same directory a moment earlier. The empty answer is half
/// the test — without it, a Claude Code reading the real login on this machine
/// would look like one honouring the directory.
///
/// On macOS this also asserts that the plaintext store is consulted when the
/// keychain holds nothing for that directory, which is the fallback half of the
/// same composite. If that stops being true, this is the test that should say
/// so.
#[test]
fn claude_code_reads_the_credential_perch_would_write_for_a_config_directory() {
    let host = RealHost::new();
    let config_dir =
        std::env::temp_dir().join(format!("perch-contract-config-{}", std::process::id()));
    let _ = host.remove_dir_all(&config_dir);
    host.create_private_dir_all(&config_dir)
        .expect("a config directory of our own");

    let Some(logged_out) = auth_status(&config_dir) else {
        let _ = host.remove_dir_all(&config_dir);
        eprintln!("skipping: no Claude Code on this machine to ask");
        return;
    };

    let store = probe::credentials_file_for(&config_dir);
    host.write_private_file(&store, NOT_A_CREDENTIAL)
        .expect("a Credential goes where Perch would put one");
    let logged_in = auth_status(&config_dir);
    let _ = host.remove_dir_all(&config_dir);

    assert!(
        !logged_out,
        "a config directory holding nothing must read as logged out, or this \
         test is about the login on this machine rather than about {}",
        config_dir.display()
    );
    assert_eq!(
        logged_in,
        Some(true),
        "Claude Code did not find the Credential at {}, so Perch would be \
         writing one where nothing reads it",
        store.display()
    );
}

/// A file Perch writes a Credential into is readable by its owner and nobody
/// else, on a real filesystem rather than the fake one.
#[test]
#[cfg(unix)]
fn a_credential_file_reaches_the_disk_private() {
    let host = RealHost::new();
    let dir = std::env::temp_dir().join(format!("perch-contract-store-{}", std::process::id()));
    let file = probe::credentials_file_for(&dir);
    let _ = host.remove_dir_all(&dir);

    host.write_private_file(&file, NOT_A_CREDENTIAL)
        .expect("a Credential can be stored");
    let mode = host.file_mode(&file).expect("a mode").expect("on unix");
    let dir_mode = host.file_mode(&dir).expect("a mode").expect("on unix");
    let _ = host.remove_dir_all(&dir);

    assert_eq!(mode, host::PRIVATE_FILE_MODE, "{mode:04o}");
    assert_eq!(dir_mode, host::PRIVATE_DIR_MODE, "{dir_mode:04o}");
}

/// The whole path a Credential takes into a new Profile, on a real filesystem:
/// the directory is made, the Credential goes in, and neither is left open to
/// anybody else along the way.
///
/// The two steps have different owners — `profile::create` makes the directory
/// and [`perch::credentials`] writes the file — which is exactly how a
/// directory ends up with the mode of whoever got there first. Run off macOS,
/// where the store is the file, so nothing here goes near a real keychain.
#[test]
#[cfg(all(unix, not(target_os = "macos")))]
fn a_profile_is_private_from_the_moment_it_exists() {
    let host = RealHost::new();
    let dir = std::env::temp_dir().join(format!("perch-contract-profile-{}", std::process::id()));
    let _ = host.remove_dir_all(&dir);

    let store =
        perch::profile::create(&host, &dir, NOT_A_CREDENTIAL).expect("a Profile is created");
    let on_the_directory = host.file_mode(&dir).expect("a mode").expect("on unix");
    let on_the_file = host
        .file_mode(&store.credentials_file)
        .expect("a mode")
        .expect("on unix");
    let _ = host.remove_dir_all(&dir);

    assert_eq!(
        on_the_directory,
        host::PRIVATE_DIR_MODE,
        "the Profile is {on_the_directory:04o}, so anyone can list what it holds"
    );
    assert_eq!(on_the_file, host::PRIVATE_FILE_MODE, "{on_the_file:04o}");
}

/// Whether the installed Claude Code considers itself logged in when pointed at
/// a config directory, or `None` when there is no Claude Code to ask.
///
/// `auth status` is the one question that reads the credential store and
/// answers locally. It is asked with `CLAUDE_CONFIG_DIR` set, so the machine's
/// own login is neither read nor disturbed.
fn auth_status(config_dir: &Path) -> Option<bool> {
    let output = Command::new(installed_claude_code()?)
        .args(["auth", "status"])
        .env("CLAUDE_CONFIG_DIR", config_dir)
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&output.stdout);

    // Read as loosely as the question allows: this is a contract test about a
    // path, and an unrecognisable reply is a reason to say nothing rather than
    // to fail over the shape of an answer Perch never parses in earnest.
    if said.contains("\"loggedIn\": true") {
        Some(true)
    } else if said.contains("\"loggedIn\": false") {
        Some(false)
    } else {
        eprintln!("`claude auth status` no longer answers in a shape this test reads: {said}");
        None
    }
}

/// The Claude Code `claude` runs, with any symlinks followed.
fn installed_claude_code() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join("claude"))
        .find(|candidate| candidate.is_file())?;
    std::fs::canonicalize(found).ok()
}
