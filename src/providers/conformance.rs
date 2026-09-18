//! A third adapter exercises shared workflows without native Claude or Codex files.

use super::*;
use crate::host::FakeHost;
use crate::host::prelude::*;
use std::cell::Cell;

thread_local! {
    static CATALOG: Cell<Option<&'static [Provider]>> = const { Cell::new(None) };
}

pub(super) fn catalog_override() -> Option<&'static [Provider]> {
    CATALOG.get()
}

fn with_fixture(test: impl FnOnce()) {
    static PROVIDERS: &[Provider] = &[
        Provider {
            adapter: &super::super::claude::Claude,
        },
        Provider {
            adapter: &super::super::codex::Codex,
        },
        Provider { adapter: &Fixture },
    ];
    struct Reset(Option<&'static [Provider]>);
    impl Drop for Reset {
        fn drop(&mut self) {
            CATALOG.set(self.0);
        }
    }
    let _reset = Reset(CATALOG.replace(Some(PROVIDERS)));
    test();
}

struct Fixture;
impl Adapter for Fixture {
    fn id(&self) -> Id {
        Id::Fixture
    }
    fn name(&self) -> &'static str {
        "Fixture"
    }
    fn executable_name(&self) -> &'static str {
        "fixture"
    }
    fn discard_login(&self, host: &dyn Host, dir: &std::path::Path) {
        let _ = host.remove_dir_all(dir);
    }
    fn service_environment(&self) -> &'static [&'static str] {
        &[]
    }
    fn service_probe_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            live_switch: false,
            shared_state: false,
        }
    }
    fn diagnostic_session(
        &self,
        installation: &Installation,
        request: &DiagnosticSession<'_>,
    ) -> Result<PreparedLaunch<'static>> {
        super::super::diagnostics::session(installation, request)
    }
    fn diagnose(&self, _host: &dyn Host, installation: Result<Installation>) -> DiagnosticReport {
        DiagnosticReport {
            path: installation.ok().map(|installed| installed.executable),
            version: Ok("fixture-1".into()),
            assumptions: vec![],
            findings: vec![],
        }
    }
    fn session_evidence(
        &self,
        _host: &dyn Host,
        _directory: &std::path::Path,
    ) -> std::result::Result<Vec<SessionEvidence>, crate::live::Unsure> {
        Ok(vec![])
    }
    fn check_replacement(
        &self,
        _host: &dyn Host,
        _profile: &ProfileRef,
        _reason: Option<&'static str>,
        _consequence: &crate::live::Consequence,
    ) -> Result<()> {
        Ok(())
    }
    fn authenticate(&self, host: &dyn Host, _installation: &Installation) -> Result<Authenticated> {
        Ok(Authenticated {
            provider: Id::Fixture,
            identity: crate::domain::Identity {
                email: "fixture@example.com".into(),
                account_uuid: Some("fixture-user".into()),
                organization_uuid: None,
                organization_name: None,
            },
            subject: Some(AccountIdentity::from_subject(
                Id::Fixture,
                "fixture-user".into(),
                None,
            )?),
            plan: host.env_var("FIXTURE_PLAN"),
            credential: zeroize::Zeroizing::new(
                host.env_var("FIXTURE_CREDENTIAL")
                    .unwrap_or_else(|| "fixture credential".into()),
            ),
            configuration: None,
        })
    }
    fn install<'a>(
        &self,
        host: &'a dyn Host,
        profile: &ProfileRef,
        authenticated: &Authenticated,
        mode: InstallMode,
    ) -> Result<AppliedProfile<'a>> {
        let home = profile.directory.clone();
        let applied = if mode == InstallMode::New {
            host.create_private_dir_all(home.parent().unwrap())
                .map_err(|error| PerchError::file_write(&home, error))?;
            host.create_dir_exclusive(&home)
                .map_err(|error| PerchError::file_write(&home, error))?;
            let rollback = home.clone();
            AppliedProfile::reversible(move || {
                host.remove_dir_all(&rollback)
                    .map_err(|error| PerchError::file_write(&rollback, error))
            })
        } else {
            AppliedProfile::retained()
        };
        let path = home.join("fixture.auth");
        host.write_private_file(&path, &authenticated.credential)
            .map_err(|error| PerchError::file_write(&path, error))?;
        Ok(applied)
    }
    fn forget_profile_credential(
        &self,
        host: &dyn Host,
        home: &std::path::Path,
    ) -> Result<CredentialRemoval> {
        let path = home.join("fixture.auth");
        host.remove_file(&path)
            .map_err(|error| PerchError::file_write(&path, error))?;
        Ok(CredentialRemoval {
            removed: true,
            note: None,
        })
    }
    fn snapshot(&self, host: &dyn Host, context: &ProfileContext) -> Result<ProfileBundle> {
        let path = context.profile.directory.join("fixture.auth");
        let content = host
            .read_file(&path)
            .map_err(|error| PerchError::Other(error.to_string()))?;
        let mut bundle = ProfileBundle::default();
        bundle.insert("fixture.auth", ArtifactPurpose::Credential, content);
        Ok(bundle)
    }
    fn prepare_restore<'a>(
        &self,
        host: &'a dyn Host,
        request: RestoreRequest<'a>,
    ) -> Result<Box<dyn Restore + 'a>> {
        if let Some(bundle) = request.bundle {
            bundle.expect(&[("fixture.auth", ArtifactPurpose::Credential)])?;
        }
        let home = request.profile.directory;
        if host.path_exists(&home) {
            return Err(PerchError::Conflict(
                "Fixture Profile already exists".into(),
            ));
        }
        Ok(Box::new(FixtureRestore {
            host,
            home,
            content: request.bundle.and_then(|bundle| bundle.get("fixture.auth")),
            created: false,
            committed: false,
        }))
    }
    fn observe(
        &self,
        _host: &dyn Host,
        _held: &mut crate::lock::Held<'_>,
        _request: Observation<'_>,
        still_ours: crate::lock::StillOurs<'_>,
    ) -> std::result::Result<Vec<crate::domain::WindowUtilization>, crate::observe::Outcome> {
        still_ours().map_err(crate::observe::Outcome::Stopped)?;
        Ok(vec![crate::domain::WindowUtilization {
            group: None,
            window: "fixture-quota".into(),
            used_percent: 17.0,
            resets_at: None,
        }])
    }
    fn prepare_launch<'a>(
        &self,
        _host: &'a dyn Host,
        request: &LaunchRequest<'_>,
    ) -> Result<PreparedLaunch<'a>> {
        assert!(request.shared_profiles.is_empty());
        let program = match request.kind {
            LaunchKind::Client(installation) => {
                installation.executable().to_string_lossy().into_owned()
            }
            LaunchKind::Custom(program) => program.into(),
        };
        Ok(PreparedLaunch {
            program,
            arguments: request.arguments.to_vec(),
            environment: LaunchEnvironment::Overlay(vec![]),
            _claim: None,
        })
    }
}

