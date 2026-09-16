//! Tool selection and stable provider identity (ADR an-account-has-a-workspace).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use super::diagnostics::{
    Assumption, AssumptionStatus, DiagnosticReport, DiagnosticSession, Finding, diagnostic_code,
};

use crate::{Host, PerchError, Result, host};

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Id {
    #[default]
    Claude,
    Codex,
    #[cfg(test)]
    Fixture,
}

impl Id {
    pub fn word(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            #[cfg(test)]
            Self::Fixture => "fixture",
        }
    }

    pub fn parse(word: &str) -> Result<Self> {
        catalog()
            .iter()
            .map(|provider| provider.id())
            .find(|id| id.word() == word)
            .ok_or_else(|| {
                PerchError::Invalid(format!(
                    "Unknown provider {word}; supported providers: {}",
                    catalog()
                        .iter()
                        .map(|provider| provider.id().word())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }

    pub fn home(self, host: &dyn Host) -> Result<PathBuf> {
        Ok(crate::holdings::perch_home(host)?
            .join("providers")
            .join(self.word()))
    }

    pub fn executable_override(self) -> String {
        format!("PERCH_{}_BIN", self.word().to_ascii_uppercase())
    }

    pub fn executable(self, host: &dyn Host) -> Result<PathBuf> {
        Ok(self
            .adapter()
            .configured(host)?
            .installation(host)?
            .executable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountIdentity {
    pub user_id: String,
    pub workspace_id: Option<String>,
    pub key: String,
}

impl AccountIdentity {
    pub fn new(provider: Id, user_id: String, workspace_id: String) -> Result<Self> {
        Self::from_subject(provider, user_id, Some(workspace_id))
    }

    pub fn from_subject(
        provider: Id,
        user_id: String,
        workspace_id: Option<String>,
    ) -> Result<Self> {
        if user_id.trim().is_empty() || workspace_id.as_ref().is_some_and(|id| id.trim().is_empty())
        {
            return Err(PerchError::Invalid(
                "The provider must identify the user and cannot supply an empty Workspace".into(),
            ));
        }
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update((user_id.len() as u64).to_be_bytes());
        hash.update(user_id.as_bytes());
        if let Some(workspace) = &workspace_id {
            hash.update(workspace.as_bytes());
        }
        let hex: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let key = format!("{}:{hex}", provider.word());
        let identity = Self {
            user_id,
            workspace_id,
            key,
        };
        provider.adapter().adapter.validate_identity(&identity)?;
        Ok(identity)
    }

    pub fn validate(&self, provider: Id) -> Result<()> {
        let expected =
            Self::from_subject(provider, self.user_id.clone(), self.workspace_id.clone())?;
        if self.key != expected.key {
            return Err(PerchError::Invalid(
                "Account identity does not match its storage key".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub live_switch: bool,
    pub shared_state: bool,
}

/// A prepared restore owns rollback until the metadata commit succeeds.
pub trait Restore {
    fn write(&mut self) -> Result<()>;
    fn commit(&mut self);
    fn rollback(&mut self) -> Result<()>;
}

/// Cleanup attempts every owned resource and retains every failure.
#[derive(Default)]
pub struct Cleanup {
    failures: Vec<String>,
}
impl Cleanup {
    pub fn record(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.failures.push(error.to_string());
        }
    }
    pub fn result(self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(PerchError::Other(format!(
                "Rollback incomplete: {}. Keep the original files and resolve cleanup before retrying.",
                self.failures.join("; ")
            )))
        }
    }
}

/// The subset of an Account needed to operate on its native Profile.
#[derive(Clone)]
pub struct ProfileRef {
    pub(crate) id: String,
    pub(crate) provider: Id,
    pub identity: crate::domain::Identity,
    pub provider_identity: Option<AccountIdentity>,
    pub quarantine: Option<crate::domain::Quarantine>,
    pub(crate) directory: PathBuf,
}
impl ProfileRef {
    pub fn key(&self) -> &str {
        &self.id
    }
    pub fn email(&self) -> &str {
        &self.identity.email
    }
    pub fn provider(&self) -> Id {
        self.provider
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }
    pub(crate) fn profile_dir(&self, _host: &dyn Host) -> Result<PathBuf> {
        Ok(self.directory.clone())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DefaultRelation {
    Parked,
    Active,
    Leaving,
    Arriving,
}
pub struct ProfileContext {
    pub profile: ProfileRef,
    pub default: DefaultRelation,
    pub shared_with: Option<String>,
}

pub struct RestoreRequest<'a> {
    pub profile: ProfileRef,
    pub bundle: Option<&'a ProfileBundle>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CredentialRemoval {
    pub removed: bool,
    pub note: Option<String>,
}

/// Provider evidence is corroborated against the Host’s process lifetime.
pub struct SessionEvidence {
    pub pid: u32,
    pub marker: PathBuf,
    /// Milliseconds since the Unix epoch; absent when the record is unreadable.
    pub started_at: Option<i64>,
}

pub(super) trait Adapter: Sync {
    fn validate_identity(&self, _identity: &AccountIdentity) -> Result<()> {
        Ok(())
    }

    fn diagnostic_session(
        &self,
        installation: &Installation,
        request: &DiagnosticSession<'_>,
    ) -> Result<PreparedLaunch<'static>>;

    fn diagnose(&self, host: &dyn Host, installation: Result<Installation>) -> DiagnosticReport;
    fn session_evidence(
        &self,
        host: &dyn Host,
        directory: &std::path::Path,
    ) -> std::result::Result<Vec<SessionEvidence>, crate::live::Unsure>;

    fn default_workload(&self) -> Option<&'static str> {
        None
    }
    fn configured_workload(
        &self,
        _options: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<&'static str> {
        None
    }
    fn window_role(
        &self,
        _workload: &str,
        _window: &crate::domain::WindowUtilization,
    ) -> WindowRole {
        WindowRole::Constraint
    }

    fn validate_options(
        &self,
        options: &std::collections::BTreeMap<String, serde_json::Value>,
        _scope: OptionScope,
    ) -> Result<()> {
        if let Some(key) = options.keys().next() {
            return Err(PerchError::Invalid(format!(
                "{} does not support the option `{key}`",
                self.name()
            )));
        }
        Ok(())
    }

    fn inspect_default<'a>(&self, _host: &'a dyn Host) -> Result<Box<dyn DefaultInspection + 'a>> {
        Err(PerchError::Invalid(format!(
            "{} does not support inspecting a live Default",
            self.name()
        )))
    }
    fn default_matches(&self, _host: &dyn Host, _profile: &ProfileRef) -> Result<bool> {
        Ok(false)
    }

    fn prepare_default<'a>(
        &self,
        _host: &'a dyn Host,
        _held: &mut crate::lock::Held<'_>,
        _request: DefaultRequest,
    ) -> Result<Box<dyn DefaultChange + 'a>> {
        Err(PerchError::Invalid(format!(
            "{} does not support changing its live Default",
            self.name()
        )))
    }

    fn maintain(&self, _host: &dyn Host) {}
    fn discover(
        &self,
        _host: &dyn Host,
        _installation: &Installation,
    ) -> Result<Option<Discovered>> {
        Ok(None)
    }
    fn check_replacement(
        &self,
        host: &dyn Host,
        account: &ProfileRef,
        default_reason: Option<&'static str>,
        consequence: &crate::live::Consequence,
    ) -> Result<()>;

    fn authenticate(&self, host: &dyn Host, installation: &Installation) -> Result<Authenticated>;
    /// What the person has to do for the login to come back, where the client
    /// waits for them; `None` for a login that returns on its own.
    fn login_instruction(&self) -> Option<&'static str> {
        None
    }
    /// What a Switch cannot promise about clients already open, where the
    /// provider has no evidence of them; `None` where a running client is
    /// refused instead.
    fn switched_note(&self) -> Option<&'static str> {
        None
    }
    fn install<'a>(
        &self,
        host: &'a dyn Host,
        account: &ProfileRef,
        authenticated: &Authenticated,
        mode: InstallMode,
    ) -> Result<AppliedProfile<'a>>;

    fn forget_profile_credential(
        &self,
        host: &dyn Host,
        profile: &std::path::Path,
    ) -> Result<CredentialRemoval>;

    fn snapshot(&self, host: &dyn Host, context: &ProfileContext) -> Result<ProfileBundle>;
    fn prepare_restore<'a>(
        &self,
        host: &'a dyn Host,
        request: RestoreRequest<'a>,
    ) -> Result<Box<dyn Restore + 'a>>;

    fn observe(
        &self,
        host: &dyn Host,
        held: &mut crate::lock::Held<'_>,
        request: Observation<'_>,
        still_ours: crate::lock::StillOurs<'_>,
    ) -> std::result::Result<Vec<crate::domain::WindowUtilization>, crate::observe::Outcome>;

    fn id(&self) -> Id;
    fn name(&self) -> &'static str;
    fn executable_name(&self) -> &'static str;
    fn service_environment(&self) -> &'static [&'static str];
    fn service_probe_args(&self) -> &'static [&'static str];
    fn capabilities(&self) -> Capabilities;
    fn prepare_launch<'a>(
        &self,
        host: &'a dyn Host,
        request: &LaunchRequest<'_>,
    ) -> Result<PreparedLaunch<'a>>;
}

