//! Behaviour: naming Accounts, and what a typed Target turns out to mean.
//!
//! No command should ever require typing an email address, which is what an
//! Alias is for. The rest of these tests are about the one namespace Aliases
//! and Group names share: because a collision is refused when it is created,
//! the single Target every command takes always has one answer, and
//! Perch can say which kind of thing it matched.

mod common;

use common::*;
use perch::commands::add::AddArgs;
use perch::commands::alias::AliasCommand;
use perch::commands::group::GroupCommand;
use perch::error::{EXIT_CONFLICT, EXIT_INVALID, EXIT_NOT_FOUND};
use perch::host::FakeHost;
use perch::target::{self, Target};

/// Two Accounts, the second one already named `overflow`.
fn machine_with_a_named_second_account() -> FakeHost {
    let host =
        logged_in_machine().with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            alias: Some("overflow".into()),
            ..AddArgs::default()
        },
    )
    .0
    .unwrap();
    host
}

#[test]
fn an_alias_names_an_account_and_survives_a_restart() {
    let host = machine_with_two_accounts();

    let (result, printed) = set_alias(&host, "work", SECOND_EMAIL);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("work") && printed.contains(SECOND_EMAIL),
        "the name and what it now reaches should both be said:\n{printed}"
    );
    assert_eq!(
        registry_of(&host).aliases.get("work").map(String::as_str),
        Some(SECOND_EMAIL),
        "the name is written down, not held for this command only"
    );
}

#[test]
fn an_alias_can_be_given_to_an_account_that_is_already_named() {
    let host = machine_with_a_named_second_account();

    let (result, printed) = set_alias(&host, "work", "overflow");

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("overflow"),
        "the name being given up should be said, not silently dropped:\n{printed}"
    );
    let registry = registry_of(&host);
    assert_eq!(
        registry.aliases.get("work").map(String::as_str),
        Some(SECOND_EMAIL)
    );
    assert!(
        !registry.aliases.contains_key("overflow"),
        "an Account answers to one Alias, so the old name is freed rather than kept alive"
    );
}

#[test]
fn unsetting_an_alias_frees_the_name_for_something_else() {
    let host = machine_with_a_named_second_account();

    let (result, printed) = unset_alias(&host, "overflow");

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("overflow"),
        "the freed name should be said:\n{printed}"
    );
    assert!(registry_of(&host).aliases.is_empty());
    assert!(
        run_group(
            &host,
            GroupCommand::Add {
                name: "overflow".into(),
            },
        )
        .0
        .is_ok(),
        "a freed name is free for the other half of the namespace too"
    );
}

#[test]
fn unsetting_a_name_that_reaches_nothing_is_refused() {
    let host = machine_with_a_named_second_account();

    let (result, _) = unset_alias(&host, "overflw");

    let error = result.expect_err("there is no such Alias");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("overflow"),
        "a near miss should be suggested rather than left to be guessed at:\n{error}"
    );
}

#[test]
fn an_alias_colliding_with_a_group_name_is_refused_and_names_the_group() {
    let host = machine_with_two_accounts();
    declare_group(&host, "work");

    let (result, _) = set_alias(&host, "work", SECOND_EMAIL);

    let error = result.expect_err("a name cannot be both an Alias and a Group");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    let message = error.to_string();
    assert!(
        message.contains("work") && message.contains("Group"),
        "the Group standing in the way should be named:\n{message}"
    );
    assert!(
        registry_of(&host).aliases.is_empty(),
        "and nothing was named"
    );
}

#[test]
fn an_alias_named_while_adding_an_account_goes_through_the_same_namespace() {
    let host = logged_in_machine();
    declare_group(&host, "work");
    let host = host.with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));

    let (result, _) = run_add(
        &host,
        AddArgs {
            no_group: true,
            alias: Some("work".into()),
            ..AddArgs::default()
        },
    );

    let error = result.expect_err("`work` is already a Group");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains("Group"),
        "the Group standing in the way should be named:\n{error}"
    );
}

#[test]
fn an_alias_that_would_be_ambiguous_is_refused_with_its_own_code() {
    let host = machine_with_two_accounts();

    for name in ["none", "someone@example.com", "", " work"] {
        let (result, _) = set_alias(&host, name, SECOND_EMAIL);
        let error = result.expect_err("this cannot be an Alias");
        assert_eq!(
            error.exit_code(),
            EXIT_INVALID,
            "a name Perch will not accept is its own outcome, not a general failure: {error}"
        );
    }

    assert!(registry_of(&host).aliases.is_empty());
}

