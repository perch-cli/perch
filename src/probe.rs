//! What Perch believes about the installed Claude Code, and how confident it is
//! (ADR an-assumption-is-probed).
//!
//! Every path, service-name derivation and struct shape Perch depends on is
//! reverse-engineered and none of it is a public contract. They live here, in
//! one module, and nowhere else — so when Claude Code drifts, there is exactly
//! one place that stops recognizing it, and every dangerous operation is gated
//! on the verdict it returns.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::{Host, HostError, Platform};
use crate::json;
use crate::secret::Secret;

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

/// Which Claude Code Perch is talking to. Quoted, never compared: every refusal
/// this module raises names the assumption that failed *and* the version it was
/// reading. A value rather than a `&str` for two reasons — reading it is a
/// `PATH` walk and a subprocess, so a command asks once; and a caller with no
/// version to give has [`Installed::unknown`] rather than a made-up string.
#[derive(Clone)]
pub enum Installed<'h> {
    /// The version, already read or already given.
    Said(String),
    /// Where to ask, for the one caller that has not asked yet, and what it
    /// answered once it has.
    Asking {
        host: &'h dyn Host,
        said: std::cell::OnceCell<String>,
    },
}

impl<'h> Installed<'h> {
    /// Asks the installed Claude Code what it is. Once per command: the answer
    /// cannot change under a process that is already running.
    pub fn probed(host: &dyn Host) -> Result<Installed<'static>> {
        Ok(Installed::Said(claude_version(host)?))
    }

    /// The same for a process that outlives a command, where asking every round
    /// forks a Node program to quote a string most rounds never quote. Only the
    /// version waits: a refusal quotes the Claude Code installed when it was
    /// raised rather than when the process started.
    pub fn asked_when_needed(host: &'h dyn Host) -> Result<Installed<'h>> {
        // Claude Code being *there* is still established now, a round with none
        // having nothing to do — and it is a `PATH` walk rather than a fork.
        claude_bin(host)?;
        Ok(Installed::Asking {
            host,
            said: std::cell::OnceCell::new(),
        })
    }

    /// When the question could not be asked, or is not the thing being tested.
    /// `said` is what a refusal will quote.
    pub fn unknown(said: &str) -> Installed<'static> {
        Installed::Said(said.to_string())
    }

    /// The version, or `(not installed)` where Claude Code will not answer.
    ///
    /// A Remove, a Purge, an Import and an Export run on a machine whose Claude
    /// Code may be gone, and refusing for want of a version would hold a
    /// Credential hostage to a program neither of them needs.
    pub fn probed_or_absent(host: &dyn Host) -> Installed<'static> {
        Installed::probed(host).unwrap_or_else(|_| Installed::unknown("(not installed)"))
    }

    /// What a refusal quotes.
    pub fn version(&self) -> &str {
        match self {
            Installed::Said(said) => said,
            // The binary was found when this was made, so what is left to fail
            // is running it — which is what `unknown` exists to say.
            Installed::Asking { host, said } => {
                said.get_or_init(|| claude_version(*host).unwrap_or_else(|_| "unknown".to_string()))
            }
        }
    }
}

/// The keychain service name Claude Code uses when `CLAUDE_CONFIG_DIR` is
/// unset. Every other config directory gets this plus a hash of its path.
pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";

/// The non-secret description of an Account, as Claude Code records it: what
/// Claude Code displays to say who you are. Perch stores it verbatim, so the
/// registry and the probe describe an Account with one type rather than two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub email: String,
    pub account_uuid: Option<String>,
    pub organization_name: Option<String>,
    pub organization_uuid: Option<String>,
}

/// A Credential, kept as the exact bytes the keychain holds — the only fields
/// read out are the ones something needs. The three secret ones are `Zeroizing`
/// because dropping a `String` returns its bytes to the allocator untouched and
/// this type is `Clone`: the `Debug` below stops one being *printed*, and this
/// stops one being *left behind*.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    raw: Zeroizing<String>,
    /// What proves the caller is this Account for the length of a session.
    pub access_token: Zeroizing<String>,
    /// What buys a fresh access token, and what Anthropic retires when it
    /// Rotates one.
    pub refresh_token: Option<Zeroizing<String>>,
    pub subscription_type: Option<String>,
    /// When the access token stops being accepted, in milliseconds.
    pub expires_at: Option<i64>,
}

/// How long before a Credential expires Perch stops relying on it. A token that
/// expires while a request is in flight is a request wasted against a budget
/// that does not refill early (ADR a-figure-carries-its-age), and the margin
/// costs nothing: it moves a Rotation Perch was about to need anyway.
pub const EXPIRY_MARGIN_SECONDS: i64 = 60;

