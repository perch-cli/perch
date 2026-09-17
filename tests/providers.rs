//! Provider boundaries and Workspace identity (ADR an-account-has-a-workspace).

mod common;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use perch::commands::selection::Selection;
use perch::commands::{add, config, relogin, remove, run};
use perch::host::FakeHost;
use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::providers::provider::Id;
use perch::{registry, target};
use serde_json::json;

const CODEX: &str = "/usr/bin/codex";
const EMAIL: &str = "person@example.com";

// Synthetic claims exercise identity attribution; no fixture is a usable token.
fn credential(workspace: &str, email: &str) -> String {
    let payload = URL_SAFE_NO_PAD.encode(
        json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_user_id": "user-one",
                "chatgpt_account_id": workspace,
                "chatgpt_plan_type": "plus"
            }
        })
        .to_string(),
    );
    json!({"auth_mode":"chatgpt","tokens":{"id_token":format!("fake.{payload}.fake"),"account_id":workspace}}).to_string()
}

fn machine(workspace: &str) -> FakeHost {
    let document = credential(workspace, EMAIL);
    FakeHost::new()
        .with_env("PATH", "/usr/bin")
        .with_file(CODEX, "")
        .with_login(move |host, at| {
            host.set_file(at.join("auth.json"), &document);
            0
        })
}

fn add_account(host: &FakeHost, alias: &str) {
    add::run(
        host,
        add::AddArgs {
            provider: Selection {
                provider: None,
                codex: true,
                claude: false,
            },
            alias: Some(alias.into()),
            no_group: true,
            group: None,
        },
        &mut Vec::new(),
    )
    .expect("Codex Account added");
}

fn launch(host: &FakeHost, provider: Selection, command: &[&str]) -> perch::Result<i32> {
    run::run(
        host,
        run::RunArgs {
            provider,
            target: "personal".into(),
            command: command.iter().map(|word| (*word).into()).collect(),
        },
        &mut Vec::new(),
    )
}

#[test]
fn the_same_email_in_two_workspaces_is_two_accounts_with_distinct_profiles() {
    let host = machine("personal");
    add_account(&host, "personal");
    let document = credential("company", EMAIL);
    let host = host.with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add_account(&host, "company");
    let registry = registry::load(&host).unwrap().unwrap();
    assert_eq!(registry.accounts.len(), 2);
    assert_ne!(
        registry.accounts[0].profile_dir(&host).unwrap(),
        registry.accounts[1].profile_dir(&host).unwrap()
    );
    assert!(
        target::resolve_account(&registry, EMAIL)
            .unwrap_err()
            .to_string()
            .contains("names more than one Account")
    );
    assert!(target::resolve_for(&registry, EMAIL, Some(Id::Codex)).is_err());
    assert_ne!(
        target::resolve_account(&registry, "personal")
            .unwrap()
            .email,
        target::resolve_account(&registry, "company").unwrap().email
    );
}

#[test]
fn adding_the_same_workspace_twice_refuses_without_replacing_the_held_credential() {
    let host = machine("personal");
    add_account(&host, "personal");
    let before = registry::load(&host).unwrap().unwrap();
    let refused = add::run(
        &host,
        add::AddArgs {
            provider: Selection {
                provider: None,
                codex: true,
                claude: false,
            },
            no_group: true,
            ..Default::default()
        },
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(refused.to_string().contains("already holds"));
    assert_eq!(registry::load(&host).unwrap().unwrap(), before);
    assert!(
        exported_credential(&host, &before, &before.accounts[0])
            .unwrap()
            .is_some()
    );
}

#[test]
fn run_falls_back_only_when_the_preferred_cli_is_absent() {
    let host = machine("personal");
    add_account(&host, "personal");
    assert_eq!(launch(&host, Selection::default(), &[]).unwrap(), 0);
    let host = host.with_file("/usr/bin/claude", "");
    let error = launch(&host, Selection::default(), &[]).unwrap_err();
    assert!(error.to_string().contains("`--codex` selects it"));
    assert!(
        launch(
            &host,
            Selection {
                provider: None,
                codex: true,
                claude: false
            },
            &[]
        )
        .is_ok()
    );
}

#[test]
fn the_global_preference_can_be_set_before_any_cli_or_account_exists() {
    let host = FakeHost::new();
    config::run(
        &host,
        config::ConfigCommand::Set {
            words: ["--global", "run-provider", "codex"]
                .map(String::from)
                .into(),
        },
        &mut Vec::new(),
    )
    .unwrap();
    let mut output = Vec::new();
    config::run(
        &host,
        config::ConfigCommand::Get {
            words: ["--global", "run-provider"].map(String::from).into(),
        },
        &mut output,
    )
    .unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "codex\n");
    assert_eq!(
        registry::load(&host).unwrap().unwrap().run_provider,
        Id::Codex
    );
}

#[test]
fn codex_receives_forwarded_arguments_and_no_inherited_api_key() {
    let host = machine("personal")
        .with_env("OPENAI_API_KEY", "a-shell-key")
        .with_env("PROJECT_TOOL_SETTING", "project-value")
        .with_env("CODEX_HOME", "/another/account");
    add_account(&host, "personal");
    host.forget_effects();
    launch(&host, Selection::default(), &["--resume", "--json"]).unwrap();
    let effects = host.effects();
    let (args, env) = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ExecInteractive {
                program, args, env, ..
            } if program == CODEX => Some((args, env)),
            _ => None,
        })
        .unwrap();
    assert!(args.ends_with(&["--resume".into(), "--json".into()]));
    assert!(!env.iter().any(|(key, _)| key == "OPENAI_API_KEY"));
    assert!(
        env.iter()
            .any(|(key, value)| key == "PROJECT_TOOL_SETTING" && value == "project-value")
    );
    let registry = registry::load(&host).unwrap().unwrap();
    let home = registry.accounts[0].profile_dir(&host).unwrap();
    assert!(
        env.iter()
            .any(|(key, value)| key == "CODEX_HOME" && value == &home.to_string_lossy())
    );
}

#[test]
fn a_custom_command_uses_the_account_without_requiring_a_coding_cli() {
    let host = machine("personal");
    add_account(&host, "personal");
    host.remove_file(std::path::Path::new(CODEX)).unwrap();
    host.forget_effects();
    assert_eq!(
        launch(&host, Selection::default(), &["npm", "test"]).unwrap(),
        0
    );
    assert!(host.effects().iter().any(|effect| matches!(effect, Effect::ExecInteractive {program, args, env, ..} if program == "npm" && args == &["test".to_string()] && env.len() == 1 && env[0].0 == "CODEX_HOME")));
}

#[test]
fn a_child_failure_never_launches_the_other_provider() {
    let host = machine("personal");
    add_account(&host, "personal");
    let host = host.with_file("/usr/bin/claude", "").with_login(|_, _| 37);
    host.forget_effects();
    assert_eq!(
        launch(
            &host,
            Selection {
                provider: None,
                codex: true,
                claude: false
            },
            &[]
        )
        .unwrap(),
        37
    );
    assert_eq!(
        host.effects()
            .iter()
            .filter(|effect| matches!(effect, Effect::ExecInteractive { .. }))
            .count(),
        1
    );
}

#[test]
fn a_relogin_into_another_workspace_preserves_the_original_account() {
    let host = machine("personal");
    add_account(&host, "personal");
    let before = registry::load(&host).unwrap().unwrap();
    let document = credential("company", EMAIL);
    let host = host.with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    assert!(
        relogin::run(
            &host,
            relogin::ReloginArgs {
                target: "personal".into()
            },
            &mut Vec::new()
        )
        .is_err()
    );
    assert_eq!(registry::load(&host).unwrap().unwrap(), before);
    assert_eq!(
        exported_credential(&host, &before, &before.accounts[0])
            .unwrap()
            .unwrap()
            .as_str(),
        credential("personal", EMAIL)
    );
}

#[test]
fn removing_a_codex_account_deletes_its_profile_and_alias() {
    let host = machine("personal");
    add_account(&host, "personal");
    let before = registry::load(&host).unwrap().unwrap();
    let home = before.accounts[0].profile_dir(&host).unwrap();
    remove::run(
        &host,
        remove::RemoveArgs {
            target: "personal".into(),
            yes: true,
        },
        &mut Vec::new(),
    )
    .unwrap();
    let after = registry::load(&host).unwrap().unwrap();
    assert!(after.accounts.is_empty());
    assert!(after.aliases.is_empty());
    assert!(!host.path_exists(&home));
}

