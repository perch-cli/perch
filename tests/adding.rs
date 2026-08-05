//! Behaviour: gaining a second Account without losing the one you are using.
//!
//! The thing under test is mostly an absence — `perch add` must leave the
//! active Account, its Credential, and its Identity exactly as it found them,
//! whether the login worked or not. So most of these assert on what did not
//! happen as much as on what did.

mod common;

use common::*;
use perch::Host;
use perch::commands::add::AddArgs;
use perch::error::{EXIT_CONFLICT, EXIT_NOT_FOUND};
use perch::host::{FakeHost, fake::Effect};

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
    assert_eq!(registry.active.as_deref(), Some(EMAIL));
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
            } => Some((program, config_dir)),
            _ => None,
        })
        .collect();

    assert_eq!(launched.len(), 1, "one login, launched once");
    let (program, config_dir) = &launched[0];
    assert_eq!(program, "claude");
    assert_ne!(
        config_dir,
        &host.home_dir().join(".claude"),
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
    assert!(
        printed.contains(EMAIL) && printed.contains("still the active Account"),
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

    assert!(
        printed.contains("Overflow Ltd"),
        "the organization should be offered as the default:\n{printed}"
    );
    assert_eq!(
        registry_of(&host)
            .account(SECOND_EMAIL)
            .unwrap()
            .group
            .as_deref(),
        Some("Overflow Ltd")
    );
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
        printed.contains("email address"),
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
            "/Users/someone/.perch/profiles/overflow-example-com/.claude.json",
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

#[test]
fn adding_an_account_makes_no_network_call() {
    let host = ready_to_add();

    run_add(&host, add_to_group("work")).0.unwrap();

    assert!(host.http_calls().is_empty());
}
