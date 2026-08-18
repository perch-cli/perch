//! Behavior tests for `perch run <target>`: a client against one Account's
//! Profile, and nothing else moved (ADR 0010).
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
use perch::error::{EXIT_INVALID, EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE, EXIT_QUARANTINED};
use perch::host::FakeHost;
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::prelude::*;

/// The Default Profile: where Shared State lives, and what a Run has to
/// Reconcile out of.
const SHARED: &str = "/Users/someone/.claude";

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

/// The whole of what makes a Run not a Switch: no Credential is written to the
/// Default Profile, no Identity is patched, and Perch's own record of who is
/// active is exactly where it was.
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

/// A Profile is storage every other day and a live configuration directory for
/// the length of a Run, so the pass that makes Shared State reachable happens
/// before the client that has to see it.
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

/// A client launched by a Run hands `CLAUDE_CONFIG_DIR` on to everything it
/// starts, so a `perch run` typed inside one arrives with a Profile named in its
/// environment. Shared State is still read from the Default Profile: a Profile
/// holds links rather than the person's things, and sharing out of one would
/// share links to links — or nothing at all, silently, where the Account whose
/// Profile it is has just been added.
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

/// The other half of the same rule: a configuration directory somebody has
/// deliberately moved is where their Shared State is, and Perch reads it from
/// there. Only a Profile is disqualified, because only a Profile is somewhere
/// Perch itself pointed the variable.
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

/// Reconcile refuses rather than copying, and a Run served a copy of somebody's
/// memory is the failure that refusal exists to prevent — so the refusal has to
/// stop the launch rather than be printed beside one.
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

/// Two terminals running two Accounts is the point of the feature rather than
/// an edge case: the second Run is started from inside the first one's client,
/// so nothing Perch holds can be held for as long as a session lasts.
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

/// The refusal every Quarantine gets, because the repair every Quarantine has
/// is the same one: no amount of re-running the command reaches it.
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

/// A Group names a set of Accounts declared interchangeable. A Cycle can act on
/// that; a Run has one process to point at one directory, and guessing which
/// Account was meant is the failure the refusal prevents.
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

/// The client inherits this process's standard output, and a Run is meant to
/// stand in a script wherever `claude` would — so `perch run dev -- claude -p
/// … --output-format json | jq .` has to be a document `jq` can read. Two lines
/// of Perch's own in front of it is not. Everything a Run says about the machine
/// goes to standard error, which is where every other remark of Perch's already
/// goes.
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

/// Said before the client takes the screen, because it is the difference
/// between the command that was typed and the one next to it.
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

/// And says nothing about it where nothing has settled who is active.
///
/// A Switch written down and not yet recorded is a **Landing**, and
/// `Active::whose` answers one with the Account being *left* — the last thing
/// Perch established rather than anything it knows. Said off that answer, a Run
/// promised that Account "stays the active Account everywhere else" while the
/// other one's Credential may already be the live one. That is the same claim
/// ADR 0055 had taken out of `perch watcher run`'s opening line, still being
/// made by the command whose whole point is the second half of the sentence.
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

/// A Run against the Account that is already active is not the "nothing to do"
/// a Switch to it would be: it still gets a Profile of its own, which is what
/// keeps the session out of the way of a later Switch.
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

/// A Run is a way of launching a program, so what the program said is what a
/// script reads — a wrapper that flattened it into success or failure would be
/// unusable in anything that branches.
#[test]
fn the_clients_exit_code_is_perchs_exit_code() {
    for status in [0, 1, 2, 42, 130] {
        let host = machine_with_two_accounts().with_login(client_exiting(status));

        let outcome = run_run(&host, SECOND_EMAIL).0.expect("the client ran");

        assert_eq!(outcome, status);
    }
}

/// Everything after `--` belongs to the launched program. Perch has flags of
/// its own spelled the same way, and the whole of the separator's job is that it
/// stops reading at it: a `--json` after `--` is Claude Code's `--json`, not the
/// one `perch list` takes.
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

/// A word that is not a flag names the program, which is what makes a Run a way
/// of running anything under an Account rather than a way of running Claude Code
/// (ADR 0010). The Profile is still what the program is pointed at: that is the
/// point of running it here rather than in the shell.
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

