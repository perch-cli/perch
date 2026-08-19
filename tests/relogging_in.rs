//! Behavior: `perch relogin` — the way back from a Quarantine.
//!
//! The Account that comes back has to be the *same* Account: same Alias, same
//! Group, same standing with Cycling, same place in the listing. Removing and
//! re-adding would produce something that looks similar and is not, and would
//! hand the user the job of putting back settings they never changed. So most
//! of these tests are about what a repair leaves alone.
//!
//! The other half is the promise that repairing one Account never costs the
//! session being worked in. The login happens somewhere the active Account
//! cannot be reached from, and the one time the Default Profile is written is
//! when the Account being repaired is the one you are on — where leaving it
//! alone would mean repairing an Account that goes on being broken everywhere
//! it is used.

mod common;

use common::*;
use perch::commands::add::AddArgs;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_PROFILE_LIVE};
use perch::host::FakeHost;
use perch::host::fake::{Effect, THIS_PROCESS};
use perch::host::prelude::*;
use perch::registry::{Active, Quarantine};

/// What a repairing login produces for the second Account: the same person,
/// with a Credential that works.
const SECOND_REPAIRED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-second-repaired","refreshToken":"sk-ant-ort01-second-repaired","expiresAt":1785848400000,"scopes":["user:inference","user:profile"],"subscriptionType":"max"}}"#;

/// The same for the Account adoption started everybody on.
const REPAIRED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-repaired","refreshToken":"sk-ant-ort01-repaired","expiresAt":1785848400000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#;

/// Two Accounts, the second Quarantined, named and in a Group and disabled —
/// every setting a repair has to leave where it found it.
fn broken_second_account() -> FakeHost {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work").0.expect("joined");
    }
    set_alias(&host, "overflow", SECOND_EMAIL).0.expect("named");
    quarantine(&host, SECOND_EMAIL);
    host.with_login(login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE))
}

fn is_disabled(host: &FakeHost, email: &str) -> bool {
    registry_of(host)
        .account(email)
        .expect("an Account Perch holds")
        .disabled
}

#[test]
fn a_repair_replaces_the_credential_and_clears_the_quarantine() {
    let host = broken_second_account();

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("a login is what a Quarantine is waiting for");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        None,
        "the Account is not broken any more, so nothing goes on saying it is: {printed}"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_REPAIRED),
        "and the Credential in its Profile is the one the login just produced"
    );
}

/// **What ADR 0061 leaves of the report.** What was repaired, and nothing about
/// the Alias, the Group or a Cycling state that reads "may choose it".
///
/// A repair leaves all three exactly as it found them, so a column of them was
/// Perch reassuring somebody about work it did not do — and `perch list` is
/// where they are read. The test above is what holds them to being kept.
#[test]
fn a_repair_says_what_it_repaired_and_nothing_about_what_it_left_alone() {
    let host = broken_second_account();

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    // The last line, asserted whole (ADR 0043) — which is a stronger claim than
    // three greps for the labels that used to follow it, because it fails on
    // anything appended rather than only on the wording that was cut.
    assert_eq!(
        printed.trim_end().lines().last(),
        Some(
            format!("Repaired {SECOND_EMAIL} (as `overflow`) — it is no longer Quarantined.")
                .as_str()
        ),
        "{printed}"
    );
}

/// The one of the three that can still surprise, and the reason it is the
/// exception.
///
/// The Credential works again and Cycling will go on passing the Account over
/// — a second thing to undo, and no part of what a repair promises. So it is
/// said, where "may choose it" is not: this is the case the guide cannot have
/// told anybody about in advance.
#[test]
fn a_repaired_account_that_is_still_disabled_is_told_so() {
    let host = broken_second_account();
    disable_account(&host, "overflow")
        .0
        .expect("taken out of Cycling");

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    assert!(
        printed.contains(
            "Cycling still will not choose it — it is disabled, which a repair \
             does not undo."
        ),
        "{printed}"
    );
}