/// Explicit paths pass through unchanged; discovered candidates need a service rehearsal.
pub struct ServiceSetup {
    pub override_key: String,
    pub explicit_path: Option<PathBuf>,
    pub candidates: Vec<PathBuf>,
    pub environment: Vec<(String, String)>,
    pub probe_args: &'static [&'static str],
}

pub(super) struct Observation<'a> {
    pub configured: &'a ConfiguredProvider,
    pub context: &'a ProfileContext,
    pub profile: &'a ProfileRef,
}

/// Settings are fixed when an operation opens the provider.
pub struct ConfiguredProvider {
    provider: Provider,
    settings: crate::storage::ProviderSettings,
    override_path: Option<PathBuf>,
}

impl ConfiguredProvider {
    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn diagnose(&self, host: &dyn Host) -> DiagnosticReport {
        self.provider
            .diagnostic_report(host, self.installation(host))
    }

    pub fn observe(
        &self,
        host: &dyn Host,
        held: &mut crate::lock::Held<'_>,
        context: &ProfileContext,
        profile: &ProfileRef,
        still_ours: crate::lock::StillOurs<'_>,
    ) -> std::result::Result<Vec<crate::domain::WindowUtilization>, crate::observe::Outcome> {
        self.provider.accepts(profile)?;
        self.provider.accepts(&context.profile)?;
        if context.profile.id != profile.id || context.profile.directory != profile.directory {
            return Err(
                PerchError::Invalid("Observation context names another Profile".into()).into(),
            );
        }
        self.provider.adapter.observe(
            host,
            held,
            Observation {
                configured: self,
                context,
                profile,
            },
            still_ours,
        )
    }
    pub fn installation(&self, host: &dyn Host) -> Result<Installation> {
        let id = self.provider.id();
        if !self.enabled() {
            return Err(PerchError::Invalid(format!(
                "{} is disabled. `perch config set --provider {} enabled true` turns it on.",
                id.adapter().name(),
                id.word()
            )));
        }
        let executable = if let Some(path) = &self.override_path {
            if !host.is_file(path) {
                return Err(PerchError::NotFound(format!(
                    "No {} CLI is at {}. `perch config set --provider {} cli-path <path>` names \
                     where it is.",
                    id.adapter().name(),
                    path.display(),
                    id.word()
                )));
            }
            path.clone()
        } else {
            host::programs::on_path(host, self.provider.executable_name()).ok_or_else(|| {
                PerchError::NotFound(format!(
                    "No {} CLI is on PATH. `perch config set --provider {} cli-path <path>` names \
                     where it is.",
                    id.adapter().name(),
                    id.word()
                ))
            })?
        };
        Ok(Installation {
            provider: id,
            executable,
        })
    }
}

