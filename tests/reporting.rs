//! Behavior: what `perch status` shows, and what it refuses to do to get it.

mod common;

use chrono::{TimeZone, Utc};
use common::*;
use perch::error::EXIT_NOT_FOUND;
use perch::host::FakeHost;
use perch::registry::{Active, CURRENT_VERSION};

/// A machine where Perch has already adopted the login, with whatever
/// Utilization the test wants in the cache.
///
/// Its version is read from the build rather than typed: a document whose
/// number belies its shape is believed (ADR a-registry-comes-forward).
fn adopted_machine(utilization: &str) -> FakeHost {
    let registry = format!(
        r#"{{
  "version": {CURRENT_VERSION},
  "active": {{"settled": "someone@example.com"}},
  "accounts": [
    {{
      "identity": {{
        "email": "someone@example.com",
        "account_uuid": "account-uuid-1",
        "organization_name": "Acme"
      }},
      "plan": "pro"{utilization}
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

    let windows = document["active"]["utilization"]["windows"]
        .as_array()
        .unwrap();
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

/// A machine mid-Landing is indistinguishable from a healthy one
/// (ADR a-switch-is-written-down-first), so status says a Switch was in flight
/// and not recorded and exits 0: it reports what it found rather than judging
/// it, and a state the next Switch settles should not fail a shell prompt.
#[test]
fn status_says_a_switch_was_in_flight_and_not_recorded_and_still_exits_zero() {
    let host = machine_with_two_accounts();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);

    let (result, printed) = run_status(&host, false);

    result.expect("a Switch in flight is a state to report rather than a failure");
    assert!(
        printed.contains("A Switch was in flight and was not recorded"),
        "{printed}"
    );
    assert!(
        printed.contains(EMAIL) && printed.contains(SECOND_EMAIL),
        "it names both Accounts the live Credential could belong to: {printed}"
    );

    let (result, as_json) = run_status(&host, true);

    result.expect("and the same in --json");
    let document: serde_json::Value = serde_json::from_str(&as_json).expect("valid JSON");
    assert_eq!(document["landing"]["leaving"], EMAIL, "{as_json}");
    assert_eq!(document["landing"]["arriving"], SECOND_EMAIL, "{as_json}");
}

/// A Landing that left nobody behind is the one shape with no Account under it
/// to describe, and it is still a state to report rather than an absence to fail
/// on: the line and the field are said alone, and the command exits 0.
#[test]
fn a_switch_in_flight_from_nobody_is_still_reported_and_still_exits_zero() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);
    a_switch_died_mid_flight(&host, None, SECOND_EMAIL);

    let (result, printed) = run_status(&host, false);

    result.expect("a Switch in flight is a state to report rather than a failure");
    assert!(
        printed.contains("A Switch was in flight and was not recorded"),
        "{printed}"
    );
    assert!(printed.contains("Perch was on no Account"), "{printed}");

    let (result, as_json) = run_status(&host, true);

    result.expect("and the same in --json");
    let document: serde_json::Value = serde_json::from_str(&as_json).expect("valid JSON");
    assert!(document["landing"]["leaving"].is_null(), "{as_json}");
    assert_eq!(document["landing"]["arriving"], SECOND_EMAIL, "{as_json}");
    assert!(
        document["active"].is_null(),
        "there is no Account established to describe: {as_json}"
    );
}

/// The other way Perch is on nobody: no Landing to explain it, so it refuses —
/// and `perch list --json` on the same machine renders a whole document with a
/// null `active_account`, so two shapes for one absence is two shapes a prompt
/// script has to know about.
#[test]
fn a_registry_on_nobody_still_answers_json_with_the_shape_a_script_reads() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let (result, as_json) = run_status(&host, true);

    let refused = result.expect_err("there is no Account to report on");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND, "{refused}");
    let document: serde_json::Value =
        serde_json::from_str(&as_json).expect("a document, not an empty stdout");
    assert!(document["active"].is_null(), "{as_json}");
    assert!(document["landing"].is_null(), "{as_json}");
    assert!(
        document.get("refresh").is_some(),
        "every key the ordinary document has: {as_json}"
    );
}

/// The `*` a listing draws is on the Account Perch was on rather than one it can
/// establish is live, and it is said at every breadth the listing has
/// (ADR the-listing-owns-the-set): narrowing to a Scope narrows which Accounts
/// are shown and not which facts are true of them.
#[test]
fn a_switch_in_flight_is_said_by_the_listing_at_every_breadth() {
    let host = machine_with_two_accounts();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);

    for (what, printed, as_json) in [
        ("list", run_list(&host, false), run_list(&host, true)),
        (
            "list ungrouped",
            run_list_in(&host, "ungrouped", false),
            run_list_in(&host, "ungrouped", true),
        ),
    ] {
        let (result, printed) = printed;
        result.unwrap_or_else(|error| panic!("{what}: {error}"));
        assert!(
            printed.contains("A Switch was in flight and was not recorded"),
            "{what}: {printed}"
        );

        let (result, as_json) = as_json;
        result.unwrap_or_else(|error| panic!("{what} --json: {error}"));
        let document: serde_json::Value = serde_json::from_str(&as_json).expect("valid JSON");
        assert_eq!(document["landing"]["leaving"], EMAIL, "{what}: {as_json}");
        assert_eq!(
            document["landing"]["arriving"], SECOND_EMAIL,
            "{what}: {as_json}"
        );
    }
}

