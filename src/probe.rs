//! What Perch believes about the installed Claude Code, and how confident it is
//! (ADR 0007).
//!
//! Every path, service-name derivation and struct shape Perch depends on is
//! reverse-engineered and none of it is a public contract. They live here, in
//! one module, and nowhere else — so when Claude Code drifts, there is exactly
//! one place that stops recognising it, and every dangerous operation is gated
//! on the verdict it returns.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::json;

/// Named assumptions. A refusal quotes one of these, so the failure a user
/// experiences says which belief stopped holding.
pub mod assumption {
    pub const INSTALLED: &str = "Claude Code is installed and reports a version";
    pub const ACCOUNT_NAME: &str = "the keychain item is stored under the login name";
    pub const CREDENTIAL_SHAPE: &str = "the credential store holds a claudeAiOauth block";
    pub const CREDENTIAL_LOCATION: &str = "a Credential is kept in the keychain namespace, or the file, that the \
         config directory derives";
    pub const IDENTITY_BLOCK: &str = "the identity file holds an oauthAccount block";
    pub const SESSION_MARKER: &str =
        "a session marker names its process and when the session started";
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
/// verbatim, so the only fields read out are the ones something needs: to
/// describe the Account, to ask Anthropic a question as it, and to renew it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    raw: String,
    /// What proves the caller is this Account for the length of a session.
    pub access_token: String,
    /// What buys a fresh access token, and what Anthropic retires when it
    /// Rotates one.
    pub refresh_token: Option<String>,
    pub subscription_type: Option<String>,
    /// When the access token stops being accepted, in milliseconds.
    pub expires_at: Option<i64>,
}

/// How long before a Credential expires Perch stops relying on it.
///
/// A token that expires while a request is in flight is a request wasted
/// against a budget that does not refill early (ADR 0015), and the margin costs
/// nothing: it moves a Rotation Perch was about to need anyway.
pub const EXPIRY_MARGIN_SECONDS: i64 = 60;

impl Credential {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether the access token can still be asked a question at `now`.
    ///
    /// A Credential that says nothing about when it expires is taken at its
    /// word: Perch has no evidence it has run out, and Rotating one that has
    /// not spends the only refresh token there is for nothing.
    pub fn usable_at(&self, now: DateTime<Utc>) -> bool {
        match self.expires_at {
            Some(millis) => millis - now.timestamp_millis() > EXPIRY_MARGIN_SECONDS * 1_000,
            None => true,
        }
    }
}

impl std::fmt::Display for Credential {
    /// Never renders the secret. A Credential that reaches a log or an error
    /// message shows its shape and nothing else.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "<credential {} bytes>", self.raw.len())
    }
}

