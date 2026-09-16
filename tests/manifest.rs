//! Configuration persistence and provider-local runtime (ADR a-fresh-provider-layout).

use perch::config::{Scope, Strategy};
use perch::domain::Identity;
use perch::host::{FakeHost, Files, Refusing};
use perch::providers::provider::Id;
use perch::registry::{self, Account, Registry};
use serde_json::{Value, json};

const CONFIG: &str = "/perch/config.json";
const CLAUDE_STATE: &str = "/perch/providers/claude/state.json";

fn machine() -> FakeHost {
    FakeHost::new().with_env("PERCH_HOME", "/perch")
}
fn save(host: &FakeHost, registry: &mut Registry) {
    let mut held = perch::holdings::lock(host).unwrap();
    registry::save(host, &mut held, registry).unwrap();
}
fn account(email: &str) -> Account {
    Account {
        storage_key: None,
        provider: Id::Claude,
        provider_identity: None,
        identity: Identity {
            email: email.into(),
            account_uuid: None,
            organization_uuid: None,
            organization_name: None,
        },
        plan: None,
        disabled: false,
        quarantine: None,
        group: None,
        utilization: None,
    }
}
fn document() -> Value {
    json!({"format":"perch-config", "version":registry::CURRENT_VERSION,
        "accounts":{"one@example.com":{"provider":"claude","identity":{"email":"one@example.com"},"enabled":true,"position":0}}})
}

#[test]
fn account_order_survives_the_directory_map() {
    let host = machine();
    let mut registry = Registry::default();
    for email in ["z@example.com", "a@example.com", "m@example.com"] {
        registry.upsert(account(email));
    }
    save(&host, &mut registry);
    let restored = registry::load(&host).unwrap().unwrap();
    assert_eq!(
        restored
            .accounts
            .iter()
            .map(Account::key)
            .collect::<Vec<_>>(),
        ["z@example.com", "a@example.com", "m@example.com"]
    );
}

#[test]
fn a_group_rename_preserves_each_providers_cooldown_without_rewriting_runtime() {
    let host = machine();
    let mut registry = Registry::default();
    registry.declare_group("work").unwrap();
    let at = "2026-09-01T12:00:00Z".parse().unwrap();
    for provider in [Id::Claude, Id::Codex] {
        registry.select_provider(provider);
        registry.record_switch("work", at);
    }
    save(&host, &mut registry);
    let runtime = host.read_file(std::path::Path::new(CLAUDE_STATE)).unwrap();
    let host = host.with_a_path_refusing(CLAUDE_STATE, Refusing::Write, "runtime must not change");
    registry.rename_group("work", "office").unwrap();
    save(&host, &mut registry);
    assert_eq!(
        host.read_file(std::path::Path::new(CLAUDE_STATE)).unwrap(),
        runtime
    );
    let mut restored = registry::load(&host).unwrap().unwrap();
    for provider in [Id::Claude, Id::Codex] {
        restored.select_provider(provider);
        assert_eq!(restored.checked("office").unwrap().switched_at, at);
    }
}

#[test]
fn watcher_grants_do_not_follow_inherited_policy_or_other_providers() {
    let mut registry = Registry::default();
    registry.scope_defaults.cycle.strategy = Some(Strategy::SoonestReset);
    registry.scope_defaults.watcher.threshold_percent = Some(75);
    registry.declare_group("work").unwrap();
    registry.declare_group("personal").unwrap();
    let scope = Scope::Group("work".into());
    let work = registry.scope_settings_mut(&scope).unwrap();
    work.watcher.threshold_percent = Some(65);
    let claude = work.providers.entry(Id::Claude).or_default();
    claude.watcher.enabled = true;
    claude.watcher.threshold_percent = Some(55);
    let resolved = registry.resolved_policy(&scope, Id::Claude);
    assert_eq!(resolved.settings.strategy, Strategy::SoonestReset);
    assert_eq!(resolved.settings.watcher_threshold_percent, 55);
    assert_eq!(
        resolved.sources["watcher-threshold-percent"],
        "scope provider"
    );
    assert!(resolved.settings.watcher_may_act);
    let codex = registry.resolved_policy(&scope, Id::Codex);
    assert_eq!(codex.settings.watcher_threshold_percent, 65);
    assert!(!codex.settings.watcher_may_act);
    let personal = registry.resolved_policy(&Scope::Group("personal".into()), Id::Claude);
    assert_eq!(personal.settings.watcher_threshold_percent, 75);
    assert!(!personal.settings.watcher_may_act);
}

