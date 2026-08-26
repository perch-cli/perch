//! Behavior: gaining a second Account without losing the one you are using.
//!
//! The thing under test is mostly an absence — `perch add` must leave the
//! active Account, its Credential, and its Identity exactly as it found them,
//! whether the login worked or not. So most of these assert on what did not
//! happen as much as on what did.

mod common;

use common::*;
use perch::commands::add::AddArgs;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND};
use perch::host::prelude::*;
use perch::host::{FakeHost, Platform, fake::Effect};

/// A machine with the first Account already adopted and a second login waiting
/// to be run.
fn ready_to_add() -> FakeHost {
    logged_in_machine().with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
}

/// The config directory the launched login was pointed at.
fn login_directory(host: &FakeHost) -> std::path::PathBuf {
    host.effects()
        .into_iter()
        .find_map(|effect| match effect {
            Effect::ExecInteractive { config_dir, .. } => Some(config_dir),
            _ => None,
        })
        .expect("a login was launched")
}

/// What every failure has to be true of: the Account you were using is still
/// there, still active, and its Credential is untouched.
fn assert_the_active_account_survived(host: &FakeHost) {
    let registry = registry_of(host);
    assert_eq!(registry.active().whose(), Some(EMAIL));
    assert_eq!(
        host.keychain_item(DEFAULT_SERVICE, LOGIN_NAME).as_deref(),
        Some(CREDENTIAL),
        "the live Credential must be exactly as Claude Code left it"
    );
    assert_eq!(
        host.file(IDENTITY_PATH).as_deref(),
        Some(IDENTITY_FILE),
        "the live Identity must be exactly as Claude Code left it"
    );
}

/// An Add records the Account and saves the registry before it reports, so a
/// stdout that will not take the report is a login that landed under a non-zero
/// exit — and unnoted it sends somebody back through a browser login for an
/// Account Perch already holds.
#[test]
fn an_add_that_landed_says_so_when_only_the_report_could_not_be_written() {
    /// A stdout that takes everything until the report itself. What is written
    /// before it is written before the Account is recorded.
    struct NowhereToWriteTheReport;

    impl std::io::Write for NowhereToWriteTheReport {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match String::from_utf8_lossy(bytes).contains("Added ") {
                true => Err(std::io::Error::other("No space left on device")),
                false => Ok(bytes.len()),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let host = ready_to_add();

    let refused =
        perch::commands::add::run(&host, add_to_group("work"), &mut NowhereToWriteTheReport)
            .expect_err("the report could not be written");

    assert!(
        refused.to_string().contains("Account was added"),
        "the failure says which half of the command it was: {refused}"
    );
    assert!(
        registry_of(&host).account(SECOND_EMAIL).is_some(),
        "and it was the reporting half, so the Account is held"
    );
}

#[test]
fn a_login_is_launched_inside_a_new_profile_of_its_own() {
    let host = ready_to_add();

    let (result, _) = run_add(&host, add_to_group("work"));
    assert!(result.is_ok(), "{:?}", result.err());

    let launched: Vec<_> = host
        .effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::ExecInteractive {
                program,
                config_dir,
                ..
            } => Some((program, config_dir)),
            _ => None,
        })
        .collect();

    assert_eq!(launched.len(), 1, "one login, launched once");
    let (program, config_dir) = &launched[0];
    assert_eq!(program, common::CLAUDE_BIN);
    assert_ne!(
        config_dir,
        &host.home_dir().unwrap().join(".claude"),
        "a login in the active Account's own directory would log it out"
    );
}

#[test]
fn the_account_is_recorded_under_the_email_the_login_produced() {
    let host = ready_to_add();

    run_add(&host, add_to_group("work")).0.unwrap();

    let registry = registry_of(&host);
    let added = registry
        .account(SECOND_EMAIL)
        .expect("the new Account is recorded under its email address");
    assert_eq!(
        added.identity.organization_name.as_deref(),
        Some("Overflow Ltd")
    );
    assert_eq!(
        added.plan.as_deref(),
        Some("max"),
        "the plan comes from the Credential the login produced"
    );
    assert_eq!(added.group.as_deref(), Some("work"));
}

