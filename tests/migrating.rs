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

use std::collections::BTreeMap;
use std::path::Path;

use common::*;
use perch::commands::config::ConfigCommand;
use perch::host::FakeHost;
use perch::host::prelude::*;
use perch::name;
use perch::registry::{
    self, CURRENT_VERSION, DEFAULT_WATCHER_THRESHOLD_PERCENT, Settings, Strategy,
};

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

/// The other half of it: `record_switch` writes one spelling of that key, so a
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
/// here, and both come forward under the one spelling `record_switch` writes. The
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

// ————— every version 1 registry, and not only the two on disk —————

/// The name rules the published version 1 builds shipped, at their loosest:
/// v0.1.0's four checks, its own `to_lowercase` fold and all.
///
/// History rather than a rule: v0.1.0, v0.1.1 and v0.2.0 are the only builds
/// that stamped version 1, so what they accepted cannot move again.
fn a_published_perch_accepted(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.chars().any(char::is_whitespace)
        && name.to_lowercase() != "none"
        && !name.contains('@')
}

/// Whole spellings no generated name reaches: the reserved words, both folds of
/// a Greek sigma, and the two shapes a terminal reads as an instruction.
const SPELLINGS: &[&str] = &[
    "work",
    "Work",
    "global",
    "Global",
    "GLOBAL",
    "ungrouped",
    "Ungrouped",
    "UNGROUPED",
    "-dev",
    "--dev",
    "-",
    "---",
    "group",
    "group-1",
    "alias",
    "\u{1b}[31mred",
    "red\u{202e}",
    "café",
    "CAFÉ",
    "ΟΔΟΣ",
    "οδος",
    "straße",
    "İ",
    "\u{ad}",
    "\u{feff}soft",
];

/// The characters every name rule so far has turned on.
const ALPHABET: &[char] = &['a', '-', '\u{1b}', '\u{202e}', 'Σ', 'ς', '.', '0'];

/// Every spelling up to three characters long over one alphabet.
///
/// Three deep, which is a leading character, a body and a trailing one: every
/// rule is per character or a whole-string `same_name` against a word
/// [`SPELLINGS`] holds. At four the corpus finds nothing and costs eight seconds.
fn every_spelling_over(alphabet: &[char]) -> Vec<String> {
    let mut names = Vec::new();
    let mut wider = vec![String::new()];
    for _ in 0..3 {
        wider = wider
            .iter()
            .flat_map(|held| {
                alphabet.iter().map(move |c| {
                    let mut name = held.clone();
                    name.push(*c);
                    name
                })
            })
            .collect();
        names.extend(wider.iter().cloned());
    }
    names
}

/// Every spelling a published Perch would have let somebody create, out of the
/// words above and the alphabet under them.
fn every_name_a_published_perch_accepted() -> Vec<String> {
    let mut names: Vec<String> = SPELLINGS.iter().map(|held| (*held).to_string()).collect();
    names.extend(every_spelling_over(ALPHABET));
    names.retain(|name| a_published_perch_accepted(name));
    names
}

/// One version 1 registry, with `group` in all three places a Group name is
/// written down — declared, claimed and keyed on by a Check — and `alias` in the
/// one place an Alias is.
fn a_v1_registry_naming(group: &str, alias: &str) -> String {
    serde_json::json!({
        "version": 1,
        "active": "one@example.com",
        "accounts": [
            {
                "identity": {
                    "email": "one@example.com",
                    "account_uuid": "uuid-one",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                },
                "plan": "max",
                "enabled": true,
                "group": group
            },
            {
                "identity": {
                    "email": "two@example.com",
                    "account_uuid": "uuid-two",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                },
                "enabled": true
            }
        ],
        "aliases": { alias: "two@example.com" },
        "groups": { group: { "watcher_threshold_percent": 80 } },
        "ungrouped": { "strategy": "soonest-reset" },
        "global": { "cycle_ungrouped": true, "settings": { "strategy": "most-headroom" } },
        "checks": { group: { "switched_at": "2026-08-14T10:00:00Z" } }
    })
    .to_string()
}

