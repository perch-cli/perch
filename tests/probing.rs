//! Behavior: what `perch probe` says of a machine, and what the Trail records.

mod common;

use std::path::Path;

use chrono::{TimeZone, Utc};
use common::*;
use perch::commands::probe::ProbeArgs;
use perch::error::EXIT_OK;
use perch::host::{FakeHost, Files, Refusing};

const TRAIL: &str = "/Users/someone/.config/perch/trail.log";
const ROTATED: &str = "/Users/someone/.config/perch/trail.log.1";

fn run_probe(host: &FakeHost, args: ProbeArgs) -> (i32, String) {
    let mut written = Vec::new();
    let code = perch::commands::probe::run(host, args, &mut written).expect("a Probe answers");
    (code, String::from_utf8(written).expect("output is UTF-8"))
}

fn probed(host: &FakeHost) -> String {
    run_probe(
        host,
        ProbeArgs {
            json: false,
            raw: false,
        },
    )
    .1
}

fn probed_json(host: &FakeHost) -> serde_json::Value {
    let (_, said) = run_probe(
        host,
        ProbeArgs {
            json: true,
            raw: false,
        },
    );
    serde_json::from_str(&said).expect("the document parses")
}

/// A line as the Trail holds one, so a fixture can stage what only a killed
/// command or an older Perch would leave.
fn a_line(id: &str, at: &str, event: &str, tail: &str) -> String {
    format!(r#"{{"id":"{id}","at":"{at}","event":"{event}"{tail}}}"#)
}

#[test]
fn a_probe_says_which_perch_which_claude_code_and_what_it_holds() {
    let said = probed(&machine_with_two_accounts());

    assert!(said.contains(env!("CARGO_PKG_VERSION")), "{said}");
    assert!(said.contains(CLAUDE_VERSION), "{said}");
    assert!(said.contains("2 Accounts"), "{said}");
    assert!(said.contains("registry version"), "{said}");
}

#[test]
fn every_account_comes_out_as_its_place_in_the_registry() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &a_line(
            "one",
            "2026-08-04T11:59:00Z",
            "start",
            &format!(r#","words":["switch","{SECOND_EMAIL}"]"#),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(
        !said.contains(EMAIL) && !said.contains(SECOND_EMAIL),
        "an address reached the output a Probe exists to have pasted: {said}"
    );
    assert!(
        said.contains("<account 1>") && said.contains("<account 2>"),
        "{said}"
    );
}

/// The numbering is the registry's order rather than the order names are met,
/// which is what makes two Probes a week apart comparable.
#[test]
fn the_same_account_is_the_same_number_wherever_it_appears() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &a_line(
            "one",
            "2026-08-04T11:59:00Z",
            "start",
            &format!(r#","words":["switch","{EMAIL}"]"#),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);
    let first = said
        .lines()
        .filter(|line| line.contains("<account 1>"))
        .count();

    assert!(
        first >= 2,
        "the active Account and the Trail line naming it disagree: {said}"
    );
    assert!(!said.contains("<account 3>"), "{said}");
}

#[test]
fn raw_says_the_names_as_they_are() {
    let (code, said) = run_probe(
        &machine_with_two_accounts(),
        ProbeArgs {
            json: false,
            raw: true,
        },
    );

    assert_eq!(code, EXIT_OK);
    assert!(said.contains(EMAIL), "{said}");
    assert!(!said.contains("<account 1>"), "{said}");
}

#[test]
fn an_account_no_registry_still_holds_is_taken_out_all_the_same() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &a_line(
            "one",
            "2026-08-04T11:59:00Z",
            "start",
            r#","words":["remove","gone@example.com"]"#,
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(
        !said.contains("gone@example.com"),
        "nothing on the machine can number it, and it is still an address: {said}"
    );
    assert!(said.contains("no longer held"), "{said}");
}

#[test]
fn a_registry_that_will_not_load_is_a_finding_rather_than_a_refusal() {
    let host = logged_in_machine().with_file(REGISTRY_PATH, "{ not json");

    let (code, said) = run_probe(
        &host,
        ProbeArgs {
            json: false,
            raw: false,
        },
    );

    assert_eq!(code, EXIT_OK, "a Probe answers, whatever it found");
    assert!(said.contains("registry would not load"), "{said}");
}

#[test]
fn a_finding_carries_the_code_the_refusal_would_carry() {
    let host = logged_in_machine().with_file(REGISTRY_PATH, "{ not json");

    let document = probed_json(&host);
    let findings = document["findings"].as_array().expect("findings");

    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "registry-unreadable"
                && finding["exit_code"].is_number()),
        "{document:#}"
    );
}