/// Said before the program takes the terminal, because "you are about to run
/// this, as this Account" is the whole of what a Run announces — and naming
/// Claude Code where something else is launching would be a lie about which
/// program is about to write in that Profile.
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

/// Shared State is what a Run is for, and a program other than Claude Code reads
/// the same directory: `npm test` inside a Profile that cannot see the person's
/// settings is the same failure as a client that cannot.
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

/// The probe finds Claude Code, and it is reached only where Claude Code is what
/// is being launched: a Run of `npm` on a machine Perch could not find a client
/// on is still a Run of `npm`, and only the Run that needed a client is refused
/// for the want of one.
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

/// The refusals a Run makes are about the Account rather than about the program,
/// so naming one changes nothing about them.
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

// ---- a Run makes the Profile Live (ADR 0027) ---------------------------

/// The processes Perch would say are running against a Profile right now.
fn live_against(host: &FakeHost, email: &str) -> Vec<u32> {
    perch::probe::live_clients(
        host,
        &profile_of(host, email),
        &perch::probe::Installed::unknown(CLAUDE_VERSION),
    )
    .expect("every marker here can be corroborated or dismissed")
}

/// A Profile with a Run against it is a Live Profile, and stops being one when
/// the Run ends. Asserted from inside the program the Run launched, because
/// "while it lasts" is the only moment the claim is about.
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

/// A marker arrives whole or not at all.
///
/// Written straight into place, the file is truncated and then filled, and a
/// reader catching it between the two reads a file it can see all of and that
/// says nothing — which `clients_in` settles as "nothing is running", the
/// opposite of the arm whose comment names this very case ("halfway through
/// being written by a client that is starting up right now"). A `perch switch`
/// landing in that window Captures the Credential the Run is about to hand a
/// client, which is the mid-task logout ADR 0027's marker exists to prevent.
///
/// Asserted as a rename, because that is what makes a write atomic: the reader
/// sees the old name or the new one and never a prefix of either.
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

/// The consequence that matters: while somebody is working in a Profile, the
/// Capture that a Switch away from that Account would perform is refused. That
/// write would land under a client holding the same file, which is the mid-task
/// logout ADR 0005 exists to prevent.
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

/// And the direction that is allowed. Reading a Credential out of a Profile to
/// copy it into the Default Profile takes nothing away from the session using
/// it, and an Account you are running in one terminal is exactly the one you
/// would want active in the others.
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

/// A Run is killed often — closing the terminal is how a session usually ends —
/// so the marker outliving the process it names is the ordinary case rather
/// than the exception. It says nothing once that process is gone (ADR 0022).
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

/// The same marker, with its pid worn by something that started later. A
/// recycled pid necessarily belongs to a process younger than the marker, which
/// is what makes the check exact rather than a guess.
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

/// Liveness is a fact about one configuration directory, so the directory that
/// records it is the one entry a Reconcile holds back that is neither the
/// person's nor the Account's (ADR 0027). Shared, a Run against one Account
/// would make every Profile on the machine Live at once, and its own marker
/// would land in the Default Profile.
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

/// What the marker buys is that no other Perch Captures or Renews the Credential
/// this session is holding. A Run that cannot claim its Profile is a Run nothing
/// is protecting, so it is refused rather than launched unguarded — and a person
/// told beforehand has lost a command rather than a session.
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

/// Nothing is launched for an Account Perch does not hold, and the refusal is
/// the one every mistyped Target gets.
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

/// An empty first word names a program no more than a leading `-` does.
///
/// The rule after `--` is that the first word decides, and decides totally: a
/// word beginning with `-` is an argument because nothing beginning with `-`
/// names a program the operating system would find. The empty string does not
/// either — `PATH` is searched for names and a path is written with a separator
/// — but it does not begin with `-`, so it was taken as the program and handed
/// to the operating system to launch, under the announcement "Running `` as".
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

/// A Run against a Profile two Accounts share is refused, the way a Switch onto
/// one is.
///
/// One directory means one Credential Store and one Credential, so the client
/// this launches runs as whichever of the two is in it — while the line Perch
/// prints a moment earlier names the other. `perch switch` refuses exactly this
/// (ADR 0006); the Run reached the same directory by another route and did not.
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
