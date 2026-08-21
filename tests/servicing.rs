//! Behavior: `perch watcher install`, `uninstall` and `status` — having the machine run
//! the Watcher for you.
//!
//! The unit files themselves are argued with in `src/service.rs`'s unit tests. What is
//! asserted here is the part that touches a machine, and what a Purge and an Upgrade
//! owe a running Service (ADR the-machine-runs-the-watcher).
//!
//! Three properties, each with a test that fails if it stops holding: a Service is
//! refused to root, an install that fails installs nothing, and a Purge never deletes a
//! Profile while something is still Switching Credentials into it.

mod common;

use common::*;
use perch::commands::watcher::WatcherCommand;
use perch::error::{EXIT_HELD, EXIT_INVALID, EXIT_NOTHING_TO_DO, EXIT_OK};
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{Execution, FakeHost, Platform};

/// Where a `systemd --user` unit goes on the fixture's machine.
const UNIT: &str = "/Users/someone/.config/systemd/user/perch-watch.service";

/// Where this platform keeps the unit, asked of the code under test.
///
/// `PathBuf::join` renders with `\` on Windows, so a hard-coded forward-slash path
/// matches the *file* — `FakeHost` normalizes what it stores — but not the **argument**
/// `launchctl` is handed, which is compared as a string.
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
/// The platform is set after the fixture has built the machine, which is the idiom the
/// reconciling and importing suites already use: what `watched()` arranges is Accounts
/// and Groups, and none of that is platform-shaped.
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

/// A macOS machine whose `launchctl` answers. The fixture's own platform, so nothing
/// about the Credential Store shifts under a command that empties one — which is why
/// the Purge tests are here rather than on the Linux fixture.
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
            "systemctl --user restart perch-watch.service",
        ],
        "reloaded before it is asked about a unit that is new, started now \
         rather than at the next login — and restarted, because `enable --now` \
         starts a stopped unit and does nothing to a running one, which is the \
         state re-installing after an Upgrade is always in"
    );
    // Asserted whole, because the claim is the sentence (ADR perch-says-what-it-did):
    // what it did, and which of this machine's binaries the unit was written against.
    assert!(
        printed.contains(&format!(
            "Installed the Service. It runs {} as a systemd user unit.",
            host.current_exe().expect("the fixture has one").display()
        )),
        "{printed}"
    );
}

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

#[test]
fn a_carried_value_that_would_write_its_own_directive_is_refused_before_anything_is_installed() {
    let host = linux().with_env("PERCH_HOME", "/tmp/perch\nExecStartPre=/bin/sh -c evil");

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("a value no unit can hold is not installable");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal.to_string().contains("PERCH_HOME"),
        "it names which value: {refusal}"
    );
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "and nothing was written at all"
    );
    assert!(ran(&host).is_empty(), "and nothing was run");
}

#[test]
fn an_upgrade_refuses_to_rewrite_the_unit_with_a_value_no_unit_can_hold() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("a Service is installed before the Upgrade");
    let installed = host.read_file(std::path::Path::new(UNIT)).expect("a unit");
    // The environment changes between the install and the Upgrade, which is what a
    // `PERCH_HOME` exported in a shell profile after the fact looks like.
    let host = host
        .with_env("PERCH_HOME", "/tmp/perch\nExecStartPre=/bin/sh -c evil")
        .with_reply(
            perch::upgrade::LATEST_URL,
            200,
            r#"{"tag_name":"v999.0.0","name":"whatever"}"#,
        )
        .with_file("/usr/bin/npm", "");

    let mut out = Vec::new();
    let outcome = perch::commands::upgrade::run(
        &host,
        perch::commands::upgrade::UpgradeArgs {
            channel: Some("npm".to_string()),
            ..perch::commands::upgrade::UpgradeArgs::default()
        },
        &mut out,
    );
    let said = String::from_utf8(out).expect("it said text");

    outcome.expect("the Upgrade itself succeeded — the binary really is newer");
    assert_eq!(
        host.read_file(std::path::Path::new(UNIT)).as_deref().ok(),
        Some(installed.as_str()),
        "the unit on disk is untouched: a value no format can hold is refused \
         before anything is written, whichever door asked"
    );
    assert!(
        said.contains("could not be restarted") && said.contains("perch watcher install"),
        "and it is a warning with a one-command repair rather than a silent \
         rewrite: {said}"
    );
}