#[test]
fn a_start_with_no_end_is_a_command_that_never_finished() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &format!(
            "{}\n{}\n",
            a_line(
                "hung",
                "2026-08-04T11:20:00Z",
                "start",
                r#","words":["switch"],"pid":4242"#
            ),
            a_line(
                "done",
                "2026-08-04T11:59:00Z",
                "start",
                r#","words":["list"]"#
            ),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(said.contains("never finished"), "{said}");
}

#[test]
fn a_line_this_perch_cannot_read_is_skipped_and_the_rest_are_kept() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &format!(
            "{}\nnot json at all\n{{\"event\":\"from a newer Perch\"}}\n{}\n",
            a_line(
                "one",
                "2026-08-04T11:59:00Z",
                "start",
                r#","words":["list"],"something_new":true"#
            ),
            a_line("one", "2026-08-04T11:59:01Z", "end", r#","exit_code":0"#),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(
        said.contains("list") && said.contains("exit 0"),
        "a line with a key this build has no field for is still a line: {said}"
    );
}

#[test]
fn the_window_widens_to_reach_the_last_failure() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &format!(
            "{}\n{}\n",
            a_line(
                "old",
                "2026-08-04T09:00:00Z",
                "start",
                r#","words":["switch","nobody"]"#
            ),
            a_line("old", "2026-08-04T09:00:01Z", "end", r#","exit_code":12"#),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(
        said.contains("exit 12"),
        "three hours is outside the hour, and it is the last thing that failed: {said}"
    );
}

#[test]
fn a_probe_adds_no_line_of_its_own() {
    let host = machine_with_two_accounts();

    let _ = probed(&host);

    assert!(
        host.read_file(Path::new(TRAIL)).is_err(),
        "a Probe that writes one pushes out what somebody wanted to read"
    );
}