struct FixtureRestore<'a> {
    host: &'a dyn Host,
    home: PathBuf,
    content: Option<&'a str>,
    created: bool,
    committed: bool,
}
impl Restore for FixtureRestore<'_> {
    fn write(&mut self) -> Result<()> {
        self.host
            .create_private_dir_all(self.home.parent().unwrap())
            .map_err(|error| PerchError::file_write(&self.home, error))?;
        self.host
            .create_dir_exclusive(&self.home)
            .map_err(|error| PerchError::file_write(&self.home, error))?;
        self.created = true;
        if let Some(content) = self.content {
            let path = self.home.join("fixture.auth");
            self.host
                .write_private_file(&path, content)
                .map_err(|error| PerchError::file_write(&path, error))?;
        }
        Ok(())
    }
    fn commit(&mut self) {
        self.committed = true;
    }
    fn rollback(&mut self) -> Result<()> {
        if !self.created || self.committed {
            return Ok(());
        }
        self.created = false;
        self.host
            .remove_dir_all(&self.home)
            .map_err(|error| PerchError::file_write(&self.home, error))
    }
}
impl Drop for FixtureRestore<'_> {
    fn drop(&mut self) {
        if self.created && !self.committed {
            let _ = self.host.remove_dir_all(&self.home);
        }
    }
}

