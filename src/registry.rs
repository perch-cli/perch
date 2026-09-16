//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned, and the version moves when the shape does
//! (ADR the-holdings-outlive-a-perch): a Registry claiming more than this build
//! understands is refused rather than silently misread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Scope, Settings, UngroupedConfig};
use crate::domain::Identity;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::lock;
use crate::name::{self, NameKind, UNGROUPED, means_ungrouped, same_name};

/// The version this build writes.
///
/// A Registry claiming a higher one is refused rather than silently misread, and
/// the guard is only worth having if this moves whenever the shape does.
pub const CURRENT_VERSION: u32 = 9;

pub use crate::domain::{CachedUtilization, Quarantine, WindowUtilization};

impl Quarantine {
    /// What happened, as the middle of a sentence about the Account: "{named}
    /// is Quarantined: {because}."
    pub fn because(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "the provider would not renew its Credential",
            Quarantine::RotationLost => {
                "the provider Rotated its refresh token and the new one could not be stored, \
                 so the one Perch holds is retired"
            }
            Quarantine::NoRefreshToken => {
                "the Credential Perch holds carries no refresh token, so it cannot be renewed"
            }
            Quarantine::NoCredential => "Perch holds no Credential for it",
        }
    }

    /// Whether getting here cost a request to the provider, which is what the
    /// Watcher's Back-off paces. A property of what happened rather than of why
    /// the Renewal was wanted: both reasons reach both halves of this.
    pub fn reached_provider(&self) -> bool {
        match self {
            Quarantine::RenewalRejected | Quarantine::RotationLost => true,
            Quarantine::NoRefreshToken | Quarantine::NoCredential => false,
        }
    }

    /// The reason as a script reads it, which is the spelling the Registry
    /// records.
    pub fn as_str(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "renewal-rejected",
            Quarantine::RotationLost => "rotation-lost",
            Quarantine::NoRefreshToken => "no-refresh-token",
            Quarantine::NoCredential => "no-credential",
        }
    }

    /// The whole of what is said about a Quarantined Account where nothing around
    /// it says any of it: which Account, what happened, and how to end it.
    ///
    /// `detail` is whatever the failure underneath said. The reason is what
    /// happened; the detail is how.
    pub fn said_of(&self, named: &str, target: &str, detail: Option<&str>) -> String {
        let how = match detail {
            Some(detail) => format!(" ({detail})"),
            None => String::new(),
        };
        format!(
            "{named} is Quarantined: {}{how}. {}",
            self.because(),
            how_to_repair(target)
        )
    }

    /// What is true of this Account and no other: which one it is and what
    /// happened to it, without the repair.
    ///
    /// For a surface that has already said the state and says the repair once
    /// beneath all of them (ADR perch-says-what-it-did).
    pub fn shown_of(&self, named: &str) -> String {
        format!("{named}: {}.", self.because())
    }

    /// The refusal a command raises rather than acting on a Quarantined Account,
    /// as opposed to [`said_of`](Quarantine::said_of), which is how one is
    /// *shown*.
    ///
    /// `consequence` is the caller's, and is the only part that differs.
    pub fn refusal(self, named: &str, target: &str, consequence: &str) -> PerchError {
        PerchError::Quarantined {
            why: self,
            said: format!(
                "{named} is Quarantined: {}.\n{consequence} {}",
                self.because(),
                how_to_repair(target),
            ),
        }
    }

    /// The same as a script reads it. Absent reads as false wherever a script
    /// asks whether it is set, so one already branching on the fact carries why.
    ///
    /// `said` rather than `detail`: the Registry records a `Quarantine` and not
    /// the failure behind one, so there is no "how" here to carry.
    pub fn document(quarantine: Option<Quarantine>) -> serde_json::Value {
        match quarantine {
            Some(why) => {
                serde_json::json!({"reason": why.as_str(), "said": why.because()})
            }
            None => serde_json::Value::Null,
        }
    }
}

/// How a Quarantine is asked about and how it is put right, said the same way
/// wherever an Account is shown as broken.
pub fn how_to_repair(target: &str) -> String {
    format!(
        "`perch relogin {target}` logs it in again in place, keeping its Alias, \
         its Group and whether Cycling may choose it."
    )
}

/// The same repair, for however many Accounts are in that state — said once,
/// because it is the same repair.
///
/// Named where there is exactly one to name: "logs *it* in again" over a set
/// tells somebody holding three broken Accounts to repair the first.
pub fn how_to_repair_them(targets: &[impl AsRef<str>]) -> Option<String> {
    match targets {
        [] => None,
        [one] => Some(how_to_repair(one.as_ref())),
        _ => Some(
            "`perch relogin <target>` logs one in again in place, keeping its \
             Alias, its Group and whether Cycling may choose it."
                .to_string(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_identity: Option<crate::providers::provider::AccountIdentity>,
    #[serde(default)]
    pub provider: crate::providers::provider::Id,
    /// Who this Account is. Its email address is also its identifier.
    pub identity: Identity,
    /// The subscription the Credential reports — `pro`, `max`, and so on. It
    /// comes from the Credential rather than the Identity, which is why it is
    /// not part of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Whether the Account has been taken out of Cycling.
    ///
    /// Said only when true: the positive state has no name to write down — it is
    /// the absence of this one (ADR a-command-names-its-noun).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Why this Account's Credential can no longer be used, when it cannot.
    ///
    /// Left out of the file entirely rather than written as a null: the Registry
    /// is something a person may open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<CachedUtilization>,
}

/// The last unasked Switch within a Scope, so the next round can be paced by it
/// (ADR a-watcher-knob-is-arithmetic). Written down rather than kept in memory
/// because a Watcher is a process its own Service restarts, and a Cooldown a
/// restart clears is no Cooldown. Per Scope: a Switch within `work` says nothing
/// about how soon `personal` may move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checked {
    /// When the Switch happened, which is what the cooldown counts from.
    pub switched_at: DateTime<Utc>,
}

/// Which Account is active — and, while a Switch is under way, that Perch cannot
/// yet say (ADR a-switch-is-written-down-first).
///
/// One field with three states rather than two, so a Registry naming both a
/// settled active Account and a different in-flight one cannot be written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Active {
    /// Perch is on nobody: a machine that has never Switched, one a removal
    /// left with nowhere to land, or one a repair took off the Account it could
    /// not make live.
    #[default]
    Nobody,
    /// One Account is active, nothing is in flight, and the live Credential is
    /// theirs as far as anything Perch wrote is concerned.
    Settled(String),
    /// A **Landing**: a Switch that has been written down and not yet recorded.
    ///
    /// Written after the Capture and before the Credential moves, so a Perch that
    /// finds one knows the live Credential belongs to one of these two Accounts.
    /// Every Switch path settles one before it acts.
    Landing {
        /// The Account being left. `None` where Perch was on nobody, which is a
        /// Switch with no Capture to lose.
        leaving: Option<String>,
        /// The Account being switched to.
        arriving: String,
    },
}

impl Active {
    /// The Account to treat as active, which during a Landing is the one being
    /// left.
    ///
    /// Nothing has been recorded as having moved, and every path that could
    /// *lose* something by believing it settles the Landing first.
    pub fn whose(&self) -> Option<&str> {
        match self {
            Active::Nobody => None,
            Active::Settled(email) => Some(email),
            Active::Landing { leaving, .. } => leaving.as_deref(),
        }
    }

    /// Whether this address is the one a reader would call active, which during
    /// a Landing is the Account being *left*. Ungated, for a renderer that shows
    /// the Landing beside the answer rather than declining to answer.
    pub fn is_active(&self, email: &str) -> bool {
        self.whose().is_some_and(|active| same_name(active, email))
    }

    /// Whether this address is named here in any role, case-folded like every
    /// other way the Registry is asked about a name.
    pub fn names(&self, email: &str) -> bool {
        match self {
            Active::Nobody => false,
            Active::Settled(held) => same_name(held, email),
            Active::Landing { leaving, arriving } => {
                same_name(arriving, email)
                    || leaving
                        .as_deref()
                        .is_some_and(|leaving| same_name(leaving, email))
            }
        }
    }

    /// The Switch that was in flight and never recorded, said out loud.
    ///
    /// Never changes an exit code: the next Switch resolves this by itself. Here
    /// rather than in a command, because `perch status` and `perch list` both
    /// say it.
    pub fn a_switch_in_flight(&self) -> Option<String> {
        let Active::Landing { leaving, arriving } = self else {
            return None;
        };
        let was_on = match leaving {
            Some(leaving) => format!("Perch was on {leaving}"),
            None => "Perch was on no Account".to_string(),
        };
        Some(format!(
            "A Switch was in flight and was not recorded. {was_on} and was \
             switching to {arriving}, so which Credential is live is not \
             settled. The next Switch resolves it, and says so if it cannot."
        ))
    }

    /// The Switch that was in flight and never recorded, as a script reads it,
    /// and `null` on every machine that is not mid-Landing.
    ///
    /// Beside whichever key already says who is active rather than folded into
    /// it: *which Account* and *whether Perch can say* are different questions.
    pub fn document(&self) -> serde_json::Value {
        match self {
            Active::Landing { leaving, arriving } => {
                serde_json::json!({"leaving": leaving, "arriving": arriving})
            }
            Active::Nobody | Active::Settled(_) => serde_json::Value::Null,
        }
    }

    /// Being on the Account a Switch was leaving, or on nobody where it was
    /// leaving nobody. What a Landing comes back to when nothing moved.
    pub fn settled_on(leaving: Option<String>) -> Active {
        match leaving {
            Some(leaving) => Active::Settled(leaving),
            None => Active::Nobody,
        }
    }
}

/// No Landing is in flight, so the Registry a reader is about to ask tells the
/// truth about who is active. A witness (ADR an-ordering-is-a-type), and the
/// negative of a Landing, so nothing is promoted. Two things earn it:
/// [`Registry::settle`] records what a walk settled a Landing on, and
/// [`nothing_in_flight`] finds there was none to settle.
pub struct Settled(());

/// The witness for a reader that has a Landing to *check* rather than one to
/// settle: a `perch watcher run` says what it is about to watch off a Registry it
/// has not locked, and a Landing in flight is the state where it has nothing to
/// say yet, because [`Active::whose`] answers with the Account being *left*.
/// `None` is the whole of what it can answer about a Landing.
pub fn nothing_in_flight(registry: &Registry) -> Option<Settled> {
    match registry.active() {
        Active::Landing { .. } => None,
        Active::Nobody | Active::Settled(_) => Some(Settled(())),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    #[serde(default = "crate::storage::provider_defaults")]
    pub provider_settings:
        BTreeMap<crate::providers::provider::Id, crate::storage::ProviderSettings>,
    #[serde(default)]
    pub run_provider: crate::providers::provider::Id,
    #[serde(default = "enabled_by_default")]
    pub run_fallback: bool,
    #[serde(default)]
    pub watcher_paused: bool,
    #[serde(default)]
    pub scope_defaults: crate::config::PolicyDefaults,
    #[serde(default)]
    pub next_group_id: u64,
    #[serde(default)]
    pub runtime: BTreeMap<crate::providers::provider::Id, ProviderState>,
    #[serde(skip)]
    pub(crate) selected_provider: crate::providers::provider::Id,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Alias to Account email.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// The Groups the user has declared, with the Settings each one holds. A
    /// Group exists here even when it holds no Accounts: it is a statement
    /// somebody made, not a summary of where the Accounts happen to be.
    #[serde(default)]
    pub groups: BTreeMap<String, crate::config::ScopeSettings>,
    /// What the Accounts in no Group hold, taken as one Scope. Not a Group and
    /// never one; here rather than under a reserved key in `groups` so that
    /// nothing can walk it as one.
    #[serde(default)]
    pub ungrouped: UngroupedConfig,
}

/// Defaults and watcher timing are independent for each provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderState {
    #[serde(default)]
    active: Active,
    #[serde(default)]
    pub checks: BTreeMap<String, Checked>,
}

impl ProviderState {
    pub(crate) fn from_parts(active: Active, checks: BTreeMap<String, Checked>) -> Self {
        Self { active, checks }
    }
}

fn enabled_by_default() -> bool {
    true
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            provider_settings: crate::storage::provider_defaults(),
            run_provider: crate::providers::provider::Id::Claude,
            run_fallback: true,
            watcher_paused: false,
            scope_defaults: crate::config::PolicyDefaults::default(),
            next_group_id: 0,
            runtime: BTreeMap::new(),
            selected_provider: crate::providers::provider::Id::default(),
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
            groups: BTreeMap::new(),
            ungrouped: UngroupedConfig::default(),
        }
    }
}

