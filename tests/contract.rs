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
use perch::host::{Host, RealHost};
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
        assert!(store
            .keychain_service
            .starts_with(&format!("{}-", probe::DEFAULT_SERVICE)));
    }

    assert_eq!(store.keychain_account, std::env::var("USER").unwrap());
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