fn fixture_export() -> crate::export::Export {
    let mut export = crate::export::Export {
        version: crate::export::CURRENT_VERSION,
        registry: crate::registry::Registry::default(),
        profiles: Default::default(),
    };
    for user in ["first", "second"] {
        let subject = AccountIdentity::from_subject(Id::Fixture, user.into(), None).unwrap();
        let key = subject.key.clone();
        export.registry.upsert(crate::registry::Account {
            storage_key: None,
            provider: Id::Fixture,
            provider_identity: Some(subject),
            identity: crate::domain::Identity {
                email: format!("{user}@example.com"),
                account_uuid: Some(user.into()),
                organization_uuid: None,
                organization_name: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        let mut bundle = ProfileBundle::default();
        bundle.insert(
            "fixture.auth",
            ArtifactPurpose::Credential,
            format!("{user} credential"),
        );
        export.profiles.insert(key, bundle);
    }
    export
}

#[test]
fn third_provider_restore_commits_only_after_metadata_and_rolls_back_every_profile_on_failure() {
    with_fixture(|| {
        let export = fixture_export();
        for failed in [false, true] {
            let host = FakeHost::new();
            let profiles: Vec<_> = export
                .registry
                .accounts
                .iter()
                .map(|account| account.profile_dir(&host).unwrap())
                .collect();
            let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();
            let mut saved = false;
            let result = crate::import::place(&host, &export, &fresh, || {
                assert!(
                    profiles
                        .iter()
                        .all(|path| host.is_file(&path.join("fixture.auth")))
                );
                if failed {
                    return Err(PerchError::Other("fixture metadata failure".into()));
                }
                let mut held = crate::holdings::lock(&host)?;
                let mut registry = export.registry.clone();
                crate::registry::save(&host, &mut held, &mut registry)?;
                saved = true;
                Ok(())
            });
            assert_eq!(result.is_ok(), !failed);
            assert_eq!(saved, !failed);
            assert!(profiles.iter().all(|path| host.path_exists(path) != failed));
            if failed {
                assert!(crate::registry::load(&host).unwrap().is_none());
            } else {
                let restored = crate::registry::load(&host).unwrap().unwrap();
                let snapshot = crate::export::gather(&host, &restored).unwrap();
                assert_eq!(snapshot.profiles, export.profiles);
            }
        }
    });
}

#[test]
fn third_provider_restore_validates_every_bundle_before_any_profile_write() {
    with_fixture(|| {
        let mut export = fixture_export();
        let last = export.registry.accounts.last().unwrap().key().to_string();
        export.profiles.get_mut(&last).unwrap().insert(
            "unknown",
            ArtifactPurpose::Configuration,
            "fixture".into(),
        );
        let host = FakeHost::new();
        let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();
        let result = crate::import::place(&host, &export, &fresh, || {
            panic!("invalid restore saved metadata")
        });
        assert!(result.is_err());
        assert!(host.effects().is_empty(), "{:?}", host.effects());
    });
}

#[test]
fn third_provider_restore_rolls_back_an_earlier_profile_and_the_partial_failing_profile() {
    with_fixture(|| {
        let export = fixture_export();
        let host = FakeHost::new();
        let profiles: Vec<_> = export
            .registry
            .accounts
            .iter()
            .map(|account| account.profile_dir(&host).unwrap())
            .collect();
        let host = host.with_a_path_refusing(
            profiles[1].join("fixture.auth"),
            crate::host::Refusing::Write,
            "fixture write failure",
        );
        let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();
        let result = crate::import::place(&host, &export, &fresh, || {
            panic!("partial restore saved metadata")
        });
        assert!(result.is_err());
        assert!(profiles.iter().all(|path| !host.path_exists(path)));
    });
}

#[test]
fn a_third_provider_uses_shared_commands_configuration_and_observation() {
    with_fixture(|| {
        use crate::commands::{add, config, list, run, selection::Selection};
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/fixture", "")
            .with_login(|_, _| 0);
        assert_eq!(Id::parse("fixture").unwrap(), Id::Fixture);
        add::run(
            &host,
            add::AddArgs {
                provider: Selection {
                    provider: Some(Id::Fixture),
                    ..Default::default()
                },
                group: Some("shared".into()),
                alias: Some("third".into()),
                no_group: false,
            },
            &mut Vec::new(),
        )
        .unwrap();
        config::run(
            &host,
            config::ConfigCommand::Set {
                words: vec!["--global".into(), "run-provider".into(), "fixture".into()],
            },
            &mut Vec::new(),
        )
        .unwrap();
        let mut registry = crate::registry::load(&host).unwrap().unwrap();
        let account = registry.accounts[0].clone();
        assert_eq!(account.provider(), Id::Fixture);
        assert_eq!(registry.run_provider, Id::Fixture);
        let profile_dir = account.profile_dir(&host).unwrap();
        assert!(
            profile_dir
                .components()
                .any(|part| part.as_os_str() == "fixture")
                && profile_dir
                    .components()
                    .any(|part| part.as_os_str() == "providers"),
            "{}",
            profile_dir.display()
        );
        assert_eq!(
            run::run(
                &host,
                run::RunArgs {
                    provider: Selection::default(),
                    target: "third".into(),
                    command: vec!["--version".into()]
                },
                &mut Vec::new()
            )
            .unwrap(),
            0
        );
        let mut output = Vec::new();
        list::run(
            &host,
            list::ListArgs {
                scope: Some("shared".into()),
                refresh: true,
                json: true,
                ..Default::default()
            },
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("fixture-quota"), "{output}");
        assert!(output.contains("17.0"), "{output}");
        let exported = crate::export::gather(&host, &registry).unwrap();
        assert!(
            exported
                .profile_for(account.key())
                .unwrap()
                .has_credentials()
        );
        registry.select_provider(Id::Fixture);
        let settled = crate::registry::nothing_in_flight(&registry).unwrap();
        let error = match crate::round::permitted(&registry, &settled) {
            Err(error) => error,
            Ok(_) => panic!("unsupported Watcher admitted"),
        };
        assert!(
            error
                .to_string()
                .contains("fixture does not support automatic live Switching"),
            "{error}"
        );
    });
    assert_eq!(catalog().len(), 2);
}

#[test]
fn third_provider_repair_keeps_the_fresh_credential_when_metadata_cannot_be_saved() {
    with_fixture(|| {
        use crate::commands::{add, relogin, selection::Selection};
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/fixture", "");
        add::run(
            &host,
            add::AddArgs {
                provider: Selection {
                    provider: Some(Id::Fixture),
                    ..Default::default()
                },
                group: None,
                alias: Some("third".into()),
                no_group: true,
            },
            &mut Vec::new(),
        )
        .unwrap();
        let registry = crate::registry::load(&host).unwrap().unwrap();
        let account = &registry.accounts[0];
        let credential = account.profile_dir(&host).unwrap().join("fixture.auth");
        let manifest = crate::holdings::registry_path(&host).unwrap();
        let original = host.read_file(&manifest).unwrap();
        let host = host
            .with_env("FIXTURE_CREDENTIAL", "renewed fixture credential")
            .with_env("FIXTURE_PLAN", "renewed-plan")
            .with_a_path_refusing(
                &manifest,
                crate::host::Refusing::Write,
                "fixture metadata failure",
            );
        let result = relogin::run(
            &host,
            relogin::ReloginArgs {
                target: "third".into(),
            },
            &mut Vec::new(),
        );
        assert!(result.is_err());
        assert_eq!(
            host.read_file(&credential).unwrap(),
            "renewed fixture credential"
        );
        assert_eq!(host.read_file(&manifest).unwrap(), original);
    });
}

#[test]
fn rollback_reports_every_failed_cleanup_without_claiming_profiles_were_removed() {
    with_fixture(|| {
        let export = fixture_export();
        let mut host = FakeHost::new();
        let profiles: Vec<_> = export
            .registry
            .accounts
            .iter()
            .map(|account| account.profile_dir(&host).unwrap())
            .collect();
        for path in &profiles {
            host = host.with_a_path_refusing(
                path,
                crate::host::Refusing::Delete,
                "fixture cleanup failure",
            );
        }
        let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();
        let error = crate::import::place(&host, &export, &fresh, || {
            Err(PerchError::Conflict("fixture metadata failure".into()))
        })
        .unwrap_err();
        assert_eq!(error.exit_code(), crate::error::EXIT_CONFLICT);
        let said = error.to_string();
        assert!(said.contains("fixture metadata failure"), "{said}");
        assert!(said.contains("Rollback incomplete"), "{said}");
        assert!(
            !said.contains("Profiles have been taken back out"),
            "{said}"
        );
        for path in &profiles {
            assert!(said.contains(&path.to_string_lossy().to_string()), "{said}");
            assert!(host.is_file(&path.join("fixture.auth")));
        }
        assert!(!said.contains("first credential"));
        assert!(!said.contains("second credential"));
    });
}

/// A machine holding the third provider's CLI and nothing native.
fn a_fixture_machine() -> FakeHost {
    FakeHost::new()
        .with_env("PATH", "/usr/bin")
        .with_file("/usr/bin/fixture", "")
        .with_login(|_, _| 0)
}

/// The same machine, holding one Fixture Account.
fn a_machine_holding_one(alias: &str) -> (FakeHost, crate::registry::Account) {
    use crate::commands::{add, selection::Selection};
    let host = a_fixture_machine();
    add::run(
        &host,
        add::AddArgs {
            provider: Selection {
                provider: Some(Id::Fixture),
                ..Default::default()
            },
            group: None,
            alias: Some(alias.into()),
            no_group: true,
        },
        &mut Vec::new(),
    )
    .unwrap();
    let account = crate::registry::load(&host).unwrap().unwrap().accounts[0].clone();
    (host, account)
}

#[test]
fn a_third_provider_answers_the_installation_questions_every_command_asks() {
    with_fixture(|| {
        let host = a_fixture_machine();
        let provider = Id::Fixture.adapter();

        let setup = provider
            .service_setup(&host)
            .unwrap()
            .expect("it is enabled");
        assert!(setup.environment.is_empty());
        assert_eq!(setup.probe_args, ["--version"]);
        assert_eq!(setup.candidates, vec![PathBuf::from("/usr/bin/fixture")]);
        assert_eq!(setup.override_key, "PERCH_FIXTURE_BIN");

        let report = provider.diagnose(&host);
        assert_eq!(report.version.unwrap(), "fixture-1");
        assert_eq!(report.path, Some(PathBuf::from("/usr/bin/fixture")));
        assert!(report.findings.is_empty());

        let installation = provider
            .configured(&host)
            .unwrap()
            .installation(&host)
            .unwrap();
        let session = installation
            .diagnostic_session(&DiagnosticSession {
                model: Some("fixture-mini"),
                prompt: "ping",
            })
            .unwrap();
        assert_eq!(session.program(), "/usr/bin/fixture");
        assert_eq!(session.arguments, ["--model", "fixture-mini", "ping"]);
    });
}

#[test]
fn a_third_provider_with_no_live_default_is_refused_rather_than_switched() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let provider = Id::Fixture.adapter();
        let profile = account.profile(&host).unwrap();

        assert!(!provider.default_matches(&host, &profile).unwrap());
        let Err(said) = provider.inspect_default(&host) else {
            panic!("a provider with no live Default cannot inspect one")
        };
        assert!(
            said.to_string()
                .contains("Fixture does not support inspecting a live Default"),
            "{said}"
        );

        let mut held = crate::holdings::lock(&host).unwrap();
        let refused = provider.prepare_default(
            &host,
            &mut held,
            DefaultRequest {
                incoming: profile,
                outgoing: None,
                known: Vec::new(),
                overwrite: None,
            },
        );
        let Err(said) = refused else {
            panic!("a provider with no live Switching cannot switch")
        };
        assert!(
            said.to_string()
                .contains("Fixture does not support live Switching"),
            "{said}"
        );
    });
}

