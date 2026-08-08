//! `perch purge` — giving the machine back (ADR 0014).
//!
//! The one command that destroys everything at once, so almost all of this is
//! about what it will not destroy quietly: not without the word typed out, not
//! without offering the file that makes it survivable, and never the login
//! Claude Code is running on. The other half is that it finishes — a Purge that
//! left a keychain item behind, or a directory, would be a machine somebody
//! believes they have given back.

mod common;

use std::path::Path;

use common::*;
use perch::error::{EXIT_INVALID, EXIT_NOTHING_TO_DO, EXIT_PROFILE_LIVE};
use perch::export;
use perch::host::fake::Effect;
use perch::host::{FakeHost, Host};
use perch::registry::{Quarantine, Registry};

const PERCH_HOME: &str = "/Users/someone/.config/perch";
const DEFAULT_PROFILE: &str = "/Users/someone/.claude";
const AT: &str = "/Users/someone/perch-backup.age";
const PASSPHRASE: &str = "correct horse battery staple";

/// Three Accounts with everything a user gives them, on a machine where
/// somebody declines the Export and types the word.
fn a_machine_to_give_back() -> FakeHost {
    let host = machine_with_three_accounts();
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the name is free");
    quarantine_for(&host, THIRD_EMAIL, Quarantine::RenewalRejected);
    host.with_answers(&["n", "purge"])
}

/// The same directory as Perch would *write* it, which is not the same string
/// on every platform: a path is displayed with the separator the platform uses,
/// so a test that matched output against the literal above would pass on two of
/// the three machines Perch is built for.
fn perch_home_as_written(host: &FakeHost) -> String {
    perch::registry::perch_home(host)
        .expect("home is known")
        .display()
        .to_string()
}

/// The registry as it is on disk, or nothing at all — which is what a machine
/// given back looks like.
fn registry_on(host: &FakeHost) -> Option<Registry> {
    perch::registry::load(host).expect("whatever is there is readable")
}

/// What the live store holds right now: the Credential every client reads, and
/// the one thing on this machine that is not Perch's to take away.
fn live_credential(host: &FakeHost) -> Option<String> {
    host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME)
}

/// The whole of what a Purge promises: every Profile, every Credential Perch
/// holds, and Perch's own registry, gone in one act.
#[test]
fn a_purge_takes_every_profile_every_credential_and_the_registry() {
    let host = a_machine_to_give_back();
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert!(credential_of(&host, email).is_some(), "{email} is held");
    }

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(
            credential_of(&host, email),
            None,
            "{email}'s Credential is gone: {printed}"
        );
    }
    assert_eq!(registry_on(&host), None, "{printed}");
    assert!(
        !host.path_exists(Path::new(PERCH_HOME)),
        "and the directory Perch kept it all in is not there either"
    );
    assert!(printed.contains("Purged 3 Accounts"), "{printed}");
}

/// Claude Code's own login is Claude Code's. A Purge that logged the user out of
/// the tool they are using would be doing more than giving the machine back.
#[test]
fn the_login_claude_code_is_running_on_is_left_exactly_where_it_is() {
    let host = a_machine_to_give_back();

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert_eq!(
        live_credential(&host).as_deref(),
        Some(CREDENTIAL),
        "the Credential in the Default Profile is the active Account's and is \
         still live: {printed}"
    );
    assert_eq!(
        host.file(IDENTITY_PATH).as_deref(),
        Some(IDENTITY_FILE),
        "and the file naming who that is was not touched"
    );
    assert!(
        printed.contains("still logged in"),
        "which is said, because it is the one thing a Purge deliberately leaves \
         behind:\n{printed}"
    );
}

/// Off macOS that login is a file inside the Default Profile rather than a
/// keychain item (ADR 0020), and the Purge that empties every Profile Perch made
/// leaves that directory standing.
#[test]
fn the_credential_the_default_profile_keeps_in_a_file_is_left_alone_too() {
    let host = logged_in_machine_off_macos().with_answers(&["n", "purge"]);
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert_eq!(
        host.file(CREDENTIALS_PATH).as_deref(),
        Some(CREDENTIAL),
        "{printed}"
    );
    assert!(host.path_exists(Path::new(DEFAULT_PROFILE)));
    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
}