#[test]
fn runtime_cannot_select_an_account_from_another_provider() {
    let host = machine().with_file(CONFIG, &document().to_string()).with_file(
        "/perch/providers/codex/state.json",
        &json!({"version":registry::CURRENT_VERSION,"active":{"settled":"one@example.com"},"checks":{},"accounts":{}}).to_string(),
    );
    let error = registry::load(&host).unwrap_err().to_string();
    assert!(error.contains("another provider"), "{error}");
}

#[test]
fn malformed_configuration_is_refused_at_the_manifest() {
    let cases = [
        ("unknown key", "/typo", json!(true)),
        (
            "invalid threshold",
            "/scope_defaults",
            json!({"watcher":{"threshold_percent":101}}),
        ),
        (
            "invalid margin",
            "/scope_defaults",
            json!({"watcher":{"margin_percent":0}}),
        ),
        ("invalid group", "/groups", json!({"my work":{"id":"g1"}})),
        (
            "case collision",
            "/groups",
            json!({"work":{"id":"g1"},"Work":{"id":"g2"}}),
        ),
        (
            "namespace collision",
            "/groups",
            json!({"one@example.com":{"id":"g1"}}),
        ),
    ];
    for (case, key, value) in cases {
        let mut manifest = document();
        manifest[key.trim_start_matches('/')] = value;
        let host = machine().with_file(CONFIG, &manifest.to_string());
        assert!(registry::load(&host).is_err(), "{case}");
    }
}

#[test]
fn unsupported_configurations_require_a_fresh_installation() {
    for version in [
        0,
        registry::CURRENT_VERSION - 1,
        registry::CURRENT_VERSION + 1,
    ] {
        let mut manifest = document();
        manifest["version"] = version.into();
        let host = machine().with_file(CONFIG, &manifest.to_string());
        let error = registry::load(&host).unwrap_err().to_string();
        assert!(
            error.contains(if version > registry::CURRENT_VERSION {
                "Upgrade Perch"
            } else {
                "fresh installation"
            }),
            "{error}"
        );
    }
}

#[test]
fn aliases_are_unique_even_when_two_accounts_use_the_same_exact_spelling() {
    for alias in ["shared", "SHARED"] {
        let mut manifest = document();
        manifest["accounts"]["one@example.com"]["alias"] = "shared".into();
        let mut second = manifest["accounts"]["one@example.com"].clone();
        second["identity"]["email"] = "two@example.com".into();
        second["position"] = 1.into();
        second["alias"] = alias.into();
        manifest["accounts"]["two@example.com"] = second;
        let host = machine().with_file(CONFIG, &manifest.to_string());
        assert!(registry::load(&host).is_err(), "{alias}");
    }
}

#[test]
fn invalid_quota_values_are_refused_in_provider_runtime() {
    for percentage in [-50, 150] {
        let runtime = json!({"version":registry::CURRENT_VERSION,"active":registry::Active::Nobody,"checks":{},
            "accounts":{"one@example.com":{"utilization":{"observed_at":"2026-09-01T12:00:00Z","windows":[{"window":"quota","used_percent":percentage}]}}}});
        let host = machine()
            .with_file(CONFIG, &document().to_string())
            .with_file(CLAUDE_STATE, &runtime.to_string());
        let error = registry::load(&host).unwrap_err().to_string();
        assert!(
            error.contains("quota") && error.contains("one@example.com"),
            "{error}"
        );
    }
}

