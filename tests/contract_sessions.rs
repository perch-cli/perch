//! Contract tests for session markers (ADR 0022), asserted against the markers
//! the installed Claude Code has actually written and against real processes.
//!
//! These are cross-platform: the marker shape and the "when did this process
//! start" primitive are beliefs Perch holds everywhere, and the Windows PID
//! recycling that motivated them is exactly where a canary belongs. On a
//! machine where no client has run — a CI runner, for instance — each test
//! asserts what it still can and says what it skipped.

use perch::host::{Host, RealHost};
use perch::probe;

/// The primitive the corroboration stands on: the operating system answers
/// "when did this process begin" for a process that is running, and declines
/// for one that is gone. Asserted against the one process guaranteed to be
/// running — this test.
#[test]
fn the_operating_system_says_when_a_running_process_began() {
    let host = RealHost::new();
    let me = std::process::id();

    let began = host
        .process_started_at(me)
        .expect("this test is running, so its start can be read");
    assert!(
        began <= host.now(),
        "a process cannot begin after the clock: began {began}, now {}",
        host.now()
    );
    assert!(
        (host.now() - began).num_hours() < 24,
        "this test began moments ago, not {began}"
    );
}

#[test]
fn a_process_that_has_exited_has_no_start_to_read() {
    let host = RealHost::new();
    let mut child = std::process::Command::new(env!("CARGO"))
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("a child of our own");
    let pid = child.id();
    child.wait().expect("and it exits");

    assert_eq!(
        host.process_started_at(pid),
        None,
        "a reaped process is gone, which is what makes a leftover marker no \
         evidence of a client"
    );
}

/// The marker a Run writes, held to the same standard as a client's (ADR 0027).
///
/// A Run marks its Profile Live by naming its own process, and the whole of
/// what makes that evidence is that the process began before the marker says
/// the session did. Asserted here rather than against a fake, because the two
/// halves come from different places — Perch's clock and the operating system's
/// answer for a process — and a machine where they disagree is one where a Run
/// silently protects nothing.
#[test]
fn the_marker_a_run_writes_corroborates_itself_on_this_machine() {
    let host = RealHost::new();
    let dir = std::env::temp_dir().join(format!("perch-live-{}", std::process::id()));
    host.create_dir_all(&probe::sessions_dir(&dir))
        .expect("a directory of our own");

    let me = host.process_id();
    assert_eq!(me, std::process::id());
    let marker = probe::session_marker_at(&dir, me);
    host.write_file(&marker, &probe::session_marker(me, host.now()))
        .expect("the marker is written");

    let live = probe::live_clients(&host, &dir, "contract")
        .expect("the marker names this very process, which is plainly alive");
    assert_eq!(
        live,
        vec![me],
        "a Run's own marker has to read as a Live Profile, or nothing is \
         protecting the session it launched"
    );

    host.remove_file(&marker).expect("the marker is taken away");
    assert!(
        probe::live_clients(&host, &dir, "contract")
            .expect("an empty directory is no evidence")
            .is_empty(),
        "and the Profile stops being Live when the Run ends"
    );
    let _ = host.remove_dir_all(&dir);
}

/// The marker's shape, asserted against a client that is actually running: it
/// names its process in its filename and in its body, and `startedAt` is when
/// the session began, in milliseconds since the epoch. If Claude Code stops
/// writing markers this way, every Profile on the machine reads as idle and
/// the refusal that protects a running client stops firing — this is the test
/// that should say so before a user does.
#[test]
fn a_running_clients_marker_is_the_shape_perch_believes_in() {
    let host = RealHost::new();
    let Ok(store) = probe::default_store(&host) else {
        return;
    };
    let sessions = probe::sessions_dir(&store.config_dir);
    let Ok(markers) = host.list_dir(&sessions) else {
        eprintln!("skipping: {} does not exist", sessions.display());
        return;
    };

    let corroborated = probe::live_clients(&host, &store.config_dir, "contract")
        .expect("every marker here can be corroborated or dismissed");

    let mut live = 0;
    for marker in &markers {
        let Some(pid) = marker
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(process_began) = host.process_started_at(pid) else {
            continue; // A marker left behind by a client that died.
        };
        live += 1;

        let contents = host.read_file(marker).expect("a live marker is readable");
        let recorded: serde_json::Value =
            serde_json::from_str(&contents).expect("a live marker is JSON");
        assert_eq!(
            recorded.get("pid").and_then(serde_json::Value::as_u64),
            Some(u64::from(pid)),
            "a marker names its process in its body as well as its filename"
        );

        let session_began = recorded
            .get("startedAt")
            .and_then(serde_json::Value::as_i64)
            .expect("a marker says when its session began");
        assert!(
            (1_500_000_000_000..3_000_000_000_000).contains(&session_began),
            "startedAt is epoch milliseconds, not {session_began}"
        );

        // The belief the whole of ADR 0022 turns on, held against a real
        // client: a genuine session's process began no later than the marker
        // says the session did. If the two clocks drift apart, running clients
        // read as recycled PIDs and every Live Profile protection stops firing.
        assert!(
            process_began.timestamp_millis() <= session_began,
            "{}: the process began at {} but the session claims {session_began} — \
             a genuine client must corroborate its own marker",
            marker.display(),
            process_began.timestamp_millis()
        );
        assert!(
            corroborated.contains(&pid),
            "{} is corroborated, so live_clients must report it",
            marker.display()
        );
    }

    if live == 0 {
        eprintln!(
            "skipping the live half: no client is running against {}",
            store.config_dir.display()
        );
    }
}