#[test]
fn a_claude_cycle_never_selects_a_codex_account_in_the_same_group() {
    let host = common::three_accounts_in_one_group();
    let document = credential("personal", EMAIL);
    let host = host.with_file(CODEX, "").with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add_account(&host, "personal");
    perch::commands::group::run(
        &host,
        perch::commands::group::GroupCommand::Move {
            target: "personal".into(),
            group: "work".into(),
        },
        &mut Vec::new(),
    )
    .unwrap();
    let mut registry = registry::load(&host).unwrap().unwrap();
    registry.select_provider(Id::Claude);
    let ranked = perch::cycle::ranked(
        &registry,
        &perch::config::Scope::Group("work".into()),
        host.now(),
    );
    assert_eq!(ranked.len(), 3, "the three Claude Accounts");
    assert!(
        ranked
            .iter()
            .all(|account| account.provider() == Id::Claude)
    );
}

#[test]
fn an_export_carries_the_codex_credential_and_config() {
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let export = perch::export::gather(&host, &registry).unwrap();
    assert_eq!(export.version, perch::export::CURRENT_VERSION);
    assert_eq!(
        common::exported_artifact(&export, account.key(), "auth.json").unwrap(),
        credential("personal", EMAIL)
    );
    assert!(
        common::exported_artifact(&export, account.key(), "config.toml")
            .unwrap()
            .contains("forced_login_method")
    );
}

#[test]
fn a_running_codex_profile_is_not_opened_for_refresh_or_relogin() {
    let host = machine("personal");
    add_account(&host, "personal");
    let host = host.with_login(|host, _| {
        let registry = registry::load(host).unwrap().unwrap();
        let account = &registry.accounts[0];
        let observed = account
            .provider()
            .adapter()
            .configured(host)
            .unwrap()
            .observe(
                host,
                &mut perch::holdings::lock(host).unwrap(),
                &registry.profile_context(host, account).unwrap(),
                &account.profile(host).unwrap(),
                &mut || Ok(()),
            );
        assert!(matches!(
            observed,
            Err(perch::observe::Outcome::Failed { spent: false, .. })
        ));
        assert!(matches!(
            relogin::run(
                host,
                relogin::ReloginArgs {
                    target: "personal".into()
                },
                &mut Vec::new()
            ),
            Err(perch::PerchError::ProfileLive(_))
        ));
        0
    });
    launch(&host, Selection::default(), &[]).unwrap();
}