/// The corpus has to break the rules it is for. A generator that stopped
/// producing a name this build refuses would go on passing, having asserted
/// that the names Perch accepts are names Perch accepts.
#[test]
fn the_corpus_holds_names_this_build_refuses() {
    let refused = every_name_a_published_perch_accepted()
        .into_iter()
        .filter(|name| name::validate(name::NameKind::Group, name).is_err())
        .count();

    assert!(
        refused >= 10,
        "a corpus of names this build already accepts asserts nothing: {refused} of them are \
         refused"
    );
}

/// Every version 1 registry a published Perch could have written comes forward
/// into one this build reads (ADR a-class-not-its-instances).
///
/// Over the name space rather than the instances: a rule joining `validate_name`
/// that no step forward satisfies takes every command with it.
#[test]
fn every_name_a_published_perch_accepted_comes_forward_into_one_that_loads() {
    for name in every_name_a_published_perch_accepted() {
        // The counterparts are spelled outside the corpus, so no case pairs a
        // name with itself: a Group and an Alias of one name is the collision
        // every published `validate` refused, and so is no v1 registry.
        for (group, alias) in [(name.as_str(), "the-alias"), ("the-group", name.as_str())] {
            let host = machine_holding(&a_v1_registry_naming(group, alias));

            perch::migration::bring_forward(&host).unwrap_or_else(|refused| {
                panic!(
                    "a Group `{group}` and an Alias `{alias}` are \
                     names a published Perch accepted, and the step forward refused them: \
                     {refused}"
                )
            });

            registry::load(&host)
                .unwrap_or_else(|refused| {
                    panic!(
                        "a Group `{group}` and an Alias `{alias}` came \
                     forward into a registry no command can read: {refused}"
                    )
                })
                .expect("the registry is there");
        }
    }
}

// ————— every pair of names, and not only every name on its own —————

/// Whether the three published version 1 builds held these as two names.
///
/// All three compared with `to_lowercase`, and all three refused a pair it
/// called one — in either half of the namespace and across the two. So a pair
/// this refuses is in no registry a published Perch wrote.
fn a_published_perch_held_them_as_two(one: &str, other: &str) -> bool {
    one.to_lowercase() != other.to_lowercase()
}

/// Every pair out of the corpus a published Perch held as two names and this
/// build folds into one.
///
/// The corpus walks names one at a time and a fold is about a pair: `ΟΔΟΣ`
/// beside `οδος` is a registry v0.2.0 wrote and this build reads as one Group.
fn every_pair_this_build_folds_together() -> Vec<(String, String)> {
    let names = every_name_a_published_perch_accepted();
    let mut pairs = Vec::new();
    for (at, one) in names.iter().enumerate() {
        for other in &names[at + 1..] {
            if name::same_name(one, other) && a_published_perch_held_them_as_two(one, other) {
                pairs.push((one.clone(), other.clone()));
            }
        }
    }
    pairs
}

/// Every pair of a name this build refuses and the spelling the rename pass
/// gives it.
///
/// What the pass may rename onto is what nothing else in the registry answers
/// to, and a registry holding one name is one where every spelling is free.
fn every_pair_of_a_rename_and_where_it_lands() -> Vec<(String, String)> {
    every_name_a_published_perch_accepted()
        .into_iter()
        .filter_map(|name| {
            let is_now = perch::migration::renames(&a_v1_registry_naming(&name, "the-alias"))
                .into_iter()
                .find(|entry| entry.was == name)?
                .is_now;
            (is_now != name).then_some((name, is_now))
        })
        .collect()
}