impl std::fmt::Debug for Credential {
    /// The same, because `Debug` is the likelier of the two to reach a log by
    /// accident. A derived one would print every field — the refresh token
    /// among them — and defeat the `Display` above from one `{:?}` away, which
    /// is a formatting specifier away from every `assert_eq!`, `panic!` and
    /// `unwrap` in the tree.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
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
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
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

/// Where the installed Claude Code keeps one Account's configuration: its
/// Identity, and both of the Credential Stores it might hold a Credential in
/// (ADR 0020).
///
/// Not itself a Credential Store, despite the name it has carried since before
/// there were two of them: this is the config directory and everything derived
/// from it, and [`crate::credentials::CredentialStore`] is the glossary's term
/// for the places a Credential goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub config_dir: PathBuf,
    pub identity_file: PathBuf,
    pub keychain_service: String,
    pub keychain_account: String,
    /// The plaintext store: the primary off macOS, and the fallback on it.
    pub credentials_file: PathBuf,
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

/// The Claude Code binary Perch runs, resolved by Perch rather than left to
/// `Command::new`: Rust appends only `.exe` and never consults `PATHEXT`, so
/// the `claude.cmd` that `npm i -g @anthropic-ai/claude-code` installs works
/// in every shell and would be invisible to a bare `Command::new("claude")`.
///
/// `$PERCH_CLAUDE_BIN` overrides the search and passes through verbatim.
pub fn claude_bin(host: &dyn Host) -> Result<PathBuf> {
    if let Some(overridden) = host.env_var("PERCH_CLAUDE_BIN") {
        return Ok(PathBuf::from(overridden));
    }

    let Some(path) = host.env_var("PATH") else {
        return Err(refusal(
            assumption::INSTALLED,
            "PATH is unset, so there is nowhere to look for `claude`. \
             Point PERCH_CLAUDE_BIN at it instead",
            "not installed",
        ));
    };

    let on_windows = host.platform() == crate::host::Platform::Windows;
    let separator = if on_windows { ';' } else { ':' };
    // What makes a name executable on Windows is carrying one of PATHEXT's
    // extensions. Spelled lowercase because that is how npm writes
    // `claude.cmd`, and the real filesystem answers case-insensitively anyway.
    let extensions: Vec<String> = if on_windows {
        host.env_var("PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };

    for dir in path.split(separator).filter(|dir| !dir.is_empty()) {
        for extension in &extensions {
            // Joined with '/' rather than `Path::join`, which would pick the
            // separator of whatever platform this build runs on. Windows
            // accepts either, and a candidate that spells differently per
            // build would make one machine read as two.
            let candidate = PathBuf::from(format!("{dir}/claude{extension}"));
            if host.is_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(refusal(
        assumption::INSTALLED,
        "no `claude` was found on PATH. Install Claude Code, or point \
         PERCH_CLAUDE_BIN at it",
        "not installed",
    ))
}

/// Reads the installed version, refusing when there is nothing to read.
pub fn claude_version(host: &dyn Host) -> Result<String> {
    let claude = claude_bin(host)?;
    let execution = host
        .exec(&claude.to_string_lossy(), &["--version"])
        .map_err(|err| {
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

    // "2.1.221 (Claude Code)" — the leading token is the version. Taken only
    // when it looks like one: `claude` printing "error: ..." on a clean exit
    // would otherwise become a version called `error:`, quoted back in every
    // refusal that names which Claude Code Perch was talking to.
    let version = execution.stdout.split_whitespace().next().unwrap_or("");
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(refusal(
            assumption::INSTALLED,
            &format!(
                "`claude --version` printed {}",
                match execution.stdout.trim() {
                    "" => "nothing".to_string(),
                    printed => format!("`{printed}`, which is not a version"),
                }
            ),
            "unknown",
        ));
    }
    Ok(version.to_string())
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
///
/// The default directory sits under home, and its identity file sits beside
/// it as `~/.claude.json` — so no home is a refusal here, where every other
/// config directory carries its own identity file and needs no home at all.
pub fn default_store(host: &dyn Host) -> Result<Store> {
    match host.env_var("CLAUDE_CONFIG_DIR").map(PathBuf::from) {
        Some(config_dir) => store_for_directory(host, config_dir, false),
        None => store_for_directory(host, default_config_dir(host)?, true),
    }
}

/// The store of the Default Profile as it stands when nothing has pointed this
/// process anywhere else — `~/.claude`, with its identity file beside it.
///
/// Distinct from [`default_store`] in exactly the case a Run creates: a client
/// launched against a Profile passes `CLAUDE_CONFIG_DIR` on to everything it
/// starts, so a Perch run inside one is told a Profile is the default. It is
/// not, and the two commands that read the person's own state — Reconcile and
/// the Carry — have to reach past that.
pub fn default_profile_store(host: &dyn Host) -> Result<Store> {
    store_for_directory(host, default_config_dir(host)?, true)
}

/// The directory Claude Code falls back to when it is told nothing — the
/// Default Profile as the glossary means it, whatever this process's
/// environment happens to say.
///
/// Distinct from [`default_store`], which answers for the directory Claude Code
/// would use *right now*. The two differ exactly when `CLAUDE_CONFIG_DIR` is
/// set, and a Run is where that difference has to be reckoned with, because a
/// Run is what sets it (see [`crate::commands::run`]).
pub fn default_config_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(home(host)?.join(".claude"))
}

/// The home directory, as a command failure rather than a Host one.
fn home(host: &dyn Host) -> Result<PathBuf> {
    host.home_dir()
        .map_err(|err| PerchError::Other(err.to_string()))
}

/// The store a Perch Profile presents. A Profile is a real config directory, so
/// this is the same derivation with `CLAUDE_CONFIG_DIR` pointed at it.
pub fn store_for_profile(host: &dyn Host, config_dir: &Path) -> Result<Store> {
    store_for_directory(host, config_dir.to_path_buf(), false)
}

fn store_for_directory(host: &dyn Host, config_dir: PathBuf, is_default: bool) -> Result<Store> {
    Ok(Store {
        identity_file: identity_file_for(&config_dir, is_default, host)?,
        keychain_service: service_name_for(&config_dir, is_default),
        keychain_account: keychain_account_name(host)?,
        credentials_file: credentials_file_for(&config_dir),
        config_dir,
    })
}

/// What the plaintext Credential Store is called, inside whichever config
/// directory it belongs to.
pub const CREDENTIALS_FILE: &str = ".credentials.json";

/// The plaintext store for a config directory.
///
/// Always inside the directory, unlike the identity file, which sits beside the
/// default one as `~/.claude.json`. That difference is Claude Code's, not
/// Perch's: the 2.1.222 build joins its config directory to
/// `.credentials.json` and nothing else, which is what gives each Profile a
/// Credential of its own off macOS (ADR 0020).
///
/// This and [`service_name_for`] are the two halves of
/// [`assumption::CREDENTIAL_LOCATION`], and the whole of what a Profile's
/// privacy rests on. Neither is probed at runtime — a store that holds nothing
/// is a logged-out Profile, not a broken belief — so what guards them is the
/// contract suite (ADR 0007).
pub fn credentials_file_for(config_dir: &Path) -> PathBuf {
    config_dir.join(CREDENTIALS_FILE)
}

/// What the identity file is called, wherever it sits: beside the default
/// config directory as `~/.claude.json`, and inside every other one.
///
/// Named here because it is one of the two entries a Reconcile refuses to share
/// (ADR 0026), and the name of a file Claude Code writes belongs in the module
/// that knows about Claude Code rather than in the one that enumerates a
/// directory.
pub const IDENTITY_FILE: &str = ".claude.json";

/// `~/.claude.json` for the default directory, `<dir>/.claude.json` otherwise.
fn identity_file_for(config_dir: &Path, is_default: bool, host: &dyn Host) -> Result<PathBuf> {
    if is_default {
        Ok(home(host)?.join(IDENTITY_FILE))
    } else {
        Ok(identity_file_in(config_dir))
    }
}

/// The identity file of a config directory that is not the default one — every
/// Profile, which is every directory Perch itself made.
pub fn identity_file_in(config_dir: &Path) -> PathBuf {
    config_dir.join(IDENTITY_FILE)
}

/// The login name a keychain item is stored under. `USERNAME` is Windows'
/// spelling of `USER`; off macOS the name is carried in the Store but never
/// looked up, since the keychain there is a store that holds nothing
/// (ADR 0020).
fn keychain_account_name(host: &dyn Host) -> Result<String> {
    host.env_var("USER")
        .or_else(|| host.env_var("USERNAME"))
        .ok_or_else(|| {
            refusal(
                assumption::ACCOUNT_NAME,
                "neither USER nor USERNAME is set, so the keychain account name \
                 cannot be derived",
                "unknown",
            )
        })
}

/// Reads and understands the Credential a config directory holds, from
/// whichever of its two Credential Stores holds one (ADR 0020), or says why it
/// cannot.
pub fn read_credential(
    host: &dyn Host,
    store: &Store,
    version: &str,
) -> Result<Option<Credential>> {
    let Some(held) = credentials::read(host, store)? else {
        return Ok(None);
    };

    understand_credential(held.credential, &held.kept_in.describe(), version).map(Some)
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

    let Some(access_token) = oauth.access_token else {
        return Err(refusal(
            assumption::CREDENTIAL_SHAPE,
            "the claudeAiOauth block has no accessToken",
            version,
        ));
    };

    Ok(Credential {
        raw,
        access_token,
        refresh_token: oauth.refresh_token,
        subscription_type: oauth.subscription_type,
        expires_at: oauth.expires_at,
    })
}

/// The key of the credential store that holds the Credential itself.
pub const CREDENTIAL_KEY: &str = "claudeAiOauth";

/// The bytes to store after a Rotation: this Credential with the three fields a
/// renewal replaces, and everything else about it left as it was.
///
/// The rest of the block is Claude Code's — the scopes it asked for, the
/// subscription it recorded — and none of it is Perch's to decide. Unlike
/// `.claude.json`, which is spliced as text because it holds a person's work
/// (ADR 0001), a Credential is one small object that only ever holds an
/// Account's tokens, so it is read and written as JSON rather than patched
/// blind.
pub fn credential_after_rotation(
    current: &Credential,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: Option<i64>,
    version: &str,
) -> Result<String> {
    let mut document: serde_json::Value =
        serde_json::from_str(current.as_str()).map_err(|err| {
            refusal(
                assumption::CREDENTIAL_SHAPE,
                &format!("the Credential being renewed is not JSON Perch understands: {err}"),
                version,
            )
        })?;

    let block = document
        .get_mut(CREDENTIAL_KEY)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            refusal(
                assumption::CREDENTIAL_SHAPE,
                "the Credential being renewed has no claudeAiOauth block",
                version,
            )
        })?;

    block.insert("accessToken".into(), access_token.into());
    // A refresh that hands back no new refresh token has not Rotated: the one
    // already stored is still the live one, and replacing it with nothing would
    // throw away the only way back.
    if let Some(token) = refresh_token {
        block.insert("refreshToken".into(), token.into());
    }
    if let Some(at) = expires_at {
        block.insert("expiresAt".into(), at.into());
    }

    serde_json::to_string(&document)
        .map_err(|err| PerchError::Other(format!("could not write the renewed Credential: {err}")))
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

    // An address is what every Profile path, keychain namespace, Alias target
    // and Group membership is keyed on, so one with nothing nameable in it is
    // refused where it enters rather than somewhere downstream that has already
    // derived a path from it.
    if !email.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err(refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} names the account `{email}`, which has no character Perch \
                 can name a Profile after",
                store.identity_file.display()
            ),
            version,
        ));
    }

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
    /// Whose lock it is, for the same message: quitting the right program is
    /// the whole of the advice a contended lock can give.
    pub held_by: &'static str,
    pub dir: PathBuf,
    pub stale_millis: i64,
    pub update_millis: i64,
    /// What it costs to have lost this one, said to the user when a renewal
    /// finds it gone. A takeover means something different for each lock — a
    /// Switch under Claude Code's locks carries on, where a command that has
    /// lost Perch's own registry lock stops — and the sentence that explains it
    /// belongs beside the lock rather than in the code that renews them all.
    pub lost_means: &'static str,
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