/// The prompt is harder than every other prompt in Perch: what will go, said by
/// email address, and that nothing brings it back.
#[test]
fn the_prompt_lists_the_accounts_by_email_and_says_nothing_undoes_it() {
    let host = a_machine_to_give_back();

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert!(printed.contains(email), "{email} is named: {printed}");
    }
    assert!(printed.contains("Nothing undoes it"), "{printed}");
    assert!(printed.contains(&perch_home_as_written(&host)), "{printed}");
}

/// A `y` is what fingers answer before eyes have read anything, and this is the
/// one command nothing undoes.
#[test]
fn a_letter_is_not_enough_to_purge() {
    for answered in [&["n", "y"], &["n", "yes"], &["n", "PURG"]] {
        let host = machine_with_three_accounts().with_answers(answered);

        let (outcome, printed) = run_purge(&host);

        outcome.expect("answering something else is an answer, not a failure");
        assert!(printed.contains("Nothing was purged"), "{printed}");
        assert_eq!(
            registry_on(&host).map(|registry| registry.accounts.len()),
            Some(3),
            "every Account is still held: {printed}"
        );
        assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
    }
}

/// The word, however it was typed: a Purge refused over a trailing space or a
/// Caps Lock would only be typed again, and the guard is against the reflex.
#[test]
fn the_word_is_taken_however_it_was_typed() {
    for answered in [&["n", "  purge  "], &["n", "PURGE"]] {
        let host = machine_with_three_accounts().with_answers(answered);

        run_purge(&host).0.expect("the word was typed");

        assert_eq!(registry_on(&host), None);
    }
}

/// End of input is not agreement. A pipe that closed is the one thing that must
/// never read as somebody agreeing to give up every Credential on the machine.
#[test]
fn end_of_input_purges_nothing() {
    let host = machine_with_three_accounts();

    let (outcome, printed) = run_purge(&host);

    outcome.expect("nothing was asked of the machine that it refused");
    assert!(printed.contains("Nothing was purged"), "{printed}");
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3)
    );
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "and nobody was asked for a passphrase for an Export nobody agreed to"
    );
}

/// Perch cannot verify that an Export exists — the file is wherever the user put
/// it, possibly on another machine — so it is offered rather than required, and
/// a no is an answer.
#[test]
fn declining_the_export_still_purges() {
    let host = a_machine_to_give_back();

    let (outcome, printed) = run_purge(&host);
    outcome.expect("declining is allowed");

    assert!(printed.contains("Write an Export first?"), "{printed}");
    assert_eq!(host.file(AT), None, "nothing was written");
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "and nobody was asked to type a passphrase for a file they declined"
    );
    assert_eq!(registry_on(&host), None, "{printed}");
}

/// The offer is the only thing that makes a Purge survivable, so it is written
/// before anything is destroyed — and it survives the Purge that offered it.
#[test]
fn the_export_it_offers_is_written_before_anything_is_destroyed() {
    let host = a_machine_to_give_back()
        .with_answers(&["", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    let sealed = host.file(AT).expect("the Export is still there afterwards");
    let exported = export::unseal(&sealed, PASSPHRASE).expect("it opens");
    assert_eq!(exported.registry.accounts.len(), 3, "{printed}");
    assert_eq!(
        exported.credentials.get(EMAIL).map(String::as_str),
        Some(CREDENTIAL),
        "with a working Credential for every Account, which is the whole of what \
         makes the Purge survivable"
    );
    assert_eq!(
        registry_on(&host),
        None,
        "and the Purge happened: {printed}"
    );
}

/// Written before anything is destroyed means exactly that: an Export that could
/// not be written stops the Purge, with everything still there. A path that is
/// taken is something somebody has to go and settle, and nothing has been lost
/// while they do.
#[test]
fn an_export_that_cannot_be_written_purges_nothing() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE])
        .with_file(AT, "an Export somebody else wrote");

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("something is already at that path");
    assert!(refused.to_string().contains(AT), "{refused}");
    assert_eq!(
        host.file(AT).as_deref(),
        Some("an Export somebody else wrote"),
        "what was there is what is there"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged: {printed}"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
}

