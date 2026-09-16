//! Behavior: organizing the Holdings one question at a time.
//!
//! Every step is a command that exists on its own, so what these tests hold is
//! the Wizard's own promises: Enter keeps what is there, a step that wrote says
//! the command it stood for, end of input keeps every step already answered,
//! and a machine with nobody at the terminal is refused with the steps listed.

mod common;

use common::*;
use perch::error::EXIT_INVALID;
use perch::host::prelude::*;
use perch::host::{Execution, FakeHost, Platform};

/// Where a `systemd --user` unit goes on the fixture's machine.
const UNIT: &str = "/Users/someone/.config/systemd/user/perch-watch.service";

fn run_wizard(host: &FakeHost) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::wizard::run(host, written)
    })
}

/// Enter at every question, more times than there are questions: running out
/// of answers is end of input, and a run that keeps every default has to reach
/// the end rather than be stopped short of it.
fn enter_throughout() -> Vec<&'static str> {
    vec![""; 24]
}

/// The lines a step prints when it wrote something, and nothing else.
fn typed_forms(printed: &str) -> Vec<&str> {
    printed
        .lines()
        .filter(|line| line.starts_with("  perch "))
        .collect()
}

/// Two Accounts, one of them in a Group and the other in none, so both kinds
/// of Scope are asked about.
fn one_grouped_one_not() -> FakeHost {
    let host = machine_with_two_accounts();
    a_group_of(&host, "work", &[EMAIL]);
    host
}

#[test]
fn a_run_that_keeps_every_default_changes_nothing() {
    let host = one_grouped_one_not().with_answers(&enter_throughout());
    let before = host.file(REGISTRY_PATH).unwrap();

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        host.file(REGISTRY_PATH).unwrap(),
        before,
        "Enter keeps what is there, at every step"
    );
    assert!(
        typed_forms(&printed).is_empty(),
        "a kept default prints no command:\n{printed}"
    );
    assert!(
        !host.path_exists(std::path::Path::new(UNIT)),
        "and nothing was installed:\n{printed}"
    );
    assert!(
        !printed.contains("stops here"),
        "the Wizard reached its end rather than running out of answers:\n{printed}"
    );
}

#[test]
fn a_new_group_name_declares_the_group_and_moves_the_account_into_it() {
    let mut answers = enter_throughout();
    // The first question after the Listing is whether to add; the second is
    // the first Account's Group.
    answers[1] = "work";
    let host = machine_with_two_accounts().with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    let registry = registry_of(&host);
    assert_eq!(
        registry
            .account(&fixture_key(&host, EMAIL))
            .unwrap()
            .group
            .as_deref(),
        Some("work"),
        "{printed}"
    );
    assert!(registry.group("work").is_some(), "declared on the way");
    assert_eq!(
        typed_forms(&printed),
        vec![
            "  perch group add work",
            &format!("  perch group move {EMAIL} work")
        ],
        "{printed}"
    );
}

#[test]
fn a_refused_group_name_is_asked_again_rather_than_ending_the_wizard() {
    let mut answers = enter_throughout();
    answers[1] = "no spaces";
    answers[2] = "work";
    let host = machine_with_two_accounts().with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(
        registry_of(&host)
            .account(&fixture_key(&host, EMAIL))
            .unwrap()
            .group
            .as_deref(),
        Some("work"),
        "{printed}"
    );
}

#[test]
fn answering_a_setting_writes_it_through_config_set_and_says_so() {
    let mut answers = enter_throughout();
    // After "add?" and two Groups come the Ungrouped Scope's four Settings,
    // then Group `work`'s three, the last of which is `watcher-may-act`.
    answers[3 + 4 + 2] = "true";
    let host = one_grouped_one_not().with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        registry_of(&host)
            .settings(&perch::config::Scope::Group("work".to_string()))
            .watcher_may_act,
        "{printed}"
    );
    assert_eq!(
        typed_forms(&printed),
        vec!["  perch config set work watcher-may-act true"],
        "{printed}"
    );
}

#[test]
fn the_may_act_question_says_the_watcher_only_observes_until_it_is_true() {
    let host = one_grouped_one_not().with_answers(&enter_throughout());

    let (_, printed) = run_wizard(&host);

    assert!(
        printed.contains(
            "The Watcher only observes within Group `work` until `watcher-may-act` is true. \
             Most people turn it on."
        ),
        "{printed}"
    );
}

#[test]
fn before_offering_the_service_every_scope_that_cannot_act_is_named_with_its_grant() {
    let host = one_grouped_one_not().with_answers(&enter_throughout());

    let (_, printed) = run_wizard(&host);

    assert!(
        printed.contains(
            "Group `work` does not let the Watcher act: `perch config set work \
             watcher-may-act true` first."
        ),
        "{printed}"
    );
    assert!(
        printed.contains(
            "The Ungrouped Scope does not let the Watcher act: `perch config set \
             ungrouped interchangeable true` and `perch config set ungrouped \
             watcher-may-act true` first."
        ),
        "{printed}"
    );
}