#[test]
fn a_target_resolves_as_an_alias_then_an_account_then_a_group() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "spare");
    let registry = registry_of(&host);

    assert_eq!(
        target::resolve(&registry, "overflow").unwrap(),
        Target::Alias {
            name: "overflow".into(),
            email: SECOND_EMAIL.into(),
        }
    );
    assert_eq!(
        target::resolve(&registry, SECOND_EMAIL).unwrap(),
        Target::Account {
            email: SECOND_EMAIL.into(),
        }
    );
    assert_eq!(
        target::resolve(&registry, "spare").unwrap(),
        Target::Group {
            name: "spare".into(),
        }
    );
}

#[test]
fn a_command_acting_on_a_target_says_which_kind_matched() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");

    let (_, by_alias) = move_to_group(&host, "overflow", "work");
    let (_, by_email) = move_to_group(&host, SECOND_EMAIL, "work");

    assert!(
        by_alias.contains("Alias") && by_alias.contains(SECOND_EMAIL),
        "a Target that matched an Alias should say so, and say what it reached:\n{by_alias}"
    );
    assert!(
        by_email.contains("Account"),
        "and a Target that matched an Account should say that instead:\n{by_email}"
    );
}

#[test]
fn a_target_perch_cannot_place_is_refused_with_the_names_it_nearly_matched() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");

    let (result, _) = move_to_group(&host, "overflw", "work");

    let error = result.expect_err("nothing is called `overflw`");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    let message = error.to_string();
    assert!(
        message.contains("overflw") && message.contains("overflow"),
        "what was typed and what it nearly matched should both be said:\n{message}"
    );
}

#[test]
fn a_target_that_names_nothing_at_all_is_still_refused_helpfully() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");

    let (result, _) = move_to_group(&host, "zzzzzz", "work");

    let error = result.expect_err("nothing is called `zzzzzz`");
    assert_eq!(error.exit_code(), EXIT_NOT_FOUND);
    assert!(
        error.to_string().contains("perch group list"),
        "with no near miss to offer, say where the answer is:\n{error}"
    );
}

#[test]
fn a_group_named_where_one_account_is_meant_is_refused_as_a_group() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");

    let (result, _) = set_alias(&host, "spare", "work");

    let error = result.expect_err("an Alias names one Account, and `work` is a Group");
    assert_eq!(error.exit_code(), EXIT_INVALID);
    assert!(
        error.to_string().contains("Group"),
        "the kind that did match should be said, so the user can see why it was refused:\n{error}"
    );
}

#[test]
fn an_account_is_named_by_its_alias_wherever_perch_talks_about_it() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");
    move_to_group(&host, "overflow", "work").0.unwrap();

    let (_, printed) = run_group(&host, GroupCommand::List);

    assert!(
        printed.contains("overflow"),
        "the name the user gave it is how they think of it:\n{printed}"
    );
}

#[test]
fn naming_an_account_makes_no_network_call() {
    let host = machine_with_two_accounts();

    set_alias(&host, "work", SECOND_EMAIL).0.unwrap();
    unset_alias(&host, "work").0.unwrap();

    assert!(host.http_calls().is_empty());
}

#[test]
fn a_name_that_differs_only_in_case_is_the_same_name() {
    let host = machine_with_a_named_second_account();

    let (as_a_group, _) = run_group(
        &host,
        GroupCommand::Add {
            name: "Overflow".into(),
        },
    );

    let error = as_a_group.expect_err("`Overflow` is `overflow` to anyone typing it");
    assert_eq!(error.exit_code(), EXIT_CONFLICT);
    assert!(
        error.to_string().contains("overflow"),
        "the name already held should be shown as it is held:\n{error}"
    );

    declare_group(&host, "spare");
    let (as_an_alias, _) = set_alias(&host, "SPARE", SECOND_EMAIL);

    assert_eq!(
        as_an_alias
            .expect_err("and the same in the other direction")
            .exit_code(),
        EXIT_CONFLICT
    );
}

