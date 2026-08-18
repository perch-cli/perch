//! Behavior: `perch watcher install`, `uninstall` and `status` — having the
//! machine run the Watcher for you.
//!
//! The unit files themselves are argued with in `src/service.rs`'s unit tests,
//! where three platforms' worth of quoting can be read side by side. What is
//! asserted here is the part that touches a machine: that a unit is written
//! where the platform keeps one, that the service manager is actually driven,
//! that a failure leaves nothing behind, and that the two commands which meet a
//! running Service — a Purge and an Upgrade — do the thing ADR 0040 says they
//! owe it.
//!
//! Three properties are what this command is for, and each has a test that fails
//! if it stops holding: a Service belongs to one person and is refused to root,
//! an install that fails installs nothing, and a Purge never deletes a Profile
//! while something is still Switching Credentials into it.

mod common;

use common::*;
use perch::commands::watcher::WatcherCommand;
use perch::error::{EXIT_HELD, EXIT_INVALID, EXIT_NOTHING_TO_DO, EXIT_OK};
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{Execution, FakeHost, Platform};

/// Where a `systemd --user` unit goes on the fixture's machine.
const UNIT: &str = "/Users/someone/.config/systemd/user/perch-watch.service";

/// Where this platform keeps the unit, asked of the code under test rather than
/// spelled out.
///
/// `PathBuf::join` renders with `\` on Windows, so a hard-coded forward-slash
/// path matches the *file* — `FakeHost` normalizes what it stores — but not the
/// **argument** `launchctl` is handed, which is compared as a string. A fixture
/// that spelled it out therefore passed everywhere except the Windows runner,
/// which is the one place nobody is looking.
fn unit_at(host: &FakeHost) -> std::path::PathBuf {
    perch::service::unit_path(host)
        .expect("home is known")
        .expect("this platform keeps a unit file")
}

fn ran(host: &FakeHost) -> Vec<String> {
    host.effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::Exec { program, args } => Some(
                std::iter::once(program.clone())
                    .chain(args.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .collect()
}

fn worked() -> Execution {
    Execution {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn failed(saying: &str) -> Execution {
    Execution {
        status: 1,
        stdout: String::new(),
        stderr: saying.to_string(),
    }
}

/// A Linux machine whose `systemctl` answers, which is the ordinary case.
///
/// The platform is set after the fixture has built the machine, which is the
/// idiom the reconciling and importing suites already use: what `watched()`
/// arranges is Accounts and Groups, and none of that is platform-shaped.
fn linux() -> FakeHost {
    watched()
        .with_platform(Platform::Other)
        .with_exec("systemctl", &["--user", "daemon-reload"], worked())
        .with_exec(
            "systemctl",
            &["--user", "enable", "--now", "perch-watch.service"],
            worked(),
        )
        .with_exec(
            "systemctl",
            &["--user", "disable", "--now", "perch-watch.service"],
            worked(),
        )
}

/// A macOS machine whose `launchctl` answers. The fixture's own platform, so
/// nothing about the Credential Store shifts under a command that empties one —
/// which is why the Purge tests are here rather than on the Linux fixture.
fn mac() -> FakeHost {
    let host = watched().with_exec(
        "launchctl",
        &["bootout", "gui/501/cli.perch.watch"],
        worked(),
    );
    let plist = unit_at(&host);
    host.set_exec(
        "launchctl",
        &["bootstrap", "gui/501", &plist.to_string_lossy()],
        worked(),
    );
    host
}

/// The same machine, with `systemctl is-active` answering that it is running.
fn and_running(host: FakeHost) -> FakeHost {
    host.with_exec(
        "systemctl",
        &["--user", "is-active", "perch-watch.service"],
        worked(),
    )
}

fn run_service(host: &FakeHost, command: WatcherCommand) -> (perch::Result<i32>, String) {
    let mut written = Vec::new();
    let result = perch::commands::watcher::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// The whole arrangement, end to end: a unit where the platform keeps one, the
/// service manager told about it, and a sentence saying what happens from now
/// on.
#[test]
fn installing_writes_a_unit_and_starts_it_through_the_service_manager() {
    let host = linux();

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("the service manager answered"), EXIT_OK);
    assert!(host.path_exists(std::path::Path::new(UNIT)), "{printed}");

    let unit = host
        .read_file(std::path::Path::new(UNIT))
        .expect("the unit is readable");
    assert!(unit.contains("Type=simple"), "{unit}");
    assert!(
        unit.contains(" watcher run\n"),
        "it runs the ordinary loop: {unit}"
    );
    assert!(unit.contains("Restart=always"), "{unit}");
    assert!(unit.contains("WantedBy=default.target"), "{unit}");

    assert_eq!(
        ran(&host),
        vec![
            "systemctl --user daemon-reload",
            "systemctl --user enable --now perch-watch.service",
        ],
        "reloaded before it is asked about a unit that is new, and started now \
         rather than at the next login"
    );
    assert!(printed.contains("log in"), "{printed}");
}

/// **The property an install rests on.** A service manager that refuses leaves
/// a unit file behind that would start at the next login having never been
/// checked — so the file is taken back, and the command says the machine is
/// unchanged.
#[test]
fn an_install_the_service_manager_refuses_leaves_no_unit_behind() {
    let host = watched()
        .with_platform(Platform::Other)
        .with_exec("systemctl", &["--user", "daemon-reload"], worked())
        .with_exec(
            "systemctl",
            &["--user", "enable", "--now", "perch-watch.service"],
            failed("Failed to connect to bus: No medium found"),
        );

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("the service manager refused");
    assert!(
        refusal.to_string().contains("No medium found"),
        "what the service manager said is what the user needs: {refusal}"
    );
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "a unit nothing checked is not left where it would start at login"
    );
    assert!(
        refusal.to_string().contains("Nothing was installed"),
        "{refusal}"
    );
}

/// **A Service belongs to one person.** Every Profile is under a home
/// directory, so one installed under `sudo` would watch root's registry — which
/// is empty — while the person who typed it wondered why nothing switched.
#[test]
fn a_service_is_refused_to_root_rather_than_installed_for_the_wrong_person() {
    let host = linux().as_superuser();

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("root is not the person this would watch");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(refusal.to_string().contains("sudo"), "{refusal}");
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "and nothing was written"
    );
    assert!(ran(&host).is_empty(), "and nothing was run");
}

/// Re-running is the documented repair for a Service whose binary moved, which
/// an Upgrade does every time it routes through Homebrew or npm (ADR 0039).
#[test]
fn installing_twice_replaces_the_unit_rather_than_refusing() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("the first");

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("the second"), EXIT_OK);
    assert!(
        printed.contains("Replaced"),
        "it says which of the two it did: {printed}"
    );
    assert!(host.path_exists(std::path::Path::new(UNIT)));
}

