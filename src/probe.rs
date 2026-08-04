//! What Perch believes about the installed Claude Code, and how confident it is
//! (ADR 0007).
//!
//! Every path, service-name derivation and struct shape Perch depends on is
//! reverse-engineered and none of it is a public contract. They live here, in
//! one module, and nowhere else — so when Claude Code drifts, there is exactly
//! one place that stops recognising it, and every dangerous operation is gated
//! on the verdict it returns.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::keychain::KeychainError;

/// Named assumptions. A refusal quotes one of these, so the failure a user
/// experiences says which belief stopped holding.
pub mod assumption {
    pub const INSTALLED: &str = "Claude Code is installed and reports a version";
    pub const ACCOUNT_NAME: &str = "the keychain item is stored under the login name";
    pub const CREDENTIAL_SHAPE: &str = "the credential store holds a claudeAiOauth block";
    pub const IDENTITY_BLOCK: &str = "the identity file holds an oauthAccount block";
}

/// The keychain service name Claude Code uses when `CLAUDE_CONFIG_DIR` is
/// unset. Every other config directory gets this plus a hash of its path.
pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";

/// The non-secret description of an Account, as Claude Code records it: what
/// Claude Code displays to say who you are. Perch stores it verbatim, so the
/// registry and the probe describe an Account with one type rather than two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub email: String,
    pub account_uuid: Option<String>,
    pub organization_name: Option<String>,
    pub organization_uuid: Option<String>,
}

/// A Credential, kept as the exact bytes the keychain holds. Perch copies it
/// verbatim and never rewrites it, so the only fields read out are the ones
/// needed to describe the Account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    raw: String,
    pub subscription_type: Option<String>,
    pub expires_at: Option<i64>,
}

impl Credential {
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for Credential {
    /// Never renders the secret. A Credential that reaches a log or an error
    /// message shows its shape and nothing else.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "<credential {} bytes>", self.raw.len())
    }
}

#[derive(Deserialize)]
struct CredentialFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OauthBlock>,
}

#[derive(Deserialize)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct IdentityFile {
    #[serde(rename = "oauthAccount")]
    oauth_account: Option<OauthAccount>,
}

#[derive(Deserialize)]
struct OauthAccount {
    #[serde(rename = "emailAddress")]
    email_address: Option<String>,
    #[serde(rename = "accountUuid")]
    account_uuid: Option<String>,
    #[serde(rename = "organizationName")]
    organization_name: Option<String>,
    #[serde(rename = "organizationUuid")]
    organization_uuid: Option<String>,
}

/// Where the installed Claude Code keeps one Account's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub config_dir: PathBuf,
    pub identity_file: PathBuf,
    pub keychain_service: String,
    pub keychain_account: String,
}

/// What the probe found in the default store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Findings {
    pub version: String,
    pub store: Store,
    pub identity: Identity,
    pub credential: Credential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Everything Perch depends on was found and understood.
    Recognised(Box<Findings>),
    /// Claude Code is installed and recognised, but nobody is logged in.
    NoLogin { version: String, store: Store },
}

/// Reads the installed version, refusing when there is nothing to read.
pub fn claude_version(host: &dyn Host) -> Result<String> {
    let execution = host.exec("claude", &["--version"]).map_err(|err| {
        refusal(
            assumption::INSTALLED,
            &format!("could not run `claude --version`: {err}"),
            "not installed",
        )
    })?;

    if !execution.succeeded() {
        return Err(refusal(
            assumption::INSTALLED,
            &format!("`claude --version` exited {}", execution.status),
            "unknown",
        ));
    }

    // "2.1.221 (Claude Code)" — the leading token is the version.
    let version = execution
        .stdout
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if version.is_empty() {
        return Err(refusal(
            assumption::INSTALLED,
            "`claude --version` printed nothing",
            "unknown",
        ));
    }
    Ok(version)
}

/// The keychain service name for a config directory.
///
/// Claude Code derives it as `Claude Code-credentials-<sha256(dir)[0:8]>`, or
/// the bare name when `CLAUDE_CONFIG_DIR` is unset — which is what gives every
/// Profile a private credential namespace for free (ADR 0001).
pub fn service_name_for(config_dir: &Path, is_default: bool) -> String {
    if is_default {
        DEFAULT_SERVICE.to_string()
    } else {
        format!("{DEFAULT_SERVICE}-{}", short_hash(config_dir))
    }
}