impl Account {
    pub fn profile(&self, host: &dyn Host) -> Result<crate::providers::provider::ProfileRef> {
        Ok(crate::providers::provider::ProfileRef {
            id: self.key().into(),
            provider: self.provider(),
            identity: self.identity.clone(),
            provider_identity: self.provider_identity.clone(),
            quarantine: self.quarantine,
            directory: self.profile_dir(host)?,
        })
    }

    pub fn key(&self) -> &str {
        self.provider_identity.as_ref().map_or(
            self.storage_key.as_deref().unwrap_or(&self.identity.email),
            |identity| &identity.key,
        )
    }

    pub fn provider(&self) -> crate::providers::provider::Id {
        self.provider
    }

    pub fn email(&self) -> &str {
        &self.identity.email
    }

    /// Profile paths derive from provider and Account identity, never mutable Aliases.
    pub fn profile_dir(&self, host: &dyn Host) -> Result<PathBuf> {
        holdings::profile_dir_for(self.provider(), host, self.key())
    }

    /// Whether this Account is Quarantined, for the places that only need the
    /// fact and not the reason.
    pub fn quarantined(&self) -> bool {
        self.quarantine.is_some()
    }

    /// The cached Utilization, if any figure has ever been observed. An empty
    /// set of windows is not an observation.
    pub fn observed_utilization(&self) -> Option<&CachedUtilization> {
        self.utilization
            .as_ref()
            .filter(|cached| !cached.windows.is_empty())
    }
}

/// One claim on the shared namespace: what a caller is doing with a name, so
/// which checks apply is [`Registry::refuse`]'s knowledge rather than each
/// site's.
#[derive(Clone, Copy)]
pub enum Claim<'a> {
    /// A name of one kind claimed outright — a Group declared or renamed, an
    /// Alias given. `instead_of` is the name the holder gives up, which waives
    /// only the collision with itself.
    Naming {
        kind: NameKind,
        name: &'a str,
        instead_of: Option<&'a str>,
    },
    /// `perch add`'s pair, either half optional. The Group half is shape and
    /// the Alias collision only, because a Group named in passing may join one
    /// already declared.
    Adding {
        alias: Option<&'a str>,
        group: Option<&'a str>,
    },
}

