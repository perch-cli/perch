//! Behavior tests for `perch run <target>`: a client against one Account's
//! Profile, and nothing else moved (ADR a-run-is-one-shot).
//!
//! What every one of these is really asserting is the difference between a Run
//! and a Switch. A Switch is about the whole machine; a Run is about one
//! process, and the things it leaves alone are the feature rather than a
//! side-effect.

mod common;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{TimeZone, Utc};
use common::*;
use perch::commands::add::AddArgs;
use perch::error::{EXIT_INVALID, EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE, EXIT_QUARANTINED};
use perch::host::FakeHost;
use perch::host::PRIVATE_DIR_MODE;
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::prelude::*;

/// The Default Profile: where Shared State lives, and what a Run has to
/// Reconcile out of.
const SHARED: &str = "/Users/someone/.claude";

/// A Profile's path is hashed into a keychain service name, so the string the
/// client is handed has to be the string the Credential Store was derived from.
/// A `PERCH_HOME` holding `//` is enough to make those two different, and then
/// Claude Code reads a namespace Perch never writes.
#[test]
fn the_client_is_pointed_at_the_directory_the_credential_store_was_derived_from() {
    let host = machine_with_claude_code()
        .with_env("PERCH_HOME", "/Users/someone//elsewhere")
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(IDENTITY_PATH, IDENTITY_FILE)
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");
    host.forget_effects();

    let (outcome, _) = run_run(&host, SECOND_EMAIL);

    outcome.expect("the client ran");
    let store = perch::probe::store_for_profile(&host, &profile_of(&host, SECOND_EMAIL))
        .expect("its Credential Store");
    // As text, which is the whole point: `PathBuf` compares by component, so a
    // `//` and a `/` are one path to it and two keychain service names to
    // `short_hash`, which hashes the string.
    let told: Vec<String> = launched(&host)
        .into_iter()
        .map(|(_, at)| at.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        told,
        vec![store.config_dir.to_string_lossy().into_owned()],
        "the client is told the spelling the store was derived from"
    );
}

/// A machine holding two Accounts, with a client that exits cleanly standing in
/// for the Claude Code a Run launches. The Accounts arrived by login, so the
/// effects of arranging them are forgotten before the Run under test.
fn machine() -> FakeHost {
    let host = machine_with_two_accounts().with_login(client_exiting(0));
    host.forget_effects();
    host
}

/// The same, with the spread of Shared State a real Default Profile holds.
fn machine_with_shared_state() -> FakeHost {
    let host = machine_with_two_accounts()
        .with_login(client_exiting(0))
        .with_file(shared("CLAUDE.md"), "remember this")
        .with_file(shared("settings.json"), r#"{"theme":"dark"}"#)
        .with_file(shared("plugins/config.json"), "{}");
    host.forget_effects();
    host
}

fn shared(entry: &str) -> String {
    format!("{SHARED}/{entry}")
}

/// Where an Account's Profile is, derived the way every command derives it.
fn profile_of(host: &FakeHost, email: &str) -> PathBuf {
    perch::registry::profile_dir_for(host, email).expect("home is known")
}

/// The same, spelled for the fixtures that take a path as a string.
fn profile_string(email: &str) -> String {
    format!(
        "/Users/someone/.config/perch/profiles/{}",
        perch::registry::slug(email)
    )
}

/// Every client a Run launched, as the program and the config directory it was
/// pointed at.
fn launched(host: &FakeHost) -> Vec<(String, PathBuf)> {
    host.effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::ExecInteractive {
                program,
                config_dir,
                ..
            } => Some((program, config_dir)),
            _ => None,
        })
        .collect()
}