/// An Export written under Perch's own home would be taken by the Purge that
/// offered it, which is the one place a backup must not go.
#[test]
fn an_export_inside_what_the_purge_will_take_is_refused() {
    let inside = format!("{PERCH_HOME}/backup.age");
    let host = a_machine_to_give_back().with_answers(&["y", &inside, "purge"]);

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("that file would not survive the Purge");
    assert_eq!(refused.exit_code(), EXIT_INVALID, "{refused}");
    assert!(
        refused.to_string().contains(&perch_home_as_written(&host)),
        "{refused}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged"
    );
}

/// The pair is the point: an Export written before a Purge, an Import after it.
/// Asserted end to end, because a Purge is only as survivable as the file it
/// offers actually being restorable.
#[test]
fn what_a_purge_gives_back_an_import_puts_back() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE, PASSPHRASE]);
    let before = {
        let mut registry = registry_of(&host);
        // The one thing that deliberately does not travel: being active is a
        // claim about which Credential is in this machine's Default Profile.
        registry.active = None;
        registry
    };

    run_purge(&host).0.expect("the word was typed");
    let (imported, printed) = run_import(&host, AT);

    imported.expect("a machine a Purge emptied is one an Import lands on");
    assert_eq!(registry_on(&host).as_ref(), Some(&before), "{printed}");
    for (email, credential) in [
        (EMAIL, CREDENTIAL),
        (SECOND_EMAIL, SECOND_CREDENTIAL),
        (THIRD_EMAIL, THIRD_CREDENTIAL),
    ] {
        assert_eq!(
            credential_of(&host, email).as_deref(),
            Some(credential),
            "{email} came back with a working Credential"
        );
    }
}

/// Every capability is reachable from a script (ADR 0011), so the flag answers
/// ahead of time — and answers both questions, because an Export is a path
/// somebody names and a passphrase somebody types.
#[test]
fn the_flag_purges_without_asking_anything() {
    let host = machine_with_three_accounts().without_terminal();
    host.forget_effects();

    let (outcome, printed) = run_purge_with(&host, perch::commands::purge::PurgeArgs { yes: true });
    outcome.expect("every capability is available non-interactively");

    assert_eq!(registry_on(&host), None, "{printed}");
    assert!(!host.path_exists(Path::new(PERCH_HOME)));
    assert!(
        !host.effects().contains(&Effect::Asked)
            && !host.effects().contains(&Effect::AskedInSecret),
        "nothing was asked: {:?}",
        host.effects()
    );
}

/// Without the flag there is nobody to type the word, and a Purge that read that
/// as agreement would be the one command a closed pipe costs every Credential
/// on the machine.
#[test]
fn without_a_terminal_and_without_the_flag_a_purge_is_refused_and_names_it() {
    let host = machine_with_three_accounts().without_terminal();

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("there is nobody to confirm with");
    assert!(refused.to_string().contains("--yes"), "{refused}");
    assert!(
        registry_on(&host).is_some(),
        "and nothing was purged on the way to saying so"
    );
}

/// A Purge deletes the Profiles a client would be holding files in, so it is
/// refused while one is — the same rule every other write into a Live Profile
/// obeys (ADR 0005), at its extreme.
#[test]
fn a_client_running_against_a_profile_stops_the_purge() {
    let host = a_machine_to_give_back();
    a_run_against(&host, SECOND_EMAIL, host.now());

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("something is holding that Profile");
    assert_eq!(refused.exit_code(), EXIT_PROFILE_LIVE, "{refused}");
    assert!(refused.to_string().contains(SECOND_EMAIL), "{refused}");
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged, not even the Accounts nothing is running against"
    );
    assert!(
        !host.effects().contains(&Effect::Asked),
        "asked before anybody was asked to agree to it, because a Purge is all \
         or nothing"
    );
}