#[test]
fn a_third_provider_gives_up_its_credential_and_reaps_its_abandoned_logins() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let provider = Id::Fixture.adapter();
        let profile = account.profile(&host).unwrap();
        let credential = profile.directory().join("fixture.auth");
        assert!(host.is_file(&credential));

        let removal = provider.forget_credential(&host, &profile).unwrap();

        assert!(removal.removed);
        assert!(removal.note.is_none());
        assert!(!host.is_file(&credential));

        let abandoned = crate::holdings::pending_login_dir(Id::Fixture, &host, host.now()).unwrap();
        host.set_file(abandoned.join("fixture.auth"), "abandoned credential");
        host.set_now(host.now() + chrono::Duration::hours(2));

        provider.maintain(&host);

        assert!(!host.path_exists(&abandoned));
    });
}

#[test]
fn a_third_provider_with_no_running_client_is_not_live() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let profile = account.profile(&host).unwrap();

        let evidence = Id::Fixture
            .adapter()
            .session_evidence(&host, profile.directory())
            .unwrap_or_else(|_| panic!("a provider with no markers is sure"));

        assert!(evidence.is_empty());
        assert!(
            !crate::live::ask(
                &host,
                &[crate::live::Place::at(Id::Fixture, profile.directory())]
            )
            .counts_as_live()
        );
    });
}