    // Stopping half way through writing a Credential is worse than finishing,
    // so a Switch that loses one of these carries on and says so.
    const CARRIES_ON: &str = "Something else may be writing the same Credential. \
                              Perch is finishing what it started rather than \
                              stopping half way; check the Account you land on.";

    vec![
        LockSpec {
            name: "the refresh lock",
            held_by: "Claude Code",
            dir: store.config_dir.join(".oauth_refresh.lock"),
            stale_millis: REFRESH_STALE_MILLIS,
            update_millis: REFRESH_UPDATE_MILLIS,
            lost_means: CARRIES_ON,
        },
        LockSpec {
            name: "the legacy config-home lock",
            held_by: "Claude Code",
            dir: legacy,
            stale_millis: REFRESH_STALE_MILLIS,
            update_millis: REFRESH_UPDATE_MILLIS,
            lost_means: CARRIES_ON,
        },
        LockSpec {
            name: "the config file lock",
            held_by: "Claude Code",
            dir: PathBuf::from(config_file),
            stale_millis: CONFIG_STALE_MILLIS,
            update_millis: CONFIG_UPDATE_MILLIS,
            lost_means: CARRIES_ON,
        },
    ]
}

/// What the directory of session markers is called inside a config directory.
///
/// Named here rather than spelled inline because two modules need it for
/// opposite reasons: this one derives the markers it reads, and
/// [`crate::reconcile`] holds it back from crossing. A directory whose contents
/// answer "is a client running *here*" is meaningless shared between config
/// directories — every Profile would report every other Profile's clients.
pub const SESSIONS: &str = "sessions";