#[test]
fn a_repair_keeps_the_alias_the_group_the_cycling_state_and_the_place() {
    let host = broken_second_account();
    disable_account(&host, "overflow")
        .0
        .expect("taken out of Cycling");

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    let registry = registry_of(&host);
    let account = registry.account(SECOND_EMAIL).expect("still held");
    assert_eq!(
        registry.alias_of(SECOND_EMAIL),
        Some("overflow"),
        "{printed}"
    );
    assert_eq!(account.group.as_deref(), Some("work"));
    assert!(
        is_disabled(&host, SECOND_EMAIL),
        "a login says nothing about whether Cycling may choose an Account, so it \
         does not quietly put one back in the pool"
    );
    assert_eq!(
        registry
            .accounts
            .iter()
            .map(|account| account.email().to_string())
            .collect::<Vec<_>>(),
        vec![EMAIL.to_string(), SECOND_EMAIL.to_string()],
        "and it is repaired where it stood rather than rebuilt at the end"
    );
}

#[test]
fn a_repaired_account_is_a_cycle_candidate_again() {
    let host = broken_second_account();
    observed(&host, EMAIL, vec![window("5-hour", 99.0)]);
    observed(&host, SECOND_EMAIL, vec![window("5-hour", 1.0)]);

    run_cycle(&host)
        .0
        .expect_err("while it is Quarantined there is nowhere to go");

    run_relogin(&host, "overflow").0.expect("repaired");
    let (cycled, printed) = run_cycle(&host);

    cycled.expect("the Account with all the room works again");
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(SECOND_EMAIL),
        "{printed}"
    );
}

#[test]
fn the_account_you_are_working_in_is_untouched_by_repairing_another() {
    let host = broken_second_account();
    let before = host.file(IDENTITY_PATH).expect("Claude Code's own file");

    let (result, printed) = run_relogin(&host, "overflow");

    result.expect("the Account is repaired");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the live Credential is the one it was, so the session being worked in \
         does not notice: {printed}"
    );
    assert_eq!(host.file(IDENTITY_PATH).as_deref(), Some(before.as_str()));
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and its own Profile is untouched too"
    );
}

#[test]
fn a_login_that_was_abandoned_changes_nothing_at_all() {
    let host = broken_second_account().with_login(abandoned_login());

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("there is no Credential to repair it with");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(error.to_string().contains("Nothing changed"), "{error}");
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected),
        "an Account that could not be repaired is still Quarantined, and for the \
         reason it always was"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "the Credential it had is still the Credential it has: a failed repair \
         does not leave the Account emptier than it found it"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "and the Account being worked in is untouched on the failure path too"
    );
}

fn leaked(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

/// The other direction of the same rule: a login under a differently
/// capitalized spelling is the Account being repaired, not somebody else.
///
/// Asked in ASCII, this was the one comparison disagreeing with `add` and
/// `target`, and it disagreed on the path `add` sends people down. Told that
/// Perch already holds `café@example.com`, `add` refuses a second login — it
/// decides both the Profile collision and whether it is one Account over the
/// whole of Unicode — and names `perch relogin` as the way to repair it
/// instead. Resolution then succeeded, the browser round trip was spent, and
/// `é` against `É` under an ASCII fold made them different people. Neither
/// command could hold the login, and the Account stayed Quarantined for good.
#[test]
fn a_login_under_an_accented_spelling_repairs_the_account_rather_than_being_a_stranger() {
    let accented = "café@example.com";
    let shouted = "CAFÉ@example.com";
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(
            IDENTITY_PATH,
            leaked(&IDENTITY_FILE.replace(EMAIL, accented)),
        )
        .with_login(login_producing(
            REPAIRED,
            leaked(&IDENTITY_FILE.replace(EMAIL, shouted)),
        ));
    // The first command adopts the existing login, which is the Account this
    // repairs (ADR 0009).
    run_list(&host, false).0.expect("the machine is adopted");
    quarantine(&host, accented);

    let (result, _) = run_relogin(&host, accented);

    result.expect("the same Account, spelled the other way");
    assert_eq!(
        quarantine_of(&host, accented),
        None,
        "the Quarantine is over rather than the login being turned away"
    );
    assert_eq!(
        credential_of(&host, accented).as_deref(),
        Some(REPAIRED),
        "and the fresh Credential is in the Account's own Profile"
    );
}

#[test]
fn a_login_as_a_different_account_is_refused_and_takes_nothing_over() {
    let host =
        broken_second_account().with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("that login was somebody else");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains(THIRD_EMAIL) && error.to_string().contains("perch add"),
        "the refusal names who logged in and what to do about them: {error}"
    );

    let registry = registry_of(&host);
    assert_eq!(
        registry.alias_of(SECOND_EMAIL),
        Some("overflow"),
        "an Alias the user chose for one Account is not handed to another because \
         a browser was signed into somebody else"
    );
    assert!(registry.account(THIRD_EMAIL).is_none());
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL)
    );
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected)
    );
}