#[test]
fn a_third_provider_launches_a_custom_command_against_its_own_profile() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let profile = account.profile(&host).unwrap();

        let prepared = Id::Fixture
            .adapter()
            .prepare_launch(
                &host,
                &LaunchRequest {
                    kind: LaunchKind::Custom("npm"),
                    account: &profile,
                    arguments: &["test".to_string()],
                    shared_profiles: Vec::new(),
                },
            )
            .unwrap();

        assert_eq!(prepared.program(), "npm");
        assert_eq!(prepared.arguments, ["test"]);
    });
}

#[test]
fn a_third_provider_installation_nobody_commits_takes_its_profile_back_out() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let provider = Id::Fixture.adapter();
        let profile = account.profile(&host).unwrap();
        let authenticated = provider
            .configured(&host)
            .unwrap()
            .installation(&host)
            .unwrap()
            .authenticate(&host)
            .unwrap();
        host.remove_dir_all(profile.directory()).unwrap();

        let Ok(applied) = provider.install(&host, &profile, &authenticated, InstallMode::New)
        else {
            panic!("the Profile is installed")
        };

        assert!(host.path_exists(profile.directory()));
        applied.rollback().unwrap();
        assert!(!host.path_exists(profile.directory()));
    });
}

#[test]
fn a_third_provider_restore_refuses_a_profile_directory_that_is_already_there() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");

        let refused = Id::Fixture.adapter().prepare_restore(
            &host,
            RestoreRequest {
                profile: account.profile(&host).unwrap(),
                bundle: None,
            },
        );

        let Err(said) = refused else {
            panic!("a Profile already there is not one to restore over")
        };
        assert!(
            said.to_string().contains("Fixture Profile already exists"),
            "{said}"
        );
    });
}