/// Provider attribution and executable selection survive configuration changes.
pub struct Installation {
    provider: Id,
    executable: PathBuf,
}
impl Installation {
    pub fn provider(&self) -> Id {
        self.provider
    }
    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }
    pub fn discover(&self, host: &dyn Host) -> Result<Option<Discovered>> {
        let provider = self.provider.adapter();
        let discovered = provider.adapter.discover(host, self)?;
        if let Some(found) = &discovered {
            provider.authenticated(&found.account)?;
        }
        Ok(discovered)
    }
    pub fn diagnostic_session(
        &self,
        request: &DiagnosticSession<'_>,
    ) -> Result<PreparedLaunch<'static>> {
        self.provider
            .adapter()
            .adapter
            .diagnostic_session(self, request)
    }

    pub fn authenticate(&self, host: &dyn Host) -> Result<Authenticated> {
        let provider = self.provider.adapter();
        let authenticated = provider.adapter.authenticate(host, self)?;
        provider.authenticated(&authenticated)?;
        Ok(authenticated)
    }
}

/// The public entry point checks attribution before native operations can act.
#[derive(Clone, Copy)]
pub struct Provider {
    adapter: &'static dyn Adapter,
}
impl Provider {
    pub fn configured(&self, host: &dyn Host) -> Result<ConfiguredProvider> {
        let settings = crate::storage::provider_settings(host, self.id())?;
        let override_path = settings.cli_path.clone().or_else(|| {
            host.env_var(&self.id().executable_override())
                .map(PathBuf::from)
        });
        Ok(ConfiguredProvider {
            provider: *self,
            settings,
            override_path,
        })
    }
    pub fn id(&self) -> Id {
        self.adapter.id()
    }
    pub fn name(&self) -> &'static str {
        self.adapter.name()
    }
    pub fn login_instruction(&self) -> Option<&'static str> {
        self.adapter.login_instruction()
    }
    pub fn switched_note(&self) -> Option<&'static str> {
        self.adapter.switched_note()
    }
    pub fn executable_name(&self) -> &'static str {
        self.adapter.executable_name()
    }
    pub fn service_setup(&self, host: &dyn Host) -> Result<Option<ServiceSetup>> {
        let configured = self.configured(host)?;
        if !configured.enabled() {
            return Ok(None);
        }
        let override_key = self.id().executable_override();
        let explicit_path = configured.override_path;
        let candidates = if explicit_path.is_some() {
            Vec::new()
        } else {
            host::programs::all_on_path(host, self.executable_name())
        };
        Ok(Some(ServiceSetup {
            override_key,
            explicit_path,
            candidates,
            environment: self
                .adapter
                .service_environment()
                .iter()
                .filter_map(|key| host.env_var(key).map(|value| ((*key).into(), value)))
                .collect(),
            probe_args: self.adapter.service_probe_args(),
        }))
    }

    pub fn capabilities(&self) -> Capabilities {
        self.adapter.capabilities()
    }
    pub fn default_workload(&self) -> Option<&'static str> {
        self.adapter.default_workload()
    }
    pub fn configured_workload(
        &self,
        options: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Option<&'static str> {
        self.adapter.configured_workload(options)
    }
    pub fn window_role(
        &self,
        workload: &str,
        window: &crate::domain::WindowUtilization,
    ) -> WindowRole {
        self.adapter.window_role(workload, window)
    }
    pub fn validate_options(
        &self,
        options: &std::collections::BTreeMap<String, serde_json::Value>,
        scope: OptionScope,
    ) -> Result<()> {
        self.adapter.validate_options(options, scope)
    }

    fn accepts(&self, profile: &ProfileRef) -> Result<()> {
        if profile.provider != self.id() {
            return Err(PerchError::Invalid(format!(
                "{} cannot operate on a {} Profile",
                self.name(),
                profile.provider.word()
            )));
        }
        if let Some(subject) = &profile.provider_identity {
            subject.validate(self.id())?;
            if profile.id != subject.key {
                return Err(PerchError::Invalid(
                    "Profile identity disagrees with its Account".into(),
                ));
            }
        }
        Ok(())
    }
    fn authenticated(&self, authenticated: &Authenticated) -> Result<()> {
        if authenticated.provider != self.id() {
            return Err(PerchError::Invalid(
                "Authentication belongs to another provider".into(),
            ));
        }
        if let Some(subject) = &authenticated.subject {
            subject.validate(self.id())?;
        }
        Ok(())
    }
    pub fn maintain(&self, host: &dyn Host) {
        self.adapter.maintain(host);
    }
    pub fn install<'a>(
        &self,
        host: &'a dyn Host,
        profile: &ProfileRef,
        authenticated: &Authenticated,
        mode: InstallMode,
    ) -> Result<AppliedProfile<'a>> {
        self.accepts(profile)?;
        self.authenticated(authenticated)?;
        if profile.provider_identity != authenticated.subject
            || (profile.provider_identity.is_none()
                && (!crate::name::same_name(
                    &profile.identity.email,
                    &authenticated.identity.email,
                ) || profile.identity.account_uuid != authenticated.identity.account_uuid
                    || profile.identity.organization_uuid
                        != authenticated.identity.organization_uuid))
        {
            return Err(PerchError::Invalid(
                "Authentication belongs to another Account or Workspace".into(),
            ));
        }
        self.adapter.install(host, profile, authenticated, mode)
    }
    pub fn check_replacement(
        &self,
        host: &dyn Host,
        profile: &ProfileRef,
        default_reason: Option<&'static str>,
        consequence: &crate::live::Consequence,
    ) -> Result<()> {
        self.accepts(profile)?;
        self.adapter
            .check_replacement(host, profile, default_reason, consequence)
    }
    pub fn diagnose(&self, host: &dyn Host) -> DiagnosticReport {
        match self.configured(host) {
            Ok(configured) => configured.diagnose(host),
            Err(error) => self.diagnostic_report(host, Err(error)),
        }
    }

    fn diagnostic_report(
        &self,
        host: &dyn Host,
        installation: Result<Installation>,
    ) -> DiagnosticReport {
        let mut report = self.adapter.diagnose(host, installation);
        for finding in &mut report.findings {
            finding.provider = Some(self.id());
        }
        report
    }

    pub fn session_evidence(
        &self,
        host: &dyn Host,
        directory: &std::path::Path,
    ) -> std::result::Result<Vec<SessionEvidence>, crate::live::Unsure> {
        self.adapter.session_evidence(host, directory)
    }

    pub fn prepare_launch<'a>(
        &self,
        host: &'a dyn Host,
        request: &LaunchRequest<'_>,
    ) -> Result<PreparedLaunch<'a>> {
        self.accepts(request.account)?;
        if !self.capabilities().shared_state && !request.shared_profiles.is_empty() {
            return Err(PerchError::Invalid(format!(
                "{} does not support sharing client state between Profiles",
                self.name()
            )));
        }
        match request.kind {
            LaunchKind::Client(installation) if installation.provider() != self.id() => {
                return Err(PerchError::Invalid(
                    "The selected installation belongs to another provider".into(),
                ));
            }
            LaunchKind::Custom("") => {
                return Err(PerchError::Invalid(
                    "A custom launch must name an executable".into(),
                ));
            }
            _ => {}
        }
        self.adapter.prepare_launch(host, request)
    }
    pub fn snapshot(&self, host: &dyn Host, context: &ProfileContext) -> Result<ProfileBundle> {
        self.accepts(&context.profile)?;
        let bundle = self.adapter.snapshot(host, context)?;
        bundle.validate()?;
        Ok(bundle)
    }
    pub fn prepare_restore<'a>(
        &self,
        host: &'a dyn Host,
        request: RestoreRequest<'a>,
    ) -> Result<Box<dyn Restore + 'a>> {
        self.accepts(&request.profile)?;
        if let Some(bundle) = request.bundle {
            bundle.validate()?;
        }
        self.adapter.prepare_restore(host, request)
    }
    pub fn forget_credential(
        &self,
        host: &dyn Host,
        profile: &ProfileRef,
    ) -> Result<CredentialRemoval> {
        self.accepts(profile)?;
        self.forget_profile_credential(host, &profile.directory)
    }
    pub fn forget_profile_credential(
        &self,
        host: &dyn Host,
        profile: &std::path::Path,
    ) -> Result<CredentialRemoval> {
        let home = self.id().home(host)?;
        let admitted = ["profiles", "pending"].iter().any(|folder| {
            let root = home.join(folder);
            profile.parent() == Some(root.as_path())
                && profile.file_name().is_some()
                && !profile.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
        });
        if !admitted {
            return Err(PerchError::Invalid(format!(
                "The Profile is outside {}'s managed directories",
                self.name()
            )));
        }
        self.adapter.forget_profile_credential(host, profile)
    }
    pub fn default_matches(&self, host: &dyn Host, profile: &ProfileRef) -> Result<bool> {
        self.accepts(profile)?;
        self.adapter.default_matches(host, profile)
    }
    pub fn inspect_default<'a>(&self, host: &'a dyn Host) -> Result<InspectedDefault<'a>> {
        Ok(InspectedDefault {
            provider: *self,
            native: self.adapter.inspect_default(host)?,
        })
    }
    pub fn prepare_default<'a>(
        &self,
        host: &'a dyn Host,
        held: &mut crate::lock::Held<'_>,
        request: DefaultRequest,
    ) -> Result<Box<dyn DefaultChange + 'a>> {
        self.accepts(&request.incoming)?;
        for profile in request.outgoing.iter().chain(&request.known) {
            self.accepts(profile)?;
        }
        if !self.capabilities().live_switch {
            return Err(PerchError::Invalid(format!(
                "{} does not support live Switching",
                self.name()
            )));
        }
        self.adapter.prepare_default(host, held, request)
    }
}

