//! Contract tests for the links Reconcile shares by (ADR 0026), asserted
//! against the filesystem this machine actually has.
//!
//! The behaviour tests pin what Perch *decides* — junctions for directories on
//! Windows, hard links where a symbolic one is refused — from any machine. What
//! they cannot pin is that those links then behave the way the decision assumes
//! they do, which is the whole argument for refusing to copy. That is what this
//! suite is: one assertion per property the design leans on, run on every
//! platform CI has.
//!
//! Cross-platform by construction. Where a link cannot be made here at all — a
//! Windows without Developer Mode, for the symbolic ones — the test says what
//! it skipped rather than failing, exactly as the design says Perch itself
//! degrades there.

use std::path::{Path, PathBuf};

use perch::host::{Host, HostError, Link, RealHost};

/// A directory of this test's own, on the real filesystem.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perch-links-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Whether this machine will make a symbolic link at all. A Windows without
/// Developer Mode will not, which is the case the other two kinds exist for.
fn made(outcome: Result<(), HostError>, kind: Link) -> bool {
    match outcome {
        Ok(()) => true,
        Err(err) => {
            eprintln!("skipping: {} cannot be made here: {err}", kind.describe());
            false
        }
    }
}

/// The property every share rests on: what is read through the link is what the
/// Default Profile holds *now*, not what it held when the link was made. A copy
/// is the thing that fails this.
#[test]
fn a_symbolic_link_to_a_file_reads_what_the_file_holds_now() {
    let dir = scratch("symlink-file");
    let host = RealHost::new();
    let target = dir.join("CLAUDE.md");
    let at = dir.join("shared-CLAUDE.md");
    std::fs::write(&target, "remember this").unwrap();

    if !made(host.link(Link::Symbolic, &target, &at), Link::Symbolic) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    std::fs::write(&target, "remember this instead").unwrap();

    let through_the_link = std::fs::read_to_string(&at);
    let reported = host.link_target(&at);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(through_the_link.unwrap(), "remember this instead");
    assert_eq!(reported.unwrap().as_deref(), Some(target.as_path()));
}

/// The same for a directory, and the part an allowlist would have got wrong: a
/// file that appears in the Default Profile after the link was made is reachable
/// through it without anything being told.
#[test]
fn a_directory_link_shows_what_the_directory_gains_afterwards() {
    let dir = scratch("dir-link");
    let host = RealHost::new();
    let target = dir.join("plugins");
    let at = dir.join("shared-plugins");
    std::fs::create_dir_all(&target).unwrap();

    // What Reconcile itself would choose for a directory on this platform.
    let kind = if cfg!(windows) {
        Link::Junction
    } else {
        Link::Symbolic
    };
    if !made(host.link(kind, &target, &at), kind) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    std::fs::write(target.join("config.json"), "{}").unwrap();

    let through_the_link = std::fs::read_to_string(at.join("config.json"));
    let reported = host.link_target(&at);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(through_the_link.unwrap(), "{}");
    assert!(
        reported.expect("the link answers for itself").is_some(),
        "a {} reads back as a link, which is how a stale one is found",
        kind.describe()
    );
}

/// The fallback Windows leans on, and the reason a hard link is re-established
/// before every Run rather than trusted: it is a name for a *file*, so a file
/// written through is shared and a file *replaced* is not. Both halves asserted
/// together, because the second is only a hazard given the first.
#[test]
fn a_hard_link_shares_a_written_file_and_stops_sharing_a_replaced_one() {
    let dir = scratch("hard-link");
    let host = RealHost::new();
    let target = dir.join("settings.json");
    let at = dir.join("shared-settings.json");
    std::fs::write(&target, "first").unwrap();

    host.link(Link::Hard, &target, &at)
        .expect("a hard link needs no privilege on any platform Perch runs on");

    std::fs::write(&target, "second").unwrap();
    let written_through = std::fs::read_to_string(&at).unwrap();

    // What an editor does when it saves: write beside, rename over. The name
    // survives; the file behind it does not.
    let beside = dir.join("settings.json.new");
    std::fs::write(&beside, "third").unwrap();
    std::fs::rename(&beside, &target).unwrap();
    let after_replacement = std::fs::read_to_string(&at).unwrap();

    let reported = host.link_target(&at);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(written_through, "second", "a hard link is the same file");
    assert_eq!(
        after_replacement, "second",
        "a replaced file leaves the hard link naming the old one — which is why \
         Reconcile makes it again every pass rather than leaving it"
    );
    assert_eq!(
        reported.unwrap(),
        None,
        "a hard link cannot say it is one, which is the other half of the same \
         reason"
    );
}

/// Removing a share takes the link and leaves what it pointed at, whatever kind
/// it is — a repair that took the Default Profile's directory with it would be
/// the worst failure in this design.
#[test]
fn removing_a_link_leaves_what_it_pointed_at() {
    let dir = scratch("remove-link");
    let host = RealHost::new();
    let target = dir.join("plans");
    let at = dir.join("shared-plans");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("one.md"), "a plan").unwrap();

    let kind = if cfg!(windows) {
        Link::Junction
    } else {
        Link::Symbolic
    };
    if !made(host.link(kind, &target, &at), kind) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    host.remove_link(&at).expect("a link is removed");

    let link_gone = !Path::new(&at).exists() && host.link_target(&at).is_err();
    let kept = std::fs::read_to_string(target.join("one.md"));
    let _ = std::fs::remove_dir_all(&dir);

    assert!(link_gone, "the link is gone");
    assert_eq!(
        kept.unwrap(),
        "a plan",
        "and everything it pointed at is not"
    );
}

/// The three answers Reconcile branches on have to stay three on a real
/// filesystem: a link, something that is not a link, and nothing at all.
#[test]
fn a_path_that_is_not_a_link_and_a_path_that_is_nothing_are_different_answers() {
    let dir = scratch("link-target");
    let host = RealHost::new();
    let file = dir.join("settings.json");
    std::fs::write(&file, "{}").unwrap();

    let plain = host.link_target(&file);
    let absent = host.link_target(&dir.join("never-existed"));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(plain.unwrap(), None, "a file is not a link");
    assert!(
        matches!(absent, Err(HostError::NotFound { .. })),
        "nothing there is not the same as something that is not a link"
    );
}