impl Registry {
    /// The Account an address names, folded as a Profile is derived: `CAFÉ@…`
    /// and `café@…` share one Profile and `perch add` refuses the second, so
    /// asking in ASCII here would disagree with the directory on disk.
    pub fn account(&self, email: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| same_name(account.key(), email))
    }

    pub fn selected_provider(&self) -> crate::providers::provider::Id {
        self.selected_provider
    }

    /// Selects the provider context for one command or watcher round; never serialized.
    pub fn select_provider(&mut self, provider: crate::providers::provider::Id) {
        self.selected_provider = provider;
    }

    pub fn state_for(&self, provider: crate::providers::provider::Id) -> &ProviderState {
        static EMPTY: std::sync::LazyLock<ProviderState> =
            std::sync::LazyLock::new(ProviderState::default);
        self.runtime.get(&provider).unwrap_or(&EMPTY)
    }

    pub fn active_for(&self, provider: crate::providers::provider::Id) -> &Active {
        &self.state_for(provider).active
    }

    pub fn state(&self) -> &ProviderState {
        self.state_for(self.selected_provider)
    }

    pub fn state_mut(&mut self) -> &mut ProviderState {
        self.runtime.entry(self.selected_provider).or_default()
    }

    pub fn profile_context(
        &self,
        host: &dyn Host,
        account: &Account,
    ) -> Result<crate::providers::provider::ProfileContext> {
        use crate::providers::provider::{DefaultRelation, ProfileContext};
        let default = match self.active_for(account.provider()) {
            Active::Settled(key) if same_name(key, account.key()) => DefaultRelation::Active,
            Active::Landing { arriving, .. } if same_name(arriving, account.key()) => {
                DefaultRelation::Arriving
            }
            Active::Landing {
                leaving: Some(key), ..
            } if same_name(key, account.key()) => DefaultRelation::Leaving,
            _ => DefaultRelation::Parked,
        };
        Ok(ProfileContext {
            profile: account.profile(host)?,
            default,
            shared_with: sharing_a_profile_with(self, account).map(|peer| peer.key().into()),
        })
    }

    pub fn active_account(&self, _settled: &Settled) -> Option<&Account> {
        self.active().whose().and_then(|email| self.account(email))
    }

    /// Which Account is active, or the Switch that was in flight when this was
    /// last written.
    ///
    /// Reading is nobody's to get wrong. Writing is three named transitions.
    pub fn active(&self) -> &Active {
        &self.state().active
    }

    /// Writes down that a Switch is about to move the live Credential, naming
    /// both Accounts it could then belong to.
    ///
    /// Hands back what it replaced, because the Landing has to reach disk before
    /// it means anything — see [`Registry::abandon_landing`].
    pub fn begin_landing(&mut self, leaving: Option<String>, arriving: &str) -> Active {
        std::mem::replace(
            &mut self.state_mut().active,
            Active::Landing {
                leaving,
                arriving: arriving.to_string(),
            },
        )
    }

    /// Puts back what [`Registry::begin_landing`] replaced, where the save that
    /// would have carried it did not happen.
    ///
    /// Not [`Registry::settle`]: this Landing never existed anywhere but in
    /// memory, and nothing has moved.
    pub fn abandon_landing(&mut self, before: Active) {
        self.state_mut().active = before;
    }

    /// Records who is active now that a Switch is over. `None` is a machine on
    /// nobody, and what is passed is whose Credential the machine is holding.
    ///
    /// An address rather than an [`Active`], which is what makes "settled" true
    /// of what it leaves: handed the enum it would accept a Landing.
    pub fn settle(&mut self, on: Option<String>) -> Settled {
        self.state_mut().active = Active::settled_on(on);
        Settled(())
    }

    /// Whether this address is the one the Registry records as active.
    ///
    /// Case-folded, like every other way the Registry is asked about a name:
    /// `upsert` stores the incoming spelling, so an Identity re-read with
    /// different capitalization would leave an exact `==` answering wrongly.
    pub fn is_active(&self, _settled: &Settled, email: &str) -> bool {
        self.active().is_active(email)
    }

    /// Every Group name in use. A Group an Account claims is always declared
    /// too — [`load`] sees to that — so this is the declared set.
    pub fn group_names(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(String::as_str)
    }

    /// The Settings of the Group declared under a name, whatever it was
    /// capitalized as.
    ///
    /// Through [`declared_group`](Self::declared_group), because that is how
    /// every other question about a Group name is answered here.
    pub fn group(&self, name: &str) -> Option<&crate::config::ScopeSettings> {
        self.groups.get(self.declared_group(name)?)
    }

    /// Every Scope a Setting can be said at, in the order they are offered: the
    /// Ungrouped Accounts, then each Group as it was declared.
    pub fn scopes(&self) -> Vec<Scope> {
        let mut every = vec![Scope::Ungrouped];
        every.extend(
            self.group_names()
                .map(|name| Scope::Group(name.to_string())),
        );
        every
    }

    /// The Settings a Scope holds.
    ///
    /// Resolved for the selected provider through the configured policy cascade.
    pub fn settings(&self, scope: &Scope) -> Settings {
        self.resolved_policy(scope, self.selected_provider).settings
    }

    pub fn resolved_policy(
        &self,
        scope: &Scope,
        provider: crate::providers::provider::Id,
    ) -> crate::config::ResolvedPolicy {
        let empty = crate::config::ScopeSettings::default();
        let configured = match scope {
            Scope::Ungrouped => &self.ungrouped.settings,
            Scope::Group(name) => self.group(name).unwrap_or(&empty),
        };
        configured.resolve(&self.scope_defaults, provider)
    }

    pub fn scope_settings(&self, scope: &Scope) -> Option<&crate::config::ScopeSettings> {
        match scope {
            Scope::Ungrouped => Some(&self.ungrouped.settings),
            Scope::Group(name) => self.group(name),
        }
    }

    pub fn scope_settings_mut(
        &mut self,
        scope: &Scope,
    ) -> Option<&mut crate::config::ScopeSettings> {
        match scope {
            Scope::Ungrouped => Some(&mut self.ungrouped.settings),
            Scope::Group(name) => {
                let declared = self.declared_group(name)?.to_string();
                self.groups.get_mut(&declared)
            }
        }
    }

    pub fn scope_id(&self, name: &str) -> String {
        if name::means_the_ungrouped_scope(name) {
            return "ungrouped".into();
        }
        match self.group(name) {
            Some(scope) if !scope.id.is_empty() => scope.id.clone(),
            _ => format!("named:{}", name::folded(name)),
        }
    }

    /// The Scope an Account's Settings come from: its Group, or the Ungrouped
    /// Accounts. One place, because there is nothing to the rule but this match,
    /// which is exactly what gets written out again at a call site.
    pub fn scope_of(&self, account: &Account) -> Scope {
        match &account.group {
            Some(name) => Scope::Group(name.clone()),
            None => Scope::Ungrouped,
        }
    }

    /// The Accounts in a Group, in the order they were added.
    pub fn accounts_in(&self, group: &str) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| {
                account
                    .group
                    .as_deref()
                    .is_some_and(|held| same_name(held, group))
            })
            .collect()
    }

    /// The Accounts that are in no Group — the ordinary starting state, not an
    /// error.
    pub fn ungrouped_accounts(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.is_none())
            .collect()
    }

    /// The Group declared under a name, whatever it was capitalized as. Two
    /// names that differ only in case are one name here, so this is how a Group
    /// typed in passing is matched to the one that exists.
    pub fn declared_group(&self, name: &str) -> Option<&str> {
        self.groups
            .keys()
            .find(|declared| same_name(declared, name))
            .map(String::as_str)
    }

    /// Declares a Group, refusing a name that is not usable or already means
    /// something else.
    pub fn declare_group(&mut self, name: &str) -> Result<()> {
        self.refuse(Claim::Naming {
            kind: NameKind::Group,
            name,
            instead_of: None,
        })?;
        // At the compiled-in defaults, which is what every Setting means until
        // somebody says otherwise about this Group.
        let id = loop {
            self.next_group_id = self
                .next_group_id
                .checked_add(1)
                .ok_or_else(|| PerchError::Invalid("Group identity space is exhausted".into()))?;
            let candidate = format!("g{}", self.next_group_id);
            if !self.groups.values().any(|group| group.id == candidate) {
                break candidate;
            }
        };
        self.groups.insert(
            name.to_string(),
            crate::config::ScopeSettings {
                id,
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Refuses a claim on a name nothing may answer to: one that is not usable,
    /// one another name of the same kind holds, or one the other namespace half
    /// holds. Shape before collision, and which checks apply is read off the
    /// claim rather than known at each site.
    pub fn refuse(&self, claim: Claim<'_>) -> Result<()> {
        match claim {
            Claim::Naming {
                kind,
                name,
                instead_of,
            } => {
                name::validate(kind, name)?;
                let renaming_itself = instead_of.is_some_and(|held| same_name(held, name));
                match kind {
                    NameKind::Group => {
                        if !renaming_itself && let Some(declared) = self.declared_group(name) {
                            return Err(PerchError::Conflict(format!(
                                "There is already a Group called `{declared}`."
                            )));
                        }
                        self.refuse_an_alias_of_this_name(name)
                    }
                    // An Account keeping its own Alias under another
                    // capitalization cannot collide with the Alias it gives up,
                    // and is still asked about the other half.
                    NameKind::Alias => {
                        if !renaming_itself && let Some((held, target)) = self.declared_alias(name)
                        {
                            let target = self.named_for_the_user(target);
                            return Err(PerchError::Conflict(format!(
                                "`{held}` already names {target}. Free it with \
                                 `perch alias {held} --unset` first."
                            )));
                        }
                        self.refuse_a_group_of_this_name(name)
                    }
                }
            }
            Claim::Adding { alias, group } => {
                // Both shapes before anything else: what is wrong with a name
                // is said before what it clashes with.
                if let Some(alias) = alias {
                    name::validate(NameKind::Alias, alias)?;
                }
                if let Some(group) = group {
                    name::validate(NameKind::Group, group)?;
                }
                // The pair against each other, which no check of one name can
                // see: a command setting both at once could otherwise plant the
                // collision this exists to prevent.
                if let (Some(alias), Some(group)) = (alias, group)
                    && same_name(alias, group)
                {
                    return Err(PerchError::Conflict(format!(
                        "`{alias}` cannot be both an Alias and a Group name."
                    )));
                }
                if let Some(alias) = alias {
                    self.refuse(Claim::Naming {
                        kind: NameKind::Alias,
                        name: alias,
                        instead_of: None,
                    })?;
                }
                if let Some(group) = group {
                    self.refuse_an_alias_of_this_name(group)?;
                }
                Ok(())
            }
        }
    }

    /// One side of the shared namespace, asked of an Alias: no Group may hold
    /// the name.
    fn refuse_a_group_of_this_name(&self, name: &str) -> Result<()> {
        match self.declared_group(name) {
            Some(declared) => Err(PerchError::Conflict(format!(
                "`{declared}` is already a Group name, and a name cannot be both."
            ))),
            None => Ok(()),
        }
    }

    /// The other side, asked of a Group name: no Alias may hold it.
    fn refuse_an_alias_of_this_name(&self, name: &str) -> Result<()> {
        match self.declared_alias(name) {
            Some((held, target)) => Err(PerchError::Conflict(format!(
                "`{held}` is already an Alias for {}, and a name cannot be both.",
                self.named_for_the_user(target)
            ))),
            None => Ok(()),
        }
    }

    /// Renames a Group, keeping everything it carries.
    ///
    /// `held` is the name as this Registry holds it. Three things move with it:
    /// the Settings, the Accounts that claim it, and what the last scheduled
    /// Check left — dropping that would let the watcher Switch again at once.
    pub fn rename_group(&mut self, held: &str, to: &str) -> Result<()> {
        // Resolved before the new name is judged, or
        // `perch group rename nosuchgroup work` exits as a name collision. And
        // through `declared_group`, as every question about a Group name is.
        let Some(declared) = self.declared_group(held).map(str::to_string) else {
            return Err(PerchError::NotFound(format!(
                "no Group is called `{held}`."
            )));
        };
        self.refuse(Claim::Naming {
            kind: NameKind::Group,
            name: to,
            instead_of: Some(held),
        })?;

        // `declared` is one of this map's own keys, `declared_group` having
        // just read it out — so the refusal above is the only one there is.
        let identity = self.scope_id(&declared);
        let mut settings = self.groups.remove(&declared).unwrap_or_default();
        settings.id = identity;
        self.groups.insert(to.to_string(), settings);
        for account in &mut self.accounts {
            if account
                .group
                .as_deref()
                .is_some_and(|held| same_name(held, &declared))
            {
                account.group = Some(to.to_string());
            }
        }
        for state in self.runtime.values_mut() {
            if let Some(checked) = state.checks.remove(&declared) {
                state.checks.insert(to.to_string(), checked);
            }
        }
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name a
    /// Group in passing rather than to create one — `perch add --group`.
    ///
    /// Returns the spelling the Group was declared under: naming `Work` in
    /// passing joins `work`.
    pub fn ensure_group(&mut self, name: &str) -> Result<String> {
        if let Some(declared) = self.declared_group(name) {
            return Ok(declared.to_string());
        }
        self.declare_group(name)?;
        Ok(name.to_string())
    }

    /// Forgets a Group. The caller establishes nothing is left in it: dropping
    /// the Group is not a way to empty it.
    ///
    /// What a Watcher left goes with it, or a Group declared under the same name
    /// later would inherit a cooldown from a Group it never was.
    pub fn forget_group(&mut self, name: &str) {
        let Some(declared) = self.declared_group(name).map(str::to_string) else {
            return;
        };
        self.groups.remove(&declared);
        for state in self.runtime.values_mut() {
            state.checks.remove(&declared);
        }
    }

    /// The last unasked Switch within a Scope, if one has happened there.
    pub fn checked(&self, group: &str) -> Option<&Checked> {
        self.state()
            .checks
            .iter()
            .find(|(declared, _)| same_name(declared, group))
            .map(|(_, checked)| checked)
    }

    /// Records an unasked Switch, for the next round to be paced by.
    ///
    /// Filed under the spelling the Group was declared under, so a round naming
    /// it in another case does not leave a second record pacing nothing.
    pub fn record_switch(&mut self, group: &str, at: DateTime<Utc>) {
        let under = self.declared_group(group).unwrap_or(group).to_string();
        self.state_mut()
            .checks
            .insert(under, Checked { switched_at: at });
    }

    pub fn account_mut(&mut self, email: &str) -> Option<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| same_name(account.key(), email))
    }

    /// The same, where the Account has to be there.
    ///
    /// The state cannot happen — [`validate`] refuses every Registry that could
    /// produce it, on the way in and, since `save` validates too, on the way out
    /// — so a refusal naming what could not be found beats a panic.
    pub fn held(&self, email: &str) -> Result<&Account> {
        self.account(email).ok_or_else(|| no_such_account(email))
    }

    /// [`Self::held`], for the callers that go on to change what they find.
    pub fn held_mut(&mut self, email: &str) -> Result<&mut Account> {
        match self.account_mut(email) {
            Some(account) => Ok(account),
            None => Err(no_such_account(email)),
        }
    }

    /// The Alias an Account answers to, if it has been given one.
    pub fn alias_of(&self, email: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, target)| same_name(target, email))
            .map(|(alias, _)| alias.as_str())
    }

    /// Every Account's Alias at once, for a caller asking about more than one.
    ///
    /// [`Registry::alias_of`] scans, the map being keyed by Alias rather than by
    /// Account, so a listing asking it per row is a scan per row.
    pub fn aliases_by_account(&self) -> AliasOf<'_> {
        let mut held: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for (alias, email) in &self.aliases {
            // First wins, as `alias_of`'s scan does: `validate` refuses two
            // Aliases for one Account, and `aliases` walks in sorted order.
            held.entry(name::folded(email)).or_insert(alias.as_str());
        }
        AliasOf(held)
    }

    /// An Account as the user names it: by its Alias when it has one, so a
    /// message about it reads the way they would say it.
    pub fn named_for_the_user(&self, email: &str) -> String {
        let display = self.account(email).map_or_else(
            || email.to_string(),
            |account| match &account.provider_identity {
                Some(identity) if identity.workspace_id.is_some() => format!(
                    "{} ({}, Workspace {})",
                    account.email(),
                    account.provider().adapter().name(),
                    identity.workspace_id.as_deref().unwrap()
                ),
                Some(_) => format!(
                    "{} ({})",
                    account.email(),
                    account.provider().adapter().name()
                ),
                None => account.email().to_string(),
            },
        );
        match self.alias_of(email) {
            Some(alias) => format!("{display} (as `{alias}`)"),
            None => display,
        }
    }

    /// The Alias held under a name, whatever it was capitalized as, and the
    /// Account it reaches.
    pub fn declared_alias(&self, name: &str) -> Option<(&str, &str)> {
        self.aliases
            .iter()
            .find(|(alias, _)| same_name(alias, name))
            .map(|(alias, email)| (alias.as_str(), email.as_str()))
    }

    /// Names an Account, refusing a name that is not usable or already means
    /// something else. Hands back the Alias it gave up, where it had one.
    ///
    /// An Account answers to one Alias, so naming one that already had a name
    /// replaces it rather than adding to it.
    pub fn name_account(&mut self, alias: &str, email: &str) -> Result<Option<String>> {
        let previous = self.alias_of(email).map(str::to_string);
        self.refuse(Claim::Naming {
            kind: NameKind::Alias,
            name: alias,
            instead_of: previous.as_deref(),
        })?;

        // The address as the Registry *holds* it, not as it was typed: the
        // lookup above folds case, and storing the typed spelling would point
        // the Alias at a string no `accounts` entry has.
        let held = self
            .account(email)
            .map_or_else(|| email.to_string(), |account| account.key().to_string());
        self.aliases.retain(|_, named| !same_name(named, email));
        self.aliases.insert(alias.to_string(), held);
        Ok(previous)
    }

    /// Frees a name, returning the name as it was held and the Account it used
    /// to reach.
    pub fn unset_alias(&mut self, alias: &str) -> Option<(String, String)> {
        let held = self.declared_alias(alias)?.0.to_string();
        let email = self.aliases.remove(&held)?;
        Some((held, email))
    }

    /// Records that an Account's Credential can no longer be used, and says
    /// whether that is news.
    ///
    /// The first reason stands: a Quarantined Account asked a second question
    /// fails a second way, and the reason worth keeping is how it broke.
    pub fn quarantine(&mut self, email: &str, why: Quarantine) -> bool {
        match self.account_mut(email) {
            Some(account) if account.quarantine.is_none() => {
                account.quarantine = Some(why);
                true
            }
            _ => false,
        }
    }

    /// Returns an Account to the pool of ones that work, reporting what it was
    /// Quarantined for. Only a login can do this: nothing else produces a
    /// Credential to replace the one that stopped working.
    pub fn release(&mut self, email: &str) -> Option<Quarantine> {
        self.account_mut(email)?.quarantine.take()
    }

    /// Forgets an Account: the entry, the Alias, and its place as the active one.
    ///
    /// Its Group is left declared, and the Credential is not this to delete —
    /// what a Profile holds is the caller's to take away first, while the Account
    /// can still be named.
    pub fn forget(&mut self, email: &str) {
        self.accounts
            .retain(|account| !same_name(account.key(), email));
        self.aliases.retain(|_, named| !same_name(named, email));
        // Either half of a Landing, and it comes back to whichever half is
        // still held: a Landing naming an Account Perch no longer holds is a
        // dangling pointer `load` refuses. Through `settle`, like every writer.
        for state in self.runtime.values_mut() {
            if state.active.names(email) {
                let remaining = state
                    .active
                    .whose()
                    .filter(|key| !same_name(key, email))
                    .map(str::to_string);
                state.active = Active::settled_on(remaining);
            }
        }
    }

    pub fn upsert(&mut self, mut account: Account) {
        if account.provider_identity.is_none() {
            let key = self
                .account(account.key())
                .map(|held| held.key().to_string())
                .unwrap_or_else(|| account.key().to_string());
            account.storage_key = Some(key);
        }
        match self
            .accounts
            .iter_mut()
            .find(|existing| same_name(existing.key(), account.key()))
        {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }
}