#[test]
fn uninstalling_stops_the_service_and_takes_the_unit_back() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    let (result, printed) = run_service(&host, WatcherCommand::Uninstall);

    assert_eq!(result.expect("it came back"), EXIT_OK);
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "the unit is gone: {printed}"
    );
    assert!(
        ran(&host).contains(&"systemctl --user disable --now perch-watch.service".to_string()),
        "{:?}",
        ran(&host)
    );
    assert!(printed.contains("`perch watcher run`"), "{printed}");
}

/// The existing code for a request that was already true. A machine with no
/// Service is the machine an `uninstall` was asked to produce.
#[test]
fn uninstalling_what_was_never_installed_is_nothing_to_do_rather_than_a_failure() {
    let host = linux();

    let (result, printed) = run_service(&host, WatcherCommand::Uninstall);

    assert_eq!(result.expect("nothing failed"), EXIT_NOTHING_TO_DO);
    assert!(printed.contains("no Service installed"), "{printed}");
}

/// A question, and answering it is success either way — the bargain
/// `perch upgrade --check` already makes, which is why a script branches on
/// `--json` rather than on the code.
#[test]
fn status_succeeds_whether_or_not_anything_is_installed() {
    let host = linux();

    let (before, said) = run_service(&host, WatcherCommand::Status { json: false });
    assert_eq!(before.expect("a question"), EXIT_OK);
    assert!(said.contains("No Service is installed"), "{said}");

    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    let host = and_running(host);

    let (after, said) = run_service(&host, WatcherCommand::Status { json: false });
    assert_eq!(after.expect("still a question"), EXIT_OK);
    assert!(said.contains("is running"), "{said}");
}