#[test]
fn repairing_the_account_you_are_on_makes_its_fresh_credential_the_live_one() {
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);

    let (result, printed) = run_relogin(&host, EMAIL);

    result.expect("the Account you are on can be repaired");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(REPAIRED),
        "a repair only its own Profile can see would leave the Account broken \
         everywhere it is actually used: {printed}"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(REPAIRED));
    assert_eq!(quarantine_of(&host, EMAIL), None);
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "and it is still the active Account: this was a repair, not a Switch"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL),
        "no other Account's Profile is written on the way"
    );
}

/// The terminal going away between the repair and the line about it still
/// leaves a machine whose live Credential is the repaired one.
///
/// `report` is the last thing said before the landing, and it is fallible: a
/// closed pty, a SIGHUP, a `| head -1`. Propagated from there, `perch relogin`
/// returned with the Quarantine released and saved, the fresh Credential in the
/// Account's own Profile, the *broken* one still live, and `active` still
/// naming this Account — so the next `perch switch` Captures the broken live
/// copy over the fresh one and undoes the repair. Neither `make_live` nor the
/// `no_longer_on_anybody` defense behind it had run.
#[test]
fn a_terminal_that_goes_away_after_the_repair_still_makes_the_fresh_credential_live() {
    /// Writes until the repair is announced, and then is not there.
    struct GoesAwayReporting;

    impl std::io::Write for GoesAwayReporting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Repaired") {
                true => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "the pipe closed",
                )),
                false => Ok(bytes.len()),
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);

    let outcome = perch::commands::relogin::run(
        &host,
        perch::commands::relogin::ReloginArgs {
            target: EMAIL.to_string(),
        },
        &mut GoesAwayReporting,
    );

    outcome.expect_err("the report could not be written");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(REPAIRED),
        "the landing is what makes the repair hold, and nothing about it is \
         contingent on somebody reading a sentence"
    );
    assert_eq!(quarantine_of(&host, EMAIL), None);
    assert_eq!(registry_of(&host).active().whose(), Some(EMAIL));
}

/// A Landing nothing can account for is the one state ADR 0048 names this
/// command as the way out of: *"`perch relogin {arriving}` finishes that Switch
/// and `perch relogin {leaving}` abandons it"*. So it is the one failure the
/// command may not be stopped by — refusing here would be Perch turning away
/// the remedy it had just told the user to run, and doing it after the browser
/// round trip that remedy costs.
///
/// Either half of the Landing lands: what settles the question the reading
/// could not is the fresh Credential going live.
#[test]
fn a_landing_nothing_accounts_for_is_repaired_rather_than_refused() {
    for (what, repairing, fresh, identity) in [
        (
            "the Account the Switch was leaving, which abandons it",
            EMAIL,
            REPAIRED,
            IDENTITY_FILE,
        ),
        (
            "the Account it was switching to, which finishes it",
            SECOND_EMAIL,
            SECOND_REPAIRED,
            SECOND_IDENTITY_FILE,
        ),
    ] {
        let host = machine_with_two_accounts().with_login(login_producing(fresh, identity));
        a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
        // A Rotation after the interruption: the corner Perch refuses to guess
        // at, and the refusal that names this command.
        host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_REPAIRED);
        run_switch(&host, SECOND_EMAIL)
            .0
            .expect_err("a Switch cannot tell whose the live Credential is");

        let (result, printed) = run_relogin(&host, repairing);

        result.unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(
            host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
            Some(fresh),
            "{what}: the fresh Credential is live, which is what answers the \
             question nothing else could: {printed}"
        );
        assert_eq!(
            *registry_of(&host).active(),
            Active::Settled(repairing.to_string()),
            "{what}: and the Landing is gone, because the Account repaired is \
             the one the machine is now on"
        );
    }
}