#[test]
fn the_new_credential_lands_in_a_namespace_of_its_own() {
    let host = ready_to_add();

    run_add(&host, add_to_group("work")).0.unwrap();

    let first = store_of(&host, EMAIL);
    let second = store_of(&host, SECOND_EMAIL);

    assert_ne!(first.config_dir, second.config_dir);
    assert_ne!(
        first.keychain_service, second.keychain_service,
        "two Accounts must not share one namespace"
    );
    assert_ne!(
        first.credentials_file, second.credentials_file,
        "nor one file, on the platforms where that is the store"
    );
    assert_eq!(
        credential_of(&host, SECOND_EMAIL).as_deref(),
        Some(SECOND_CREDENTIAL)
    );
    assert_eq!(
        credential_of(&host, EMAIL).as_deref(),
        Some(CREDENTIAL),
        "the Account you were using keeps its own stored copy"
    );
}

#[test]
fn the_account_you_were_using_stays_active_and_untouched() {
    let host = ready_to_add();

    let (result, printed) = run_add(&host, add_to_group("work"));
    assert!(result.is_ok(), "{:?}", result.err());

    assert_the_active_account_survived(&host);
    // Asserted whole, because the claim *is* the sentence — and said once,
    // before the browser opens, rather than again in the report afterwards.
    assert!(
        printed.contains(&format!(
            "Logging in to a new Profile. {EMAIL} stays active and its session \
             is untouched."
        )),
        "the user should be told the session they are in survived:\n{printed}"
    );
    assert!(
        !host
            .effects()
            .contains(&Effect::WroteFile(IDENTITY_PATH.into())),
        "nothing may write the live Identity: that is what a Switch is for"
    );
}

#[test]
fn a_group_and_an_alias_are_applied_in_the_same_invocation() {
    let host = ready_to_add();

    run_add(
        &host,
        AddArgs {
            group: Some("work".into()),
            no_group: false,
            alias: Some("overflow".into()),
        },
    )
    .0
    .unwrap();

    let registry = registry_of(&host);
    assert_eq!(
        registry.aliases.get("overflow").map(String::as_str),
        Some(SECOND_EMAIL)
    );
    assert_eq!(
        registry.account(SECOND_EMAIL).unwrap().group.as_deref(),
        Some("work")
    );
}

#[test]
fn with_no_group_the_organization_is_offered_and_accepted_by_confirming() {
    let host = ready_to_add().with_answers(&[""]);

    let (result, printed) = run_add(&host, AddArgs::default());
    assert!(result.is_ok(), "{:?}", result.err());

    // `Overflow Ltd` is not a name a Group can carry — settings are read back a
    // word at a time — so its spaces become the separator, and the offer stands.
    assert!(
        printed.contains("Overflow-Ltd"),
        "the organization should be offered as the default:\n{printed}"
    );
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("Overflow-Ltd")
    );
}

/// The offer is built once, above the loop that asks. An offer the same command
/// would refuse is therefore not a bad default but an unanswerable question:
/// Enter re-asks it, with the same offer, for as long as somebody keeps pressing.
#[test]
fn a_group_the_command_would_refuse_is_never_offered() {
    // `--alias Overflow-Ltd` takes the name the organization would be offered
    // under, and one name cannot be both.
    let host = ready_to_add().with_answers(&[""]);
    let args = AddArgs {
        alias: Some("Overflow-Ltd".to_string()),
        ..AddArgs::default()
    };

    let (result, printed) = run_add(&host, args);
    assert!(result.is_ok(), "{:?}", result.err());

    assert!(
        !printed.contains("[Overflow-Ltd]"),
        "an offer this command would refuse is no help as a default:\n{printed}"
    );
    let registry = registry_of(&host);
    let added = registry
        .account(SECOND_EMAIL)
        .expect("the Account was added");
    assert_eq!(added.group, None, "and Enter left it in no Group");
    assert_eq!(registry.alias_of(SECOND_EMAIL), Some("Overflow-Ltd"));
}

#[test]
fn the_offered_group_is_only_a_default_and_can_be_answered_over() {
    let host = ready_to_add().with_answers(&["personal"]);

    run_add(&host, AddArgs::default()).0.unwrap();

    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("personal"),
        "three subscriptions bought personally must be able to share one Group"
    );
}

#[test]
fn a_group_name_perch_cannot_accept_is_asked_about_again_rather_than_losing_the_account() {
    // By the time the question is asked the login has already happened, so
    // failing the command over a typo would cost the Account it just gained.
    let host = ready_to_add().with_answers(&["overflow@example.com", "overflow"]);

    let (result, printed) = run_add(&host, AddArgs::default());

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("carries `@` (U+0040)"),
        "the user should be told why the first answer was no good:\n{printed}"
    );
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("overflow")
    );
}