/// Every pair of names the rename pass would put in one place.
///
/// Two refused names wanting one spelling is the case a registry holding a
/// single refused name cannot pose. Four per landing: the fifth asks what the
/// fourth asked, and the corpus offers up to forty-four.
fn every_pair_wanting_one_landing() -> Vec<(String, String)> {
    let mut wanting: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (was, is_now) in every_pair_of_a_rename_and_where_it_lands() {
        wanting.entry(is_now).or_default().push(was);
    }
    let mut pairs = Vec::new();
    for mut names in wanting.into_values() {
        // The corpus spells `-` both as a word and out of the alphabet, and one
        // JSON object holds one key: two of a spelling is no registry.
        names.sort();
        names.dedup();
        let names = &names[..names.len().min(4)];
        for (at, one) in names.iter().enumerate() {
            for other in &names[at + 1..] {
                pairs.push((one.clone(), other.clone()));
            }
        }
    }
    pairs
}

/// One version 1 registry naming two Groups and two Aliases, each Group with
/// Settings, an Account and a Check of its own.
///
/// Two of everything, because what a rename loses is not a name: it is one
/// Group's Settings folded into the other's, or an Account's place.
fn a_v1_registry_naming_two(groups: (&str, &str), aliases: (&str, &str)) -> String {
    let (first_group, second_group) = groups;
    let (first_alias, second_alias) = aliases;
    serde_json::json!({
        "version": 1,
        "active": "one@example.com",
        "accounts": [
            {
                "identity": {
                    "email": "one@example.com",
                    "account_uuid": "uuid-one",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                },
                "plan": "max",
                "enabled": true,
                "group": first_group
            },
            {
                "identity": {
                    "email": "two@example.com",
                    "account_uuid": "uuid-two",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                },
                "enabled": true,
                "group": second_group
            }
        ],
        "aliases": { first_alias: "one@example.com", second_alias: "two@example.com" },
        "groups": {
            first_group: { "watcher_threshold_percent": 70 },
            second_group: { "watcher_threshold_percent": 80 }
        },
        "ungrouped": { "strategy": "soonest-reset" },
        "global": { "cycle_ungrouped": true, "settings": { "strategy": "most-headroom" } },
        "checks": {
            first_group: { "switched_at": "2026-08-14T10:00:00Z" },
            second_group: { "switched_at": "2026-08-15T10:00:00Z" }
        }
    })
    .to_string()
}

/// The two names still two, each carrying what it carried.
///
/// `load` answering is not the claim. A rename that folds one Group into the
/// other comes forward into a registry every command reads happily, short one
/// Group, one set of Settings and one Account's place.
fn they_come_forward_as_two(groups: (&str, &str), aliases: (&str, &str)) {
    let host = machine_holding(&a_v1_registry_naming_two(groups, aliases));
    let said = format!(
        "Groups `{}` and `{}`, Aliases `{}` and `{}`",
        groups.0, groups.1, aliases.0, aliases.1
    );

    perch::migration::bring_forward(&host).unwrap_or_else(|refused| {
        panic!("{said}: a published Perch wrote them, and the step forward refused them: {refused}")
    });

    let registry = registry::load(&host)
        .unwrap_or_else(|refused| {
            panic!("{said}: they came forward into a registry no command can read: {refused}")
        })
        .expect("the registry is there");

    let claims = |email: &str| -> String {
        registry
            .account(email)
            .unwrap_or_else(|| panic!("{said}: {email} is not in the registry any more"))
            .group
            .clone()
            .unwrap_or_else(|| panic!("{said}: {email} came forward claiming no Group"))
    };
    let (first, second) = (claims("one@example.com"), claims("two@example.com"));

    assert_ne!(
        first, second,
        "{said}: two Accounts in two Groups came forward in one"
    );
    assert_eq!(
        registry.groups.len(),
        2,
        "{said}: two declared Groups came forward as {:?}",
        registry.groups.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        registry
            .groups
            .get(&first)
            .map(|held| held.watcher_threshold_percent),
        Some(70),
        "{said}: the Group `{first}` came forward holding Settings that are not its own"
    );
    assert_eq!(
        registry
            .groups
            .get(&second)
            .map(|held| held.watcher_threshold_percent),
        Some(80),
        "{said}: the Group `{second}` came forward holding Settings that are not its own"
    );
    assert_eq!(
        registry.aliases.len(),
        2,
        "{said}: two Aliases came forward as {:?}",
        registry.aliases.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        registry
            .aliases
            .values()
            .filter(|named| *named == "one@example.com")
            .count(),
        1,
        "{said}: one Account answers to {} of the two Aliases",
        registry
            .aliases
            .values()
            .filter(|named| *named == "one@example.com")
            .count()
    );
    assert_eq!(
        registry.checks.len(),
        2,
        "{said}: two Checks came forward as {:?}",
        registry.checks.keys().collect::<Vec<_>>()
    );
}