/// The two questions a machine can answer differently, and the reason `status`
/// asks both: a Service that is installed and stopped, and a `perch watcher
/// run` somebody typed in a terminal, are different states with the same shape.
#[test]
fn status_tells_an_installed_service_apart_from_a_watcher_that_is_running() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    let (_, said) = run_service(&host, WatcherCommand::Status { json: true });
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");

    assert_eq!(reported["installed"], true);
    assert_eq!(
        reported["watching"], false,
        "nothing holds the watcher lock: {said}"
    );

    // Somebody takes the watcher lock — a `perch watcher run` in another
    // terminal, or the Service having got as far as starting.
    let _held = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("the lock is free");

    let (_, said) = run_service(&host, WatcherCommand::Status { json: true });
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");
    assert_eq!(
        reported["watching"], true,
        "the lock is what a Watcher holds, and what says one is running: {said}"
    );
}

/// A Service that names a binary an Upgrade has since moved is the failure
/// `status` exists to make visible — it comes up silently broken at the next
/// login otherwise.
#[test]
fn status_says_when_the_unit_names_a_binary_that_is_no_longer_there() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    // What `brew upgrade` does to a Cellar path, and what an npm install does to
    // a platform package: the binary the unit names stops existing.
    let unit = host
        .read_file(std::path::Path::new(UNIT))
        .expect("the unit is readable");
    let named = unit
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))
        .and_then(|line| line.strip_suffix(" watcher run"))
        .expect("the unit names a binary");
    host.remove_file(std::path::Path::new(named))
        .expect("the Upgrade moved it");

    let (_, said) = run_service(&host, WatcherCommand::Status { json: false });

    assert!(said.contains("not there any more"), "{said}");
    assert!(
        said.contains("perch watcher install"),
        "and the repair is one command: {said}"
    );
}

/// Said rather than refused, for the reason ADR 0013 gives about a Margin at or
/// above a Threshold: refusing would make the order two `perch config set`s are
/// typed in matter. A Service with no grant holds harmlessly and takes over the
/// moment one is given.
#[test]
fn installing_with_no_grant_anywhere_succeeds_and_says_the_service_will_hold() {
    let host = linux();
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("not a refusal"), EXIT_OK);
    assert!(printed.contains("watcher-may-act"), "{printed}");
    assert!(
        printed.contains("will hold"),
        "and it says what that means rather than only what is missing: {printed}"
    );
}

/// A grant said about the Ungrouped Accounts is not on its own enough for the
/// Watcher to act on them: `interchangeable` is the declaration that has to come
/// first, and without it every round holds (ADR 0017).
///
/// Read from `watcher-may-act` alone, this line stayed silent on a machine where
/// the Watcher can never act — telling somebody their Service was arranged when
/// it was one `perch config set` short of it. The same claim `perch group list`
/// was making until it took the gate on.
#[test]
fn a_grant_the_watcher_will_never_act_on_still_says_the_service_will_hold() {
    let host = linux();
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");
    // The only grant on the machine, and one the Watcher declines: nothing has
    // said the Accounts in no Group are interchangeable at all.
    config_set(&host, &["ungrouped", "watcher-may-act", "true"])
        .0
        .expect("the Ungrouped Accounts take the permission");

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("not a refusal"), EXIT_OK);
    assert!(
        printed.contains("will hold"),
        "a grant the Watcher will never act on is not a Service that will act: \
         {printed}"
    );
}

/// **The hazard a Purge exists to avoid, one level up.** A Watcher is the one
/// process that writes Credentials without being asked, and a supervised one
/// comes straight back — so it is stopped before anything is deleted, and the
/// consent covers it (ADR 0024's shape, ADR 0040's rule).
#[test]
fn a_purge_stops_the_service_before_it_deletes_anything() {
    // Declining the Export and then typing the word, which is what a Purge
    // actually asks for.
    let host = mac().with_answers(&["n", "purge"]);
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    let (result, printed) = run_purge(&host);

    result.expect("the Purge finished");
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "the unit goes with everything else: {printed}"
    );
    assert!(
        printed.contains("The Service goes too, and goes first"),
        "and the confirmation said so, because a unit lives outside Perch's \
         home and consent to one is not consent to the other: {printed}"
    );
    assert!(
        ran(&host).contains(&"launchctl bootout gui/501/cli.perch.watch".to_string()),
        "{:?}",
        ran(&host)
    );
}