/// The same, as the whole command line each Run launched: the program and every
/// word it was handed.
fn command_lines(host: &FakeHost) -> Vec<Vec<String>> {
    host.effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::ExecInteractive { program, args, .. } => {
                Some(std::iter::once(program).chain(args).collect())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_run_launches_claude_code_against_that_accounts_profile() {
    let host = machine();

    let (outcome, _) = run_run(&host, SECOND_EMAIL);

    assert_eq!(outcome.expect("the client ran"), 0);
    assert_eq!(
        launched(&host),
        vec![(CLAUDE_BIN.to_string(), profile_of(&host, SECOND_EMAIL))],
        "one client, pointed at the Profile of the Account named"
    );
}

#[test]
fn the_active_account_and_the_default_profile_are_untouched() {
    let host = machine();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the live Credential is still the active Account's"
    );
    assert_eq!(host.file(IDENTITY_PATH).as_deref(), Some(IDENTITY_FILE));

    let into_the_default_profile: Vec<Effect> = host
        .effects()
        .into_iter()
        .filter(|effect| match effect {
            Effect::KeychainSet { service, .. } | Effect::KeychainDelete { service, .. } => {
                service == DEFAULT_SERVICE
            }
            Effect::WroteFile(path) | Effect::WrotePrivateFile(path) => {
                path.starts_with(SHARED) || path.as_os_str() == IDENTITY_PATH
            }
            _ => false,
        })
        .collect();
    assert!(
        into_the_default_profile.is_empty(),
        "{into_the_default_profile:?}"
    );
}

#[test]
fn shared_state_is_reconciled_before_the_client_is_launched() {
    let host = machine_with_shared_state();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let profile = profile_of(&host, SECOND_EMAIL);
    for entry in ["CLAUDE.md", "settings.json", "plugins"] {
        assert_eq!(
            host.link_at(profile.join(entry)).map(|(_, target)| target),
            Some(PathBuf::from(shared(entry))),
            "{entry} is reachable from the Profile the Run launches"
        );
    }

    let effects = host.effects();
    let last_link = effects
        .iter()
        .rposition(|effect| matches!(effect, Effect::Linked { .. }))
        .expect("Shared State was linked");
    let launch = effects
        .iter()
        .position(|effect| matches!(effect, Effect::ExecInteractive { .. }))
        .expect("a client was launched");
    assert!(
        last_link < launch,
        "every link is in place before the client that reads them"
    );
}

#[test]
fn shared_state_is_read_from_the_default_profile_even_inside_another_run() {
    let host = machine_with_shared_state()
        .with_env("CLAUDE_CONFIG_DIR", &profile_string(EMAIL))
        .with_file(
            format!("{}/CLAUDE.md", profile_string(EMAIL)),
            "a link into the Default Profile, not the person's memory",
        );

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let profile = profile_of(&host, SECOND_EMAIL);
    for entry in ["CLAUDE.md", "settings.json", "plugins"] {
        assert_eq!(
            host.link_at(profile.join(entry)).map(|(_, target)| target),
            Some(PathBuf::from(shared(entry))),
            "{entry} comes from the Default Profile rather than from a Profile"
        );
    }
}

#[test]
fn a_configuration_directory_that_is_not_a_profile_is_where_shared_state_is_read() {
    let moved = "/Users/someone/elsewhere";
    let host = machine()
        .with_env("CLAUDE_CONFIG_DIR", moved)
        .with_file(format!("{moved}/CLAUDE.md"), "the memory they actually use");

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        host.link_at(profile_of(&host, SECOND_EMAIL).join("CLAUDE.md"))
            .map(|(_, target)| target),
        Some(PathBuf::from(format!("{moved}/CLAUDE.md")))
    );
}

#[test]
fn a_run_that_cannot_reconcile_does_not_launch() {
    let host = machine_with_shared_state();
    host.set_file(
        profile_of(&host, SECOND_EMAIL).join("CLAUDE.md"),
        "something else put this here",
    );

    let refusal = run_run(&host, SECOND_EMAIL)
        .0
        .expect_err("the Run does not launch");

    let said = refusal.to_string();
    assert!(said.contains("`CLAUDE.md`"), "{said}");
    assert!(said.contains("is not a link Perch made"), "{said}");
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

#[test]
fn a_second_run_works_while_the_first_client_is_still_running() {
    let inside = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&inside);
    let first = Cell::new(true);

    let host = machine_with_two_accounts().with_login(move |host: &FakeHost, _dir: &_| {
        if first.replace(false) {
            let outcome = run_run(host, EMAIL).0;
            *recorded.borrow_mut() = Some(outcome);
        }
        0
    });
    host.forget_effects();

    let outer = run_run(&host, SECOND_EMAIL)
        .0
        .expect("the first client ran");

    assert_eq!(outer, 0);
    assert_eq!(
        inside
            .borrow_mut()
            .take()
            .expect("a second Run happened while the first client was running")
            .expect("the second client ran too"),
        0
    );
    assert_eq!(
        launched(&host),
        vec![
            (CLAUDE_BIN.to_string(), profile_of(&host, SECOND_EMAIL)),
            (CLAUDE_BIN.to_string(), profile_of(&host, EMAIL)),
        ],
        "each Run is pointed at its own Account's Profile"
    );
}