/// [`Registry::alias_of`] answered for every Account rather than for one.
pub struct AliasOf<'a>(std::collections::HashMap<String, &'a str>);

impl<'a> AliasOf<'a> {
    pub fn account(&self, email: &str) -> Option<&'a str> {
        self.0.get(name::folded(email).as_str()).copied()
    }
}

/// Which Profiles more than one Account derives, settled in one pass.
///
/// `is_a_candidate` asks this of every Account and is asked of every one, so
/// answering it by [`sharing_a_profile_with`]'s scan cost n² for one fact.
pub struct Sharers(std::collections::HashSet<String>);

impl Sharers {
    pub fn across(registry: &Registry) -> Sharers {
        // Counting is sound because `validate` refuses two Accounts under one
        // folded address, so nobody is counted as sharing with themselves.
        let mut once: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(registry.accounts.len());
        let mut twice = std::collections::HashSet::new();
        let mut slugged = String::new();
        for account in registry
            .accounts
            .iter()
            .filter(|account| account.provider_identity.is_none())
        {
            holdings::slug_into(&mut slugged, account.key());
            if !once.insert(slugged.clone()) {
                twice.insert(slugged.clone());
            }
        }
        Sharers(twice)
    }

    /// Whether this Account's Profile is one another Account derives too.
    pub fn hold(&self, email: &str) -> bool {
        self.0.contains(holdings::slug(email).as_str())
    }
}

/// The other Account a Profile belongs to as well, where there is one.
///
/// Three commands ask it — a Switch, a Renewal and a Remove — and each spelled
/// its own scan, two of them comparing addresses by bytes where the third folded
/// case.
pub fn sharing_a_profile_with<'a>(
    registry: &'a Registry,
    account: &Account,
) -> Option<&'a Account> {
    // Slugged once rather than once per comparison, and the other side into a
    // buffer this scan keeps: `is_a_candidate` asks this of every Account and
    // is itself asked of every one, so an allocation here is paid n² times.
    let mine = holdings::slug(account.key());
    let mut theirs = String::with_capacity(mine.len());
    registry.accounts.iter().find(|held| {
        held.provider() == account.provider() && !same_name(held.key(), account.key()) && {
            holdings::slug_into(&mut theirs, held.key());
            theirs == mine
        }
    })
}

/// Reads the Registry, or `None` when Perch has never run here.
pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    crate::storage::load(host)
}

/// Where to put right something only a hand edit could have put wrong.
///
/// Kept apart from [`validate`]'s rule because [`save`] is holding a Registry
/// nobody hand-edited, and telling somebody to edit a value that is not in the
/// file yet is the one sentence that would make it worse.
pub fn the_file_to_edit(path: &Path) -> String {
    format!(
        "It is in {}, which every Perch command reads, including the ones that \
         would set it. Edit the value there.",
        path.display(),
    )
}

/// The refusal for an Account that was named and is not there.
///
/// Unreachable by construction, so it is worded as what it is rather than as
/// something to go and fix.
fn no_such_account(email: &str) -> PerchError {
    PerchError::Other(format!(
        "Perch was asked for {email}, which it does not hold, by something that \
         had already established it did.\n\
         {}\n\
         Nothing was changed.",
        crate::report::this_is_a_bug(),
    ))
}