impl Credential {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether the access token can still be asked a question at `now`. A
    /// Credential that says nothing about when it expires is taken at its word:
    /// Rotating one that has not expired spends the only refresh token there is.
    /// Saturating, because `expiresAt` comes out of a file Perch does not own and
    /// `i64::MIN` would wrap into "usable" in a release build.
    pub fn usable_at(&self, now: DateTime<Utc>) -> bool {
        match self.expires_at {
            Some(millis) => {
                millis.saturating_sub(now.timestamp_millis()) > EXPIRY_MARGIN_SECONDS * 1_000
            }
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
    /// accident: it is what every `assert_eq!`, `panic!` and `unwrap` reaches
    /// for, so a derived one defeats the `Display` above from one `{:?}` away.
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
/// (ADR claude-code-chooses-the-store). Not itself a Credential Store — this is
/// the config directory and everything derived from it, and
/// [`crate::credentials::CredentialStore`] is the glossary's term.
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
    Recognized(Box<Findings>),
    /// Claude Code is installed and recognized, but nobody is logged in.
    NoLogin { version: String, store: Store },
}

/// The Claude Code binary Perch runs, resolved by Perch rather than left to
/// `Command::new`: Rust appends only `.exe` and never consults `PATHEXT`, so the
/// `claude.cmd` that `npm i -g` installs works in every shell and would be
/// invisible to a bare `Command::new("claude")`. `$PERCH_CLAUDE_BIN` overrides
/// the search and passes through verbatim.
pub fn claude_bin(host: &dyn Host) -> Result<PathBuf> {
    if let Some(overridden) = host.env_var("PERCH_CLAUDE_BIN") {
        return Ok(PathBuf::from(overridden));
    }

    if host.env_var("PATH").is_none() {
        return Err(refusal(
            assumption::INSTALLED,
            "PATH is unset, so there is nowhere to look for `claude`. \
             Point PERCH_CLAUDE_BIN at it instead",
            "not installed",
        ));
    }

    on_path(host, "claude").ok_or_else(|| {
        refusal(
            assumption::INSTALLED,
            "no `claude` was found on PATH. Install Claude Code, or point \
             PERCH_CLAUDE_BIN at it",
            "not installed",
        )
    })
}

/// The first program of this name on `PATH`, or `None`. A search for any name
/// rather than for `claude`, because `perch upgrade` hands the work back to
/// `npm` on an npm Installation (ADR an-upgrade-asks-its-channel) and finding
/// `npm.cmd` is the same problem. The name is taken as given: no extension is
/// stripped and none is required.
pub fn on_path(host: &dyn Host, name: &str) -> Option<PathBuf> {
    let path = host.env_var("PATH")?;

    let on_windows = host.platform() == crate::host::Platform::Windows;
    let separator = if on_windows { ';' } else { ':' };
    // What makes a name executable on Windows is carrying one of PATHEXT's
    // extensions. Lowercase because that is how npm writes `claude.cmd`, and
    // the real filesystem answers case-insensitively anyway.
    let extensions: Vec<String> = if on_windows {
        // The bare name too, and last rather than first, which is the ordering
        // that keeps it safe: npm ships `npm` and `npm.cmd` side by side, and
        // the extensionless one is a shell script Windows cannot run.
        host.env_var("PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_lowercase)
            .chain(std::iter::once(String::new()))
            .collect()
    } else {
        vec![String::new()]
    };

    // Rooted directories only, as `curl_at` takes them: an empty element and a
    // `.` both mean the working directory, and `perch upgrade` runs what it
    // finds here.
    for dir in path.split(separator).filter(|dir| rooted(dir, on_windows)) {
        for extension in &extensions {
            // Joined with '/' rather than `Path::join`, which picks the
            // separator of whatever platform this build runs on: Windows
            // accepts either, and two spellings are two machines.
            let candidate = PathBuf::from(format!("{dir}/{name}{extension}"));
            if host.is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether a path names a place from the root rather than from wherever Perch
/// was run. Asked of the platform the host reports rather than through
/// `Path::is_absolute`, which reads the separator of the platform this build
/// runs on — the reason the search above joins with `/` by hand. Public because
/// the conformance table asks it of a fake claiming a platform it is not on.
pub fn rooted(dir: &str, on_windows: bool) -> bool {
    if dir.starts_with('/') {
        return true;
    }
    // A root of the current drive, and a drive named outright. `C:name` with no
    // separator is relative to that drive's own working directory, which is what
    // is being refused rather than a spelling of the root.
    on_windows
        && (dir.starts_with('\\')
            || matches!(
                dir.as_bytes(),
                [drive, b':', separator, ..]
                    if drive.is_ascii_alphabetic() && matches!(separator, b'\\' | b'/')
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

    // "2.1.221 (Claude Code)" — the leading token is the version, taken only
    // when it looks like one: `claude` printing "error: ..." on a clean exit
    // would otherwise be quoted back as a version by every later refusal.
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

/// The keychain service name for a config directory, as Claude Code derives it:
/// `Claude Code-credentials-<sha256(dir)[0:8]>`, or the bare name when
/// `CLAUDE_CONFIG_DIR` is unset — which is what gives every Profile a private
/// credential namespace for free.
pub fn service_name_for(config_dir: &Path, is_default: bool) -> String {
    if is_default {
        DEFAULT_SERVICE.to_string()
    } else {
        format!("{DEFAULT_SERVICE}-{}", short_hash(config_dir))
    }
}

/// The first eight hex characters of the SHA-256 of the directory path.
pub fn short_hash(config_dir: &Path) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut hasher = Sha256::new();
    hasher.update(config_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    // Two table lookups a byte rather than a `format!` each, which drives the
    // whole of `core::fmt` to write two characters.
    let mut hex = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    hex
}

/// The store Claude Code uses right now, honoring `CLAUDE_CONFIG_DIR`. The
/// default directory sits under home and its identity file sits beside it as
/// `~/.claude.json`, so no home is a refusal here — where every other config
/// directory carries its own identity file and needs no home at all.
pub fn default_store(host: &dyn Host) -> Result<Store> {
    match host.env_var("CLAUDE_CONFIG_DIR").map(PathBuf::from) {
        Some(config_dir) => store_for_directory(host, config_dir, false),
        None => store_for_directory(host, default_config_dir(host)?, true),
    }
}

/// The store of the Default Profile as it stands when nothing has pointed this
/// process anywhere else. Distinct from [`default_store`] in exactly the case a
/// Run creates: a client launched against a Profile passes `CLAUDE_CONFIG_DIR`
/// on, so a Perch run inside one is told a Profile is the default — and
/// Reconcile and the Carry have to reach past that.
pub fn default_profile_store(host: &dyn Host) -> Result<Store> {
    store_for_directory(host, default_config_dir(host)?, true)
}

/// The directory Claude Code falls back to when it is told nothing — the Default
/// Profile as the glossary means it, whatever this process's environment says.
/// [`default_store`] answers for the directory Claude Code would use *right
/// now*, and the two differ exactly when `CLAUDE_CONFIG_DIR` is set.
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

/// One spelling of a config directory, before anything is derived from it.
///
/// Every derivation reads the path as text, so two spellings are two keychain
/// namespaces and two locks — and a client handed the other one writes where
/// Perch will not read. `components` leaves `..`.
pub fn one_spelling(config_dir: &Path) -> PathBuf {
    config_dir.components().collect()
}

fn store_for_directory(host: &dyn Host, config_dir: PathBuf, is_default: bool) -> Result<Store> {
    let config_dir = one_spelling(&config_dir);

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

/// The plaintext store for a config directory: always inside it, unlike the
/// identity file, because Claude Code joins its config directory to
/// `.credentials.json` and nothing else. This and [`service_name_for`] are the
/// two halves of [`assumption::CREDENTIAL_LOCATION`], and neither is probed at
/// runtime, so what guards them is `tests/your_machine.rs`.
pub fn credentials_file_for(config_dir: &Path) -> PathBuf {
    config_dir.join(CREDENTIALS_FILE)
}

/// What the identity file is called, wherever it sits: beside the default config
/// directory as `~/.claude.json`, and inside every other one. Named here because
/// the name of a file Claude Code writes belongs in the module that knows about
/// Claude Code rather than in the one that enumerates a directory
/// (ADR everything-but-the-account).
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

/// What stands in for the login name where there is no keychain to store
/// anything under. Never looked up — it exists so that a `Store` off macOS can
/// be derived at all, and it is spelled to be recognizable in a diagnostic.
const NO_LOGIN_NAME: &str = "(no keychain)";

/// The login name a keychain item is stored under. `USERNAME` is Windows'
/// spelling of `USER`; off macOS the name is carried in the Store and never
/// looked up, so it is only *required* on macOS. Every `Store` is derived
/// through here, so an unset `USER` refused everywhere would fail every command
/// on a container or a systemd timer over a value nothing there reads.
fn keychain_account_name(host: &dyn Host) -> Result<String> {
    if let Some(name) = host.env_var("USER").or_else(|| host.env_var("USERNAME")) {
        return Ok(name);
    }
    if host.platform() != Platform::MacOs {
        return Ok(NO_LOGIN_NAME.to_string());
    }
    Err(refusal(
        assumption::ACCOUNT_NAME,
        "neither USER nor USERNAME is set, so the keychain account name \
         cannot be derived",
        "unknown",
    ))
}

/// Reads and understands the Credential a config directory holds, from whichever
/// of its two Credential Stores holds one, or says why it cannot.
pub fn read_credential(
    host: &dyn Host,
    store: &Store,
    installed: &Installed,
) -> Result<Option<Credential>> {
    let Some(held) = credentials::read(host, store)? else {
        return Ok(None);
    };

    understand_credential(held.credential, &held.kept_in.describe(), installed).map(Some)
}

/// Makes sense of the bytes a keychain namespace holds, or says which belief
/// they broke. `held_in` names the namespace they came out of, so a refusal
/// says which Account's store stopped being recognizable.
pub fn understand_credential(
    raw: Zeroizing<String>,
    held_in: &str,
    installed: &Installed,
) -> Result<Credential> {
    let parsed: CredentialFile = serde_json::from_str(&raw).map_err(|err| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!(
                "{held_in} is not JSON Perch understands: {}",
                where_it_is_wrong(&err)
            ),
            installed.version(),
        )
    })?;

    let oauth = parsed.claude_ai_oauth.ok_or_else(|| {
        refusal(
            assumption::CREDENTIAL_SHAPE,
            &format!("{held_in} has no claudeAiOauth block"),
            installed.version(),
        )
    })?;

    // Into wiping types before the refusal below can return: serde's `String`
    // dropped on that path leaves a live refresh token in freed heap, which
    // outlives the process in a core dump or a hibernation image.
    let access_token = oauth.access_token.map(Zeroizing::new);
    let refresh_token = oauth.refresh_token.map(Zeroizing::new);

    let Some(access_token) = access_token else {
        return Err(refusal(
            assumption::CREDENTIAL_SHAPE,
            "the claudeAiOauth block has no accessToken",
            installed.version(),
        ));
    };

    Ok(Credential {
        raw,
        access_token,
        refresh_token,
        subscription_type: oauth.subscription_type,
        expires_at: oauth.expires_at,
    })
}

/// The key of the credential store that holds the Credential itself.
pub const CREDENTIAL_KEY: &str = "claudeAiOauth";

/// The bytes to store after a Rotation: this Credential with the three fields a
/// renewal replaces, and everything else — the scopes Claude Code asked for, the
/// subscription it recorded — left as it was. Read and written as JSON rather
/// than spliced as text, because a Credential is one small object that only ever
/// holds an Account's tokens.
pub fn credential_after_rotation(
    current: &Credential,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_at: Option<i64>,
    installed: &Installed,
) -> Result<Secret> {
    let mut document: serde_json::Value =
        serde_json::from_str(current.as_str()).map_err(|err| {
            refusal(
                assumption::CREDENTIAL_SHAPE,
                &format!(
                    "the Credential being renewed is not JSON Perch understands: {}",
                    where_it_is_wrong(&err)
                ),
                installed.version(),
            )
        })?;

    let block = document
        .get_mut(CREDENTIAL_KEY)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            refusal(
                assumption::CREDENTIAL_SHAPE,
                "the Credential being renewed has no claudeAiOauth block",
                installed.version(),
            )
        })?;

    // Each `insert` hands back the token it displaced, which is the retired
    // generation: dropped as it comes, it would be freed untouched, which is
    // what the tree is emptied by hand below to prevent.
    wipe(block.insert("accessToken".into(), access_token.into()));
    // A refresh that hands back no new refresh token has not Rotated: the one
    // already stored is still the live one, and replacing it with nothing would
    // throw away the only way back.
    if let Some(token) = refresh_token {
        wipe(block.insert("refreshToken".into(), token.into()));
    }
    // Taken out rather than left alone where the renewal gave no lifetime: a
    // Credential is renewed *because* its `expiresAt` had passed, so leaving
    // the stale one renews again on every command after.
    match expires_at {
        Some(at) => block.insert("expiresAt".into(), at.into()),
        None => block.remove("expiresAt"),
    };

    let written = crate::json::sealed(&document);

    // The document is still holding both tokens, and dropping a
    // `serde_json::Value` frees its strings untouched. This is the freshly
    // Rotated refresh token, so the tree is emptied by hand before it goes.
    if let Some(block) = document
        .get_mut(CREDENTIAL_KEY)
        .and_then(serde_json::Value::as_object_mut)
    {
        for key in ["accessToken", "refreshToken"] {
            if let Some(serde_json::Value::String(held)) = block.get_mut(key) {
                held.zeroize();
            }
        }
    }

    Ok(written)
}

/// Wipes a token a `serde_json::Map` handed back, if it handed one back.
///
/// Taking a `Value` rather than a `String` because that is what `insert` and
/// `remove` return, and a caller that has to match before it can wipe is a
/// caller that will one day forget to.
fn wipe(displaced: Option<serde_json::Value>) {
    if let Some(serde_json::Value::String(mut held)) = displaced {
        held.zeroize();
    }
}

/// Reads the Identity out of a store's `.claude.json`.
pub fn read_identity(
    host: &dyn Host,
    store: &Store,
    installed: &Installed,
) -> Result<Option<Identity>> {
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

    // Claude Code's file, not Perch's: a shape Perch cannot parse is an
    // assumption failing rather than a corrupt file. Through `where_it_is_wrong`
    // for the reason the Credential is — serde quotes the value it tripped on.
    let parsed: IdentityFile = serde_json::from_str(&contents).map_err(|err| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} is not JSON Perch understands: {}",
                store.identity_file.display(),
                where_it_is_wrong(&err)
            ),
            installed.version(),
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
            installed.version(),
        )
    })?;

    // An address is what every Profile path, keychain namespace, Alias target
    // and Group membership is keyed on, so one with nothing nameable in it is
    // refused where it enters rather than downstream of a derived path.
    if !email.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err(refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} names the account `{email}`, which has no character Perch \
                 can name a Profile after",
                store.identity_file.display()
            ),
            installed.version(),
        ));
    }

    // And an address is also a Target, which is why `name::validate`
    // refuses an `@` in an Alias or a Group name — a rule that only holds if
    // this half does.
    if !email.contains('@') {
        return Err(refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} names the account `{email}`, which is not an address an \
                 Alias or a Group name could be told from",
                store.identity_file.display()
            ),
            installed.version(),
        ));
    }

    // Refused here and not in `registry::validate`, which `load` meets over a
    // value Perch wrote down (ADR a-registry-comes-forward). The address on the
    // whole set, the organization on `Cc` (ADR nothing-drawn-is-obeyed).
    let asked = [
        (
            "the account",
            crate::host::unshowable_character_in(email.as_str()),
        ),
        (
            "an organization",
            crate::host::control_character_in(account.organization_name.as_deref().unwrap_or("")),
        ),
    ];
    for (what, carrying) in asked {
        let Some(said) = carrying else {
            continue;
        };
        return Err(refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} names {what} carrying {said}, which no line of Perch's \
                 output could show as part of one",
                store.identity_file.display()
            ),
            installed.version(),
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
/// parameters it treats that lock under. A lock artifact is a **directory**,
/// because `mkdir` either succeeds or fails with nothing in between; it is
/// abandoned once its modification time is older than `stale_millis`, and a
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

