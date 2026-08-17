//! The beliefs that can only be checked against state the developer owns and
//! did not offer: their login keychain, their `~/.claude`, the Claude Code they
//! have installed, and whatever clients they happen to have open (ADR 0007,
//! ADR 0050).
//!
//! That is the whole of why this suite is held back by the `your-machine`
//! feature, and it is a question of consent rather than of damage. A read of
//! the developer's real state counts as much as a write: nothing here is the
//! repository's to determine, and a suite that reported green on Tuesday and
//! skipped on Wednesday — same machine, same commit, according to whether a
//! client was open — is the one thing a default-on suite must never be.
//! Everything that works in a directory of its own runs by default instead, in
//! `conformance.rs` and `corroboration.rs`.
//!
//! Gated per test rather than per file. A file-level `#![cfg(target_os =
//! "macos")]` is what once narrowed a claim about every filesystem to one
//! platform without anybody choosing it, so the keychain cases say `macos` for
//! themselves and the ones that hold everywhere say nothing.
//!
//! On a machine with no Claude Code and no login — a CI runner, for instance —
//! each test asserts the outcome that situation should produce, rather than
//! quietly passing. None of them ever prints a Credential.

use std::path::{Path, PathBuf};
use std::process::Command;

use perch::error::PerchError;
use perch::host::{Host, RealHost};
#[cfg(target_os = "macos")]
use perch::keychain::KeychainError;
use perch::probe;
#[cfg(target_os = "macos")]
use perch::probe::Verdict;

/// Set to any value to skip the tests that touch the real keychain, for
/// environments where the login keychain cannot be unlocked. CI does not set it.
#[cfg(target_os = "macos")]
const SKIP_KEYCHAIN: &str = "PERCH_SKIP_KEYCHAIN";

#[cfg(target_os = "macos")]
fn skipping_keychain() -> bool {
    if std::env::var_os(SKIP_KEYCHAIN).is_some() {
        eprintln!("skipping: {SKIP_KEYCHAIN} is set");
        true
    } else {
        false
    }
}

/// A service name of this test's own, so nothing here can collide with the item
/// Claude Code keeps.
#[cfg(target_os = "macos")]
fn test_service(name: &str) -> String {
    format!("Perch test-{name}-{}", std::process::id())
}

/// A Credential in the shape Claude Code stores, holding nothing that is worth
/// anything to anybody.
const NOT_A_CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-not-a-token","refreshToken":"sk-ant-ort01-not-a-token","expiresAt":4102444800000,"scopes":["user:inference"],"subscriptionType":"pro"}}"#;

#[test]
fn the_installed_claude_code_reports_a_version_perch_can_parse() {
    let host = RealHost::new();

    match probe::claude_version(&host) {
        Ok(version) => {
            let parts: Vec<&str> = version.split('.').collect();
            assert!(
                parts.len() >= 2 && parts.iter().take(2).all(|p| p.parse::<u32>().is_ok()),
                "`claude --version` no longer starts with a version: {version}"
            );
        }
        Err(error) => {
            // No Claude Code here. The only acceptable outcome is a refusal
            // that says so.
            assert!(
                matches!(error, PerchError::ProbeRefused { .. }),
                "a missing Claude Code must be a refusal, not {error}"
            );
            assert!(error.to_string().contains("Claude Code is installed"));
        }
    }
}

/// The load-bearing belief: `Claude Code-credentials-<sha256(dir)[0:8]>`. Get it
/// wrong and Perch stores every Credential in a namespace no Profile is ever
/// read from, silently.
///
/// It can only be checked against reality on a machine whose `CLAUDE_CONFIG_DIR`
/// is set and logged in — Claude Code has to have written the item for there to
/// be anything to find. Where that holds, this fails the moment the derivation
/// drifts; elsewhere it asserts what it can and says what it skipped.
#[test]
#[cfg(target_os = "macos")]
fn a_non_default_config_directory_finds_its_credential_under_the_derived_name() {
    if skipping_keychain() {
        return;
    }
    let Some(config_dir) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
        eprintln!(
            "skipping: CLAUDE_CONFIG_DIR is unset, so there is no hashed \
             service name on this machine to check against"
        );
        return;
    };

    let host = RealHost::new();
    let store = probe::default_store(&host).expect("USER is set");
    let account = &store.keychain_account;

    match host.keychain_get(&store.keychain_service, account) {
        Ok(credential) => {
            assert!(
                credential.contains("claudeAiOauth"),
                "the derived name found something that is not a Credential"
            );
            assert!(
                matches!(
                    host.keychain_get(probe::DEFAULT_SERVICE, account),
                    Err(KeychainError::NotFound { .. })
                ),
                "a non-default config directory must not be using the bare name"
            );
        }
        Err(KeychainError::NotFound { .. }) => {
            eprintln!(
                "skipping: {} is not logged in, so the derivation cannot be \
                 confirmed against a real item",
                std::path::Path::new(&config_dir).display()
            );
        }
        Err(error) => panic!("the keychain could not be consulted: {error}"),
    }
}