#[test]
fn a_command_writes_a_line_when_it_starts_and_another_when_it_ends() {
    let host = machine_with_two_accounts();

    let invocation = perch::trail::began(&host, &["switch".to_string(), "work".to_string()]);
    perch::trail::ended(&host, &invocation, 17);

    let held = host
        .read_file(Path::new(TRAIL))
        .expect("the Trail is there");
    let lines: Vec<&str> = held.lines().collect();

    assert_eq!(lines.len(), 2, "{held}");
    assert!(lines[0].contains(r#""event":"start""#), "{held}");
    assert!(lines[1].contains(r#""exit_code":17"#), "{held}");
}

/// `perch run <target> -- claude -p "..."` hands the rest to the client, and
/// what somebody types at Claude Code is theirs.
#[test]
fn what_goes_to_the_client_is_counted_and_not_written_down() {
    let host = machine_with_two_accounts();

    let typed = ["run", "work", "--", "claude", "-p", "something private"]
        .map(str::to_string)
        .to_vec();
    perch::trail::began(&host, &typed);

    let held = host
        .read_file(Path::new(TRAIL))
        .expect("the Trail is there");

    assert!(!held.contains("something private"), "{held}");
    assert!(held.contains(r#""passed_on":3"#), "{held}");
}

#[test]
fn a_trail_that_cannot_be_written_costs_a_line_and_nothing_else() {
    let host = machine_with_two_accounts().with_a_path_refusing(
        TRAIL,
        Refusing::Write,
        "Read-only file system (os error 30)",
    );

    let invocation = perch::trail::began(&host, &["list".to_string()]);
    perch::trail::ended(&host, &invocation, 0);

    let (code, said) = run_probe(
        &host,
        ProbeArgs {
            json: false,
            raw: false,
        },
    );
    assert_eq!(code, EXIT_OK, "and the Probe still answers");
    assert!(said.contains("Trail holds nothing"), "{said}");
}

#[test]
fn the_live_file_is_moved_aside_once_it_passes_the_cap() {
    let host = machine_with_two_accounts();

    // Past a megabyte, one invocation at a time, which is how it fills.
    for _ in 0..12_000 {
        let invocation = perch::trail::began(&host, &["status".to_string()]);
        perch::trail::ended(&host, &invocation, 0);
    }

    let rotated = host
        .read_file(Path::new(ROTATED))
        .expect("what the cap moved aside");
    let live = host
        .read_file(Path::new(TRAIL))
        .expect("and what took over");

    assert!(rotated.len() >= 1024 * 1024, "{}", rotated.len());
    assert!(
        live.len() < rotated.len(),
        "the live file starts again rather than carrying on"
    );
    assert!(
        probed(&host).contains("Trail"),
        "and both files are still read"
    );
}

/// The rule a Purge depends on: a machine Perch holds nothing on has nothing to
/// record, and one just emptied must not find a directory behind it.
#[test]
fn nothing_is_written_where_perch_has_no_home() {
    let host = machine_with_claude_code();

    let invocation = perch::trail::began(&host, &["status".to_string()]);
    perch::trail::ended(&host, &invocation, 0);

    assert!(
        host.read_file(Path::new(TRAIL)).is_err(),
        "a Trail line made the home it lives in"
    );
}

/// An entry is two lines or none. The command that creates Perch's home is the
/// one that would otherwise leave an end with no beginning.
#[test]
fn a_start_that_did_not_land_writes_no_end_either() {
    let host = machine_with_claude_code();

    let invocation = perch::trail::began(&host, &["add".to_string()]);
    host.set_file("/Users/someone/.config/perch/registry.json", "{}");
    perch::trail::ended(&host, &invocation, 0);

    assert!(host.read_file(Path::new(TRAIL)).is_err());
}

/// `perch watcher run` is a loop that runs until the session ends, so on every
/// machine with a Service one start line is unpaired at all times.
#[test]
fn a_start_whose_process_is_still_alive_is_running_rather_than_lost() {
    let host = machine_with_two_accounts().with_live_process(4242);
    host.set_file(
        TRAIL,
        &a_line(
            "watching",
            "2026-08-04T09:00:00Z",
            "start",
            r#","words":["watcher","run"],"pid":4242"#,
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(
        !said.contains("never finished"),
        "the Watcher in its loop is reported as a failure on every machine: {said}"
    );
}

/// The pid alone is not enough: the machine hands a number to somebody else.
#[test]
fn a_pid_the_machine_gave_to_something_newer_is_not_the_command_that_wrote_it() {
    let host = machine_with_two_accounts()
        .with_live_process_started_at(4242, Utc.with_ymd_and_hms(2026, 8, 4, 11, 50, 0).unwrap());
    host.set_file(
        TRAIL,
        &a_line(
            "gone",
            "2026-08-04T11:40:00Z",
            "start",
            r#","words":["switch"],"pid":4242"#,
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    assert!(probed(&host).contains("never finished"));
}

/// The Watcher's rounds reach the Trail, which is what lets a Probe say what it
/// decided on Linux without reading the journal.
#[test]
fn a_watcher_that_moved_something_writes_a_line_and_one_that_looked_does_not() {
    let host = machine_with_two_accounts();

    assert!(
        perch::watch::Outcome::Waiting.what_it_moved().is_none(),
        "a round that looked and did nothing is not an event"
    );
    assert!(
        perch::watch::Outcome::Refused {
            why: "a client is running".to_string(),
            after_reading: false,
            contended: true,
        }
        .what_it_moved()
        .is_none(),
        "a round turned away recurs every interval and would flood the file"
    );
    let moved = perch::watch::Outcome::Switched {
        to: SECOND_EMAIL.to_string(),
        unread: Vec::new(),
    }
    .what_it_moved()
    .expect("a Switch is");
    perch::trail::acted(&host, &moved);

    let said = probed(&host);

    assert!(said.contains("watcher switched to <account 2>"), "{said}");
    assert!(
        !said.contains("no end line"),
        "a round has nothing to end: {said}"
    );
}

#[test]
fn the_probe_names_the_watchers_own_log_and_does_not_read_it() {
    let host = machine_with_two_accounts();
    // A unit is what makes a Service installed, and a Service is what makes a
    // log worth naming.
    host.set_file(
        "/Users/someone/Library/LaunchAgents/cli.perch.watch.plist",
        "<plist/>",
    );
    host.set_file(
        "/Users/someone/.config/perch/watch.log",
        "a decision nobody reads here",
    );

    let said = probed(&host);

    assert!(said.contains("Its log"), "{said}");
    assert!(
        !said.contains("a decision nobody reads here"),
        "reaching the log means a subprocess three ways: {said}"
    );
}

/// The one reading that catches the write Perch makes in silence: every command
/// writes the Trail, so a registry newer than its last line means one could not.
#[test]
fn a_registry_written_after_the_trails_last_line_says_the_trail_is_not_kept() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &a_line(
            "old",
            "2026-08-04T09:00:00Z",
            "start",
            r#","words":["list"]"#,
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());
    // Touched last, so the registry is the newer of the two.
    host.set_file(
        REGISTRY_PATH,
        &host
            .read_file(Path::new(REGISTRY_PATH))
            .expect("a registry"),
    );

    let said = probed(&host);

    assert!(said.contains("wrote nothing down"), "{said}");
}

/// A Landing is what a killed Switch leaves, and it has to be visible to both
/// renderings rather than to the one a person happens to read.
#[test]
fn a_landing_is_named_in_the_text_and_in_the_json_alike() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        &format!(
            r#"{{"version":{},"active":{{"landing":{{"leaving":"{EMAIL}","arriving":"{SECOND_EMAIL}"}}}},
                "accounts":[{{"identity":{{"email":"{EMAIL}"}}}},{{"identity":{{"email":"{SECOND_EMAIL}"}}}}]}}"#,
            perch::registry::CURRENT_VERSION
        ),
    );

    let said = probed(&host);
    let document = probed_json(&host);

    assert!(
        said.contains("a Landing from <account 1> to <account 2>"),
        "{said}"
    );
    assert_eq!(
        document["holdings"]["active"], "a Landing from <account 1> to <account 2>",
        "{document:#}"
    );
}

/// The judgment first, and the facts under it standing on their own.
#[test]
fn the_findings_come_before_the_facts() {
    let said = probed(&machine_with_two_accounts());
    let findings = said.find("Findings").expect("a Findings section");
    let facts = said.find("Perch  ").expect("the facts");

    assert!(findings < facts, "{said}");
}

/// A machine going down leaves a start nothing ever ends. Reported forever, one
/// per reboot, it would bury the command somebody is filing about.
#[test]
fn a_start_a_reboot_left_behind_falls_out_of_the_window_with_everything_else() {
    let host = machine_with_two_accounts();
    host.set_file(
        TRAIL,
        &format!(
            "{}\n{}\n",
            a_line(
                "killed",
                "2026-08-01T09:00:00Z",
                "start",
                r#","words":["watcher","run"],"pid":4242"#
            ),
            a_line(
                "since",
                "2026-08-04T11:59:00Z",
                "start",
                r#","words":["list"]"#
            ),
        ),
    );
    let host = host.with_now(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

    let said = probed(&host);

    assert!(!said.contains("never finished"), "{said}");
}

/// `registry::load` carries an old registry forward in memory, so the loaded
/// value says every machine is current. What the file states is the finding.
#[test]
fn a_registry_no_command_has_carried_forward_is_said_at_the_version_it_states() {
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        &format!(r#"{{"version":4,"accounts":[{{"identity":{{"email":"{EMAIL}"}}}}]}}"#),
    );

    let said = probed(&host);
    let document = probed_json(&host);

    assert!(said.contains("registry version 4"), "{said}");
    assert!(said.contains("No command has brought it forward"), "{said}");
    assert_eq!(document["home"]["registry_version"], 4, "{document:#}");
}
