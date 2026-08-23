//! Behavior: a registry an older Perch wrote, on this build
//! (ADR a-registry-comes-forward).
//!
//! The fixtures are what those releases actually wrote, serialized by each tag's
//! own serde impls rather than by hand — the shape is the claim here, so a
//! fixture written from this tree's memory of it would assert nothing.
//!
//! A case per released version rather than per field that moved: both stamped
//! `"version": 1` and their shapes differ, so a per-field case over one document
//! passes while the other stays unreadable.

mod common;

use std::path::Path;

use common::*;
use perch::commands::config::ConfigCommand;
use perch::host::FakeHost;
use perch::host::prelude::*;
use perch::registry::{self, CURRENT_VERSION, DEFAULT_WATCHER_THRESHOLD_PERCENT, Strategy};

/// A registry v0.2.0 wrote: `active` a bare address, `groups` a map of partial
/// Overrides, an `ungrouped` beside a `global`, and one Account kept out of
/// Cycling.
const V0_2_0: &str = include_str!("fixtures/registry-v0.2.0.json");

/// A registry v0.1.1 wrote, under the same version number: `groups` a map of
/// whole GroupConfigs, no `ungrouped` at all, and a `global` holding one flag.
const V0_1_1: &str = include_str!("fixtures/registry-v0.1.1.json");

fn machine_holding(registry: &str) -> FakeHost {
    logged_in_machine().with_file(REGISTRY_PATH, registry)
}

fn on_disk(host: &FakeHost) -> String {
    host.read_file(Path::new(REGISTRY_PATH))
        .expect("the registry is there")
}

#[test]
fn a_registry_v0_2_0_wrote_is_read_rather_than_refused() {
    let host = machine_holding(V0_2_0);

    let (outcome, printed) = run_list(&host, false);

    outcome.expect("a registry Perch itself wrote three releases ago is readable");
    assert!(printed.contains("work@example.com"), "{printed}");
    assert!(printed.contains("old@example.com"), "{printed}");
}

#[test]
fn a_registry_v0_1_1_wrote_is_read_rather_than_refused() {
    let host = machine_holding(V0_1_1);

    let (outcome, printed) = run_list(&host, false);

    outcome.expect("the shape before the Override layer is readable too");
    assert!(printed.contains("work@example.com"), "{printed}");
}

/// The one field whose polarity inverted: read as an absent `disabled` rather
/// than translated, an Account somebody took out of Cycling comes back into it.
#[test]
fn an_account_kept_out_of_cycling_is_still_out_of_it() {
    for (which, fixture) in [("v0.2.0", V0_2_0), ("v0.1.1", V0_1_1)] {
        let host = machine_holding(fixture);

        let (outcome, printed) = run_list(&host, false);

        outcome.expect("it reads");
        assert!(
            printed.contains("disabled"),
            "the Account {which} disabled is still disabled: {printed}"
        );
    }
}

/// What a Group Inherited is read out of Global, because Global is where the
/// value somebody set actually was. `most-headroom` here is the Strategy the
/// Group never declared, and 80 is the threshold it did.
#[test]
fn what_a_group_inherited_from_global_arrives_as_the_group_holds_it() {
    let host = machine_holding(V0_2_0);

    let registry = registry::load(&host)
        .expect("it reads")
        .expect("it is there");

    let work = registry.group("work").expect("the Group is declared");
    assert_eq!(work.strategy, Strategy::MostHeadroom);
    assert!(work.watcher_may_act);
    assert_eq!(work.watcher_threshold_percent, 80);
}

/// Global's own Settings become the Ungrouped Scope's, and its one flag becomes
/// the declaration that those Accounts are interchangeable.
#[test]
fn global_becomes_the_ungrouped_scope() {
    let host = machine_holding(V0_2_0);

    let registry = registry::load(&host)
        .expect("it reads")
        .expect("it is there");

    assert!(registry.ungrouped.interchangeable);
    assert_eq!(registry.ungrouped.settings.strategy, Strategy::SoonestReset);
    assert_eq!(registry.ungrouped.settings.watcher_threshold_percent, 85);
}

/// Every Setting, as the command a person would read it with: what a Group
/// declared, what it Inherited, and the Scope Global became.
#[test]
fn every_setting_arrives_where_the_old_registry_put_it() {
    let host = machine_holding(V0_2_0);

    let (outcome, printed) = run_config(&host, ConfigCommand::Get { words: Vec::new() });

    outcome.expect("it reads");
    for line in [
        "ungrouped interchangeable true",
        "ungrouped strategy soonest-reset",
        "ungrouped watcher-threshold-percent 85",
        "work strategy most-headroom",
        "work watcher-may-act true",
        "work watcher-threshold-percent 80",
    ] {
        assert!(
            printed.contains(line),
            "`{line}` is missing from:\n{printed}"
        );
    }
}