/// The refusal above is the *only* one this command steps past, and the reason
/// is that Perch weighed the evidence and could not choose. A store that would
/// not answer is not that: nothing was weighed, so nothing is known — including
/// whether the Default Profile this repair may be about to write is somebody
/// else's. Repairing on it would be deciding by not looking.
#[test]
fn a_store_that_will_not_answer_stops_the_repair_rather_than_being_stepped_past() {
    // Off macOS, where a Profile keeps its Credential in a file (ADR 0020) —
    // the only way to make the live store alone refuse to answer.
    let host = logged_in_machine_off_macos().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    run_list(&host, false)
        .0
        .expect("adoption holds the login it finds");
    a_switch_died_mid_flight(&host, Some(EMAIL), EMAIL);
    host.set_unreadable(CREDENTIALS_PATH, "Permission denied");

    let (result, _) = run_relogin(&host, EMAIL);

    let said = result
        .expect_err("nothing on the machine could be read")
        .to_string();
    assert!(
        said.contains("Make that store readable"),
        "it says what to put right rather than repairing blind: {said}"
    );
}

/// Repairing a *third* Account while a Landing is unaccounted for is an
/// ordinary repair. It writes that Account's own Profile and nothing else, so
/// the Landing has no bearing on it — and a command refused for a state it
/// cannot possibly disturb is a command that stops working on a machine that
/// needs it.
#[test]
fn a_landing_nothing_accounts_for_does_not_stop_an_unrelated_repair() {
    let host = machine_with_three_accounts()
        .with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_REPAIRED);

    let (result, printed) = run_relogin(&host, THIRD_EMAIL);

    result.expect("an Account the Landing does not name is repaired as ever");
    assert_eq!(
        credential_of(&host, THIRD_EMAIL).as_deref(),
        Some(THIRD_CREDENTIAL),
        "its own Profile holds the fresh Credential: {printed}"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(SECOND_REPAIRED),
        "and the live Credential is untouched, because this repair was never \
         about the Default Profile"
    );
    assert!(
        matches!(*registry_of(&host).active(), Active::Landing { .. }),
        "so the Landing is still there, still unresolved, and still nobody's \
         business but a Switch's"
    );
}

#[test]
fn a_healthy_account_may_be_logged_in_again() {
    let host = machine_with_two_accounts()
        .with_login(login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE));

    let (result, printed) = run_relogin(&host, SECOND_EMAIL);

    result.expect(
        "a Credential somebody suspects is going wrong should not have to break \
         first before it can be replaced",
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_REPAIRED)
    );
    assert_eq!(quarantine_of(&host, SECOND_EMAIL), None);
    assert!(
        !printed.contains("was Quarantined"),
        "and it does not claim to have repaired something that was not broken: {printed}"
    );
}

#[test]
fn a_profile_a_client_is_running_against_is_refused_before_a_login_is_spent() {
    let host = broken_second_account();
    let profile = store_of(&host, SECOND_EMAIL).config_dir;
    let marker = format!(
        r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
        host.now().timestamp_millis()
    );
    let host = host
        .with_file(profile.join("sessions/4242.json"), &marker)
        .with_live_process(4242);
    host.forget_effects();

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "a Profile Perch may not write to is one no browser round trip was going \
         to repair: {:?}",
        host.effects()
    );
}