/// Where Claude Code records the sessions it is running: one `<pid>.json` per
/// client, in the config directory it was launched against.
pub fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(SESSIONS)
}

/// The marker for one process running against a config directory.
pub fn session_marker_at(config_dir: &Path, pid: u32) -> PathBuf {
    sessions_dir(config_dir).join(format!("{pid}.json"))
}

/// The marker a Run writes to say a Profile is Live, in the shape Claude Code
/// writes one and this module reads one back (ADR 0022).
///
/// Three fields and no more: the two that make it evidence — the process it
/// names, and when the session began — and one that says who wrote it, so
/// somebody who finds a stale marker knows what left it. Claude Code's own
/// carries a good deal more, none of it read here, and inventing a `sessionId`
/// or a version Perch does not have would be a file claiming to be something it
/// is not.
///
/// `started_at` is the moment the Run began rather than the moment the process
/// did. That is what makes the file corroborate itself: the process it names
/// began strictly earlier, so the marker holds for as long as that process
/// lives and for no longer — a pid recycled after this was written necessarily
/// belongs to a process younger than it.
pub fn session_marker(pid: u32, started_at: DateTime<Utc>) -> String {
    serde_json::json!({
        "pid": pid,
        "startedAt": started_at.timestamp_millis(),
        "writtenBy": "perch",
    })
    .to_string()
}

/// The marker Claude Code writes for a running session, to the extent Perch
/// reads it. `startedAt` is when the session began, in milliseconds since the
/// epoch — which means the same thing on every platform, and is why it is the
/// field matched rather than the platform-encoded `procStart` (ADR 0022).
#[derive(Deserialize)]
struct SessionMarker {
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
}

/// The processes running against a config directory right now.
///
/// A marker file names its process in its own name, and Claude Code leaves them
/// behind when it dies — so a marker is evidence of a client only when it is
/// corroborated (ADR 0022): the process it names began no later than the marker
/// says the session did. A recycled PID necessarily belongs to a process that
/// began after the marker was written, so the check is exact rather than
/// heuristic. A marker that cannot be read or understood is no evidence at all:
/// a Profile is Live when something says so, not when nothing does.
///
/// One situation is neither belief nor dismissal — a running process whose
/// start the operating system will not say. That marker can be neither
/// corroborated nor dismissed, and guessing either way is silently wrong on
/// one side or the other, so the answer is a refusal naming the assumption.
pub fn live_clients(host: &dyn Host, config_dir: &Path, version: &str) -> Result<Vec<u32>> {
    clients_in(host, config_dir)
        .map_err(|unsure| refusal(assumption::SESSION_MARKER, &unsure.detail(), version))
}

/// Why whether anything is running went unanswered.
///
/// Both are doubt rather than an answer, and neither is decided here: a caller
/// that must not write under a client reads either as one, and the caller that
/// can name a Claude Code version turns either into a refusal that says which
/// it met.
enum Unsure {
    /// A marker naming a running process whose start the operating system will
    /// not say, so it can be neither corroborated nor dismissed.
    Marker(PathBuf),
    /// The sessions directory is there and would not be read. Told apart from
    /// an absent one, which is the ordinary "nothing is running" and the whole
    /// reason [`Host::list_dir`] reports the two differently.
    Unlistable { dir: PathBuf, why: HostError },
}

impl Unsure {
    fn detail(&self) -> String {
        match self {
            Unsure::Marker(marker) => format!(
                "{} names a running process, but when that process began could \
                 not be read, so the marker can be neither corroborated nor \
                 dismissed. If that session is dead, delete the file",
                marker.display()
            ),
            Unsure::Unlistable { dir, why } => format!(
                "{} could not be read ({why}), so whether a client is running \
                 against this Profile is not a question that got an answer. \
                 Nothing is assumed either way — make that directory readable, \
                 or delete it if no client is running",
                dir.display()
            ),
        }
    }
}

/// Whether anything may be running against a config directory, where a marker
/// that can be neither corroborated nor dismissed counts as one.
///
/// The question [`live_clients`] answers, for the caller that has no Claude
/// Code version to name an assumption against and does not need one: the Carry
/// writes into a Profile only when it is quiet (ADR 0003), and doubt is the
/// same answer as a client for that purpose. Not writing costs a dialog;
/// writing under a client that rewrites the file wholesale on its way out costs
/// the file.
pub fn anything_running(host: &dyn Host, config_dir: &Path) -> bool {
    match clients_in(host, config_dir) {
        Ok(running) => !running.is_empty(),
        Err(_) => true,
    }
}