/// And on the ordinary machine the field is there and empty, so a script may
/// read it rather than test for its absence.
#[test]
fn status_json_says_no_switch_is_in_flight_rather_than_leaving_the_field_out() {
    let host = adopted_machine("");

    let (result, printed) = run_status(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert!(document["landing"].is_null(), "{printed}");
}

/// One Account is one shape, whichever command is describing it. What each
/// *document* answers still differs — one Account under `active`, a set under
/// `sections` — and that is the difference worth having.
#[test]
fn the_account_status_describes_has_the_same_keys_the_listing_gives_one() {
    let host = adopted_machine(OBSERVED_THREE_MINUTES_AGO);

    let (_, from_status) = run_status(&host, true);
    let (_, from_list) = run_list(&host, true);

    let status: serde_json::Value = serde_json::from_str(&from_status).expect("valid JSON");
    let list: serde_json::Value = serde_json::from_str(&from_list).expect("valid JSON");
    let active = &status["active"];
    let listed = &list["sections"][0]["accounts"][0];

    assert_eq!(active["email"], EMAIL, "the same Account: {status}");
    let keys = |value: &serde_json::Value| -> Vec<String> {
        let mut named: Vec<String> = value
            .as_object()
            .expect("an Account is an object")
            .keys()
            .cloned()
            .collect();
        named.sort();
        named
    };
    assert_eq!(keys(active), keys(listed), "{status}\n{list}");
    assert_eq!(active, listed, "and the same answers, key for key");
    assert_eq!(
        active["account_uuid"], "account-uuid-1",
        "with what only `status` used to carry: {active}"
    );
    assert_eq!(
        active["disabled"], false,
        "and what only the listing used to: {active}"
    );
}

#[test]
fn json_says_a_figure_has_never_been_observed_rather_than_reporting_zero() {
    let host = adopted_machine("");

    let (result, printed) = run_status(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).unwrap();
    let utilization = &document["active"]["utilization"];
    assert_eq!(utilization["never_observed"], true);
    assert!(utilization["observed_at"].is_null());
    assert_eq!(utilization["windows"].as_array().unwrap().len(), 0);
}

/// The Utilization is said once, under the Account it is of. This document
/// answers about exactly one Account and cannot be anything else, so there is no
/// wrong shape for a top-level copy to insure against — and the same figure
/// written twice is two places for one answer to go stale from.
#[test]
fn the_document_carries_the_utilization_under_the_account_and_nowhere_else() {
    let host = adopted_machine(OBSERVED_THREE_MINUTES_AGO);

    let (result, printed) = run_status(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert!(
        document["active"]["utilization"]["windows"][0]["used_percent"].is_number(),
        "the figures are under the Account they describe: {printed}"
    );
    assert!(
        document
            .as_object()
            .expect("a document is an object")
            .get("utilization")
            .is_none(),
        "and the top-level duplicate is gone: {printed}"
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

/// The version has to be read *before* the document is, or the guard only fires
/// for registries a newer Perch happened to keep readable by this one — and a
/// newer Perch is exactly the thing that writes a value this build has no
/// variant for, which deserializing first reports as invalid JSON about a file
/// that is perfectly valid JSON.
#[test]
fn a_registry_from_a_newer_perch_says_so_even_when_it_spells_things_this_build_cannot_read() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        r#"{"version": 99, "active": "someone@example.com", "accounts": [],
            "groups": {"work": {"strategy": "least-recently-used"}}}"#,
    );

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("a registry from the future cannot be trusted");
    assert!(
        error.to_string().contains("newer Perch"),
        "not a complaint about the JSON, which is valid: {error}"
    );
    assert!(error.to_string().contains("Upgrade Perch"), "{error}");
}

/// With Accounts held but Perch on nobody, a login is not the answer: Perch has
/// Credentials and has been left on nobody, which is what `perch switch` is for.
#[test]
fn status_with_no_active_account_names_the_remedy_that_applies() {
    let host = machine_with_two_accounts().with_answers(&["y"]);
    disable_account(&host, SECOND_EMAIL)
        .0
        .expect("taken out of Cycling");
    run_remove(&host, EMAIL)
        .0
        .expect("the active one is given up");
    assert_eq!(*registry_of(&host).active(), Active::Nobody);

    let (result, _) = run_status(&host, false);

    let error = result.expect_err("there is no Account to report on");
    let message = error.to_string();
    assert!(message.contains("perch switch"), "{message}");
    assert!(
        !message.contains("log in"),
        "Perch holds a Credential for the Account it would land on:\n{message}"
    );
}

/// `perch status` is advertised for a shell prompt, where two of them drawing at
/// once is ordinary rather than a race: taking the registry lock to read would
/// have one wait out the other and then fail. Nothing is written without
/// `--refresh`, so nothing needs shutting out.
#[test]
fn status_reads_alongside_another_perch_rather_than_waiting_on_it() {
    let host = machine_with_two_accounts();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, printed) = run_status(&host, false);

    result.expect("a read does not wait on a writer");
    assert!(printed.contains(EMAIL), "{printed}");
    drop(held);
}

/// The other half of the same rule: `--refresh` writes what it fetched, so it
/// does take the lock, and two of them do exclude each other.
#[test]
fn status_that_refreshes_waits_for_the_other_perch_because_it_writes() {
    let host = machine_with_two_accounts();
    let _held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (result, _) = run_status_refresh(&host, false);

    let refused = result.expect_err("a writer waits on a writer");
    assert!(
        refused.to_string().contains("the Perch registry lock"),
        "{refused}"
    );
}