#[test]
fn a_quarantined_account_is_refused_and_nothing_is_launched() {
    let host = machine();
    quarantine(&host, SECOND_EMAIL);

    let refusal = run_run(&host, SECOND_EMAIL)
        .0
        .expect_err("a Run that could not authenticate is refused");

    assert_eq!(refusal.exit_code(), EXIT_QUARANTINED);
    let said = refusal.to_string();
    assert!(said.contains("is Quarantined"), "{said}");
    assert!(
        said.contains(&format!("`perch relogin {SECOND_EMAIL}`")),
        "{said}"
    );
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

#[test]
fn a_group_target_is_refused_as_naming_no_one_account() {
    let host = machine();
    declare_group(&host, "work");
    move_to_group(&host, SECOND_EMAIL, "work")
        .0
        .expect("the Account joins the Group");

    let refusal = run_run(&host, "work")
        .0
        .expect_err("a Group is not one Account");

    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    let said = refusal.to_string();
    assert!(said.contains("`work` is a Group"), "{said}");
    assert!(said.contains("name the Account itself"), "{said}");
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

#[test]
fn which_kind_of_target_matched_is_said_before_the_client_takes_the_terminal() {
    let host = machine();
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the Alias is given");

    let (_, printed) = run_run(&host, "overflow");
    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!("`overflow` is an Alias for {SECOND_EMAIL}.")),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "Running Claude Code as {SECOND_EMAIL} (as `overflow`)"
        )),
        "{said}"
    );
    assert!(
        printed.is_empty(),
        "and none of it on the stream the client is about to write to: {printed}"
    );

    host.forget_notes();
    let _ = run_run(&host, SECOND_EMAIL);
    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!("`{SECOND_EMAIL}` is an Account.")),
        "{said}"
    );
}

#[test]
fn a_run_says_nothing_on_the_stream_the_client_writes_to() {
    let host = machine();
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the Alias is given");
    host.forget_notes();

    let (ended, printed) = run_run_with(&host, "overflow", &["--print", "hello"]);

    assert_eq!(ended.expect("the client ran"), 0);
    assert_eq!(
        printed, "",
        "stdout belongs to the client from the moment the Run starts"
    );
    assert!(
        !host.notes().is_empty(),
        "and what Perch had to say was still said, on stderr"
    );
}

#[test]
fn a_run_says_which_account_stays_active_everywhere_else() {
    let host = machine();

    let _ = run_run(&host, SECOND_EMAIL);

    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!("{EMAIL} stays the active Account everywhere else")),
        "{said}"
    );
}

#[test]
fn a_run_claims_nothing_about_who_is_active_while_a_switch_is_in_flight() {
    let host = machine();
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);

    let _ = run_run(&host, SECOND_EMAIL);

    let said = host.notes().join("\n");
    assert!(
        said.contains("Running Claude Code as"),
        "the Run still says what it launched: {said}"
    );
    assert!(
        !said.contains("stays the active Account"),
        "and claims nothing about an Account nothing has established: {said}"
    );
}

#[test]
fn running_the_account_that_is_already_active_is_not_refused() {
    let host = machine();

    let outcome = run_run(&host, EMAIL).0.expect("the client ran");

    assert_eq!(outcome, 0);
    assert_eq!(
        launched(&host),
        vec![(CLAUDE_BIN.to_string(), profile_of(&host, EMAIL))],
        "the Profile, not the Default Profile"
    );
}

#[test]
fn the_clients_exit_code_is_perchs_exit_code() {
    for status in [0, 1, 2, 42, 130] {
        let host = machine_with_two_accounts().with_login(client_exiting(status));

        let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

        assert_eq!(outcome, status);
    }
}

#[test]
fn arguments_after_the_separator_reach_the_client_verbatim() {
    let host = machine();

    let outcome = run_run_with(
        &host,
        SECOND_EMAIL,
        &["--resume", "--json", "-p", "two words", "--group"],
    )
    .0
    .expect("the client ran");

    assert_eq!(outcome, 0);
    assert_eq!(
        command_lines(&host),
        vec![vec![
            CLAUDE_BIN.to_string(),
            "--resume".to_string(),
            "--json".to_string(),
            "-p".to_string(),
            "two words".to_string(),
            "--group".to_string(),
        ]],
        "Claude Code, handed every word as it was typed"
    );
}