#[test]
fn a_third_provider_restore_that_fails_on_the_first_profile_makes_none_of_the_rest() {
    with_fixture(|| {
        let export = fixture_export();
        let host = FakeHost::new();
        let profiles: Vec<_> = export
            .registry
            .accounts
            .iter()
            .map(|account| account.profile_dir(&host).unwrap())
            .collect();
        let host = host.with_a_path_refusing(
            profiles[0].join("fixture.auth"),
            crate::host::Refusing::Write,
            "fixture write failure",
        );
        let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();

        let result = crate::import::place(&host, &export, &fresh, || {
            panic!("a restore that never started saved metadata")
        });

        assert!(result.is_err());
        assert!(profiles.iter().all(|path| !host.path_exists(path)));
    });
}

#[test]
fn a_third_provider_restore_makes_a_profile_for_an_account_carrying_no_credential() {
    with_fixture(|| {
        let mut export = fixture_export();
        let last = export.registry.accounts.last().unwrap().key().to_string();
        export.profiles.remove(&last);
        let host = FakeHost::new();
        let profiles: Vec<_> = export
            .registry
            .accounts
            .iter()
            .map(|account| account.profile_dir(&host).unwrap())
            .collect();
        let (_, _, fresh) = crate::wait::across(&mut (), |_| Ok(()), |_| Ok(())).unwrap();

        crate::import::place(&host, &export, &fresh, || {
            let mut held = crate::holdings::lock(&host)?;
            let mut registry = export.registry.clone();
            crate::registry::save(&host, &mut held, &mut registry)
        })
        .expect("an Account with nothing to place still gets its Profile");

        assert!(host.is_file(&profiles[0].join("fixture.auth")));
        assert!(host.path_exists(&profiles[1]));
        assert!(!host.is_file(&profiles[1].join("fixture.auth")));
    });
}