/// And where it will not stop, the Purge refuses rather than deleting Profiles
/// underneath a process that is still Switching into them.
#[test]
fn a_purge_refuses_rather_than_deleting_under_a_service_that_will_not_stop() {
    let host = mac().with_answers(&["n", "purge"]);
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    // Told to stop, and still there afterwards: `launchctl print` answers, which
    // is how the machine says the Service is still loaded.
    let host = host
        .with_exec(
            "launchctl",
            &["bootout", "gui/501/cli.perch.watch"],
            failed("Operation not permitted"),
        )
        .with_exec("launchctl", &["print", "gui/501/cli.perch.watch"], worked());

    let (result, _) = run_purge(&host);

    let refusal = result.expect_err("nothing may be deleted underneath it");
    assert_eq!(
        refusal.exit_code(),
        EXIT_HELD,
        "nothing was changed, and trying again after stopping it works: {refusal}"
    );
    assert!(
        refusal.to_string().contains("perch watcher uninstall"),
        "{refusal}"
    );
    assert!(
        !registry_of(&host).accounts.is_empty(),
        "and every Account is still there"
    );
}

/// macOS keeps its unit somewhere else and is driven by something else, and the
/// same three properties hold. Asserted because the platform split is the whole
/// of what this feature is, and one arm of it going untested is one arm of it
/// being written from memory.
#[test]
fn a_mac_gets_a_launchagent_in_its_own_place_bootstrapped_into_its_own_session() {
    let host = mac();

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("launchctl answered"), EXIT_OK);
    let at = unit_at(&host);
    assert!(host.path_exists(&at), "{printed}");

    let plist = host.read_file(&at).expect("the plist is readable");
    assert!(plist.contains("<key>Label</key>"), "{plist}");
    assert!(plist.contains("cli.perch.watch"), "{plist}");
    assert!(
        plist.contains("<key>StandardOutPath</key>"),
        "launchd keeps no log of its own, so the unit names one: {plist}"
    );
    assert!(
        printed.contains("watch.log"),
        "and the install says where it is: {printed}"
    );
}

// ---- what an Upgrade owes a running Service -------------------------------

/// A machine whose newest published Release is newer than this build, installed
/// by Homebrew — the Channel `perch upgrade` hands the work to (ADR 0039).
fn upgradable() -> FakeHost {
    mac()
        .with_reply(
            perch::upgrade::LATEST_URL,
            200,
            r#"{"tag_name":"v999.0.0","name":"whatever"}"#,
        )
        .installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch")
}

fn upgrading(host: &FakeHost) -> (perch::Result<i32>, String) {
    let mut out = Vec::new();
    let outcome = perch::commands::upgrade::run(
        host,
        perch::commands::upgrade::UpgradeArgs::default(),
        &mut out,
    );
    (outcome, String::from_utf8(out).expect("it said text"))
}

/// **The failure A7 was written to prevent, arriving through a different door.**
///
/// `brew` and `npm` have never heard of a unit file, and on Unix the running
/// Service keeps the inode of the binary it started with — so an Upgrade nobody
/// followed up leaves a Service running yesterday's Perch until the next login.
/// The Upgrade re-points the unit and restarts it onto the new binary.
#[test]
fn an_upgrade_restarts_the_service_onto_the_binary_it_just_moved() {
    let host = upgradable();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("a Service is installed before the Upgrade");
    let before = ran(&host).len();

    let (outcome, said) = upgrading(&host);

    outcome.expect("the Upgrade ran");
    let after: Vec<String> = ran(&host).into_iter().skip(before).collect();
    assert!(
        after.contains(&format!(
            "launchctl bootstrap gui/501 {}",
            unit_at(&host).display()
        )),
        "the Service is started again, onto the binary that is there now: \
         {after:?}"
    );
    assert!(
        said.contains("The Service was restarted"),
        "and it says so, because a Service silently left on the old binary is \
         the thing this exists to prevent: {said}"
    );
}

/// The Upgrade is what succeeded; the Service is a follow-up. A refresh that
/// fails is a warning with a one-command repair rather than a reason to report
/// an Upgrade that did happen as one that did not.
#[test]
fn a_service_that_will_not_restart_is_a_warning_rather_than_a_failed_upgrade() {
    let host = upgradable();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("a Service is installed before the Upgrade");
    // The service manager stops answering between the install and the Upgrade,
    // which is what a `launchctl` refusing a GUI domain over SSH looks like.
    let plist = unit_at(&host);
    host.set_exec(
        "launchctl",
        &["bootstrap", "gui/501", &plist.to_string_lossy()],
        failed("Bootstrap failed: 5: Input/output error"),
    );

    let (outcome, said) = upgrading(&host);

    assert_eq!(
        outcome.expect("the binary really is newer, so the Upgrade succeeded"),
        EXIT_OK
    );
    assert!(said.contains("could not be restarted"), "{said}");
    assert!(
        said.contains("perch watcher install"),
        "and the repair is one command: {said}"
    );
}