/// Every pair of names one version 1 registry could hold comes forward as two
/// names, each still carrying what it carried.
///
/// Over the pair space rather than the name space: the guard above puts one
/// corpus name in a registry at a time, and a fold decides about two.
#[test]
fn every_pair_a_published_perch_held_as_two_comes_forward_as_two() {
    let mut pairs = every_pair_this_build_folds_together();
    pairs.extend(every_pair_of_a_rename_and_where_it_lands());
    pairs.extend(every_pair_wanting_one_landing());

    for (one, other) in pairs {
        // All three arrangements of one namespace: both halves, and across.
        they_come_forward_as_two((&one, &other), ("the-alias", "the-other-alias"));
        they_come_forward_as_two(("the-group", "the-other-group"), (&one, &other));
        they_come_forward_as_two((&one, "the-group"), (&other, "the-alias"));
    }
}

/// Version 2's own name rules, at the loosest of the builds that stamped it.
///
/// A character rule joined `validate_name` part way through version 2 and the
/// version did not move with it, so version 2 is two shapes. The earlier one
/// refused no character at all, and a rename is owed every name it accepted.
fn a_version_2_perch_accepted(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.chars().any(char::is_whitespace)
        && !matches!(
            name.to_lowercase().as_str(),
            "none" | "ungrouped" | "global"
        )
        && !name.contains('@')
        && !name.starts_with('-')
}

/// The characters version 3 added to the unshowable set, beside two it already
/// had and one ordinary letter: a name is made of what it is allowed and what it
/// is not, and the pairs either side of a boundary are where a rule reads wrong.
const V2_ALPHABET: &[char] = &[
    'a', '-', '\u{FE00}', '\u{3164}', '\u{034F}', '\u{2065}', '\u{1b}', '\u{202e}',
];

/// Every spelling a version 2 Perch would have let somebody create, out of the
/// alphabet above.
fn every_name_a_version_2_perch_accepted() -> Vec<String> {
    let mut names = every_spelling_over(V2_ALPHABET);
    names.retain(|name| a_version_2_perch_accepted(name));
    names
}

/// One version 2 registry, with `group` in all three places a Group name is
/// written down and `alias` in the one place an Alias is. The shape version 3
/// reads: only the name rules moved between them.
fn a_v2_registry_naming(group: &str, alias: &str) -> String {
    serde_json::json!({
        "version": 2,
        "active": { "settled": "one@example.com" },
        "accounts": [
            {
                "identity": {
                    "email": "one@example.com",
                    "account_uuid": "uuid-one",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                },
                "plan": "max",
                "group": group
            },
            {
                "identity": {
                    "email": "two@example.com",
                    "account_uuid": "uuid-two",
                    "organization_name": "Acme",
                    "organization_uuid": "org-1"
                }
            }
        ],
        "aliases": { alias: "two@example.com" },
        "groups": { group: { "strategy": "most-headroom", "watcher_may_act": false, "watcher_threshold_percent": 80 } },
        "ungrouped": {
            "interchangeable": false,
            "settings": { "strategy": "most-headroom", "watcher_may_act": false, "watcher_threshold_percent": 80 }
        },
        "checks": { group: { "switched_at": "2026-08-14T10:00:00Z" } }
    })
    .to_string()
}