/// The same question, asked again on the other side of the login.
///
/// A browser round trip is the longest wait in Perch — the first answer is
/// minutes old by the time it comes back — and what follows it writes a fresh
/// Credential into the Account's own Profile. A `perch run` started while the
/// person was logging in would be written under, so the repair stops instead
/// (ADR 0027). The login itself runs against a directory of its own, which is
/// what lets this be asked at all without the repair tripping over itself.
#[test]
fn a_run_started_during_the_login_stops_the_repair_before_it_writes() {
    let host = broken_second_account();
    let held = store_of(&host, SECOND_EMAIL).config_dir;
    let before = credential_of(&host, SECOND_EMAIL);
    let host = host.with_login(move |host: &FakeHost, dir: &std::path::Path| {
        // Somebody starts working in that Account while the browser is open.
        a_run_against(host, SECOND_EMAIL, host.now());
        login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE)(host, dir)
    });
    host.forget_effects();

    let (result, _) = run_relogin(&host, "overflow");

    let error = result.expect_err("the repair would write under that Run");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(
        error.to_string().contains(&THIS_PROCESS.to_string()),
        "{error}"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL),
        before,
        "and {} was left exactly as the Run found it",
        held.display()
    );
    assert_eq!(
        quarantine_of(&host, SECOND_EMAIL),
        Some(Quarantine::RenewalRejected),
        "the Account is still broken, because nothing was repaired"
    );
}

/// And the Default Profile is asked about on that side of the login too.
///
/// Repairing the Account you are on writes the fresh Credential into the Default
/// Profile as well, so a `claude` started while the browser was open is holding
/// the file the last step of the repair replaces (ADR 0005). The check before
/// the login covers it; the check after has to cover the same pair, or the
/// second ask is weaker than the first and the whole minutes-long window it
/// exists for is the one it does not see.
#[test]
fn a_client_started_during_the_login_stops_the_repair_of_the_account_you_are_on() {
    let host = machine_with_two_accounts();
    quarantine(&host, EMAIL);
    let host = host.with_login(|host: &FakeHost, dir: &std::path::Path| {
        // Somebody starts a client on the Default Profile while the browser is
        // open — nothing to do with the directory the login itself runs in.
        host.set_file(
            "/Users/someone/.claude/sessions/4242.json",
            &format!(
                r#"{{"pid":4242,"cwd":"/Users/someone/work","startedAt":{}}}"#,
                host.now().timestamp_millis()
            ),
        );
        host.set_live_process(4242);
        login_producing(REPAIRED, IDENTITY_FILE)(host, dir)
    });
    host.forget_effects();

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the repair would write under that client");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the session goes on holding exactly what it was holding"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::RenewalRejected),
        "and the Account is still broken, because nothing was repaired"
    );
}

/// And once more under the locks, which is where a Switch has always asked it.
///
/// The check after the login is taken while Perch's own registry lock is held,
/// and everything between it and the write is still to happen: the registry is
/// saved, and then Claude Code's three locks are taken — a wait of up to four
/// seconds against a client that is holding them. A `claude` started in that
/// gap was one nothing had seen by the time its Credential was replaced, which
/// is the mid-task logout ADR 0005 exists to prevent, arriving at the one write
/// that does not Capture first. A `perch switch` closes exactly this window by
/// asking again once the locks are held; the repair did not.
#[test]
fn a_client_that_starts_during_the_lock_wait_still_stops_the_repair() {
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let now = host.now();
    let host = host
        .with_dir_held_since("/Users/someone/.claude/.oauth_refresh.lock", now)
        .once_while_waiting(move |host: &FakeHost| {
            // The holder gives the lock back — and in the same moment somebody
            // starts working against the Default Profile.
            host.remove_dir_all(std::path::Path::new(
                "/Users/someone/.claude/.oauth_refresh.lock",
            ))
            .expect("the holder is done");
            host.set_file(
                "/Users/someone/.claude/sessions/7788.json",
                &format!(
                    r#"{{"pid":7788,"cwd":"/Users/someone/work","startedAt":{}}}"#,
                    now.timestamp_millis()
                ),
            );
            host.set_live_process(7788);
        });

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("that Credential belongs to the session holding it");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("7788"), "{error}");
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the session goes on holding exactly what it was holding"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "and the repair itself stands in the Account's own Profile, which is \
         what the refusal says"
    );
}

#[test]
fn repairing_the_account_you_are_on_is_refused_while_a_client_holds_the_default_profile() {
    // The Default Profile is the one a repair of the active Account writes, and
    // it is the Credential a running session is holding.
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let host = client_running_against(host, "/Users/someone/.claude", 4242);
    host.forget_effects();

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("that Credential belongs to the client until it exits");
    assert_eq!(error.exit_code(), EXIT_PROFILE_LIVE);
    assert!(error.to_string().contains("4242"), "{error}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "and no login was spent on a repair that could not land"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the session goes on holding exactly what it was holding"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        Some(Quarantine::RenewalRejected)
    );
}

