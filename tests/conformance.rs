//! Conformance: the two adapters at the Host port, asked the same questions.
//!
//! The port's interface is not its signatures but the sentences on them — an
//! absent directory reports `NotFound` rather than emptiness, `link_target`
//! answers for the link and not for what it points at, `create_dir_exclusive`
//! never succeeds quietly. A sentence with two implementations and no reader is
//! one they drift apart on, so the table asks both, and its shape is what lets
//! a failure name which adapter and which sentence.
//!
//! Ungated, unlike `your_machine.rs` (ADR a-suite-is-named-and-gated).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use perch::host::{
    Clock, FakeHost, Files, Filesystem, Host, HostError, Link, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE,
    Platform, RealHost, Waited,
};

/// This machine, as the port names it, so the fake is asked to be the platform
/// the real host is already on. `RealHost` gates on `#[cfg]` and the fake on
/// this value, so a fake left at its default claims macOS permissions on a
/// Windows runner and any agreement between the two is a coincidence.
fn this_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Other
    }
}

/// Whether permission bits mean anything here, which is the one thing the two
/// adapters are *allowed* to differ about — and only because the port says so:
/// `file_mode` answers `None` on a platform that does not answer in those terms.
fn modes_mean_something() -> bool {
    cfg!(unix)
}

/// A directory of this case's own on the real filesystem, named after the case
/// so a failure says which one left it behind.
fn scratch(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perch-conformance-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Which links this machine will make, asked of the real filesystem: a Windows
/// without Developer Mode makes no symbolic link, and that is a fact about the
/// *machine* rather than about either adapter. The fake's knob for it defaults
/// to off, so left alone each adapter skips a different set and the suite
/// reports an agreement it never checked.
fn links_this_machine_makes() -> &'static [Link] {
    // Once: the answer belongs to the machine rather than to either test, and
    // the two run in parallel over one scratch directory.
    static MADE: std::sync::OnceLock<Vec<Link>> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        let host = RealHost::new();
        let root = scratch("what-links-this-machine-makes");
        let made = [Link::Symbolic, Link::Junction, Link::Hard]
            .into_iter()
            .filter(|kind| can_link(&host, *kind, &root, "the machine"))
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        made
    })
}

/// Whether this machine will make a link of this kind at all. Said out loud,
/// because a case that skipped itself and a case that asserted must not look
/// identical.
fn can_link(host: &dyn Filesystem, kind: Link, root: &Path, adapter: &str) -> bool {
    let target = root.join("can-link-target");
    let at = root.join("can-link-at");
    host.create_file_with_mode(&target, "x", PRIVATE_FILE_MODE)
        .expect("a file to point at");
    match host.link(kind, &target, &at) {
        Ok(()) => {
            // By kind, because `remove_link` refuses a hard link on every
            // adapter — a swallowed `remove_link` here is how that disagreement
            // stayed invisible while the suite performed it on both.
            match kind {
                Link::Hard => {
                    let _ = host.remove_file(&at);
                }
                _ => {
                    let _ = host.remove_link(&at);
                }
            }
            let _ = host.remove_file(&target);
            true
        }
        Err(err) => {
            eprintln!(
                "skipping ({adapter}): {} cannot be made here: {err}",
                kind.describe()
            );
            let _ = host.remove_file(&target);
            false
        }
    }
}

struct Case {
    /// What sentence on the port this asserts. Named rather than numbered,
    /// because the useful failure is "the fake fails this one and the real host
    /// does not".
    named: &'static str,
    /// The adapter's own `now`, handed in because a case holds a
    /// `&dyn Filesystem` and cannot reach the concrete clock
    /// (ADR the-port-fits-the-machine). `lock.rs` reads staleness as
    /// `now() - modified_at(artifact)`, so an adapter whose clock and whose
    /// mtimes disagree answers both questions plausibly and breaks every rule.
    asserts: fn(&dyn Filesystem, &Path, &str, DateTime<Utc>),
}