/// The processes running against a config directory, or the marker that could
/// be neither corroborated nor dismissed. Both callers phrase that doubt in
/// their own terms, and neither decides it.
fn clients_in(host: &dyn Host, config_dir: &Path) -> std::result::Result<Vec<u32>, Unsure> {
    let dir = sessions_dir(config_dir);
    let markers = match host.list_dir(&dir) {
        Ok(markers) => markers,
        // Never having run a client is the ordinary case, and it is the *only*
        // one that means nothing is running. A directory that is there and will
        // not be read — root-owned after a `sudo claude`, say — is doubt, and
        // reading it as emptiness is how a Switch comes to write the Credential
        // out from under a session (ADR 0005). Every other doubt in this
        // function resolves towards Live; so does this one.
        Err(HostError::NotFound { .. }) => return Ok(Vec::new()),
        Err(why) => return Err(Unsure::Unlistable { dir, why }),
    };

    let mut running = Vec::new();
    for marker in markers {
        let pid: u32 = match marker
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse().ok())
        {
            Some(pid) => pid,
            None => continue,
        };
        let session_began = match session_start_in(host, &marker) {
            Marker::Began(at) => at,
            // Parsed, or not, and either way it does not say what a marker has
            // to say. That is a judgement about the *content* of a file Perch
            // can see all of, and the answer is that a Profile is Live when
            // something says so rather than when nothing does.
            Marker::SaysNothing => continue,
            // A different thing entirely: the marker is there and would not be
            // read — root-owned after a `sudo claude`, or halfway through being
            // written by a client that is starting up right now. Nothing about
            // it has been established, so it resolves the way every other doubt
            // in this function resolves, which is towards Live. Only for a pid
            // that is actually running: a file nobody can read beside a process
            // that is not there is litter, and litter must not refuse every
            // Switch against this Profile for ever.
            Marker::Unreadable if host.process_alive(pid) => return Err(Unsure::Marker(marker)),
            Marker::Unreadable => continue,
        };

        match host.process_started_at(pid) {
            Some(process_began) => {
                if process_began.timestamp_millis() <= session_began {
                    running.push(pid);
                }
            }
            // No start to compare. The process being gone is the ordinary way
            // that happens — a marker left behind by a client that died.
            None if !host.process_alive(pid) => {}
            None => return Err(Unsure::Marker(marker)),
        }
    }
    Ok(running)
}

/// What a session marker turned out to be.
///
/// Three answers rather than an `Option`, because the two ways of having no
/// timestamp resolve in opposite directions and folding them together resolved
/// both the wrong way — towards "nothing is running", in the one function whose
/// every other doubt resolves towards Live.
enum Marker {
    /// It says when its session began.
    Began(i64),
    /// Perch read the whole file and it is not a marker, or is one that does
    /// not say. A judgement about content, which is settled: a Profile is Live
    /// when something says so, not when nothing does.
    SaysNothing,
    /// Perch could not read it. Nothing has been established either way.
    Unreadable,
}