static SUPPORTED: &[Provider] = &[
    Provider {
        adapter: &super::claude::Claude,
    },
    Provider {
        adapter: &super::codex::Codex,
    },
];

pub fn catalog() -> &'static [Provider] {
    #[cfg(test)]
    if let Some(catalog) = conformance::catalog_override() {
        return catalog;
    }
    SUPPORTED
}

#[cfg(test)]
#[path = "conformance.rs"]
mod conformance;

impl Id {
    pub fn adapter(self) -> Provider {
        catalog()
            .iter()
            .copied()
            .find(|provider| provider.id() == self)
            .expect("every provider identifier has a registered adapter")
    }
}

/// Native payloads stay opaque to command workflows.
pub struct Authenticated {
    pub(super) provider: Id,
    pub(super) identity: crate::domain::Identity,
    pub(super) subject: Option<AccountIdentity>,
    pub(super) plan: Option<String>,
    pub(super) credential: zeroize::Zeroizing<String>,
    pub(super) configuration: Option<zeroize::Zeroizing<String>>,
}

impl Authenticated {
    pub fn identity(&self) -> &crate::domain::Identity {
        &self.identity
    }
    pub fn subject(&self) -> &Option<AccountIdentity> {
        &self.subject
    }
    pub fn plan(&self) -> &Option<String> {
        &self.plan
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    New,
    Repair,
}

/// Only reversible installation is undone when its manifest commit fails.
pub struct AppliedProfile<'a> {
    rollback: Option<Box<dyn FnOnce() -> Result<()> + 'a>>,
}
impl<'a> AppliedProfile<'a> {
    pub(crate) fn reversible(rollback: impl FnOnce() -> Result<()> + 'a) -> Self {
        Self {
            rollback: Some(Box::new(rollback)),
        }
    }
    pub(crate) fn retained() -> Self {
        Self { rollback: None }
    }
    pub fn commit(mut self) {
        self.rollback.take();
    }
    pub fn rollback(mut self) -> Result<()> {
        self.rollback.take().map_or(Ok(()), |rollback| rollback())
    }
}
impl Drop for AppliedProfile<'_> {
    fn drop(&mut self) {
        if let Some(rollback) = self.rollback.take() {
            let _ = rollback();
        }
    }
}