/// The shape with no Ungrouped Scope in it at all: the declaration comes off
/// Global's one flag, and the Settings it never held take the defaults.
#[test]
fn the_older_shape_gains_the_scope_it_never_had() {
    let host = machine_holding(V0_1_1);

    let (outcome, printed) = run_config(&host, ConfigCommand::Get { words: Vec::new() });

    outcome.expect("it reads");
    for line in [
        "ungrouped interchangeable true".to_string(),
        format!("ungrouped watcher-threshold-percent {DEFAULT_WATCHER_THRESHOLD_PERCENT}"),
        "work watcher-may-act true".to_string(),
    ] {
        assert!(
            printed.contains(&line),
            "`{line}` is missing from:\n{printed}"
        );
    }
}

#[test]
fn the_account_the_registry_was_left_on_is_still_the_active_one() {
    let host = machine_holding(V0_2_0);

    let registry = registry::load(&host)
        .expect("it reads")
        .expect("it is there");

    assert_eq!(registry.active().whose(), Some("work@example.com"));
}

/// The run that migrates writes the result back, so the step is paid once rather
/// than on every command for the rest of the machine's life.
#[test]
fn the_registry_is_written_back_in_the_shape_this_build_reads() {
    let host = machine_holding(V0_2_0);

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written = on_disk(&host);
    assert!(
        written.contains(&format!("\"version\": {CURRENT_VERSION}")),
        "{written}"
    );
    assert!(!written.contains("\"global\""), "{written}");
    assert!(!written.contains("\"enabled\""), "{written}");
    assert!(!written.contains("switched_off"), "{written}");
}

/// Said because a file the user did not ask to have rewritten was rewritten, and
/// on stderr because a `--json` document is what stdout is for.
/// Every command goes through `load`, so a name this build refuses is a machine
/// with no working `perch` on it until the step forward moves the name. Driven
/// end to end, because the rename has to reach the file as well as the read.
#[test]
fn a_name_this_build_refuses_is_renamed_on_disk_and_the_note_says_which() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert(
        "groups".to_string(),
        serde_json::json!({ "global": { "watcher_threshold_percent": 70 } }),
    );
    held.insert(
        "aliases".to_string(),
        serde_json::json!({ "-w": "work@example.com" }),
    );
    held.insert(
        "checks".to_string(),
        serde_json::json!({ "global": { "switched_at": "2026-08-14T10:00:00Z" } }),
    );
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        if account.get("group").is_some() {
            account["group"] = serde_json::json!("global");
        }
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let said = host.notes().join("\n");
    assert!(
        said.contains("a Group `global` is now `global-1`"),
        "the note says what it renamed and to what: {said:?}"
    );
    assert!(
        said.contains("an Alias `-w` is now `w`"),
        "both halves of the namespace: {said:?}"
    );

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert!(
        written["groups"].get("global-1").is_some(),
        "and the file holds the new name: {written}"
    );
    assert!(
        written["checks"].get("global-1").is_some(),
        "with the Check keyed on it, or the Cooldown paces nothing: {written}"
    );
    assert!(
        written["aliases"].get("w").is_some(),
        "and the Alias with them: {written}"
    );

    // The whole point: every command works afterwards.
    let (outcome, printed) = run_list(&host, false);
    outcome.expect("a name the step forward moved is one `load` accepts");
    assert!(printed.contains("global-1"), "{printed}");
}

#[test]
fn the_migration_says_so_once() {
    let host = machine_holding(V0_2_0);

    perch::migration::bring_forward(&host).expect("it comes forward");

    let said = host.notes().join("\n");
    assert!(
        said.contains("version 1") && said.contains(&format!("version {CURRENT_VERSION}")),
        "it says which version it read and which it wrote: {said:?}"
    );

    host.forget_notes();
    perch::migration::bring_forward(&host).expect("there is nothing left to do");
    assert!(
        host.notes().is_empty(),
        "and says nothing on a run that migrated nothing: {:?}",
        host.notes()
    );
}

#[test]
fn a_registry_this_build_wrote_is_not_rewritten() {
    let host = machine_holding(V0_2_0);
    perch::migration::bring_forward(&host).expect("it comes forward");
    let settled = on_disk(&host);

    perch::migration::bring_forward(&host).expect("and again");

    assert_eq!(on_disk(&host), settled, "byte for byte");
}