#[test]
fn a_path_a_shell_would_split_survives_the_unit_it_is_written_into() {
    for awkward in [
        "/opt/My Tools/perch",
        "/opt/100%/perch",
        "/opt/say \"hi\"/perch",
    ] {
        let unit = perch::service::Unit {
            binary: std::path::PathBuf::from(awkward),
            environment: Vec::new(),
            log: None,
            user_id: None,
            user_name: None,
        };
        let text = unit
            .rendered(Platform::Other)
            .expect("Linux keeps a unit file");

        assert_eq!(
            perch::service::binary_in(Platform::Other, &text).as_deref(),
            Some(std::path::Path::new(awkward)),
            "the path written is the path read back: {text}"
        );
    }
}

#[test]
fn a_carried_value_a_scheduled_task_cannot_quote_is_refused_on_windows() {
    for hostile in ["x\" && evil.exe && set \"y=", "%APPDATA%"] {
        let host = watched()
            .with_platform(Platform::Windows)
            .with_env("PERCH_HOME", hostile);

        let (result, _) = run_service(&host, WatcherCommand::Install);

        let refusal = result.expect_err("a Scheduled Task cannot hold this");
        assert_eq!(refusal.exit_code(), EXIT_INVALID);
        // Whichever value it reaches first — the log path is derived from `PERCH_HOME`,
        // so it carries the same character — and why.
        assert!(
            refusal.to_string().contains("Scheduled Task"),
            "it says what cannot hold it: {refusal}"
        );
        assert!(
            ran(&host).is_empty(),
            "and nothing was registered: {refusal}"
        );
    }
}

#[test]
fn installing_on_windows_starts_the_task_rather_than_only_registering_it() {
    let host = watched().with_platform(Platform::Windows);
    // `schtasks /Create` carries a command built out of this machine's own paths, so
    // the only honest way to arrange an answer for it is to let the install say what it
    // would run — the same trick the test above uses.
    let _ = run_service(&host, WatcherCommand::Install);
    for effect in host.effects() {
        if let Effect::Exec { program, args } = effect {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            host.set_exec(&program, &args, worked());
        }
    }
    host.forget_effects();

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    result.expect("the service manager answered");
    // The `/Query` that asks whether one is already registered comes first and is not
    // part of the install; these three are.
    let driven = ran(&host);
    let registered = driven
        .iter()
        .position(|line| line.contains("/Create"))
        .expect("it registers the task");
    assert_eq!(
        &driven[registered + 1..],
        [
            r"schtasks /End /TN Perch\Watch".to_string(),
            r"schtasks /Run /TN Perch\Watch".to_string(),
        ],
        "{driven:?} — {printed}"
    );
}

#[test]
fn a_unit_in_a_shape_perch_does_not_write_names_no_binary() {
    for foreign in [
        "[Service]\nExecStart=/usr/local/bin/perch watcher run\n",
        "[Service]\nExecStart=\"/usr/local/bin/%h/perch\" watcher run\n",
        "[Service]\nExecStart=\"/usr/local/bin/perch watcher run\n",
    ] {
        assert_eq!(
            perch::service::binary_in(Platform::Other, foreign),
            None,
            "not a unit this writer produced: {foreign}"
        );
    }
}

#[test]
fn a_start_that_fails_over_a_service_that_was_working_leaves_the_unit_where_it_is() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("the first install works");
    // The same machine, with the service manager no longer answering — an SSH session
    // with no user bus is the ordinary way to reach this.
    let host = host.with_exec(
        "systemctl",
        &["--user", "enable", "--now", "perch-watch.service"],
        failed("Failed to connect to bus: No medium found"),
    );

    let (result, _) = run_service(&host, WatcherCommand::Install);

    let refusal = result.expect_err("the service manager refused");
    assert!(
        host.path_exists(std::path::Path::new(UNIT)),
        "the Service that was working is still installed: {refusal}"
    );
    assert!(
        !refusal.to_string().contains("Perch is unchanged"),
        "and nothing claims otherwise: {refusal}"
    );
    assert!(
        refusal.to_string().contains("perch watcher status"),
        "and it says how to see what is there now: {refusal}"
    );
}

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
fn installing_twice_on_windows_says_it_replaced_what_was_already_registered() {
    let host = watched().with_platform(Platform::Windows).with_exec(
        "schtasks",
        &["/Query", "/TN", r"Perch\Watch"],
        worked(),
    );
    // `schtasks /Create` carries a command built out of this machine's own paths, so
    // the only way to arrange an answer is to let the install say what it would run:
    // the first fails for want of exactly that.
    let _ = run_service(&host, WatcherCommand::Install);
    for effect in host.effects() {
        if let Effect::Exec { program, args } = effect {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            host.set_exec(&program, &args, worked());
        }
    }

    let (result, printed) = run_service(&host, WatcherCommand::Install);

    assert_eq!(result.expect("the task scheduler answered"), EXIT_OK);
    assert!(
        printed.contains("Replaced"),
        "a task that is already registered is replaced, and Windows can be \
         asked whether one is: {printed}"
    );
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
    // The whole of what it says, and nothing about what stops starting at login or
    // about a terminal that was never affected.
    assert!(
        printed.contains("The Service is stopped and its unit is gone."),
        "{printed}"
    );
}