/// Only Profiles admitted by shared Group policy may contribute shared client state.
pub struct SharedProfile {
    pub path: PathBuf,
    pub is_default: bool,
}
pub enum LaunchKind<'a> {
    Client(&'a Installation),
    Custom(&'a str),
}

pub struct LaunchRequest<'a> {
    pub kind: LaunchKind<'a>,
    pub account: &'a ProfileRef,
    pub arguments: &'a [String],
    pub shared_profiles: Vec<SharedProfile>,
}

pub enum LaunchEnvironment {
    Overlay(Vec<(String, String)>),
    Exact(Vec<(String, String)>),
}

/// The native Profile claim lives until the launched process exits.
pub struct PreparedLaunch<'a> {
    pub(crate) program: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) environment: LaunchEnvironment,
    pub(crate) _claim: Option<Box<dyn ResourceLease + 'a>>,
}
impl PreparedLaunch<'_> {
    pub fn program(&self) -> &str {
        &self.program
    }
    pub fn execute(self, host: &dyn Host) -> Result<i32> {
        let args: Vec<_> = self.arguments.iter().map(String::as_str).collect();
        let values = match &self.environment {
            LaunchEnvironment::Overlay(v) | LaunchEnvironment::Exact(v) => v,
        };
        let env: Vec<_> = values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let result = match self.environment {
            LaunchEnvironment::Overlay(_) => host.exec_interactive(&self.program, &args, &env),
            LaunchEnvironment::Exact(_) => host.exec_interactive_under(&self.program, &args, &env),
        };
        result.map_err(|error| {
            PerchError::Other(format!("Could not launch {}: {error}", self.program))
        })
    }
}