#[cfg(unix)]
#[test]
fn the_real_rpc_transport_matches_ids_and_skips_notifications() {
    use perch::host::Processes;
    let script = r#"
printf '%s\n' '{"method":"notice"}'
read -r request
printf '%s\n' '{"id":1,"result":{}}'
read -r notification
read -r request
printf '%s\n' '{"id":77,"result":{}}' '{"id":2,"result":{"account":{"type":"chatgpt"}}}'
read -r request
printf '%s\n' '{"id":3,"result":{"rateLimits":{}}}'
"#;
    let replies = perch::host::RealHost::new()
        .rpc(
            "/bin/sh",
            &["-c", script],
            &[],
            &[
                json!({"id":1,"method":"initialize"}).to_string(),
                json!({"method":"initialized"}).to_string(),
                json!({"id":2,"method":"account/read"}).to_string(),
                json!({"id":3,"method":"account/rateLimits/read"}).to_string(),
            ],
            perch::host::RpcControl {
                timeout: std::time::Duration::from_secs(5),
                checkpoint: &mut || Ok(()),
            },
        )
        .unwrap();
    let ids: Vec<_> = replies
        .iter()
        .map(|reply| {
            serde_json::from_str::<serde_json::Value>(reply).unwrap()["id"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn a_codex_export_restores_its_workspace_alias_and_preference_on_another_machine() {
    let host = machine("personal");
    add_account(&host, "personal");
    config::run(
        &host,
        config::ConfigCommand::Set {
            words: ["--global", "run-provider", "codex"]
                .map(String::from)
                .into(),
        },
        &mut Vec::new(),
    )
    .unwrap();
    let registry = registry::load(&host).unwrap().unwrap();
    let export = perch::export::gather(&host, &registry).unwrap();
    let sealed = perch::export::seal(&export, "offline fixture passphrase").unwrap();
    let destination = FakeHost::new()
        .with_file("/backup.age", &sealed)
        .with_secrets(&["offline fixture passphrase"]);
    perch::commands::import::run(
        &destination,
        std::path::Path::new("/backup.age"),
        &mut Vec::new(),
    )
    .unwrap();
    let restored = registry::load(&destination).unwrap().unwrap();
    assert_eq!(restored.run_provider, Id::Codex);
    assert_eq!(restored.accounts, registry.accounts);
    assert_eq!(restored.aliases, registry.aliases);
    assert_eq!(
        exported_credential(&destination, &restored, &restored.accounts[0])
            .unwrap()
            .unwrap()
            .as_str(),
        credential("personal", EMAIL)
    );
}

#[test]
fn a_codex_refresh_saves_attributed_windows_and_a_blocked_refresh_keeps_their_age() {
    let host = machine("personal");
    add_account(&host, "personal");
    let host = with_codex_limits(host);
    common::run_list_refresh(&host, true).0.unwrap();
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let before = account.utilization.clone().unwrap();
    assert_eq!(before.windows[0].used_percent, 32.0);
    host.set_file(
        account
            .profile_dir(&host)
            .unwrap()
            .join(format!("sessions/{}.json", host.process_id())),
        &json!({"startedAt": host.now().timestamp_millis(), "writtenBy": "perch"}).to_string(),
    );
    common::run_list_refresh(&host, true).0.unwrap();
    let after = registry::load(&host).unwrap().unwrap();
    assert_eq!(after.accounts[0].utilization.as_ref(), Some(&before));
}

#[test]
fn mixed_groups_list_every_provider_without_a_joint_quota_ranking() {
    let host = common::three_accounts_in_one_group();
    let document = credential("personal", EMAIL);
    let host = host.with_file(CODEX, "").with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add::run(
        &host,
        add::AddArgs {
            provider: Selection {
                provider: None,
                codex: true,
                claude: false,
            },
            alias: Some("personal".into()),
            group: Some("work".into()),
            no_group: false,
        },
        &mut Vec::new(),
    )
    .unwrap();
    let registry = registry::load(&host).unwrap().unwrap();
    let section = perch::listing::Section::of(
        &registry,
        perch::config::Scope::Group("work".into()),
        host.now(),
    );
    let value = section.document(&host, &registry, &registry.aliases_by_account(), host.now());
    assert_eq!(value["order"], "held");
    assert_eq!(value["accounts"].as_array().unwrap().len(), 4);
}

fn exported_credential(
    host: &FakeHost,
    registry: &registry::Registry,
    account: &registry::Account,
) -> perch::Result<Option<String>> {
    let bundle = account
        .provider()
        .adapter()
        .snapshot(host, &registry.profile_context(host, account)?)?;
    Ok(
        serde_json::to_value(bundle).unwrap()["artifacts"]["auth.json"]["content"]
            .as_str()
            .map(str::to_owned),
    )
}

fn with_codex_limits(host: FakeHost) -> FakeHost {
    let response = [
        json!({"id":1,"result":{}}),
        json!({"id":2,"result":{"account":{"type":"chatgpt","email":EMAIL}}}),
        json!({"id":3,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":32,"windowDurationMins":300,"resetsAt":1800000000}}}}),
    ].map(|value| value.to_string()).join("\n");
    host.with_exec_under(
        CODEX,
        &[
            "app-server",
            "-c",
            "cli_auth_credentials_store=\"file\"",
            "-c",
            "forced_login_method=\"chatgpt\"",
        ],
        perch::host::Execution {
            status: 0,
            stdout: response,
            stderr: String::new(),
        },
    )
}

#[test]
fn codex_observation_honors_cancellation_before_spending_and_before_returning_figures() {
    for stop_at in [1, 2, 3, 4, 5] {
        let host = machine("personal");
        add_account(&host, "personal");
        let host = with_codex_limits(host);
        let registry = registry::load(&host).unwrap().unwrap();
        let account = &registry.accounts[0];
        let mut held = perch::holdings::lock(&host).unwrap();
        host.forget_effects();
        let mut checkpoints = 0;
        let result = account
            .provider()
            .adapter()
            .configured(&host)
            .unwrap()
            .observe(
                &host,
                &mut held,
                &registry.profile_context(&host, account).unwrap(),
                &account.profile(&host).unwrap(),
                &mut || {
                    checkpoints += 1;
                    if checkpoints == stop_at {
                        Err(perch::lock::Lost::Stopped)
                    } else {
                        Ok(())
                    }
                },
            );
        assert!(matches!(
            result,
            Err(perch::observe::Outcome::Stopped(perch::lock::Lost::Stopped))
        ));
        let requests = host.effects().iter().filter(|effect| matches!(effect,
            Effect::ExecUnder { program, args, .. } if program == CODEX && args.first().is_some_and(|arg| arg == "app-server")
        )).count();
        assert_eq!(requests, usize::from(stop_at >= 4));
    }
}

#[test]
fn the_provider_handle_refuses_a_foreign_profile_before_native_effects() {
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let profile = account.profile(&host).unwrap();
    let context = registry.profile_context(&host, account).unwrap();
    let claude = Id::Claude.adapter();
    host.forget_effects();
    assert!(
        claude
            .check_replacement(&host, &profile, None, &perch::live::NOTHING_WAS_CHANGED)
            .is_err()
    );
    assert!(claude.forget_credential(&host, &profile).is_err());
    assert!(claude.snapshot(&host, &context).is_err());
    assert!(
        claude
            .prepare_launch(
                &host,
                &perch::providers::provider::LaunchRequest {
                    kind: perch::providers::provider::LaunchKind::Custom("true"),
                    account: &profile,
                    arguments: &[],
                    shared_profiles: Vec::new(),
                }
            )
            .is_err()
    );
    assert!(
        claude
            .prepare_restore(
                &host,
                perch::providers::provider::RestoreRequest {
                    profile,
                    bundle: None,
                }
            )
            .is_err()
    );
    assert!(host.effects().is_empty(), "{:?}", host.effects());
}

#[test]
fn orphan_credential_deletion_refuses_foreign_and_unmanaged_directories() {
    let host = FakeHost::new();
    let claude = Id::Claude.adapter();
    let home = Id::Claude.home(&host).unwrap();
    for path in [
        Id::Codex.home(&host).unwrap().join("profiles/orphan"),
        home.join("profiles"),
        home.join("profiles/orphan/nested"),
        home.join("profiles/../outside"),
        home.join("unmanaged/orphan"),
        std::path::PathBuf::from("/tmp/unmanaged"),
    ] {
        host.forget_effects();
        assert!(claude.forget_profile_credential(&host, &path).is_err());
        assert!(host.effects().is_empty(), "{:?}", host.effects());
    }
}

#[test]
fn codex_credential_deletion_reports_its_own_store_and_preserves_configuration() {
    let host = FakeHost::new();
    let codex = Id::Codex.adapter();
    for folder in ["profiles", "pending"] {
        let profile = Id::Codex.home(&host).unwrap().join(folder).join("orphan");
        host.set_file(profile.join("auth.json"), "synthetic credential");
        host.set_file(profile.join("config.toml"), "synthetic configuration");
        let first = codex.forget_profile_credential(&host, &profile).unwrap();
        assert!(first.removed);
        assert!(first.note.is_none());
        assert!(host.file(profile.join("auth.json")).is_none());
        assert_eq!(
            host.file(profile.join("config.toml")).as_deref(),
            Some("synthetic configuration")
        );
        let second = codex.forget_profile_credential(&host, &profile).unwrap();
        assert!(!second.removed);
        assert!(second.note.is_none());
    }
}

#[test]
fn liveness_uses_each_providers_session_evidence_in_a_mixed_query() {
    use perch::live::{self, Place};

    let host = FakeHost::new();
    let claude = Id::Claude.home(&host).unwrap().join("profiles/one");
    let codex = Id::Codex.home(&host).unwrap().join("profiles/two");
    let pid = host.process_id();
    let native = json!({"startedAt": host.now().timestamp_millis()}).to_string();
    for directory in [&claude, &codex] {
        host.set_file(directory.join(format!("sessions/{pid}.json")), &native);
    }
    let places = [Place::at(Id::Claude, &claude), Place::at(Id::Codex, &codex)];
    let live::Answer::NotIdle(live::NotIdle::Live(clients)) = live::ask(&host, &places) else {
        panic!("Claude's native marker names a running client");
    };
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].whose, claude.display().to_string());

    host.set_file(
        codex.join(format!("sessions/{pid}.json")),
        &json!({"startedAt": host.now().timestamp_millis(), "writtenBy": "perch"}).to_string(),
    );
    let live::Answer::NotIdle(live::NotIdle::Live(clients)) = live::ask(&host, &places) else {
        panic!("both providers have evidence of a running client");
    };
    assert_eq!(clients.len(), 2);
}

#[test]
fn an_unreadable_codex_session_is_refused_without_claiming_a_claude_version() {
    use perch::host::Refusing;
    use perch::live::{self, Place};

    let host = FakeHost::new();
    let profile = Id::Codex.home(&host).unwrap().join("profiles").join("one");
    let marker = profile
        .join("sessions")
        .join(format!("{}.json", host.process_id()));
    host.set_file(&marker, "unreadable");
    let host = host.with_a_path_refusing(&marker, Refusing::Read, "Permission denied");
    let error = live::ask(&host, &[Place::at(Id::Codex, &profile)])
        .idle_or(&live::NOTHING_WAS_CHANGED)
        .err()
        .expect("an unreadable live marker cannot establish an idle Profile");
    assert_eq!(error.exit_code(), perch::error::EXIT_PROBE_REFUSED);
    let said = error.to_string();
    assert!(said.contains(&marker.display().to_string()), "{said}");
    assert!(!said.contains("Claude"), "{said}");
}

#[test]
fn probe_reports_each_installed_provider_and_attributes_failures() {
    use perch::commands::probe::{self, ProbeArgs};
    use perch::host::Execution;

    let host = common::machine_with_two_accounts()
        .with_file(CODEX, "")
        .with_exec(
            CODEX,
            &["--version"],
            Execution {
                status: 0,
                stdout: "codex-cli 0.100.0\n".into(),
                stderr: String::new(),
            },
        );
    let mut output = Vec::new();
    probe::run(
        &host,
        ProbeArgs {
            json: true,
            raw: false,
        },
        &mut output,
    )
    .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let providers = report["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2);
    assert_eq!(report["holdings"]["active"]["codex"], "nobody");
    assert_ne!(report["holdings"]["active"]["claude"], "nobody");
    let claude = providers
        .iter()
        .find(|provider| provider["id"] == "claude")
        .unwrap();
    let codex = providers
        .iter()
        .find(|provider| provider["id"] == "codex")
        .unwrap();
    assert_eq!(claude["assumptions"].as_array().unwrap().len(), 6);
    assert_eq!(codex["version"], "codex-cli 0.100.0");

    host.set_exec(
        CODEX,
        &["--version"],
        Execution {
            status: 1,
            stdout: String::new(),
            stderr: "failed".into(),
        },
    );
    output.clear();
    probe::run(
        &host,
        ProbeArgs {
            json: true,
            raw: false,
        },
        &mut output,
    )
    .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "provider-unreadable"
                && finding["provider"] == "codex")
    );
    assert!(
        report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["id"] == "claude" && provider["version"].is_string())
    );
}

#[test]
fn triage_uses_the_preferred_codex_provider_without_a_managed_profile() {
    use perch::commands::triage::{self, TriageArgs};
    use perch::host::Execution;

    let host = machine("personal").with_exec(
        CODEX,
        &["--version"],
        Execution {
            status: 0,
            stdout: "codex-cli 0.100.0\n".into(),
            stderr: String::new(),
        },
    );
    config::run(
        &host,
        config::ConfigCommand::Set {
            words: ["--global", "run-provider", "codex"]
                .map(String::from)
                .into(),
        },
        &mut Vec::new(),
    )
    .unwrap();
    host.forget_effects();
    triage::run(
        &host,
        TriageArgs {
            model: Some("chosen-model".into()),
            raw: false,
        },
        &mut Vec::new(),
    )
    .unwrap();
    assert!(
        host.effects().iter().any(|effect| matches!(effect,
            Effect::ExecInteractive { program, args, env, .. }
            if program == CODEX && args.first().is_some_and(|arg| arg == "--model")
            && args.get(1).is_some_and(|arg| arg == "chosen-model")
            && args.last().is_some_and(|arg| arg.contains("prompt.md"))
            && env.is_empty()
        )),
        "{:?}",
        host.effects()
    );
    assert!(!host.effects().iter().any(|effect| matches!(effect,
        Effect::WroteFile(path) if path.components().any(|part| part.as_os_str() == "sessions")
    )));
}

#[test]
fn default_inspection_refuses_foreign_profiles_before_reading_credentials() {
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let profile = registry.accounts[0].profile(&host).unwrap();
    let mut held = perch::holdings::lock(&host).unwrap();
    let mut inspection = Id::Claude.adapter().inspect_default(&host).unwrap();
    host.forget_effects();
    let error = inspection
        .resolve(&mut held, &[profile], None, "personal", &mut || true)
        .err()
        .expect("a foreign Profile is refused");
    assert!(
        error
            .to_string()
            .contains("cannot operate on a codex Profile")
    );
    assert!(host.effects().is_empty(), "{:?}", host.effects());
}

#[test]
fn managed_paths_keep_every_catalog_provider_in_its_own_namespace() {
    use perch::holdings;
    let host = FakeHost::new();
    let mut profiles = std::collections::BTreeSet::new();
    let mut pending = std::collections::BTreeSet::new();
    for provider in perch::providers::provider::catalog() {
        let id = provider.id();
        let profile = holdings::profile_dir_for(id, &host, "shared-key").unwrap();
        assert_eq!(
            profile.parent().unwrap(),
            holdings::profiles_dir(id, &host).unwrap()
        );
        assert_eq!(profile, id.home(&host).unwrap().join("profiles/shared-key"));
        assert!(profiles.insert(profile));
        let login = holdings::pending_login_dir(id, &host, host.now()).unwrap();
        assert_eq!(
            login.parent().unwrap(),
            holdings::pending_logins_dir(id, &host).unwrap()
        );
        assert_eq!(holdings::pending_login_started_at(&login), Some(host.now()));
        assert!(pending.insert(login));
        assert!(holdings::profile_dir_for(id, &host, "../").is_err());
    }
}

#[test]
fn a_subject_without_a_workspace_has_a_stable_distinct_storage_identity() {
    use perch::providers::provider::AccountIdentity;
    let subject = AccountIdentity::from_subject(Id::Claude, "user-one".into(), None).unwrap();
    let workspace = AccountIdentity::new(Id::Claude, "user-one".into(), "work".into()).unwrap();
    assert_ne!(subject.key, workspace.key);
    let host = FakeHost::new();
    let mut registry = registry::Registry::default();
    registry.upsert(registry::Account {
        storage_key: None,
        provider: Id::Claude,
        provider_identity: Some(subject.clone()),
        identity: perch::domain::Identity {
            email: EMAIL.into(),
            account_uuid: Some(subject.user_id.clone()),
            organization_name: None,
            organization_uuid: None,
        },
        plan: None,
        disabled: false,
        quarantine: None,
        group: None,
        utilization: None,
    });
    let mut held = perch::holdings::lock(&host).unwrap();
    registry::save(&host, &mut held, &mut registry).unwrap();
    let restored = registry::load(&host).unwrap().unwrap();
    let account = restored.account(&subject.key).unwrap();
    assert_eq!(account.provider_identity, Some(subject.clone()));
    assert!(
        !restored
            .named_for_the_user(&subject.key)
            .contains("Workspace")
    );
    assert_eq!(
        account.profile_dir(&host).unwrap(),
        registry.accounts[0].profile_dir(&host).unwrap()
    );
}

#[test]
fn absent_workspaces_are_provider_policy_and_empty_identifiers_are_always_refused() {
    use perch::providers::provider::AccountIdentity;
    assert!(AccountIdentity::from_subject(Id::Codex, "user".into(), None).is_err());
    for provider in perch::providers::provider::catalog() {
        assert!(AccountIdentity::new(provider.id(), "user".into(), "".into()).is_err());
        assert!(AccountIdentity::new(provider.id(), " ".into(), "work".into()).is_err());
    }
    let mut identity = AccountIdentity::new(Id::Codex, "user".into(), "work".into()).unwrap();
    identity.workspace_id = None;
    assert!(identity.validate(Id::Codex).is_err());
}

#[test]
fn claude_defaults_and_backups_attribute_stable_subjects_instead_of_email() {
    use perch::providers::provider::AccountIdentity;
    let host = common::logged_in_machine();
    let subject = AccountIdentity::new(
        Id::Claude,
        "account-uuid-1".into(),
        "organization-uuid-1".into(),
    )
    .unwrap();
    let account = registry::Account {
        storage_key: None,
        provider: Id::Claude,
        provider_identity: Some(subject.clone()),
        identity: perch::domain::Identity {
            email: "renamed@example.com".into(),
            account_uuid: Some(subject.user_id.clone()),
            organization_uuid: subject.workspace_id.clone(),
            organization_name: None,
        },
        plan: None,
        disabled: false,
        quarantine: None,
        group: None,
        utilization: None,
    };
    let store = common::store_of(&host, account.key());
    common::claude_fixture::stores_for(&host, &store)[0]
        .write(&host, common::SECOND_CREDENTIAL)
        .unwrap();
    let mut registry = registry::Registry::default();
    registry.settle(Some(account.key().into()));
    registry.upsert(account.clone());
    let provider = Id::Claude.adapter();
    let snapshot = || {
        let bundle = provider
            .snapshot(&host, &registry.profile_context(&host, &account).unwrap())
            .unwrap();
        serde_json::to_value(bundle).unwrap()["artifacts"]["oauth"]["content"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(
        provider
            .default_matches(&host, &account.profile(&host).unwrap())
            .unwrap()
    );
    assert_eq!(snapshot(), common::CREDENTIAL);
    for native in [
        common::IDENTITY_FILE.replace("organization-uuid-1", "another-workspace"),
        common::IDENTITY_FILE.replace("account-uuid-1", "another-user"),
        common::IDENTITY_FILE.replace("\"accountUuid\": \"account-uuid-1\",", ""),
    ] {
        host.set_file(
            common::IDENTITY_PATH,
            &native.replace(common::EMAIL, account.email()),
        );
        assert!(
            !provider
                .default_matches(&host, &account.profile(&host).unwrap())
                .unwrap()
        );
        assert_eq!(snapshot(), common::SECOND_CREDENTIAL);
    }
    host.remove_file(std::path::Path::new(common::IDENTITY_PATH))
        .unwrap();
    assert_eq!(snapshot(), common::SECOND_CREDENTIAL);
}

#[test]
fn a_claude_subject_without_a_workspace_does_not_match_an_organization_login() {
    use perch::providers::provider::AccountIdentity;
    let host = common::logged_in_machine();
    let mut account = registry::Account {
        storage_key: None,
        provider: Id::Claude,
        provider_identity: Some(
            AccountIdentity::from_subject(Id::Claude, "account-uuid-1".into(), None).unwrap(),
        ),
        identity: perch::domain::Identity {
            email: common::EMAIL.into(),
            account_uuid: Some("account-uuid-1".into()),
            organization_uuid: None,
            organization_name: None,
        },
        plan: None,
        disabled: false,
        quarantine: None,
        group: None,
        utilization: None,
    };
    let provider = Id::Claude.adapter();
    assert!(
        !provider
            .default_matches(&host, &account.profile(&host).unwrap())
            .unwrap()
    );
    host.set_file(
        common::IDENTITY_PATH,
        &common::IDENTITY_FILE.replace("\"organizationUuid\": \"organization-uuid-1\",", ""),
    );
    assert!(
        provider
            .default_matches(&host, &account.profile(&host).unwrap())
            .unwrap()
    );
    account.identity.email = "changed@example.com".into();
    assert!(
        provider
            .default_matches(&host, &account.profile(&host).unwrap())
            .unwrap()
    );
}

#[test]
fn claude_usage_requires_the_remote_subject_and_workspace_even_when_email_matches() {
    use perch::providers::provider::AccountIdentity;
    let valid = json!({"account":{"uuid":"account-uuid-1","email":"renamed@example.com"},"organization":{"uuid":"organization-uuid-1"}});
    let replies = [
        (valid.clone(), true),
        (
            json!({"account":{"uuid":"another-user","email":common::EMAIL},"organization":{"uuid":"organization-uuid-1"}}),
            false,
        ),
        (
            json!({"account":{"uuid":"account-uuid-1","email":common::EMAIL},"organization":{"uuid":"another-workspace"}}),
            false,
        ),
        (json!({"account":{"email":common::EMAIL}}), false),
        (
            json!({"account":{"uuid":"account-uuid-1","email":common::EMAIL},"organization":{"uuid":null}}),
            false,
        ),
        (
            json!({"account":{"uuid":"account-uuid-1","email":common::EMAIL},"organization":{"uuid":" "}}),
            false,
        ),
        (
            json!({"account":{"uuid":"account-uuid-1"},"organization":{"uuid":"organization-uuid-1"}}),
            false,
        ),
    ];
    for (reply, allowed) in replies {
        let host = common::machine_with_claude_code()
            .with_reply_to(
                common::PROFILE_URL,
                "sk-ant-oat01-test",
                200,
                &reply.to_string(),
            )
            .with_reply_to(
                common::USAGE_URL,
                "sk-ant-oat01-test",
                200,
                &common::usage(25.0),
            );
        let subject = AccountIdentity::new(
            Id::Claude,
            "account-uuid-1".into(),
            "organization-uuid-1".into(),
        )
        .unwrap();
        let account = registry::Account {
            storage_key: None,
            provider: Id::Claude,
            provider_identity: Some(subject.clone()),
            identity: perch::domain::Identity {
                email: common::EMAIL.into(),
                account_uuid: Some(subject.user_id.clone()),
                organization_uuid: subject.workspace_id.clone(),
                organization_name: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        };
        let store = common::store_of(&host, account.key());
        common::claude_fixture::stores_for(&host, &store)[0]
            .write(
                &host,
                &common::CREDENTIAL.replace("1785000000000", "4102444800000"),
            )
            .unwrap();
        host.set_file(&store.identity_file, common::IDENTITY_FILE);
        let mut registry = registry::Registry::default();
        registry.upsert(account.clone());
        let mut held = perch::holdings::lock(&host).unwrap();
        let result = Id::Claude.adapter().configured(&host).unwrap().observe(
            &host,
            &mut held,
            &registry.profile_context(&host, &account).unwrap(),
            &account.profile(&host).unwrap(),
            &mut || Ok(()),
        );
        assert_eq!(result.is_ok(), allowed, "{reply}: {result:?}");
        assert_eq!(
            host.effects()
                .iter()
                .any(|effect| matches!(effect, Effect::Http { url } if url == common::USAGE_URL)),
            allowed,
            "{reply}"
        );
    }
}

#[test]
fn claude_enrollment_and_relogin_keep_storage_stable_when_email_changes() {
    let host = common::logged_in_machine();
    let initial = perch::adopt::ensure_adopted(&host).unwrap();
    let first = &initial.accounts[0];
    let key = first.key().to_owned();
    let path = first.profile_dir(&host).unwrap();
    assert!(key.starts_with("claude:"));
    assert_eq!(
        first.provider_identity.as_ref().unwrap().user_id,
        "account-uuid-1"
    );
    let renamed = common::IDENTITY_FILE.replace(common::EMAIL, "new@example.com");
    let host = host.with_login(move |host, dir| {
        let store = common::claude_fixture::store_for_profile(host, dir).unwrap();
        common::claude_fixture::stores_for(host, &store)[0]
            .write(host, common::CREDENTIAL)
            .unwrap();
        host.set_file(dir.join(".claude.json"), &renamed);
        0
    });
    common::run_relogin(&host, common::EMAIL).0.unwrap();
    let updated = registry::load(&host).unwrap().unwrap();
    let account = updated.account(&key).unwrap();
    assert_eq!(account.email(), "new@example.com");
    assert_eq!(account.profile_dir(&host).unwrap(), path);
    let duplicate = common::run_add(
        &host,
        add::AddArgs {
            no_group: true,
            ..Default::default()
        },
    )
    .0
    .unwrap_err();
    assert!(
        duplicate.to_string().contains("already holds"),
        "{duplicate}"
    );
    assert_eq!(registry::load(&host).unwrap().unwrap().accounts.len(), 1);
}

#[test]
fn claude_enrollment_refuses_an_email_without_a_stable_subject() {
    let identity = common::IDENTITY_FILE.replace("\"accountUuid\": \"account-uuid-1\",", "");
    let host = common::machine_with_claude_code().with_login(move |host, dir| {
        let store = common::claude_fixture::store_for_profile(host, dir).unwrap();
        common::claude_fixture::stores_for(host, &store)[0]
            .write(host, common::CREDENTIAL)
            .unwrap();
        host.set_file(dir.join(".claude.json"), &identity);
        0
    });
    let error = common::run_add(
        &host,
        add::AddArgs {
            no_group: true,
            ..Default::default()
        },
    )
    .0
    .unwrap_err();
    assert!(error.to_string().contains("stable account UUID"), "{error}");
    assert!(registry::load(&host).unwrap().is_none());
}

#[test]
fn claude_workspaces_with_the_same_email_enroll_as_separate_accounts() {
    let host = common::logged_in_machine();
    let first = perch::adopt::ensure_adopted(&host).unwrap().accounts[0].clone();
    let work = common::IDENTITY_FILE.replace("organization-uuid-1", "work-workspace");
    let host = host.with_login(move |host, dir| {
        let store = common::claude_fixture::store_for_profile(host, dir).unwrap();
        common::claude_fixture::stores_for(host, &store)[0]
            .write(host, common::SECOND_CREDENTIAL)
            .unwrap();
        host.set_file(dir.join(".claude.json"), &work);
        0
    });
    common::run_add(
        &host,
        add::AddArgs {
            no_group: true,
            alias: Some("work".into()),
            ..Default::default()
        },
    )
    .0
    .unwrap();
    let registry = registry::load(&host).unwrap().unwrap();
    let selected = target::resolve_account(&registry, "work").unwrap();
    let second = registry.held(&selected.email).unwrap();
    assert_eq!(registry.accounts.len(), 2);
    assert_eq!(first.email(), second.email());
    assert_ne!(first.key(), second.key());
    assert_ne!(
        first.profile_dir(&host).unwrap(),
        second.profile_dir(&host).unwrap()
    );
    assert!(
        target::resolve_account(&registry, common::EMAIL)
            .unwrap_err()
            .to_string()
            .contains("names more than one Account")
    );
    let exported = perch::export::gather(&host, &registry).unwrap();
    assert_eq!(
        common::exported_artifact(&exported, first.key(), "oauth").as_deref(),
        Some(common::CREDENTIAL)
    );
    assert_eq!(
        common::exported_artifact(&exported, second.key(), "oauth").as_deref(),
        Some(common::SECOND_CREDENTIAL)
    );
}

#[test]
fn a_run_keeps_its_selected_installation_when_configuration_changes_during_the_lock_wait() {
    for provider in perch::providers::provider::catalog() {
        let id = provider.id();
        let (host, target) = match id {
            Id::Claude => (common::machine_with_two_accounts(), common::SECOND_EMAIL),
            Id::Codex => {
                let host = machine("personal");
                add_account(&host, "personal");
                (host, "personal")
            }
        };
        let selected = id.executable(&host).unwrap();
        let replacement = "/configured/replacement";
        let lock = perch::holdings::lock_spec(&host).unwrap();
        let now = host.now();
        let host = host
            .with_file(replacement, "")
            .with_login(|_, _| 0)
            .with_dir_held_since(&lock.dir, now)
            .once_while_waiting(move |host| {
                host.remove_dir_all(&lock.dir).unwrap();
                let mut held = perch::holdings::lock(host).unwrap();
                let mut registry = registry::load(host).unwrap().unwrap();
                let settings = registry.provider_settings.get_mut(&id).unwrap();
                settings.cli_path = Some(replacement.into());
                settings.enabled = false;
                registry::save(host, &mut held, &mut registry).unwrap();
            });
        host.forget_effects();
        run::run(
            &host,
            run::RunArgs {
                provider: Selection {
                    provider: Some(id),
                    ..Default::default()
                },
                target: target.into(),
                command: vec!["--version".into()],
            },
            &mut Vec::new(),
        )
        .unwrap();
        let launched: Vec<_> = host
            .effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::ExecInteractive { program, .. } => Some(program),
                _ => None,
            })
            .collect();
        assert_eq!(launched, vec![selected.to_string_lossy().into_owned()]);
        assert!(!id.adapter().configured(&host).unwrap().enabled());
    }
}

#[test]
fn launch_requests_reject_foreign_installations_and_empty_custom_programs_before_effects() {
    use perch::providers::provider::{LaunchKind, LaunchRequest};
    let host = common::machine_with_two_accounts().with_file(CODEX, "");
    let registry = registry::load(&host).unwrap().unwrap();
    let profile = registry.accounts[0].profile(&host).unwrap();
    let foreign = Id::Codex
        .adapter()
        .configured(&host)
        .unwrap()
        .installation(&host)
        .unwrap();
    for kind in [LaunchKind::Client(&foreign), LaunchKind::Custom("")] {
        host.forget_effects();
        assert!(
            Id::Claude
                .adapter()
                .prepare_launch(
                    &host,
                    &LaunchRequest {
                        kind,
                        account: &profile,
                        arguments: &[],
                        shared_profiles: vec![],
                    }
                )
                .is_err()
        );
        assert!(host.effects().is_empty(), "{:?}", host.effects());
    }
}

#[test]
fn authentication_keeps_the_selected_installation_after_configuration_changes() {
    for provider in perch::providers::provider::catalog() {
        let id = provider.id();
        let host = match id {
            Id::Claude => common::logged_in_machine().with_login(common::login_producing(
                common::SECOND_CREDENTIAL,
                common::SECOND_IDENTITY_FILE,
            )),
            Id::Codex => machine("personal"),
        };
        let installation = provider
            .configured(&host)
            .unwrap()
            .installation(&host)
            .unwrap();
        let mut held = perch::holdings::lock(&host).unwrap();
        let mut registry = registry::load(&host).unwrap().unwrap_or_default();
        let settings = registry.provider_settings.get_mut(&id).unwrap();
        settings.cli_path = Some("/missing/replacement".into());
        settings.enabled = false;
        registry::save(&host, &mut held, &mut registry).unwrap();
        drop(held);
        host.forget_effects();
        let authenticated = installation.authenticate(&host).unwrap();
        assert!(authenticated.subject().is_some());
        let programs: Vec<_> = host
            .effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::ExecInteractive { program, .. } => Some(program),
                _ => None,
            })
            .collect();
        assert_eq!(
            programs,
            vec![installation.executable().to_string_lossy().into_owned()]
        );
        assert!(
            provider
                .configured(&host)
                .unwrap()
                .installation(&host)
                .is_err()
        );
    }
}

#[test]
fn discovery_uses_the_selected_installation_when_provider_settings_change() {
    let host = common::logged_in_machine();
    let provider = Id::Claude.adapter();
    let installation = provider
        .configured(&host)
        .unwrap()
        .installation(&host)
        .unwrap();
    let mut registry = registry::Registry::default();
    let settings = registry.provider_settings.get_mut(&Id::Claude).unwrap();
    settings.enabled = false;
    settings.cli_path = Some("/missing/replacement".into());
    common::save_registry(&host, &registry);
    let discovered = installation.discover(&host).unwrap().unwrap();
    assert_eq!(discovered.account.identity().email, common::EMAIL);
    assert!(
        provider
            .configured(&host)
            .unwrap()
            .installation(&host)
            .is_err()
    );
}

#[test]
fn observations_keep_their_configured_provider_after_settings_change() {
    for provider in perch::providers::provider::catalog() {
        let id = provider.id();
        let host = match id {
            Id::Claude => {
                let host = common::logged_in_machine();
                let store = common::claude_fixture::default_store(&host).unwrap();
                common::claude_fixture::stores_for(&host, &store)[0]
                    .write(
                        &host,
                        &common::CREDENTIAL.replace("1785000000000", "4102444800000"),
                    )
                    .unwrap();
                perch::adopt::ensure_adopted(&host).unwrap();
                host.with_reply_to(
                    common::PROFILE_URL,
                    "sk-ant-oat01-test",
                    200,
                    &common::profile_of(common::EMAIL),
                )
                .with_reply_to(
                    common::USAGE_URL,
                    "sk-ant-oat01-test",
                    200,
                    &common::usage(25.0),
                )
            }
            Id::Codex => {
                let host = machine("personal");
                add_account(&host, "personal");
                with_codex_limits(host)
            }
        };
        let configured = provider.configured(&host).unwrap();
        let mut registry = registry::load(&host).unwrap().unwrap();
        let account = registry.accounts[0].clone();
        let settings = registry.provider_settings.get_mut(&id).unwrap();
        settings.enabled = false;
        settings.cli_path = Some("/missing/replacement".into());
        let mut held = perch::holdings::lock(&host).unwrap();
        registry::save(&host, &mut held, &mut registry).unwrap();
        host.forget_effects();
        let result = configured.observe(
            &host,
            &mut held,
            &registry.profile_context(&host, &account).unwrap(),
            &account.profile(&host).unwrap(),
            &mut || Ok(()),
        );
        assert!(result.is_ok(), "{id:?}: {result:?}");
        assert!(
            provider
                .configured(&host)
                .unwrap()
                .installation(&host)
                .is_err()
        );
        if id == Id::Claude {
            assert!(!host.effects().iter().any(|effect| matches!(effect,
                Effect::Exec { args, .. } if args == &["--version"]
            )));
        }
    }
}

#[test]
fn diagnostics_keep_the_selected_configuration_and_read_one_version() {
    use perch::providers::provider::DiagnosticSession;
    for provider in perch::providers::provider::catalog() {
        let id = provider.id();
        let host = match id {
            Id::Claude => common::logged_in_machine(),
            Id::Codex => machine("personal").with_exec(
                CODEX,
                &["--version"],
                perch::host::Execution {
                    status: 0,
                    stdout: "codex-cli 0.115.0".into(),
                    stderr: String::new(),
                },
            ),
        }
        .with_login(|_, _| 0);
        let configured = provider.configured(&host).unwrap();
        let installation = configured.installation(&host).unwrap();
        let mut registry = registry::Registry::default();
        let settings = registry.provider_settings.get_mut(&id).unwrap();
        settings.enabled = false;
        settings.cli_path = Some("/missing/replacement".into());
        common::save_registry(&host, &registry);
        host.forget_effects();
        let report = configured.diagnose(&host);
        assert!(report.version.is_ok(), "{:?}", report.version);
        assert_eq!(report.path.as_deref(), Some(installation.executable()));
        assert_eq!(
            host.effects()
                .iter()
                .filter(|effect| matches!(effect,
                    Effect::Exec { args, .. } if args == &["--version"]
                ))
                .count(),
            1
        );
        let session = installation
            .diagnostic_session(&DiagnosticSession {
                model: Some("selected-model"),
                prompt: "Read the diagnostic report",
            })
            .unwrap();
        assert_eq!(session.execute(&host).unwrap(), 0);
        assert!(host.effects().iter().any(|effect| matches!(effect,
            Effect::ExecInteractive { program, args, .. }
            if program == &installation.executable().to_string_lossy()
                && args == &["--model", "selected-model", "Read the diagnostic report"]
        )));
        let refused = provider.diagnose(&host);
        assert!(refused.version.is_err());
        assert!(
            refused
                .findings
                .iter()
                .all(|finding| finding.provider == Some(id))
        );
    }
}

#[cfg(unix)]
#[test]
fn rpc_cancellation_and_deadlines_reap_children_blocked_on_reads_or_writes() {
    use perch::host::{HostError, Processes, RealHost, RpcControl};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    let host = RealHost::new();
    for writing in [false, true] {
        for cancel in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "perch-rpc-{}-{}-ready",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let script = if writing {
                "printf '%s' \"$$\" > \"$1\"; kill -STOP \"$$\""
            } else {
                "read -r request; printf '%s' \"$$\" > \"$1\"; kill -STOP \"$$\""
            };
            let request = json!({"id":1,"method":"test","params": if writing { "x".repeat(4_194_304) } else { String::new() }}).to_string();
            let began = Instant::now();
            let mut ready_at = None;
            let result = host.rpc(
                "/bin/sh",
                &["-c", script, "perch-rpc-fixture", path.to_str().unwrap()],
                &[],
                &[request],
                RpcControl {
                    timeout: if cancel {
                        Duration::from_secs(5)
                    } else {
                        Duration::from_millis(750)
                    },
                    checkpoint: &mut || {
                        if path.exists() {
                            let ready = ready_at.get_or_insert_with(Instant::now);
                            if cancel && ready.elapsed() >= Duration::from_millis(100) {
                                return Err(HostError::Other("fixture cancellation".into()));
                            }
                        }
                        Ok(())
                    },
                },
            );
            let error = result.unwrap_err().to_string();
            let pid: u32 = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            std::fs::remove_file(&path).unwrap();
            assert!(
                !host.process_alive(pid),
                "RPC child {pid} survived: {error}"
            );
            assert!(began.elapsed() < Duration::from_secs(4), "{error}");
            assert!(
                error.contains(if cancel {
                    "fixture cancellation"
                } else {
                    "deadline expired"
                }),
                "{error}"
            );
        }
    }
}

