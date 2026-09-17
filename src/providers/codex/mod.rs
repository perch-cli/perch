//! Codex implementation of the shared provider contract.

mod auth;
mod defaults;
mod diagnostics;
mod layout;
mod process;
mod profiles;
mod usage;

use crate::providers::provider::Id;
use crate::providers::provider::ProfileRef as Account;
use crate::{Host, PerchError, Result, registry};
use auth::identity;
use auth::login;
use process::prepare_launch;
use profiles::{Restore, refuse_live};
use profiles::{credential, write_credential};
use usage::read_limits;
use zeroize::Zeroizing;

const CONFIG: &str = "cli_auth_credentials_store = \"file\"\nforced_login_method = \"chatgpt\"\n";
/// The one file Codex's file store is: Credential and identity in one document.
pub(super) const AUTH_FILE: &str = "auth.json";
fn refused(what: &str) -> PerchError {
    PerchError::Invalid(format!("Codex {what}"))
}

pub struct Codex;

impl super::provider::Adapter for Codex {
    fn validate_identity(&self, identity: &super::provider::AccountIdentity) -> Result<()> {
        if identity.workspace_id.is_none() {
            return Err(crate::PerchError::Invalid(
                "Codex requires an authenticated Workspace identity".into(),
            ));
        }
        Ok(())
    }

