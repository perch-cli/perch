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
