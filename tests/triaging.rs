//! Behavior: what `perch triage` writes down, what it launches, and what it
//! declines to launch.
//!
//! Plus the one correspondence a Triage rests on and nothing else asserts: the
//! playbook tells an agent to fill fields, and the template decides whether
//! those fields exist.

mod common;

use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use common::*;
use perch::commands::triage::TriageArgs;
use perch::error::EXIT_OK;
use perch::host::fake::Effect;
use perch::host::{FakeHost, Files, Platform};
use perch::registry::Quarantine;

/// The moment every run below starts, so the directory it writes into is
/// knowable from the test rather than read back out of the output.
const AT: i64 = 1_787_059_012_431;

const TRIAGE: &str = "/Users/someone/.config/perch/triage";

fn at_a_fixed_moment(host: FakeHost) -> FakeHost {
    host.with_now(Utc.timestamp_millis_opt(AT).single().expect("a moment"))
}

fn run_triage(host: &FakeHost, args: TriageArgs) -> (i32, String) {
    // The fixtures log an Account in, which is an interactive launch of its own:
    // what this suite asserts about is what the Triage does after that.
    host.forget_effects();
    let mut written = Vec::new();
    let code = perch::commands::triage::run(host, args, &mut written).expect("a Triage answers");
    (code, String::from_utf8(written).expect("output is UTF-8"))
}

fn triaged(host: &FakeHost) -> (i32, String) {
    run_triage(
        host,
        TriageArgs {
            model: None,
            raw: false,
        },
    )
}

fn run_dir() -> PathBuf {
    Path::new(TRIAGE).join(format!("run-{AT}"))
}

fn written(host: &FakeHost, name: &str) -> String {
    host.file(run_dir().join(name))
        .unwrap_or_else(|| panic!("{name} is written"))
}

/// What the machine was asked to launch, if anything.
fn launched(host: &FakeHost) -> Option<(String, Vec<String>)> {
    host.effects().into_iter().find_map(|effect| match effect {
        Effect::ExecInteractive { program, args, .. } => Some((program, args)),
        _ => None,
    })
}

#[test]
fn a_triage_writes_the_playbook_and_both_readings_of_the_probe() {
    let host = at_a_fixed_moment(machine_with_two_accounts());

    triaged(&host);

    let prompt = written(&host, "prompt.md");
    assert!(
        prompt.contains("You are a support engineer for Perch"),
        "the playbook travels with the prompt: {prompt}"
    );
    assert!(
        prompt.contains("probe.raw.txt") && prompt.contains("probe.txt"),
        "and points at both readings: {prompt}"
    );

    let raw = written(&host, "probe.raw.txt");
    let redacted = written(&host, "probe.txt");
    assert!(
        raw.contains(EMAIL),
        "the agent investigates from names: {raw}"
    );
    assert!(
        !redacted.contains(EMAIL) && !redacted.contains(SECOND_EMAIL),
        "and pastes placeholders: {redacted}"
    );
    assert!(
        redacted.contains("<account 1>"),
        "which are numbered by the registry: {redacted}"
    );
    assert!(
        raw.contains(CLAUDE_VERSION) && redacted.contains(CLAUDE_VERSION),
        "both readings are the same gathering"
    );
}

/// The flag is for whoever is debugging the Triage itself, so it reaches the
/// copy meant for an issue and leaves the agent's copy alone.
#[test]
fn raw_writes_the_names_into_the_copy_that_would_be_pasted() {
    let host = at_a_fixed_moment(machine_with_two_accounts());

    run_triage(
        &host,
        TriageArgs {
            model: None,
            raw: true,
        },
    );

    assert_eq!(written(&host, "probe.txt"), written(&host, "probe.raw.txt"));
    assert!(written(&host, "probe.txt").contains(EMAIL));
}

#[test]
fn a_triage_hands_claude_code_the_file_rather_than_the_playbook() {
    let host = at_a_fixed_moment(machine_with_two_accounts());

    let (code, said) = triaged(&host);

    let (program, args) = launched(&host).expect("Claude Code is launched");
    assert_eq!(program, CLAUDE_BIN);
    assert_eq!(
        args.len(),
        1,
        "one word, for a `.cmd` shim on Windows: {args:?}"
    );
    assert!(
        args[0].contains(&run_dir().join("prompt.md").display().to_string()),
        "and it names the prompt: {args:?}"
    );
    assert!(
        !args[0].contains("You are a support engineer"),
        "rather than carrying it: {args:?}"
    );
    assert_eq!(code, EXIT_OK, "what Claude Code exited with");
    assert!(said.is_empty(), "stdout belongs to the session: {said}");
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("Starting Claude Code")),
        "Perch's own remark is a note: {:?}",
        host.notes()
    );
}

/// Bare, and not a Run: pointing a process at a Profile is what a Run is, and
/// every part of one is machinery a Triage may be investigating.
#[test]
fn a_triage_points_claude_code_at_no_profile() {
    let host = at_a_fixed_moment(machine_with_two_accounts());

    triaged(&host);

    let told = host.effects().into_iter().find_map(|effect| match effect {
        Effect::ExecInteractive { env, .. } => Some(env),
        _ => None,
    });
    assert_eq!(
        told,
        Some(Vec::new()),
        "nothing is added to the environment"
    );
}

#[test]
fn a_model_is_passed_through_and_nothing_is_passed_by_default() {
    let host = at_a_fixed_moment(machine_with_two_accounts());

    run_triage(
        &host,
        TriageArgs {
            model: Some("a-model".to_string()),
            raw: false,
        },
    );

    let (_, args) = launched(&host).expect("Claude Code is launched");
    assert_eq!(args[..2], ["--model".to_string(), "a-model".to_string()]);

    let bare = at_a_fixed_moment(machine_with_two_accounts());
    triaged(&bare);
    let (_, args) = launched(&bare).expect("Claude Code is launched");
    assert!(!args.iter().any(|word| word == "--model"), "{args:?}");
}