    fn diagnostic_session(
        &self,
        installation: &super::provider::Installation,
        request: &super::provider::DiagnosticSession<'_>,
    ) -> Result<super::provider::PreparedLaunch<'static>> {
        super::diagnostics::session(installation, request)
    }
    fn diagnose(
        &self,
        host: &dyn Host,
        installation: Result<super::provider::Installation>,
    ) -> super::provider::DiagnosticReport {
        diagnostics::gather(host, installation)
    }

    fn session_evidence(
        &self,
        host: &dyn Host,
        directory: &std::path::Path,
    ) -> std::result::Result<Vec<super::provider::SessionEvidence>, crate::live::Unsure> {
        super::sessions::read(host, directory, true)
    }

    fn check_replacement(
        &self,
        host: &dyn Host,
        account: &Account,
        default_reason: Option<&'static str>,
        consequence: &crate::live::Consequence,
    ) -> Result<()> {
        let mut places = vec![crate::live::Place::new(
            Id::Codex,
            format!("{}'s Profile", account.key()),
            account.directory(),
        )];
        if let Some(reason) = default_reason {
            places.push(crate::live::Place::new(
                Id::Codex,
                reason,
                layout::default_home(host)?,
            ));
        }
        crate::live::ask(host, &places).idle_or(consequence)?;
        Ok(())
    }

    fn inspect_default<'a>(
        &self,
        host: &'a dyn Host,
    ) -> Result<Box<dyn super::provider::DefaultInspection + 'a>> {
        defaults::inspect(host)
    }
    fn default_matches(&self, host: &dyn Host, profile: &Account) -> Result<bool> {
        defaults::already_landed(host, profile)
    }
    fn prepare_default<'a>(
        &self,
        host: &'a dyn Host,
        _held: &mut crate::lock::Held<'_>,
        request: super::provider::DefaultRequest,
    ) -> Result<Box<dyn super::provider::DefaultChange + 'a>> {
        defaults::begin(host, request)
    }
    fn switched_note(&self) -> Option<&'static str> {
        Some("Note: a Codex already open keeps its Account until it is restarted.")
    }

    fn authenticate(
        &self,
        host: &dyn Host,
        installation: &super::provider::Installation,
    ) -> Result<super::provider::Authenticated> {
        let credential = login(host, installation.executable())?;
        let (subject, identity, plan) = identity(&credential)?;
        Ok(super::provider::Authenticated {
            provider: Id::Codex,
            identity,
            subject: Some(subject),
            plan,
            credential,
            configuration: Some(Zeroizing::new(CONFIG.to_string())),
        })
    }
    fn install<'a>(
        &self,
        host: &'a dyn Host,
        account: &Account,
        authenticated: &super::provider::Authenticated,
        mode: super::provider::InstallMode,
    ) -> Result<super::provider::AppliedProfile<'a>> {
        use super::provider::{AppliedProfile, InstallMode};
        let home = account.directory().to_path_buf();
        refuse_live(host, &home)?;
        if mode == InstallMode::New {
            host.create_private_dir_all(home.parent().unwrap())
                .map_err(|e| PerchError::file_write(&home, e))?;
            host.create_dir_exclusive(&home)
                .map_err(|e| PerchError::file_write(&home, e))?;
            let rollback_home = home.clone();
            let applied = AppliedProfile::reversible(move || {
                host.remove_dir_all(&rollback_home)
                    .map_err(|error| PerchError::file_write(&rollback_home, error))
            });
            let written = (|| {
                host.make_private(&home)
                    .map_err(|e| PerchError::file_write(&home, e))?;
                write_credential(host, account, &authenticated.credential)
            })();
            if let Err(error) = written {
                return Err(match applied.rollback() {
                    Ok(()) => error,
                    Err(cleanup) => error.with_note(&format!("Rollback incomplete: {cleanup}")),
                });
            }
            return Ok(applied);
        }
        write_credential(host, account, &authenticated.credential)?;
        Ok(AppliedProfile::retained())
    }

    fn forget_profile_credential(
        &self,
        host: &dyn Host,
        profile: &std::path::Path,
    ) -> Result<super::provider::CredentialRemoval> {
        let path = profile.join(AUTH_FILE);
        if !host.path_exists(&path) {
            return Ok(super::provider::CredentialRemoval::default());
        }
        host.remove_file(&path)
            .map_err(|error| PerchError::file_write(&path, error))?;
        Ok(super::provider::CredentialRemoval {
            removed: true,
            note: None,
        })
    }

    fn snapshot(
        &self,
        host: &dyn Host,
        context: &super::provider::ProfileContext,
    ) -> Result<super::provider::ProfileBundle> {
        use super::provider::{ArtifactPurpose, ProfileBundle};
        let account = &context.profile;
        let mut bundle = ProfileBundle::default();
        if let Some(credential) = credential(host, account)? {
            bundle.insert(
                AUTH_FILE,
                ArtifactPurpose::Credential,
                credential.to_string(),
            );
        }
        let path = account.directory().join("config.toml");
        match host.read_file(&path) {
            Ok(config) => bundle.insert("config.toml", ArtifactPurpose::Configuration, config),
            Err(crate::host::HostError::NotFound { .. }) => {}
            Err(_) => {
                return Err(refused(
                    "Configuration could not be read; no partial Export was written",
                ));
            }
        }
        Ok(bundle)
    }
    fn prepare_restore<'a>(
        &self,
        host: &'a dyn Host,
        request: crate::providers::provider::RestoreRequest<'a>,
    ) -> Result<Box<dyn super::provider::Restore + 'a>> {
        Ok(Box::new(Restore::prepare(host, request)?))
    }

    fn observe(
        &self,
        host: &dyn Host,
        held: &mut crate::lock::Held<'_>,
        request: super::provider::Observation<'_>,
        still_ours: crate::lock::StillOurs<'_>,
    ) -> std::result::Result<Vec<registry::WindowUtilization>, crate::observe::Outcome> {
        read_limits(host, held, request, still_ours)
    }
    fn id(&self) -> Id {
        Id::Codex
    }
    fn name(&self) -> &'static str {
        "Codex"
    }
    fn service_environment(&self) -> &'static [&'static str] {
        &["CODEX_HOME"]
    }

    fn service_probe_args(&self) -> &'static [&'static str] {
        &["--version"]
    }

    fn executable_name(&self) -> &'static str {
        "codex"
    }
    fn capabilities(&self) -> super::provider::Capabilities {
        super::provider::Capabilities {
            live_switch: true,
            shared_state: false,
        }
    }
    fn prepare_launch<'a>(
        &self,
        host: &'a dyn Host,
        request: &super::provider::LaunchRequest<'_>,
    ) -> Result<super::provider::PreparedLaunch<'a>> {
        prepare_launch(host, request)
    }
}