#[test]
fn declining_the_offered_group_leaves_the_account_in_none() {
    let host = ready_to_add().with_answers(&["none"]);

    run_add(&host, AddArgs::default()).0.unwrap();

    assert_eq!(
        registry_of(&host).account(SECOND_EMAIL).unwrap().group,
        None
    );
}

#[test]
fn a_machine_with_no_terminal_is_told_to_name_the_group_rather_than_guessed_for() {
    let host = ready_to_add().without_terminal();

    let (result, _) = run_add(&host, AddArgs::default());

    let message = result
        .expect_err("there is nobody to confirm a Group")
        .to_string();
    assert!(
        message.contains("--group"),
        "the user should be told the fix:\n{message}"
    );
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "a login Perch was always going to refuse should never be launched"
    );
}

#[test]
fn no_group_is_available_without_a_terminal() {
    let host = ready_to_add().without_terminal();

    let (result, _) = run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    );

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        registry_of(&host).account(SECOND_EMAIL).unwrap().group,
        None
    );
}

#[test]
fn logging_in_as_an_account_perch_already_holds_is_refused_and_names_it() {
    let host = logged_in_machine().with_login(login_producing(CREDENTIAL, IDENTITY_FILE));

    let (result, _) = run_add(&host, add_to_group("work"));

    let error = result.expect_err("one Account cannot have two Profiles");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    let message = error.to_string();
    assert!(
        message.contains(EMAIL),
        "the existing entry should be named:\n{message}"
    );

    let registry = registry_of(&host);
    assert_eq!(
        registry.accounts.len(),
        1,
        "no second entry may be created for one Account"
    );
    assert_the_active_account_survived(&host);
}

#[test]
fn a_duplicate_is_named_by_its_alias_when_it_has_one() {
    let host = ready_to_add();
    run_add(
        &host,
        AddArgs {
            group: Some("work".into()),
            no_group: false,
            alias: Some("overflow".into()),
        },
    )
    .0
    .unwrap();

    let host = host.with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    let (result, _) = run_add(&host, add_to_group("work"));

    let message = result.expect_err("it is already held").to_string();
    assert!(
        message.contains("overflow"),
        "the entry should be named the way the user names it:\n{message}"
    );
}

#[test]
fn an_abandoned_login_leaves_no_half_created_account() {
    let host = logged_in_machine().with_login(abandoned_login());
    run_status(&host, false).0.unwrap();
    let services_before = host.keychain_services();

    let (result, _) = run_add(&host, add_to_group("work"));

    let error = result.expect_err("there is no Account to add");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("Nothing changed"),
        "the user should be told nothing was lost: {error}"
    );

    let registry = registry_of(&host);
    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(
        host.keychain_services(),
        services_before,
        "an abandoned login must leave nothing in the keychain"
    );
    assert_the_active_account_survived(&host);
}

#[test]
fn the_directory_a_login_ran_in_does_not_outlive_the_command() {
    let host = logged_in_machine();
    run_status(&host, false).0.unwrap();
    let services_before = host.keychain_services();

    let host = host.with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(&host, add_to_group("work")).0.unwrap();

    let config_dir = login_directory(&host);
    let profile_dir = store_of(&host, SECOND_EMAIL).config_dir;

    assert_ne!(
        config_dir, profile_dir,
        "the Profile is named for an Account only knowable once the login is done"
    );
    assert!(
        !host.path_exists(&config_dir),
        "the directory the login ran in should be gone: {}",
        config_dir.display()
    );
    assert_eq!(
        host.keychain_services().len(),
        services_before.len() + 1,
        "exactly one new namespace should survive: the new Profile's, not the \
         one the login briefly wrote into"
    );
}

#[test]
fn the_new_profile_keeps_the_identity_the_login_wrote() {
    let host = ready_to_add();

    run_add(&host, add_to_group("work")).0.unwrap();

    let carried = host
        .file(store_of(&host, SECOND_EMAIL).identity_file)
        .expect("the Profile keeps the Identity Claude Code wrote for it");
    assert!(carried.contains(SECOND_EMAIL));
}

#[test]
fn status_afterwards_still_reports_the_original_account() {
    let host = ready_to_add();
    run_add(&host, add_to_group("work")).0.unwrap();

    let (result, printed) = run_status(&host, false);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains(EMAIL),
        "the Account you were using is still the active one:\n{printed}"
    );
    assert!(
        !printed.contains(SECOND_EMAIL),
        "adding an Account does not switch to it:\n{printed}"
    );
}