const CASES: &[Case] = &[
    // ---- reading and writing --------------------------------------------
    Case {
        named: "a file that is not there reports NotFound",
        asserts: |host, root, adapter, _now| {
            let missing = root.join("nothing-here");
            match host.read_file(&missing) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
        },
    },
    Case {
        named: "reading a directory is an error that is not NotFound",
        asserts: |host, root, adapter, _now| {
            let at = root.join("a-directory");
            host.create_private_dir_all(&at)
                .expect("the directory is made");
            // Not `NotFound`, because that answer is load-bearing: a
            // `CredentialStore` reads it as "this store holds nothing".
            match host.read_file(&at) {
                Err(HostError::NotFound { .. }) => {
                    panic!("{adapter}: a directory read as an empty store")
                }
                Err(_) => {}
                Ok(held) => panic!("{adapter}: a directory read as {held:?}"),
            }
        },
    },
    Case {
        named: "a write under a path a file occupies is refused",
        asserts: |host, root, adapter, _now| {
            let occupied = root.join("not-a-directory");
            host.create_file_with_mode(&occupied, "a file", PRIVATE_FILE_MODE)
                .expect("the file is written");
            // Every private write goes through `replace_via_tmp`, which creates
            // the parent — and a parent that is a regular file is `ENOTDIR`.
            let under = occupied.join("child");
            if let Ok(()) = host.create_file_with_mode(&under, "x", PRIVATE_FILE_MODE) {
                panic!("{adapter}: a directory was invented over a file");
            }
        },
    },
    Case {
        named: "a read follows a symbolic link",
        asserts: |host, root, adapter, _now| {
            let real = root.join("behind-the-link");
            let link = root.join("the-link");
            host.create_file_with_mode(&real, "what it holds", PRIVATE_FILE_MODE)
                .expect("the file is written");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link)
                .expect("the link is made");

            assert_eq!(
                host.read_file(&link).ok().as_deref(),
                Some("what it holds"),
                "{adapter}: a read through a link reads what the link names"
            );
            assert!(
                host.path_exists(&link),
                "{adapter}: and the link is there to be found"
            );
        },
    },
    Case {
        named: "a read follows a symbolic link whose target is relative",
        asserts: |host, root, adapter, _now| {
            let real = root.join("beside-the-link");
            let link = root.join("relative-link");
            host.create_file_with_mode(&real, "what it holds", PRIVATE_FILE_MODE)
                .expect("the file is written");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            // What `ln -s`, stow and chezmoi all record unless handed an
            // absolute path, so a managed `~/.claude.json` is one of these.
            host.link(Link::Symbolic, Path::new("beside-the-link"), &link)
                .expect("the link is made");

            assert_eq!(
                host.read_file(&link).ok().as_deref(),
                Some("what it holds"),
                "{adapter}: a relative target is resolved against where the link sits"
            );
            assert!(
                host.path_exists(&link),
                "{adapter}: and the link is there to be found"
            );
        },
    },
    Case {
        named: "a file that is there has a modification time",
        asserts: |host, root, adapter, _now| {
            let written = root.join("has-an-age");
            host.create_file_with_mode(&written, "anything", PRIVATE_FILE_MODE)
                .expect("the file is written");

            assert!(
                host.modified_at(&written).is_ok(),
                "{adapter}: `carry` ranks Profiles on this, and every staleness \
                 question in `lock` is `now() - modified_at`",
            );
        },
    },
    // Both are about a lock artifact that is a dangling link: the state
    // `reconcile`'s denylist keeps Perch out of and `lock::clear_the_abandoned`
    // has to be able to meet anyway.
    Case {
        named: "asking when a dangling link was written is not an answer",
        asserts: |host, root, adapter, _now| {
            let link = root.join("points-nowhere");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &root.join("was-never-there"), &link)
                .expect("a link to nothing is still a link");

            assert!(
                matches!(host.modified_at(&link), Err(HostError::NotFound { .. })),
                "{adapter}: a dangling link has no modification time — following \
                 it is what `metadata` does, and there is nothing at the end of \
                 it. This is the arm that reads a lock as free.",
            );
        },
    },
    Case {
        named: "removing a directory tree at a link takes the link and not what it points at",
        asserts: |host, root, adapter, _now| {
            let real = root.join("a-real-directory");
            let held = real.join("held");
            host.create_dir_all(&real).expect("it is made");
            host.create_file_with_mode(&held, "still here", PRIVATE_FILE_MODE)
                .expect("with something in it");

            let link = root.join("points-at-it");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link).expect("linked");

            host.remove_dir_all(&link)
                .expect("the last component is not followed, so the link goes");

            assert!(
                host.read_file(&held).is_ok(),
                "{adapter}: and what it pointed at is untouched"
            );
            assert!(
                matches!(host.link_target(&link), Err(HostError::NotFound { .. })),
                "{adapter}: while the link itself is gone"
            );
        },
    },
    Case {
        named: "a file is created with exactly the mode asked for",
        asserts: |host, root, adapter, _now| {
            let path = root.join("narrow");
            host.create_file_with_mode(&path, "secret", 0o600)
                .expect("it is created");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("secret"));
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&path).ok().flatten(),
                    Some(0o600),
                    "{adapter}: the mode is the creation's, not the umask's"
                );
            }
        },
    },
    // Bits every ordinary umask strips, unlike the case above, so this can
    // only pass where the mode is the creation's — which `write_atomically`
    // rests on, reading a target's mode and writing the replacement with it.
    Case {
        named: "the mode asked for survives a umask that would have taken bits out of it",
        asserts: |host, root, adapter, _now| {
            let path = root.join("wide");
            host.create_file_with_mode(&path, "shared", 0o666)
                .expect("it is created");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("shared"));
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&path).ok().flatten(),
                    Some(0o666),
                    "{adapter}: the mode is the one asked for, not the one the \
                     shell that launched this happened to leave"
                );
            }
        },
    },
    Case {
        named: "creating over an existing file takes the new mode",
        asserts: |host, root, adapter, _now| {
            let path = root.join("was-open");
            host.create_file_with_mode(&path, "first", 0o644)
                .expect("it is created");
            host.create_file_with_mode(&path, "second", 0o600)
                .expect("it is replaced");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("second"));
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&path).ok().flatten(),
                    Some(0o600),
                    "{adapter}: what was there is replaced rather than written into"
                );
            }
        },
    },
    Case {
        named: "a private write creates the file and its directory closed",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("private-dir");
            let path = dir.join("credential");
            host.write_private_file(&path, "a secret")
                .expect("it is written");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("a secret"));
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&path).ok().flatten(),
                    Some(PRIVATE_FILE_MODE),
                    "{adapter}: the owner and nobody else (ADR claude-code-chooses-the-store)"
                );
                // A Credential at 0600 inside a directory anybody may list is
                // one whose name, size and mtime are public — and the directory
                // here is one this very call made.
                assert_eq!(
                    host.file_mode(&dir).ok().flatten(),
                    Some(PRIVATE_DIR_MODE),
                    "{adapter}: and the directory it had to make on the way"
                );
            }
        },
    },
    // The two steps have different owners: a Profile's directory is made by one
    // module and its Credential written by another, so a directory otherwise
    // wears the mode of whichever got there first.
    Case {
        named: "a write into a directory already made private leaves it private",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("a-profile");
            host.create_private_dir_all(&dir).expect("it is made");
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&dir).ok().flatten(),
                    Some(PRIVATE_DIR_MODE),
                    "{adapter}: closed from the moment it exists"
                );
            }

            host.write_private_file(&dir.join("credential"), "a secret")
                .expect("the Credential goes in");

            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&dir).ok().flatten(),
                    Some(PRIVATE_DIR_MODE),
                    "{adapter}: and the second writer does not widen it"
                );
            }
        },
    },
    Case {
        named: "a private write leaves nothing beside the file",
        asserts: |host, root, adapter, _now| {
            let path = root.join("replaced");
            host.write_private_file(&path, "first")
                .expect("it is written");
            host.write_private_file(&path, "second")
                .expect("it is replaced");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("second"));
            let left = host
                .list_dir(root)
                .expect("the directory is there")
                .into_iter()
                .filter(|found| {
                    found
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("perch-tmp"))
                })
                .collect::<Vec<_>>();
            assert!(
                left.is_empty(),
                "{adapter}: the copy written beside it is moved, not left: {left:?}"
            );
        },
    },
    Case {
        named: "an existing directory keeps the mode it has",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("already-here");
            host.create_dir_all(&dir).expect("it is made");
            host.create_private_dir_all(&dir)
                .expect("it is already there");

            assert!(
                host.path_exists(&dir),
                "{adapter}: and it is still there — this is not a chmod in disguise"
            );
        },
    },
    Case {
        named: "narrowing an existing file leaves nobody but the owner",
        asserts: |host, root, adapter, _now| {
            // The one `chmod` Perch performs, and the place the adapters are
            // gated differently — `#[cfg(unix)]` against the runtime Platform.
            // Asked through `file_mode` they agree.
            let path = root.join("was-open");
            host.create_file_with_mode(&path, "found looser than it should be", 0o644)
                .expect("it is created");

            host.make_private(&path).expect("it is narrowed");

            let mode = host.file_mode(&path).expect("the file is there");
            match modes_mean_something() {
                true => assert!(
                    mode.is_some_and(perch::host::is_private),
                    "{adapter}: expected a private mode, got {mode:?}"
                ),
                false => assert_eq!(
                    mode, None,
                    "{adapter}: a platform that does not answer in those terms says so"
                ),
            }
        },
    },
    Case {
        named: "narrowing refuses a link rather than narrowing what it points at",
        asserts: |host, root, adapter, _now| {
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            // The store this narrows sits under `CLAUDE_CONFIG_DIR`, taken
            // verbatim, so it can name a directory somebody else writes. A
            // `chmod` on a *name* follows a link; `O_NOFOLLOW` refuses.
            let elsewhere = root.join("someone-elses-file");
            host.create_file_with_mode(&elsewhere, "not mine", 0o644)
                .expect("it is created");
            let planted = root.join("planted");
            host.link(Link::Symbolic, &elsewhere, &planted)
                .expect("linked");

            match modes_mean_something() {
                true => {
                    host.make_private(&planted)
                        .expect_err("a link is refused rather than followed");
                }
                // No mode to send anywhere: the narrowing is a documented
                // no-op where bits mean nothing, so there is nothing to
                // redirect.
                false => {
                    host.make_private(&planted).unwrap_or_else(|err| {
                        panic!("{adapter}: a no-op cannot be redirected: {err}")
                    });
                }
            }

            assert_eq!(
                host.file_mode(&elsewhere).expect("it is still there"),
                match modes_mean_something() {
                    true => Some(0o644),
                    false => None,
                },
                "{adapter}: and what it pointed at is exactly as it was"
            );
        },
    },
    Case {
        named: "removing a file that was not there is not a failure",
        asserts: |host, root, adapter, _now| {
            host.remove_file(&root.join("never-existed"))
                .unwrap_or_else(|err| panic!("{adapter}: an absent file is not a failure: {err}"));
        },
    },
    Case {
        named: "a rename replaces what is at the destination",
        asserts: |host, root, adapter, _now| {
            let from = root.join("moving");
            let to = root.join("moved-over");
            host.create_file_with_mode(&from, "incoming", PRIVATE_FILE_MODE)
                .expect("the source");
            host.create_file_with_mode(&to, "outgoing", PRIVATE_FILE_MODE)
                .expect("the destination");

            host.rename(&from, &to).expect("it is moved over");

            assert_eq!(host.read_file(&to).ok().as_deref(), Some("incoming"));
            assert!(
                !host.path_exists(&from),
                "{adapter}: and nothing is left at the source"
            );
        },
    },
    Case {
        named: "asking what a name under a linked directory links to reads through the link",
        asserts: |host, root, adapter, _now| {
            let real = root.join("the-real-directory");
            let held = real.join("an-ordinary-file");
            host.create_dir_all(&real).expect("it is made");
            host.create_file_with_mode(&held, "what it holds", PRIVATE_FILE_MODE)
                .expect("with something in it");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            let linked = root.join("the-linked-directory");
            host.link(Link::Symbolic, &real, &linked)
                .expect("the directory is linked");

            // `symlink_metadata` follows every component but the last, and the
            // three answers here are what `reconcile::establish` branches on:
            // `NotFound` is "make it" and `Ok(None)` is "something is in the way".
            assert_eq!(
                host.link_target(&linked.join("an-ordinary-file")).ok(),
                Some(None),
                "{adapter}: a file under a linked directory is a file, not absent"
            );
            assert!(
                matches!(
                    host.link_target(&linked.join("never-there")),
                    Err(HostError::NotFound { .. })
                ),
                "{adapter}: and a name nothing holds under one is still absent"
            );
        },
    },
    Case {
        named: "a rename onto a directory fails and moves nothing",
        asserts: |host, root, adapter, _now| {
            let from = root.join("would-move");
            let onto = root.join("a-directory-in-the-way");
            host.create_file_with_mode(&from, "incoming", PRIVATE_FILE_MODE)
                .expect("the source");
            host.create_dir_all(&onto).expect("the destination");

            host.rename(&from, &onto)
                .expect_err("a directory is not a name a rename may take over");

            assert_eq!(
                host.read_file(&from).ok().as_deref(),
                Some("incoming"),
                "{adapter}: and the source is where it was"
            );
        },
    },
    Case {
        named: "removing a file at a directory fails rather than doing nothing",
        asserts: |host, root, adapter, _now| {
            let at = root.join("a-directory-not-a-file");
            let held = at.join("held");
            host.create_dir_all(&at).expect("it is made");
            host.create_file_with_mode(&held, "still here", PRIVATE_FILE_MODE)
                .expect("with something in it");

            // `Ok` here reads as a store this emptied — `credentials::forget`
            // says so on the strength of it — so answering it for a directory
            // reports a Credential as deleted that is still on the machine.
            host.remove_file(&at)
                .expect_err("a directory is not a file to unlink");

            assert!(
                host.read_file(&held).is_ok(),
                "{adapter}: and what was under it is untouched"
            );
        },
    },
    Case {
        named: "a rename of what is not there fails, and not as NotFound",
        asserts: |host, root, adapter, _now| {
            let failed = host
                .rename(&root.join("never-existed"), &root.join("nowhere"))
                .expect_err("there is nothing to move");

            // `NotFound` is load-bearing at this port — `CredentialStore::read`
            // reads it as "this store holds nothing" and `clients_in` as
            // "nothing is running" — so a rename must not spend it.
            assert!(
                !matches!(failed, HostError::NotFound { .. }),
                "{adapter}: a rename propagates what the filesystem said rather \
                 than naming it: {failed:?}"
            );
        },
    },
    Case {
        named: "removing a link refuses a hard link, which is not one it can recognize",
        asserts: |host, root, adapter, _now| {
            if !can_link(host, Link::Hard, root, adapter) {
                return;
            }
            let target = root.join("the-file-itself");
            let at = root.join("the-other-name");
            host.create_file_with_mode(&target, "what it holds", PRIVATE_FILE_MODE)
                .expect("a file to point at");
            host.link(Link::Hard, &target, &at)
                .expect("the second name");

            // `remove_link`'s whole contract is that it takes away Perch's own
            // share and never the person's file. A hard link says nothing about
            // itself, so neither adapter can tell one from a file of theirs.
            let failed = host
                .remove_link(&at)
                .expect_err("a hard link is not Perch's to remove");
            assert!(
                failed.to_string().contains("not a link"),
                "{adapter}: {failed:?}"
            );
        },
    },
    Case {
        named: "a rename leaves nothing to ask about at the source",
        asserts: |host, root, adapter, _now| {
            let from = root.join("moved-away");
            let to = root.join("landed-here");
            host.create_file_with_mode(&from, "what it holds", PRIVATE_FILE_MODE)
                .expect("the file is written");
            host.rename(&from, &to).expect("it moves");

            // `lock::abandoned` reads `modified_at` for whether an artifact is
            // still there, and takes `NotFound` as "it has gone". A source that
            // keeps answering is a lock the fake would never call abandoned.
            match host.modified_at(&from) {
                Err(HostError::NotFound { .. }) => {}
                other => {
                    panic!("{adapter}: the source of a rename still reports an age: {other:?}")
                }
            }
        },
    },
    // ---- asking about what is there --------------------------------------
    Case {
        named: "a directory is not a file",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("a-directory");
            host.create_dir_all(&dir).expect("it is made");

            assert!(host.path_exists(&dir), "{adapter}: it is there");
            assert!(
                !host.is_file(&dir),
                "{adapter}: and a program search must not let it win the walk"
            );
        },
    },
    Case {
        named: "an absent directory is NotFound rather than empty",
        asserts: |host, root, adapter, _now| {
            // `probe::clients_in` is built on this distinction: "nothing is
            // running" and "nowhere to look" are different answers
            // (ADR a-profile-is-live-by-evidence).
            match host.list_dir(&root.join("no-such-directory")) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
        },
    },
    Case {
        named: "listing a file is an error, and not the absent-directory one",
        asserts: |host, root, adapter, _now| {
            // The other half: `NotFound` lets a Switch replace the live
            // Credential, so a `sessions` that is a file must not read as idle.
            // The two may word it differently; the side they land must agree.
            let file = root.join("not-a-directory");
            host.create_file_with_mode(&file, "x", PRIVATE_FILE_MODE)
                .expect("a file to ask about");
            match host.list_dir(&file) {
                Err(HostError::NotFound { .. }) => panic!(
                    "{adapter}: a file read as an absent directory is a Switch \
                     replacing a Credential something else is holding"
                ),
                Err(_) => {}
                Ok(listed) => {
                    panic!("{adapter}: a file is not a directory, got {listed:?}")
                }
            }
        },
    },
    Case {
        named: "a directory lists what it holds, as full paths",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("holding-three");
            host.create_dir_all(&dir).expect("it is made");
            for name in ["one", "two", "three"] {
                host.create_file_with_mode(&dir.join(name), name, PRIVATE_FILE_MODE)
                    .expect("a file in it");
            }

            let mut found = host.list_dir(&dir).expect("it is there");
            found.sort();
            let mut expected: Vec<PathBuf> = ["one", "two", "three"]
                .iter()
                .map(|n| dir.join(n))
                .collect();
            expected.sort();
            assert_eq!(found, expected, "{adapter}: full paths, all of them");
        },
    },
    Case {
        named: "an empty directory lists as empty rather than NotFound",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("holding-nothing");
            host.create_dir_all(&dir).expect("it is made");

            assert_eq!(
                host.list_dir(&dir).expect("it is there"),
                Vec::<PathBuf>::new(),
                "{adapter}: the other half of the same distinction"
            );
        },
    },
    Case {
        named: "removing a directory takes what is under it",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("going");
            host.create_dir_all(&dir.join("nested"))
                .expect("it is made");
            host.create_file_with_mode(&dir.join("nested/held"), "x", PRIVATE_FILE_MODE)
                .expect("something under it");

            host.remove_dir_all(&dir).expect("it goes");

            assert!(!host.path_exists(&dir), "{adapter}: and it is gone");
            assert!(!host.path_exists(&dir.join("nested/held")));
        },
    },
    Case {
        named: "a touch leaves the contents alone",
        asserts: |host, root, adapter, _now| {
            let path = root.join("touched");
            host.create_file_with_mode(&path, "unchanged", PRIVATE_FILE_MODE)
                .expect("it is written");

            host.touch(&path).expect("it is marked written");

            assert_eq!(
                host.read_file(&path).ok().as_deref(),
                Some("unchanged"),
                "{adapter}: how a lock holder says it is still there, without saying anything else"
            );
            host.modified_at(&path)
                .unwrap_or_else(|err| panic!("{adapter}: and it has a time: {err}"));
        },
    },
    Case {
        named: "asking when an absent path was written is NotFound",
        asserts: |host, root, adapter, _now| match host.modified_at(&root.join("never-written")) {
            Err(HostError::NotFound { .. }) => {}
            other => panic!("{adapter}: expected NotFound, got {other:?}"),
        },
    },
    // ---- the whole of what makes a lock a lock ---------------------------
    Case {
        named: "only the first exclusive create succeeds",
        asserts: |host, root, adapter, _now| {
            let dir = root.join("the-lock");
            host.create_dir_exclusive(&dir)
                .unwrap_or_else(|err| panic!("{adapter}: the first takes it: {err}"));

            match host.create_dir_exclusive(&dir) {
                Err(HostError::AlreadyExists { .. }) => {}
                other => {
                    panic!("{adapter}: a lock that succeeds quietly is not a lock, got {other:?}")
                }
            }
        },
    },
    Case {
        named: "a lock carries the time it was taken",
        asserts: |host, root, adapter, now| {
            // Exclusivity says who gets the lock; the age is how a holder that
            // died holding it is told from one still working, read off the
            // artifact rather than off a file inside.
            let dir = root.join("the-dated-lock");
            host.create_dir_exclusive(&dir).expect("it is taken");

            let held_since = host.modified_at(&dir).unwrap_or_else(|err| {
                panic!("{adapter}: a lock with no age is never stale: {err}")
            });
            assert!(
                (now - held_since).num_seconds().abs() < 60,
                "{adapter}: taken just now, not {held_since}"
            );
        },
    },
    // ---- links ------------------------------------------------------------
    Case {
        named: "link_target answers for the link and not for the file",
        asserts: |host, root, adapter, _now| {
            let real = root.join("the-file");
            let link = root.join("names-the-file");
            host.create_file_with_mode(&real, "held", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link)
                .expect("the link is made");

            assert_eq!(
                host.link_target(&link).expect("it is there"),
                Some(real.clone()),
                "{adapter}: the link names what it names"
            );
            assert_eq!(
                host.link_target(&real).expect("it is there"),
                None,
                "{adapter}: and a file is not a link"
            );
        },
    },
    // The property every share rests on, and the one a copy would fail: what
    // is read through the link is what the Default Profile holds *now*, not
    // what it held when Reconcile made it (ADR everything-but-the-account).
    Case {
        named: "a write after the link is made is read through it",
        asserts: |host, root, adapter, _now| {
            let real = root.join("CLAUDE.md");
            let link = root.join("shared-CLAUDE.md");
            host.create_file_with_mode(&real, "remember this", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link)
                .expect("the link is made");

            host.create_file_with_mode(&real, "remember this instead", PRIVATE_FILE_MODE)
                .expect("and the file changes afterwards");

            assert_eq!(
                host.read_file(&link).ok().as_deref(),
                Some("remember this instead"),
                "{adapter}: a link is a name, not a copy taken when it was made"
            );
        },
    },
    // The same for a directory, and the part an allowlist gets wrong: a file
    // appearing in the Default Profile after the link was made is reachable
    // through it without anything being told.
    Case {
        named: "a directory link shows what the directory gains afterwards",
        asserts: |host, root, adapter, _now| {
            let real = root.join("plugins");
            let link = root.join("shared-plugins");
            host.create_dir_all(&real).expect("a directory to share");

            // What Reconcile picks for a directory here, so this is the
            // junction path on a Windows without Developer Mode.
            let kind = if cfg!(windows) {
                Link::Junction
            } else {
                Link::Symbolic
            };
            // A junction needs no privilege; a symbolic one a machine may
            // decline.
            if kind == Link::Symbolic && !can_link(host, kind, root, adapter) {
                return;
            }
            host.link(kind, &real, &link).expect("the link is made");

            host.create_file_with_mode(&real.join("config.json"), "{}", PRIVATE_FILE_MODE)
                .expect("something arrives in it afterwards");

            assert_eq!(
                host.read_file(&link.join("config.json")).ok().as_deref(),
                Some("{}"),
                "{adapter}: reached through the link, having been told nothing"
            );
            assert!(
                host.link_target(&link)
                    .expect("the link answers for itself")
                    .is_some(),
                "{adapter}: and a {} reads back as a link, which is how a stale \
                 one is found",
                kind.describe()
            );
        },
    },
    Case {
        named: "a link whose target has gone is still a link",
        asserts: |host, root, adapter, _now| {
            // How a broken one is found and repaired: `reconcile::establish`
            // branches three ways on exactly this.
            let real = root.join("about-to-go");
            let link = root.join("left-dangling");
            host.create_file_with_mode(&real, "briefly", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link)
                .expect("the link is made");
            host.remove_file(&real).expect("the target goes");

            assert_eq!(
                host.link_target(&link).expect("the link is still there"),
                Some(real.clone()),
                "{adapter}: a dangling link is Some, not NotFound"
            );
        },
    },
    Case {
        named: "nothing at all is a third answer",
        asserts: |host, root, adapter, _now| match host
            .link_target(&root.join("nothing-of-any-kind"))
        {
            Err(HostError::NotFound { .. }) => {}
            other => {
                panic!("{adapter}: absent and not-a-link are different repairs, got {other:?}")
            }
        },
    },
    Case {
        named: "removing a link leaves what it points at",
        asserts: |host, root, adapter, _now| {
            let real = root.join("kept");
            let link = root.join("removed");
            host.create_file_with_mode(&real, "still here", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &link)
                .expect("the link is made");

            host.remove_link(&link).expect("the link goes");

            assert!(!host.path_exists(&link), "{adapter}: the link is gone");
            assert_eq!(
                host.read_file(&real).ok().as_deref(),
                Some("still here"),
                "{adapter}: and what it pointed at is not"
            );
        },
    },
    Case {
        named: "making a directory at a link uses what it points at",
        asserts: |host, root, adapter, _now| {
            // `probe::claim` does `create_dir_all` on a Profile's `sessions`,
            // so this decides where the Marker lands; a directory *shadowing*
            // the link is a third behavior no filesystem has.
            let elsewhere = root.join("another-profile-sessions");
            let here = root.join("sessions");
            host.create_dir_all(&elsewhere)
                .expect("somewhere to point at");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &elsewhere, &here)
                .expect("the link is made");

            host.create_dir_all(&here)
                .expect("a directory that is already there is not a failure");

            assert_eq!(
                host.link_target(&here).expect("it is there"),
                Some(elsewhere.clone()),
                "{adapter}: the link is still a link rather than shadowed"
            );

            // The other half of "uses what it points at": a write under the
            // link lands in the target, which is the hazard — a Marker written
            // into a Profile's `sessions` reaching the Default Profile's.
            host.create_file_with_mode(&here.join("a-marker"), "written", PRIVATE_FILE_MODE)
                .expect("a file under it");
            assert!(
                host.path_exists(&elsewhere.join("a-marker")),
                "{adapter}: writing under it writes through it"
            );
        },
    },
    Case {
        named: "making a directory where a file already sits is refused",
        asserts: |host, root, adapter, _now| {
            // `create_dir_all`'s third answer, reachable wherever a Profile
            // holds a file at a name Perch expects a directory: `mkdir` says
            // EEXIST and the `is_dir` fallback says no, so this fails.
            let occupied = root.join("not-a-directory");
            host.create_file_with_mode(&occupied, "a file", PRIVATE_FILE_MODE)
                .expect("the file");

            let refused = host.create_dir_all(&occupied);

            assert!(
                refused.is_err(),
                "{adapter}: a file is not a directory to make, got {refused:?}"
            );
            assert_eq!(
                host.read_file(&occupied).ok().as_deref(),
                Some("a file"),
                "{adapter}: and it is left exactly as it was"
            );
        },
    },
    Case {
        named: "making a directory at a link to nothing is refused",
        asserts: |host, root, adapter, _now| {
            // `mkdir` says EEXIST and the `is_dir` it falls back on follows
            // the link and finds nothing — the one shape of `mkdir -p` that
            // fails on a path nothing occupies.
            let gone = root.join("target-that-was-removed");
            let dangling = root.join("points-at-nothing");
            host.create_dir_all(&gone).expect("somewhere to point at");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &gone, &dangling)
                .expect("the link is made");
            host.remove_dir_all(&gone).expect("and then it goes");

            let refused = host.create_dir_all(&dangling);

            assert!(
                refused.is_err(),
                "{adapter}: a link to nothing is not a directory to make, got {refused:?}"
            );
        },
    },
    Case {
        named: "an exclusive directory is mkdir rather than mkdir -p",
        asserts: |host, root, adapter, _now| {
            // Not `mkdir -p`: a lock taken inside a Profile that does not
            // exist is a Switch proceeding under a lock the machine would have
            // refused it.
            let missing = root.join("no-such-profile");
            match host.create_dir_exclusive(&missing.join(".oauth_refresh.lock")) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
            assert!(
                !host.path_exists(&missing),
                "{adapter}: and the parent was not invented on the way"
            );
        },
    },
    Case {
        named: "touching a path that is not there reports NotFound",
        asserts: |host, root, adapter, _now| {
            // The answer `lock::renew` reads as "the artifact has gone, so
            // this hold is no longer mine" — so a `utimes` failure funneled
            // into `Io` loses the one variant this port treats as meaningful.
            match host.touch(&root.join("no-such-artifact")) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
        },
    },
    Case {
        named: "a link onto an occupied name reports AlreadyExists",
        asserts: |host, root, adapter, _now| {
            // The variant this port treats as contention rather than
            // breakage, so a `?` that flattens EEXIST into `Io` is a caller
            // written against the fake and wrong on the machine.
            let target = root.join("something-to-point-at");
            let taken = root.join("already-here");
            host.create_file_with_mode(&target, "x", PRIVATE_FILE_MODE)
                .expect("the target");
            host.create_file_with_mode(&taken, "in the way", PRIVATE_FILE_MODE)
                .expect("something at the name");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }

            match host.link(Link::Symbolic, &target, &taken) {
                Err(HostError::AlreadyExists { .. }) => {}
                other => panic!("{adapter}: expected AlreadyExists, got {other:?}"),
            }
        },
    },
    Case {
        named: "removing a link refuses anything that is not one",
        asserts: |host, root, adapter, _now| {
            // The one call whose whole contract is that it takes away only
            // Perch's own share — and the refusal is per-adapter, so asking
            // the fake alone proves it of the adapter that has it.
            let plain = root.join("the-persons-own-file");
            host.create_file_with_mode(&plain, "not Perch's", PRIVATE_FILE_MODE)
                .expect("the file");

            let refused = host.remove_link(&plain);

            assert!(
                refused.is_err(),
                "{adapter}: a plain file is not a link, got {refused:?}"
            );
            assert_eq!(
                host.read_file(&plain).ok().as_deref(),
                Some("not Perch's"),
                "{adapter}: and it is still there"
            );
        },
    },
    Case {
        named: "a kind this platform will not make is refused rather than substituted",
        asserts: |host, root, adapter, _now| {
            // A junction is Windows' link for a directory. Which kind was made
            // decides what happens when the target is replaced, so a platform
            // that cannot make one says so rather than substituting.
            let target = root.join("a-directory");
            host.create_dir_all(&target).expect("something to point at");
            let at = root.join("the-junction");

            match (cfg!(windows), host.link(Link::Junction, &target, &at)) {
                (true, Ok(())) => assert_eq!(
                    host.link_target(&at).expect("it is there"),
                    Some(target),
                    "{adapter}: and it names what it was pointed at"
                ),
                (false, Err(_)) => {}
                (windows, made) => {
                    panic!("{adapter}: windows is {windows} and the link was {made:?}")
                }
            }
        },
    },
    Case {
        named: "a hard link is a second name rather than a link",
        asserts: |host, root, adapter, _now| {
            let real = root.join("first-name");
            let second = root.join("second-name");
            host.create_file_with_mode(&real, "one file", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Hard, root, adapter) {
                return;
            }
            host.link(Link::Hard, &real, &second)
                .expect("the hard link is made");

            assert_eq!(
                host.read_file(&second).ok().as_deref(),
                Some("one file"),
                "{adapter}: it reads as the file it names"
            );
            assert_eq!(
                host.link_target(&second).expect("it is there"),
                None,
                "{adapter}: and it is indistinguishable from the file's first name"
            );
        },
    },
    Case {
        named: "a hard link stops naming a file that is replaced rather than written",
        asserts: |host, root, adapter, _now| {
            // Why Reconcile re-establishes a hard link before every Run: it
            // names a *file*, so the write-beside-then-rename-over an editor
            // performs leaves the name on a file nobody can reach.
            let real = root.join("settings.json");
            let second = root.join("shared-settings.json");
            host.create_file_with_mode(&real, "first", PRIVATE_FILE_MODE)
                .expect("the file");
            if !can_link(host, Link::Hard, root, adapter) {
                return;
            }
            host.link(Link::Hard, &real, &second)
                .expect("the hard link is made");

            let beside = root.join("settings.json.new");
            host.create_file_with_mode(&beside, "second", PRIVATE_FILE_MODE)
                .expect("written beside");
            host.rename(&beside, &real).expect("and renamed over");

            assert_eq!(
                host.read_file(&real).ok().as_deref(),
                Some("second"),
                "{adapter}: the name the editor saved through has the new file"
            );
            assert_eq!(
                host.read_file(&second).ok().as_deref(),
                Some("first"),
                "{adapter}: and the hard link is still naming the old one, which \
                 is the sharing silently ending"
            );
        },
    },
    Case {
        named: "a write over a directory is refused rather than performed",
        asserts: |host, root, adapter, _now| {
            // The mirror of "writing under a file is ENOTDIR": `create_new`
            // meets what `remove_file` would not take away, and a fake that
            // wrote anyway leaves one path a file and a directory at once.
            let at = root.join("a-directory");
            host.create_private_dir_all(&at)
                .expect("the directory is made");

            let written = host.create_file_with_mode(&at, "{}", PRIVATE_FILE_MODE);

            assert!(
                written.is_err(),
                "{adapter}: a directory is not a file to write over"
            );
            assert!(
                host.read_file(&at).is_err(),
                "{adapter}: and it is still a directory afterwards"
            );
        },
    },
    Case {
        named: "a file made under a linked directory can be removed again",
        asserts: |host, root, adapter, _now| {
            // What Reconcile makes, and what `replace_via_tmp` then writes its
            // copy beside and clears on a failure: a write that resolved a link
            // and a remove that did not would leave the temp file behind.
            let real = root.join("real");
            let linked = root.join("linked");
            host.create_private_dir_all(&real).expect("the directory");
            if !can_link(host, Link::Symbolic, root, adapter) {
                return;
            }
            host.link(Link::Symbolic, &real, &linked)
                .expect("the link is made");

            let beside = linked.join("settings.json.new");
            host.create_file_with_mode(&beside, "half", PRIVATE_FILE_MODE)
                .expect("written through the link");
            host.remove_file(&beside).expect("and removed through it");

            assert!(
                !host.path_exists(&real.join("settings.json.new")),
                "{adapter}: the write and the remove resolve the same path, or a \
                 failed replacement leaves its copy where nothing names it"
            );
        },
    },
];