/// Most machines have no Service, and an Upgrade on one must not go looking for
/// a service manager or say anything about one.
#[test]
fn an_upgrade_with_no_service_installed_says_nothing_about_one() {
    let host = upgradable();

    let (outcome, said) = upgrading(&host);

    outcome.expect("the Upgrade ran");
    assert!(!said.contains("Service"), "{said}");
    assert!(
        !ran(&host).iter().any(|line| line.starts_with("launchctl")),
        "nothing asked the service manager anything: {:?}",
        ran(&host)
    );
}

// ---- reading back what was written ----------------------------------------

/// **A round trip through Perch's own plist.** `status` reads the binary back
/// out of the installed unit rather than recomputing it, because the question it
/// answers is whether the unit and the machine have come apart — and a value
/// worked out again from the machine agrees with the machine by construction.
///
/// Both halves are hand-rolled: Perch writes the XML and Perch parses it. A path
/// holding an `&` is escaped on the way in and has to survive the way out, or
/// `status` reports a binary that is missing when it is not.
#[test]
fn the_binary_is_read_back_out_of_a_plist_that_had_to_be_escaped_to_write() {
    let awkward = "/Users/some & one/bin/perch";
    let host = mac()
        .with_file(awkward, "")
        .installed_at(awkward)
        .with_exec("launchctl", &["print", "gui/501/cli.perch.watch"], worked());

    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    let plist = host
        .read_file(&unit_at(&host))
        .expect("the plist is readable");
    assert!(
        plist.contains("/Users/some &amp; one/bin/perch"),
        "escaped on the way in: {plist}"
    );

    let (_, said) = run_service(&host, WatcherCommand::Status { json: true });
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");

    assert_eq!(
        reported["binary"], awkward,
        "and unescaped on the way out, or a path with an ampersand in it reads \
         as a binary that has gone: {said}"
    );
    assert_eq!(
        reported["binaryExists"], true,
        "which is what the answer turns on: {said}"
    );
}

/// The same question on the platform that keeps no file at all. Windows holds
/// the task itself, so there is nothing to read a binary back out of, and
/// `status` says what it knows rather than guessing.
#[test]
fn windows_keeps_the_task_itself_so_status_reads_no_unit_file() {
    let host = watched().with_platform(Platform::Windows).with_exec(
        "schtasks",
        &["/Query", "/TN", r"Perch\Watch"],
        worked(),
    );

    let (result, said) = run_service(&host, WatcherCommand::Status { json: true });

    assert_eq!(result.expect("a question"), EXIT_OK);
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");
    assert_eq!(
        reported["unit"],
        serde_json::Value::Null,
        "there is no file to name: {said}"
    );
    assert_eq!(
        reported["installed"], true,
        "and what is installed is what the task scheduler says is: {said}"
    );
    assert_eq!(
        reported["running"], false,
        "a registered task that has not fired is not a running one — and \
         `schtasks /Query` answers whether it exists, which is the question \
         `installed` already asked it: {said}"
    );
}

/// A registered task and a Watcher holding the watch is the whole of what
/// Windows can be asked, so it is what "running" is answered from there.
///
/// `schtasks /Query` succeeds whenever the task *exists*, which is the same
/// query `installed` is read off — so a `running` computed from it was true by
/// construction, and a logon task that had not fired since boot reported itself
/// as running to the prose and to the `--json` a script branches on.
#[test]
fn a_windows_task_is_running_when_a_watcher_is_actually_holding_the_watch() {
    let host = watched().with_platform(Platform::Windows).with_exec(
        "schtasks",
        &["/Query", "/TN", r"Perch\Watch"],
        worked(),
    );
    let _watching_alone = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody holds it yet");

    let (result, said) = run_service(&host, WatcherCommand::Status { json: true });

    assert_eq!(result.expect("a question"), EXIT_OK);
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");
    assert_eq!(reported["running"], true, "{said}");
    assert_eq!(reported["watching"], true, "{said}");
}

