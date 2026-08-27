//! `perch holdings purge` — giving the machine back
//! (ADR the-holdings-go-out-sealed).
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
use perch::error::{EXIT_INVALID, EXIT_NOTHING_TO_DO, EXIT_PROBE_REFUSED, EXIT_PROFILE_LIVE};
use perch::export;
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{FakeHost, Platform, Refusing};
use perch::registry::{Quarantine, Registry};

const PERCH_HOME: &str = "/Users/someone/.config/perch";
const DEFAULT_PROFILE: &str = "/Users/someone/.claude";
const AT: &str = "/Users/someone/perch-backup.age";
const PASSPHRASE: &str = "correct horse battery staple";

/// Three Accounts with everything a user gives them, on a machine where
/// somebody declines the Export and types the word.
fn a_machine_to_give_back() -> FakeHost {
    let host = machine_with_three_accounts();
    a_group_of(&host, "work", &[EMAIL, SECOND_EMAIL]);
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
    perch::holdings::perch_home(host)
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
    // One line, asserted whole (ADR perch-says-what-it-did): how many, and the
    // directory they were kept in. That every Profile and every Credential went
    // with them is what a Purge *is*, and was said in the question.
    assert!(
        printed.trim_end().ends_with(&format!(
            "Purged 3 Accounts, and {} is gone.",
            perch_home_as_written(&host)
        )),
        "{printed}"
    );
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
    // Said where it is load-bearing — in the question this Purge was agreed to —
    // rather than again in the report. Asserted whole, because the sentence is
    // the claim.
    assert!(
        printed.contains(
            "Claude Code goes on running as whatever it is logged in as — the \
             live Credential is not Perch's to take away."
        ),
        "which is said, because it is the one thing a Purge deliberately leaves \
         behind:\n{printed}"
    );
}

/// Off macOS that login is a file inside the Default Profile rather than a
/// keychain item (ADR claude-code-chooses-the-store).
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

#[test]
fn the_word_is_taken_however_it_was_typed() {
    for answered in [&["n", "  purge  "], &["n", "PURGE"]] {
        let host = machine_with_three_accounts().with_answers(answered);

        run_purge(&host).0.expect("the word was typed");

        assert_eq!(registry_on(&host), None);
    }
}

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