/// A machine Perch has never run on has nothing to bring forward, and nothing to
/// adopt either: adoption is a command's to do when it is asked for.
#[test]
fn a_machine_with_no_registry_is_left_alone() {
    let host = logged_in_machine();

    perch::migration::bring_forward(&host).expect("there is nothing there");

    assert!(
        host.read_file(Path::new(REGISTRY_PATH)).is_err(),
        "and none was written"
    );
}

/// A number no Perch stamped says nothing about which shape the file is in, and
/// reading it as the current one is a guess.
#[test]
fn a_version_no_perch_wrote_is_refused() {
    for claimed in ["0", "null"] {
        let host = machine_holding(&format!(
            "{{\"version\":{claimed},\"accounts\":[],\"aliases\":{{}}}}"
        ));

        let refused = registry::load(&host).expect_err("this is not a registry Perch wrote");
        let said = refused.to_string();
        assert!(
            said.contains(&CURRENT_VERSION.to_string()),
            "it says what this build reads: {said}"
        );
    }
}

#[test]
fn a_registry_with_no_version_at_all_is_refused() {
    let host = machine_holding(r#"{"accounts":[],"aliases":{}}"#);

    let refused = registry::load(&host).expect_err("every Perch has written one");
    let said = refused.to_string();
    assert!(
        said.contains("version"),
        "and says that is what is missing: {said}"
    );
}

/// The guard the version field exists for, in the direction it always worked:
/// unchanged, and asserted here beside its mirror.
#[test]
fn a_registry_from_a_newer_perch_is_still_refused() {
    let host = machine_holding(&format!(
        "{{\"version\":{},\"accounts\":[]}}",
        CURRENT_VERSION + 1
    ));

    let refused = registry::load(&host).expect_err("this build does not understand it");
    assert!(refused.to_string().contains("Upgrade Perch"), "{refused}");
}

/// The version is read as a `u64` so that the guard reaches every number a newer
/// Perch could stamp. Read as a `u32` it did not: a number past that ceiling
/// fell through to serde, and the one thing the version exists to catch was
/// reported as a corrupt file to hand-edit.
#[test]
fn a_version_past_this_builds_integer_ceiling_still_says_upgrade_perch() {
    let host = machine_holding(&format!(
        "{{\"version\":{},\"accounts\":[]}}",
        u64::from(u32::MAX) + 1
    ));

    let refused = registry::load(&host).expect_err("this build does not understand it");
    let said = refused.to_string();
    assert!(said.contains("Upgrade Perch"), "{said}");
    assert!(
        said.contains("4294967296"),
        "and it says the version it read rather than one it clamped to: {said}"
    );
}

/// A file that is not a document at all keeps the refusal it has: the migration
/// makes no claim about nonsense.
#[test]
fn a_registry_that_is_not_json_is_still_malformed() {
    let host = machine_holding("this was never a registry");

    let refused = registry::load(&host).expect_err("it is not readable");
    assert!(refused.to_string().contains("could not read"), "{refused}");
}

/// Every command reads the registry, so bringing it forward is not one command's
/// business — but it is the *commands* that must all work, and a listing is the
/// cheapest proof that one does end to end.
#[test]
fn a_command_run_against_an_old_registry_leaves_a_current_one_behind() {
    let host = machine_holding(V0_1_1);

    perch::migration::bring_forward(&host).expect("it comes forward");
    let (outcome, printed) = run_list(&host, false);

    outcome.expect("the listing works");
    assert!(printed.contains("work@example.com"), "{printed}");
    let written = on_disk(&host);
    assert!(
        written.contains(&format!("\"version\": {CURRENT_VERSION}")),
        "{written}"
    );
}

/// The rename pass decides which names collide with `same_name` and then matched
/// the claim byte-exactly, so a claim spelled in another case kept naming a Group
/// the rename had just taken away.
#[test]
fn an_account_claiming_its_group_in_another_case_is_renamed_with_it() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert(
        "groups".to_string(),
        serde_json::json!({ "-dev": { "watcher_threshold_percent": 70 } }),
    );
    held.insert("checks".to_string(), serde_json::json!({}));
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        if account.get("group").is_some() {
            account["group"] = serde_json::json!("-DEV");
        }
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let registry = registry::load(&host)
        .expect("and every command can read it afterwards")
        .expect("a registry is there");
    assert!(
        registry.group("dev").is_some(),
        "the Group came forward under the accepted name: {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        registry.groups.len(),
        1,
        "and the claim did not declare a second one: {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
}

/// `ungrouped` is a legitimate `checks` key and is not a Group, so the Ungrouped
/// Scope's Cooldown must not be re-keyed onto a Group the pass renamed.
#[test]
fn the_ungrouped_scopes_cooldown_is_not_moved_onto_a_group_called_ungrouped() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert(
        "groups".to_string(),
        serde_json::json!({ "ungrouped": { "watcher_threshold_percent": 70 } }),
    );
    held.insert(
        "checks".to_string(),
        serde_json::json!({ "ungrouped": { "switched_at": "2026-08-14T10:00:00Z" } }),
    );
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        if account.get("group").is_some() {
            account["group"] = serde_json::json!("ungrouped");
        }
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert!(
        written["checks"].get("ungrouped").is_some(),
        "the Ungrouped Scope keeps the Cooldown it recorded: {written}"
    );
    assert!(
        written["checks"].get("ungrouped-1").is_none(),
        "and the renamed Group is paced by a Switch it never made: {written}"
    );
}