/// One step earlier than the test below, and the more dangerous of the two: the
/// repair happened and could not be *recorded*.
///
/// The registry still says Quarantined and still names this Account as the one
/// you are on, while the broken Credential is still the live one and the fresh
/// one sits in the Account's own Profile. That is exactly the state
/// `no_longer_on_anybody` defends against by clearing `active` — and clearing
/// `active` is a registry write, which is what just failed. So the only defense
/// left is saying what not to do, and a bare `?` said only that a file could not
/// be written.
#[test]
fn a_repair_that_could_not_be_recorded_says_the_login_worked_and_not_to_switch() {
    let host = logged_in_machine_off_macos().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    run_list(&host, false)
        .0
        .expect("the first command adopts the login");
    quarantine(&host, EMAIL);

    let host = host.with_unwritable_file(REGISTRY_PATH, "read-only file system");

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the repair could not be recorded");
    let said = error.to_string();
    assert!(
        said.contains("The login itself worked"),
        "the browser round trip is not repeated for nothing: {said}"
    );
    assert!(
        said.contains("Do not run `perch switch`"),
        "and the Capture that would destroy the repair is named: {said}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "the fresh Credential really is in the Account's own Profile"
    );
}

#[test]
fn a_repair_that_could_not_be_made_live_still_stands_and_says_what_is_left() {
    // Off macOS, so the Default Profile's Credential is the file below and the
    // login's own directory is somewhere else entirely: the write that fails is
    // the last one, not the login.
    let host = logged_in_machine_off_macos().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    run_list(&host, false)
        .0
        .expect("the first command adopts the login");
    quarantine(&host, EMAIL);

    // Neither store of the Default Profile will take it, so the repair lands in
    // the Account's own Profile and goes no further.
    let host = host.with_unwritable_file(CREDENTIALS_PATH, "no space left on device");
    host.lock_keychain("could not run /usr/bin/security: No such file or directory");

    let (result, printed) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the live Credential could not be replaced");
    assert!(
        error.to_string().contains("The repair itself stands"),
        "a partial outcome says which half happened: {error}"
    );
    assert_eq!(
        quarantine_of(&host, EMAIL),
        None,
        "the Account has a working Credential in its own Profile, which is the \
         whole of what the Quarantine said it did not have: {printed}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "and that Credential is the repaired one"
    );
}

#[test]
fn a_group_named_where_one_account_is_meant_is_refused_as_a_group() {
    let host = broken_second_account();

    let (result, _) = run_relogin(&host, "work");

    let error = result.expect_err("this acts on one Account");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(error.to_string().contains("Group"), "{error}");
}

/// Making the repaired Credential live is two writes, and the second one
/// failing does not undo the first. Here the Credential *is* live — only Claude
/// Code's own note of whose it is is behind — so Perch has to record what is
/// true.
///
/// Recording nobody as active here is the expensive mistake, not the cautious
/// one: with nothing active there is nothing for a Switch to Capture into, so
/// the Rotation this now-working session goes on to make is destroyed by the
/// next one — which is the very hazard stopping to name nobody exists to
/// prevent. `observe::holding` also stops reading the Default Profile for the
/// active Account once `active` is wrong, so a Renewal can log that client out.
#[test]
fn a_repair_whose_identity_patch_failed_is_live_and_still_recorded_as_active() {
    let host = machine_with_two_accounts().with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let host = host.with_unwritable_file(IDENTITY_PATH, "read-only file");

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the Identity could not be patched");
    assert!(
        error
            .to_string()
            .contains("its fresh Credential is the live one"),
        "it says the repair stands rather than the opposite: {error}"
    );
    assert!(
        !error.to_string().contains("no active Account"),
        "and does not claim to have stopped recording anybody: {error}"
    );
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(REPAIRED),
        "the fresh Credential really is the live one"
    );
    assert_eq!(
        registry_of(&host).active().whose(),
        Some(EMAIL),
        "so Perch goes on recording the Account it is really on, and a Switch \
         away from it Captures whatever this session Rotates"
    );
    assert_eq!(quarantine_of(&host, EMAIL), None);
}