/// The corpus has to break the rules it is for, for the reason the version 1 one
/// does.
#[test]
fn the_version_2_corpus_holds_names_this_build_refuses() {
    let refused = every_name_a_version_2_perch_accepted()
        .into_iter()
        .filter(|name| name::validate(name::NameKind::Group, name).is_err())
        .count();

    assert!(
        refused >= 10,
        "a corpus of names this build already accepts asserts nothing: {refused} of them are \
         refused"
    );
}

/// Every version 2 registry a published Perch could have written comes forward
/// into one this build reads.
///
/// The version 1 guard's twin, on the version whose rules moved under it: a rule
/// joining `validate_name` with no step is a refusal `load` makes every command.
#[test]
fn every_name_a_version_2_perch_accepted_comes_forward_into_one_that_loads() {
    for name in every_name_a_version_2_perch_accepted() {
        for (group, alias) in [(name.as_str(), "the-alias"), ("the-group", name.as_str())] {
            let host = machine_holding(&a_v2_registry_naming(group, alias));

            perch::migration::bring_forward(&host).unwrap_or_else(|refused| {
                panic!(
                    "a Group `{group}` and an Alias `{alias}` are names a version 2 Perch \
                     accepted, and the step forward refused them: {refused}"
                )
            });

            registry::load(&host)
                .unwrap_or_else(|refused| {
                    panic!(
                        "a Group `{group}` and an Alias `{alias}` came forward into a registry \
                         no command can read: {refused}"
                    )
                })
                .expect("the registry is there");
        }
    }
}

/// The step's business is what Perch wrote, and a hand edit is neither carried
/// nor quietly given a different name.
#[test]
fn a_name_no_version_2_perch_accepted_is_named_rather_than_renamed() {
    let host = machine_holding(&a_v2_registry_naming("global", "the-alias"));

    let refused = registry::load(&host).expect_err("`global` is a name no Perch ever wrote");

    let said = refused.to_string();
    assert!(said.contains("global"), "the name is named: {said}");
    assert!(
        said.contains("registry.json"),
        "and the file to edit: {said}"
    );
}