#[test]
fn saying_yes_to_the_service_installs_it_and_says_the_command() {
    let worked = || Execution {
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    // One question to add, two Groups, the Ungrouped Scope's four Settings,
    // then the Service.
    let host = machine_with_two_accounts()
        .with_platform(Platform::Other)
        .with_exec("systemctl", &["--user", "daemon-reload"], worked())
        .with_exec(
            "systemctl",
            &["--user", "enable", "--now", "perch-watch.service"],
            worked(),
        )
        .with_exec(
            "systemctl",
            &["--user", "restart", "perch-watch.service"],
            worked(),
        )
        .with_answers(&["", "", "", "", "", "", "", "y"]);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(host.path_exists(std::path::Path::new(UNIT)), "{printed}");
    assert_eq!(
        typed_forms(&printed),
        vec!["  perch watcher install"],
        "{printed}"
    );
}

#[test]
fn a_service_already_installed_is_shown_and_not_offered_again() {
    let host = one_grouped_one_not()
        .with_platform(Platform::Other)
        .with_file(UNIT, "")
        .with_answers(&enter_throughout());

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("The Watcher runs as a Service."),
        "{printed}"
    );
    assert!(!printed.contains("Install the Watcher"), "{printed}");
}

#[test]
fn end_of_input_keeps_every_step_already_answered() {
    // Yes to adding, no Group for the Account it added, and then nobody.
    let host = logged_in_machine()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
        .with_answers(&["y", "none"]);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        registry_of(&host)
            .account(&fixture_key(&host, SECOND_EMAIL))
            .is_some(),
        "the Account added in step two is kept:\n{printed}"
    );
    assert_eq!(typed_forms(&printed), vec!["  perch add"], "{printed}");
    assert!(printed.contains("stops here"), "{printed}");
}

#[test]
fn a_login_walked_away_from_is_said_and_the_question_is_asked_again() {
    let host = logged_in_machine()
        .with_login(abandoned_login())
        .with_answers(&["y", "n", "", "", "", "", "", ""]);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(registry_of(&host).accounts.len(), 1, "{printed}");
    assert_eq!(
        printed.matches("Add an Account?").count(),
        2,
        "asked once more after the login failed:\n{printed}"
    );
}

#[test]
fn nobody_at_the_terminal_is_refused_with_the_steps_listed() {
    let host = one_grouped_one_not().without_terminal();
    let before = host.file(REGISTRY_PATH).unwrap();

    let (result, _) = run_wizard(&host);

    let refused = result.unwrap_err();
    assert_eq!(refused.exit_code(), EXIT_INVALID);
    let said = refused.to_string();
    for (step, command) in [
        "perch list",
        "perch add",
        "perch group move <target> <group>",
        "perch config set <scope> <key> <value>",
        "perch watcher install",
    ]
    .iter()
    .enumerate()
    {
        let at = said
            .find(command)
            .unwrap_or_else(|| panic!("step {} `{command}` is missing: {said}", step + 1));
        assert!(
            said[..at].matches("perch ").count() == step,
            "in step order: {said}"
        );
    }
    assert_eq!(host.file(REGISTRY_PATH).unwrap(), before);
}

/// A Codex login for the fixtures below; synthetic claims, no usable token.
fn codex_credential(workspace: &str, email: &str) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_user_id": "user-one",
                "chatgpt_account_id": workspace,
                "chatgpt_plan_type": "plus"
            }
        })
        .to_string(),
    );
    serde_json::json!({
        "auth_mode": "chatgpt",
        "tokens": {"id_token": format!("fake.{payload}.fake"), "account_id": workspace}
    })
    .to_string()
}

/// Both CLIs on the machine, and every login from here on a Codex one.
fn with_codex_installed(host: FakeHost, workspace: &str, email: &str) -> FakeHost {
    let document = codex_credential(workspace, email);
    host.with_file("/usr/bin/codex", "")
        .with_login(move |host, at| {
            host.set_file(at.join("auth.json"), &document);
            0
        })
}

/// Two Claude Accounts, one in `work`, and a Codex Account beside it in `work`.
fn one_group_holding_both_providers() -> FakeHost {
    let host = with_codex_installed(one_grouped_one_not(), "workspace-1", "person@example.com");
    perch::commands::add::run(
        &host,
        perch::commands::add::AddArgs {
            provider: perch::commands::selection::Selection {
                codex: true,
                ..Default::default()
            },
            alias: Some("personal".into()),
            group: Some("work".into()),
            ..Default::default()
        },
        &mut Vec::new(),
    )
    .expect("the Codex Account is added into `work`");
    host
}