#[test]
fn an_alias_command_on_a_registry_from_before_aliases_finds_nothing() {
    // Holding the Account it names: a registry from before Aliases still had
    // the Account it was active on, and one that named an Account it did not
    // hold is a state `validate` refuses rather than a machine anybody had.
    let host = logged_in_machine().with_file(
        REGISTRY_PATH,
        r#"{"version":2,"active":{"settled":"someone@example.com"},"accounts":[{"identity":{"email":"someone@example.com"}}]}"#,
    );

    let (result, _) = run_alias(
        &host,
        AliasCommand::Unset {
            target: "work".into(),
        },
    );

    assert_eq!(
        result.expect_err("there are no Aliases at all").exit_code(),
        EXIT_NOT_FOUND
    );
}

/// The registry refuses an Alias or a Group that differs from a held name only
/// in case, precisely so that nobody has to remember how they capitalised one
/// months ago. Resolving a Target has to honour the same rule, or `perch group
/// add Work` produces a Group that `perch group remove work` will drop and
/// `perch switch work` says does not exist.
#[test]
fn a_name_is_reached_however_it_is_capitalised() {
    let host = machine_with_two_accounts();
    declare_group(&host, "Work");
    set_alias(&host, "Overflow", SECOND_EMAIL).0.expect("named");
    let registry = registry_of(&host);

    assert_eq!(
        target::resolve(&registry, "work").expect("the Group is reached"),
        Target::Group {
            name: "Work".to_string()
        },
        "and it comes back spelled the way it was declared"
    );
    assert_eq!(
        target::resolve(&registry, "OVERFLOW").expect("the Alias is reached"),
        Target::Alias {
            name: "Overflow".to_string(),
            email: SECOND_EMAIL.to_string(),
        }
    );
    assert_eq!(
        target::resolve(&registry, &EMAIL.to_uppercase()).expect("the Account is reached"),
        Target::Account {
            email: EMAIL.to_string()
        },
        "an address is one address however it is typed, which is the rule \
         `already_landed` has always compared by"
    );

    // And over the whole of Unicode, not the ASCII half. `registry::slug`
    // lowercases everything, so `CAFÉ@` and `café@` already share one Profile
    // and `perch add` already refuses the second as a collision — resolving in
    // ASCII made this the one place that disagreed, and the refusal said Perch
    // holds nothing by that name about an Account it holds and will not let you
    // have two of.
    let mut registry = registry;
    let accented = "café@example.com";
    let mut account = registry.account(EMAIL).expect("the Account").clone();
    account.identity.email = accented.to_string();
    registry.upsert(account);

    assert_eq!(
        target::resolve(&registry, "CAFÉ@EXAMPLE.COM").expect("the Account is reached"),
        Target::Account {
            email: accented.to_string()
        },
        "the rule that two names differing only in case are one name is not an \
         ASCII rule anywhere else it is asked"
    );
}

/// The same rule reaching the command that acts on it, rather than only the
/// resolution underneath.
#[test]
fn a_switch_accepts_the_spelling_every_other_command_accepts() {
    let host = machine_with_two_accounts();

    let (result, printed) = run_switch(&host, &SECOND_EMAIL.to_uppercase());

    result.expect("that is the Account, typed loudly");
    assert_eq!(registry_of(&host).active().whose(), Some(SECOND_EMAIL));
    assert!(printed.contains(SECOND_EMAIL), "{printed}");
}

/// `validate_name` accepts the whole of Unicode, so the rule that two names
/// differing only in case are one name has to hold over the whole of Unicode
/// too. Comparing only ASCII would make `work` and `Work` one Group while
/// making `café` and `CAFÉ` two — the exact ambiguity the rule exists to
/// prevent, kept for the users whose language has accents in it.
#[test]
fn a_name_is_one_name_however_it_is_capitalised_in_any_language() {
    let host = machine_with_two_accounts();
    declare_group(&host, "café");

    let (refused, _) = run_group(
        &host,
        GroupCommand::Add {
            name: "CAFÉ".to_string(),
        },
    );
    assert_eq!(
        refused
            .expect_err("that is the Group that is already declared")
            .exit_code(),
        EXIT_CONFLICT
    );

    let registry = registry_of(&host);
    assert_eq!(registry.declared_group("CAFÉ"), Some("café"));
    assert_eq!(
        target::resolve(&registry, "Café").expect("the Group is reached"),
        Target::Group {
            name: "café".to_string()
        }
    );
}