#[test]
fn an_alias_that_is_already_a_group_name_is_refused_before_any_login() {
    let host = ready_to_add();
    run_add(&host, add_to_group("work")).0.unwrap();

    let host = host.with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    let effects_before = host.effects().len();
    let (result, _) = run_add(
        &host,
        AddArgs {
            group: Some("work".into()),
            no_group: false,
            alias: Some("work".into()),
        },
    );

    let error = result.expect_err("one name cannot mean two things");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        !host.effects()[effects_before..]
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "a name Perch was always going to refuse should not cost a login"
    );
}

#[test]
fn one_invocation_cannot_plant_the_collision_it_would_later_refuse() {
    let host = ready_to_add();

    let (result, _) = run_add(
        &host,
        AddArgs {
            group: Some("work".into()),
            no_group: false,
            alias: Some("work".into()),
        },
    );

    let error = result.expect_err("`work` cannot mean both an Alias and a Group");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::ExecInteractive { .. })),
        "and it should cost no login to find out"
    );
}

#[test]
fn a_profile_that_cannot_be_completed_is_not_left_half_built() {
    // A new Profile is filled in two steps: the Credential goes to the
    // keychain, then the Identity to a file. A failure between them would
    // otherwise leave a stored Credential that no registry entry names.
    let host = logged_in_machine();
    run_status(&host, false).0.unwrap();
    let services_before = host.keychain_services();

    let host = host
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
        .with_unwritable_file(
            "/Users/someone/.config/perch/profiles/overflow-example-com/.claude.json",
            "Permission denied (os error 13)",
        );
    let (result, _) = run_add(&host, add_to_group("work"));

    assert!(result.is_err(), "the Profile could not be completed");
    assert_eq!(
        registry_of(&host).accounts.len(),
        1,
        "no Account is recorded"
    );
    assert_eq!(
        host.keychain_services(),
        services_before,
        "and no Credential is left in a namespace nothing names"
    );
    assert_the_active_account_survived(&host);
}

/// One step *earlier* than either: the Credential itself will not go down.
/// `profile::create` sat outside every caller's undo, because each one opens
/// its undo with the Store this hands back — so what it left was a directory,
/// and on the path where the fallback store takes the Credential and the
/// preferred one keeps the copy it replaces, a live Credential inside it.
#[test]
fn a_profile_whose_credential_would_not_go_down_is_not_left_behind() {
    let host = logged_in_machine();
    run_status(&host, false).0.unwrap();

    let host = host
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
        .with_platform(Platform::Other)
        .with_unwritable_file(
            "/Users/someone/.config/perch/profiles/overflow-example-com/.credentials.json",
            "Permission denied (os error 13)",
        );
    let profile = store_of(&host, SECOND_EMAIL).config_dir;

    let (result, _) = run_add(&host, add_to_group("work"));

    result.expect_err("the Credential could not be stored");
    assert!(
        !host.path_exists(&profile),
        "the directory this made goes with the failure that made it useless"
    );
    assert_the_active_account_survived(&host);
}

/// The same one step later: the Profile is complete and the registry will not
/// take it. A Profile nothing records holds a live refresh token that
/// `reap_abandoned` never walks, since that only walks `pending/`.
#[test]
fn a_profile_the_registry_would_not_record_is_taken_back_out_again() {
    let host = logged_in_machine();
    run_status(&host, false).0.unwrap();
    let services_before = host.keychain_services();

    let host = host
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
        .with_unwritable_file(REGISTRY_PATH, "Permission denied (os error 13)");

    let (result, _) = run_add(&host, add_to_group("work"));

    let refusal = result.expect_err("the registry could not be written");
    assert!(
        refusal.to_string().contains("taken back out"),
        "it says the machine is as it was: {refusal}"
    );
    assert_eq!(
        host.keychain_services(),
        services_before,
        "no Credential is left in a namespace nothing names"
    );
    assert!(
        host.file("/Users/someone/.config/perch/profiles/overflow-example-com/.claude.json")
            .is_none(),
        "and no Profile directory either"
    );
    assert_the_active_account_survived(&host);
}

#[test]
fn adding_an_account_makes_no_network_call() {
    let host = ready_to_add();

    run_add(&host, add_to_group("work")).0.unwrap();

    assert!(host.http_calls().is_empty());
}

