//! Behaviour: what `perch list` answers when the question is "what do I have",
//! and what `perch status --group` shows of the Group around the active
//! Account. Both render from cache and neither touches the network (ADR 0015).

mod common;

use chrono::{DateTime, TimeZone, Utc};
use common::*;
use perch::host::FakeHost;
use perch::probe::Identity;
use perch::registry::{
    Account, CachedUtilization, GroupConfig, Quarantine, Registry, WindowUtilization,
};

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 4, hour, minute, 0).unwrap()
}

fn account(email: &str, organization: &str) -> Account {
    Account {
        identity: Identity {
            email: email.to_string(),
            account_uuid: Some(format!("uuid-{email}")),
            organization_name: Some(organization.to_string()),
            organization_uuid: None,
        },
        plan: Some("pro".to_string()),
        enabled: true,
        quarantine: None,
        group: None,
        utilization: None,
    }
}

fn observed(observed_at: DateTime<Utc>, windows: &[(&str, f64)]) -> CachedUtilization {
    CachedUtilization {
        observed_at,
        windows: windows
            .iter()
            .map(|(window, used_percent)| WindowUtilization {
                window: window.to_string(),
                used_percent: *used_percent,
                resets_at: None,
            })
            .collect(),
    }
}

/// Three Accounts, deliberately unalike: the active one in a Group with a
/// second that is Quarantined, and a third that is in no Group and disabled.
fn machine_holding_three_accounts() -> FakeHost {
    let mut registry = Registry::default();

    let mut active = account(EMAIL, "Acme");
    active.group = Some("work".to_string());
    active.utilization = Some(observed(at(11, 57), &[("5-hour", 42.0), ("7-day", 18.0)]));
    registry.upsert(active);

    let mut overflow = account(SECOND_EMAIL, "Overflow Ltd");
    overflow.group = Some("work".to_string());
    overflow.quarantine = Some(Quarantine::RenewalRejected);
    registry.upsert(overflow);

    let mut spare = account(THIRD_EMAIL, "Spare Ltd");
    spare.enabled = false;
    spare.utilization = Some(observed(at(10, 0), &[("5-hour", 91.0)]));
    registry.upsert(spare);

    registry.active = Some(EMAIL.to_string());
    registry
        .groups
        .insert("work".to_string(), GroupConfig::default());
    registry.set_alias("overflow", SECOND_EMAIL);

    machine_holding(&registry)
}

fn machine_holding(registry: &Registry) -> FakeHost {
    let host = logged_in_machine().with_now(at(12, 0));
    common::save_registry(&host, registry);
    host
}

#[test]
fn list_shows_every_account_with_its_alias_group_and_state() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert!(printed.contains(email), "{email} is missing:\n{printed}");
    }
    assert!(
        printed.contains("overflow"),
        "the Alias is shown:\n{printed}"
    );
    assert!(printed.contains("work"), "the Group is shown:\n{printed}");
    assert!(
        printed.contains("enabled, quarantined"),
        "a broken Account stays listed, says so, and still says whether it is \
         a Cycle candidate — enabling it would not repair it:\n{printed}"
    );
    assert!(
        printed.contains("disabled"),
        "an Account out of the pool says so:\n{printed}"
    );
}

#[test]
fn list_says_which_account_is_active() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    let active_line = printed
        .lines()
        .find(|line| line.contains(EMAIL))
        .expect("the active Account is listed");
    assert!(
        active_line.starts_with('*'),
        "the active Account is marked:\n{printed}"
    );
    assert!(
        printed.contains("* is the active Account"),
        "and the mark is explained:\n{printed}"
    );
}

#[test]
fn list_shows_each_utilization_figure_with_its_age() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(printed.contains("5-hour"), "{printed}");
    assert!(printed.contains("42%"), "{printed}");
    assert!(printed.contains("7-day"), "{printed}");
    assert!(printed.contains("18%"), "{printed}");
    assert_eq!(
        printed.matches("as of 3m ago").count(),
        2,
        "both of the active Account's figures carry their age:\n{printed}"
    );
    assert!(
        printed.contains("91%") && printed.contains("as of 2h ago"),
        "an older figure reads as older:\n{printed}"
    );
}

#[test]
fn an_account_with_no_observed_utilization_is_not_shown_as_zero() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    let line = printed
        .lines()
        .find(|line| line.contains(SECOND_EMAIL))
        .expect("the Account with no figure is still listed");
    assert!(line.contains("never observed"), "{printed}");
    assert!(!line.contains('%'), "no number is invented:\n{printed}");
}

#[test]
fn list_never_touches_the_network() {
    let host = machine_holding_three_accounts().with_now(at(23, 0));

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(printed.contains("11h ago"), "{printed}");
    assert!(host.http_calls().is_empty(), "list must not fetch");
}

