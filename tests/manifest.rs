//! Configuration persistence and provider-local runtime (ADR a-fresh-provider-layout).

use perch::config::{Scope, Strategy};
use perch::domain::Identity;
use perch::host::{FakeHost, Files, Refusing, Waiting};
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
        // The file's name rather than its path: the Host spells the separator.
        assert!(
            error.contains(unknown) && error.contains("config.json"),
            "{error}"
        );
    }
    let runtime = json!({"version":registry::CURRENT_VERSION,"active":{"landing":{"leavign":"one@example.com","arriving":"one@example.com"}},"checks":{},"accounts":{}});
    let host = machine()
        .with_file(CONFIG, &document().to_string())
        .with_file(CLAUDE_STATE, &runtime.to_string());
    let error = registry::load(&host).unwrap_err().to_string();
    assert!(
        error.contains("leavign") && error.contains("state.json"),
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

/// A runtime record that is there and will not be read is not an absent one:
/// reading it as absent would leave the provider with no Default and no
/// Cooldown, and the next command would Switch inside one still running.
#[test]
fn a_runtime_record_that_will_not_be_read_is_a_failure_rather_than_an_absence() {
    let host = machine()
        .with_file(CONFIG, &document().to_string())
        .with_file(
            CLAUDE_STATE,
            &json!({"version":registry::CURRENT_VERSION,
            "active":registry::Active::Nobody,"checks":{},"accounts":{}})
            .to_string(),
        )
        .with_a_path_refusing(CLAUDE_STATE, Refusing::Read, "Permission denied");

    let error = registry::load(&host).unwrap_err().to_string();

    assert!(error.contains("state.json"), "{error}");
    assert!(error.contains("Permission denied"), "{error}");
}

/// The order Accounts are listed in is a position each one carries, so two
/// carrying the same one leave the order to whatever the map happened to yield.
#[test]
fn two_accounts_claiming_one_position_are_refused_rather_than_ordered_arbitrarily() {
    let mut manifest = document();
    let mut second = manifest["accounts"]["one@example.com"].clone();
    second["identity"]["email"] = "two@example.com".into();
    manifest["accounts"]["two@example.com"] = second;
    let host = machine().with_file(CONFIG, &manifest.to_string());

    let error = registry::load(&host).unwrap_err().to_string();

    assert!(error.contains("same position"), "{error}");
}

/// The directory key is what every command looks an Account up by, and an
/// Account carrying a provider identity derives its key from that identity —
/// so a manifest filing one under a different name is a lookup that misses.
#[test]
fn an_account_filed_under_a_key_its_identity_does_not_derive_is_refused() {
    let mut manifest = document();
    manifest["accounts"]["one@example.com"]["provider_identity"] =
        json!({"user_id":"user-1","workspace_id":null,"key":"claude:not-this-one"});
    let host = machine().with_file(CONFIG, &manifest.to_string());

    let error = registry::load(&host).unwrap_err().to_string();

    assert!(error.contains("directory key"), "{error}");
}

/// The runtime carries its own version, and a record this build does not
/// understand is refused with the same instruction the manifest gives: there is
/// no migration, so starting fresh is the whole of the way forward.
#[test]
fn a_runtime_record_of_another_version_asks_for_a_fresh_installation_too() {
    for version in [registry::CURRENT_VERSION - 1, registry::CURRENT_VERSION + 1] {
        let runtime = json!({"version":version,"active":registry::Active::Nobody,
            "checks":{},"accounts":{}});
        let host = machine()
            .with_file(CONFIG, &document().to_string())
            .with_file(CLAUDE_STATE, &runtime.to_string());

        let error = registry::load(&host).unwrap_err().to_string();

        assert!(
            error.contains("fresh installation"),
            "version {version}: {error}"
        );
    }
}

/// A Switch recorded as under way to an Account the manifest no longer holds:
/// the arrival is nowhere to go, so what is left is the Account it was leaving,
/// which is where the machine still is.
#[test]
fn a_landing_on_an_account_the_manifest_no_longer_holds_comes_back_to_the_one_being_left() {
    let runtime = json!({"version":registry::CURRENT_VERSION,
        "active":{"landing":{"leaving":"one@example.com","arriving":"gone@example.com"}},
        "checks":{},"accounts":{}});
    let host = machine()
        .with_file(CONFIG, &document().to_string())
        .with_file(CLAUDE_STATE, &runtime.to_string());

    let registry = registry::load(&host).unwrap().unwrap();

    assert_eq!(
        *registry.active(),
        registry::Active::Settled("one@example.com".into())
    );
}

/// `perch run` falls back to the installed CLI unless somebody says not to, and
/// the manifest says which of the two it is in words rather than as a flag —
/// so a person reading the file can tell what it will do.
#[test]
fn a_run_that_may_not_fall_back_says_so_in_the_manifest_and_reads_back_the_same() {
    let host = machine();
    let mut registry = Registry::default();
    registry.run_fallback = false;
    save(&host, &mut registry);

    let written: Value =
        serde_json::from_str(&host.file(std::path::Path::new(CONFIG)).unwrap()).unwrap();
    assert_eq!(written["global"]["run"]["fallback"], "disabled");
    assert!(!registry::load(&host).unwrap().unwrap().run_fallback);
}

/// Each file checks the hold as it is about to be written, rather than trusting
/// the one check the command made before any of them: `storage::save` writes
/// several files, and the hold can go between two of them. Called under the
/// command's guard rather than through it, which is the moment being described.
#[test]
fn every_file_a_save_writes_checks_the_hold_rather_than_the_command_checking_once() {
    let host = machine();
    let mut held = perch::holdings::lock(&host).unwrap();
    // Perch's hold goes quiet past the staleness window, and somebody clears the
    // artifact and makes their own — which carries their timestamp, not Perch's.
    host.sleep(200_000);
    host.touch(std::path::Path::new("/perch/.registry.lock"))
        .unwrap();

    let mut registry = Registry::default();
    registry.upsert(account("one@example.com"));
    let refused = perch::storage::save(&host, &mut held, &registry).unwrap_err();

    assert!(refused.to_string().contains("lock was lost"), "{refused}");
    assert!(
        host.file(std::path::Path::new(CONFIG)).is_none(),
        "and nothing was written over theirs"
    );
}