/// The marker's own record of when its session began.
fn session_start_in(host: &dyn Host, marker: &Path) -> Marker {
    // A marker that has gone between the listing and the read is one the client
    // took with it on its way out, which is the ordinary end of a session
    // rather than a doubt about one.
    let contents = match host.read_file(marker) {
        Ok(contents) => contents,
        Err(HostError::NotFound { .. }) => return Marker::SaysNothing,
        Err(_) => return Marker::Unreadable,
    };
    match serde_json::from_str::<SessionMarker>(&contents) {
        Ok(recorded) => match recorded.started_at {
            Some(at) => Marker::Began(at),
            None => Marker::SaysNothing,
        },
        Err(_) => Marker::SaysNothing,
    }
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
    // Written rather than replaced, so a file that has no `oauthAccount` yet
    // gets one. Claude Code writes `.claude.json` the moment it is first run —
    // onboarding, a theme, a project entry — and only writes the identity block
    // when somebody logs in through it. A machine restored from an Export has
    // never logged in through Claude Code, and one used with an API key never
    // will, so "the file is there and the block is not" is an ordinary state
    // rather than drift.
    //
    // Refusing it made the Switch unfinishable rather than failed: the
    // Credential is live and the registry records the Account by the time this
    // runs, so every retry got as far as here and stopped in the same place,
    // and the note saying "run it again to finish the job" was advice that
    // could never work. Only hand-editing `.claude.json` got out of it.
    //
    // The refusal that is left is the one it was always for: a `.claude.json`
    // that is not a JSON object at all is a file Perch does not recognise, and
    // nothing can be written into it.
    json::set_value_at(contents, IDENTITY_KEY, block).ok_or_else(|| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} is not a JSON object, so there is nowhere to write the \
                 Account into it",
                path.display()
            ),
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
///
/// The store is handed in rather than derived, because which directory counts
/// as the Default Profile is a question about Perch's own layout — a Run points
/// `CLAUDE_CONFIG_DIR` at a Profile, and adopting one as "the existing login"
/// would make the Account Perch already holds into a second Account with the
/// same Credential. [`crate::registry::the_default_profile`] is the answer, and
/// this module is below the one that can give it.
pub fn probe(host: &dyn Host, store: Store) -> Result<Verdict> {
    let version = claude_version(host)?;

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
    use crate::host::{Execution, FakeHost, Platform};

    #[test]
    fn claude_is_the_first_match_on_path() {
        let host = FakeHost::new()
            .with_env("PATH", "/empty:/usr/local/bin:/usr/bin")
            .with_file("/usr/local/bin/claude", "")
            .with_file("/usr/bin/claude", "");

        assert_eq!(
            claude_bin(&host).unwrap(),
            PathBuf::from("/usr/local/bin/claude")
        );
    }

    #[test]
    fn perch_claude_bin_overrides_the_search_verbatim() {
        let host = FakeHost::new()
            .with_env("PERCH_CLAUDE_BIN", "/somewhere/claude-nightly")
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "");

        assert_eq!(
            claude_bin(&host).unwrap(),
            PathBuf::from("/somewhere/claude-nightly")
        );
    }

    #[test]
    fn windows_consults_pathext_for_the_cmd_shim_npm_installs() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/tools;C:/npm")
            .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .with_file("C:/npm/claude.cmd", "");

        assert_eq!(
            claude_bin(&host).unwrap(),
            PathBuf::from("C:/npm/claude.cmd")
        );
    }

    #[test]
    fn windows_with_no_pathext_still_finds_the_exe() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/bin")
            .with_file("C:/bin/claude.exe", "");

        assert_eq!(
            claude_bin(&host).unwrap(),
            PathBuf::from("C:/bin/claude.exe")
        );
    }

    #[test]
    fn a_directory_named_claude_is_not_the_program() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            // A file inside is what makes /usr/bin/claude a directory here.
            .with_file("/usr/bin/claude/keep", "");

        assert!(claude_bin(&host).is_err());
    }

    #[test]
    fn the_keychain_account_name_knows_windows_spelling() {
        let host = FakeHost::new()
            .without_env("USER")
            .with_env("USERNAME", "someone");
        assert_eq!(keychain_account_name(&host).unwrap(), "someone");

        let bare = FakeHost::new().without_env("USER");
        let error = keychain_account_name(&bare).unwrap_err();
        assert!(
            matches!(error, PerchError::ProbeRefused { ref assumption, .. }
                if assumption == assumption::ACCOUNT_NAME),
            "{error}"
        );
    }

    #[test]
    fn no_claude_anywhere_is_a_refusal_naming_the_assumption() {
        let host = FakeHost::new().with_env("PATH", "/usr/bin");

        let error = claude_bin(&host).unwrap_err();
        assert!(
            matches!(error, PerchError::ProbeRefused { ref assumption, .. }
                if assumption == assumption::INSTALLED),
            "{error}"
        );
        assert!(error.to_string().contains("PERCH_CLAUDE_BIN"), "{error}");
    }

    #[test]
    fn the_default_directory_uses_the_bare_service_name() {
        assert_eq!(
            service_name_for(Path::new("/Users/someone/.claude"), true),
            "Claude Code-credentials"
        );
    }

    #[test]
    fn every_other_directory_gets_a_hash_of_its_path() {
        let service = service_name_for(Path::new("/Users/someone/.config/perch/profiles/a"), false);
        let hash = service.strip_prefix("Claude Code-credentials-").unwrap();
        assert_eq!(hash.len(), 8);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn two_directories_get_two_namespaces() {
        let one = service_name_for(Path::new("/Users/someone/.config/perch/profiles/a"), false);
        let two = service_name_for(Path::new("/Users/someone/.config/perch/profiles/b"), false);
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

    /// Claude Code writes `.claude.json` the first time it runs and writes the
    /// identity block only when somebody logs in through it, so a file with no
    /// block is an ordinary machine — one restored from an Export, or one used
    /// with an API key — rather than drift.
    ///
    /// Refusing it wedged the Switch permanently: the Credential is live and
    /// the registry records the Account by the time this runs, so every retry
    /// reached here and stopped in the same place, and the note advising
    /// another run was advice that could not work.
    #[test]
    fn a_file_that_has_no_block_yet_gets_one_rather_than_refusing_for_ever() {
        let patched = patch_oauth_account(
            r#"{"numStartups": 41}"#,
            r#"{"emailAddress": "someone@example.com"}"#,
            Path::new("/Users/someone/.claude.json"),
            "2.1.221",
        )
        .expect("a file with no block is a file to write one into");

        assert!(patched.contains(r#""oauthAccount""#), "{patched}");
        assert!(
            patched.contains(r#""emailAddress": "someone@example.com""#),
            "{patched}"
        );
        assert!(
            patched.contains(r#""numStartups": 41"#),
            "and every other member is left exactly as it was: {patched}"
        );
    }

    /// The refusal that is left is the one it was always for: nothing can be
    /// written into a `.claude.json` that is not a JSON object.
    #[test]
    fn a_file_that_is_not_an_object_is_a_refusal_naming_the_assumption() {
        let error = patch_oauth_account(
            r#"["not what this file is"]"#,
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

    /// The other half of the same rule: what the Identity does know is carried
    /// through. An Account belonging to an organization is one Claude Code
    /// displays by that organization, so a composed block that dropped it would
    /// leave the client naming the person and not the team they are working as.
    #[test]
    fn a_composed_block_carries_the_organization_when_the_identity_has_one() {
        let block = Identity {
            email: "someone@example.com".into(),
            account_uuid: Some("account-uuid-1".into()),
            organization_name: Some("Example Ltd".into()),
            organization_uuid: Some("org-uuid-9".into()),
        }
        .oauth_account_block();

        assert!(
            block.contains(r#""organizationName": "Example Ltd""#),
            "{block}"
        );
        assert!(
            block.contains(r#""organizationUuid": "org-uuid-9""#),
            "{block}"
        );
        assert!(
            block.contains(r#""emailAddress": "someone@example.com""#),
            "{block}"
        );
    }

    #[test]
    fn the_locks_are_the_three_claude_code_takes_in_the_order_it_takes_them() {
        let store = Store {
            config_dir: PathBuf::from("/Users/someone/.claude"),
            identity_file: PathBuf::from("/Users/someone/.claude.json"),
            keychain_service: DEFAULT_SERVICE.to_string(),
            keychain_account: "someone".to_string(),
            credentials_file: PathBuf::from("/Users/someone/.claude/.credentials.json"),
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

    /// The marker a Run writes and the marker Perch corroborates are the same
    /// file, and this is the one place both shapes are stated — so a change to
    /// either that forgot the other would leave a Run unable to say its own
    /// Profile is Live.
    #[test]
    fn the_marker_a_run_writes_is_one_this_module_reads_back() {
        let began = DateTime::from_timestamp_millis(NOON).expect("a time");
        let written = session_marker(4242, began);

        let host = FakeHost::new().with_file("/tmp/profile/sessions/4242.json", &written);
        assert!(
            matches!(
                session_start_in(&host, Path::new("/tmp/profile/sessions/4242.json")),
                Marker::Began(NOON)
            ),
            "the marker a Run writes says when its session began"
        );
        assert_eq!(
            session_marker_at(Path::new("/tmp/profile"), 4242),
            PathBuf::from("/tmp/profile/sessions/4242.json"),
            "the marker is named after the process, where the corroboration \
             reads the pid back out of the name"
        );
    }

    #[test]
    fn a_credential_never_renders_its_secret() {
        let credential = understood(r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret"}}"#);
        let rendered = format!("{credential}");
        assert!(!rendered.contains("sk-ant-oat01-secret"));
    }

    /// However it is asked to render.
    ///
    /// `Debug` is the likelier of the two to reach a log by accident — it is
    /// what `assert_eq!`, `panic!` and a failed `unwrap` all reach for — so a
    /// derived one would put the refresh token in CI output one `{:?}` away
    /// from the `Display` written to stop exactly that.
    #[test]
    fn a_credential_never_debugs_its_secret_either() {
        let credential = understood(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret","refreshToken":"sk-ant-ort01-secret"}}"#,
        );

        let rendered = format!("{credential:?}");

        assert!(!rendered.contains("sk-ant-oat01-secret"), "{rendered}");
        assert!(!rendered.contains("sk-ant-ort01-secret"), "{rendered}");
        assert!(rendered.contains("credential"), "{rendered}");
    }

    fn understood(raw: &str) -> Credential {
        understand_credential(raw.to_string(), "a test", "2.1.221").expect("a Credential")
    }

    /// Midday on the day the rest of the fixtures are set, in milliseconds.
    const NOON: i64 = 1_785_844_800_000;

    fn at(millis: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(millis).expect("a time")
    }

    #[test]
    fn a_credential_is_spent_before_it_expires_rather_than_as_it_does() {
        let hour_left = understood(&format!(
            r#"{{"claudeAiOauth":{{"accessToken":"a","expiresAt":{}}}}}"#,
            NOON + 3_600_000
        ));
        assert!(hour_left.usable_at(at(NOON)));

        let seconds_left = understood(&format!(
            r#"{{"claudeAiOauth":{{"accessToken":"a","expiresAt":{}}}}}"#,
            NOON + 30_000
        ));
        assert!(
            !seconds_left.usable_at(at(NOON)),
            "a token that expires mid-request is one Perch renews first"
        );
    }

    #[test]
    fn a_credential_that_says_nothing_about_expiring_is_taken_at_its_word() {
        let credential = understood(r#"{"claudeAiOauth":{"accessToken":"a"}}"#);
        assert!(credential.usable_at(at(NOON)));
    }

    #[test]
    fn a_rotation_replaces_the_tokens_and_leaves_the_rest_of_the_block_alone() {
        let current = understood(
            r#"{"claudeAiOauth":{"accessToken":"old-access","refreshToken":"old-refresh","expiresAt":1,"scopes":["user:inference"],"subscriptionType":"max"}}"#,
        );

        let rotated = credential_after_rotation(
            &current,
            "new-access",
            Some("new-refresh"),
            Some(NOON + 3_600_000),
            "2.1.221",
        )
        .expect("the block is there to renew");

        let back = understood(&rotated);
        assert_eq!(back.access_token, "new-access");
        assert_eq!(back.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(back.expires_at, Some(NOON + 3_600_000));
        assert_eq!(
            back.subscription_type.as_deref(),
            Some("max"),
            "what Claude Code recorded about the Account survives a renewal"
        );
        assert!(rotated.contains("user:inference"), "{rotated}");
        assert!(!rotated.contains("old-access") && !rotated.contains("old-refresh"));
    }

    #[test]
    fn a_renewal_that_hands_back_no_refresh_token_keeps_the_one_there_is() {
        let current = understood(
            r#"{"claudeAiOauth":{"accessToken":"old-access","refreshToken":"still-good"}}"#,
        );

        let rotated = credential_after_rotation(&current, "new-access", None, None, "2.1.221")
            .expect("the block is there to renew");

        let back = understood(&rotated);
        assert_eq!(back.refresh_token.as_deref(), Some("still-good"));
        assert_eq!(back.access_token, "new-access");
    }

    /// A `claude` that is there and will not run at all. The refusal names the
    /// assumption rather than the command, because "not installed" is the thing
    /// a user can act on and `exec` failing is not.
    #[test]
    fn a_claude_that_cannot_be_run_is_a_refusal_naming_the_assumption() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "");

        let refused = claude_version(&host).expect_err("nothing answers `--version`");

        match refused {
            PerchError::ProbeRefused {
                assumption,
                detail,
                version,
            } => {
                assert_eq!(assumption, assumption::INSTALLED);
                assert!(
                    detail.contains("could not run `claude --version`"),
                    "{detail}"
                );
                assert_eq!(version, "not installed");
            }
            other => panic!("an assumption failed, so it is a refusal: {other:?}"),
        }
    }

    #[test]
    fn a_claude_that_exits_non_zero_is_a_refusal_saying_what_it_exited_with() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 127,
                    stdout: String::new(),
                    stderr: "not found".to_string(),
                },
            );

        let refused = claude_version(&host).expect_err("it exited 127");

        let said = refused.to_string();
        assert!(said.contains("exited 127"), "{said}");
        assert!(
            said.contains("Claude Code unknown"),
            "the version is unknown rather than guessed: {said}"
        );
    }

    /// A clean exit that printed something other than a version. Worth its own
    /// refusal because the alternative is a version called `error:` quoted back
    /// in every later refusal that names which Claude Code Perch was talking to.
    #[test]
    fn a_version_that_does_not_start_with_a_digit_is_not_taken_as_one() {
        let refused =
            version_printing("error: could not start\n").expect_err("`error:` is not a version");

        let said = refused.to_string();
        assert!(
            said.contains("`error: could not start`, which is not a version"),
            "{said}"
        );
    }

    #[test]
    fn a_claude_that_printed_nothing_says_nothing_rather_than_quoting_emptiness() {
        let refused = version_printing("   \n").expect_err("nothing is not a version");

        assert!(refused.to_string().contains("printed nothing"), "{refused}");
    }

    #[test]
    fn the_leading_token_of_the_version_line_is_the_version() {
        assert_eq!(
            version_printing("2.1.221 (Claude Code)\n").expect("that is a version"),
            "2.1.221"
        );
    }

    /// A `claude --version` that exits cleanly having printed `printed`.
    fn version_printing(printed: &str) -> Result<String> {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 0,
                    stdout: printed.to_string(),
                    stderr: String::new(),
                },
            );
        claude_version(&host)
    }

    /// The three ways a Credential stops being one Perch recognises. Each names
    /// the store it came out of, so a refusal says which Account went wrong
    /// rather than that some Credential somewhere did.
    #[test]
    fn a_credential_that_is_not_json_names_the_store_it_came_out_of() {
        let refused = understand_credential(
            "not json at all".to_string(),
            "the Credential Perch holds for someone@example.com",
            "2.1.221",
        )
        .expect_err("that is not JSON");

        let said = refused.to_string();
        assert!(said.contains(assumption::CREDENTIAL_SHAPE), "{said}");
        assert!(said.contains("someone@example.com"), "{said}");
        assert!(said.contains("is not JSON Perch understands"), "{said}");
    }

    #[test]
    fn a_credential_with_no_oauth_block_is_refused_as_the_wrong_shape() {
        let refused = understand_credential(
            r#"{"somethingElse": {}}"#.to_string(),
            "the keychain",
            "2.1.221",
        )
        .expect_err("there is no claudeAiOauth block");

        assert!(
            refused.to_string().contains("has no claudeAiOauth block"),
            "{refused}"
        );
    }

    #[test]
    fn a_credential_with_no_access_token_is_refused_rather_than_used_empty() {
        let refused = understand_credential(
            r#"{"claudeAiOauth": {"refreshToken": "sk-ant-ort01-x"}}"#.to_string(),
            "the keychain",
            "2.1.221",
        )
        .expect_err("there is nothing to ask Anthropic with");

        assert!(
            refused
                .to_string()
                .contains("the claudeAiOauth block has no accessToken"),
            "{refused}"
        );
    }

    #[test]
    fn a_credential_perch_understands_carries_its_bytes_verbatim() {
        let raw =
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-x","refreshToken":"sk-ant-ort01-x"}}"#;

        let credential = understand_credential(raw.to_string(), "the keychain", "2.1.221")
            .expect("that is a Credential");

        assert_eq!(credential.access_token, "sk-ant-oat01-x");
        assert_eq!(credential.refresh_token.as_deref(), Some("sk-ant-ort01-x"));
        assert_eq!(
            credential.as_str(),
            raw,
            "the bytes are copied rather than re-serialised"
        );
    }

    /// A Rotation rewrites one key of a Credential Perch already holds, so the
    /// two ways that Credential could be unreadable are refusals about the
    /// Credential being renewed rather than about the reply.
    #[test]
    fn a_rotation_onto_a_credential_that_is_not_json_is_refused() {
        let broken = Credential {
            raw: "{ not json".to_string(),
            access_token: "sk-ant-oat01-old".to_string(),
            refresh_token: None,
            expires_at: None,
            subscription_type: None,
        };

        let refused = credential_after_rotation(&broken, "sk-ant-oat01-new", None, None, "2.1.221")
            .expect_err("what is being renewed is not JSON");

        assert!(
            refused
                .to_string()
                .contains("the Credential being renewed is not JSON Perch understands"),
            "{refused}"
        );
    }

    #[test]
    fn a_rotation_onto_a_credential_with_no_oauth_block_is_refused() {
        let broken = Credential {
            raw: r#"{"somethingElse": {}}"#.to_string(),
            access_token: "sk-ant-oat01-old".to_string(),
            refresh_token: None,
            expires_at: None,
            subscription_type: None,
        };

        let refused = credential_after_rotation(&broken, "sk-ant-oat01-new", None, None, "2.1.221")
            .expect_err("there is no block to write into");

        assert!(
            refused
                .to_string()
                .contains("the Credential being renewed has no claudeAiOauth block"),
            "{refused}"
        );
    }
}