#[test]
fn a_program_named_after_the_separator_is_what_runs() {
    let host = machine();

    let outcome = run_run_with(&host, SECOND_EMAIL, &["npm", "test", "--", "--watch"])
        .0
        .expect("the program ran");

    assert_eq!(outcome, 0);
    assert_eq!(
        command_lines(&host),
        vec![vec![
            "npm".to_string(),
            "test".to_string(),
            "--".to_string(),
            "--watch".to_string(),
        ]],
        "the program named, and everything after it — including a second separator"
    );
    assert_eq!(
        launched(&host),
        vec![("npm".to_string(), profile_of(&host, SECOND_EMAIL))],
        "pointed at the Account's Profile, like every other Run"
    );
}

#[test]
fn the_program_being_launched_is_named_when_it_is_not_claude_code() {
    let host = machine();

    let _ = run_run_with(&host, SECOND_EMAIL, &["npm", "test"]);
    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!(
            "Running `npm` as {SECOND_EMAIL}, in this terminal alone."
        )),
        "{said}"
    );

    host.forget_notes();
    let _ = run_run_with(&host, SECOND_EMAIL, &["--resume"]);
    let said = host.notes().join("\n");
    assert!(
        said.contains(&format!("Running Claude Code as {SECOND_EMAIL}")),
        "{said}"
    );
}

#[test]
fn another_program_is_reconciled_for_like_any_other_run() {
    let host = machine_with_shared_state();

    run_run_with(&host, SECOND_EMAIL, &["npm", "test"])
        .0
        .expect("the program ran");

    let profile = profile_of(&host, SECOND_EMAIL);
    for entry in ["CLAUDE.md", "settings.json", "plugins"] {
        assert_eq!(
            host.link_at(profile.join(entry)).map(|(_, target)| target),
            Some(PathBuf::from(shared(entry))),
            "{entry} is reachable from the Profile the program runs in"
        );
    }
}

#[test]
fn another_program_runs_on_a_machine_with_no_claude_code_to_find() {
    let host = machine().without_env("PATH");

    let outcome = run_run_with(&host, SECOND_EMAIL, &["npm", "test"])
        .0
        .expect("`npm` is not Claude Code and does not need one");

    assert_eq!(outcome, 0);
    assert_eq!(
        launched(&host),
        vec![("npm".to_string(), profile_of(&host, SECOND_EMAIL))]
    );

    let refusal = run_run(&host, SECOND_EMAIL)
        .0
        .expect_err("Claude Code is what this one asked for");
    assert_eq!(refusal.exit_code(), EXIT_PROBE_REFUSED);
}

#[test]
fn a_quarantined_account_is_refused_whatever_is_being_launched() {
    let host = machine();
    quarantine(&host, SECOND_EMAIL);

    let refusal = run_run_with(&host, SECOND_EMAIL, &["npm", "test"])
        .0
        .expect_err("a Run that could not authenticate is refused");

    assert_eq!(refusal.exit_code(), EXIT_QUARANTINED);
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

// ---- a Run makes the Profile Live
// ---------------------------

/// The processes Perch would say are running against a Profile right now.
fn live_against(host: &FakeHost, email: &str) -> Vec<u32> {
    perch::probe::live_clients(
        host,
        &profile_of(host, email),
        &perch::probe::Installed::unknown(CLAUDE_VERSION),
    )
    .expect("every marker here can be corroborated or dismissed")
}

#[test]
fn a_run_marks_its_profile_live_for_as_long_as_it_lasts() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&seen);
    let host = machine_with_two_accounts().with_login(move |host: &FakeHost, _dir: &_| {
        *recorded.borrow_mut() = live_against(host, SECOND_EMAIL);
        0
    });
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        *seen.borrow(),
        vec![THIS_PROCESS],
        "the Run names its own process, which is alive for exactly as long as it is"
    );
    assert!(
        live_against(&host, SECOND_EMAIL).is_empty(),
        "and the marker is taken away when the Run ends"
    );
}

#[test]
fn a_runs_marker_arrives_whole_rather_than_being_filled_in_place() {
    let host = machine_with_two_accounts();
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let marker = perch::probe::session_marker_at(&profile_of(&host, SECOND_EMAIL), THIS_PROCESS);
    assert!(
        host.effects().iter().any(|effect| matches!(
            effect,
            Effect::Renamed { to, .. } if to == &marker
        )),
        "the marker was filled in place rather than moved into it:\n{:?}",
        host.effects()
    );
}

