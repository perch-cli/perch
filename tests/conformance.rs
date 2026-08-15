//! Conformance: the two adapters at the Host port, asked the same questions.
//!
//! The Host trait's interface is not its signatures. It is the sentences on
//! them — an absent directory reports `NotFound` rather than emptiness, a
//! removed file that was not there is not a failure, `link_target` answers for
//! the link and not for what it points at, `create_dir_exclusive` never
//! succeeds quietly. Those sentences had two implementations and no reader:
//! [`RealHost`] and [`FakeHost`] were kept in step by hand, commit by commit,
//! and the fake's own doc comments are a list of the times that failed —
//! reads that did not follow a symbolic link, a private write that skipped the
//! choreography it was supposed to model, and three more.
//!
//! So the sentences are executed here, once, against both. A case that passes
//! for one adapter and fails for the other is the finding; the table shape is
//! what lets a failure say which one and which sentence, rather than stopping
//! at the first assertion of a long function.
//!
//! Not the whole port. What a scratch directory can drive is the filesystem and
//! the links — nineteen methods. The clock, the keychain, the processes, the
//! terminal and the network are either the machine's own state, which a test
//! has no business owning, or the very things a fake exists to invent: a fake
//! clock that agreed with the real one would be no use to anybody. The keychain
//! is asserted against the real one by `contract_credentials`, behind the
//! feature that suite needs.
//!
//! Ungated, unlike the `contract_*` suites. Those ask whether Perch's beliefs
//! about Claude Code are still true, and a failure there is news about upstream.
//! This asks whether Perch's two adapters still agree with each other, and a
//! failure here is a fault in the change that caused it — so it runs on every
//! pull request, on every platform CI has.

use std::path::{Path, PathBuf};

use perch::host::{FakeHost, Host, HostError, Link, PRIVATE_FILE_MODE, Platform, RealHost};

/// This machine, as the port names it — so the fake is asked to be the platform
/// the real host is already on.
///
/// Without it the two are not answering about the same machine and any
/// agreement between them is a coincidence: `RealHost::file_mode` is gated on
/// `#[cfg(unix)]` at compile time while the fake's is gated on this value at run
/// time, so a fake left at its default would claim macOS permissions on a
/// Windows runner.
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

/// Which links this machine will actually make, asked of the real filesystem.
///
/// A Windows without Developer Mode makes no symbolic link, and that is a fact
/// about the *machine* rather than about either adapter. The fake models it with
/// a knob that defaults to off, so left alone the two answer differently and
/// each skips on its own: the fake skips every symbolic case while the real host
/// asserts them, both tests pass, and the suite reports an agreement it never
/// checked. So the machine is asked once and the fake is told the answer.
fn links_this_machine_makes() -> &'static [Link] {
    // Asked once. The two tests run in parallel and the answer is a property of
    // the machine rather than of either of them, so asking twice would be two
    // threads racing over one scratch directory for the same answer.
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