/// Discovery reports a native Default without deciding whether Perch should hold it.
pub struct Discovered {
    pub account: Authenticated,
    pub version: String,
}

/// What the Capture found — the part of a Switch worth saying out loud, because
/// it is what protects the Account being left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    /// The live Credential went back into the outgoing Account's Profile.
    Copied { from: String },
    /// Nothing was live to Capture — Claude Code is logged out.
    NothingLive,
    /// Something was live, and the Identity beside it names somebody other than
    /// the Account Perch believes is active — so it was left where it was rather
    /// than filed under a Profile it does not belong to.
    NotTheirs {
        /// The Account it was about to be written into.
        outgoing: String,
        /// Who the Identity beside the live Credential names instead.
        live: String,
    },
    /// Something was live, the store handed it over, and it is not a Credential
    /// Perch can make sense of — so it was left where it was: bytes nothing
    /// understands are not a Rotation to lose. Only where the store *answered*;
    /// one that would not is a refusal, during Capture.
    Unreadable { outgoing: String, why: String },
    /// Perch holds no active Account, so there was nothing to Capture into.
    NoOutgoing,
    /// The live Credential is already byte-for-byte the one this Switch would
    /// write — the trace of a Switch interrupted after the Credential moved and
    /// before it was recorded. No Rotation to save, whether or not the Account
    /// being left is the Account being switched to.
    NothingToSave,
    /// The outgoing Account's own Profile holds a Credential newer than the live
    /// one, so Capturing would write a retired refresh token over the working
    /// copy. Declined: a Capture exists to keep the newest Credential, and here
    /// the newest is the one already stored.
    Superseded { outgoing: String },
}