#[test]
fn rpc_refuses_cancellation_and_expired_deadlines_before_starting_a_child() {
    use perch::host::{Host, HostError, RealHost, RpcControl};
    let fake = FakeHost::new();
    let real = RealHost::new();
    for host in [&fake as &dyn Host, &real as &dyn Host] {
        for cancel in [false, true] {
            let error = host
                .rpc(
                    "/no-such-rpc-fixture",
                    &[],
                    &[],
                    &[],
                    RpcControl {
                        timeout: std::time::Duration::ZERO,
                        checkpoint: &mut || {
                            if cancel {
                                Err(HostError::Other("fixture cancellation".into()))
                            } else {
                                Ok(())
                            }
                        },
                    },
                )
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(if cancel {
                    "fixture cancellation"
                } else {
                    "deadline expired"
                }),
                "{error}"
            );
        }
    }
    assert!(fake.effects().is_empty());
}

#[test]
fn codex_releases_its_profile_when_the_registry_lock_is_lost_during_rpc() {
    let host = machine("personal");
    add_account(&host, "personal");
    let host = with_codex_limits(host);
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let configured = account.provider().adapter().configured(&host).unwrap();
    let context = registry.profile_context(&host, account).unwrap();
    let profile = account.profile(&host).unwrap();
    let mut held = perch::holdings::lock(&host).unwrap();
    let lock = perch::holdings::lock_spec(&host).unwrap();
    let mut checkpoints = 0;
    let result = configured.observe(&host, &mut held, &context, &profile, &mut || {
        checkpoints += 1;
        if checkpoints == 4 {
            host.set_now(host.now() + chrono::Duration::milliseconds(lock.update_millis));
            host.remove_dir_all(&lock.dir).unwrap();
        }
        Ok(())
    });
    assert!(
        matches!(result, Err(perch::observe::Outcome::Failed { ref why, spent: true })
        if why.contains("configuration lock was lost")),
        "{result:?}"
    );
    drop(held);
    let mut held = perch::holdings::lock(&host).unwrap();
    assert!(
        configured
            .observe(&host, &mut held, &context, &profile, &mut || Ok(()))
            .is_ok()
    );
}