/// The first eight hex characters of the SHA-256 of the directory path.
pub fn short_hash(config_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(config_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// The store Claude Code uses right now, honouring `CLAUDE_CONFIG_DIR`.
pub fn default_store(host: &dyn Host) -> Result<Store> {
    let configured = host.env_var("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    let is_default = configured.is_none();
    let config_dir = configured.unwrap_or_else(|| host.home_dir().join(".claude"));
    Ok(Store {
        identity_file: identity_file_for(&config_dir, is_default, host),
        keychain_service: service_name_for(&config_dir, is_default),
        keychain_account: keychain_account_name(host)?,
        config_dir,
    })
}

/// The store a Perch Profile presents. A Profile is a real config directory, so
/// this is the same derivation with `CLAUDE_CONFIG_DIR` pointed at it.
pub fn store_for_profile(host: &dyn Host, config_dir: &Path) -> Result<Store> {
    Ok(Store {
        identity_file: identity_file_for(config_dir, false, host),
        keychain_service: service_name_for(config_dir, false),
        keychain_account: keychain_account_name(host)?,
        config_dir: config_dir.to_path_buf(),
    })
}

/// `~/.claude.json` for the default directory, `<dir>/.claude.json` otherwise.
fn identity_file_for(config_dir: &Path, is_default: bool, host: &dyn Host) -> PathBuf {
    if is_default {
        host.home_dir().join(".claude.json")
    } else {
        config_dir.join(".claude.json")
    }
}

fn keychain_account_name(host: &dyn Host) -> Result<String> {
    host.env_var("USER").ok_or_else(|| {
        refusal(
            assumption::ACCOUNT_NAME,
            "USER is unset, so the keychain account name cannot be derived",
            "unknown",
        )
    })
}

/// Reads and understands the Credential in a store, or says why it cannot.
pub fn read_credential(
    host: &dyn Host,
    store: &Store,
    version: &str,
) -> Result<Option<Credential>> {
    let raw = match host.keychain_get(&store.keychain_service, &store.keychain_account) {
        Ok(raw) => raw,
        Err(KeychainError::NotFound { .. }) => return Ok(None),
        Err(other) => return Err(PerchError::from(other)),
    };

    let parsed: CredentialFile = serde_json::from_str(&raw).map_err(|err| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!(
                "{} is not JSON Perch understands: {err}",
                store.keychain_service
            ),
            version,
        )
    })?;

    let oauth = parsed.claude_ai_oauth.ok_or_else(|| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!("{} has no claudeAiOauth block", store.keychain_service),
            version,
        )
    })?;

    if oauth.access_token.is_none() {
        return Err(refusal(
            assumption::CREDENTIAL_SHAPE,
            "the claudeAiOauth block has no accessToken",
            version,
        ));
    }

    Ok(Some(Credential {
        raw,
        subscription_type: oauth.subscription_type,
        expires_at: oauth.expires_at,
    }))
}

/// Reads the Identity out of a store's `.claude.json`.
pub fn read_identity(host: &dyn Host, store: &Store, version: &str) -> Result<Option<Identity>> {
    let contents = match host.read_file(&store.identity_file) {
        // No file at all is a machine that has never logged in.
        Err(HostError::NotFound { .. }) => return Ok(None),
        // A file that is there but cannot be read is a different thing
        // entirely, and saying "no account" would be the wrong diagnosis.
        Err(err) => {
            return Err(PerchError::Other(format!(
                "could not read {}: {err}",
                store.identity_file.display()
            )));
        }
        Ok(contents) => contents,
    };

    // The identity file is Claude Code's, not Perch's: one it writes in a shape
    // Perch cannot parse is an assumption failing, not a corrupt file.
    let parsed: IdentityFile = serde_json::from_str(&contents).map_err(|err| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} is not JSON Perch understands: {err}",
                store.identity_file.display()
            ),
            version,
        )
    })?;

    let account = match parsed.oauth_account {
        Some(account) => account,
        None => return Ok(None),
    };

    let email = account.email_address.ok_or_else(|| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} has an oauthAccount with no emailAddress",
                store.identity_file.display()
            ),
            version,
        )
    })?;

    Ok(Some(Identity {
        email,
        account_uuid: account.account_uuid,
        organization_name: account.organization_name,
        organization_uuid: account.organization_uuid,
    }))
}

/// The one question this module exists to answer: what do we believe about the
/// installed Claude Code, and how confident are we?
pub fn probe(host: &dyn Host) -> Result<Verdict> {
    let version = claude_version(host)?;
    let store = default_store(host)?;

    let credential = read_credential(host, &store, &version)?;
    let identity = read_identity(host, &store, &version)?;

    match (credential, identity) {
        (Some(credential), Some(identity)) => Ok(Verdict::Recognised(Box::new(Findings {
            version,
            store,
            identity,
            credential,
        }))),
        (Some(_), None) => Err(refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "a credential is stored but {} names no account",
                store.identity_file.display()
            ),
            &version,
        )),
        // An identity with no credential is a logged-out machine that still
        // remembers who it was: nothing to adopt.
        (None, _) => Ok(Verdict::NoLogin { version, store }),
    }
}

fn refusal(assumption: &str, detail: &str, version: &str) -> PerchError {
    PerchError::ProbeRefused {
        assumption: assumption.to_string(),
        detail: detail.to_string(),
        version: version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_directory_uses_the_bare_service_name() {
        assert_eq!(
            service_name_for(Path::new("/Users/someone/.claude"), true),
            "Claude Code-credentials"
        );
    }

    #[test]
    fn every_other_directory_gets_a_hash_of_its_path() {
        let service = service_name_for(Path::new("/Users/someone/.perch/profiles/a"), false);
        let hash = service.strip_prefix("Claude Code-credentials-").unwrap();
        assert_eq!(hash.len(), 8);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn two_directories_get_two_namespaces() {
        let one = service_name_for(Path::new("/Users/someone/.perch/profiles/a"), false);
        let two = service_name_for(Path::new("/Users/someone/.perch/profiles/b"), false);
        assert_ne!(one, two);
    }

    #[test]
    fn the_hash_is_the_first_eight_hex_characters_of_the_sha256_of_the_path() {
        // Independently computed:
        //   printf '%s' "/tmp/perch-fixture" | shasum -a 256 | cut -c1-8
        // Pinned so a change to the derivation — which would send every stored
        // Credential to a namespace no Profile is ever read from — has to be a
        // deliberate edit rather than an accident.
        assert_eq!(short_hash(Path::new("/tmp/perch-fixture")), "1b3b8f67");
        assert_eq!(short_hash(Path::new("/Users/someone/.claude")), "b38b2c3b");
    }

    #[test]
    fn a_credential_never_renders_its_secret() {
        let credential = Credential {
            raw: r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret"}}"#.into(),
            subscription_type: None,
            expires_at: None,
        };
        let rendered = format!("{credential}");
        assert!(!rendered.contains("sk-ant-oat01-secret"));
    }
}