#[test]
fn unknown_nested_configuration_and_runtime_keys_are_refused() {
    for (key, value, unknown) in [
        (
            "scope_defaults",
            json!({"watcher":{"unknown_threshold":50}}),
            "unknown_threshold",
        ),
        (
            "groups",
            json!({"work":{"unknown_watcher":{}}}),
            "unknown_watcher",
        ),
        (
            "ungrouped",
            json!({"unknown_interchangeability":true}),
            "unknown_interchangeability",
        ),
        (
            "providers",
            json!({"claude":{"unknown_enabled":true}}),
            "unknown_enabled",
        ),
    ] {
        let mut manifest = document();
        manifest[key] = value;
        let host = machine().with_file(CONFIG, &manifest.to_string());
        let error = registry::load(&host).unwrap_err().to_string();
        assert!(error.contains(unknown) && error.contains(CONFIG), "{error}");
    }
    let runtime = json!({"version":registry::CURRENT_VERSION,"active":{"landing":{"leavign":"one@example.com","arriving":"one@example.com"}},"checks":{},"accounts":{}});
    let host = machine()
        .with_file(CONFIG, &document().to_string())
        .with_file(CLAUDE_STATE, &runtime.to_string());
    let error = registry::load(&host).unwrap_err().to_string();
    assert!(
        error.contains("leavign") && error.contains(CLAUDE_STATE),
        "{error}"
    );
}

#[test]
fn invalid_account_directory_values_are_refused() {
    for (field, value) in [
        ("alias", json!("my alias")),
        ("group", json!("none")),
        ("enabled", json!("yes")),
    ] {
        let mut manifest = document();
        manifest["accounts"]["one@example.com"][field] = value;
        let host = machine().with_file(CONFIG, &manifest.to_string());
        assert!(registry::load(&host).is_err(), "{field}");
    }
}

#[test]
fn claimed_groups_are_normalized_without_merging_distinct_groups() {
    let mut manifest = document();
    manifest["groups"] = json!({"Work":{"id":"g1"}});
    manifest["next_group_id"] = 1.into();
    manifest["accounts"]["one@example.com"]["group"] = "work".into();
    let host = machine().with_file(CONFIG, &manifest.to_string());
    let registry = registry::load(&host).unwrap().unwrap();
    assert_eq!(registry.groups.len(), 1);
    assert_eq!(registry.accounts[0].group.as_deref(), Some("Work"));
}

#[test]
fn removing_a_group_and_declaring_another_cannot_reuse_its_identity() {
    let host = machine();
    let mut registry = Registry::default();
    registry.declare_group("work").unwrap();
    let previous = registry.scope_id("work");
    registry.forget_group("work");
    save(&host, &mut registry);
    let mut restored = registry::load(&host).unwrap().unwrap();
    restored.declare_group("work").unwrap();
    assert_ne!(previous, restored.scope_id("work"));
}

#[test]
fn duplicate_manifest_names_are_refused_before_an_account_can_be_dropped() {
    let one = serde_json::to_string(&document()["accounts"]["one@example.com"]).unwrap();
    let content = format!(
        r#"{{"format":"perch-config","version":{},"accounts":{{"one@example.com":{one},"one@example.com":{one}}}}}"#,
        registry::CURRENT_VERSION
    );
    let host = machine().with_file(CONFIG, &content);
    assert!(registry::load(&host).is_err());
    assert_eq!(
        host.file(std::path::Path::new(CONFIG)).as_deref(),
        Some(content.as_str())
    );
}

#[test]
fn a_hand_edited_group_counter_cannot_reuse_an_existing_identity() {
    let host = machine();
    let mut value = document();
    value["groups"] = json!({"work":{"id":"g1"}});
    value["next_group_id"] = json!(0);
    host.set_file(CONFIG, &value.to_string());
    let mut registry = registry::load(&host).unwrap().unwrap();
    registry.declare_group("personal").unwrap();
    assert_ne!(registry.scope_id("work"), registry.scope_id("personal"));
    save(&host, &mut registry);
    registry::load(&host).unwrap();
}

#[test]
fn the_documented_configuration_example_loads_as_a_mixed_provider_group() {
    let host = machine().with_file(
        CONFIG,
        include_str!("../docs/design/provider-config.example.json"),
    );
    let registry = registry::load(&host).unwrap().unwrap();
    assert_eq!(registry.accounts.len(), 2);
    assert_ne!(
        registry.accounts[0].provider(),
        registry.accounts[1].provider()
    );
    assert_eq!(registry.accounts[0].group.as_deref(), Some("work"));
    assert_eq!(registry.accounts[1].group.as_deref(), Some("work"));
}