/// What an adapter hands back from a login, with the identity a test is about.
fn an_authentication(
    subject: Option<AccountIdentity>,
    identity: crate::domain::Identity,
) -> Authenticated {
    Authenticated {
        provider: Id::Fixture,
        identity,
        subject,
        plan: None,
        credential: zeroize::Zeroizing::new("fixture credential".into()),
        configuration: None,
    }
}

#[test]
fn an_authentication_whose_identity_disagrees_with_its_own_key_is_never_installed() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let profile = account.profile(&host).unwrap();
        let mut subject = profile.provider_identity.clone().unwrap();
        subject.key = "fixture:0000000000000000".into();
        let authenticated = an_authentication(Some(subject), profile.identity.clone());

        let refused =
            Id::Fixture
                .adapter()
                .install(&host, &profile, &authenticated, InstallMode::Repair);

        let Err(said) = refused else {
            panic!("an identity that does not match its key names no Account")
        };
        assert!(
            said.to_string()
                .contains("Account identity does not match its storage key"),
            "{said}"
        );
    });
}

#[test]
fn an_authentication_with_no_subject_is_matched_by_every_part_of_the_identity_it_names() {
    with_fixture(|| {
        let (host, account) = a_machine_holding_one("third");
        let mut profile = account.profile(&host).unwrap();
        profile.provider_identity = None;
        let theirs = profile.identity.clone();

        for elsewhere in [
            crate::domain::Identity {
                email: "somebody-else@example.com".into(),
                ..theirs.clone()
            },
            crate::domain::Identity {
                account_uuid: Some("another-user".into()),
                ..theirs.clone()
            },
            crate::domain::Identity {
                organization_uuid: Some("another-workspace".into()),
                ..theirs.clone()
            },
        ] {
            let authenticated = an_authentication(None, elsewhere);

            let refused =
                Id::Fixture
                    .adapter()
                    .install(&host, &profile, &authenticated, InstallMode::Repair);

            let Err(said) = refused else {
                panic!("another Identity is another Account")
            };
            assert!(
                said.to_string()
                    .contains("Authentication belongs to another Account or Workspace"),
                "{said}"
            );
        }
    });
}
