//! Behaviour tests for `perch run <target>`: a client against one Account's
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

use common::*;
use perch::error::{EXIT_INVALID, EXIT_QUARANTINED};
use perch::host::FakeHost;
use perch::host::fake::Effect;

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

/// Every client a Run launched, as the program and the config directory it was
/// pointed at.
fn launched(host: &FakeHost) -> Vec<(String, PathBuf)> {
    host.effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::ExecInteractive {
                program,
                config_dir,
            } => Some((program, config_dir)),
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

    assert_eq!(registry_of(&host).active.as_deref(), Some(EMAIL));
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
    assert!(
        printed.contains(&format!("`overflow` is an Alias for {SECOND_EMAIL}.")),
        "{printed}"
    );
    assert!(
        printed.contains(&format!(
            "Running Claude Code as {SECOND_EMAIL} (as `overflow`)"
        )),
        "{printed}"
    );

    let (_, printed) = run_run(&host, SECOND_EMAIL);
    assert!(
        printed.contains(&format!("`{SECOND_EMAIL}` is an Account.")),
        "{printed}"
    );
}

/// Said before the client takes the screen, because it is the difference
/// between the command that was typed and the one next to it.
#[test]
fn a_run_says_which_account_stays_active_everywhere_else() {
    let host = machine();

    let (_, printed) = run_run(&host, SECOND_EMAIL);

    assert!(
        printed.contains(&format!("{EMAIL} stays the active Account everywhere else")),
        "{printed}"
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