#[test]
fn oversized_bundles_are_refused_before_restore_effects_or_export_encryption() {
    use perch::providers::provider::{ProfileBundle, RestoreRequest};
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let profile = account.profile(&host).unwrap();
    let bundle: ProfileBundle = serde_json::from_value(json!({"artifacts":{
        "auth.json":{"purpose":"credential","content":"x".repeat(16 * 1024 * 1024 + 1)}
    }}))
    .unwrap();
    host.forget_effects();
    let error = {
        let result = account.provider().adapter().prepare_restore(
            &host,
            RestoreRequest {
                profile,
                bundle: Some(&bundle),
            },
        );
        match result {
            Err(error) => error,
            Ok(_) => panic!("oversized bundle accepted"),
        }
    };
    assert!(error.to_string().contains("16 MiB"));
    assert!(host.effects().is_empty(), "{:?}", host.effects());
    let mut export = perch::export::Export {
        version: perch::export::CURRENT_VERSION,
        registry,
        profiles: Default::default(),
    };
    export.profiles.insert("fixture".into(), bundle);
    assert!(
        perch::export::seal(&export, "fixture passphrase")
            .unwrap_err()
            .to_string()
            .contains("16 MiB")
    );
}

#[test]
fn a_bundle_whose_configuration_leaves_the_file_store_is_refused_before_any_write() {
    use perch::providers::provider::{ProfileBundle, RestoreRequest};
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let profile = account.profile(&host).unwrap();
    host.remove_dir_all(profile.directory()).unwrap();
    let bundle: ProfileBundle = serde_json::from_value(json!({"artifacts":{
        "auth.json":{"purpose":"credential","content":credential("personal", EMAIL)},
        "config.toml":{"purpose":"configuration","content":"cli_auth_credentials_store = \"keyring\"\n"}
    }}))
    .unwrap();
    host.forget_effects();
    let error = match account.provider().adapter().prepare_restore(
        &host,
        RestoreRequest {
            profile,
            bundle: Some(&bundle),
        },
    ) {
        Err(error) => error,
        Ok(_) => panic!("a keyring configuration was accepted"),
    };
    assert!(error.to_string().contains("file store"), "{error}");
    assert!(host.effects().is_empty(), "{:?}", host.effects());
}