#[test]
fn with_both_clis_installed_the_add_step_asks_which_provider_and_types_the_flag() {
    let mut answers = enter_throughout();
    // Yes to adding, Codex, Enter for the new Account's Group, then no more.
    answers[0] = "y";
    answers[1] = "codex";
    let host = with_codex_installed(
        machine_with_two_accounts(),
        "workspace-1",
        "person@example.com",
    )
    .with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed
            .contains("Provider for the new Account [claude] (Enter keeps; `claude` or `codex`): "),
        "{printed}"
    );
    assert!(
        typed_forms(&printed).contains(&"  perch add --codex"),
        "{printed}"
    );
    assert!(
        registry_of(&host)
            .accounts
            .iter()
            .any(|account| account.provider() == perch::providers::provider::Id::Codex),
        "{printed}"
    );
}

#[test]
fn with_one_cli_installed_no_provider_is_asked() {
    let host = logged_in_machine()
        .with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE))
        .with_answers(&["y", "none"]);

    let (_, printed) = run_wizard(&host);

    assert!(
        !printed.contains("Provider for the new Account"),
        "{printed}"
    );
}

#[test]
fn a_scope_holding_both_providers_asks_the_grant_per_provider() {
    let mut answers = enter_throughout();
    // Add, three Groups, `run-provider`, the Ungrouped Scope's four Settings,
    // then `work`'s Strategy and workload; Claude Code's grant is the twelfth
    // question and Codex's the thirteenth.
    answers[11] = "true";
    let host = one_group_holding_both_providers().with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains("`watcher-may-act` for Claude Code within Group `work` [false]"),
        "{printed}"
    );
    assert!(
        printed.contains("`watcher-may-act` for Codex within Group `work` [false]"),
        "{printed}"
    );
    assert_eq!(
        typed_forms(&printed),
        vec!["  perch config set work --provider claude watcher-may-act true"],
        "{printed}"
    );
}

#[test]
fn a_mixed_scope_names_its_grant_with_the_provider_before_the_service_is_offered() {
    let host = one_group_holding_both_providers().with_answers(&enter_throughout());

    let (_, printed) = run_wizard(&host);

    assert!(
        printed.contains(
            "Group `work` does not let the Watcher act: `perch config set work \
             --provider claude watcher-may-act true` and `perch config set work \
             --provider codex watcher-may-act true` first."
        ),
        "{printed}"
    );
}

#[test]
fn run_provider_is_asked_once_both_providers_hold_accounts_and_written_globally() {
    let mut answers = enter_throughout();
    // Add and three Groups come before it.
    answers[4] = "codex";
    let host = one_group_holding_both_providers().with_answers(&answers);

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(
        printed.contains(
            "`run-provider` for a bare `perch run` [claude] (Enter keeps; `claude` or `codex`): "
        ),
        "{printed}"
    );
    assert!(
        typed_forms(&printed).contains(&"  perch config set --global run-provider codex"),
        "{printed}"
    );
    assert_eq!(
        registry_of(&host).run_provider,
        perch::providers::provider::Id::Codex
    );
}

#[test]
fn run_provider_is_not_asked_while_only_one_provider_holds_accounts() {
    let host = one_grouped_one_not().with_answers(&enter_throughout());

    let (_, printed) = run_wizard(&host);

    assert!(!printed.contains("`run-provider`"), "{printed}");
}

#[test]
fn a_scope_of_codex_accounts_alone_is_asked_its_settings() {
    let host = with_codex_installed(one_grouped_one_not(), "workspace-1", "person@example.com");
    perch::commands::add::run(
        &host,
        perch::commands::add::AddArgs {
            provider: perch::commands::selection::Selection {
                codex: true,
                ..Default::default()
            },
            alias: Some("personal".into()),
            group: Some("codex".into()),
            ..Default::default()
        },
        &mut Vec::new(),
    )
    .expect("the Codex Account is added into `codex`");
    let host = host.with_answers(&enter_throughout());

    let (result, printed) = run_wizard(&host);

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(printed.contains("within Group `work`"), "{printed}");
    assert!(
        printed.contains("`strategy` within Group `codex`"),
        "Codex Accounts Cycle, so their Scope is asked:\n{printed}"
    );
}

#[test]
fn an_account_sharing_its_email_across_providers_is_asked_for_by_alias_or_key() {
    // The Codex login carries the first Claude Account's email.
    let host = with_codex_installed(one_grouped_one_not(), "workspace-1", EMAIL);
    perch::commands::add::run(
        &host,
        perch::commands::add::AddArgs {
            provider: perch::commands::selection::Selection {
                codex: true,
                ..Default::default()
            },
            alias: Some("personal".into()),
            no_group: true,
            ..Default::default()
        },
        &mut Vec::new(),
    )
    .expect("the Codex Account is added");
    let host = host.with_answers(&enter_throughout());

    let (_, printed) = run_wizard(&host);

    assert!(printed.contains("Group for personal ["), "{printed}");
    assert!(
        printed.contains(&format!("Group for {} [", fixture_key(&host, EMAIL))),
        "the unaliased sharer is asked for by key:\n{printed}"
    );
    assert!(
        !printed.contains(&format!("Group for {EMAIL} [")),
        "a shared email names no Account:\n{printed}"
    );
}
