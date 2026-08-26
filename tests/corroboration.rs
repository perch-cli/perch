//! What makes a marker evidence, asserted against the real operating system
//! (ADR a-profile-is-live-by-evidence).
//!
//! Corroboration rests on one primitive: the operating system answers "when did
//! this process begin" for a process that is running and declines for one that
//! is gone. Perch believes that everywhere, and Windows' PID recycling is
//! exactly where a canary belongs, so nothing here is platform-gated. Not in
//! `conformance.rs`, which excludes processes from its table by charter. Ungated,
//! because every process read here is one of these tests' own and the only
//! marker written goes into `temp_dir` (ADR a-suite-is-named-and-gated).

use perch::host::RealHost;
use perch::host::prelude::*;
use perch::live;
use perch::probe;

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

#[test]
fn the_marker_a_run_writes_corroborates_itself_on_this_machine() {
    let host = RealHost::new();
    let dir = std::env::temp_dir().join(format!("perch-live-{}", std::process::id()));
    host.create_dir_all(&probe::sessions_dir(&dir))
        .expect("a directory of our own");

    let me = host.process_id();
    assert_eq!(me, std::process::id());
    let marker = probe::session_marker_at(&dir, me);
    host.create_file_with_mode(&marker, &probe::session_marker(me, host.now()), 0o600)
        .expect("the marker is written");

    let running = pids(live::ask(&host, &[live::Place::at(&dir)]));
    assert_eq!(
        running,
        vec![me],
        "a Run's own marker has to read as a Live Profile, or nothing is \
         protecting the session it launched"
    );

    host.remove_file(&marker).expect("the marker is taken away");
    assert!(
        pids(live::ask(&host, &[live::Place::at(&dir)])).is_empty(),
        "and the Profile stops being Live when the Run ends"
    );
    let _ = host.remove_dir_all(&dir);
}

/// The corroborated pids, where the ask came back Idle or Live. A doubt is a
/// failure of this suite's own fixture rather than an answer about it.
fn pids(answer: live::Answer) -> Vec<u32> {
    match answer {
        live::Answer::Idle(_) => Vec::new(),
        live::Answer::NotIdle(live::NotIdle::Live(clients)) => {
            clients.iter().map(|client| client.pid).collect()
        }
        live::Answer::NotIdle(live::NotIdle::Unsure(unsure)) => {
            panic!(
                "{}",
                unsure.refusal(&probe::Installed::unknown("this machine"))
            )
        }
    }
}