/// One sentence on the port that no `&dyn Filesystem` can be asked: [`Case`]
/// narrows to the filesystem, which is 19 of the port's 43 methods. A table of
/// its own rather than a wider [`Case`], because nothing here wants a scratch
/// directory or a machine's link support to decide.
struct WholeHostCase {
    /// What sentence this asserts, for [`Case::named`]'s reason — and here it
    /// travels into the message, there being no scratch directory to name.
    named: &'static str,
    asserts: fn(&dyn Host, &str),
}

const WHOLE_HOST_CASES: &[WholeHostCase] = &[
    WholeHostCase {
        named: "a number that names no process is neither alive nor started",
        asserts: |host, adapter| {
            // `probe::clients_in` parses a pid out of any filename in a
            // `sessions` directory, so both are reachable from a stray file.
            for pid in [0, u32::MAX] {
                assert!(
                    !host.process_alive(pid),
                    "{adapter}: {pid} names no process, so nothing is alive at it"
                );
                assert_eq!(
                    host.process_started_at(pid),
                    None,
                    "{adapter}: {pid} names no process, so nothing began at it — \
                     a start time here corroborates a marker for ever, because \
                     boot is older than every session"
                );
            }
        },
    },
    WholeHostCase {
        named: "this process is alive and began no later than now",
        asserts: |host, adapter| {
            let me = host.process_id();
            assert!(
                host.process_alive(me),
                "{adapter}: the process asking is running"
            );
            let began = host
                .process_started_at(me)
                .unwrap_or_else(|| panic!("{adapter}: this process has a start time"));
            assert!(
                began <= host.now(),
                "{adapter}: a process began before the clock reads now, or every \
                 marker it writes is evidence of nothing"
            );
        },
    },
    WholeHostCase {
        named: "where this process is, and where it came from, are rooted",
        asserts: |host, adapter| {
            // Through `probe::rooted` rather than `Path::is_absolute`, which reads
            // the separator of the platform this build runs on: a fake claiming
            // Windows answers the paths a test wrote, and `/a/b` is a root there.
            let on_windows = host.platform() == Platform::Windows;
            for (what, path) in [
                ("home_dir", host.home_dir().expect("a home")),
                (
                    "current_dir",
                    host.current_dir().expect("a working directory"),
                ),
                ("current_exe", host.current_exe().expect("a binary")),
            ] {
                assert!(
                    perch::probe::rooted(&path.to_string_lossy(), on_windows),
                    "{adapter}: {what} answered {path:?}, and every path Perch \
                     joins onto is joined onto from somewhere other than where \
                     the command happened to be run"
                );
            }
        },
    },
    WholeHostCase {
        named: "the platform is the machine the suite is running on",
        asserts: |host, adapter| {
            assert_eq!(
                host.platform(),
                this_platform(),
                "{adapter}: the Credential Store, the Service and the program \
                 search are all chosen off this"
            );
        },
    },
    WholeHostCase {
        named: "a variable nothing set is not there",
        asserts: |host, adapter| {
            // Named as no machine names one: the fake is asked the same
            // question as the real host, which reads this process's own
            // environment.
            assert_eq!(
                host.env_var("PERCH_A_VARIABLE_NOTHING_SETS"),
                None,
                "{adapter}: an unset variable is nothing, not an empty value — \
                 `CLAUDE_CONFIG_DIR` read as `Some(\"\")` derives a Credential \
                 Store from the root"
            );
        },
    },
    WholeHostCase {
        named: "a user id is answered exactly where the filesystem judges by one",
        asserts: |host, adapter| {
            assert_eq!(
                host.user_id().is_some(),
                modes_mean_something(),
                "{adapter}: the Service files a session under this and every \
                 private write is judged by it, so a platform that has no uid \
                 must not answer with one"
            );
        },
    },
    WholeHostCase {
        named: "a program that is nowhere is an error and not a status",
        asserts: |host, adapter| {
            let refused = host.exec("perch-no-such-program-anywhere", &[]);

            assert!(
                refused.is_err(),
                "{adapter}: a program that could not be run is told apart from \
                 one that ran and failed — `probe::installed` reads a status of \
                 its own from the second: {refused:?}"
            );
        },
    },
    WholeHostCase {
        named: "nothing is asked to stop before anything asks it to",
        asserts: |host, adapter| {
            assert!(
                !host.asked_to_stop(),
                "{adapter}: a Watcher that read this as true at the top of its \
                 first round would stop before it had watched anything"
            );

            // Twice, because `perch watcher check` and `perch watcher run` both
            // reach the same process-wide disposition and one may follow the
            // other in a single run.
            host.listen_for_interrupts();
            host.listen_for_interrupts();

            assert!(
                !host.asked_to_stop(),
                "{adapter}: listening for a signal is not being sent one"
            );
        },
    },
    WholeHostCase {
        named: "a wait nobody interrupts is a wait that finished",
        asserts: |host, adapter| {
            assert_eq!(
                host.wait(1),
                Waited::Fully,
                "{adapter}: the Watcher's whole pace rests on telling a wait \
                 that ended from one somebody ended"
            );
            // Only that it returns: a sleep asserts nothing about a clock,
            // because the fake's moves and the real one's is the machine's.
            host.sleep(1);
        },
    },
];