/// The note names the character rather than the name it is in.
///
/// A name refused for a character that draws as nothing reaches a terminal as
/// one the registry may also hold, so quoting it would say a Group `dev` became
/// `dev-1` beside an untouched Group of exactly that name.
#[test]
fn the_note_for_a_rename_names_the_character_that_caused_it() {
    let document = a_v2_registry_naming("dev\u{FE00}", "the-alias");
    let held: serde_json::Value = serde_json::from_str(&document).expect("a document");
    let mut held = held.as_object().cloned().expect("an object");
    let mut groups = held["groups"].as_object().cloned().expect("groups");
    groups.insert(
        "dev".to_string(),
        groups.values().next().cloned().expect("settings"),
    );
    held.insert("groups".to_string(), serde_json::Value::Object(groups));
    let host = machine_holding(&serde_json::Value::Object(held).to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let said = host.notes().join("\n");
    assert!(
        said.contains("carrying a character a terminal does not draw as itself (U+FE00)"),
        "the note names the character: {said}"
    );
    assert!(
        !said.contains("a Group `dev` is now"),
        "and never quotes the stripped name, which is a Group this registry holds: {said}"
    );
}

// ————— every version 3 registry, and not only the ones on disk —————

/// One version 3 registry. Version 3 changed no shape, so it is the version 2
/// document under the later number.
fn a_v3_registry_naming(group: &str, alias: &str) -> String {
    let mut held: serde_json::Value =
        serde_json::from_str(&a_v2_registry_naming(group, alias)).expect("a document");
    held["version"] = serde_json::json!(3);
    held.to_string()
}

/// Version 3's own name rules, as version 3 held them.
///
/// The unshowable set is read off this build rather than spelled out, unlike
/// version 2's: version 4 left that set alone. What moved is the clauses around
/// it, and those are written here.
fn a_version_3_perch_accepted(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.chars().any(char::is_whitespace)
        && !name.chars().any(perch::host::is_unshowable)
        && !matches!(
            name.to_lowercase().as_str(),
            "none" | "ungrouped" | "global"
        )
        && !name.contains('@')
        && !name.starts_with('-')
}

/// The characters the allow-list turns on, against the four it carries: a
/// symbol, a punctuation mark, an emoji and a combining mark, beside a letter, a
/// digit, `_` and `-`.
const V3_ALPHABET: &[char] = &['a', '2', '-', '_', '★', '.', '\u{301}', '🚀'];

/// Every spelling a version 3 Perch would have let somebody create, out of the
/// alphabet above.
fn every_name_a_version_3_perch_accepted() -> Vec<String> {
    let mut names = every_spelling_over(V3_ALPHABET);
    names.retain(|name| a_version_3_perch_accepted(name));
    names
}

/// The corpus has to break the rules it is for, for the reason the version 1 one
/// does.
#[test]
fn the_version_3_corpus_holds_names_this_build_refuses() {
    let refused = every_name_a_version_3_perch_accepted()
        .into_iter()
        .filter(|name| name::validate(name::NameKind::Group, name).is_err())
        .count();

    assert!(
        refused >= 10,
        "a corpus of names this build already accepts asserts nothing: {refused} of them are \
         refused"
    );
}

/// Every version 3 registry a Perch could have written comes forward into one
/// this build reads.
///
/// The third of these. `validate_name` went from a deny-list to an allow-list,
/// so a name the step cannot repair is a refusal `load` makes every command.
#[test]
fn every_name_a_version_3_perch_accepted_comes_forward_into_one_that_loads() {
    for name in every_name_a_version_3_perch_accepted() {
        for (group, alias) in [(name.as_str(), "the-alias"), ("the-group", name.as_str())] {
            let host = machine_holding(&a_v3_registry_naming(group, alias));

            perch::migration::bring_forward(&host).unwrap_or_else(|refused| {
                panic!(
                    "a Group `{group}` and an Alias `{alias}` are names a version 3 Perch \
                     accepted, and the step forward refused them: {refused}"
                )
            });

            registry::load(&host)
                .unwrap_or_else(|refused| {
                    panic!(
                        "a Group `{group}` and an Alias `{alias}` came forward into a registry \
                         no command can read: {refused}"
                    )
                })
                .expect("the registry is there");
        }
    }
}

/// A name of nothing the allow-list carries leaves no base to repair, so the
/// kind supplies one — and the note says which name it was, since an emoji is
/// drawn.
#[test]
fn a_version_3_name_of_symbols_comes_forward_named_for_its_kind() {
    let host = machine_holding(&a_v3_registry_naming("🚀", "the-alias"));

    perch::migration::bring_forward(&host).expect("it comes forward");

    let said = host.notes().join("\n");
    assert!(
        said.contains("a Group `🚀` is now `group`"),
        "the note says what it renamed and to what: {said:?}"
    );
    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert!(
        written["groups"].get("group").is_some(),
        "and the file holds the new name: {written}"
    );
    assert_eq!(
        written["accounts"][0]["group"], "group",
        "with the Account claiming it: {written}"
    );

    // The whole point: every command works afterwards.
    let (outcome, printed) = run_list(&host, false);
    outcome.expect("a name the step forward moved is one `load` accepts");
    assert!(printed.contains("group"), "{printed}");
}

/// The repair takes out the characters rather than the name, so what a person
/// meant by it survives.
#[test]
fn a_version_3_name_of_a_word_and_a_symbol_keeps_the_word() {
    let host = machine_holding(&a_v3_registry_naming("dev★", "the-alias"));

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert!(
        written["groups"].get("dev").is_some(),
        "`dev★` is `dev`, not `group`: {written}"
    );
}

/// Renaming onto a name something already answers to would be no rename at all,
/// and the repair puts two version 3 names on one base every time.
#[test]
fn a_version_3_rename_that_would_collide_takes_the_suffix() {
    let mut held: serde_json::Value =
        serde_json::from_str(&a_v3_registry_naming("dev★", "the-alias")).expect("a document");
    let settings = held["groups"]
        .as_object()
        .and_then(|groups| groups.values().next().cloned())
        .expect("the Settings the fixture declares");
    held["groups"]["dev"] = settings;
    let host = machine_holding(&held.to_string());

    perch::migration::bring_forward(&host).expect("it comes forward");

    let written: serde_json::Value =
        serde_json::from_str(&on_disk(&host)).expect("a document came back");
    assert!(
        written["groups"].get("dev-1").is_some(),
        "`dev` is taken, so the repair takes the suffix: {written}"
    );
    assert!(
        written["groups"].get("dev").is_some(),
        "and the Group that was already called `dev` keeps its name: {written}"
    );
}

/// `load` is every command, so a name this build refuses is a machine with no
/// working `perch` on it — including `perch remove`, the one command that gives
/// an Account up, and the one somebody reaches for when nothing else works.
#[test]
fn a_registry_holding_a_name_this_build_refuses_still_has_a_perch_remove() {
    let host = machine_with_two_accounts();
    let mut registry = registry_of(&host);
    registry.version = 3;
    registry
        .groups
        .insert("🚀".to_string(), Settings::default());
    registry
        .accounts
        .iter_mut()
        .find(|account| account.email() == SECOND_EMAIL)
        .expect("the second Account")
        .group = Some("🚀".to_string());
    host.set_file(
        Path::new(REGISTRY_PATH),
        &serde_json::to_string(&registry).expect("a document"),
    );

    let (outcome, printed) = run_remove(&host, SECOND_EMAIL);

    outcome.expect("a command that reads a registry it can repair is a command that runs");
    assert!(
        registry_of(&host).account(SECOND_EMAIL).is_none(),
        "and the Account is gone: {printed}"
    );
}

// ————— a name rule and the registry version move together —————

/// What this build's name rules answer over a fixed corpus, as one number.
///
/// FNV-1a over the accept/reject bits. Written out rather than taken from a
/// crate, because what it has to be is stable across builds and not fast.
fn what_the_name_rules_answer() -> u64 {
    let mut names: Vec<String> = SPELLINGS.iter().map(|held| (*held).to_string()).collect();
    names.extend(every_spelling_over(ALPHABET));
    names.extend(every_spelling_over(V2_ALPHABET));
    names.sort();
    names.dedup();

    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for name in names {
        for kind in [name::NameKind::Group, name::NameKind::Alias] {
            let bit = u8::from(name::validate(kind, &name).is_ok());
            digest = (digest ^ u64::from(bit)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    digest
}

/// The rule table itself, as one number: every row, every rule, every set and
/// every fold, rendered and folded with FNV-1a.
///
/// Written out rather than taken from a crate, because what it has to be is
/// stable across builds and not fast.
fn what_the_table_holds() -> u64 {
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{:?}", perch::name::ROWS).bytes() {
        digest = (digest ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    digest
}

/// A name rule that moves takes the registry version with it. Two numbers,
/// neither covering the other: the table sees a row gain, lose or reorder a
/// rule for any character; the corpus sees a rule's own body change, bounded to
/// the spellings it holds. A row arriving or leaving is `registry.rs`'s `const`
/// assertion instead (ADR an-invariant-gets-a-door).
#[test]
fn a_name_rule_that_moves_takes_the_registry_version_with_it() {
    assert_eq!(
        (
            CURRENT_VERSION,
            what_the_table_holds(),
            what_the_name_rules_answer()
        ),
        (5, 0x33db_f1d3_4cc6_b249, 0xedc6_ef37_3813_2c93),
        "the name rules and the registry version have to move together: give \
         `migration::forward` a step for the shape below this one if the shape \
         moved, add the `name::Rules` row that says what the version below \
         accepted, bump `CURRENT_VERSION`, and record the new number here"
    );
}