#[test]
fn a_restore_onto_a_profile_that_exists_is_refused_before_any_write() {
    use perch::providers::provider::RestoreRequest;
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let profile = account.profile(&host).unwrap();
    host.forget_effects();
    let error = match account.provider().adapter().prepare_restore(
        &host,
        RestoreRequest {
            profile,
            bundle: None,
        },
    ) {
        Err(error) => error,
        Ok(_) => panic!("a Profile that exists was restored over"),
    };
    assert!(error.to_string().contains("already exists"), "{error}");
    assert!(host.effects().is_empty(), "{:?}", host.effects());
}

#[test]
fn a_bundle_credential_for_another_workspace_is_refused_before_any_write() {
    use perch::providers::provider::{ProfileBundle, RestoreRequest};
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let account = &registry.accounts[0];
    let profile = account.profile(&host).unwrap();
    host.remove_dir_all(profile.directory()).unwrap();
    let bundle: ProfileBundle = serde_json::from_value(json!({"artifacts":{
        "auth.json":{"purpose":"credential","content":credential("work", EMAIL)}
    }}))
    .unwrap();
    host.forget_effects();
    let error = match account.provider().adapter().prepare_restore(
        &host,
        RestoreRequest {
            profile,
            bundle: Some(&bundle),
        },
    ) {
        Err(error) => error,
        Ok(_) => panic!("another Workspace's Credential was accepted"),
    };
    assert!(error.to_string().contains("another Account"), "{error}");
    assert!(host.effects().is_empty(), "{:?}", host.effects());
}

