//! Contract tests: the probe's beliefs, asserted against the Claude Code that
//! is actually installed and against the real keychain (ADR 0007).
//!
//! These run in CI to find drift before users do. They are macOS-only, because
//! everything they assert is macOS-specific, and they never print a Credential.
//!
//! On a machine with no Claude Code and no login — a CI runner, for instance —
//! each test asserts the outcome that situation should produce, rather than
//! quietly passing.

#![cfg(target_os = "macos")]

use perch::error::PerchError;
use perch::host::{Host, HostError, RealHost};
use perch::keychain::KeychainError;
use perch::probe::{self, Verdict};

/// Set to any value to skip the tests that touch the real keychain, for
/// environments where the login keychain cannot be unlocked. CI does not set it.
const SKIP_KEYCHAIN: &str = "PERCH_SKIP_KEYCHAIN_CONTRACT";

fn skipping_keychain() -> bool {
    if std::env::var_os(SKIP_KEYCHAIN).is_some() {
        eprintln!("skipping: {SKIP_KEYCHAIN} is set");
        true
    } else {
        false
    }
}

fn test_service(name: &str) -> String {
    format!("Perch contract test-{name}-{}", std::process::id())
}

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

#[test]
fn the_default_store_is_where_perch_believes_it_is() {
    let host = RealHost::new();
    let store = probe::default_store(&host).expect("USER is set");

    if std::env::var_os("CLAUDE_CONFIG_DIR").is_none() {
        assert_eq!(
            store.keychain_service,
            probe::DEFAULT_SERVICE,
            "the default config directory uses the bare service name"
        );
        assert_eq!(store.config_dir, host.home_dir().join(".claude"));
        assert_eq!(store.identity_file, host.home_dir().join(".claude.json"));
    } else {
        assert!(
            store
                .keychain_service
                .starts_with(&format!("{}-", probe::DEFAULT_SERVICE))
        );
    }

    assert_eq!(store.keychain_account, std::env::var("USER").unwrap());
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
fn the_installed_claude_code_stores_what_perch_expects_to_find() {
    if skipping_keychain() {
        return;
    }
    let host = RealHost::new();

    match probe::probe(&host) {
        Ok(Verdict::Recognised(findings)) => {
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
                "an unrecognised Claude Code must be a refusal naming the assumption: {error}"
            );
        }
    }
}

#[test]
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

    host.keychain_set(&service, &account, &secret)
        .expect("writing to the login keychain");
    let read_back = host.keychain_get(&service, &account);
    let _ = host.keychain_delete(&service, &account);

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

    host.keychain_set(&service, &account, &secret)
        .expect("writing a large item to the login keychain");
    let read_back = host.keychain_get(&service, &account);
    let _ = host.keychain_delete(&service, &account);

    assert_eq!(
        read_back.as_deref(),
        Ok(secret.as_str()),
        "a large Credential was truncated on the way in or out"
    );
}

/// The whole lock protocol rests on this one property of the filesystem: two
/// processes calling `mkdir` on the same path cannot both be told they made it.
/// Perch's locks are directories rather than files for no other reason.
#[test]
fn taking_a_lock_is_a_directory_only_one_caller_can_make() {
    let host = RealHost::new();
    let lock = std::env::temp_dir().join(format!("perch-contract-lock-{}", std::process::id()));
    let _ = host.remove_dir_all(&lock);

    host.create_dir_exclusive(&lock)
        .expect("the first caller takes it");
    let contended = host.create_dir_exclusive(&lock);
    let held_since = host.modified_at(&lock);
    host.remove_dir_all(&lock).expect("and gives it back");

    assert!(
        matches!(contended, Err(HostError::AlreadyExists { .. })),
        "a second caller must be refused, not given the same lock: {contended:?}"
    );
    assert!(
        held_since.is_ok_and(|at| (host.now() - at).num_seconds().abs() < 60),
        "a lock's age is how staleness is judged, so its directory has to carry one"
    );
    assert!(!host.path_exists(&lock));
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

    for pid in probe::live_clients(&host, &store.config_dir) {
        assert!(
            host.process_alive(pid),
            "a Live Profile is one with a process still behind it"
        );
    }
}

#[test]
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