#[test]
#[cfg(target_os = "macos")]
fn the_installed_claude_code_stores_what_perch_expects_to_find() {
    if skipping_keychain() {
        return;
    }
    let host = RealHost::new();

    // The Default Profile as Perch means it, which is what this asserts about:
    // the directory the installed Claude Code falls back to, never a Profile.
    let store = perch::registry::the_default_profile(&host).expect("home is known");

    match probe::probe(&host, store) {
        Ok(Verdict::Recognized(findings)) => {
            // A real login on this machine: every belief held.
            assert!(findings.identity.email.contains('@'));
            assert!(
                findings.credential.as_str().contains("claudeAiOauth"),
                "the credential store no longer holds a claudeAiOauth block"
            );
            assert!(host.path_exists(&findings.store.identity_file));
        }
        Ok(Verdict::NoLogin { store, .. }) => {
            // Nothing logged in. Assert the beliefs that can still be checked.
            assert_eq!(
                host.keychain_get(&store.keychain_service, &store.keychain_account),
                Err(KeychainError::NotFound {
                    service: store.keychain_service.clone(),
                    account: store.keychain_account.clone(),
                }),
                "'no login' must mean the item is absent, not unreadable"
            );
        }
        Err(error) => {
            assert!(
                matches!(error, PerchError::ProbeRefused { .. }),
                "an unrecognized Claude Code must be a refusal naming the assumption: {error}"
            );
        }
    }
}

/// Takes the item back out of the real login keychain when the test ends,
/// however it ends.
///
/// The straight-line `keychain_delete` these tests used runs only if every
/// assertion before it passed, so the one run that fails is the one that leaves
/// something behind — in the user's own keychain, on the day they are already
/// looking at a failure.
#[cfg(target_os = "macos")]
struct Reaped<'a> {
    host: &'a RealHost,
    service: String,
    account: String,
}