#[test]
fn a_capture_into_the_profile_a_run_is_against_is_refused() {
    let refused = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&refused);
    let host = machine_with_two_accounts().with_login(move |host: &FakeHost, _dir: &_| {
        *recorded.borrow_mut() = Some(run_switch(host, SECOND_EMAIL).0);
        0
    });
    host.forget_effects();

    // A Run against the Account that is active, so the Switch attempted inside
    // it would Capture into the very Profile the Run is holding.
    run_run(&host, EMAIL).0.expect("the client ran");

    let error = refused
        .borrow_mut()
        .take()
        .expect("a Switch was attempted while the Run was live")
        .expect_err("the Capture would write under the Run");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains(EMAIL), "{error}");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "nothing moved"
    );
}

#[test]
fn switching_onto_the_account_a_run_is_against_succeeds() {
    let landed = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&landed);
    let host = machine_with_two_accounts().with_login(move |host: &FakeHost, _dir: &_| {
        *recorded.borrow_mut() = Some(run_switch(host, SECOND_EMAIL).0);
        0
    });
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    landed
        .borrow_mut()
        .take()
        .expect("a Switch was attempted while the Run was live")
        .expect("a Run does not close an Account to a Switch");
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
}

#[test]
fn a_run_that_was_killed_does_not_leave_a_profile_live_for_ever() {
    let host = machine();
    a_run_against(&host, EMAIL, host.now());
    let host = host.with_this_process_dead();

    assert!(live_against(&host, EMAIL).is_empty());
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("nothing is holding that Profile");
}

#[test]
fn a_killed_runs_marker_does_not_come_back_to_life_with_a_recycled_pid() {
    let host = machine();
    a_run_against(
        &host,
        EMAIL,
        Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap(),
    );
    let host =
        host.with_this_process_replaced_at(Utc.with_ymd_and_hms(2026, 8, 4, 11, 0, 0).unwrap());

    assert!(live_against(&host, EMAIL).is_empty());
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("that Run died; the pid was recycled");
}

#[test]
fn a_run_answers_for_its_own_profile_and_for_nobody_elses() {
    let elsewhere = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&elsewhere);
    let host = machine_with_shared_state()
        // An ordinary `claude` in another terminal, against the Default Profile.
        .with_file(shared("sessions/33.json"), &{
            let now = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
            format!(
                r#"{{"pid":33,"cwd":"/Users/someone/work","startedAt":{}}}"#,
                now.timestamp_millis()
            )
        })
        .with_live_process(33)
        .with_login(move |host: &FakeHost, _dir: &_| {
            *recorded.borrow_mut() = live_against(host, EMAIL);
            0
        });
    host.forget_effects();

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert!(
        elsewhere.borrow().is_empty(),
        "a client against the Default Profile is not a client against a Profile: {:?}",
        elsewhere.borrow()
    );
    assert_eq!(
        host.link_at(profile_of(&host, SECOND_EMAIL).join("sessions")),
        None,
        "`sessions` does not cross, or every Profile would answer for every other"
    );
    assert!(
        host.file(shared("sessions/700.json")).is_none(),
        "and the Run's own marker does not land in the Default Profile"
    );
}

