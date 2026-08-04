//! Failures Perch reports, and the exit codes they map to.
//!
//! Exit codes are part of the interface: a shell prompt or a script needs to
//! tell "this account is gone" from "the keychain is locked" from "Perch does
//! not recognise this Claude Code" without parsing prose.

use std::path::PathBuf;

use crate::keychain::KeychainError;

/// Exit code returned when everything worked.
pub const EXIT_OK: i32 = 0;
/// Exit code for a failure with no more specific meaning.
pub const EXIT_GENERAL: i32 = 1;
/// Exit code for a refused operation: an assumption about Claude Code failed.
pub const EXIT_PROBE_REFUSED: i32 = 10;
/// Exit code for a keychain that is locked, denied, or otherwise unavailable.
pub const EXIT_KEYCHAIN_UNAVAILABLE: i32 = 11;
/// Exit code for a target that does not exist — no login, no such Account.
pub const EXIT_NOT_FOUND: i32 = 12;
/// Exit code for a request that collides with what Perch already holds: an
/// Account added twice, a name already spoken for.
pub const EXIT_CONFLICT: i32 = 13;

#[derive(Debug, thiserror::Error)]
pub enum PerchError {
    /// The probe does not recognise the installed Claude Code well enough to
    /// touch anything. Names the assumption that failed (ADR 0007).
    #[error("Perch declined to act: {assumption} ({detail}), Claude Code {version}")]
    ProbeRefused {
        assumption: String,
        detail: String,
        version: String,
    },

    /// The keychain could not be consulted at all. Deliberately distinct from
    /// "not found", which reads as an Account having vanished (ADR 0008).
    #[error("Keychain unavailable: {0}")]
    KeychainUnavailable(String),

    /// Something Perch was asked about does not exist.
    #[error("{0}")]
    NotFound(String),

    /// The request collides with something Perch already holds, and naming the
    /// existing entry is the whole of the answer.
    #[error("{0}")]
    Conflict(String),

    #[error("Could not read {path}: {source}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Could not write {path}: {source}")]
    FileWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid JSON: {detail}")]
    Malformed { path: String, detail: String },

    #[error("{0}")]
    Other(String),
}

impl PerchError {
    pub fn exit_code(&self) -> i32 {
        match self {
            PerchError::ProbeRefused { .. } => EXIT_PROBE_REFUSED,
            PerchError::KeychainUnavailable(_) => EXIT_KEYCHAIN_UNAVAILABLE,
            PerchError::NotFound(_) => EXIT_NOT_FOUND,
            PerchError::Conflict(_) => EXIT_CONFLICT,
            _ => EXIT_GENERAL,
        }
    }
}

impl From<KeychainError> for PerchError {
    fn from(err: KeychainError) -> Self {
        match err {
            KeychainError::NotFound { service, account } => PerchError::NotFound(format!(
                "No credential stored for {account} under {service}"
            )),
            other => PerchError::KeychainUnavailable(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, PerchError>;