#[cfg(target_os = "macos")]
impl Drop for Reaped<'_> {
    fn drop(&mut self) {
        let _ = self.host.keychain_delete(&self.service, &self.account);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn the_security_binary_round_trips_a_credential_sized_secret() {
    if skipping_keychain() {
        return;
    }
    let host = RealHost::new();
    let service = test_service("small");
    let account = std::env::var("USER").unwrap();
    // The shape and size of a real Credential, with a secret that is not one.
    let secret = format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{}","subscriptionType":"pro"}}}}"#,
        "a".repeat(300)
    );

    let reaped = Reaped {
        host: &host,
        service: service.clone(),
        account: account.clone(),
    };
    host.keychain_set(&service, &account, &secret)
        .expect("writing to the login keychain");
    let read_back = host.keychain_get(&service, &account);
    drop(reaped);

    assert_eq!(read_back.as_deref(), Ok(secret.as_str()));
    assert_eq!(
        host.keychain_get(&service, &account),
        Err(KeychainError::NotFound {
            service: service.clone(),
            account: account.clone(),
        }),
        "a deleted item reports as not found, which is exit 44"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn a_secret_past_the_stdin_buffer_limit_survives_the_argv_fallback() {
    if skipping_keychain() {
        return;
    }
    let host = RealHost::new();
    let service = test_service("large");
    let account = std::env::var("USER").unwrap();
    // Hex-encoding doubles the length, so this is comfortably past the 4096-byte
    // buffer that truncates mid-argument (Claude Code issue #30337).
    let secret = format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{}"}}}}"#,
        "b".repeat(4096)
    );

    let _reaped = Reaped {
        host: &host,
        service: service.clone(),
        account: account.clone(),
    };
    host.keychain_set(&service, &account, &secret)
        .expect("writing a large item to the login keychain");
    let read_back = host.keychain_get(&service, &account);

    assert_eq!(
        read_back.as_deref(),
        Ok(secret.as_str()),
        "a large Credential was truncated on the way in or out"
    );
}

#[test]
#[cfg(target_os = "macos")]
fn a_missing_item_is_not_found_rather_than_an_error() {
    if skipping_keychain() {
        return;
    }
    let host = RealHost::new();
    let service = test_service("absent");

    assert_eq!(
        host.keychain_get(&service, "nobody"),
        Err(KeychainError::NotFound {
            service,
            account: "nobody".to_string(),
        })
    );
}

/// Liveness is read from the marker files Claude Code writes for its running
/// sessions. If it stops writing them, or names them differently, every Profile
/// on the machine reads as idle and the refusal that protects a running client
/// stops firing.
#[test]
fn every_session_marker_claude_code_has_left_names_a_process() {
    let host = RealHost::new();
    let Ok(store) = probe::default_store(&host) else {
        return;
    };
    let sessions = probe::sessions_dir(&store.config_dir);

    let Ok(markers) = host.list_dir(&sessions) else {
        eprintln!("skipping: {} does not exist", sessions.display());
        return;
    };

    let named_after_a_pid = markers
        .iter()
        .filter_map(|marker| marker.file_name()?.to_str()?.strip_suffix(".json"))
        .filter(|name| name.parse::<u32>().is_ok())
        .count();
    if markers.is_empty() {
        eprintln!(
            "skipping: no Claude Code session has run against {}",
            store.config_dir.display()
        );
        return;
    }
    assert!(
        named_after_a_pid > 0,
        "{} holds {} marker(s), none of them named after a process: {markers:?}",
        sessions.display(),
        markers.len()
    );

    for pid in probe::live_clients(
        &host,
        &store.config_dir,
        &probe::Installed::unknown("whatever is installed here"),
    )
    .expect("every marker here can be corroborated or dismissed")
    {
        assert!(
            host.process_alive(pid),
            "a Live Profile is one with a process still behind it"
        );
    }
}

/// The marker's shape, asserted against a client that is actually running: it
/// names its process in its filename and in its body, and `startedAt` is when
/// the session began, in milliseconds since the epoch. If Claude Code stops
/// writing markers this way, every Profile on the machine reads as idle and
/// the refusal that protects a running client stops firing — this is the test
/// that should say so before a user does.
///
/// Here rather than beside the rest of ADR 0022's corroboration, because what
/// it reads is the developer's own `~/.claude/sessions`: whether it asserts or
/// skips is decided by their afternoon rather than by this repository.
#[test]
fn a_running_clients_marker_is_the_shape_perch_believes_in() {
    let host = RealHost::new();
    let Ok(store) = probe::default_store(&host) else {
        return;
    };
    let sessions = probe::sessions_dir(&store.config_dir);
    let Ok(markers) = host.list_dir(&sessions) else {
        eprintln!("skipping: {} does not exist", sessions.display());
        return;
    };

    let corroborated = probe::live_clients(
        &host,
        &store.config_dir,
        &probe::Installed::unknown("whatever is installed here"),
    )
    .expect("every marker here can be corroborated or dismissed");

    let mut live = 0;
    for marker in &markers {
        let Some(pid) = marker
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(process_began) = host.process_started_at(pid) else {
            continue; // A marker left behind by a client that died.
        };
        live += 1;

        let contents = host.read_file(marker).expect("a live marker is readable");
        let recorded: serde_json::Value =
            serde_json::from_str(&contents).expect("a live marker is JSON");
        assert_eq!(
            recorded.get("pid").and_then(serde_json::Value::as_u64),
            Some(u64::from(pid)),
            "a marker names its process in its body as well as its filename"
        );

        let session_began = recorded
            .get("startedAt")
            .and_then(serde_json::Value::as_i64)
            .expect("a marker says when its session began");
        assert!(
            (1_500_000_000_000..3_000_000_000_000).contains(&session_began),
            "startedAt is epoch milliseconds, not {session_began}"
        );

        // The belief the whole of ADR 0022 turns on, held against a real
        // client: a genuine session's process began no later than the marker
        // says the session did. If the two clocks drift apart, running clients
        // read as recycled PIDs and every Live Profile protection stops firing.
        assert!(
            process_began.timestamp_millis() <= session_began,
            "{}: the process began at {} but the session claims {session_began} — \
             a genuine client must corroborate its own marker",
            marker.display(),
            process_began.timestamp_millis()
        );
        assert!(
            corroborated.contains(&pid),
            "{} is corroborated, so live_clients must report it",
            marker.display()
        );
    }

    if live == 0 {
        eprintln!(
            "skipping the live half: no client is running against {}",
            store.config_dir.display()
        );
    }
}

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
/// would look like one honoring the directory.
///
/// On macOS this also asserts that the plaintext store is consulted when the
/// keychain holds nothing for that directory, which is the fallback half of the
/// same composite. If that stops being true, this is the test that should say
/// so.
///
/// Its own config directory, and yet gated: it runs the Claude Code the
/// developer installed, which is a program of theirs executing on their machine
/// rather than a filesystem call in `temp_dir`.
#[test]
fn claude_code_reads_the_credential_perch_would_write_for_a_config_directory() {
    let host = RealHost::new();
    let config_dir = std::env::temp_dir().join(format!("perch-config-{}", std::process::id()));
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

    // Read as loosely as the question allows: this is a test about a path, and
    // an unrecognizable reply is a reason to say nothing rather than to fail
    // over the shape of an answer Perch never parses in earnest.
    if said.contains("\"loggedIn\": true") {
        Some(true)
    } else if said.contains("\"loggedIn\": false") {
        Some(false)
    } else {
        eprintln!("`claude auth status` no longer answers in a shape this test reads: {said}");
        None
    }
}

/// The Claude Code Perch itself would run — the same PATH (and, on Windows,
/// PATHEXT) walk, so this test asks the binary the resolution actually finds.
/// Symlinks are followed where they exist; on Windows `canonicalize` yields a
/// `\\?\` path that `cmd.exe` cannot launch a `.cmd` shim from, and there is
/// no symlink to follow anyway.
fn installed_claude_code() -> Option<PathBuf> {
    let found = perch::probe::claude_bin(&RealHost::new()).ok()?;
    if cfg!(windows) {
        Some(found)
    } else {
        std::fs::canonicalize(found).ok()
    }
}