/// The locks a Switch runs inside, in the order Claude Code takes them — which
/// is the contract with a concurrently-running Claude Code rather than a
/// preference: any other order is how two processes each hold one of the pair.
/// Under these locks Claude Code's double-checked re-read sees a swapped,
/// non-expired Credential and abandons its own refresh.
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
            dir: store.config_dir.join(REFRESH_LOCK),
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
/// Two modules need it for opposite reasons: this one derives the markers it
/// reads, and [`crate::reconcile`] holds it back from crossing — a directory
/// whose contents answer *is a client running here* is meaningless shared.
pub const SESSIONS: &str = "sessions";

/// The lock Claude Code takes inside a config directory while it renews a
/// Credential — the one of its three that is not a sibling of the directory but
/// an entry in it, and so the one a Reconcile would otherwise enumerate. Named
/// here for the reason [`SESSIONS`] is.
pub const REFRESH_LOCK: &str = ".oauth_refresh.lock";

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
/// writes one and this module reads one back. Three fields and no more: the two
/// that make it evidence, and one that says who wrote it. `started_at` is when
/// the Run began rather than when the process did, which is what makes the file
/// corroborate itself — the process it names began strictly earlier.
pub fn session_marker(pid: u32, started_at: DateTime<Utc>) -> String {
    serde_json::json!({
        "pid": pid,
        "startedAt": started_at.timestamp_millis(),
        "writtenBy": "perch",
    })
    .to_string()
}