pub struct DefaultRequest {
    pub incoming: ProfileRef,
    pub outgoing: Option<ProfileRef>,
    pub known: Vec<ProfileRef>,
    pub overwrite: Option<String>,
}
pub struct DefaultFailure {
    pub error: PerchError,
    pub moved: bool,
}

/// A native lock guard spans Capture, the shared Landing journal, and application.
pub trait DefaultChange {
    fn capture(&mut self, held: &mut crate::lock::Held<'_>) -> Result<Captured>;
    fn apply(
        &mut self,
        held: &mut crate::lock::Held<'_>,
    ) -> std::result::Result<(), DefaultFailure>;
}

pub enum DefaultObservation {
    Settled(Option<String>),
    Unknown,
    Stopped,
}

/// The native guard remains held until shared metadata records the observation.
pub(super) trait DefaultInspection {
    fn resolve(
        &mut self,
        held: &mut crate::lock::Held<'_>,
        profiles: &[ProfileRef],
        leaving: Option<&str>,
        arriving: &str,
        may_continue: &mut dyn FnMut() -> bool,
    ) -> Result<DefaultObservation>;
}

/// The provider check precedes native credential reads; the native guard stays held.
pub struct InspectedDefault<'a> {
    provider: Provider,
    native: Box<dyn DefaultInspection + 'a>,
}