/// The interleaving Perch's registry lock exists for: `perch add` reads the
/// registry, spends minutes in a browser, and another terminal Switches while it
/// waits (ADR a-switch-is-written-down-first).
#[test]
fn an_add_does_not_revert_a_switch_that_ran_while_its_login_was_open() {
    let host = machine_with_two_accounts().with_login(|host, dir| {
        // Another terminal, while the browser is open. It lands, and records
        // itself as active.
        run_switch(host, SECOND_EMAIL)
            .0
            .expect("the other terminal switches");
        login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE)(host, dir)
    });

    let (result, printed) = run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    );

    result.expect("the Account is added");
    let registry = registry_of(&host);
    assert!(registry.account(THIRD_EMAIL).is_some(), "{printed}");
    assert_eq!(
        registry.active().whose(),
        Some(SECOND_EMAIL),
        "the Switch that happened during the login stands: an add records an \
         Account, it does not put the whole registry back to what it was"
    );
}

/// Plus-addressing on one inbox, which is exactly how somebody comes to hold
/// several Anthropic Accounts, produces two addresses that slug to one Profile
/// (ADR claude-code-chooses-the-store).
#[test]
fn a_login_whose_profile_is_already_held_is_refused_even_under_another_address() {
    const DOTTED: &str = "team.lead@example.com";
    const PLUSSED: &str = "team+lead@example.com";
    assert_eq!(
        perch::registry::slug(DOTTED),
        perch::registry::slug(PLUSSED),
        "the two addresses this test is about flatten to one Profile"
    );

    let host = logged_in_machine().with_login(login_producing(
        leaked(&CREDENTIAL.replace("oat01-test", "oat01-dotted")),
        leaked(&IDENTITY_FILE.replace(EMAIL, DOTTED)),
    ));
    run_add(&host, no_group()).0.expect("a second Account");

    let host = host.with_login(login_producing(
        leaked(&CREDENTIAL.replace("oat01-test", "oat01-plussed")),
        leaked(&IDENTITY_FILE.replace(EMAIL, PLUSSED)),
    ));
    let (result, _) = run_add(&host, no_group());

    let error = result.expect_err("both Accounts would be kept in one Profile");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    let message = error.to_string();
    assert!(
        message.contains(DOTTED) && message.contains(PLUSSED),
        "both addresses are named, since neither is obviously the wrong \
         one:\n{message}"
    );

    let registry = registry_of(&host);
    assert_eq!(registry.accounts.len(), 2);
    assert!(
        credential_of(&host, DOTTED).is_some_and(|held| held.contains("oat01-dotted")),
        "and the Credential already in that Profile is untouched"
    );
    assert_the_active_account_survived(&host);
}

/// A `&'static str` from a string built at run time, for the fixtures that take
/// one. The test process ends soon enough that leaking a few is nothing.
fn leaked(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

fn no_group() -> AddArgs {
    AddArgs {
        no_group: true,
        ..AddArgs::default()
    }
}

/// The same rule catching the case `switch` and `relogin` already treat as one
/// Account: a login under a differently capitalized spelling of an address
/// Perch holds is that Account, not a second one.
#[test]
fn a_login_under_a_differently_capitalized_address_is_the_account_perch_holds() {
    let shouted = EMAIL.to_uppercase();
    let host = logged_in_machine().with_login(login_producing(
        CREDENTIAL,
        leaked(&IDENTITY_FILE.replace(EMAIL, &shouted)),
    ));

    let (result, _) = run_add(&host, no_group());

    let error = result.expect_err("one Account cannot have two entries");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains("perch relogin"),
        "and the way back is the one that repairs it in place: {error}"
    );
    assert_eq!(registry_of(&host).accounts.len(), 1);
}

/// And the same for an address whose capitalization is only a capitalization
/// outside ASCII: `same_profile` lowercases over the whole of Unicode, so asking
/// whether it is *one Account* in ASCII would make these two.
#[test]
fn a_login_under_an_accented_spelling_is_the_account_perch_holds_too() {
    let accented = "café@example.com";
    let shouted = "CAFÉ@example.com";
    // The Account Perch adopts is the lower-case spelling; the login it is then
    // offered is the upper-case one.
    let host = machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file(
            IDENTITY_PATH,
            leaked(&IDENTITY_FILE.replace(EMAIL, accented)),
        )
        .with_login(login_producing(
            CREDENTIAL,
            leaked(&IDENTITY_FILE.replace(EMAIL, shouted)),
        ));

    let (result, _) = run_add(&host, no_group());

    let error = result.expect_err("one Account cannot have two entries");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains("perch relogin"),
        "the way back is the repair, not an address that does not exist: {error}"
    );
    assert_eq!(registry_of(&host).accounts.len(), 1);
}