/// A config directory this process has made Live, for as long as this value is
/// held. Perch writes session markers as well as reading them: a Run makes the
/// Profile it launches Live (ADR a-run-is-one-shot), and a login makes the
/// directory it is driving Live. A value with a `Drop` rather than a call to
/// make, because a bare removal is the line the next early return walks past.
pub struct Claim<'a> {
    host: &'a dyn Host,
    marker: PathBuf,
}

impl Drop for Claim<'_> {
    /// However the operation ended, including not having started. A login's
    /// directory is gone by now — `profile::discard` takes it whole — so this is
    /// removing a file inside a directory that is not there, which the port says
    /// is not a failure.
    fn drop(&mut self) {
        let _ = self.host.remove_file(&self.marker);
    }
}

/// Makes a config directory Live, naming this process. Perch's own pid, because
/// Perch waits for what it started, so the marker holds for exactly as long as
/// the operation. Written atomically, because a plain write truncates and then
/// fills and a file that can be read whole and says nothing settles as "not
/// Live". Whether a claim that fails is fatal is the caller's to decide.
pub fn claim<'a>(host: &'a dyn Host, config_dir: &Path) -> Result<Claim<'a>> {
    let pid = host.process_id();
    let marker = session_marker_at(config_dir, pid);
    let sessions = sessions_dir(config_dir);

    // A `sessions` that is a link is refused rather than written through and
    // rather than repaired, which is Reconcile's: `create_dir_all` at a link
    // uses the target, so the Marker would land in the Default Profile.
    if matches!(host.link_target(&sessions), Ok(Some(_))) {
        return Err(PerchError::Other(format!(
            "{} is a link rather than a directory of its own, so recording that \
             a client is running here would write the marker into whatever it \
             points at — and that directory would report this Run as its own. \
             Nothing was launched.",
            sessions.display()
        )));
    }

    // Private, because this is the third path that brings a Profile directory
    // into being and 0700 is what a Profile owes. One already there is left as
    // it is, so the Default Profile keeps whatever mode it has.
    host.create_private_dir_all(&sessions)
        .and_then(|()| {
            crate::host::write_atomically(host, &marker, &session_marker(pid, host.now()))
        })
        .map_err(|err| {
            PerchError::Other(format!(
                "{} could not be written ({err}), so Perch cannot record that a \
                 client is running against this Profile — and another Perch \
                 would be free to Capture or Renew the Credential that client \
                 is holding. Nothing was launched.",
                marker.display()
            ))
        })?;

    Ok(Claim { host, marker })
}

/// The marker Claude Code writes for a running session, to the extent Perch
/// reads it. `startedAt` is when the session began, in milliseconds since the
/// epoch — which means the same thing on every platform, and is why it is the
/// field matched rather than the platform-encoded `procStart`.
#[derive(Deserialize)]
struct SessionMarker {
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
}