/// A run in which every link case skipped checked nothing about the half of
/// this port a Profile's shares rest on.
fn refuse_a_run_with_no_links_in_it(made: &[Link]) {
    assert!(
        made.iter()
            .any(|kind| matches!(kind, Link::Symbolic | Link::Hard)),
        "this machine makes neither a symbolic nor a hard link, so every link \
         case skipped itself and the suite passed without asking anything"
    );
}

/// The real filesystem, one scratch directory per case.
#[test]
fn the_real_host_conforms_to_the_port() {
    refuse_a_run_with_no_links_in_it(links_this_machine_makes());

    let host = RealHost::new();
    for case in CASES {
        let root = scratch(&case.named.replace(' ', "-"));
        (case.asserts)(&host, &root, "RealHost", host.now());
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The fake, asked to be the platform the real host is already on, one fresh
/// world per case.
#[test]
fn the_fake_host_conforms_to_the_port() {
    let made = links_this_machine_makes();
    refuse_a_run_with_no_links_in_it(made);
    let developer_mode = made.contains(&Link::Symbolic);

    for case in CASES {
        let host = FakeHost::new().with_platform(this_platform());
        // What the machine will actually make, so the two skip the same cases
        // rather than each skipping its own.
        let host = match developer_mode {
            true => host.with_developer_mode(),
            false => host,
        };
        let root = PathBuf::from("/conformance").join(case.named.replace(' ', "-"));
        host.create_dir_all(&root).expect("a root to work under");
        (case.asserts)(&host, &root, "FakeHost", host.now());
    }
}

/// The half of the port a [`Case`] cannot reach, asked of both adapters in one
/// test: neither needs a world built for it, so there is nothing to set up
/// differently between them.
#[test]
fn both_adapters_conform_to_the_port_beyond_the_filesystem() {
    for case in WHOLE_HOST_CASES {
        (case.asserts)(&RealHost::new(), &format!("RealHost ({})", case.named));
        (case.asserts)(
            &FakeHost::new().with_platform(this_platform()),
            &format!("FakeHost ({})", case.named),
        );
    }
}

// ————— the table against the port it is a table of —————

mod tree;

/// Every method on the port, trait by trait, read off `Host`'s own supertraits.
///
/// Read rather than listed, because a list is the thing that goes out of step:
/// three reviews in a row found a sentence with two implementations and no
/// reader, on a method nobody had noticed the table never asked.
fn the_port() -> Vec<(String, String)> {
    let source = std::fs::read_to_string(tree::repo().join("src/host/mod.rs"))
        .expect("the port declares itself in one file");

    let mut found = Vec::new();
    let mut pending = supertraits_of("Host", &source);
    while let Some(name) = pending.pop() {
        pending.extend(supertraits_of(&name, &source));
        for method in methods_on(&name, &source) {
            found.push((name.clone(), method));
        }
    }
    found.sort();
    found
}

/// The traits one trait is the sum of, so `Filesystem` carries `Files` and
/// `Links` in without either being named at `Host`.
fn supertraits_of(name: &str, source: &str) -> Vec<String> {
    let Some(at) = source.find(&format!("\npub trait {name}")) else {
        return Vec::new();
    };
    let declaration = &source[at + 1..];
    let Some(open) = declaration.find('{') else {
        return Vec::new();
    };
    match declaration[..open].split_once(':') {
        None => Vec::new(),
        Some((_, sum)) => sum
            .split('+')
            .map(|held| held.trim().to_string())
            .filter(|held| !held.is_empty())
            .collect(),
    }
}

/// The methods a trait declares itself, which are the lines at one indent under
/// its own `{`.
fn methods_on(name: &str, source: &str) -> Vec<String> {
    let Some(at) = source.find(&format!("\npub trait {name}")) else {
        return Vec::new();
    };
    source[at..]
        .lines()
        .skip(1)
        .take_while(|line| !line.starts_with('}'))
        .filter_map(|line| line.strip_prefix("    fn "))
        .filter_map(|rest| rest.split(['(', '<']).next())
        .map(str::to_string)
        .collect()
}

/// The methods no case can ask, each with what stops one.
///
/// The list is short and every entry names a machine effect a test may not
/// have, rather than a case somebody has not got round to: a method left off
/// for the second reason is one the fake may answer however it likes.
const UNASKED: &[(&str, &str)] = &[
    (
        "exec_interactive",
        "hands this process's terminal to a child and waits for a person",
    ),
    (
        "http",
        "reaches Anthropic; `your_machine.rs` is where that is asked",
    ),
    (
        "keychain_get",
        "the real one raises the macOS dialog, which is `your_machine.rs`'s to answer",
    ),
    (
        "keychain_set",
        "the real one raises the macOS dialog, which is `your_machine.rs`'s to answer",
    ),
    (
        "keychain_delete",
        "the real one raises the macOS dialog, which is `your_machine.rs`'s to answer",
    ),
    (
        "note",
        "the real one writes to this process's stderr and the fake keeps a list, \
              so the two have no observable in common",
    ),
    (
        "read_line",
        "reads this process's stdin, which a suite may not consume",
    ),
    (
        "read_secret",
        "reads this process's stdin, which a suite may not consume",
    ),
    (
        "is_interactive",
        "answers about this process's stdin, which the harness decides and neither adapter can",
    ),
];

/// Every `host.<method>(` the table writes, which is what it asks the two
/// adapters. Read out of the source rather than declared beside each case: a
/// case that names the method it asks in a field is one that can name the wrong
/// one, and this cannot.
fn whatever_the_table_calls() -> std::collections::BTreeSet<String> {
    let table = std::fs::read_to_string(tree::repo().join("tests/conformance.rs"))
        .expect("the table is a file");
    table
        .match_indices("host.")
        .filter_map(|(at, _)| table[at + "host.".len()..].split('(').next())
        .filter(|called| {
            !called.is_empty() && called.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
        .map(str::to_string)
        .collect()
}

/// Every method on the port is asked of both adapters, or says why not.
///
/// A sentence with two implementations and no reader is one they drift apart on,
/// and a method the table never names is that (ADR a-class-not-its-instances).
#[test]
fn every_method_on_the_port_is_asked_of_both_adapters() {
    let asked = whatever_the_table_calls();

    let unasked: Vec<String> = the_port()
        .into_iter()
        .filter(|(_, method)| !asked.contains(method))
        .filter(|(_, method)| !UNASKED.iter().any(|(held, _)| held == method))
        .map(|(trait_name, method)| format!("{trait_name}::{method}"))
        .collect();

    assert!(
        unasked.is_empty(),
        "the two adapters answer these and nothing compares the answers — add a \
         case, or an `UNASKED` entry naming the machine effect that stops one:\n{}",
        unasked.join("\n")
    );

    // The exemptions are a list, so they are a list that can outlive its
    // reason: one naming a method the port no longer has says nothing.
    let port: std::collections::BTreeSet<String> =
        the_port().into_iter().map(|(_, method)| method).collect();
    for (method, _) in UNASKED {
        assert!(
            port.contains(*method),
            "`{method}` is excused from the table and is not on the port"
        );
    }
}
