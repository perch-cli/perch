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
/// Exit code for a request Perch understood and refused on its own terms: a
/// name it will not accept, a configured value outside the range it means
/// something in.
pub const EXIT_INVALID: i32 = 14;
/// Exit code for a request that was already true: the Account asked for is the
/// one that is already active. Distinct from success, so a script can tell a
/// Switch that happened from one that was not needed.
pub const EXIT_NOTHING_TO_DO: i32 = 15;
/// Exit code for a refusal to touch a Profile a client is running against.
pub const EXIT_PROFILE_LIVE: i32 = 16;
/// Exit code for a Cycle with nowhere to land: every Account in the Group is
/// exhausted, or none of them is a candidate at all. Distinct from "nothing to
/// do", because waiting is the answer here and there is nothing to wait for
/// there.
pub const EXIT_NO_CANDIDATE: i32 = 17;
/// Exit code for a bare Cycle among Accounts nobody has declared
/// interchangeable — the ungrouped pool, with the setting that governs it off
/// (ADR 0017).
pub const EXIT_NOT_INTERCHANGEABLE: i32 = 18;

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

    /// The request was understood and is not one Perch will accept — a Group
    /// name that would be ambiguous, a threshold that is not a percentage.
    #[error("{0}")]
    Invalid(String),

    /// What was asked for is already so, and doing it would mean rewriting
    /// Credentials for nothing.
    #[error("{0}")]
    NothingToDo(String),

    /// A client is running against the Profile, so its Credential belongs to
    /// that client until it exits.
    #[error("{0}")]
    ProfileLive(String),

    /// A Cycle found nowhere worth landing. Says which Account frees up
    /// soonest, so waiting is a decision the user makes rather than one Perch
    /// makes for them by switching somewhere useless.
    #[error("{0}")]
    NoCandidate(String),

    /// A bare Cycle from an Account whose interchangeability nobody has
    /// declared. Names both ways to declare it (ADR 0017).
    #[error("{0}")]
    NotInterchangeable(String),

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
    /// The same failure, with a line about what it left behind.
    ///
    /// A step that fails part way through a sequence has to say what happened
    /// *and* what the machine is holding now, and those are two different
    /// pieces of knowledge: the failure belongs to whatever failed, and what it
    /// left belongs to whatever was running the sequence. The kind is kept, so
    /// the exit code a script branches on is still the one the failure earned.
    pub fn with_note(self, note: &str) -> PerchError {
        match self {
            PerchError::ProbeRefused {
                assumption,
                detail,
                version,
            } => PerchError::ProbeRefused {
                assumption,
                detail: format!("{detail}\n\n{note}"),
                version,
            },
            PerchError::KeychainUnavailable(message) => {
                PerchError::KeychainUnavailable(format!("{message}\n\n{note}"))
            }
            PerchError::NotFound(message) => PerchError::NotFound(format!("{message}\n\n{note}")),
            PerchError::Conflict(message) => PerchError::Conflict(format!("{message}\n\n{note}")),
            PerchError::Invalid(message) => PerchError::Invalid(format!("{message}\n\n{note}")),
            PerchError::NothingToDo(message) => {
                PerchError::NothingToDo(format!("{message}\n\n{note}"))
            }
            PerchError::ProfileLive(message) => {
                PerchError::ProfileLive(format!("{message}\n\n{note}"))
            }
            PerchError::NoCandidate(message) => {
                PerchError::NoCandidate(format!("{message}\n\n{note}"))
            }
            PerchError::NotInterchangeable(message) => {
                PerchError::NotInterchangeable(format!("{message}\n\n{note}"))
            }
            // The rest carry structure rather than a message. They all exit as
            // a general failure already, so folding them into one loses the
            // shape and nothing a caller could act on.
            other => PerchError::Other(format!("{other}\n\n{note}")),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            PerchError::ProbeRefused { .. } => EXIT_PROBE_REFUSED,
            PerchError::KeychainUnavailable(_) => EXIT_KEYCHAIN_UNAVAILABLE,
            PerchError::NotFound(_) => EXIT_NOT_FOUND,
            PerchError::Conflict(_) => EXIT_CONFLICT,
            PerchError::Invalid(_) => EXIT_INVALID,
            PerchError::NothingToDo(_) => EXIT_NOTHING_TO_DO,
            PerchError::ProfileLive(_) => EXIT_PROFILE_LIVE,
            PerchError::NoCandidate(_) => EXIT_NO_CANDIDATE,
            PerchError::NotInterchangeable(_) => EXIT_NOT_INTERCHANGEABLE,
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