/// What a session marker turned out to be. Three answers rather than an
/// `Option`, because the two ways of having no timestamp resolve in opposite
/// directions: one is a judgment about content, and the other is a file nothing
/// has been established about.
pub enum Marker {
    /// It says when its session began.
    Began(i64),
    /// Perch read the whole file and it is not a marker, or is one that does
    /// not say. A judgment about content, which is settled: a Profile is Live
    /// when something says so, not when nothing does.
    SaysNothing,
    /// Perch could not read it. Nothing has been established either way.
    Unreadable,
}

/// The marker's own record of when its session began.
pub fn session_start_in(host: &dyn Host, marker: &Path) -> Marker {
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

/// The key of `.claude.json` that says who the Account is, and the only key of
/// that file Perch ever writes.
pub const IDENTITY_KEY: &str = "oauthAccount";

/// A file Claude Code would read, holding this Account and nothing else. What
/// it writes for itself on a machine that has never run it.
pub fn fresh_identity_file(block: &str) -> String {
    let indented = block.replace('\n', "\n  ");
    format!("{{\n  \"{IDENTITY_KEY}\": {indented}\n}}\n")
}

/// Rewrites the `oauthAccount` block of an identity file, leaving every other
/// byte of it exactly as it was. `.claude.json` also holds project history, MCP
/// configuration and settings, and leaving those in place is what makes them
/// follow the person across a Switch — so this splices one value
/// ([`crate::json`]) rather than parsing the file and writing it back.
pub fn patch_oauth_account(
    contents: &str,
    block: &str,
    path: &Path,
    installed: &Installed,
) -> Result<Secret> {
    // Written rather than replaced, so a file with no `oauthAccount` yet gets
    // one: Claude Code writes the identity block only at a login. The one
    // refusal left is a file that is not a JSON object at all.
    json::set_value_at(contents, IDENTITY_KEY, block).ok_or_else(|| {
        refusal(
            assumption::IDENTITY_BLOCK,
            &format!(
                "{} is not a JSON object, so there is nowhere to write the \
                 Account into it",
                path.display()
            ),
            installed.version(),
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
            .expect("a map of strings serializes")
    }
}

/// The one question this module exists to answer: what do we believe about the
/// installed Claude Code, and how confident are we? The store is handed in
/// rather than derived, because which directory counts as the Default Profile is
/// a question about Perch's own layout, and this module is below the one that can
/// answer it ([`crate::registry::the_default_profile`]).
pub fn probe(host: &dyn Host, store: Store) -> Result<Verdict> {
    let installed = Installed::probed(host)?;
    let version = installed.version().to_string();

    let credential = read_credential(host, &store, &installed)?;
    let identity = read_identity(host, &store, &installed)?;

    match (credential, identity) {
        (Some(credential), Some(identity)) => Ok(Verdict::Recognized(Box::new(Findings {
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

/// What is wrong with a document, without what was in it.
///
/// serde quotes the offending value verbatim — `invalid type: string
/// "sk-ant-oat01-…", expected i64` — and these bytes came out of a Credential
/// Store, so the words are the fault's and never the file's.
fn where_it_is_wrong(err: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let what = match err.classify() {
        Category::Io => "it could not be read",
        Category::Syntax => "it is not JSON",
        Category::Data => "a field is not the type Perch expects",
        Category::Eof => "it ends before it is finished",
    };
    format!("{what}, at line {} column {}", err.line(), err.column())
}

pub(crate) fn refusal(assumption: &str, detail: &str, version: &str) -> PerchError {
    PerchError::ProbeRefused(Box::new(crate::error::ProbeRefusal {
        assumption: assumption.to_string(),
        detail: detail.to_string(),
        version: version.to_string(),
        note: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patch as text a case can compare: it answers a `Secret`, which
    /// neither prints nor compares.
    fn patched_block(
        contents: &str,
        block: &str,
        path: &Path,
        installed: &Installed,
    ) -> Result<String> {
        patch_oauth_account(contents, block, path, installed)
            .map(|written| written.as_str().to_string())
    }

    /// A version to quote, for the tests that are not about the quoting.
    fn version_under_test() -> Installed<'static> {
        Installed::unknown("2.1.221")
    }
    use crate::host::{Execution, FakeHost, Platform};

    /// Every derivation here reads the path as text — `short_hash` hashes it into
    /// the keychain service name, `locks_for` pushes `.lock` onto it — so a
    /// trailing separator is the same directory with a different Credential Store
    /// and a lock inside the directory rather than the sibling one a running
    /// Claude Code contends for.
    #[test]
    fn a_configuration_directory_spelled_with_a_trailing_separator_is_the_same_store() {
        let host = FakeHost::new();
        let plain =
            store_for_profile(&host, Path::new("/Users/someone/elsewhere")).expect("USER is set");
        let trailing =
            store_for_profile(&host, Path::new("/Users/someone/elsewhere/")).expect("USER is set");

        assert_eq!(
            plain.keychain_service, trailing.keychain_service,
            "the namespace a Credential is kept in is the directory, not its spelling"
        );
        assert_eq!(plain.config_dir, trailing.config_dir);
        assert_eq!(plain.credentials_file, trailing.credentials_file);
        assert_eq!(plain.identity_file, trailing.identity_file);
        assert_eq!(
            locks_for(&plain)
                .iter()
                .map(|lock| lock.dir.clone())
                .collect::<Vec<_>>(),
            locks_for(&trailing)
                .iter()
                .map(|lock| lock.dir.clone())
                .collect::<Vec<_>>(),
            "and the locks are the ones a running Claude Code takes"
        );
    }

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

    /// `perch upgrade` runs what this finds, so a `PATH` element that resolves
    /// against the working directory hands the machine to whatever `npm` or
    /// `brew` the shell happened to be sitting in.
    #[test]
    fn a_path_element_that_means_the_working_directory_is_not_searched() {
        for (platform, path, relative) in [
            (Platform::Other, ".:/usr/bin", "./claude"),
            (Platform::Other, ":/usr/bin", "claude"),
            (Platform::Other, "tools:/usr/bin", "tools/claude"),
            (Platform::Windows, ".;C:/bin", "./claude.exe"),
            (Platform::Windows, "C:tools;C:/bin", "C:tools/claude.exe"),
        ] {
            let wanted = match platform {
                Platform::Windows => "C:/bin/claude.exe",
                _ => "/usr/bin/claude",
            };
            let host = FakeHost::new()
                .with_platform(platform)
                .with_env("PATH", path)
                .with_file(relative, "")
                .with_file(wanted, "");

            assert_eq!(
                claude_bin(&host).unwrap(),
                PathBuf::from(wanted),
                "the case this is about: {path}"
            );
        }
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

    /// "The name is taken as given — no extension is stripped and none is
    /// required", which was true everywhere but the one platform that has
    /// extensions. Built from `PATHEXT` alone, `npm.cmd` was only ever probed
    /// as `npm.cmd.com`, `npm.cmd.exe`, `npm.cmd.bat` and `npm.cmd.cmd`.
    #[test]
    fn windows_finds_a_name_that_already_carries_its_extension() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/npm")
            .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .with_file("C:/npm/npm.cmd", "");

        assert_eq!(
            on_path(&host, "npm.cmd"),
            Some(PathBuf::from("C:/npm/npm.cmd"))
        );
    }

    /// And the bare name is asked *last*, which is what keeps that safe: npm
    /// ships `npm` and `npm.cmd` side by side, and the extensionless one is a
    /// shell script Windows cannot run.
    #[test]
    fn windows_prefers_the_spelling_it_can_execute_over_the_bare_name() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/npm")
            .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .with_file("C:/npm/npm", "")
            .with_file("C:/npm/npm.cmd", "");

        assert_eq!(on_path(&host, "npm"), Some(PathBuf::from("C:/npm/npm.cmd")));
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
            matches!(&error, PerchError::ProbeRefused(refusal)
                if refusal.assumption == assumption::ACCOUNT_NAME),
            "{error}"
        );
    }

    /// The name is only load-bearing where there is a keychain to store an item
    /// in. Off macOS every `Store` still has to be derivable without it, or a
    /// container with no `USER` cannot run a single Perch command — including
    /// the `perch watcher check` a systemd timer fires, which is where
    /// ADR a-watcher-knob-is-arithmetic says an unattended watcher belongs.
    #[test]
    fn a_machine_with_no_keychain_needs_no_login_name() {
        for platform in [Platform::Other, Platform::Windows] {
            let bare = FakeHost::new().without_env("USER").with_platform(platform);
            assert_eq!(
                keychain_account_name(&bare).expect("nothing here reads the name"),
                NO_LOGIN_NAME,
                "{platform:?}"
            );
            default_store(&bare).expect("a Store is still derivable");
        }
    }

    #[test]
    fn no_claude_anywhere_is_a_refusal_naming_the_assumption() {
        let host = FakeHost::new().with_env("PATH", "/usr/bin");

        let error = claude_bin(&host).unwrap_err();
        assert!(
            matches!(&error, PerchError::ProbeRefused(refusal)
                if refusal.assumption == assumption::INSTALLED),
            "{error}"
        );
        assert!(error.to_string().contains("PERCH_CLAUDE_BIN"), "{error}");
    }

    /// What `default_store` composes rather than any one derivation it composes
    /// it from: `service_name_for` is asserted on its own further down, and
    /// passing there says nothing about whether this reaches it with
    /// `is_default` the right way round. It reads no machine to say so, which is
    /// why it is here (ADR a-suite-is-named-and-gated).
    #[test]
    fn the_default_store_is_where_perch_believes_it_is() {
        let host = FakeHost::new();

        let store = default_store(&host).expect("USER is set");

        assert_eq!(store.config_dir, Path::new("/Users/someone/.claude"));
        assert_eq!(
            store.identity_file,
            Path::new("/Users/someone/.claude.json"),
            "beside the directory, not inside it"
        );
        assert_eq!(
            store.credentials_file,
            Path::new("/Users/someone/.claude/.credentials.json")
        );
        assert_eq!(
            store.keychain_service, DEFAULT_SERVICE,
            "the default config directory uses the bare service name"
        );
        assert_eq!(store.keychain_account, "someone");
    }

    /// And the other arm, which is the one every Profile takes: a config
    /// directory Perch was pointed at derives a namespace of its own.
    #[test]
    fn a_config_directory_perch_was_pointed_at_derives_its_own_store() {
        let profile = "/Users/someone/.perch/profiles/someone-example-com";
        let host = FakeHost::new().with_env("CLAUDE_CONFIG_DIR", profile);

        let store = default_store(&host).expect("USER is set");

        assert_eq!(store.config_dir, Path::new(profile));
        assert_eq!(
            store.identity_file,
            Path::new(profile).join(".claude.json"),
            "inside it, where the default one's sits beside"
        );
        assert!(
            store
                .keychain_service
                .starts_with(&format!("{DEFAULT_SERVICE}-")),
            "a namespace of its own, not the bare name: {}",
            store.keychain_service
        );
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
        // Independently computed, and pinned so a change to the derivation has
        // to be a deliberate edit:
        //   printf '%s' "/tmp/perch-fixture" | shasum -a 256 | cut -c1-8
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
        let patched = patched_block(
            IDENTITY_FILE,
            "{\n  \"emailAddress\": \"overflow@example.com\"\n}",
            Path::new("/Users/someone/.claude.json"),
            &Installed::unknown("2.1.221"),
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

        let patched = patched_block(
            IDENTITY_FILE,
            from_elsewhere,
            Path::new("/Users/someone/.claude.json"),
            &Installed::unknown("2.1.221"),
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
    #[test]
    fn a_file_that_has_no_block_yet_gets_one_rather_than_refusing_for_ever() {
        let patched = patched_block(
            r#"{"numStartups": 41}"#,
            r#"{"emailAddress": "someone@example.com"}"#,
            Path::new("/Users/someone/.claude.json"),
            &Installed::unknown("2.1.221"),
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
        let error = patched_block(
            r#"["not what this file is"]"#,
            "{}",
            Path::new("/Users/someone/.claude.json"),
            &Installed::unknown("2.1.221"),
        )
        .unwrap_err();
        assert!(
            matches!(&error, PerchError::ProbeRefused(refusal)
                if refusal.assumption == assumption::IDENTITY_BLOCK),
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

    /// Both halves of what makes an address usable, in one place: nothing
    /// nameable in it and there is no Profile to put it in, and no `@` in it and
    /// it is a Target an Alias or a Group name could not be told from — which is
    /// what `name::validate` refuses those two for.
    #[test]
    fn an_address_perch_could_not_key_anything_on_is_refused_where_it_enters() {
        for (email, expected) in [
            ("...", "no character Perch can name a Profile after"),
            ("work", "could be told from"),
        ] {
            let host = FakeHost::new()
                .with_env("HOME", "/Users/someone")
                .with_env("USER", "someone")
                .with_file(
                    "/Users/someone/.claude.json",
                    &format!(r#"{{"oauthAccount":{{"emailAddress":"{email}"}}}}"#),
                );
            let store = default_store(&host).expect("the store is derivable");

            let refused = read_identity(&host, &store, &version_under_test())
                .expect_err("Perch cannot key anything on that");

            assert!(refused.to_string().contains(expected), "{refused}");
        }
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

    /// However it is asked to render. `Debug` is the likelier of the two to reach
    /// a log by accident — it is what `assert_eq!`, `panic!` and a failed
    /// `unwrap` all reach for.
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
        understand_credential(
            Zeroizing::new(raw.to_string()),
            "a test",
            &version_under_test(),
        )
        .expect("a Credential")
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

    /// `expiresAt` is whatever is in a file Perch does not own, so the arithmetic
    /// over it has to survive the whole of `i64` rather than the part of it a
    /// Claude Code would write. Both ends: `i64::MIN` overflowed the subtraction
    /// — a panic here and, with no `overflow-checks` in the release profile, a
    /// wrap that answered "usable" in the binary somebody downloads.
    #[test]
    fn an_expiry_at_either_end_of_the_range_is_answered_rather_than_overflowed() {
        let ran_out = understood(&format!(
            r#"{{"claudeAiOauth":{{"accessToken":"a","expiresAt":{}}}}}"#,
            i64::MIN
        ));
        assert!(
            !ran_out.usable_at(at(NOON)),
            "a time that far in the past has run out"
        );

        let never = understood(&format!(
            r#"{{"claudeAiOauth":{{"accessToken":"a","expiresAt":{}}}}}"#,
            i64::MAX
        ));
        assert!(
            never.usable_at(at(NOON)),
            "and one that far ahead has not, rather than wrapping into having"
        );
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
            &Installed::unknown("2.1.221"),
        )
        .expect("the block is there to renew");

        let back = understood(&rotated);
        assert_eq!(*back.access_token, "new-access");
        assert_eq!(
            back.refresh_token.as_ref().map(|t| t.as_str()),
            Some("new-refresh")
        );
        assert_eq!(back.expires_at, Some(NOON + 3_600_000));
        assert_eq!(
            back.subscription_type.as_deref(),
            Some("max"),
            "what Claude Code recorded about the Account survives a renewal"
        );
        assert!(rotated.contains("user:inference"), "{}", rotated.as_str());
        assert!(!rotated.contains("old-access") && !rotated.contains("old-refresh"));
    }

    #[test]
    fn a_renewal_that_hands_back_no_refresh_token_keeps_the_one_there_is() {
        let current = understood(
            r#"{"claudeAiOauth":{"accessToken":"old-access","refreshToken":"still-good"}}"#,
        );

        let rotated =
            credential_after_rotation(&current, "new-access", None, None, &version_under_test())
                .expect("the block is there to renew");

        let back = understood(&rotated);
        assert_eq!(
            back.refresh_token.as_ref().map(|t| t.as_str()),
            Some("still-good")
        );
        assert_eq!(*back.access_token, "new-access");
    }

    /// The test above cannot see this: its Credential carries no `expiresAt` at
    /// all, so there is nothing for a renewal to leave behind. A Credential is
    /// only ever renewed *because* its `expiresAt` has passed, which makes the
    /// stale value the case that actually arises.
    #[test]
    fn a_renewal_that_hands_back_no_lifetime_does_not_keep_the_expiry_that_had_passed() {
        let current = understood(&format!(
            r#"{{"claudeAiOauth":{{"accessToken":"old-access","refreshToken":"still-good","expiresAt":{}}}}}"#,
            NOON - 3_600_000,
        ));
        assert!(
            !current.usable_at(DateTime::from_timestamp_millis(NOON).expect("a time"),),
            "the Credential being renewed is one that had already expired",
        );

        let rotated =
            credential_after_rotation(&current, "new-access", None, None, &version_under_test())
                .expect("the block is there to renew");

        let back = understood(&rotated);
        assert_eq!(back.expires_at, None);
        assert!(
            back.usable_at(DateTime::from_timestamp_millis(NOON).expect("a time")),
            "a renewal that gave no lifetime leaves a Credential that says \
             nothing about when it expires — not one still carrying the expiry \
             that caused the renewal, which renews again on every command",
        );
        assert!(!rotated.contains("expiresAt"), "{}", rotated.as_str());
    }

    /// The whole of what `asked_when_needed` promises: `claude` is established
    /// as being there, and its version is read the first time something quotes
    /// one and only once however often it is quoted after that.
    #[test]
    fn a_deferred_version_is_read_when_it_is_first_quoted_and_not_before() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 0,
                    stdout: "2.1.221 (Claude Code)".to_string(),
                    stderr: String::new(),
                },
            );

        let installed = Installed::asked_when_needed(&host).expect("`claude` is on PATH");
        assert_eq!(
            versions_read_by(&host),
            0,
            "nothing has quoted it, so nothing has been forked"
        );

        assert_eq!(installed.version(), "2.1.221");
        assert_eq!(installed.version(), "2.1.221");
        assert_eq!(versions_read_by(&host), 1, "{:?}", host.effects());
    }

    /// A `claude` that goes missing between the two is quoted as unknown rather
    /// than raised: `version` is reached from inside a refusal being built, and
    /// there is nothing there to hand a second failure to.
    #[test]
    fn a_deferred_version_that_will_not_read_is_quoted_as_unknown() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "");

        let installed = Installed::asked_when_needed(&host).expect("`claude` is on PATH");

        assert_eq!(installed.version(), "unknown");
    }

    #[test]
    fn a_machine_with_no_claude_code_is_answered_rather_than_refused() {
        assert_eq!(
            Installed::probed_or_absent(&FakeHost::new()).version(),
            "(not installed)"
        );
    }

    #[test]
    fn a_machine_that_has_one_is_answered_with_what_it_says() {
        let host = FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 0,
                    stdout: "2.1.221 (Claude Code)".to_string(),
                    stderr: String::new(),
                },
            );

        assert_eq!(Installed::probed_or_absent(&host).version(), "2.1.221");
    }

    fn versions_read_by(host: &FakeHost) -> usize {
        host.effects()
            .iter()
            .filter(|effect| {
                matches!(effect, crate::host::fake::Effect::Exec { program, args }
                    if program == "/usr/bin/claude" && args == &["--version".to_string()])
            })
            .count()
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
            PerchError::ProbeRefused(refusal) => {
                let crate::error::ProbeRefusal {
                    assumption,
                    detail,
                    version,
                    ..
                } = *refusal;
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

    /// The three ways a Credential stops being one Perch recognizes. Each names
    /// the store it came out of, so a refusal says which Account went wrong
    /// rather than that some Credential somewhere did.
    #[test]
    fn a_credential_that_is_not_json_names_the_store_it_came_out_of() {
        let refused = understand_credential(
            Zeroizing::new("not json at all".to_string()),
            "the Credential Perch holds for someone@example.com",
            &Installed::unknown("2.1.221"),
        )
        .expect_err("that is not JSON");

        let said = refused.to_string();
        assert!(said.contains(assumption::CREDENTIAL_SHAPE), "{said}");
        assert!(said.contains("someone@example.com"), "{said}");
        assert!(said.contains("is not JSON Perch understands"), "{said}");
    }

    /// A type error is the shape serde answers by quoting the value it read,
    /// and every value in this file is a secret. The refusal reaches a terminal
    /// and `--json` alike, so what it may not carry it may not carry at all.
    #[test]
    fn a_credential_of_the_wrong_shape_is_refused_without_quoting_what_was_in_it() {
        const TOKEN: &str = "sk-ant-oat01-must-not-be-said";

        for held in [
            format!(r#"{{"claudeAiOauth":"{{\"accessToken\":\"{TOKEN}\"}}"}}"#),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"a","expiresAt":"{TOKEN}"}}}}"#),
        ] {
            let refused = understand_credential(
                Zeroizing::new(held.clone()),
                "the keychain",
                &Installed::unknown("2.1.221"),
            )
            .err()
            .unwrap_or_else(|| panic!("the case this is about: {held}"));

            let said = refused.to_string();
            assert!(said.contains("is not JSON Perch understands"), "{said}");
            assert!(
                !said.contains(TOKEN),
                "the refusal carries the token: {said}"
            );
        }
    }

    /// The same rule for the file beside the Credential. `.claude.json` is not a
    /// secret in the way a Credential is, but it routinely carries an API key in
    /// an MCP server's `env` block — and serde quotes the value it tripped on.
    #[test]
    fn an_identity_file_of_the_wrong_shape_is_refused_without_quoting_what_was_in_it() {
        const KEY: &str = "sk-ant-api03-must-not-be-said";

        let host = FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("USER", "someone")
            .with_file(
                "/Users/someone/.claude.json",
                &format!(r#"{{"oauthAccount":{{"emailAddress":{{"held":"{KEY}"}}}}}}"#),
            );
        let store = default_store(&host).expect("the store is derivable");

        let refused = read_identity(&host, &store, &version_under_test())
            .expect_err("an address is not an object");

        let said = refused.to_string();
        assert!(said.contains("is not JSON Perch understands"), "{said}");
        assert!(!said.contains(KEY), "the refusal carries the key: {said}");
    }

    /// `perch status` writes the organization into the same labeled block as the
    /// address beside it, and the address is guarded against exactly this. A
    /// `\r` overwrites the line from column nought, taking the label and half
    /// the name with it. Both values, because both come out of this block —
    /// and refused here, because `registry::validate` refuses at `load`.
    #[test]
    fn a_block_a_terminal_would_obey_is_refused_where_it_enters() {
        for (block, names) in [
            (
                r#"{"emailAddress":"some\u001b[31mone@example.com"}"#,
                "names the account",
            ),
            (
                r#"{"emailAddress":"someone@example.com","organizationName":"Acme\u001b[31m Ltd"}"#,
                "names an organization",
            ),
        ] {
            let host = FakeHost::new()
                .with_env("HOME", "/Users/someone")
                .with_env("USER", "someone")
                .with_file(
                    "/Users/someone/.claude.json",
                    &format!(r#"{{"oauthAccount":{block}}}"#),
                );
            let store = default_store(&host).expect("the store is derivable");

            let refused = read_identity(&host, &store, &version_under_test())
                .expect_err("an escape is not something Perch can render");

            let said = refused.to_string();
            assert!(said.contains("control character (U+001B)"), "{said}");
            assert!(said.contains(names), "{said}");
        }
    }

    #[test]
    fn a_credential_with_no_oauth_block_is_refused_as_the_wrong_shape() {
        let refused = understand_credential(
            Zeroizing::new(r#"{"somethingElse": {}}"#.to_string()),
            "the keychain",
            &Installed::unknown("2.1.221"),
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
            Zeroizing::new(r#"{"claudeAiOauth": {"refreshToken": "sk-ant-ort01-x"}}"#.to_string()),
            "the keychain",
            &Installed::unknown("2.1.221"),
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

        let credential = understand_credential(
            Zeroizing::new(raw.to_string()),
            "the keychain",
            &version_under_test(),
        )
        .expect("that is a Credential");

        assert_eq!(*credential.access_token, "sk-ant-oat01-x");
        assert_eq!(
            credential.refresh_token.as_ref().map(|t| t.as_str()),
            Some("sk-ant-ort01-x")
        );
        assert_eq!(
            credential.as_str(),
            raw,
            "the bytes are copied rather than re-serialized"
        );
    }

    /// A Rotation rewrites one key of a Credential Perch already holds, so the
    /// two ways that Credential could be unreadable are refusals about the
    /// Credential being renewed rather than about the reply.
    #[test]
    fn a_rotation_onto_a_credential_that_is_not_json_is_refused() {
        let broken = Credential {
            raw: Zeroizing::new("{ not json".to_string()),
            access_token: Zeroizing::new("sk-ant-oat01-old".to_string()),
            refresh_token: None,
            expires_at: None,
            subscription_type: None,
        };

        let refused = credential_after_rotation(
            &broken,
            "sk-ant-oat01-new",
            None,
            None,
            &version_under_test(),
        )
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
            raw: Zeroizing::new(r#"{"somethingElse": {}}"#.to_string()),
            access_token: Zeroizing::new("sk-ant-oat01-old".to_string()),
            refresh_token: None,
            expires_at: None,
            subscription_type: None,
        };

        let refused = credential_after_rotation(
            &broken,
            "sk-ant-oat01-new",
            None,
            None,
            &version_under_test(),
        )
        .expect_err("there is no block to write into");

        assert!(
            refused
                .to_string()
                .contains("the Credential being renewed has no claudeAiOauth block"),
            "{refused}"
        );
    }
}