/// The refusal above is checked before the questions and again after them, and
/// the window in between is however long somebody takes over four prompts. A
/// client started in there was not running when the first check ran, and its
/// Profile is what the next line would delete.
#[test]
fn a_client_that_starts_while_the_questions_are_answered_stops_the_purge_too() {
    let host = a_machine_to_give_back()
        // The fake performs this the first time Perch waits, which is the first
        // prompt: the terminal has to take some time for there to be a window at
        // all.
        .with_a_terminal_that_takes(1_000)
        .once_while_waiting(|host| a_run_against(host, SECOND_EMAIL, host.now()));

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("something started holding that Profile");
    assert_eq!(refused.exit_code(), EXIT_PROFILE_LIVE, "{refused}");
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged, however far the questions had got: {printed}"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
}

/// A Purge that stopped part way is run again, and finishes. The Credentials go
/// first and the registry naming them goes last, so what has already been
/// deleted is found already gone rather than lost track of.
#[test]
fn a_purge_that_stopped_part_way_finishes_when_it_is_run_again() {
    let second_profile = format!("{PERCH_HOME}/profiles/overflow-example-com/.credentials.json");
    let host = a_machine_to_give_back()
        .with_answers(&["n", "purge", "n", "purge"])
        .with_undeletable_file(&second_profile, "read-only");

    let (stopped, printed) = run_purge(&host);

    stopped.expect_err("one of the stores would not give its Credential up");
    assert_eq!(
        credential_of(&host, EMAIL),
        None,
        "it had already taken the Account listed before the one that stopped it, \
         which is what makes this a Purge stopped part way: {printed}"
    );
    assert!(
        registry_on(&host).is_some(),
        "the registry naming what is left is still there: {printed}"
    );
    assert_eq!(
        credential_of(&host, THIRD_EMAIL).as_deref(),
        Some(THIRD_CREDENTIAL),
        "and the Accounts it had not reached are untouched"
    );

    host.deletable_again(&second_profile);
    let (finished, printed) = run_purge(&host);

    finished.expect("running it again finishes it");
    assert_eq!(registry_on(&host), None, "{printed}");
    assert!(!host.path_exists(Path::new(PERCH_HOME)));
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(credential_of(&host, email), None, "{email}: {printed}");
    }
}

/// The last thing a Purge does is take Perch's home, and a home left behind with
/// no registry in it is what an interrupted one leaves. Running it again takes
/// it, rather than reporting there is nothing to do and leaving the directory
/// standing.
#[test]
fn a_home_left_behind_with_no_registry_is_taken_by_the_next_purge() {
    let host = machine_with_three_accounts().with_answers(&["purge"]);
    host.remove_file(Path::new(REGISTRY_PATH))
        .expect("what a Purge that stopped in its last step leaves");

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
    assert!(
        printed.contains("no Accounts") && !printed.contains("Purged 0"),
        "and what happened is said as what happened rather than as a count \
         of nothing:\n{printed}"
    );
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "and no Export was offered, because there is nothing left to put in one"
    );
}

/// A machine Perch has never run on has nothing to give back, and saying so is
/// not a failure worth a stack of advice.
#[test]
fn a_machine_perch_never_ran_on_has_nothing_to_give_back() {
    let host = machine_with_claude_code().with_answers(&["purge"]);

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("there is nothing here");
    assert_eq!(refused.exit_code(), EXIT_NOTHING_TO_DO, "{refused}");
    assert!(
        refused.to_string().contains(&perch_home_as_written(&host)),
        "{refused}"
    );
    assert!(
        !host.path_exists(Path::new(PERCH_HOME)),
        "and no directory was made on the way to saying so"
    );
}

/// A keychain item is filed under `$USER`, and a delete that finds nothing
/// reports success. The Account is still given up; what must not happen is Perch
/// claiming a Credential is beyond recovery when it may be sitting in a keychain
/// under a different login name.
#[test]
fn a_purge_that_found_no_credential_does_not_claim_to_have_deleted_one() {
    let host = a_machine_to_give_back();
    let store = store_of(&host, THIRD_EMAIL);
    host.forget_keychain_item(&store.keychain_service, &store.keychain_account);

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert!(printed.contains("$USER"), "{printed}");
    assert_eq!(registry_on(&host), None, "and the Purge still finished");
}