/// Ctrl-C during a login kills Perch without unwinding, so what is left behind
/// is a complete, working Credential in a directory named after the moment the
/// login started (ADR a-login-perch-does-not-need).
#[test]
fn what_an_abandoned_login_left_behind_is_reaped_by_the_next_command() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first Account is adopted");

    // What a login that was interrupted after Claude Code wrote its Credential
    // and before Perch cleared up leaves on the machine.
    let abandoned = perch::registry::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &abandoned).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.credentials_file, SECOND_CREDENTIAL);
    assert!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .is_some(),
        "as we start"
    );

    // A command run straight afterwards leaves it alone: somebody may still be
    // in the middle of that login in another terminal.
    run_list(&host, false).0.expect("a listing");
    assert!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .is_some(),
        "a login half an hour old is one somebody may still be driving"
    );

    host.set_now(host.now() + chrono::Duration::hours(2));
    run_list(&host, false).0.expect("a listing");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None,
        "the orphaned Credential is gone from the keychain, where nothing but \
         this could ever have found it"
    );
    assert_eq!(host.file(&store.credentials_file), None);
    assert!(!host.path_exists(&abandoned), "and so is its directory");
}

/// A name that is already an Alias is exactly as much a typo as a name with a
/// space in it, and the prompt re-asks rather than failing either way.
#[test]
fn a_group_name_that_is_already_an_alias_is_asked_about_again() {
    let host = ready_to_add().with_answers(&["overflow", "work"]);
    set_alias(&host, "overflow", EMAIL).0.expect("named");

    let (result, printed) = run_add(&host, AddArgs::default());

    result.expect("the login is not thrown away over a name");
    assert!(
        printed.contains("already an Alias") || printed.contains("already names"),
        "the collision is explained where it can still be corrected:\n{printed}"
    );
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .expect("the Account was added")
            .group
            .as_deref(),
        Some("work"),
        "and the second answer is the one that stands"
    );
}

/// A login that exits cleanly having written nothing is somebody who closed the
/// browser tab rather than one whose Claude Code failed. A status of zero says
/// nothing, so the refusal must not pretend it does.
#[test]
fn a_login_that_finished_without_logging_anybody_in_says_just_that() {
    let host = logged_in_machine().with_login(|_host, _dir| 0);

    let (result, _) = run_add(&host, AddArgs::default());

    let said = result.expect_err("nobody logged in").to_string();
    assert!(said.contains("The login did not complete"), "{said}");
    assert!(
        !said.contains("exited 0"),
        "an exit status of zero is not a reason: {said}"
    );
    assert!(said.contains("Nothing changed."), "{said}");
    assert_eq!(
        registry_of(&host).accounts.len(),
        1,
        "and no Account was gained"
    );
}

/// The other half of the same branch: a login that failed has a status, and the
/// status is the only extra thing anybody can act on.
#[test]
fn a_login_that_exited_badly_repeats_the_status_it_exited_with() {
    let host = logged_in_machine().with_login(|_host, _dir| 7);

    let (result, _) = run_add(&host, AddArgs::default());

    let said = result.expect_err("the login failed").to_string();
    assert!(said.contains("The login exited 7"), "{said}");
    assert_eq!(registry_of(&host).accounts.len(), 1);
}

/// A `.claude.json` that is there but cannot be read is a different thing from
/// one that is absent, and saying "nobody logged in" would be the wrong
/// diagnosis: the login may well have worked. The refusal names the file.
#[test]
fn a_login_whose_identity_file_cannot_be_read_is_refused_by_name() {
    let host = logged_in_machine();
    let pending = perch::registry::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &pending).expect("USER is set");

    let host = host
        .with_unreadable_file(&store.identity_file, "permission denied")
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));

    let (result, _) = run_add(&host, AddArgs::default());

    let said = result
        .expect_err("the Identity could not be read")
        .to_string();
    assert!(said.contains("could not read"), "{said}");
    assert!(said.contains(".claude.json"), "it names the file: {said}");
    assert!(
        !said.contains("did not complete"),
        "an unreadable file is not an abandoned login: {said}"
    );
    assert_eq!(registry_of(&host).accounts.len(), 1);
}

/// Age is evidence and not proof, and the reaper runs at the start of every
/// command: a login somebody is in the middle of is a Live Profile, and nothing
/// reaps one (ADR a-profile-is-live-by-evidence).
#[test]
fn a_login_somebody_is_still_driving_is_never_reaped_however_old_it_is() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first Account is adopted");

    let pending = perch::registry::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &pending).expect("USER is set");
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.identity_file, SECOND_IDENTITY_FILE);

    // The `claude` this login is being driven through, still running.
    host.set_file(
        perch::probe::session_marker_at(&pending, 4242),
        &perch::probe::session_marker(4242, host.now()),
    );
    host.set_live_process(4242);

    // Long past the point where an abandoned one would have gone.
    host.set_now(host.now() + chrono::Duration::hours(6));
    run_list(&host, false).0.expect("a listing");

    assert!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .is_some(),
        "the Credential Claude Code just wrote is still there"
    );
    assert!(
        host.path_exists(&pending),
        "and so is the directory the login is running in"
    );
}