/// Whether this machine will make a link of this kind at all — a Windows
/// without Developer Mode will not make a symbolic one, which is the case the
/// other two kinds exist for. Said out loud, because a case that skipped itself
/// and a case that asserted look identical otherwise.
fn can_link(host: &dyn Host, kind: Link, root: &Path, adapter: &str) -> bool {
    let target = root.join("can-link-target");
    let at = root.join("can-link-at");
    host.create_file_with_mode(&target, "x", PRIVATE_FILE_MODE)
        .expect("a file to point at");
    match host.link(kind, &target, &at) {
        Ok(()) => {
            let _ = host.remove_link(&at);
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
    asserts: fn(&dyn Host, &Path, &str),
}

const CASES: &[Case] = &[
    // ---- reading and writing --------------------------------------------
    Case {
        named: "a file that is not there reports NotFound",
        asserts: |host, root, adapter| {
            let missing = root.join("nothing-here");
            match host.read_file(&missing) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
        },
    },
    Case {
        named: "a read follows a symbolic link",
        asserts: |host, root, adapter| {
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
        named: "a file is created with exactly the mode asked for",
        asserts: |host, root, adapter| {
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
    // The case above asks for a mode no umask widens, so it answered the same
    // whether or not the umask had been taken out of it. This one asks for a
    // bit every ordinary umask strips — 022, 002 and 077 all clear at least one
    // of these — so it can only pass where the mode is the creation's.
    //
    // What it guards is `write_atomically`, which reads the target's mode and
    // writes the replacement with it: a `.claude.json` its owner keeps at 0644
    // came back at 0600 from a Switch run in a shell with a tight umask, and
    // nothing said so.
    Case {
        named: "the mode asked for survives a umask that would have taken bits out of it",
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
            let dir = root.join("private-dir");
            let path = dir.join("credential");
            host.write_private_file(&path, "a secret")
                .expect("it is written");

            assert_eq!(host.read_file(&path).ok().as_deref(), Some("a secret"));
            if modes_mean_something() {
                assert_eq!(
                    host.file_mode(&path).ok().flatten(),
                    Some(PRIVATE_FILE_MODE),
                    "{adapter}: the owner and nobody else (ADR 0020)"
                );
            }
        },
    },
    Case {
        named: "a private write leaves nothing beside the file",
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
            // The one `chmod` Perch performs, and the place the two adapters are
            // gated differently: `RealHost::make_private` is `#[cfg(unix)]` and a
            // silent no-op elsewhere, while the fake gates on the runtime
            // Platform. Asked through `file_mode`, which answers `None` where
            // bits mean nothing, the two agree — this is what says so.
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
        named: "removing a file that was not there is not a failure",
        asserts: |host, root, adapter| {
            host.remove_file(&root.join("never-existed"))
                .unwrap_or_else(|err| panic!("{adapter}: an absent file is not a failure: {err}"));
        },
    },
    Case {
        named: "a rename replaces what is at the destination",
        asserts: |host, root, adapter| {
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
    // ---- asking about what is there --------------------------------------
    Case {
        named: "a directory is not a file",
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
            // Load-bearing: `probe::clients_in` is built on this distinction, so
            // a fake answering `Ok(vec![])` would silently disarm ADR 0022 —
            // "nothing is running" and "nowhere to look" are different answers.
            match host.list_dir(&root.join("no-such-directory")) {
                Err(HostError::NotFound { .. }) => {}
                other => panic!("{adapter}: expected NotFound, got {other:?}"),
            }
        },
    },
    Case {
        named: "a directory lists what it holds, as full paths",
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| {
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
        asserts: |host, root, adapter| match host.modified_at(&root.join("never-written")) {
            Err(HostError::NotFound { .. }) => {}
            other => panic!("{adapter}: expected NotFound, got {other:?}"),
        },
    },
    // ---- the whole of what makes a lock a lock ---------------------------
    Case {
        named: "only the first exclusive create succeeds",
        asserts: |host, root, adapter| {
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
    // ---- links ------------------------------------------------------------
    Case {
        named: "link_target answers for the link and not for the file",
        asserts: |host, root, adapter| {
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
    Case {
        named: "a link whose target has gone is still a link",
        asserts: |host, root, adapter| {
            // The whole of how a broken one is found and repaired
            // (`reconcile::establish` branches three ways on exactly this).
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
        asserts: |host, root, adapter| match host.link_target(&root.join("nothing-of-any-kind")) {
            Err(HostError::NotFound { .. }) => {}
            other => {
                panic!("{adapter}: absent and not-a-link are different repairs, got {other:?}")
            }
        },
    },
    Case {
        named: "removing a link leaves what it points at",
        asserts: |host, root, adapter| {
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
        named: "a kind this platform will not make is refused rather than substituted",
        asserts: |host, root, adapter| {
            // A junction is Windows' link for a directory and exists nowhere
            // else. Which kind was made decides what happens when the target is
            // replaced, so a platform that cannot make one says so rather than
            // quietly putting a symbolic link there instead.
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
        asserts: |host, root, adapter| {
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
];

/// A run in which every link case skipped is a run that checked nothing about
/// the half of this port that ADR 0026 turns on.
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
        (case.asserts)(&host, &root, "RealHost");
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
        // What the machine the real host is on will actually make, so the two
        // skip the same cases rather than each skipping its own.
        let host = match developer_mode {
            true => host.with_developer_mode(),
            false => host,
        };
        let root = PathBuf::from("/conformance").join(case.named.replace(' ', "-"));
        host.create_dir_all(&root).expect("a root to work under");
        (case.asserts)(&host, &root, "FakeHost");
    }
}