#[test]
fn what_claude_code_exited_with_is_what_a_triage_exits_with() {
    let host = at_a_fixed_moment(machine_with_two_accounts()).with_login(|_, _| 3);

    let (code, _) = triaged(&host);

    assert_eq!(code, 3);
}

#[test]
fn a_quarantined_active_account_is_said_rather_than_handed_to_a_login_prompt() {
    let host = at_a_fixed_moment(machine_with_two_accounts());
    quarantine_for(&host, EMAIL, Quarantine::RenewalRejected);

    let (code, said) = triaged(&host);

    assert!(launched(&host).is_none(), "nothing is launched: {said}");
    assert_eq!(code, EXIT_OK, "which is an answer rather than a refusal");
    assert!(said.contains("Quarantined"), "{said}");
    for name in ["prompt.md", "probe.raw.txt", "probe.txt"] {
        assert!(
            said.contains(&run_dir().join(name).display().to_string()),
            "{name} is named so it can be pasted by hand: {said}"
        );
    }
}

/// A Quarantine somewhere else says nothing about whether Claude Code will come
/// up, so it withholds nothing.
#[test]
fn a_quarantine_on_another_account_launches_as_usual() {
    let host = at_a_fixed_moment(machine_with_two_accounts());
    quarantine_for(&host, SECOND_EMAIL, Quarantine::RenewalRejected);

    let (_, said) = triaged(&host);

    assert!(launched(&host).is_some(), "{said}");
}

#[test]
fn a_credential_that_will_not_read_is_said_rather_than_launched_into() {
    let host = at_a_fixed_moment(machine_with_two_accounts().with_platform(Platform::Other));
    host.set_file(CREDENTIALS_PATH, "{}");

    let (code, said) = triaged(&host);

    assert!(launched(&host).is_none(), "nothing is launched: {said}");
    assert_eq!(code, EXIT_OK);
    assert!(
        said.contains("Claude Code will not come up"),
        "and it says which belief stopped holding: {said}"
    );
    assert!(
        host.file(run_dir().join("prompt.md")).is_some(),
        "the evidence is written either way"
    );
}

#[test]
fn no_claude_code_writes_the_files_and_says_where_they_are() {
    let host = at_a_fixed_moment(machine_with_two_accounts());
    host.remove_file(Path::new(CLAUDE_BIN))
        .expect("Claude Code is uninstalled");

    let (code, said) = triaged(&host);

    assert!(launched(&host).is_none(), "{said}");
    assert_eq!(code, EXIT_OK);
    assert!(said.contains("prompt.md"), "{said}");
    assert!(
        said.contains("Paste prompt.md into any coding agent"),
        "the files are the fallback: {said}"
    );
}

/// Evidence rather than one of the Holdings: three runs is enough to hold the
/// one before a fix beside the one after it.
#[test]
fn only_the_newest_three_runs_are_kept() {
    let mut host = machine_with_two_accounts();
    let mut kept = Vec::new();
    for minute in 0..5 {
        let at = Utc
            .timestamp_millis_opt(AT + minute * 60_000)
            .single()
            .expect("a moment");
        host = host.with_now(at);
        triaged(&host);
        kept.push(format!("run-{}", at.timestamp_millis()));
    }

    let held: Vec<String> = host
        .list_dir(Path::new(TRIAGE))
        .expect("the triage directory is there")
        .into_iter()
        .filter_map(|path| Some(path.file_name()?.to_str()?.to_string()))
        .collect();

    assert_eq!(held.len(), 3, "{held:?}");
    for newest in &kept[2..] {
        assert!(
            held.contains(newest),
            "{newest} is one of the newest: {held:?}"
        );
    }
}

/// The playbook tells an agent to fill fields it does not own. A field renamed
/// in the template is one GitHub silently drops, and nothing else would notice.
#[test]
fn every_field_the_playbook_names_is_one_the_template_offers() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let playbook =
        std::fs::read_to_string(repo.join(".github/triage/PLAYBOOK.md")).expect("the playbook");
    let template = std::fs::read_to_string(repo.join(".github/ISSUE_TEMPLATE/agent-filed.yml"))
        .expect("the template");

    let offered: Vec<String> = template
        .lines()
        .filter_map(|line| line.trim().strip_prefix("id: ").map(str::to_string))
        .collect();
    assert!(!offered.is_empty(), "the template names fields by id");

    // The list the playbook walks the agent through, one `- ` bullet each.
    let named: Vec<String> = playbook
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- `")?.split_once('`'))
        .map(|(field, _)| field.to_string())
        .filter(|field| !field.contains(' ') && !field.contains('/'))
        .collect();
    assert!(named.len() > 5, "the playbook names the fields: {named:?}");

    for field in named {
        assert!(
            offered.contains(&field),
            "the playbook tells an agent to fill `{field}`, which the template does not offer: {offered:?}"
        );
    }
}

/// The label the playbook applies has to be one the template already puts on
/// every report, or a maintainer filtering on it misses half of them.
#[test]
fn the_label_the_playbook_applies_is_one_the_template_carries() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let playbook =
        std::fs::read_to_string(repo.join(".github/triage/PLAYBOOK.md")).expect("the playbook");
    let template = std::fs::read_to_string(repo.join(".github/ISSUE_TEMPLATE/agent-filed.yml"))
        .expect("the template");

    assert!(playbook.contains("Label it `filed-by-agent`"));
    assert!(template.contains("  - filed-by-agent"));
}
