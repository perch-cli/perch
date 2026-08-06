//! Behaviour: what `perch status` shows, and what it refuses to do to get it.

mod common;

use chrono::{TimeZone, Utc};
use common::*;
use perch::host::FakeHost;

/// A machine where Perch has already adopted the login, with whatever
/// Utilization the test wants in the cache.
fn adopted_machine(utilization: &str) -> FakeHost {
    let registry = format!(
        r#"{{
  "version": 1,
  "active": "someone@example.com",
  "accounts": [
    {{
      "identity": {{
        "email": "someone@example.com",
        "account_uuid": "account-uuid-1",
        "organization_name": "Acme"
      }},
      "plan": "pro",
      "profile": {{
        "dir": "/Users/someone/.perch/profiles/someone-example-com",
        "keychain_service": "Claude Code-credentials-abcd1234",
        "keychain_account": "someone"
      }},
      "enabled": true,
      "quarantined": false{utilization}
    }}
  ],
  "aliases": {{}}
}}"#
    );
    logged_in_machine()
        .with_file(REGISTRY_PATH, &registry)
        .with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap())
}

const OBSERVED_THREE_MINUTES_AGO: &str = r#",
      "utilization": {
        "observed_at": "2026-08-04T11:57:00Z",
        "windows": [
          {"window": "5-hour", "used_percent": 42.0, "resets_at": "2026-08-04T14:30:00Z"},
          {"window": "7-day", "used_percent": 18.0}
        ]
      }"#;

#[test]
fn status_shows_the_active_accounts_identity() {
    let host = adopted_machine("");

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(printed.contains("Acme"), "{printed}");
    assert!(printed.contains("pro"), "{printed}");
}

#[test]
fn status_shows_each_utilization_figure_with_its_age() {
    let host = adopted_machine(OBSERVED_THREE_MINUTES_AGO);

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(printed.contains("5-hour"), "{printed}");
    assert!(printed.contains("42%"), "{printed}");
    assert!(printed.contains("7-day"), "{printed}");
    assert!(printed.contains("18%"), "{printed}");
    assert_eq!(
        printed.matches("as of 3m ago").count(),
        2,
        "every figure carries its age:\n{printed}"
    );
}

#[test]
fn a_figure_that_has_never_been_observed_says_so_rather_than_fetching() {
    let host = adopted_machine("");

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(printed.contains("never observed"), "{printed}");
    assert!(host.http_calls().is_empty(), "status must not fetch");
}

#[test]
fn status_never_touches_the_network_even_with_a_stale_cache() {
    let host = adopted_machine(OBSERVED_THREE_MINUTES_AGO)
        .with_now(Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap());

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(
        printed.contains("7d ago"),
        "a week-old figure reads as old:\n{printed}"
    );
    assert!(host.http_calls().is_empty());
}

#[test]
fn json_carries_an_observation_time_on_every_utilization_figure() {
    let host = adopted_machine(OBSERVED_THREE_MINUTES_AGO);

    let (result, printed) = run_status(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["active"]["email"], EMAIL);
    assert_eq!(document["active"]["organization"], "Acme");
    assert_eq!(document["active"]["plan"], "pro");

    let windows = document["utilization"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    for window in windows {
        assert_eq!(
            window["observed_at"]
                .as_str()
                .map(|at| at.starts_with("2026-08-04T11:57:00")),
            Some(true),
            "every figure carries its observation time: {window}"
        );
        assert!(window["window"].is_string());
        assert!(window["used_percent"].is_number());
    }
    assert_eq!(windows[0]["observed_seconds_ago"], 180);
}

#[test]
fn json_says_a_figure_has_never_been_observed_rather_than_reporting_zero() {
    let host = adopted_machine("");

    let (result, printed) = run_status(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).unwrap();
    assert_eq!(document["utilization"]["never_observed"], true);
    assert!(document["utilization"]["observed_at"].is_null());
    assert_eq!(
        document["utilization"]["windows"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn a_registry_from_a_newer_perch_is_refused_rather_than_misread() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        r#"{"version": 99, "active": "someone@example.com", "accounts": []}"#,
    );

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("a registry from the future cannot be trusted");
    assert!(error.to_string().contains("newer Perch"), "{error}");
}

/// With Accounts held but Perch on nobody, a login is not the answer: Perch
/// has Credentials and has simply been left on nobody, which is what `perch
/// switch` is for — and what `perch remove` itself recommends when it leaves
/// the machine in this state.
#[test]
fn status_with_no_active_account_names_the_remedy_that_applies() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    disable_account(&host, SECOND_EMAIL).0.expect("reserved");
    run_remove(&host, EMAIL)
        .0
        .expect("the active one is given up");
    assert_eq!(registry_of(&host).active, None);

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("there is no Account to report on");
    let message = error.to_string();
    assert!(message.contains("perch switch"), "{message}");
    assert!(
        !message.contains("log in"),
        "Perch holds a Credential for the Account it would land on:\n{message}"
    );
}