impl InspectedDefault<'_> {
    pub fn resolve(
        &mut self,
        held: &mut crate::lock::Held<'_>,
        profiles: &[ProfileRef],
        leaving: Option<&str>,
        arriving: &str,
        may_continue: &mut dyn FnMut() -> bool,
    ) -> Result<DefaultObservation> {
        for profile in profiles {
            self.provider.accepts(profile)?;
        }
        self.native
            .resolve(held, profiles, leaving, arriving, may_continue)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptionScope {
    Installation,
    Policy,
}

/// Opaque resources stay alive until the native operation finishes.
pub(crate) trait ResourceLease {}
impl<T> ResourceLease for T {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Ranking,
    Constraint,
    Other,
}

/// Native bundles hold authentication and configuration, not client histories.
const MAX_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
const MAX_BUNDLE_ARTIFACTS: usize = 256;
const MAX_ARTIFACT_NAME_BYTES: usize = 1024;

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileBundle {
    #[serde(deserialize_with = "crate::json::unique_map")]
    artifacts: std::collections::BTreeMap<String, Artifact>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    purpose: ArtifactPurpose,
    content: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPurpose {
    Credential,
    Configuration,
}
impl Drop for Artifact {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.content.zeroize();
    }
}
impl std::fmt::Debug for ProfileBundle {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("ProfileBundle")
            .field("artifacts", &self.artifacts.keys())
            .finish()
    }
}
impl ProfileBundle {
    pub fn has_credentials(&self) -> bool {
        self.artifacts
            .values()
            .any(|artifact| artifact.purpose == ArtifactPurpose::Credential)
    }
    pub fn bytes(&self) -> usize {
        self.artifacts
            .values()
            .map(|artifact| artifact.content.len())
            .sum()
    }
    pub fn validate(&self) -> Result<()> {
        if self.artifacts.len() > MAX_BUNDLE_ARTIFACTS {
            return Err(PerchError::Invalid(
                "A Profile bundle exceeds the limit of 256 artifacts.".into(),
            ));
        }
        let mut remaining = MAX_BUNDLE_BYTES;
        for (name, artifact) in &self.artifacts {
            remaining = remaining
                .checked_sub(artifact.content.len())
                .ok_or_else(|| {
                    PerchError::Invalid(
                        "A Profile bundle exceeds the content limit of 16 MiB.".into(),
                    )
                })?;
            if name.len() > MAX_ARTIFACT_NAME_BYTES {
                return Err(PerchError::Invalid(
                    "A Profile artifact name exceeds the limit of 1024 bytes.".into(),
                ));
            }
            if name.is_empty()
                || name.chars().any(char::is_control)
                || name.contains(['\\', ':'])
                || std::path::Path::new(name)
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(PerchError::Invalid(
                    "A Profile artifact must have a relative name without parent components".into(),
                ));
            }
        }
        Ok(())
    }
    pub(super) fn insert(&mut self, name: &str, purpose: ArtifactPurpose, content: String) {
        self.artifacts
            .insert(name.into(), Artifact { purpose, content });
    }
    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.artifacts
            .get(name)
            .map(|artifact| artifact.content.as_str())
    }
    pub(super) fn expect(&self, permitted: &[(&str, ArtifactPurpose)]) -> Result<()> {
        self.validate()?;
        for (name, artifact) in &self.artifacts {
            if !permitted
                .iter()
                .any(|(key, purpose)| *key == name && *purpose == artifact.purpose)
            {
                return Err(PerchError::Invalid(format!(
                    "The provider does not recognize Profile artifact `{name}`"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod bundle_tests {
    use super::ProfileBundle;

    #[test]
    fn bundle_limits_count_utf8_bytes_and_total_content() {
        let mut bundle = ProfileBundle::default();
        bundle.insert(
            "first",
            super::ArtifactPurpose::Configuration,
            "é".repeat(super::MAX_BUNDLE_BYTES / 4),
        );
        bundle.insert(
            "second",
            super::ArtifactPurpose::Credential,
            "x".repeat(super::MAX_BUNDLE_BYTES / 2),
        );
        assert!(bundle.validate().is_ok());
        bundle.insert("third", super::ArtifactPurpose::Credential, "x".into());
        assert!(
            bundle
                .validate()
                .unwrap_err()
                .to_string()
                .contains("16 MiB")
        );
    }

    #[test]
    fn bundle_limits_bound_empty_artifacts_and_name_bytes() {
        let mut bundle = ProfileBundle::default();
        for index in 0..super::MAX_BUNDLE_ARTIFACTS {
            bundle.insert(
                &format!("artifact-{index}"),
                super::ArtifactPurpose::Configuration,
                String::new(),
            );
        }
        assert!(bundle.validate().is_ok());
        bundle.insert(
            "excess",
            super::ArtifactPurpose::Configuration,
            String::new(),
        );
        assert!(
            bundle
                .validate()
                .unwrap_err()
                .to_string()
                .contains("256 artifacts")
        );
        let mut bundle = ProfileBundle::default();
        bundle.insert(
            &"é".repeat(super::MAX_ARTIFACT_NAME_BYTES / 2),
            super::ArtifactPurpose::Configuration,
            String::new(),
        );
        assert!(bundle.validate().is_ok());
        bundle.insert(
            &"é".repeat(super::MAX_ARTIFACT_NAME_BYTES / 2 + 1),
            super::ArtifactPurpose::Configuration,
            String::new(),
        );
        assert!(
            bundle
                .validate()
                .unwrap_err()
                .to_string()
                .contains("1024 bytes")
        );
    }

    #[test]
    fn duplicate_artifact_names_cannot_discard_a_secret() {
        let document = r#"{"artifacts":{"oauth":{"purpose":"credential","content":"first"},"oauth":{"purpose":"credential","content":"second"}}}"#;
        assert!(serde_json::from_str::<ProfileBundle>(document).is_err());
    }

    #[test]
    fn bundle_debug_never_displays_artifact_contents() {
        let bundle: ProfileBundle = serde_json::from_str(
            r#"{"artifacts":{"oauth":{"purpose":"credential","content":"synthetic secret"}}}"#,
        )
        .unwrap();
        assert!(!format!("{bundle:?}").contains("synthetic secret"));
    }
}