/// On macOS the keychain item lives outside the directory its name is derived
/// from, so a store that refuses stops the removal rather than being shrugged
/// off. What is left is a Profile nothing names: untidy, and recoverable.
#[test]
fn a_store_that_will_not_give_a_credential_up_keeps_the_directory_that_names_it() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first Account is adopted");

    let abandoned = perch::registry::pending_login_dir(&host, host.now()).expect("home is known");
    let store = perch::probe::store_for_profile(&host, &abandoned).expect("USER is set");
    // The Credential in the keychain, and the directory it is named after — on
    // macOS the item is outside the directory, which is the whole hazard.
    host.set_keychain_item(&store.keychain_service, LOGIN_NAME, SECOND_CREDENTIAL);
    host.set_file(&store.identity_file, SECOND_IDENTITY_FILE);
    host.lock_keychain("User interaction is not allowed");

    host.set_now(host.now() + chrono::Duration::hours(2));
    run_list(&host, false).0.expect("a listing");

    assert!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME)
            .is_some(),
        "the locked keychain kept it, which is the situation being tested"
    );
    assert!(
        host.path_exists(&abandoned),
        "so the directory stays: it is the only thing that can name that \
         keychain item, and the next reap is what finishes the job"
    );
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("name the store")),
        "and the remark says why it is still there: {:?}",
        host.notes()
    );

    // The keychain comes back, and the reap that could not finish finishes.
    host.unlock_keychain();
    run_list(&host, false).0.expect("a listing");

    assert_eq!(
        host.keychain_item(&store.keychain_service, LOGIN_NAME),
        None
    );
    assert!(!host.path_exists(&abandoned));
}

/// A directory name the reaper cannot read a moment out of is left alone rather
/// than guessed at: being wrong that way costs a stale directory, and being
/// wrong the other way costs somebody the login they are in the middle of.
#[test]
fn a_pending_login_directory_with_an_unreadable_name_is_never_reaped() {
    let host = logged_in_machine();
    run_list(&host, false).0.expect("the Account is adopted");

    let pending = perch::registry::pending_logins_dir(&host).expect("home is known");
    let unnamed = pending.join("login-not-a-moment");
    host.set_file(unnamed.join("marker"), "left by something else");

    // Far past the point where anything reapable would have gone.
    host.set_now(host.now() + chrono::Duration::days(30));
    run_list(&host, false).0.expect("a listing");

    assert!(
        host.path_exists(&unnamed),
        "a directory whose age cannot be established is left alone"
    );
}

/// What is wrong with a name is said before what it clashes with, so the same
/// mistake does not get two answers depending on how much of it was made.
#[test]
fn a_name_that_is_not_usable_is_refused_as_that_rather_than_as_a_collision() {
    let host = ready_to_add();

    let (result, _) = run_add(
        &host,
        AddArgs {
            alias: Some(String::new()),
            group: Some(String::new()),
            ..AddArgs::default()
        },
    );

    let refusal = result.expect_err("neither name is usable");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(refusal.to_string().contains("cannot be empty"), "{refusal}");
}

/// The protection above has to exist without anybody arranging it: Perch writes
/// the session marker itself, before the browser opens, because it is waiting on
/// this login exactly as a Run waits on its client (ADR a-run-is-one-shot).
#[test]
fn a_login_perch_is_driving_survives_a_command_run_in_another_terminal() {
    let host = logged_in_machine();
    run_list(&host, false)
        .0
        .expect("the first Account is adopted");

    let writes_what_claude_code_would = login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE);
    let host = host.with_login(move |host, dir| {
        let ended = writes_what_claude_code_would(host, dir);
        // Somebody hunting for their second factor, and a scheduled command
        // firing past the window while they do.
        host.set_now(host.now() + chrono::Duration::minutes(31));
        run_list(host, false)
            .0
            .expect("a listing in another terminal");
        ended
    });

    let (result, printed) = run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    );

    result.expect("the login completes rather than being reaped mid-flight");
    assert!(
        registry_of(&host).account(SECOND_EMAIL).is_some(),
        "and the Account it produced is held: {printed}"
    );
}