/// The whole of what makes the flipped argument order a superset rather than a
/// swap (ADR 0054): `--unset` takes a Target, so the person who knows only the
/// email address can free a name they would otherwise have had to know first.
#[test]
fn unsetting_by_email_frees_the_alias_the_account_answers_to() {
    let host = machine_with_a_named_second_account();

    let (result, printed) = unset_alias(&host, SECOND_EMAIL);

    result.expect("an email address is a Target like any other");
    assert!(
        printed.contains("overflow"),
        "the name that was freed is said, since the user never typed it:\n{printed}"
    );
    assert_eq!(registry_of(&host).alias_of(SECOND_EMAIL), None);
}

/// Both arms take a Target, so both get the Group refusal the shared
/// resolution path gives — the same answer `perch alias <group> <name>` has
/// always had, where freeing a name used to be told there was no Alias by that
/// name. An Alias is one Account's, and saying which kind did match is what
/// tells somebody they named the wrong thing rather than misspelled it.
#[test]
fn unsetting_a_group_is_refused_as_a_group_the_way_naming_one_is() {
    let host = machine_with_a_named_second_account();
    declare_group(&host, "work");

    let (result, _) = unset_alias(&host, "work");

    let refusal = result.expect_err("an Alias names one Account, and `work` is a Group");
    assert_eq!(refusal.exit_code(), EXIT_INVALID);
    assert!(
        refusal.to_string().contains("Group"),
        "the kind that did match should be said: {refusal}"
    );
    assert_eq!(
        registry_of(&host).alias_of(SECOND_EMAIL),
        Some("overflow"),
        "and nothing was freed"
    );
}

/// The one refusal the shared resolution path does not already cover. The
/// Target is perfectly good and the Account is held — there is simply no name
/// to give up, which is a different thing from a typo and reads as one.
#[test]
fn unsetting_an_account_that_answers_to_no_alias_is_refused() {
    let host = machine_with_a_named_second_account();

    let (result, _) = unset_alias(&host, EMAIL);

    let refusal = result.expect_err("this Account was never named");
    assert_eq!(refusal.exit_code(), EXIT_NOT_FOUND);
    let said = refusal.to_string();
    assert!(
        said.contains(EMAIL) && said.contains("no Alias"),
        "it says which Account has nothing to free: {said}"
    );
    assert_eq!(
        registry_of(&host).alias_of(SECOND_EMAIL),
        Some("overflow"),
        "and the Account that does answer to one still does"
    );
}

/// Aliases and Group names share one namespace and a name reaches one Account.
/// Handing a name that already names somebody else to a second Account would
/// leave `perch switch <name>` meaning whichever the registry happened to
/// resolve first — so it is refused, and the refusal names the command that
/// frees the name rather than leaving somebody to guess at it.
///
/// The sentence it prints survived the argument flip untouched: the held name
/// now sits where the Target goes rather than where the name did, and reaches
/// the same Account either way, because a Target resolves an Alias first.
#[test]
fn a_name_that_already_reaches_another_account_is_refused_and_says_how_to_free_it() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the name is free");

    let (result, _) = set_alias(&host, "overflow", EMAIL);

    let refusal = result.expect_err("`overflow` already reaches somebody");
    assert_eq!(refusal.exit_code(), EXIT_CONFLICT);
    let said = refusal.to_string();
    assert!(
        said.contains(&format!("`overflow` already names {SECOND_EMAIL}")),
        "{said}"
    );
    assert!(
        said.contains("perch alias overflow --unset"),
        "it names the command that frees it: {said}"
    );
    assert_eq!(
        registry_of(&host).alias_of(SECOND_EMAIL),
        Some("overflow"),
        "and the Account that held it still does"
    );
    assert_eq!(registry_of(&host).alias_of(EMAIL), None);
}

/// The same name given back to the Account that already answers to it is not a
/// collision with itself, so it is not refused — and renaming an Account to a
/// free name gives up the one it held, because an Account answers to one Alias
/// at a time.
#[test]
fn renaming_an_account_gives_up_the_name_it_answered_to_before() {
    let host = machine_with_two_accounts();
    set_alias(&host, "overflow", SECOND_EMAIL)
        .0
        .expect("the name is free");

    let (result, printed) = set_alias(&host, "spare", SECOND_EMAIL);

    result.expect("a free name is a rename, not a collision");
    assert!(
        printed.contains("`overflow` no longer does"),
        "it says which name was given up: {printed}"
    );
    assert_eq!(registry_of(&host).alias_of(SECOND_EMAIL), Some("spare"));
    assert_eq!(
        registry_of(&host).declared_alias("overflow"),
        None,
        "and the old name is free to use again"
    );
}