/// The other half of it: `record_check` writes one spelling of that key, so a
/// Cooldown that came forward under any other one is a second key `validate`
/// refuses the moment the next Check tries to record its own.
#[test]
fn an_ungrouped_cooldown_comes_forward_under_the_spelling_a_check_writes() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert(
        "groups".to_string(),
        serde_json::json!({ "Ungrouped": { "watcher_threshold_percent": 70 } }),
    );
    held.insert(
        "checks".to_string(),
        serde_json::json!({ "Ungrouped": { "switched_at": "2026-08-14T10:00:00Z" } }),
    );
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        if account.get("group").is_some() {
            account["group"] = serde_json::json!("Ungrouped");
        }
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert_eq!(
        written["checks"].as_object().map(serde_json::Map::len),
        Some(1),
        "one key, and it is the one a Check writes: {written}"
    );
    assert!(written["checks"].get("ungrouped").is_some(), "{written}");
    // And the next Check really does record against it rather than building a
    // registry `save` refuses.
    let registry = registry::load(&host)
        .expect("the migrated registry loads")
        .expect("and there is one there");
    assert!(
        registry.checks.contains_key("ungrouped"),
        "{:?}",
        registry.checks
    );
}

/// `enabled` inverted into `disabled`, and a value that is no boolean read as
/// "not false" — so an Account somebody took out of Cycling came back into it,
/// which is the one loss this step refuses everywhere else.
#[test]
fn an_enabled_flag_that_is_not_a_boolean_is_refused_rather_than_read_as_true() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")[0]["enabled"] = serde_json::json!("false");
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    let refused =
        registry::load(&host).expect_err("Perch will not guess which shape the rest is in");

    assert!(
        refused.to_string().contains("accounts[].enabled"),
        "the field is named: {refused}"
    );
    assert!(
        refused.to_string().contains("registry.json"),
        "and so is the file it is in, as every other refusal `load` makes names \
         one — a field with no file is a repair nobody can start: {refused}"
    );
}

/// The pass read the Group names off `groups` alone, so a Group only an Account
/// claimed was carried through untouched — and `load`, which declares every
/// claim it finds, then declared the very name `validate` refuses.
#[test]
fn a_group_only_an_account_claims_is_renamed_rather_than_left_to_brick_the_machine() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert("groups".to_string(), serde_json::json!({}));
    held.insert("checks".to_string(), serde_json::json!({}));
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        account["group"] = serde_json::json!("-dev");
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let (outcome, printed) = run_list(&host, false);
    outcome.expect("and every command works afterwards, the repairs among them");
    assert!(
        printed.contains("dev"),
        "under the accepted name: {printed}"
    );
    let registry = registry::load(&host)
        .expect("the registry reads")
        .expect("a registry is there");
    assert!(
        registry.group("dev").is_some(),
        "the claim declares the renamed Group: {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
}

/// A Group called `Ungrouped` and the Ungrouped Scope itself both key a record
/// here, and both come forward under the one spelling `record_check` writes. The
/// later of the two is the one that survives: the older one winning is a Check
/// free to Switch inside a Cooldown that is still running.
#[test]
fn two_v1_cooldowns_landing_on_one_key_keep_the_later_switch() {
    let held: serde_json::Value = serde_json::from_str(V0_2_0).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    held.insert(
        "groups".to_string(),
        serde_json::json!({ "Ungrouped": { "watcher_threshold_percent": 70 } }),
    );
    held.insert(
        "checks".to_string(),
        serde_json::json!({
            "Ungrouped": { "switched_at": "2026-08-14T10:00:00Z" },
            "ungrouped": { "switched_at": "2026-01-01T00:00:00Z" },
        }),
    );
    for account in held
        .get_mut("accounts")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the fixture lists Accounts")
    {
        account["group"] = serde_json::json!("Ungrouped");
    }
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert_eq!(
        written["checks"]["ungrouped"]["switched_at"], "2026-08-14T10:00:00Z",
        "the August record paces the Scope, not the January one: {written}"
    );
}