#[test]
fn unsupported_shared_state_is_refused_before_native_launch_effects() {
    use perch::providers::provider::{LaunchKind, LaunchRequest, SharedProfile};
    let host = machine("personal");
    add_account(&host, "personal");
    let registry = registry::load(&host).unwrap().unwrap();
    let profile = registry.accounts[0].profile(&host).unwrap();
    host.forget_effects();
    let result = Id::Codex.adapter().prepare_launch(
        &host,
        &LaunchRequest {
            kind: LaunchKind::Custom("true"),
            account: &profile,
            arguments: &[],
            shared_profiles: vec![SharedProfile {
                path: profile.directory().into(),
                is_default: false,
            }],
        },
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("unsupported shared state accepted"),
    };
    assert!(
        error
            .to_string()
            .contains("does not support sharing client state")
    );
    assert!(host.effects().is_empty());
}

#[test]
fn native_restore_cleanup_failures_are_reported_and_preserve_the_original_export() {
    for id in [Id::Claude, Id::Codex] {
        let source = match id {
            Id::Claude => common::machine_with_two_accounts(),
            Id::Codex => {
                let host = machine("personal");
                add_account(&host, "personal");
                host
            }
        };
        let registry = registry::load(&source).unwrap().unwrap();
        let export = perch::export::gather(&source, &registry).unwrap();
        let before = export.clone();
        let host = FakeHost::new().with_platform(perch::host::Platform::Other);
        let path = registry.accounts[0].profile_dir(&host).unwrap();
        let host = host.with_a_path_refusing(
            &path,
            perch::host::Refusing::Delete,
            "fixture cleanup failure",
        );
        let (_, _, fresh) = perch::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();
        let error = perch::import::place(&host, &export, &fresh, || {
            Err(perch::PerchError::Conflict(
                "fixture metadata failure".into(),
            ))
        })
        .unwrap_err();
        let said = error.to_string();
        assert!(said.contains("Rollback incomplete"), "{id:?}: {said}");
        // The directory's own name: the Host spells the separators above it.
        assert!(
            said.contains(path.file_name().unwrap().to_str().unwrap()),
            "{said}"
        );
        assert!(
            !said.contains("Profiles have been taken back out"),
            "{said}"
        );
        assert!(host.path_exists(&path));
        assert_eq!(export, before);
    }
}

/// Two Codex Workspaces under one email, each its own Account, on a machine
/// whose Default home Codex has never configured.
fn two_codex_workspaces() -> FakeHost {
    let host = machine("personal").with_env("HOME", "/Users/someone");
    add_account(&host, "personal");
    let document = credential("work", EMAIL);
    let host = host.with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add_account(&host, "work");
    host
}

const DEFAULT_AUTH: &str = "/Users/someone/.codex/auth.json";

/// The same login Renewed: other tokens, a later `last_refresh`.
fn rotated(workspace: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(&credential(workspace, EMAIL)).unwrap();
    value["tokens"]["access_token"] = json!("rotated");
    value["last_refresh"] = json!("2026-09-16T12:00:00Z");
    value.to_string()
}

fn codex_active(host: &FakeHost) -> Option<String> {
    let mut registry = registry::load(host).unwrap().unwrap();
    registry.select_provider(Id::Codex);
    registry.active().whose().map(str::to_string)
}