/// A Scope that has just grown to two Accounts holds a set nothing may Cycle
/// between unasked (ADR a-group-is-a-declaration). The Add says what is now true
/// and names the Setting that would say yes; it does not ask.
#[test]
fn an_add_that_makes_a_group_a_set_says_what_that_group_still_cannot_do() {
    let host = ready_for_a_third_in("work");

    let (result, printed) = run_add(&host, add_to_group("work"));

    result.expect("the third Account is added");
    assert!(
        printed.contains("perch config set work watcher-may-act true"),
        "the Setting and the Scope it is said about are both named:\n{printed}"
    );
    assert!(
        !printed.contains("interchangeable"),
        "a Group *is* the declaration that its Accounts are interchangeable \
         (ADR a-group-is-a-declaration), so there is no second yes to name:\n{printed}"
    );
}

/// The same question asked of the other Scope there is, which needs two yeses
/// rather than one: being ungrouped is the absence of a declaration that those
/// Accounts are interchangeable, not a weaker form of one.
#[test]
fn an_add_that_makes_the_ungrouped_scope_a_set_names_both_settings_it_needs() {
    let host = ready_to_add();

    let (result, printed) = run_add(&host, no_group());

    result.expect("the second Account is added");
    assert!(
        printed.contains("perch config set ungrouped interchangeable true"),
        "the declaration comes first, because it is the one that has to:\n{printed}"
    );
    assert!(
        printed.contains("perch config set ungrouped watcher-may-act true"),
        "and the grant that lets the watcher act on them:\n{printed}"
    );
}

/// A Scope somebody has already permitted is told nothing: the line exists to
/// say what is missing, and nothing is.
#[test]
fn an_add_into_a_scope_that_may_already_be_acted_on_says_nothing_about_settings() {
    let host = ready_for_a_third_in("work");
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("the Group is told the watcher may act on it");

    let (result, printed) = run_add(&host, add_to_group("work"));

    result.expect("the third Account is added");
    assert!(
        !printed.contains("perch config set"),
        "nothing is missing, so nothing is said:\n{printed}"
    );
}

/// And neither is the Scope that has just gained its first Account. One
/// Account is not a set worth Cycling within, and a rule for choosing has
/// nothing to say to a set of one.
#[test]
fn an_add_that_leaves_a_scope_holding_one_account_says_nothing_about_settings() {
    let host = ready_to_add();

    let (result, printed) = run_add(&host, add_to_group("work"));

    result.expect("the second Account is added, in a Group of its own");
    assert!(
        !printed.contains("perch config set"),
        "`work` holds one Account, and there is nothing to Cycle between:\n{printed}"
    );
}

/// The line is a statement rather than a question. `add` asks about the Group
/// and nothing else, so a `--no-group` add with no terminal to answer on still
/// completes — and still says what the Scope it grew cannot do.
#[test]
fn the_line_is_said_rather_than_asked_about() {
    let host = ready_to_add().without_terminal();

    let (result, printed) = run_add(&host, no_group());

    result.expect("nothing was waiting on an answer");
    assert!(
        printed.contains("perch config set ungrouped watcher-may-act true"),
        "{printed}"
    );
    assert!(
        !printed.contains('?'),
        "a consent gate is not collected in the middle of an Add:\n{printed}"
    );
}

/// And said again on the Add after that one, for as long as it stays true: a
/// fact about the Scope rather than a command explaining itself
/// (ADR perch-says-what-it-did).
#[test]
fn a_scope_that_stays_unanswered_is_told_again_on_the_add_after_that() {
    let host = ready_for_a_third_in("work");
    run_add(&host, add_to_group("work"))
        .0
        .expect("the third Account is added");
    let host = host.with_login(login_producing(
        SPARE,
        leaked(&SECOND_IDENTITY_FILE.replace(SECOND_EMAIL, "fourth@example.com")),
    ));

    let (result, printed) = run_add(&host, add_to_group("work"));

    result.expect("the fourth Account is added");
    assert!(
        printed.contains("perch config set work watcher-may-act true"),
        "still true, so still said:\n{printed}"
    );
    assert!(printed.contains("3 Accounts"), "and counted:\n{printed}");
}

/// A machine holding a second Account in one Group, with a third login waiting
/// — so the Add under test is the one that makes that Group a set of two.
fn ready_for_a_third_in(group: &str) -> FakeHost {
    let host = ready_to_add();
    run_add(&host, add_to_group(group))
        .0
        .expect("the second Account is added");
    host.with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE))
}