#[test]
fn a_run_that_cannot_mark_its_profile_live_does_not_launch() {
    let host = machine();
    let marker = perch::probe::session_marker_at(&profile_of(&host, SECOND_EMAIL), THIS_PROCESS);
    let host = host.with_unwritable_file(&marker, "permission denied");

    let refusal = run_run(&host, SECOND_EMAIL)
        .0
        .expect_err("nothing would be protecting that session");

    let said = refusal.to_string();
    assert!(said.contains("permission denied"), "{said}");
    assert!(said.contains("Nothing was launched"), "{said}");
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

#[test]
fn a_target_that_names_nothing_is_refused_before_anything_is_linked() {
    let host = machine();

    let refusal = run_run(&host, "nobody@example.com")
        .0
        .expect_err("there is no such Account");

    assert!(
        refusal
            .to_string()
            .contains("Nothing Perch holds is called `nobody@example.com`"),
        "{refusal}"
    );
    assert!(launched(&host).is_empty(), "{:?}", launched(&host));
}

#[test]
fn an_empty_first_word_is_an_argument_rather_than_a_program() {
    let host = machine();

    let outcome = run_run_with(&host, SECOND_EMAIL, &["", "--resume"])
        .0
        .expect("Claude Code ran");

    assert_eq!(outcome, 0);
    assert_eq!(
        command_lines(&host),
        vec![vec![
            CLAUDE_BIN.to_string(),
            String::new(),
            "--resume".to_string(),
        ]],
        "Claude Code, handed both words as they were typed"
    );
}

/// A machine holding an ordinary active Account and two whose email addresses
/// derive one Profile between them — `slug` flattens every non-alphanumeric
/// character, so `some-one@` and `some.one@` name one directory.
fn machine_holding_the_two_that_share_a_profile() -> FakeHost {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    let mut registry = registry_of(&host);
    for email in ["some-one@example.com", "some.one@example.com"] {
        registry.upsert(perch::registry::Account {
            identity: perch::probe::Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
    }
    common::save_registry(&host, &registry);

    let store = store_of(&host, "some-one@example.com");
    host.set_keychain_item(&store.keychain_service, &store.keychain_account, CREDENTIAL);
    host
}

#[test]
fn a_run_against_a_profile_two_accounts_share_is_refused() {
    let host = machine_holding_the_two_that_share_a_profile().with_login(client_exiting(0));

    let (result, _) = run_run(&host, "some-one@example.com");

    let error = result.expect_err("Perch cannot say which Account that would run as");
    assert!(error.to_string().contains("share one Profile"), "{error}");
    assert!(
        error.to_string().contains("some.one@example.com"),
        "and it names the other one: {error}"
    );
    assert!(launched(&host).is_empty(), "and nothing was launched");
}

#[test]
fn a_run_marks_its_profile_live_before_it_touches_anything_in_it() {
    // With Shared State to link and a `.claude.json` to Carry, so there is
    // something for the claim to come before.
    let host = machine_with_shared_state();
    let profile = perch::registry::profile_dir_for(&host, SECOND_EMAIL).expect("home is known");
    let sessions = profile.join("sessions");

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    let touched: Vec<Effect> = host
        .effects()
        .into_iter()
        .filter(|effect| match effect {
            Effect::WroteFile(at)
            | Effect::WrotePrivateFile(at)
            | Effect::Linked { at, .. }
            | Effect::RemovedLink(at)
            | Effect::RemovedFile(at)
            | Effect::CreatedDir(at) => at.starts_with(&profile),
            Effect::Renamed { to, .. } => to.starts_with(&profile),
            _ => false,
        })
        .collect();

    let first = touched.first().expect("a Run prepares its Profile");
    assert!(
        matches!(first, Effect::CreatedDir(at) if at.starts_with(&sessions)),
        "the Marker's own directory is the first thing made in the Profile, so \
         every later step happens under a Profile that is already Live: \
         {touched:?}"
    );
}

#[test]
fn a_run_whose_sessions_is_a_link_is_refused_rather_than_marking_somewhere_else() {
    let profile = profile_string(SECOND_EMAIL);
    // A *live* link, which is the hazard. A dangling one is refused by
    // `create_dir_all` itself and says something else entirely.
    let host = machine_with_shared_state()
        .with_file(shared("sessions/33.json"), "{}")
        .with_link(
            perch::host::Link::Symbolic,
            shared("sessions"),
            format!("{profile}/sessions"),
        );
    host.forget_effects();

    let (result, _) = run_run(&host, SECOND_EMAIL);

    let refused = result.expect_err("a linked sessions is not this Profile's to record in");
    let said = refused.to_string();
    assert!(
        said.contains("is a link"),
        "the refusal says what is wrong with the Profile: {said}"
    );
    assert!(
        host.file(shared("sessions/700.json")).is_none(),
        "and no marker was written into the directory it pointed at: {said}"
    );
}

/// The claim is taken before Reconcile, and it brought the whole chain into
/// being — the Profile directory included — at the ordinary mode. Reconcile's
/// own `create_private_dir_all` then found a directory already there and left
/// it, so a Profile that had gone missing came back world-traversable with a
/// Credential written into it.
#[test]
fn a_profile_a_run_brings_back_is_the_owners_alone() {
    let host = machine_with_two_accounts().with_login(client_exiting(0));
    let profile = profile_of(&host, SECOND_EMAIL);
    host.remove_dir_all(&profile).expect("it is taken away");

    run_run(&host, SECOND_EMAIL).0.expect("the client ran");

    assert_eq!(
        host.mode_of(&profile),
        Some(PRIVATE_DIR_MODE),
        "the Profile a Run made is its owner's alone"
    );
}