/// The other side of it: the Credential never became live at all, so the broken
/// one it replaced still is. Now `active` must stop naming that Account — the
/// very next `perch switch`, which is at least as natural a thing to reach for
/// as running the repair again, would Capture the broken copy over the fresh one
/// and undo the whole browser round trip (ADR 0006).
///
/// Off macOS, where the plaintext file is the store written first (ADR 0020), so
/// making it unwritable is what stops the Credential reaching the live store at
/// all rather than only stopping the Identity patch after it.
#[test]
fn a_repair_that_could_not_be_made_live_leaves_nothing_to_capture_into() {
    let host = logged_in_machine_off_macos()
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
    let host = host.with_login(login_producing(REPAIRED, IDENTITY_FILE));
    quarantine(&host, EMAIL);
    let host = host.with_unwritable_file(CREDENTIALS_PATH, "read-only file");

    let (result, _) = run_relogin(&host, EMAIL);

    let error = result.expect_err("the repair could not be made live");
    assert!(
        error.to_string().contains("no active Account"),
        "it says what it did about it: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("goes on using the one that stopped working"),
        "and that the live Credential really is still the broken one: {error}"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "the repair itself stands"
    );
    assert_eq!(
        *registry_of(&host).active(),
        Active::Nobody,
        "and Perch is on nobody, so nothing can Capture over it"
    );

    // The command a user would reach for next, which used to be the one that
    // destroyed the repair.
    host.writable_again(CREDENTIALS_PATH);
    run_switch(&host, SECOND_EMAIL)
        .0
        .expect("a Switch still works");
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(REPAIRED),
        "the fresh Credential survives the Switch that follows"
    );
}

/// A browser round trip is the longest wait in Perch, and the registry is read
/// again afterwards for exactly that reason. An Account given up in another
/// terminal while the login was open leaves nothing to repair — and the login
/// itself worked, so the sentence has to say where that Credential went rather
/// than leaving somebody thinking they lost it.
#[test]
fn an_account_removed_while_its_login_was_open_says_the_login_still_worked() {
    let host = broken_second_account().with_login(|host, dir| {
        // Somebody in another terminal gives the Account up while the browser
        // is still open. Through `forget`, which is what `perch remove` calls:
        // dropping the entry by hand left the Alias it answered to pointing at
        // an Account nothing held, which is a registry `load` now refuses
        // outright — so the fixture was arranging a state no `perch remove`
        // produces and the refusal under test was never reached.
        let mut registry = registry_of(host);
        registry.forget(SECOND_EMAIL);
        save_registry(host, &registry);

        login_producing(SECOND_REPAIRED, SECOND_IDENTITY_FILE)(host, dir)
    });

    let (result, _) = run_relogin(&host, SECOND_EMAIL);

    let refusal = result.expect_err("there is nothing left to repair");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    let said = refusal.to_string();
    assert!(
        said.contains(&format!(
            "{SECOND_EMAIL} was removed while that login was happening"
        )),
        "{said}"
    );
    assert!(
        said.contains("The login itself worked") && said.contains("perch add"),
        "it says the Credential is not lost, and how to keep it: {said}"
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

/// A repair of an Account whose Profile is not its alone is refused before the
/// browser round trip.
///
/// `settle_into_its_own_profile` writes the fresh Credential into the shared
/// store, which supersedes the other Account's — and a retired refresh token is
/// the one loss ADR 0006 calls unrecoverable. Worse, it would happen *after* the
/// login, having already told the user this was the way back.
#[test]
fn repairing_an_account_whose_profile_is_shared_is_refused_before_the_login() {
    let host = machine_holding_the_two_that_share_a_profile();
    host.forget_effects();

    let (result, _) = run_relogin(&host, "some-one@example.com");

    let error = result.expect_err("the repair would destroy the other Account's Credential");
    assert!(error.to_string().contains("share one Profile"), "{error}");
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Exec { .. })),
        "and no browser round trip was spent finding that out: {:?}",
        host.effects()
    );
}