#[test]
fn list_json_carries_an_observation_time_on_every_figure() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "all");
    assert_eq!(document["active_account"], EMAIL);

    let accounts = document["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 3);

    let active = &accounts[0];
    assert_eq!(active["email"], EMAIL);
    assert_eq!(active["active"], true);
    assert_eq!(active["group"], "work");
    assert_eq!(active["enabled"], true);
    assert!(
        active["quarantined"].is_null(),
        "an Account that works says nothing about a Quarantine, which is what a \
         script reading it as false reads"
    );
    assert!(active["alias"].is_null());

    let windows = active["utilization"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    for window in windows {
        assert_eq!(
            window["observed_at"]
                .as_str()
                .map(|at| at.starts_with("2026-08-04T11:57:00")),
            Some(true),
            "every figure carries its observation time: {window}"
        );
        assert_eq!(window["observed_seconds_ago"], 180);
    }

    let overflow = &accounts[1];
    assert_eq!(overflow["alias"], "overflow");
    assert_eq!(
        overflow["quarantined"]["reason"], "renewal-rejected",
        "and a broken one says what broke it, so a script can tell a Credential \
         Anthropic turned down from one Perch never had: {overflow}"
    );
    assert!(overflow["quarantined"]["detail"].is_string());
    assert_eq!(overflow["active"], false);
    assert_eq!(
        overflow["utilization"]["never_observed"], true,
        "a figure nobody has observed is said to be missing, not reported as zero"
    );
    assert!(overflow["utilization"]["observed_at"].is_null());

    let spare = &accounts[2];
    assert_eq!(spare["enabled"], false);
    assert!(spare["group"].is_null());
}

#[test]
fn status_group_shows_every_account_in_the_current_accounts_group() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_status_group(&host, false);

    result.unwrap();
    assert!(printed.contains("work"), "the Group is named:\n{printed}");
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(printed.contains(SECOND_EMAIL), "{printed}");
    assert!(
        !printed.contains(THIRD_EMAIL),
        "an Account in another Group is not somewhere you would land:\n{printed}"
    );
    assert!(printed.contains("as of 3m ago"), "{printed}");
    assert!(host.http_calls().is_empty(), "status must not fetch");
}

#[test]
fn status_group_from_an_ungrouped_account_shows_every_ungrouped_account() {
    let mut registry = Registry::default();
    let mut first = account(EMAIL, "Acme");
    first.utilization = Some(observed(at(11, 57), &[("5-hour", 42.0)]));
    registry.upsert(first);
    let mut second = account(SECOND_EMAIL, "Overflow Ltd");
    second.group = Some("work".to_string());
    registry.upsert(second);
    registry.upsert(account(THIRD_EMAIL, "Spare Ltd"));
    registry
        .groups
        .insert("work".to_string(), GroupConfig::default());
    registry.active = Some(EMAIL.to_string());
    let host = machine_holding(&registry);

    let (result, printed) = run_status_group(&host, false);

    result.unwrap();
    assert!(printed.contains("In no Group"), "{printed}");
    assert!(
        printed.contains(EMAIL) && printed.contains(THIRD_EMAIL),
        "{printed}"
    );
    assert!(
        !printed.contains(SECOND_EMAIL),
        "a Group the active Account is not in is not shown:\n{printed}"
    );
    assert!(
        printed.contains("only moves between these when you say it may"),
        "being ungrouped is not a Group, and Cycling says so (ADR 0017):\n{printed}"
    );
}

#[test]
fn status_group_json_says_which_group_it_narrowed_to() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_status_group(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "group");
    assert_eq!(document["scope"]["name"], "work");
    let accounts = document["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["email"], EMAIL);
    assert_eq!(accounts[1]["email"], SECOND_EMAIL);
}

#[test]
fn status_group_json_from_an_ungrouped_account_says_it_narrowed_to_no_group() {
    let mut registry = Registry::default();
    registry.upsert(account(EMAIL, "Acme"));
    registry.active = Some(EMAIL.to_string());
    let host = machine_holding(&registry);

    let (result, printed) = run_status_group(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "ungrouped");
    assert!(document["scope"]["name"].is_null());
}

#[test]
fn status_without_group_still_shows_only_the_active_account() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(
        !printed.contains(SECOND_EMAIL),
        "plain status is about the Account you are on:\n{printed}"
    );
}

#[test]
fn list_on_a_machine_perch_has_never_run_on_adopts_first() {
    let host = logged_in_machine();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(
        printed.contains("Adopted the Claude Code login"),
        "{printed}"
    );
    assert!(printed.contains(EMAIL), "{printed}");
}