#[test]
fn switching_to_a_codex_account_writes_its_credential_into_the_default_home() {
    let host = two_codex_workspaces();

    let (result, printed) = common::run_switch(&host, "work");

    result.expect("a Codex Account is switched to");
    assert_eq!(
        host.file(DEFAULT_AUTH).as_deref(),
        Some(credential("work", EMAIL).as_str())
    );
    assert!(
        host.file("/Users/someone/.codex/config.toml")
            .is_some_and(|config| config.contains("cli_auth_credentials_store = \"file\"")),
        "a home Codex never configured is pinned to the file store"
    );
    assert!(
        printed.contains("Switched to ") && printed.contains("(as `work`)."),
        "{printed}"
    );
    assert!(
        printed.contains("Note: a Codex already open keeps its Account until it is restarted."),
        "{printed}"
    );
    let registry = registry::load(&host).unwrap().unwrap();
    let work = registry
        .accounts
        .iter()
        .find(|a| registry.alias_of(a.key()) == Some("work"))
        .unwrap();
    assert_eq!(codex_active(&host).as_deref(), Some(work.key()));
}

#[test]
fn a_codex_switch_captures_the_renewed_live_credential_into_the_outgoing_profile() {
    let host = two_codex_workspaces();
    common::run_switch(&host, "personal").0.unwrap();
    host.set_file(DEFAULT_AUTH, &rotated("personal"));

    common::run_switch(&host, "work")
        .0
        .expect("the Switch lands");

    let registry = registry::load(&host).unwrap().unwrap();
    let personal = registry
        .accounts
        .iter()
        .find(|a| registry.alias_of(a.key()) == Some("personal"))
        .unwrap();
    assert_eq!(
        host.file(personal.profile_dir(&host).unwrap().join("auth.json"))
            .as_deref(),
        Some(rotated("personal").as_str()),
        "the Renewed copy went home"
    );
    assert_eq!(
        host.file(DEFAULT_AUTH).as_deref(),
        Some(credential("work", EMAIL).as_str())
    );
}

#[test]
fn a_codex_default_kept_in_the_keyring_is_refused_and_the_pin_is_named() {
    let host = two_codex_workspaces().with_file(
        "/Users/someone/.codex/config.toml",
        "cli_auth_credentials_store = \"keyring\"\n",
    );

    let (result, _) = common::run_switch(&host, "work");

    let refused = result.unwrap_err().to_string();
    assert!(
        refused.contains("cli_auth_credentials_store = \"file\""),
        "{refused}"
    );
    assert!(refused.contains("config.toml"), "{refused}");
    assert!(host.file(DEFAULT_AUTH).is_none(), "nothing was written");
}

#[test]
fn a_codex_landing_left_in_flight_is_settled_by_the_identity_of_the_live_credential() {
    let host = two_codex_workspaces();
    common::run_switch(&host, "personal").0.unwrap();
    let mut registry = registry::load(&host).unwrap().unwrap();
    registry.select_provider(Id::Codex);
    let key_of = |alias: &str| {
        registry
            .accounts
            .iter()
            .find(|a| registry.alias_of(a.key()) == Some(alias))
            .unwrap()
            .key()
            .to_string()
    };
    let (personal, work) = (key_of("personal"), key_of("work"));
    registry.begin_landing(Some(personal), &work);
    common::save_registry(&host, &registry);
    // Renewed since the Credential moved: no held copy is byte-equal.
    host.set_file(DEFAULT_AUTH, &rotated("work"));

    let mut perch = perch::holdings::lock(&host).unwrap();
    let mut registry = registry::load(&host).unwrap().unwrap();
    registry.select_provider(Id::Codex);
    perch::commands::a_settled_landing(&host, &mut perch, &mut registry).expect("settled");

    assert_eq!(registry.active().whose(), Some(work.as_str()));
}

#[test]
fn the_active_codex_account_is_observed_where_its_login_is_live() {
    let host = with_codex_limits(two_codex_workspaces());
    common::run_switch(&host, "personal").0.unwrap();
    let registry = registry::load(&host).unwrap().unwrap();
    let personal = registry
        .accounts
        .iter()
        .find(|a| registry.alias_of(a.key()) == Some("personal"))
        .unwrap();
    // The Profile copy is gone; only the live one can answer.
    host.remove_file(&personal.profile_dir(&host).unwrap().join("auth.json"))
        .unwrap();

    let observed = personal
        .provider()
        .adapter()
        .configured(&host)
        .unwrap()
        .observe(
            &host,
            &mut perch::holdings::lock(&host).unwrap(),
            &registry.profile_context(&host, personal).unwrap(),
            &personal.profile(&host).unwrap(),
            &mut || Ok(()),
        );

    let windows = observed.expect("read off the Default home");
    assert_eq!(windows.len(), 1);
}

#[test]
fn an_email_a_claude_and_a_codex_account_share_resolves_under_either_provider_flag() {
    let host = common::logged_in_machine();
    let document = credential("company", common::EMAIL);
    let host = host.with_file(CODEX, "").with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add_account(&host, "company");
    let registry = registry::load(&host).unwrap().unwrap();
    assert_eq!(registry.accounts.len(), 2);

    let claude = target::resolve_for(&registry, common::EMAIL, Some(Id::Claude))
        .expect("`--claude` names the Claude Account alone");
    assert_eq!(registry.held(&claude.email).unwrap().provider(), Id::Claude);
    let codex = target::resolve_for(&registry, common::EMAIL, Some(Id::Codex))
        .expect("`--codex` names the Codex Account alone");
    assert_eq!(registry.held(&codex.email).unwrap().provider(), Id::Codex);
    assert!(
        target::resolve_for(&registry, common::EMAIL, None)
            .unwrap_err()
            .to_string()
            .contains("names more than one Account"),
        "with no provider named, the email is ambiguous"
    );
}

#[test]
fn a_provider_flag_refuses_an_account_of_the_other_provider_by_email_as_by_alias() {
    let host = machine("personal");
    add_account(&host, "personal");
    let host = host.with_file("/usr/bin/claude", "");
    let claude = Selection {
        provider: None,
        codex: false,
        claude: true,
    };
    for target in [EMAIL, "personal"] {
        let mut printed = Vec::new();
        let switched = perch::commands::switch::run(
            &host,
            perch::commands::switch::SwitchArgs {
                provider: claude,
                target: Some(target.into()),
                no_refresh: true,
            },
            &mut printed,
        );
        assert!(
            switched
                .unwrap_err()
                .to_string()
                .contains("is a Codex Account. `--codex` selects it."),
            "`perch switch --claude {target}` refuses rather than switching Codex"
        );
        assert!(
            printed.is_empty(),
            "{}",
            String::from_utf8(printed).unwrap()
        );
        let ran = run::run(
            &host,
            run::RunArgs {
                provider: claude,
                target: target.into(),
                command: vec![],
            },
            &mut Vec::new(),
        );
        assert!(
            ran.unwrap_err()
                .to_string()
                .contains("is a Codex Account. `--codex` selects it."),
            "`perch run --claude {target}` says the same sentence"
        );
    }
}

#[test]
fn status_and_list_speak_for_the_one_provider_whose_accounts_are_held() {
    let host = two_codex_workspaces();
    common::run_switch(&host, "work").0.unwrap();

    let (status, said) = common::run_status(&host, false);
    status.expect("the Codex Account just switched to is the one you are on");
    assert!(said.contains(EMAIL), "{said}");

    let (list, listed) = common::run_list(&host, true);
    list.unwrap();
    let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
    assert!(
        listed["active_account"]
            .as_str()
            .is_some_and(|key| key.starts_with("codex:")),
        "{listed}"
    );
}

#[test]
fn status_speaks_for_the_run_preference_where_both_providers_are_held_unless_a_flag_names_one() {
    let host = common::logged_in_machine();
    let document = credential("company", EMAIL);
    let host = host.with_file(CODEX, "").with_login(move |host, at| {
        host.set_file(at.join("auth.json"), &document);
        0
    });
    add_account(&host, "company");
    let codex = Selection {
        provider: None,
        codex: true,
        claude: false,
    };
    perch::commands::switch::run(
        &host,
        perch::commands::switch::SwitchArgs {
            provider: codex,
            target: Some("company".into()),
            no_refresh: true,
        },
        &mut Vec::new(),
    )
    .unwrap();

    let (unnamed, said) = common::run_status(&host, false);
    unnamed.unwrap();
    assert!(
        said.contains(common::EMAIL) && !said.contains(EMAIL),
        "the Run preference is Claude: {said}"
    );
    let (named, said) = common::run_status_with(
        &host,
        perch::commands::status::StatusArgs {
            provider: codex,
            ..Default::default()
        },
    );
    named.unwrap();
    assert!(
        said.contains(EMAIL) && !said.contains(common::EMAIL),
        "{said}"
    );
}
