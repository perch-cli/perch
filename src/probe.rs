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
use crate::json;
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

    understand_credential(raw, &store.keychain_service, version).map(Some)
}

/// Makes sense of the bytes a keychain namespace holds, or says which belief
/// they broke. `held_in` names the namespace they came out of, so a refusal
/// says which Account's store stopped being recognisable.
pub fn understand_credential(raw: String, held_in: &str, version: &str) -> Result<Credential> {
    let parsed: CredentialFile = serde_json::from_str(&raw).map_err(|err| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!("{held_in} is not JSON Perch understands: {err}"),
            version,
        )
    })?;

    let oauth = parsed.claude_ai_oauth.ok_or_else(|| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!("{held_in} has no claudeAiOauth block"),
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

    Ok(Credential {
        raw,
        subscription_type: oauth.subscription_type,
        expires_at: oauth.expires_at,
    })
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

/// One of the locks Claude Code takes around its own credential work, with the
/// parameters it treats that lock under.
///
/// A lock artifact is a **directory**, because `mkdir` either succeeds or fails
/// with nothing in between. It is held by whoever created it and understood to
/// be abandoned once its modification time is older than `stale_millis`; a
/// holder says it is still there by touching it every `update_millis`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockSpec {
    /// How the lock is named when Perch has to say it could not take one.
    pub name: &'static str,
    pub dir: PathBuf,
    pub stale_millis: i64,
    pub update_millis: i64,
}

/// The staleness and update intervals Claude Code uses for the two OAuth
/// refresh locks.
const REFRESH_STALE_MILLIS: i64 = 60_000;
const REFRESH_UPDATE_MILLIS: i64 = 5_000;
/// The config file lock is taken for a single read-modify-write and is treated
/// as abandoned far sooner.
const CONFIG_STALE_MILLIS: i64 = 10_000;
const CONFIG_UPDATE_MILLIS: i64 = 5_000;

/// The locks a Switch runs inside, in the order Claude Code takes them.
///
/// The order is the contract with a concurrently-running Claude Code, not a
/// preference: taking them in any other order is how two processes each hold
/// one of the pair and neither can proceed. Under these locks Claude Code's
/// double-checked re-read sees a swapped, non-expired Credential and abandons
/// its own refresh, which is what makes swapping safe at all.
pub fn locks_for(store: &Store) -> Vec<LockSpec> {
    let legacy = {
        let mut path = store.config_dir.clone().into_os_string();
        path.push(".lock");
        PathBuf::from(path)
    };
    let mut config_file = store.identity_file.clone().into_os_string();
    config_file.push(".lock");

    vec![
        LockSpec {
            name: "the refresh lock",
            dir: store.config_dir.join(".oauth_refresh.lock"),
            stale_millis: REFRESH_STALE_MILLIS,
            update_millis: REFRESH_UPDATE_MILLIS,
        },
        LockSpec {
            name: "the legacy config-home lock",
            dir: legacy,
            stale_millis: REFRESH_STALE_MILLIS,
            update_millis: REFRESH_UPDATE_MILLIS,
        },
        LockSpec {
            name: "the config file lock",
            dir: PathBuf::from(config_file),
            stale_millis: CONFIG_STALE_MILLIS,
            update_millis: CONFIG_UPDATE_MILLIS,
        },
    ]
}

/// Where Claude Code records the sessions it is running: one `<pid>.json` per
/// client, in the config directory it was launched against.
pub fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("sessions")
}

/// The processes running against a config directory right now.
///
/// A marker file names its process in its own name, and Claude Code leaves them
/// behind when it dies — so a marker is evidence of a client only when the
/// process it names is still alive. Anything unreadable is treated as no
/// evidence: a Profile is Live when something says so, not when nothing does.
pub fn live_clients(host: &dyn Host, config_dir: &Path) -> Vec<u32> {
    let markers = match host.list_dir(&sessions_dir(config_dir)) {
        Ok(markers) => markers,
        Err(_) => return Vec::new(),
    };

    markers
        .iter()
        .filter_map(|marker| {
            let name = marker.file_name()?.to_str()?;
            let pid: u32 = name.strip_suffix(".json")?.parse().ok()?;
            host.process_alive(pid).then_some(pid)
        })
        .collect()
}

/// The key of `.claude.json` that says who the Account is. The only key of that
/// file Perch ever writes (ADR 0001).
pub const IDENTITY_KEY: &str = "oauthAccount";

/// A file Claude Code would read, holding this Account and nothing else. What
/// it writes for itself on a machine that has never run it.
pub fn fresh_identity_file(block: &str) -> String {
    let indented = block.replace('\n', "\n  ");
    format!("{{\n  \"{IDENTITY_KEY}\": {indented}\n}}\n")
}

/// Rewrites the `oauthAccount` block of an identity file, leaving every other
/// byte of it exactly as it was.
///
/// `.claude.json` also holds project history, MCP configuration and settings,
/// none of which belong to the Account (ADR 0001). Leaving them in place is
/// what makes them follow the person across a Switch for free — so this splices
/// one value ([`crate::json`]) rather than parsing the file and writing it
/// back, which would reorder keys and reformat values Perch has no business
/// touching.
pub fn patch_oauth_account(
    contents: &str,
    block: &str,
    path: &Path,
    version: &str,
) -> Result<String> {
    json::replace_object_at(contents, IDENTITY_KEY, block).ok_or_else(|| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!("{} has no oauthAccount block to patch", path.display()),
            version,
        )
    })
}

