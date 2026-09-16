//! Claude Code implementation of the shared provider interface.

mod auth;
mod backup;
mod carry;
mod credentials;
mod defaults;
mod diagnostics;
mod identity;
mod layout;
mod observe;
mod probe;
mod process;
mod profile;
mod reconcile;
mod service;

use super::provider::{Adapter, Capabilities, Id};
use crate::{Host, Result};

pub struct Claude;

impl Adapter for Claude {
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
        super::sessions::read(host, directory, false)
    }

    fn default_workload(&self) -> Option<&'static str> {
        Some("fable")
    }
    fn configured_workload(
        &self,
        options: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<&'static str> {
        (options
            .get("preferred_workload")
            .and_then(serde_json::Value::as_str)
            == Some("fable"))
        .then_some("fable")
    }
    fn window_role(
        &self,
        workload: &str,
        window: &crate::domain::WindowUtilization,
    ) -> super::provider::WindowRole {
        use super::provider::WindowRole;
        if workload == "fable" && window.window == "7-day-fable" {
            WindowRole::Ranking
        } else if window.window.starts_with("7-day-") {
            WindowRole::Other
        } else {
            WindowRole::Constraint
        }
    }

    fn validate_options(
        &self,
        options: &std::collections::BTreeMap<String, serde_json::Value>,
        scope: super::provider::OptionScope,
    ) -> Result<()> {
        for (key, value) in options {
            if scope != super::provider::OptionScope::Policy
                || key != "preferred_workload"
                || value.as_str() != Some("fable")
            {
                return Err(crate::PerchError::Invalid(format!(
                    "Claude does not support `{key}` with value {value}; its policy option is preferred_workload=fable"
                )));
            }
        }
        Ok(())
    }

    fn inspect_default<'a>(
        &self,
        host: &'a dyn Host,
    ) -> Result<Box<dyn super::provider::DefaultInspection + 'a>> {
        defaults::inspect(host)
    }
    fn default_matches(
        &self,
        host: &dyn Host,
        profile: &super::provider::ProfileRef,
    ) -> Result<bool> {
        defaults::already_landed(
            host,
            &crate::providers::claude::probe::Installed::for_every_round(host),
            profile,
        )
    }

    fn prepare_default<'a>(
        &self,
        host: &'a dyn Host,
        held: &mut crate::lock::Held<'_>,
        request: super::provider::DefaultRequest,
    ) -> Result<Box<dyn super::provider::DefaultChange + 'a>> {
        defaults::begin(
            host,
            held,
            request,
            &crate::providers::claude::probe::Installed::for_every_round(host),
        )
    }

    fn maintain(&self, host: &dyn Host) {
        auth::reap_abandoned(host);
    }
    fn discover(
        &self,
        host: &dyn Host,
        installation: &super::provider::Installation,
    ) -> Result<Option<super::provider::Discovered>> {
        auth::discover(host, installation.executable())
    }
    fn check_replacement(
        &self,
        host: &dyn Host,
        account: &crate::providers::provider::ProfileRef,
        default_reason: Option<&'static str>,
    ) -> Result<()> {
        let mut places = vec![crate::live::Place::new(
            crate::providers::provider::Id::Claude,
            format!("{}'s Profile", account.key()),
            account.directory(),
        )];
        if let Some(reason) = default_reason {
            places.push(crate::live::Place::new(
                crate::providers::provider::Id::Claude,
                reason,
                crate::providers::claude::layout::default_profile(host)?.config_dir,
            ));
        }
        crate::live::ask(host, &places).idle_or(&crate::live::NOTHING_WAS_CHANGED)?;
        Ok(())
    }

    fn authenticate(
        &self,
        host: &dyn Host,
        installation: &super::provider::Installation,
    ) -> Result<super::provider::Authenticated> {
        let produced = auth::authenticate(host, installation.executable())?;
        Ok(super::provider::Authenticated {
            provider: Id::Claude,
            subject: Some(identity::subject(&produced.identity)?),
            identity: produced.identity,
            plan: produced.credential.subscription_type.clone(),
            credential: zeroize::Zeroizing::new(produced.credential.as_str().to_string()),
            configuration: Some(produced.identity_json),
        })
    }
    fn install<'a>(
        &self,
        host: &'a dyn Host,
        account: &crate::providers::provider::ProfileRef,
        authenticated: &super::provider::Authenticated,
        mode: super::provider::InstallMode,
    ) -> Result<super::provider::AppliedProfile<'a>> {
        use super::provider::{AppliedProfile, InstallMode};
        let placed = profile::place(
            host,
            &account.profile_dir(host)?,
            Some(&authenticated.credential),
            authenticated.configuration.as_deref().map(String::as_str),
            if mode == InstallMode::New {
                profile::IfItFails::TakeBack
            } else {
                profile::IfItFails::KeepWhatLanded
            },
        )?;
        Ok(if mode == InstallMode::New {
            AppliedProfile::reversible(move || placed.take_back(host))
        } else {
            AppliedProfile::retained()
        })
    }

    fn forget_profile_credential(
        &self,
        host: &dyn Host,
        profile: &std::path::Path,
    ) -> Result<super::provider::CredentialRemoval> {
        let store = crate::providers::claude::probe::store_for_profile(host, profile)?;
        let mut existed = false;
        for store in crate::providers::claude::credentials::stores_for(host, &store) {
            existed |=
                store.forget(host)? == crate::providers::claude::credentials::Forgotten::Credential;
        }
        Ok(super::provider::CredentialRemoval {
            removed: existed,
            note: (!existed).then(|| match host.platform() {
                crate::host::Platform::MacOs =>
                    "Claude's keychain items are filed under $USER; an item written under a different login name can remain.".to_string(),
                _ => "Claude's Credential Store is a file inside its Profile; no credential file was present.".to_string(),
            }),
        })
    }

    fn snapshot(
        &self,
        host: &dyn Host,
        context: &super::provider::ProfileContext,
    ) -> Result<super::provider::ProfileBundle> {
        use super::provider::{ArtifactPurpose, ProfileBundle};
        let mut bundle = ProfileBundle::default();
        if let Some(credential) = backup::credential(
            host,
            context,
            &context.profile,
            &crate::providers::claude::probe::Installed::for_every_round(host),
        )? {
            bundle.insert("oauth", ArtifactPurpose::Credential, credential);
        }
        if let Some(config) = backup::config(host, &context.profile)? {
            bundle.insert(".claude.json", ArtifactPurpose::Configuration, config);
        }
        Ok(bundle)
    }
    fn prepare_restore<'a>(
        &self,
        host: &'a dyn Host,
        request: crate::providers::provider::RestoreRequest<'a>,
    ) -> Result<Box<dyn super::provider::Restore + 'a>> {
        Ok(Box::new(backup::Restore::prepare(host, request)?))
    }

    fn observe(
        &self,
        host: &dyn Host,
        held: &mut crate::lock::Held<'_>,
        request: super::provider::Observation<'_>,
        still_ours: crate::lock::StillOurs<'_>,
    ) -> std::result::Result<Vec<crate::domain::WindowUtilization>, crate::observe::Outcome> {
        observe::observe(
            host,
            held,
            request.context,
            request.profile,
            &crate::providers::claude::probe::Installed::from_installation(
                host,
                request.configured.installation(host),
            ),
            still_ours,
        )
    }
    fn id(&self) -> Id {
        Id::Claude
    }
    fn name(&self) -> &'static str {
        "Claude Code"
    }
    fn service_environment(&self) -> &'static [&'static str] {
        &["CLAUDE_CONFIG_DIR"]
    }

    fn service_probe_args(&self) -> &'static [&'static str] {
        &["--version"]
    }

    fn executable_name(&self) -> &'static str {
        "claude"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            live_switch: true,
            shared_state: true,
        }
    }
    fn prepare_launch<'a>(
        &self,
        host: &'a dyn Host,
        request: &super::provider::LaunchRequest<'_>,
    ) -> Result<super::provider::PreparedLaunch<'a>> {
        process::prepare(host, request)
    }
}

impl super::provider::ProfileRef {
    pub(in crate::providers::claude) fn store(
        &self,
        host: &dyn Host,
    ) -> Result<crate::providers::claude::probe::Store> {
        crate::providers::claude::probe::store_for_profile(host, self.directory())
    }
}