#[test]
fn the_export_it_offers_is_written_before_anything_is_destroyed() {
    let host = a_machine_to_give_back()
        .with_answers(&["", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    let sealed = host.file(AT).expect("the Export is still there afterwards");
    let (exported, _) = export::unseal(&sealed, PASSPHRASE).expect("it opens");
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
    assert!(
        printed.contains(AT),
        "and the report names the file, which is now the only thing that names \
         the Holdings — every other way out of this command says so: {printed}"
    );
}

/// A Purge holds the registry lock across its offer, so its Export cannot go
/// through `perch holdings export` — and the settlement that command makes
/// before it reads a Credential Store is the one step that has to come with it.
/// Without it, every Credential is gathered out of its own Profile, where the
/// Account being left holds the copy a Rotation retired.
#[test]
fn the_export_it_offers_settles_a_landing_first() {
    let host = a_machine_to_give_back()
        .with_answers(&["", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);
    // A Switch that died after the Credential moved and before the Identity was
    // patched: the arriving Account's Credential is what is live.
    a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, SECOND_CREDENTIAL);

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    let sealed = host.file(AT).expect("the Export was written");
    let (exported, _) = export::unseal(&sealed, PASSPHRASE).expect("it opens");
    assert_eq!(
        exported.credentials.get(SECOND_EMAIL).map(String::as_str),
        Some(SECOND_CREDENTIAL),
        "the Account the Switch was arriving at travels as the live Credential \
         Perch settled on: {printed}"
    );
    assert_eq!(
        exported.registry.active().whose(),
        Some(SECOND_EMAIL),
        "and the Landing is settled in the file rather than traveling in it"
    );
}

/// The path typed at this prompt is the one path in Perch no shell has been
/// over, and `~/perch-backup.age` is the likeliest thing anybody types here —
/// where a refusal stops the whole Purge and every answer has to be given again.
#[test]
fn a_tilde_typed_at_the_export_prompt_means_home_because_no_shell_will_say_so() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", "~/perch-backup.age", "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    let sealed = host
        .file(AT)
        .unwrap_or_else(|| panic!("the Export is written under home: {printed}"));
    assert_eq!(
        export::unseal(&sealed, PASSPHRASE)
            .expect("it opens")
            .0
            .accounts(),
        3,
        "and it is the whole Export rather than an empty file at a strange path"
    );
}

/// A walk with no filter empties every Account the pass above has just emptied a
/// second time: on macOS that is a `security delete-generic-password` per Account
/// per pass, on the command whose whole job is to finish.
#[test]
fn a_purge_empties_each_credential_store_once() {
    let host = a_machine_to_give_back();
    let held = registry_on(&host).expect("a registry").accounts.len();
    host.forget_effects();

    run_purge(&host).0.expect("the word was typed");

    let deletes = host
        .effects()
        .iter()
        .filter(|effect| matches!(effect, Effect::KeychainDelete { .. }))
        .count();
    assert_eq!(
        deletes,
        held,
        "one keychain delete per Account, rather than one per Account per pass: \
         {:?}",
        host.effects()
    );
}

/// `~\backups\perch.age` is what somebody on Windows types, and Perch reads a
/// Windows path spelled either way everywhere else it reads one.
#[test]
fn a_windows_tilde_means_home_too_because_windows_writes_the_other_separator() {
    let host = a_machine_to_give_back()
        .with_platform(Platform::Windows)
        .with_answers(&["y", "~\\perch-backup.age", "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    assert!(
        host.file(AT).is_some(),
        "the Export is written under home: {printed}"
    );
}

/// The other half of a Windows path: rootedness was asked through
/// `Path::is_absolute`, which reads the separator of the platform the *build*
/// runs on — so `C:\...` was judged relative and joined onto the current
/// directory, and the Windows branch could not be honestly tested at all.
#[test]
fn a_windows_path_from_the_root_is_written_where_it_says() {
    let host = a_machine_to_give_back()
        .with_platform(Platform::Windows)
        .with_answers(&["y", "C:\\backups\\perch.age", "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);
    // An Export is refused where its directory is not there, and `Path::parent`
    // reads the separator of the platform this build runs on — so only a runner
    // that spells `\` finds a parent here. Made, so both ask one question.
    host.create_dir_all(Path::new("C:\\backups"))
        .expect("the directory is made");

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    assert!(
        host.file("C:\\backups\\perch.age").is_some(),
        "the Export is at the path that was typed: {printed}"
    );
}

/// The only thing that makes a Purge survivable must not be written where the
/// Purge is about to delete it — and `starts_with` matches components, so a
/// linked spelling of the same directory is a different string.
#[test]
fn an_export_path_that_reaches_perchs_home_through_a_link_is_refused_too() {
    for typed in [
        "/Users/someone/.config/perch/backup.age",
        // The same directory, reached by a name that shares no component with it.
        "/Users/someone/backups/backup.age",
    ] {
        let host = a_machine_to_give_back()
            .with_link(
                perch::host::Link::Symbolic,
                PERCH_HOME,
                "/Users/someone/backups",
            )
            .with_answers(&["y", typed, "purge"])
            .with_secrets(&[PASSPHRASE, PASSPHRASE]);

        let (outcome, printed) = run_purge(&host);

        let refused = outcome.expect_err("the Export would go with the home");
        assert!(
            refused.to_string().contains("Nothing was purged"),
            "{typed}: the Purge is off, and says so: {refused}"
        );
        assert_eq!(
            registry_on(&host).map(|registry| registry.accounts.len()),
            Some(3),
            "{typed}: and every Account is still here: {printed}"
        );
    }
}

/// A `..` was the third spelling of the same place, and the walk that follows
/// links stopped at it: a path typed at this prompt could double back into the
/// home the Purge is about to delete, and be judged outside it.
#[test]
fn an_export_that_doubles_back_into_perchs_home_is_refused_too() {
    for typed in [
        // Out of a sibling directory and back in.
        "/Users/someone/Documents/../.config/perch/backup.age",
        // The same, reaching the home by the name it is linked to.
        "/Users/someone/Documents/../backups/backup.age",
    ] {
        let host = a_machine_to_give_back()
            .with_link(
                perch::host::Link::Symbolic,
                "/Users/someone/backups",
                PERCH_HOME,
            )
            .with_file("/Users/someone/backups/.keep", "")
            .with_answers(&["y", typed, "purge"])
            .with_secrets(&[PASSPHRASE, PASSPHRASE]);

        let (outcome, printed) = run_purge(&host);

        let refused = outcome.expect_err("the Export would go with the home");
        // The guard's own refusal rather than any failure to write: every
        // export error carries "Nothing was purged", so the code and the named
        // directory are what tell the two apart.
        assert_eq!(refused.exit_code(), EXIT_INVALID, "{typed}: {refused}");
        assert!(
            refused.to_string().contains(&perch_home_as_written(&host)),
            "{typed}: it names Perch's own home: {refused}"
        );
        assert_eq!(
            registry_on(&host).map(|registry| registry.accounts.len()),
            Some(3),
            "{typed}: and every Account is still here: {printed}"
        );
    }
}

/// The relative half of the same rule. A bare filename is read as a file beside
/// the current directory, and where that directory is inside Perch's home the
/// Export lands in what the Purge deletes moments later — with the report saying
/// the Holdings are gone and nothing saying the backup went with them.
#[test]
fn an_export_named_relative_to_a_cwd_inside_perchs_home_is_refused_too() {
    let host = a_machine_to_give_back()
        .in_directory(PERCH_HOME)
        .with_answers(&["y", "backup.age", "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("the Export would go with the home");
    assert!(
        refused.to_string().contains("Nothing was purged"),
        "the Purge is off, and says so: {refused}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and every Account is still here: {printed}"
    );
}

/// Every guard downstream lets a verbatim `~` through: its parent is the current
/// directory, which exists; nothing is at that path; and it is not under Perch's
/// home, so the Purge would not take it. The Export lands at `./~`.
#[test]
fn a_tilde_this_prompt_cannot_expand_is_refused_rather_than_written_beside_the_cwd() {
    for typed in ["~", "~backups", "~someone/perch.age"] {
        let host = a_machine_to_give_back()
            .with_answers(&["y", typed, "purge"])
            .with_secrets(&[PASSPHRASE, PASSPHRASE]);

        let (outcome, printed) = run_purge(&host);

        let refused = outcome.expect_err("a `~` Perch cannot expand is not a path");
        assert!(
            refused.to_string().contains("Nothing was purged"),
            "the Purge is off, and says so: {refused}"
        );
        assert_eq!(
            registry_on(&host).map(|registry| registry.accounts.len()),
            Some(3),
            "and every Account is still here: {printed}"
        );
    }
}

/// The Export is offered before the word is asked for, so a Purge can be
/// declined *after* one is written — and `perch holdings export` refuses a path
/// that is taken, so the next Purge offering the same one aborts.
#[test]
fn declining_after_an_export_was_written_says_the_file_is_there() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "n"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let (outcome, printed) = run_purge(&host);

    outcome.expect("changing your mind is an answer, not a failure");
    assert!(printed.contains("Nothing was purged"), "{printed}");
    assert!(
        printed.contains(AT),
        "the file full of Credentials is named: {printed}"
    );
    assert!(
        host.file(AT).is_some(),
        "and it is still there, which is why it had to be said"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "nothing was given back: {printed}"
    );
}

/// A path that is taken is something somebody has to go and settle, and nothing
/// has been lost while they do.
#[test]
fn an_export_that_cannot_be_written_purges_nothing() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE])
        .with_file(AT, "an Export somebody else wrote");

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("something is already at that path");
    assert!(refused.to_string().contains(AT), "{refused}");
    // The Export's own refusal is about the Export, and true — but somebody who
    // typed `perch holdings purge` is waiting to hear about the Purge rather than
    // to infer it from a sentence about a file.
    assert!(
        refused.to_string().contains("Nothing was purged"),
        "a Purge stopped by the Export it offered still has to say the Purge is \
         off: {refused}"
    );
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
        registry.settle(None);
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

/// Every capability is reachable from a script (ADR perch-does-not-draw), and the
/// flag answers both questions, because an Export is a path and a passphrase.
#[test]
fn the_flag_purges_without_asking_anything() {
    let host = machine_with_three_accounts().without_terminal();
    host.forget_effects();

    let (outcome, printed) = run_purge_with(&host, true);
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

#[test]
fn without_a_terminal_and_without_the_flag_a_purge_is_refused_and_names_it() {
    let host = machine_with_three_accounts().without_terminal();

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("there is nobody to confirm with");
    assert_eq!(
        refused.exit_code(),
        EXIT_INVALID,
        "a request Perch understood and refused on its own terms, which a \
         script has to be able to tell from a disk that filled up: {refused}"
    );
    assert!(refused.to_string().contains("--yes"), "{refused}");
    assert!(
        registry_on(&host).is_some(),
        "and nothing was purged on the way to saying so"
    );
}

/// The rule every other write into a Live Profile obeys
/// (ADR a-profile-is-live-by-evidence), at its extreme.
#[test]
fn a_client_running_against_a_profile_stops_the_purge() {
    let host = a_machine_to_give_back();
    a_run_against(&host, SECOND_EMAIL, host.now());

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("something is holding that Profile");
    assert_eq!(refused.exit_code(), EXIT_PROFILE_LIVE, "{refused}");
    assert!(refused.to_string().contains(SECOND_EMAIL), "{refused}");
    assert!(
        refused.to_string().contains("pid "),
        "and which client to quit, since that is the whole of what the reader \
         has to do: {refused}"
    );
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

/// The registry is not the whole account of what Perch holds: a login in
/// progress lives under `pending/` and is in no `registry.accounts`, and it is
/// still a directory this command deletes.
#[test]
fn a_login_in_progress_stops_the_purge_though_no_account_names_it() {
    let host = a_machine_to_give_back();

    // What another terminal's `perch add` looks like from here: a directory
    // under `pending/`, with a client running against it.
    let pending = perch::holdings::pending_login_dir(&host, host.now()).expect("home is known");
    host.set_file(
        perch::probe::session_marker_at(&pending, 5150),
        &perch::probe::session_marker(5150, host.now()),
    );
    host.set_live_process(5150);

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("something is holding that login");
    assert_eq!(refused.exit_code(), EXIT_PROFILE_LIVE, "{refused}");
    assert!(
        refused.to_string().contains("no Account of Perch's names"),
        "there is no address to name it by, so it is named by its directory: \
         {refused}"
    );
    assert!(
        host.path_exists(&pending),
        "and the login somebody is in the middle of is still there"
    );
    assert!(
        registry_on(&host).is_some(),
        "along with everything else: a Purge is all or nothing"
    );
}

/// The window between the two checks is however long somebody takes over four
/// prompts, and a client started in it was not running when the first one ran.
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

/// An Export written a question ago is a file full of working Credentials at a
/// path the user is about to stop thinking about — and every other failure test
/// here answers `n` to the offer, which is why nothing was catching this.
#[test]
fn a_purge_that_wrote_an_export_and_then_stopped_says_the_file_is_there() {
    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE])
        // Somebody starts a client while the passphrase is being typed, which is
        // the window the second liveness check exists for.
        .with_a_terminal_that_takes(1_000)
        .once_while_waiting(|host| a_run_against(host, SECOND_EMAIL, host.now()));

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("something started holding that Profile");
    let said = refused.to_string();
    assert!(
        said.contains(AT),
        "the file that was written is named: {said}"
    );
    assert!(
        said.contains("will not write over it"),
        "and so is what the next Purge will do about it: {said}"
    );
    assert_eq!(
        export::unseal(&host.file(AT).expect("a file was written"), PASSPHRASE)
            .expect("it opens")
            .0
            .accounts(),
        3,
        "and it is a whole Export rather than a stub"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "with nothing purged"
    );
}

/// The Export's whereabouts is the last line of a finished Purge's report, so a
/// terminal that goes away partway through that report loses the one path
/// naming the Holdings — after the Holdings are gone.
#[test]
fn a_terminal_that_goes_away_during_the_report_does_not_lose_the_export() {
    /// Writes until the report of what was purged, and then is not there.
    struct GoesAwayReporting;

    impl std::io::Write for GoesAwayReporting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Purged") {
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

    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let outcome = perch::commands::purge::run(&host, false, &mut GoesAwayReporting);

    let refused = outcome.expect_err("the report could not be written");
    let said = refused.to_string();
    assert!(
        said.contains(AT),
        "the Export is named, and it is the only thing left that names the \
         Holdings: {said}"
    );
    assert!(
        said.contains("Purge itself finished"),
        "and what happened is said, because there is nothing to run again: {said}"
    );
    assert!(
        !host.path_exists(Path::new(PERCH_HOME)),
        "which is true: the Holdings went"
    );
}

/// `write_the_export` does one more fallible thing after the bytes have landed:
/// it reports what it wrote. An `Err` from that is not an Export that was never
/// written, and the armored file at the path is what has to be said.
#[test]
fn a_terminal_that_goes_away_after_the_export_lands_does_not_lose_the_file() {
    /// Writes until the Export's own report starts, and then is not there.
    struct GoesAwayReporting;

    impl std::io::Write for GoesAwayReporting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Exported") {
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

    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let outcome = perch::commands::purge::run(&host, false, &mut GoesAwayReporting);

    let refused = outcome.expect_err("the report could not be written");
    let said = refused.to_string();
    assert!(
        said.contains(AT),
        "the Export that was written is still named, whatever happened after it: {said}"
    );
    assert!(
        said.contains("will not write over it"),
        "and so is what the next Purge will do about it: {said}"
    );
    assert_eq!(
        export::unseal(&host.file(AT).expect("a file was written"), PASSPHRASE)
            .expect("it opens")
            .0
            .accounts(),
        3,
        "and it is the whole Export, which is what makes it worth saying"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "with nothing purged"
    );
}

/// The Credentials go first and the registry naming them goes last, so what is
/// already deleted is found already gone rather than lost track of.
#[test]
fn a_purge_that_stopped_part_way_finishes_when_it_is_run_again() {
    let second_profile = format!("{PERCH_HOME}/profiles/overflow-example-com/.credentials.json");
    let host = a_machine_to_give_back()
        .with_answers(&["n", "purge", "n", "purge"])
        .with_a_path_refusing(&second_profile, Refusing::Delete, "read-only");

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

    host.no_longer_refusing(&second_profile, Refusing::Delete);
    let (finished, printed) = run_purge(&host);

    finished.expect("running it again finishes it");
    assert_eq!(registry_on(&host), None, "{printed}");
    assert!(!host.path_exists(Path::new(PERCH_HOME)));
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(credential_of(&host, email), None, "{email}: {printed}");
    }
}

/// A home left behind with no registry in it is what an interrupted Purge
/// leaves, rather than a machine with nothing to do.
#[test]
fn a_home_left_behind_with_no_registry_is_taken_by_the_next_purge() {
    let host = machine_with_three_accounts().with_answers(&["purge"]);
    host.remove_file(Path::new(REGISTRY_PATH))
        .expect("what a Purge that stopped in its last step leaves");

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
    // The Profiles, because with no registry there are no Accounts to name — and
    // three working keychain items were deleted, so "no Accounts" understates it.
    assert!(
        printed.contains("3 Profiles Perch could not name") && !printed.contains("Purged 0"),
        "and what happened is said as what happened rather than as a count \
         of nothing:\n{printed}"
    );
    assert!(
        !host.effects().contains(&Effect::AskedInSecret),
        "and no Export was offered, because there is nothing left to put in one"
    );
}

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

/// A keychain item is filed under `$USER` and a delete that finds nothing reports
/// success, so a Credential may be sitting in a keychain under another name.
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

/// Off macOS there is no keychain and nothing is filed under `$USER`, so the one
/// sentence that exists to say where a Credential might still be would send
/// somebody looking in a keychain their machine does not have.
#[test]
fn a_purge_off_macos_explains_the_store_that_machine_actually_has() {
    let host = logged_in_machine_off_macos().with_answers(&["n", "purge"]);
    run_list(&host, false)
        .0
        .expect("the first command adopts the login there already is");
    // An Account whose Profile holds no credentials file, which is the whole of
    // what an empty store is where the store is a file.
    host.remove_file(&store_of(&host, EMAIL).credentials_file)
        .expect("the Profile's file goes");

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert!(
        printed.contains("nothing in either Credential Store"),
        "the sentence this is about is printed at all:\n{printed}"
    );
    assert!(
        !printed.contains("$USER") && !printed.contains("keychain"),
        "and a machine with no keychain is not told about one:\n{printed}"
    );
}

/// A keychain delete can put a dialog in front of somebody who then walks away,
/// which is the hazard `erase` renews the hold around. Past that point the
/// Credentials are gone, so the refusal must not be the one that says nothing
/// was done: somebody would go looking for a Credential that is not there.
#[test]
fn a_hold_lost_after_the_credentials_were_deleted_does_not_say_nothing_happened() {
    let host = a_machine_to_give_back()
        // Past the staleness window, on the first delete of three.
        .with_a_keychain_that_asks_first(120_000)
        .once_while_waiting(|host| {
            let lock = perch::holdings::lock_spec(host).expect("home is known");
            host.remove_dir_all(&lock.dir).expect("it was abandoned");
            host.create_dir_exclusive(&lock.dir)
                .expect("the other `perch` takes it");
        });

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("this Perch may no longer take the home away");
    let said = refused.to_string();
    assert!(
        !said.contains("Nothing was given back"),
        "every Credential is already deleted, so that sentence is false: {said}"
    );
    assert!(
        said.contains("already deleted"),
        "it says what did happen: {said}"
    );
    assert!(
        said.contains("purge` again"),
        "and that running it again finishes it: {said}"
    );
}

/// The questions a Purge asks are the one wait in Perch with no bound on them,
/// and the registry lock is held across them — so a hold this Perch has lost is
/// a registry somebody else has been changing since it was read.
#[test]
fn an_answer_that_arrives_after_another_perch_took_the_lock_purges_nothing() {
    let host = a_machine_to_give_back()
        // Past the staleness window, which is what makes the lock claimable.
        .with_a_terminal_that_takes(120_000)
        .once_while_waiting(|host| {
            let lock = perch::holdings::lock_spec(host).expect("home is known");
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

/// A Renewal on the way past would Rotate a token on its way to being deleted.
#[test]
fn nothing_is_asked_of_anthropic_by_a_purge() {
    let host = a_machine_to_give_back();

    run_purge(&host).0.expect("the word was typed");

    assert!(host.http_calls().is_empty(), "{:?}", host.http_calls());
}

/// A login abandoned at the browser step leaves a working Credential under
/// `pending/`, and nothing reaps one under thirty minutes old — so Ctrl-C and a
/// Purge a few minutes later is the ordinary sequence, not an exotic one.
#[test]
fn a_purge_takes_the_credential_an_abandoned_login_left_as_well() {
    let host = machine_with_two_accounts();

    let abandoned = perch::holdings::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &abandoned).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    run_purge_with(&host, true)
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

/// What a Purge that stopped in its last step leaves for the next one: a home
/// holding a registry and nothing else. Holding no Accounts is a real state here
/// rather than a machine Perch never ran on, so it is said as what happened.
#[test]
fn a_home_holding_a_registry_and_nothing_else_is_taken_and_said_as_that() {
    let host = machine_with_claude_code().with_answers(&["purge"]);
    host.set_file(REGISTRY_PATH, r#"{"version":2,"accounts":[]}"#);

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
    // Both halves of it: the question and the report. No Profiles either, so
    // neither says a count.
    assert_eq!(
        printed.matches("no Accounts").count(),
        2,
        "asked and answered as what happened rather than as a count of \
         nothing:\n{printed}"
    );
    assert!(!printed.contains("Profile"), "{printed}");
}

/// The prompt read "its registry says nothing this Perch can read" off the
/// Accounts being empty, which a registry that parsed perfectly can also be. A
/// login that died at the browser step is exactly that machine, and telling the
/// person their registry is corrupt is a false thing to be agreeing to in the
/// one command nothing undoes.
#[test]
fn a_readable_registry_naming_nobody_is_not_said_to_be_unreadable() {
    let host = machine_with_claude_code().with_answers(&["purge"]);
    host.set_file(REGISTRY_PATH, r#"{"version":2,"accounts":[]}"#);
    let landing = perch::holdings::pending_logins_dir(&host)
        .expect("home is known")
        .join("login-1");
    host.create_dir_all(&landing)
        .expect("the directory is made");

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    assert!(
        printed.contains("that it cannot name"),
        "the Profile is counted: {printed}"
    );
    assert!(
        !printed.contains("says nothing this Perch can read"),
        "and the registry it read is not called unreadable: {printed}"
    );
}

/// A `.DS_Store` beside the Profiles is not a Profile, and counting one tells
/// somebody agreeing to a Purge that Perch holds one it cannot name.
#[test]
fn a_stray_file_under_the_profiles_is_not_counted_as_one() {
    let host = machine_with_claude_code().with_answers(&["purge"]);
    host.set_file(REGISTRY_PATH, r#"{"version":2,"accounts":[]}"#);
    let stray = perch::holdings::profiles_dir(&host)
        .expect("home is known")
        .join(".DS_Store");
    host.set_file(&stray, "");

    let (outcome, printed) = run_purge(&host);

    outcome.expect("the word was typed");
    assert!(
        !printed.contains("Profile"),
        "nothing under the Profiles is a Profile: {printed}"
    );
}

/// The same from the other side: a `perch add` whose registry write failed
/// leaves a Profile holding a live Credential no Account names.
#[test]
fn a_purge_takes_the_credential_of_a_profile_the_registry_never_recorded() {
    let host = machine_with_two_accounts();

    let orphan = perch::holdings::profiles_dir(&host)
        .expect("home is known")
        .join("nobody-example-com");
    let store = perch::probe::store_for_profile(&host, &orphan).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    let (outcome, printed) = run_purge_with(&host, true);
    outcome.expect("the machine is given back");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "a Profile nothing records is still a Profile Perch made"
    );
    // And said, because the count of Accounts does not include it: a Credential
    // this deleted and never mentioned is one the report understates.
    assert!(
        printed.contains("1 Profile under it named no Account, and 1 Credential"),
        "the report says what it took beyond the Accounts:\n{printed}"
    );
    assert_eq!(
        host.keychain_services(),
        vec![DEFAULT_SERVICE.to_string()],
        "leaving only Claude Code's own login"
    );
}

/// A home that will not go is a Purge that took every Credential and stopped one
/// step from done, so it has to say both halves.
#[test]
fn a_home_that_will_not_go_says_the_credentials_are_gone_and_the_rest_finishes_later() {
    let host = a_machine_to_give_back()
        .with_answers(&["n", "purge"])
        .with_a_path_refusing(PERCH_HOME, Refusing::Delete, "Device or resource busy");

    let (stopped, printed) = run_purge(&host);

    let failed = stopped.expect_err("the home directory would not go");
    let said = failed.to_string();
    assert!(
        said.contains("Every Credential Perch held is deleted"),
        "the destructive half really did happen: {said}"
    );
    // Rendered as the Host built it rather than as the constant spells it: home
    // is reached by joining, so the separators are the platform's.
    let home = perch::holdings::perch_home(&host).expect("home is known");
    assert!(said.contains(&home.display().to_string()), "{said}");
    assert!(
        said.contains("Device or resource busy"),
        "what stopped it is what the user has to fix: {said}"
    );
    assert!(
        said.contains("Run `perch holdings purge` again"),
        "and the way to finish is named: {said}"
    );

    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(
            credential_of(&host, email),
            None,
            "{email} really is given up, whatever the directory did: {printed}"
        );
    }

    host.no_longer_refusing(Path::new(PERCH_HOME), Refusing::Delete);
    let (finished, printed) = run_purge_with(&host, true);
    finished.expect("running it again finishes it");
    assert!(!host.path_exists(Path::new(PERCH_HOME)), "{printed}");
}

/// A Purge reporting success with a Credential still on the machine is the one
/// failure this command cannot have, whichever pass reached it.
#[test]
fn a_leftover_profile_whose_credential_will_not_go_stops_the_purge_rather_than_being_skipped() {
    let host = machine_with_two_accounts();

    let orphan = perch::holdings::profiles_dir(&host)
        .expect("home is known")
        .join("nobody-example-com");
    let store = perch::probe::store_for_profile(&host, &orphan).expect("USER is set");
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);

    let host = host.with_a_path_refusing(&store.credentials_file, Refusing::Delete, "read-only");

    let (stopped, printed) = run_purge_with(&host, true);

    let failed = stopped.expect_err("a Credential that will not go is not a Purge that worked");
    let said = failed.to_string();
    assert!(
        said.contains("Perch's registry is untouched"),
        "so running it again is a whole Purge rather than a partial one: {said}"
    );
    assert!(
        said.contains("`perch holdings purge` can be run again"),
        "{said}"
    );
    assert!(
        registry_on(&host).is_some(),
        "the registry really is still there: {printed}"
    );
    assert!(
        host.path_exists(Path::new(PERCH_HOME)),
        "and so is home, so nothing claims to have finished"
    );
}

/// A store is unnameable when the platform will not say who the user is, which
/// is not a fact about one directory — so both halves of the walk have to answer
/// the same way, or Perch refuses one machine and reports success on its
/// leftovers. Reached the way it happens: a Purge that stopped in its last step.
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

/// Between the Export landing and the note that mentions it there is one more
/// prompt: the word `purge`, written to the terminal and answered from it. A
/// closed pty fails that too.
#[test]
fn a_terminal_that_goes_away_at_the_last_question_does_not_lose_the_export() {
    /// Writes until the Purge asks for the word, and then is not there.
    struct GoesAwayAsking;

    impl std::io::Write for GoesAwayAsking {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("to give the machine back") {
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

    let host = a_machine_to_give_back()
        .with_answers(&["y", AT, "purge"])
        .with_secrets(&[PASSPHRASE, PASSPHRASE]);

    let outcome = perch::commands::purge::run(&host, false, &mut GoesAwayAsking);

    let refused = outcome.expect_err("the question could not be put");
    let said = refused.to_string();
    assert!(
        said.contains(AT),
        "the Export that was written is still named: {said}"
    );
    assert!(
        said.contains("will not write over it"),
        "and so is what the next Purge will do about it: {said}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "with nothing purged"
    );
}

/// The deletion pass walks the list `everything_perch_holds` builds, so an
/// unlistable `pending/` read as empty leaves that pass nothing to refuse — and
/// the home goes whole, taking the only name reaching a keychain item under it.
#[test]
fn a_directory_perch_cannot_list_stops_the_purge_rather_than_reading_as_empty() {
    let host = a_machine_to_give_back();
    let pending = Path::new(PERCH_HOME).join("pending");
    host.create_dir_all(&pending).expect("the parent is there");
    let host =
        host.with_a_path_refusing(&pending, Refusing::List, "Permission denied (os error 13)");

    let (outcome, printed) = run_purge(&host);

    let refused = outcome.expect_err("a directory Perch cannot read is not an empty one");
    let said = refused.to_string();
    assert!(
        said.contains("pending"),
        "the refusal names the directory it could not walk: {said}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged: {printed}"
    );
    assert_eq!(credential_of(&host, EMAIL).as_deref(), Some(CREDENTIAL));
}

/// A registry a hand-edit or a half-written save left unparsable is the one
/// state with no way off the machine: `perch holdings import` refuses it too,
/// and on macOS a Credential Store is named after the Profile a `rm -rf` would
/// take away, so deleting the home by hand orphans every keychain item.
#[test]
fn a_registry_that_will_not_parse_does_not_stop_the_purge_that_does_not_read_one() {
    let host = machine_with_three_accounts().with_answers(&["purge"]);
    host.set_file(REGISTRY_PATH, r#"{"version":2,"accounts":[{"bogus":1}]}"#);

    let (outcome, printed) = run_purge(&host);
    outcome.expect("the word was typed");

    assert!(
        !host.path_exists(Path::new(PERCH_HOME)),
        "the Holdings are given back regardless: {printed}"
    );
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert_eq!(
            credential_of(&host, email),
            None,
            "and no Credential is left in a store nothing can name any more"
        );
    }
    // What it took, both before the question and after it. The registry names
    // nothing, so the Profiles are the only count there is — and "no Accounts"
    // is what three working logins must not be agreed to as.
    assert!(
        !printed.contains("no Accounts here"),
        "the question did not offer an empty machine:\n{printed}"
    );
    assert!(
        printed.contains("3 Profiles"),
        "it counted what it was about to take:\n{printed}"
    );
    assert!(
        printed.contains("Purged 3 Profiles Perch could not name, 3 Credentials"),
        "and the report counted what it took:\n{printed}"
    );
}

/// Doubt is not an answer. A `sessions` directory that is there and will not be
/// read — the root-owned one a `sudo claude` leaves — establishes nothing, and a
/// Purge that read it as "a client is running" would send somebody looking for a
/// client to quit that may not exist.
#[test]
fn a_sessions_directory_that_will_not_be_read_stops_the_purge_and_says_so() {
    let host = a_machine_to_give_back();
    let profile = perch::holdings::profile_dir_for(&host, SECOND_EMAIL).expect("home is known");
    let sessions = perch::probe::sessions_dir(&profile);
    host.create_dir_all(&sessions)
        .expect("a client has run here before");
    let host = host.with_a_path_refusing(&sessions, Refusing::List, "permission denied");

    let (outcome, _printed) = run_purge(&host);

    let refused = outcome.expect_err("whether a client is running got no answer");
    assert_eq!(
        refused.exit_code(),
        EXIT_PROBE_REFUSED,
        "nothing was established, which is not a Live Profile: {refused}"
    );
    assert!(
        refused.to_string().contains("make that directory readable"),
        "and it says what to do about the directory rather than naming a client \
         to quit: {refused}"
    );
    assert!(
        refused.to_string().contains("Nothing was purged"),
        "{refused}"
    );
    assert_eq!(
        registry_on(&host).map(|registry| registry.accounts.len()),
        Some(3),
        "and nothing was purged"
    );
}
