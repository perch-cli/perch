//! Behavior: what `perch triage` writes down, what it launches, and what it
//! declines to launch. The playbook and the template have to agree about the
//! fields, which is a correspondence rather than a behavior and is asserted in
//! `publication.rs` (ADR a-suite-is-named-and-gated).

mod common;

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use common::*;
use perch::commands::triage::TriageArgs;
use perch::error::EXIT_OK;
use perch::host::fake::Effect;
use perch::host::{FakeHost, Files, Platform, Refusing};
use perch::registry::Quarantine;

/// The moment every run below starts, so the directory it writes into is
/// knowable from the test rather than read back out of the output.
const AT: i64 = 1_787_059_012_431;

fn moment(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(millis).single().expect("a moment")
}

fn at_a_fixed_moment(host: FakeHost) -> FakeHost {
    host.with_now(moment(AT))
}

/// Asked of the same derivation the command uses rather than spelled out. A
/// `FakeHost` standing in for Windows joins with a backslash while its home is
/// still written the way the fixture wrote it, so a path spelled here matches on
/// one runner and not the other.
fn triage_dir(host: &FakeHost) -> PathBuf {
    perch::holdings::triage_dir(host).expect("Perch has a home")
}

fn run_dir(host: &FakeHost) -> PathBuf {
    perch::holdings::triage_run_dir(host, moment(AT)).expect("Perch has a home")
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

fn written(host: &FakeHost, name: &str) -> String {
    host.file(run_dir(host).join(name))
        .unwrap_or_else(|| panic!("{name} is written"))
}

/// The three files, by the names the command and the playbook share.
const PROMPT: &str = "prompt.md";
const RAW: &str = "probe.raw.txt";
const REDACTED: &str = "probe.txt";

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

    let prompt = written(&host, PROMPT);
    assert!(
        prompt.contains("Perch triage playbook"),
        "the playbook travels with the prompt: {prompt}"
    );
    assert!(
        prompt.contains(RAW) && prompt.contains(REDACTED),
        "and points at both readings: {prompt}"
    );

    let raw = written(&host, RAW);
    let redacted = written(&host, REDACTED);
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

/// The flag is for whoever is working on the Triage itself, so it reaches the
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

    assert_eq!(written(&host, REDACTED), written(&host, RAW));
    assert!(written(&host, REDACTED).contains(EMAIL));
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
        args[0].contains(&run_dir(&host).join(PROMPT).display().to_string()),
        "and it names the prompt: {args:?}"
    );
    assert!(
        !args[0].contains("Perch triage playbook\n"),
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
    for name in [PROMPT, RAW, REDACTED] {
        assert!(
            said.contains(&run_dir(&host).join(name).display().to_string()),
            "{name} is named so it can be pasted by hand: {said}"
        );
    }
}

/// Two platforms are in play at once: the runner's, and the one the fake claims.
/// Every path a Triage prints is derived on the second, and a case that spells
/// one out passes on one runner and not the other.
#[test]
fn the_paths_a_triage_prints_are_the_ones_it_wrote_on_the_platform_it_claims() {
    for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
        let host = at_a_fixed_moment(machine_with_two_accounts().with_platform(platform));
        quarantine_for(&host, EMAIL, Quarantine::RenewalRejected);

        let (_, said) = triaged(&host);

        for name in [PROMPT, RAW, REDACTED] {
            let at = run_dir(&host).join(name);
            assert!(
                host.file(&at).is_some(),
                "{platform:?}: {} is written",
                at.display()
            );
            assert!(
                said.contains(&at.display().to_string()),
                "{platform:?}: and named as it was written: {said}"
            );
        }
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
        host.file(run_dir(&host).join(PROMPT)).is_some(),
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
    assert!(said.contains(PROMPT), "{said}");
    assert!(
        said.contains("Paste prompt.md into any coding agent"),
        "the files are the fallback: {said}"
    );
}

/// A machine that has never Switched has no active Account for a Quarantine to
/// be recorded against, which withholds nothing: the live Credential is still
/// whatever Claude Code would find on its own.
#[test]
fn a_machine_on_nobody_is_launched_into_as_usual() {
    let host = at_a_fixed_moment(machine_with_two_accounts());
    let mut registry = registry_of(&host);
    registry.settle(None);
    save_registry(&host, &registry);

    let (_, said) = triaged(&host);

    assert!(launched(&host).is_some(), "{said}");
}

/// The only thing a Triage can fail at, and it names the file: everything before
/// the write is reading, and everything after is somebody else's session.
#[test]
fn evidence_that_cannot_be_written_is_a_refusal_naming_the_file() {
    let host = at_a_fixed_moment(machine_with_two_accounts());
    let at = run_dir(&host).join(REDACTED);
    let host = host.with_a_path_refusing(&at, Refusing::Write, "permission denied");

    host.forget_effects();
    let mut written = Vec::new();
    let refusal = perch::commands::triage::run(
        &host,
        TriageArgs {
            model: None,
            raw: false,
        },
        &mut written,
    )
    .expect_err("a Triage with nothing to hand over refuses");

    assert!(
        refusal.to_string().contains(&at.display().to_string()),
        "{refusal}"
    );
    assert!(launched(&host).is_none(), "and launches nothing");
}

/// Evidence rather than one of the Holdings: three runs is enough to hold the
/// one before a fix beside the one after it.
#[test]
fn only_the_newest_three_runs_are_kept() {
    let mut host = machine_with_two_accounts();
    let mut kept = Vec::new();
    for minute in 0..5 {
        let at = moment(AT + minute * 60_000);
        host = host.with_now(at);
        triaged(&host);
        kept.push(format!("run-{}", at.timestamp_millis()));
    }

    let held: Vec<String> = host
        .list_dir(&triage_dir(&host))
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

/// A Triage adopts nothing and saves nothing. On a machine Perch holds nothing
/// on, any other command would leave a registry behind, and the evidence is the
/// whole of what this one writes — no registry, and no line in the Trail.
#[test]
fn a_triage_adopts_nothing_and_writes_only_its_own_evidence() {
    let host = at_a_fixed_moment(logged_in_machine());

    triaged(&host);

    assert!(
        host.file(REGISTRY_PATH).is_none(),
        "a machine Perch held nothing on still holds nothing"
    );
    let wrote: Vec<PathBuf> = host
        .effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::WroteFile(at)
            | Effect::WrotePrivateFile(at)
            | Effect::AppendedPrivateLine(at) => Some(at),
            _ => None,
        })
        // A private write lands beside its target and is renamed over it, so
        // each one records the scratch name as well as the one asked for.
        .filter(|at| !at.to_string_lossy().contains(".perch-tmp."))
        .collect();
    let evidence: Vec<PathBuf> = [RAW, REDACTED, PROMPT]
        .iter()
        .map(|name| run_dir(&host).join(name))
        .collect();
    assert_eq!(wrote, evidence, "and nothing else is written");
}