/// Everything a Registry has to be true of before any command acts on it.
///
/// Checked on the way in rather than where each value is read, because the thing
/// that reads them is a loop nobody is watching. Public because an Import writes
/// a Registry without reading one, and what it accepts must not differ.
pub fn validate(registry: &Registry) -> Result<()> {
    use crate::providers::provider::OptionScope;
    for (provider, settings) in &registry.provider_settings {
        provider
            .adapter()
            .validate_options(&settings.options, OptionScope::Installation)?;
    }
    for settings in registry
        .groups
        .values()
        .chain(std::iter::once(&registry.ungrouped.settings))
    {
        for (provider, local) in &settings.providers {
            provider
                .adapter()
                .validate_options(&local.options, OptionScope::Policy)?;
        }
    }

    let mut group_ids = std::collections::BTreeSet::new();
    for name in registry.groups.keys() {
        let id = registry.scope_id(name);
        if id == "ungrouped" || !group_ids.insert(id) {
            return Err(PerchError::Invalid(
                "Group identities must be unique and cannot identify the Ungrouped Scope".into(),
            ));
        }
    }

    for account in &registry.accounts {
        if let Some(identity) = &account.provider_identity {
            identity.validate(account.provider())?;
            if account.identity.account_uuid.as_deref() != Some(identity.user_id.as_str())
                || account.identity.organization_uuid.as_deref() != identity.workspace_id.as_deref()
            {
                return Err(PerchError::Invalid(
                    "Account description disagrees with its provider identity".into(),
                ));
            }
        }
    }
    for (provider, state) in &registry.runtime {
        let keys: Vec<&str> = match &state.active {
            Active::Nobody => Vec::new(),
            Active::Settled(key) => vec![key],
            Active::Landing { leaving, arriving } => leaving
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(arriving.as_str()))
                .collect(),
        };
        for key in keys {
            let role = match &state.active {
                Active::Landing { arriving, .. } if name::same_name(key, arriving) => {
                    format!("a Switch to {key} was under way")
                }
                Active::Landing { .. } => format!("a Switch away from {key} was under way"),
                _ => format!("names {key} in {}'s active state", provider.word()),
            };
            refuse_a_dangling_pointer(registry, key, &role)?;
            if registry.held(key)?.provider() != *provider {
                return Err(PerchError::Invalid(format!(
                    "{}'s active state names an Account from another provider",
                    provider.word()
                )));
            }
        }
        for named in state.checks.keys() {
            if !same_name(named, UNGROUPED) && registry.declared_group(named).is_none() {
                return Err(PerchError::Invalid(format!(
                    "The registry records a Check against `{named}`, which is neither a Group nor Ungrouped"
                )));
            }
        }
        if let Some((a, b)) = first_collision(state.checks.keys().map(String::as_str)) {
            return Err(PerchError::Invalid(format!(
                "The registry records a Check against `{a}` and `{b}`, which are one Group"
            )));
        }
    }

    // Every Scope, and every Scope is all of them: with no layer above, one
    // walk over the Scopes is the whole of the check.
    let empty = crate::config::ScopeSettings::default();
    for provider in crate::providers::provider::catalog() {
        empty
            .resolve(&registry.scope_defaults, provider.id())
            .settings
            .validate(&Scope::Ungrouped)?;
        for scope in registry.scopes() {
            registry
                .resolved_policy(&scope, provider.id())
                .settings
                .validate(&scope)?;
        }
    }

    // The Group *names* an Account claims, the declared Groups, and the Aliases
    // with them: a hand-edited Registry is exactly where a name nothing would
    // have accepted comes from, in any of the three.
    let claimed = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.as_deref());
    for name in claimed.chain(registry.groups.keys().map(String::as_str)) {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Group, name)?;
    }
    for name in registry.aliases.keys() {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Alias, name)?;
    }

    // What each Alias points *at*, which the loop above does not look at. A
    // dangling one is not a refusal downstream — it is the `expect` in every
    // command that resolves a Target.

    // Keyed rather than scanned: both questions below are asked of every Alias,
    // and two names are one name exactly where `name::folded` agrees.
    let held: std::collections::HashSet<String> = registry
        .accounts
        .iter()
        .map(|account| name::folded(account.key()))
        .collect();
    let mut named: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for (alias, email) in &registry.aliases {
        if !held.contains(name::folded(email).as_str()) {
            return Err(PerchError::Invalid(format!(
                "The registry gives the Alias `{alias}` to {email}, which is not \
                 an Account Perch holds.",
            )));
        }
        // One Account, one Alias. With two, `alias_of` returns whichever the map
        // yields first, so `perch list` shows one while `perch switch` answers to
        // both — the same undecided answer as two names differing only in case.
        if let Some(already) = named.insert(name::folded(email), alias) {
            return Err(PerchError::Invalid(format!(
                "The registry gives {email} both the Alias `{already}` and the \
                 Alias `{alias}`, and an Account answers to one Alias at a \
                 time, so which of them Perch shows it under is not decided by \
                 anything.",
            )));
        }
    }

    // The third member of the namespace. `name::validate` keeps an Alias and a
    // Group name tellable from an address, no `@` being an identifier
    // character; the mirror rule, that an address looks like one, is this.
    for account in &registry.accounts {
        if account.provider_identity.is_none() && !account.key().contains('@') {
            return Err(PerchError::Invalid(format!(
                "The registry holds an Account called `{}`, which is not an \
                 address an Alias or a Group name could be told from, and a \
                 Target that could be either has no single answer.",
                account.key(),
            )));
        }
        // Nothing here about a character a terminal would act on. An address is
        // Claude Code's rather than anybody's choice, so it is refused where it
        // enters and drawn through `Shown` (ADR nothing-drawn-is-obeyed).
    }

    // One entry per Account. `upsert` replaces the matching entry, so two for
    // one address is a hand edit — after which `account` acts on the first,
    // `perch list` renders two rows, and a Cycle counts it twice.
    if let Some((already, again)) = first_collision(registry.accounts.iter().map(Account::key)) {
        return Err(PerchError::Invalid(format!(
            "The registry holds two Accounts spelled `{already}` and `{again}`, \
             which are one Account, so which entry a command reads, and which \
             one it writes, is not decided by anything."
        )));
    }

    refuse_two_names_that_differ_only_in_case(NameKind::Group, registry.groups.keys())?;
    refuse_two_names_that_differ_only_in_case(NameKind::Alias, registry.aliases.keys())?;

    // The percentages a Cycle ranks on. A negative figure gives
    // `cycle::headroom_of` over 100% of headroom, so `used_percent >= threshold`
    // never fires; serde_json already refuses a literal too large for an `f64`.
    for account in &registry.accounts {
        for window in account
            .utilization
            .iter()
            .flat_map(|cached| &cached.windows)
        {
            if !(0.0..=100.0).contains(&window.used_percent) {
                return Err(PerchError::Invalid(format!(
                    "The registry says {} is {}% through its {} window, and a \
                     window is between 0 and 100 percent full, so it is not a \
                     figure a Cycle could rank on.\n\
                     Deleting the Account's `utilization` lets a `perch status \
                     --refresh` read it again.",
                    account.key(),
                    window.used_percent,
                    window.window,
                )));
            }
        }
    }

    Ok(())
}

/// Refuses one of `active`'s pointers into the Accounts naming somebody Perch
/// does not hold. `said` is what the Registry claims about that address, so
/// each of the three pointers a Landing can carry says which one it was.
fn refuse_a_dangling_pointer(registry: &Registry, email: &str, said: &str) -> Result<()> {
    if registry.account(email).is_some() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "The Registry {said}, which is not an Account Perch holds."
    )))
}

/// Refuses a pair of names in one half of the namespace that only case tells
/// apart. Both halves, because the namespace is shared and one copy of the rule
/// is how the two cannot come to disagree about it.
fn refuse_two_names_that_differ_only_in_case<'a>(
    kind: NameKind,
    names: impl Iterator<Item = &'a String>,
) -> Result<()> {
    match first_collision(names.map(String::as_str)) {
        None => Ok(()),
        Some((already, name)) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{already}` and `{name}`, which differ only \
             in case, so which one a Target finds is not decided by anything.",
            kind.article(),
        ))),
    }
}

/// The first pair of names in a sequence that [`same_name`] cannot tell apart,
/// earlier one first. Keyed on [`name::folded`], which two names are one name
/// exactly where they agree on; the alternative is asking `same_name` of
/// everything already seen, which is a scan per name.
fn first_collision<'a>(names: impl Iterator<Item = &'a str>) -> Option<(&'a str, &'a str)> {
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for name in names {
        if let Some(already) = seen.insert(name::folded(name), name) {
            return Some((already, name));
        }
    }
    None
}

/// Refuses a name in the Registry that nothing would have accepted.
///
/// Named rather than repaired: a value only a hand edit can produce is one only a
/// hand edit can take out. The cross-half collision is asked from the Group side
/// only, because every Group is walked either way.
fn refuse_a_name_nothing_would_have_accepted(
    registry: &Registry,
    kind: NameKind,
    name: &str,
) -> Result<()> {
    // `none` is not a case of its own here: `name::validate` already refuses it
    // for both kinds, in words true of a claim and a declaration alike — and
    // this loop walks *declared* Groups too.
    let refused = name::validate(kind, name)
        .err()
        .map(|refusal| refusal.to_string())
        .or_else(|| {
            (kind == NameKind::Group)
                .then(|| registry.declared_alias(name).map(|(held, _)| held))
                .flatten()
                .map(|alias| {
                    format!(
                        "`{alias}` is already an Alias, and Aliases and Group \
                         names share one namespace"
                    )
                })
        });

    match refused {
        None => Ok(()),
        Some(why) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{name}`, which is not a name Perch would \
             have accepted: {why}.",
            kind.article(),
        ))),
    }
}

/// A Registry from outside this Perch, made readable: every claimed Group
/// declared, every `checks` key under its declared spelling, then validated. The
/// pair and never one — `validate` asks `declared_group` about the `checks` key,
/// so validating before normalizing refuses a shape the normalizer repairs. The
/// refusal is undecorated, because a `load` and an Import name different files.
pub fn readable(registry: Registry) -> Result<Registry> {
    let registry = with_every_claimed_group_declared(registry);
    validate(&registry)?;
    Ok(registry)
}

/// Declares any Group an Account claims but nothing declared.
///
/// One nothing declares falls out of `perch list`, which walks the declared
/// Groups and then the Accounts in none (ADR the-listing-owns-the-set). A claim
/// differing only in case joins rather than becoming a second key.
fn with_every_claimed_group_declared(mut registry: Registry) -> Registry {
    let claimed: Vec<String> = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.clone())
        .collect();
    for name in claimed {
        match registry.declared_group(&name) {
            Some(declared) if declared != name => {
                let declared = declared.to_string();
                for account in &mut registry.accounts {
                    if account.group.as_deref().is_some_and(|of| of == name) {
                        account.group = Some(declared.clone());
                    }
                }
            }
            Some(_) => {}
            None => {
                registry
                    .groups
                    .insert(name, crate::config::ScopeSettings::default());
            }
        }
    }
    with_every_check_under_the_declared_spelling(registry)
}

/// The same for what `checks` is keyed on. `checked` and `validate` fold the
/// key and the two mutators remove it exactly, so a key differing only in case
/// outlives its Group and leaves `validate` refusing what `save` just built.
fn with_every_check_under_the_declared_spelling(mut registry: Registry) -> Registry {
    let groups: Vec<String> = registry.groups.keys().cloned().collect();
    for state in registry.runtime.values_mut() {
        let keyed: Vec<String> = state.checks.keys().cloned().collect();
        for name in keyed {
            // The Ungrouped Scope has no declaration to be brought to, so it is
            // brought to the constant `record_switch` writes. Without this the key is
            // the only one in the map that can outlive its own spelling.
            let declared = match means_ungrouped(&name) {
                true => Some(UNGROUPED.to_string()),
                false => groups.iter().find(|group| same_name(group, &name)).cloned(),
            };
            let Some(declared) = declared else {
                continue;
            };
            if declared == name {
                continue;
            }
            if let Some(checked) = state.checks.remove(&name) {
                // The later of the two, where both spellings carry a record: byte
                // order would otherwise decide, and the older one winning is a
                // Check free to Switch inside a Cooldown still running.
                let later = match state.checks.get(&declared) {
                    Some(held) if held.switched_at > checked.switched_at => held.clone(),
                    _ => checked,
                };
                state.checks.insert(declared, later);
            }
        }
    }
    registry
}

/// Writes the Registry, under the hold the caller took to read it.
///
/// The hold is a parameter because a Registry is only ever written by the Perch
/// that read it, and is where [`validate`] is asked on the way out, so what this
/// writes and what [`load`] accepts cannot differ.
pub fn save(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &mut Registry) -> Result<()> {
    perch.renew();
    if !perch.still_held() {
        // A general failure rather than `Busy`, deliberately
        // (ADR a-refusal-is-a-promise): `Busy` promises nothing was changed, and
        // this save is reached as often after a Credential moved as before.
        return Err(PerchError::Other(
            "Another `perch` took the Registry lock over while this command was \
             working, and has changed the Registry since this one read it. \
             Nothing was written, because writing would have undone whatever it \
             did. Run this command again."
                .to_string(),
        ));
    }

    // What `load` will accept, asked before `load` has to refuse it: a command
    // writing a file every later command declined to read would leave a machine
    // with no working `perch` on it, and no `perch holdings purge` either.
    validate(registry).map_err(|invalid| {
        PerchError::Other(format!(
            "{invalid}\n\n{}\n\
             Nothing was written, and the registry on disk is as it was.",
            crate::report::this_is_a_bug(),
        ))
    })?;

    registry.version = CURRENT_VERSION;
    crate::storage::save(host, perch, registry)
}