/// The questions a Purge asks are the one wait in Perch with no bound on them —
/// somebody may type the word in a second or come back after lunch — and the
/// registry lock is held across them. A hold this Perch has lost is a registry
/// somebody else has been changing since it was read, and everything past the
/// question deletes Credentials. Refused before the first of them rather than
/// discovered afterwards, by which point they are already gone.
#[test]
fn an_answer_that_arrives_after_another_perch_took_the_lock_purges_nothing() {
    let host = a_machine_to_give_back()
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::registry::lock_spec(host).expect("home is known");
            host.remove_dir_all(&lock.dir).expect("it was abandoned");
            host.create_dir_exclusive(&lock.dir)
                .expect("the other `perch` takes it");
        });

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("this Perch may no longer act on what it read");
    assert!(
        refused.to_string().contains("Nothing was purged"),
        "{refused}"
    );
    assert!(!printed.contains("Purged"), "{printed}");
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "every Account is still held"
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "and no Credential was touched"
    );
}

/// A Purge reads what is stored and asks Anthropic nothing: it is not a moment
/// to spend somebody's rate limit, and a Renewal on the way past would Rotate a
/// token on its way to being deleted.
#[test]
fn nothing_is_asked_of_anthropic_by_a_purge() {
    let host = a_machine_to_give_back();

    run_purge(&host).0.expect("the word was typed");

    assert!(host.http_calls().is_empty(), "{:?}", host.http_calls());
}

/// A Purge derives the Credential Stores it empties from the registry, and the
/// registry is not the whole account of what Perch is holding. A login
/// abandoned at the browser step leaves a working Credential in a `pending/`
/// directory, and nothing reaps one under thirty minutes old — so the ordinary
/// sequence of Ctrl-C at the login and `perch purge` a few minutes later used
/// to destroy the directory the keychain item's service name is derived from,
/// leaving a live refresh token nothing could ever find again while the report
/// said the machine had been given back.
#[test]
fn a_purge_takes_the_credential_an_abandoned_login_left_as_well() {
    let host = machine_with_two_accounts();

    let abandoned = perch::registry::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &abandoned).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    run_purge_with(&host, perch::commands::purge::PurgeArgs { yes: true })
        .0
        .expect("the machine is given back");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "the abandoned login's Credential lives outside Perch's home, and its \
         directory is the only thing that could ever name it"
    );
    assert_eq!(
        host.keychain_services(),
        vec![DEFAULT_SERVICE.to_string()],
        "and the only Credential still on the machine is Claude Code's own \
         login, which a Purge deliberately leaves alone"
    );
    assert!(!host.path_exists(Path::new(PERCH_HOME)));
}

/// The same hole from the other side: a `perch add` whose registry write failed
/// leaves a Profile holding a live Credential that no Account names.
#[test]
fn a_purge_takes_the_credential_of_a_profile_the_registry_never_recorded() {
    let host = machine_with_two_accounts();

    let orphan = perch::registry::profiles_dir(&host)
        .expect("home is known")
        .join("nobody-example-com");
    let store = perch::probe::store_for_profile(&host, &orphan).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    run_purge_with(&host, perch::commands::purge::PurgeArgs { yes: true })
        .0
        .expect("the machine is given back");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "a Profile nothing records is still a Profile Perch made"
    );
    assert_eq!(
        host.keychain_services(),
        vec![DEFAULT_SERVICE.to_string()],
        "leaving only Claude Code's own login"
    );
}