#[test]
fn uninstalling_what_was_never_installed_is_nothing_to_do_rather_than_a_failure() {
    let host = linux();

    let (result, printed) = run_service(&host, WatcherCommand::Uninstall);

    assert_eq!(result.expect("nothing failed"), EXIT_NOTHING_TO_DO);
    assert!(printed.contains("no Service installed"), "{printed}");
}

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

    // Somebody takes the watcher lock — a `perch watcher run` in another terminal, or
    // the Service having got as far as starting.
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

#[test]
fn status_asks_whether_a_watcher_holds_the_lock_rather_than_waiting_for_it() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    let _held = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("the lock is free");
    host.forget_effects();

    let (_, said) = run_service(&host, WatcherCommand::Status { json: true });

    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");
    assert_eq!(reported["watching"], true, "{said}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Slept { .. })),
        "and nothing waited on it: {:?}",
        host.effects()
    );
}

#[test]
fn status_says_when_the_unit_names_a_binary_that_is_no_longer_there() {
    let host = linux();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    // What `brew upgrade` does to a Cellar path, and what an npm install does to a
    // platform package: the binary the unit names stops existing.
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

#[test]
fn status_says_where_the_installed_service_actually_writes_its_log() {
    let host = mac().with_env("PERCH_HOME", "/Users/someone/elsewhere");
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    // A `PERCH_HOME` exported in a shell profile after the fact, or a `status` run from
    // a terminal that never had it set. The Service goes on writing where it was
    // installed to write either way.
    let host = host.without_env("PERCH_HOME");

    let (_, said) = run_service(&host, WatcherCommand::Status { json: false });

    // Joined rather than spelled out, because `service::log_path` joins and the
    // separator it joins with is the *running* machine's — a `\` under the Windows
    // runner, whatever platform the fixture reports.
    let at = std::path::Path::new("/Users/someone/elsewhere").join("watch.log");
    assert!(
        said.contains(&*at.to_string_lossy()),
        "the log the unit names, not the one the environment would derive: {said}"
    );
}

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

#[test]
fn a_watcher_lock_that_will_not_be_taken_at_all_is_not_a_watcher_that_is_running() {
    let spec = perch::registry::watcher_lock_spec(&linux()).expect("home is known");
    let host = linux().with_unwritable_file(&spec.dir, "the filesystem said no");
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");

    let (_, said) = run_service(&host, WatcherCommand::Status { json: true });
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");

    assert_eq!(
        reported["watching"], false,
        "a lock that would not be taken is not a lock somebody is holding: {said}"
    );
    assert!(
        !said.contains("A Watcher is running on this machine"),
        "and nothing claims one is: {said}"
    );
}

#[test]
fn a_grant_the_watcher_will_never_act_on_still_says_the_service_will_hold() {
    let host = linux();
    config_set(&host, &["work", "watcher-may-act", "false"])
        .0
        .expect("the Group takes the permission back");
    // The only grant on the machine, and one the Watcher declines: nothing has said the
    // Accounts in no Group are interchangeable at all.
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

#[test]
fn a_purge_stops_the_service_before_it_deletes_anything() {
    // Declining the Export and then typing the word, which is what a Purge actually
    // asks for.
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

#[test]
fn a_purge_refuses_rather_than_deleting_under_a_service_that_will_not_stop() {
    let host = mac().with_answers(&["n", "purge"]);
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    // Told to stop, and still there afterwards: `launchctl print` answers, which is how
    // the machine says the Service is still loaded.
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

#[test]
fn a_purge_refuses_while_a_watcher_still_holds_the_watch() {
    let host = mac().with_answers(&["n", "purge"]);
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("installed");
    // The Service stops cleanly and the machine says so — and a Watcher is still
    // holding the watch, which is the state the unit cannot report.
    let host = host.with_exec(
        "launchctl",
        &["print", "gui/501/cli.perch.watch"],
        failed("Could not find service"),
    );
    let _still_watching = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody else holds it");

    let (result, _) = run_purge(&host);

    let refusal = result.expect_err("nothing may be deleted underneath a Watcher");
    assert_eq!(refusal.exit_code(), EXIT_HELD, "{refusal}");
    assert!(
        !registry_of(&host).accounts.is_empty(),
        "and every Account is still there"
    );
}

#[test]
fn stopping_a_windows_task_ends_the_running_instance_before_unregistering_it() {
    let driven: Vec<String> = perch::service::stopping(Platform::Windows, None)
        .iter()
        .map(|step| format!("{} {}", step.program, step.args.join(" ")))
        .collect();

    assert_eq!(
        driven,
        [
            r"schtasks /End /TN Perch\Watch",
            r"schtasks /Delete /TN Perch\Watch /F",
        ],
    );
}

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

/// A machine whose newest published Release is newer than this build, installed by
/// Homebrew — the Channel `perch upgrade` hands the work to
/// (ADR an-upgrade-asks-its-channel).
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

#[test]
fn a_service_that_will_not_restart_is_a_warning_rather_than_a_failed_upgrade() {
    let host = upgradable();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("a Service is installed before the Upgrade");
    // The service manager stops answering between the install and the Upgrade, which is
    // what a `launchctl` refusing a GUI domain over SSH looks like.
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

#[test]
fn a_channel_that_refused_the_upgrade_leaves_the_service_where_it_is() {
    let host = upgradable();
    run_service(&host, WatcherCommand::Install)
        .0
        .expect("a Service is installed before the Upgrade");
    host.set_exec(
        "/opt/homebrew/bin/brew",
        &["upgrade", "perch"],
        failed("Error: perch is pinned"),
    );
    let before = ran(&host).len();

    let (outcome, said) = upgrading(&host);

    assert_eq!(
        outcome.expect("`brew` answered, and what it answered was a refusal"),
        1
    );
    let after: Vec<String> = ran(&host).into_iter().skip(before).collect();
    assert!(
        !after.iter().any(|line| line.starts_with("launchctl")),
        "nothing was said to the service manager about a binary that did not \
         move: {after:?}"
    );
    assert!(
        !said.contains("Service"),
        "and nothing claimed an Upgrade that did not happen: {said}"
    );
}

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
        reported["binary_exists"], true,
        "which is what the answer turns on: {said}"
    );
}

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

#[test]
fn status_in_prose_names_the_binary_the_watcher_and_the_missing_grant() {
    // A binary that is actually there, so this exercises the arm that names it rather
    // than the one that reports it gone — which
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

    // Somebody is watching — a `perch watcher run` in another terminal, or the Service
    // having got as far as taking the lock.
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

#[test]
fn a_purge_refuses_under_a_watcher_run_from_a_terminal_with_no_service_installed() {
    let host = mac().with_answers(&["n", "purge"]);
    // No `perch watcher install` — this machine has never had one.
    let _still_watching = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("nobody else holds it");

    let (result, printed) = run_purge(&host);

    let refusal = result.expect_err("nothing may be deleted underneath a Watcher");
    assert_eq!(refusal.exit_code(), EXIT_HELD, "{refusal}");
    assert!(
        refusal.to_string().contains("Watcher is running"),
        "and it names what is running rather than a Service nobody installed: {refusal}"
    );
    assert!(
        perch::registry::load(&host)
            .expect("whatever is there is readable")
            .is_some(),
        "nothing was purged: {printed}"
    );
}

#[test]
fn windows_runs_the_schtasks_that_ships_with_windows_and_not_whichever_is_nearest() {
    let system32 = r"C:\Windows\System32\schtasks.exe";
    let host = watched()
        .with_platform(Platform::Windows)
        .with_env("SystemRoot", r"C:\Windows")
        // Arranged under the absolute name alone. A run that reached for the bare one
        // finds nothing arranged and fails, which is the assertion.
        .with_exec(system32, &["/Query", "/TN", r"Perch\Watch"], worked());

    let (result, said) = run_service(&host, WatcherCommand::Status { json: true });

    result.expect("a question");
    let ran: Vec<String> = host
        .effects()
        .iter()
        .filter_map(|effect| match effect {
            perch::host::fake::Effect::Exec { program, .. } => Some(program.clone()),
            _ => None,
        })
        .collect();

    assert!(
        ran.contains(&system32.to_string()),
        "the binary is spelled out, so the working directory is not searched \
         first: {ran:?}\n{said}"
    );
    assert!(
        !ran.iter().any(|program| program == "schtasks"),
        "and the bare name is not used anywhere: {ran:?}"
    );
}