/// Why there is no active Account, in the terms the way out depends on: holding
/// nothing, a login is the way in; holding Accounts, Perch has merely been left
/// on nobody and naming one is what `perch switch` is for. `because` is what the
/// command wanted an active Account for. One function, because two commands meet
/// this state and only one of them told the difference.
pub fn no_active_account(registry: &Registry, because: &str) -> PerchError {
    if registry.accounts.is_empty() {
        return PerchError::NotFound(format!(
            "Perch holds no Accounts{because}. Run `claude` and log in, then run \
             Perch again."
        ));
    }
    PerchError::NotFound(format!(
        "Perch holds no active Account{because}. `perch switch <target>` makes \
         {} active.",
        match registry.accounts.len() {
            1 => "the one it holds".to_string(),
            held => format!("one of the {held} it holds"),
        }
    ))
}

/// Refuses to act on an Account whose Credential no longer works, in the words of
/// whichever command was asked; `consequence` is what did not happen and why it
/// would have been worse than nothing. One function rather than one per command:
/// `perch run` and `perch switch` meet this state over the same Account and must
/// not describe it in two ways.
pub fn refuse_a_quarantined_account(
    registry: &Registry,
    email: &str,
    consequence: &str,
) -> Result<()> {
    let account = registry.held(email)?;
    match account.quarantine {
        None => Ok(()),
        Some(why) => Err(why.refusal(&registry.named_for_the_user(email), email, consequence)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_WATCHER_THRESHOLD_PERCENT, Strategy};
    use crate::host::Refusing;
    use crate::host::prelude::*;
    use chrono::TimeZone;

    /// The way out turns on what is held, and both commands that meet this state
    /// read the same sentence: a login is the answer only where there is nothing
    /// to switch to.
    #[test]
    fn what_to_do_about_no_active_account_depends_on_what_perch_holds() {
        let empty = Registry::default();
        let said = no_active_account(&empty, "").to_string();
        assert!(said.contains("no Accounts"), "{said}");
        assert!(said.contains("`claude`"), "{said}");

        let mut held = Registry::default();
        held.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let said = no_active_account(&held, ", so there is no Group to Cycle within").to_string();
        assert!(said.contains("no Group to Cycle within"), "{said}");
        assert!(said.contains("the one it holds"), "{said}");
        assert!(
            !said.contains("`claude`"),
            "a login repairs nothing here: {said}"
        );
    }

    #[test]
    fn an_account_is_found_however_its_address_is_capitalized() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "café@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the same Account, spelled the way somebody typed it");

        assert!(registry.account("CAFÉ@example.com").is_some());
        assert!(registry.account_mut("CAFÉ@EXAMPLE.COM").is_some());
        assert_eq!(registry.alias_of("Café@Example.com"), Some("work"));

        registry.settle(Some("CAFÉ@EXAMPLE.COM".into()));
        assert!(
            registry.is_active(&Settled(()), "café@example.com"),
            "and which Account is active is the same question, asked the same \
             way — a dozen call sites compared these by exact bytes, which is \
             the one place in Perch an address was not case-folded"
        );
        assert!(!registry.is_active(&Settled(()), "someone@example.com"));

        registry.forget("CAFÉ@example.com");
        assert!(registry.accounts.is_empty(), "and it is the one that goes");
        assert!(registry.aliases.is_empty(), "with the name it answered to");
    }

    /// Two Accounts, which is the fewest a Landing needs.
    fn holding_two() -> Registry {
        let mut registry = Registry::default();
        for email in ["one@example.com", "two@example.com"] {
            registry.upsert(Account {
                storage_key: None,
                provider: crate::providers::provider::Id::Claude,
                provider_identity: None,
                identity: Identity {
                    email: email.into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        registry
    }

    #[test]
    fn forgetting_the_arriving_half_of_a_landing_comes_back_to_the_one_being_left() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("two@example.com");

        assert_eq!(
            registry.active().whose(),
            Some("one@example.com"),
            "the Account still held is the one the registry is on"
        );
        assert!(
            registry.active().a_switch_in_flight().is_none(),
            "and the Landing is gone with the half that went"
        );
    }

    #[test]
    fn forgetting_the_leaving_half_of_a_landing_settles_on_nobody() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("one@example.com");

        assert_eq!(*registry.active(), Active::Nobody);
    }

    #[test]
    fn an_account_that_was_named_and_is_not_there_is_refused_rather_than_panicked_on() {
        let registry = Registry::default();

        let refused = registry
            .held("nobody@example.com")
            .expect_err("Perch does not hold it");

        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("bug in Perch"),
            "whose fault it is, because there is nothing for a person to do: {refused}"
        );
        assert_eq!(refused.exit_code(), crate::error::EXIT_GENERAL);

        // The mutable half answers the same way: a caller that goes on to change
        // what it finds needs the other borrow.
        let mut registry = Registry::default();
        assert_eq!(
            registry
                .held_mut("nobody@example.com")
                .expect_err("Perch does not hold it")
                .to_string(),
            refused.to_string()
        );
    }

    /// The `active` states here are written to the field rather than through the
    /// transitions, and this is the one place that is right: a dangling pointer
    /// is a state no transition can produce.
    #[test]
    fn an_active_pointer_naming_nothing_is_refused_like_a_dangling_alias() {
        let mut registry = Registry {
            runtime: BTreeMap::from([(
                crate::providers::provider::Id::default(),
                ProviderState {
                    active: Active::Settled("nobody@example.com".to_string()),
                    ..ProviderState::default()
                },
            )]),
            ..Default::default()
        };

        let refused = validate(&registry).expect_err("it names an Account Perch does not hold");
        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );

        registry.state_mut().active = Active::Nobody;
        validate(&registry).expect("holding nothing is a state rather than a fault");
    }

    #[test]
    fn both_ends_of_a_landing_are_refused_when_they_name_nothing() {
        let held = "someone@example.com";
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: held.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        for (what, active, names, says) in [
            (
                "the Account it was switching to",
                Active::Landing {
                    leaving: Some(held.to_string()),
                    arriving: "nobody@example.com".to_string(),
                },
                "nobody@example.com",
                "a Switch to nobody@example.com was under way",
            ),
            (
                "the Account it was leaving",
                Active::Landing {
                    leaving: Some("nobody@example.com".to_string()),
                    arriving: held.to_string(),
                },
                "nobody@example.com",
                "a Switch away from nobody@example.com was under way",
            ),
        ] {
            registry.state_mut().active = active;

            let refused = validate(&registry).expect_err("it names an Account Perch does not hold");

            assert!(refused.to_string().contains(names), "{what}: {refused}");
            assert!(
                refused.to_string().contains(says),
                "{what}: and says which end of the Landing dangles: {refused}"
            );
        }

        // A Landing naming Accounts Perch does hold is a state, not a fault: it
        // is what every interrupted Switch leaves, and the next one resolves it.
        registry.state_mut().active = Active::Landing {
            leaving: None,
            arriving: held.to_string(),
        };
        validate(&registry).expect("a Switch left in flight is a machine to load, not to refuse");
    }

    /// A table, because the rules are one fact with two spellings — asked of the
    /// one function all four callers go through.
    #[test]
    fn what_a_name_may_be_is_one_rule_for_both_halves_of_the_namespace() {
        /// An Account answering to `work`, and a Group called `personal`.
        fn held() -> Registry {
            let mut registry = Registry::default();
            registry.upsert(Account {
                storage_key: None,
                provider: crate::providers::provider::Id::Claude,
                provider_identity: None,
                identity: Identity {
                    email: "someone@example.com".into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
            registry
                .name_account("work", "someone@example.com")
                .expect("the name is free");
            registry.declare_group("personal").expect("so is this one");
            registry
        }

        let cases: &[(NameKind, &str, Option<&str>, Option<&str>)] = &[
            // kind, name, instead_of, the refusal it earns (None = accepted)
            (NameKind::Group, "spare", None, None),
            (NameKind::Alias, "spare", None, None),
            // Its own half.
            (NameKind::Group, "personal", None, Some("already a Group")),
            (NameKind::Alias, "work", None, Some("already names")),
            // Two names that differ only in case are one name.
            (NameKind::Group, "PERSONAL", None, Some("already a Group")),
            (NameKind::Alias, "WORK", None, Some("already names")),
            // The other half, which is what makes the namespace shared.
            (NameKind::Group, "work", None, Some("already an Alias")),
            (
                NameKind::Alias,
                "personal",
                None,
                Some("already a Group name"),
            ),
            // Renaming itself is not colliding with itself, recapitalization
            // included, and the same waiver on both halves.
            (NameKind::Group, "Personal", Some("personal"), None),
            (NameKind::Alias, "Work", Some("work"), None),
            // Shape before collision.
            (NameKind::Group, "", None, Some("cannot be empty")),
            (NameKind::Alias, "", None, Some("cannot be empty")),
            (
                NameKind::Alias,
                "has a space",
                None,
                Some("carries ` ` (U+0020)"),
            ),
            (
                NameKind::Group,
                "none",
                None,
                Some("addresses the Accounts in no Group"),
            ),
            // Not whitespace, and so not caught by the clause above — and the
            // one a terminal reads as an instruction rather than as a name.
            (
                NameKind::Group,
                "\u{1b}[31mred",
                None,
                Some("a control character (U+001B)"),
            ),
            (
                NameKind::Alias,
                "bell\u{7}",
                None,
                Some("a control character (U+0007)"),
            ),
            // `char::is_control` is `Cc` alone, so neither of these was caught
            // by the clause above: the first reverses the rest of the line it is
            // drawn on, and the second is a name drawn identically to `work`.
            (
                NameKind::Group,
                "\u{202e}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+202E)"),
            ),
            (
                NameKind::Alias,
                "wo\u{200b}rk",
                None,
                Some("a character a terminal does not draw as itself (U+200B)"),
            ),
            // The word joiner and the bidi isolates, which are the same harm
            // under two more blocks of the table.
            (
                NameKind::Group,
                "wo\u{2060}rk",
                None,
                Some("a character a terminal does not draw as itself (U+2060)"),
            ),
            (
                NameKind::Group,
                "\u{2066}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+2066)"),
            ),
        ];

        for (kind, name, instead_of, refusal) in cases {
            let asked = held().refuse(Claim::Naming {
                kind: *kind,
                name,
                instead_of: *instead_of,
            });
            match refusal {
                None => asked.unwrap_or_else(|err| {
                    panic!("{kind:?} `{name}` should be free: {err}");
                }),
                Some(said) => {
                    let refused = asked.expect_err(&format!("{kind:?} `{name}` is not free"));
                    assert!(
                        refused.to_string().contains(said),
                        "{kind:?} `{name}`: expected {said:?}, got {refused}"
                    );
                }
            }
        }
    }

    /// The Registry here is built by hand because nothing reachable produces it:
    /// a Group cannot be declared under a name an Alias already holds. It is what
    /// a third way of making a name would walk into.
    #[test]
    fn recapitalizing_an_alias_still_cannot_walk_into_a_group() {
        let mut registry = Registry::default();
        registry
            .aliases
            .insert("work".to_string(), "someone@example.com".to_string());
        registry
            .groups
            .insert("Work".to_string(), Settings::default().into());

        let refused = registry
            .refuse(Claim::Naming {
                kind: NameKind::Alias,
                name: "Work",
                instead_of: Some("work"),
            })
            .expect_err("the shared namespace is still checked");

        assert!(
            refused.to_string().contains("already a Group name"),
            "{refused}"
        );
    }

    /// Nothing reachable produces one, so what is asserted is the guard rather
    /// than the property: what happens on the day a command gets it wrong, and
    /// that the file on disk is left alone.
    #[test]
    fn a_registry_load_would_not_read_is_one_save_declines_to_write() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let path = holdings::registry_path(&host).unwrap();
        save(&host, &mut perch, &mut Registry::default()).expect("an empty one is fine");
        let before = host.file(&path).expect("it was written");

        // A dangling Alias is not a refusal downstream — it is the `expect` in
        // every command that resolves a Target.
        let mut broken = Registry::default();
        broken
            .aliases
            .insert("work".to_string(), "nobody@example.com".to_string());

        let refused = save(&host, &mut perch, &mut broken).expect_err("load would not read it");

        let said = refused.to_string();
        assert!(said.contains("nobody@example.com"), "the rule: {said}");
        assert!(said.contains("bug in Perch"), "whose fault it is: {said}");
        assert!(
            !said.contains("Edit the value there"),
            "and not an instruction to edit a file it did not write: {said}"
        );
        assert_eq!(
            refused.exit_code(),
            crate::error::EXIT_GENERAL,
            "a script told 14 would read it as its own input being wrong"
        );
        assert_eq!(
            host.file(&path).as_deref(),
            Some(before.as_str()),
            "and the registry on disk is untouched, which is the whole of it"
        );
    }

    #[test]
    fn a_command_that_takes_its_time_keeps_the_lock_it_took() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");

        // Past the staleness window several times over: the shape of a
        // `perch remove` waiting on somebody who walked away.
        for _ in 0..4 {
            host.sleep(holdings::REGISTRY_STALE_MILLIS as u64 - 10_000);
            save(&host, &mut perch, &mut Registry::default())
                .expect("it is still Perch's to write");
        }

        assert!(perch.still_held());
        assert!(
            holdings::lock(&host).is_err(),
            "and no other Perch could have taken it in the meantime"
        );
    }

    #[test]
    fn a_registry_read_before_somebody_elses_command_is_not_written_over_theirs() {
        let host = crate::host::FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");

        // The stall, and another Perch finding the lock abandoned and taking it.
        host.sleep(holdings::REGISTRY_STALE_MILLIS as u64 + 1_000);
        let theirs = holdings::lock(&host).expect("an abandoned lock is taken over");
        save(&host, &mut { theirs }, &mut Registry::default()).expect("theirs is the live hold");
        let before = load(&host).expect("it reads").expect("they wrote one");

        let mut stale = Registry {
            runtime: BTreeMap::from([(
                crate::providers::provider::Id::default(),
                ProviderState {
                    active: Active::Settled("someone@example.com".into()),
                    ..ProviderState::default()
                },
            )]),
            ..Registry::default()
        };
        let refused =
            save(&host, &mut perch, &mut stale).expect_err("this one may no longer write");

        assert!(
            refused.to_string().contains("Run this command again"),
            "{refused}"
        );
        assert_eq!(
            load(&host).expect("it reads").expect("a registry is there"),
            before,
            "what the other Perch wrote is what is on disk"
        );
    }

    #[test]
    fn a_check_against_a_group_nothing_declares_is_not_a_registry() {
        let mut registry = Registry::default();
        registry.state_mut().checks.insert(
            "a-group-nobody-declared".to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );

        let refused = validate(&registry).expect_err("the Cooldown paces nothing");
        assert!(
            refused.to_string().contains("a-group-nobody-declared"),
            "it names the entry: {refused}"
        );

        // The Ungrouped Scope keeps one and is not a Group, which is why the
        // fallback that produces these entries exists at all.
        registry.state_mut().checks.clear();
        registry.state_mut().checks.insert(
            UNGROUPED.to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );
        validate(&registry).expect("the Accounts in no Group Cycle too");
    }

    /// `checked` answers with the first in `BTreeMap` order and `record_switch`
    /// writes under the declared spelling, so the record read is not the record
    /// kept — and a Cooldown read off a stale one Switches sooner than 15
    /// minutes.
    #[test]
    fn two_checks_against_one_group_are_not_a_registry() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default().into());
        for spelling in ["Work", "work"] {
            registry
                .state_mut()
                .checks
                .insert(spelling.to_string(), Checked { switched_at: at });
        }

        let refused = validate(&registry).expect_err("one of the two paces nothing");
        let said = refused.to_string();
        assert!(
            said.contains("Work") && said.contains("work"),
            "it names both spellings: {said}"
        );
    }

    /// `checked` and `validate` fold the key and the two mutators remove it
    /// exactly, so a `checks` key that outlives its Group leaves `validate`
    /// refusing what `save` has just built — under a sentence saying the fault
    /// is Perch's, on a Registry no command can then read.
    #[test]
    fn a_check_keyed_in_another_case_than_its_group_is_brought_to_one_on_the_way_in() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default().into());
        registry
            .state_mut()
            .checks
            .insert("Work".to_string(), Checked { switched_at: at });

        let mut registry = readable(registry).expect("one Group, one Check");
        assert_eq!(
            registry.state().checks.keys().collect::<Vec<_>>(),
            vec!["work"],
            "the Check is filed under the spelling the Group was declared under"
        );

        registry.forget_group("work");
        validate(&registry).expect("and it goes when the Group it paces goes");
        assert!(
            registry.state().checks.is_empty(),
            "{:?}",
            registry.state().checks
        );
    }

    /// The order is the whole of the contract: `validate` asks `declared_group`
    /// about the `checks` key, so a Registry judged before it is normalized is
    /// refused over the very Group the normalizer is about to declare.
    #[test]
    fn a_registry_is_normalized_before_it_is_validated_and_never_after() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        // A Group an Account claims and nothing declared, which is the shape
        // `with_every_claimed_group_declared` exists to repair.
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: None,
        });
        registry
            .state_mut()
            .checks
            .insert("work".to_string(), Checked { switched_at: at });

        validate(&registry).expect_err("judged as it arrived, the key names no declared Group");
        readable(registry).expect("through the door, the Group is declared before it is asked for");
    }

    /// The fold happens before `validate`, so the collision it refuses is one
    /// `load` never reaches — it merges instead, and merging by insertion order
    /// lets `"Work"` (0x57) write over `"work"` whichever is fresher.
    #[test]
    fn two_checks_folding_to_one_group_keep_the_later_switch() {
        let noon = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let january = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default().into());
        registry
            .state_mut()
            .checks
            .insert("Work".to_string(), Checked { switched_at: noon });
        registry.state_mut().checks.insert(
            "work".to_string(),
            Checked {
                switched_at: january,
            },
        );

        let registry = with_every_claimed_group_declared(registry);

        assert_eq!(
            registry.checked("work").map(|it| it.switched_at),
            Some(noon),
            "the older record winning is a Check free to Switch at once"
        );
    }

    #[test]
    fn an_alias_points_at_the_address_as_the_registry_holds_it() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: crate::domain::Identity {
                email: "café@example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the name is free and the Account is there");

        assert_eq!(
            registry.aliases.get("work").map(String::as_str),
            Some("café@example.com"),
            "the Alias names the Account, and the Account has one spelling"
        );
    }

    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.config/perch/config.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_a_path_refusing(
                path,
                Refusing::Write,
                "No space left on device (os error 28)",
            );

        // Holding the Account its `active` names, so `validate` passes and the
        // unwritable file is what fails the save: refused at the first step, the
        // two assertions below are true of a write that was never attempted.
        let mut registry = Registry {
            runtime: BTreeMap::from([(
                crate::providers::provider::Id::default(),
                ProviderState {
                    active: Active::Settled("someone@example.com".into()),
                    ..ProviderState::default()
                },
            )]),
            ..Registry::default()
        };
        registry.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let refused = save(&host, &mut perch, &mut registry).expect_err("the write cannot land");
        assert!(
            refused.to_string().contains("No space left on device"),
            "the file is what refused it, rather than the shape: {refused}"
        );

        assert_eq!(
            host.file(path).as_deref(),
            Some(before.as_str()),
            "a reader still sees the registry that was there"
        );
        assert_eq!(
            host.file(crate::host::temp_beside(&host, Path::new(path))),
            None,
            "and the half-written copy is not left beside it"
        );
    }

    #[test]
    fn the_registry_is_written_for_its_owner_alone() {
        let host = crate::host::FakeHost::new();

        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &mut Registry::default()).expect("it is written");

        let path = holdings::registry_path(&host).unwrap();
        assert_eq!(host.mode_of(&path), Some(crate::host::PRIVATE_FILE_MODE));
        assert_eq!(
            host.mode_of(holdings::perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE),
            "a directory others may enter is a directory whose contents others \
             may open"
        );
    }

    #[test]
    fn a_registry_round_trips_through_json() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: Some("Acme".into()),
                organization_uuid: None,
            },
            plan: Some("pro".into()),
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.settle(Some("someone@example.com".into()));

        let json = serde_json::to_string(&registry).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
        assert_eq!(
            back.active_account(&Settled(())).unwrap().plan.as_deref(),
            Some("pro")
        );
    }

    #[test]
    fn what_a_check_recorded_survives_the_file_and_is_absent_until_one_switches() {
        let mut registry = Registry::default();
        assert!(
            !serde_json::to_string(&registry).unwrap().contains("checks"),
            "a machine nothing has been scheduled on records no checks"
        );

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_switch("work", at);
        let back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();

        let recorded = back.checked("work").expect("the Group it Switched within");
        assert_eq!(recorded.switched_at, at);
        assert_eq!(
            back.checked("personal"),
            None,
            "a cooldown is paced per Group, and a Switch within one says \
             nothing about how soon another may move"
        );
    }

    #[test]
    fn forgetting_a_group_forgets_what_a_check_recorded_against_it() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.record_switch("work", Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

        registry.forget_group("work");

        assert_eq!(registry.checked("work"), None);
    }

    #[test]
    fn the_version_is_recorded_so_an_older_build_can_refuse_the_file() {
        let json = serde_json::to_string(&Registry::default()).unwrap();
        assert!(json.contains(&format!("\"version\":{CURRENT_VERSION}")));
    }

    #[test]
    fn a_healthy_account_records_no_quarantine_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("quarantine"), "{json}");

        assert!(registry.quarantine("someone@example.com", Quarantine::RotationLost));
        assert!(
            !registry.quarantine("someone@example.com", Quarantine::NoCredential),
            "the reason kept is how it broke, not the last thing that could not \
             be done to it since"
        );
        let mut back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
        assert_eq!(
            back.release("someone@example.com"),
            Some(Quarantine::RotationLost),
            "the reason survives the round trip, and a login is what ends it"
        );
        assert!(!back.account("someone@example.com").unwrap().quarantined());
    }

    #[test]
    fn an_account_nobody_has_disabled_records_no_disable_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("disabled"), "{json}");

        registry
            .account_mut("someone@example.com")
            .unwrap()
            .disabled = true;
        let written = serde_json::to_string(&registry).unwrap();
        assert!(written.contains(r#""disabled":true"#), "{written}");

        let back: Registry = serde_json::from_str(&written).unwrap();
        assert!(
            back.account("someone@example.com").unwrap().disabled,
            "and it survives the round trip, because it is the half that was said"
        );
    }

    /// `enabled` is `disabled` spelled the other way round.
    #[test]
    fn a_registry_that_still_says_enabled_is_refused_rather_than_read() {
        let held: std::result::Result<Registry, _> = serde_json::from_str(
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"enabled":true}]}"#,
        );

        assert!(held.is_err(), "{held:?}");
    }

    #[test]
    fn nothing_about_where_a_credential_is_kept_is_written_down() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        for derived in ["keychain_service", "keychain_account", "profile", "dir"] {
            assert!(
                !json.contains(derived),
                "a registry that records `{derived}` can disagree with the \
                 derivation it restates: {json}"
            );
        }
    }

    #[test]
    fn a_group_carries_its_configuration_through_json() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.groups.get_mut("work").unwrap().cycle.strategy = Some(Strategy::SoonestReset);

        let json = serde_json::to_string(&registry).unwrap();
        assert!(json.contains("soonest-reset"), "{json}");
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
    }

    #[test]
    fn cycling_among_ungrouped_accounts_is_off_until_it_is_asked_for() {
        assert!(!Registry::default().ungrouped.interchangeable);
        let says_nothing_about_it: Registry =
            serde_json::from_str(&format!("{{\"version\":{CURRENT_VERSION}}}"))
                .expect("a registry with no settings in it");
        assert!(
            !says_nothing_about_it.ungrouped.interchangeable,
            "a registry that says nothing about it reads as off, not as a \
             declaration nobody made (ADR a-group-is-a-declaration)"
        );
    }

    #[test]
    fn a_new_group_holds_the_defaults_and_leaves_the_watcher_alone() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        let work = Scope::Group("work".to_string());

        let settings = registry.settings(&work);
        assert!(!settings.watcher_may_act);
        assert_eq!(settings.strategy, Strategy::MostHeadroom);
        assert_eq!(
            settings.watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT
        );

        registry.ungrouped.settings.watcher.threshold_percent = Some(55);
        assert_eq!(
            registry.settings(&work).watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT,
            "a Setting said about one Scope is said about that Scope: there is \
             no layer for it to arrive at another by"
        );
    }

    #[test]
    fn a_group_declared_later_is_not_reached_by_a_grant_made_earlier() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
            .scope_settings_mut(&Scope::Group("work".to_string()))
            .expect("declared")
            .providers
            .entry(crate::providers::provider::Id::Claude)
            .or_default()
            .watcher
            .enabled = true;
        registry
            .ungrouped
            .settings
            .providers
            .entry(crate::providers::provider::Id::Claude)
            .or_default()
            .watcher
            .enabled = true;

        registry.declare_group("personal").unwrap();

        assert!(
            !registry
                .settings(&Scope::Group("personal".to_string()))
                .watcher_may_act,
            "a Group nobody has said anything about is not one the watcher may \
             act on"
        );
    }

    /// Asserted as the number rather than as the constant, because a default is
    /// a promise made in the docs and a test reading the constant back cannot
    /// notice it change.
    #[test]
    fn the_watchers_policy_has_the_default_it_is_documented_with() {
        assert_eq!(Settings::default().watcher_threshold_percent, 80);
        assert_eq!(Settings::default().watcher_margin_percent, 10);
    }

    #[test]
    fn a_number_out_of_range_is_refused_with_the_range() {
        let cases: [(Settings, &str, &str); 3] = [
            (
                Settings {
                    watcher_threshold_percent: 101,
                    ..Settings::default()
                },
                "watcher-threshold-percent",
                "100",
            ),
            // Zero is the one a margin refuses and a percentage does not: at a
            // margin of nothing an Account is both full enough to leave and
            // clear enough to arrive at.
            (
                Settings {
                    watcher_margin_percent: 0,
                    ..Settings::default()
                },
                "watcher-margin-percent",
                "between 1 and 100",
            ),
            (
                Settings {
                    watcher_margin_percent: 101,
                    ..Settings::default()
                },
                "watcher-margin-percent",
                "between 1 and 100",
            ),
        ];

        let work = Scope::Group("work".to_string());
        for (config, key, accepted) in cases {
            let refusal = config.validate(&work).expect_err("out of range");
            let message = refusal.to_string();
            assert_eq!(refusal.exit_code(), crate::error::EXIT_INVALID, "{message}");
            assert!(message.contains("work"), "{message}");
            assert!(message.contains(key), "{message}");
            assert!(
                message.contains(accepted),
                "a refusal that does not say what would be accepted leaves the \
                 script to guess twice: {message}"
            );
        }

        assert!(Settings::default().validate(&work).is_ok());
        assert!(Settings::default().validate(&Scope::Ungrouped).is_ok());
    }

    #[test]
    fn naming_an_account_that_is_already_named_replaces_the_name() {
        let mut registry = Registry::default();
        registry
            .name_account("overflow", "someone@example.com")
            .expect("the name is free");
        registry
            .name_account("work", "someone@example.com")
            .expect("the Account renames itself");

        assert_eq!(registry.alias_of("someone@example.com"), Some("work"));
        assert!(!registry.aliases.contains_key("overflow"));
        assert_eq!(
            registry.unset_alias("Work"),
            Some(("work".to_string(), "someone@example.com".to_string())),
            "a name is freed however it is capitalized, and says how it was held"
        );
        assert!(registry.unset_alias("work").is_none());
    }

    #[test]
    fn a_group_is_declared_once() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        assert!(registry.declare_group("work").is_err());
        registry
            .ensure_group("work")
            .expect("naming it again in passing is not a conflict");
    }

    /// A script branching on why an Account is Quarantined branches on this
    /// string and nothing else, so every kind needs one and no two may share it.
    #[test]
    fn every_quarantine_has_its_own_machine_readable_name() {
        let every = [
            Quarantine::RenewalRejected,
            Quarantine::RotationLost,
            Quarantine::NoRefreshToken,
            Quarantine::NoCredential,
        ];

        let named: Vec<&str> = every.iter().map(Quarantine::as_str).collect();
        assert_eq!(
            named,
            [
                "renewal-rejected",
                "rotation-lost",
                "no-refresh-token",
                "no-credential"
            ]
        );

        let mut unique = named.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), named.len(), "two Quarantines share a name");

        for why in every {
            assert!(
                !why.because().is_empty(),
                "{why:?} says nothing about itself"
            );
        }
    }

    #[test]
    fn a_registry_that_cannot_be_read_is_a_failure_rather_than_an_empty_perch() {
        let absent = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        assert_eq!(
            load(&absent).expect("a machine Perch has never run on holds nothing"),
            None
        );

        let path = holdings::registry_path(&absent).unwrap();
        let unreadable = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_file(&path, "{}")
            .with_a_path_refusing(&path, Refusing::Read, "permission denied");

        let failed = load(&unreadable).expect_err("a registry that is there must be readable");
        let said = failed.to_string();
        assert!(said.contains("permission denied"), "{said}");
        assert!(
            said.contains(&path.display().to_string()),
            "and it names the file: {said}"
        );
    }

    #[test]
    fn a_groups_settings_are_found_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("Work").expect("a usable name");

        assert!(registry.group("work").is_some());
        assert!(registry.group("WORK").is_some());
        assert!(registry.group("play").is_none());
    }

    #[test]
    fn every_question_about_a_group_is_answered_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.upsert(Account {
            storage_key: None,
            provider: crate::providers::provider::Id::Claude,
            provider_identity: None,
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: None,
        });

        registry
            .scope_settings_mut(&Scope::Group("WORK".to_string()))
            .expect("the Group is declared, whatever it was typed as")
            .watcher
            .threshold_percent = Some(65);
        assert_eq!(
            registry
                .settings(&Scope::Group("Work".to_string()))
                .watcher_threshold_percent,
            65,
            "what was written is what is read back"
        );
        assert_eq!(registry.accounts_in("WORK").len(), 1);

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_switch("WORK", at);
        assert!(
            registry.checked("work").is_some(),
            "one Cooldown record, under the spelling the Group was declared as"
        );

        registry.rename_group("WORK", "office").expect("it renames");
        assert!(registry.group("work").is_none());
        assert_eq!(registry.accounts_in("office").len(), 1);
        assert!(
            registry.checked("office").is_some(),
            "the Cooldown came too"
        );

        registry.forget_group("OFFICE");
        assert!(registry.group("office").is_none());
        assert!(registry.checked("office").is_none());
        // And forgetting one nothing declared is nothing to do.
        registry.forget_group("office");
    }

    #[test]
    fn renaming_a_group_nothing_declared_is_refused_rather_than_panicked_on() {
        let mut registry = Registry::default();

        let error = registry
            .rename_group("work", "office")
            .expect_err("there is no such Group");

        assert!(error.to_string().contains("work"), "{error}");
    }

    #[test]
    fn a_registry_this_build_writes_says_so() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let mut stale = Registry {
            version: 0,
            ..Registry::default()
        };

        save(&host, &mut perch, &mut stale).expect("it writes");

        assert_eq!(
            load(&host).expect("it reads").expect("it is there").version,
            CURRENT_VERSION
        );
    }
}