/// The home directory goes last and goes whole, so a home that will not go is a
/// Purge that took every Credential and stopped one step from done. It has to
/// say both halves: the Credentials really are gone, and the one thing left is
/// a directory that running it again will take.
#[test]
fn a_home_that_will_not_go_says_the_credentials_are_gone_and_the_rest_finishes_later() {
    let host = a_machine_to_give_back()
        .with_answers(&["n", "purge"])
        .with_undeletable_file(PERCH_HOME, "Device or resource busy");

    let (stopped, printed) = run_purge(&host);

    let failed = stopped.expect_err("the home directory would not go");
    let said = failed.to_string();
    assert!(
        said.contains("Every Credential Perch held is deleted"),
        "the destructive half really did happen: {said}"
    );
    // Rendered as the Host built it rather than as the constant spells it: home
    // is reached by joining, so the separators are the platform's.
    let home = perch::registry::perch_home(&host).expect("home is known");
    assert!(said.contains(&home.display().to_string()), "{said}");
    assert!(
        said.contains("Device or resource busy"),
        "what stopped it is what the user has to fix: {said}"
    );
    assert!(
        said.contains("Run `perch purge` again"),
        "and the way to finish is named: {said}"
    );

    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(
            credential_of(&host, email),
            None,
            "{email} really is given up, whatever the directory did: {printed}"
        );
    }

    host.deletable_again(Path::new(PERCH_HOME));
    let (finished, printed) =
        run_purge_with(&host, perch::commands::purge::PurgeArgs { yes: true });
    finished.expect("running it again finishes it");
    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
}

/// A Profile the registry does not name is reached by walking the directory, so
/// the walk is where a store that will not give a Credential up shows up. It
/// must not be shrugged off: a Purge that reported success with a Credential
/// still on the machine is the one failure this command cannot have.
#[test]
fn a_leftover_profile_whose_credential_will_not_go_stops_the_purge_rather_than_being_skipped() {
    let host = machine_with_two_accounts();

    let orphan = perch::registry::profiles_dir(&host)
        .expect("home is known")
        .join("nobody-example-com");
    let store = perch::probe::store_for_profile(&host, &orphan).expect("USER is set");
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    let host = host.with_undeletable_file(&store.credentials_file, "read-only");

    let (stopped, printed) = run_purge_with(&host, perch::commands::purge::PurgeArgs { yes: true });

    let failed = stopped.expect_err("a Credential that will not go is not a Purge that worked");
    let said = failed.to_string();
    assert!(
        said.contains("Perch's registry is untouched"),
        "so running it again is a whole Purge rather than a partial one: {said}"
    );
    assert!(said.contains("`perch purge` can be run again"), "{said}");
    assert!(
        registry_on(&host).is_some(),
        "the registry really is still there: {printed}"
    );
    assert!(
        host.path_exists(Path::new(PERCH_HOME)),
        "and so is home, so nothing claims to have finished"
    );
}

/// The other half of the same walk answers the way the first half does: a
/// directory that will not say where its Credential lives stops the Purge.
///
/// Reached the way it happens — a Purge that stopped in its last step, so the
/// registry is already gone — on a machine whose keychain account name cannot
/// be derived at all.
///
/// The two halves used to disagree. A store is unnameable when the platform
/// will not say who the user is, which is not a fact about one directory, so
/// `erase` refused at the first Account in the registry and shrugged the
/// identical directory off in the walk — Perch refusing to purge a machine
/// holding Accounts and reporting success on the same machine holding only
/// their leftovers. Removing the directory says nothing about the keychain item
/// beside it, and "the machine is given back" is exactly the claim a Purge must
/// not make while a working login is still stored.
#[test]
fn a_leftover_directory_that_names_no_store_stops_the_purge_rather_than_being_passed_over() {
    let host = machine_with_three_accounts();
    host.remove_file(Path::new(REGISTRY_PATH))
        .expect("what a Purge that stopped in its last step leaves");
    let host = host.without_env("USER").with_answers(&["purge"]);

    let (result, _) = run_purge(&host);

    let refusal = result.expect_err("a Credential that cannot be named cannot be deleted");
    assert!(
        refusal
            .to_string()
            .contains("already deleted is already gone"),
        "it says what a second run would finish: {refusal}"
    );
    assert!(
        host.path_exists(Path::new(PERCH_HOME)),
        "and the home is still there, because the Purge did not finish"
    );
}
