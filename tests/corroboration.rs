//! What makes a marker evidence, asserted against the real operating system
//! (ADR 0022).
//!
//! A Profile is Live because a process is still behind the marker that names
//! it, and the whole of that rests on one primitive: the operating system
//! answers "when did this process begin" for a process that is running and
//! declines for one that is gone. Perch believes that everywhere, and Windows'
//! PID recycling — the thing the corroboration was invented for — is exactly
//! where a canary belongs, so nothing here is platform-gated.
//!
//! Not in `conformance.rs`, which excludes processes from its table by charter:
//! they are *"the very things a fake exists to invent"*, and a fake clock or a
//! fake process that agreed with the real one would be no use to anybody. This
//! is the other side of that sentence — the answers the fake invents, checked
//! once against the machine that really gives them.
//!
//! Ungated, unlike `your_machine.rs`. Every process these read is one of their
//! own — this test, and a child they spawn and reap — and the only marker they
//! write goes into a directory of their own in `temp_dir`. Nothing here is the
//! developer's to consent to (ADR 0050).

use perch::host::RealHost;
use perch::host::prelude::*;
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
    host.create_file_with_mode(&marker, &probe::session_marker(me, host.now()), 0o600)
        .expect("the marker is written");

    let live = probe::live_clients(&host, &dir, &probe::Installed::unknown("this machine"))
        .expect("the marker names this very process, which is plainly alive");
    assert_eq!(
        live,
        vec![me],
        "a Run's own marker has to read as a Live Profile, or nothing is \
         protecting the session it launched"
    );

    host.remove_file(&marker).expect("the marker is taken away");
    assert!(
        probe::live_clients(&host, &dir, &probe::Installed::unknown("this machine"))
            .expect("an empty directory is no evidence")
            .is_empty(),
        "and the Profile stops being Live when the Run ends"
    );
    let _ = host.remove_dir_all(&dir);
}