/// The `oauthAccount` block of an identity file, exactly as it is written
/// there. An Account's own Profile holds the block Claude Code wrote for it,
/// which carries fields beyond the Identity Perch records — so a Switch
/// prefers it and composes one only when there is none.
pub fn oauth_account_block(contents: &str) -> Option<&str> {
    json::object_at(contents, IDENTITY_KEY)
}

impl Identity {
    /// The `oauthAccount` block Claude Code would write for this Account, for
    /// the Accounts whose Profile holds no identity file of its own.
    pub fn oauth_account_block(&self) -> String {
        let mut block = serde_json::Map::new();
        if let Some(uuid) = &self.account_uuid {
            block.insert("accountUuid".into(), uuid.clone().into());
        }
        block.insert("emailAddress".into(), self.email.clone().into());
        if let Some(organization) = &self.organization_name {
            block.insert("organizationName".into(), organization.clone().into());
        }
        if let Some(uuid) = &self.organization_uuid {
            block.insert("organizationUuid".into(), uuid.clone().into());
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(block))
            .expect("a map of strings serialises")
    }
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

    const IDENTITY_FILE: &str = r#"{
  "numStartups": 41,
  "oauthAccount": {
    "accountUuid": "account-uuid-1",
    "emailAddress": "someone@example.com",
    "organizationRole": "admin"
  },
  "projects": {
    "/Users/someone/work": {
      "allowedTools": ["Bash(git status)"],
      "oauthAccount": "not the one"
    }
  }
}"#;

    #[test]
    fn patching_the_identity_leaves_every_other_byte_alone() {
        let patched = patch_oauth_account(
            IDENTITY_FILE,
            "{\n  \"emailAddress\": \"overflow@example.com\"\n}",
            Path::new("/Users/someone/.claude.json"),
            "2.1.221",
        )
        .expect("the block is there to patch");

        assert!(patched.contains(r#""emailAddress": "overflow@example.com""#));
        assert!(!patched.contains("someone@example.com"));
        // Everything that is not the Account: project history, and the settings
        // that live beside it, down to their formatting.
        assert!(patched.contains(r#"  "numStartups": 41,"#));
        assert!(patched.contains(r#"      "allowedTools": ["Bash(git status)"],"#));
        assert!(patched.contains(r#"      "oauthAccount": "not the one""#));
        // The replacement is written at the indentation of the block it
        // replaces, so the file reads as one file afterwards.
        assert!(
            patched.contains("  \"oauthAccount\": {\n    \"emailAddress\""),
            "{patched}"
        );
        serde_json::from_str::<serde_json::Value>(&patched).expect("still JSON");
    }

    #[test]
    fn a_block_that_came_from_a_file_of_its_own_is_written_at_the_indentation_here() {
        // Exactly what taking a block out of another Profile's `.claude.json`
        // hands over: already indented, for a file that is not this one.
        let from_elsewhere = "{\n    \"emailAddress\": \"overflow@example.com\"\n  }";

        let patched = patch_oauth_account(
            IDENTITY_FILE,
            from_elsewhere,
            Path::new("/Users/someone/.claude.json"),
            "2.1.221",
        )
        .expect("the block is there to patch");

        assert!(
            patched.contains(
                "  \"oauthAccount\": {\n    \"emailAddress\": \"overflow@example.com\"\n  },"
            ),
            "a block copied between files does not step further right each time: {patched}"
        );
    }

    #[test]
    fn a_file_with_no_block_to_patch_is_a_refusal_naming_the_assumption() {
        let error = patch_oauth_account(
            r#"{"numStartups": 41}"#,
            "{}",
            Path::new("/Users/someone/.claude.json"),
            "2.1.221",
        )
        .unwrap_err();
        assert!(
            matches!(error, PerchError::ProbeRefused { ref assumption, .. }
                if assumption == assumption::IDENTITY_BLOCK),
            "{error}"
        );
    }

    #[test]
    fn the_block_taken_from_a_file_is_the_top_level_one_verbatim() {
        let block = oauth_account_block(IDENTITY_FILE).expect("there is one");
        assert!(block.starts_with('{') && block.ends_with('}'));
        assert!(block.contains(r#""organizationRole": "admin""#));
        assert!(!block.contains("not the one"));
        assert_eq!(oauth_account_block(r#"{"projects": {}}"#), None);
    }

    #[test]
    fn a_composed_block_carries_what_the_identity_knows_and_no_nulls() {
        let block = Identity {
            email: "someone@example.com".into(),
            account_uuid: Some("account-uuid-1".into()),
            organization_name: None,
            organization_uuid: None,
        }
        .oauth_account_block();

        assert!(block.contains(r#""emailAddress": "someone@example.com""#));
        assert!(block.contains(r#""accountUuid": "account-uuid-1""#));
        assert!(!block.contains("organization"), "{block}");
    }

    #[test]
    fn the_locks_are_the_three_claude_code_takes_in_the_order_it_takes_them() {
        let store = Store {
            config_dir: PathBuf::from("/Users/someone/.claude"),
            identity_file: PathBuf::from("/Users/someone/.claude.json"),
            keychain_service: DEFAULT_SERVICE.to_string(),
            keychain_account: "someone".to_string(),
        };

        let dirs: Vec<PathBuf> = locks_for(&store).into_iter().map(|lock| lock.dir).collect();
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/Users/someone/.claude/.oauth_refresh.lock"),
                PathBuf::from("/Users/someone/.claude.lock"),
                PathBuf::from("/Users/someone/.claude.json.lock"),
            ]
        );
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