/// What the service manager said is what the user needs, and some of them say it
/// on standard output rather than standard error.
#[test]
fn a_service_manager_that_explains_itself_on_stdout_is_still_quoted() {
    let host = watched().with_platform(Platform::Other).with_exec(
        "systemctl",
        &["--user", "daemon-reload"],
        Execution {
            status: 1,
            stdout: "Failed to connect to bus: No such file or directory".to_string(),
            stderr: String::new(),
        },
    );

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("the service manager refused");
    assert!(
        refusal.to_string().contains("No such file or directory"),
        "a refusal that quoted an empty stderr would say nothing at all: \
         {refusal}"
    );
}

/// A machine Perch holds nothing on yet is one `watcher status` still answers
/// about — it is a question, and "no Service, and no registry either" is an
/// answer.
#[test]
fn status_answers_on_a_machine_perch_holds_nothing_on() {
    let host = FakeHost::new();

    let (result, said) = run_service(&host, WatcherCommand::Status { json: false });

    assert_eq!(result.expect("a question"), EXIT_OK);
    assert!(said.contains("No Service is installed"), "{said}");
    assert!(
        !said.contains("watcher-may-act"),
        "and it says nothing about a grant on a machine with no registry to \
         hold one: {said}"
    );
}

/// The prose `status` prints, which is what a person reads — the JSON tests
/// above assert the same facts for a script, and the two renderers can disagree.
#[test]
fn status_in_prose_names_the_binary_the_watcher_and_the_missing_grant() {
    // A binary that is actually there, so this exercises the arm that names it
    // rather than the one that reports it gone — which
    // `status_says_when_the_unit_names_a_binary_that_is_no_longer_there` covers.
    let host = mac()
        .with_file("/usr/local/bin/perch", "")
        .installed_at("/usr/local/bin/perch")
        .with_exec("launchctl", &["print", "gui/501/cli.perch.watch"], worked());
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");

    // Somebody is watching — a `perch watcher run` in another terminal, or the
    // Service having got as far as taking the lock.
    let _held = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("the lock is free");

    let (result, said) = run_service(&host, WatcherCommand::Status { json: false });

    assert_eq!(result.expect("a question"), EXIT_OK);
    assert!(said.contains("It runs "), "the binary it will run: {said}");
    assert!(
        said.contains("A Watcher is running on this machine"),
        "and that one is running, which is a different fact from the Service \
         being installed: {said}"
    );
    assert!(
        said.contains("watcher-may-act"),
        "and that nothing has told it it may act, so it will hold: {said}"
    );
}

/// A service manager that is not on `PATH` at all is a different failure from
/// one that refused, and the message has to say which — "no such program" on its
/// own reads as a Perch that is broken.
#[test]
fn a_service_manager_that_is_not_installed_says_so_rather_than_failing_blankly() {
    // No `with_exec` for `systemctl`, so the fake has no such program.
    let host = watched().with_platform(Platform::Other);

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("there is no systemctl here");
    assert!(
        refusal.to_string().contains("systemctl"),
        "it names the program: {refusal}"
    );
    assert!(
        refusal.to_string().contains("PATH"),
        "and says where to look for it: {refusal}"
    );
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "and the unit it had written is taken back"
    );
}

/// The Service writes its decisions into a file inside Perch's home, and the
/// install is what has to put that directory there.
///
/// Perch's home is made on the way to the first lock Perch takes, and an
/// install takes none — so on a machine where Claude Code is logged in and
/// Perch has never run, the directory the unit points its output at simply was
/// not there. `cmd /c … >> "…\watch.log"` cannot open a redirect into a
/// directory that does not exist, so the Windows task failed at every logon
/// without saying anything, and launchd cannot open a `StandardOutPath` there
/// either.
#[test]
fn installing_on_a_machine_perch_has_never_run_on_makes_room_for_the_log() {
    let host = logged_in_machine().with_exec(
        "launchctl",
        &["bootout", "gui/501/cli.perch.watch"],
        worked(),
    );
    let plist = unit_at(&host);
    host.set_exec(
        "launchctl",
        &["bootstrap", "gui/501", &plist.to_string_lossy()],
        worked(),
    );
    let log = perch::service::log_path(&host)
        .expect("home is known")
        .expect("this platform keeps its own log");
    assert!(
        !host.path_exists(log.parent().expect("the log is inside Perch's home")),
        "the fixture's premise: nothing has made Perch's home yet"
    );

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    result.expect("the install works on a machine Perch has never run on");
    assert!(
        host.path_exists(log.parent().expect("the log is inside Perch's home")),
        "the directory the unit points its output at is there: {printed}"
    );
}
