//! Behaviour: `perch service` — having the machine run the Watcher for you.
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
use perch::commands::service::ServiceCommand;
use perch::error::{EXIT_HELD, EXIT_INVALID, EXIT_NOTHING_TO_DO, EXIT_OK};
use perch::host::fake::Effect;
use perch::host::{Execution, FakeHost, Host, Platform};

/// Where a `systemd --user` unit goes on the fixture's machine.
const UNIT: &str = "/Users/someone/.config/systemd/user/perch-watch.service";

/// And where a LaunchAgent does.
const PLIST: &str = "/Users/someone/Library/LaunchAgents/cli.perch.watch.plist";

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
    watched()
        .with_exec(
            "launchctl",
            &["bootout", "gui/501/cli.perch.watch"],
            worked(),
        )
        .with_exec("launchctl", &["bootstrap", "gui/501", PLIST], worked())
}

/// The same machine, with `systemctl is-active` answering that it is running.
fn and_running(host: FakeHost) -> FakeHost {
    host.with_exec(
        "systemctl",
        &["--user", "is-active", "perch-watch.service"],
        worked(),
    )
}

fn run_service(host: &FakeHost, command: ServiceCommand) -> (perch::Result<i32>, String) {
    let mut written = Vec::new();
    let result = perch::commands::service::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// The whole arrangement, end to end: a unit where the platform keeps one, the
/// service manager told about it, and a sentence saying what happens from now
/// on.
#[test]
fn installing_writes_a_unit_and_starts_it_through_the_service_manager() {
    let host = linux();

    let (result, printed) = run_service(&host, ServiceCommand::Install);

    assert_eq!(result.expect("the service manager answered"), EXIT_OK);
    assert!(host.path_exists(std::path::Path::new(UNIT)), "{printed}");

    let unit = host
        .read_file(std::path::Path::new(UNIT))
        .expect("the unit is readable");
    assert!(unit.contains("Type=simple"), "{unit}");
    assert!(
        unit.contains(" watch\n"),
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

    let (result, _) = run_service(&host, ServiceCommand::Install);

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

    let (result, _) = run_service(&host, ServiceCommand::Install);

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
    run_service(&host, ServiceCommand::Install)
        .0
        .expect("the first");

    let (result, printed) = run_service(&host, ServiceCommand::Install);

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
    run_service(&host, ServiceCommand::Install)
        .0
        .expect("installed");

    let (result, printed) = run_service(&host, ServiceCommand::Uninstall);

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
    assert!(printed.contains("`perch watch`"), "{printed}");
}

/// The existing code for a request that was already true. A machine with no
/// Service is the machine an `uninstall` was asked to produce.
#[test]
fn uninstalling_what_was_never_installed_is_nothing_to_do_rather_than_a_failure() {
    let host = linux();

    let (result, printed) = run_service(&host, ServiceCommand::Uninstall);

    assert_eq!(result.expect("nothing failed"), EXIT_NOTHING_TO_DO);
    assert!(printed.contains("no Service installed"), "{printed}");
}

/// A question, and answering it is success either way — the bargain
/// `perch upgrade --check` already makes, which is why a script branches on
/// `--json` rather than on the code.
#[test]
fn status_succeeds_whether_or_not_anything_is_installed() {
    let host = linux();

    let (before, said) = run_service(&host, ServiceCommand::Status { json: false });
    assert_eq!(before.expect("a question"), EXIT_OK);
    assert!(said.contains("No Service is installed"), "{said}");

    run_service(&host, ServiceCommand::Install)
        .0
        .expect("installed");
    let host = and_running(host);

    let (after, said) = run_service(&host, ServiceCommand::Status { json: false });
    assert_eq!(after.expect("still a question"), EXIT_OK);
    assert!(said.contains("is running"), "{said}");
}

/// The two questions a machine can answer differently, and the reason `status`
/// asks both: a Service that is installed and stopped, and a `perch watch`
/// somebody typed in a terminal, are different states with the same shape.
#[test]
fn status_tells_an_installed_service_apart_from_a_watcher_that_is_running() {
    let host = linux();
    run_service(&host, ServiceCommand::Install)
        .0
        .expect("installed");

    let (_, said) = run_service(&host, ServiceCommand::Status { json: true });
    let reported: serde_json::Value = serde_json::from_str(&said).expect("it is JSON");

    assert_eq!(reported["installed"], true);
    assert_eq!(
        reported["watching"], false,
        "nothing holds the watcher lock: {said}"
    );

    // Somebody takes the watcher lock — a `perch watch` in another terminal, or
    // the Service having got as far as starting.
    let _held = perch::lock::take_all(
        &host,
        vec![perch::registry::watcher_lock_spec(&host).expect("home is known")],
    )
    .expect("the lock is free");

    let (_, said) = run_service(&host, ServiceCommand::Status { json: true });
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
    run_service(&host, ServiceCommand::Install)
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
        .and_then(|line| line.strip_suffix(" watch"))
        .expect("the unit names a binary");
    host.remove_file(std::path::Path::new(named))
        .expect("the Upgrade moved it");

    let (_, said) = run_service(&host, ServiceCommand::Status { json: false });

    assert!(said.contains("not there any more"), "{said}");
    assert!(
        said.contains("perch service install"),
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

    let (result, printed) = run_service(&host, ServiceCommand::Install);

    assert_eq!(result.expect("not a refusal"), EXIT_OK);
    assert!(printed.contains("watcher-may-act"), "{printed}");
    assert!(
        printed.contains("will hold"),
        "and it says what that means rather than only what is missing: {printed}"
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
    run_service(&host, ServiceCommand::Install)
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
    run_service(&host, ServiceCommand::Install)
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
        refusal.to_string().contains("perch service uninstall"),
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

    let (result, printed) = run_service(&host, ServiceCommand::Install);

    assert_eq!(result.expect("launchctl answered"), EXIT_OK);
    assert!(host.path_exists(std::path::Path::new(PLIST)), "{printed}");

    let plist = host
        .read_file(std::path::Path::new(PLIST))
        .expect("the plist is readable");
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
