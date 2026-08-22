//! Failures Perch reports, and the exit codes they map to.
//!
//! Exit codes are part of the interface: a shell prompt or a script needs to
//! tell "this account is gone" from "the keychain is locked" from "Perch does
//! not recognize this Claude Code" without parsing prose.

use std::path::PathBuf;

use crate::keychain::KeychainError;

/// Exit code returned when everything worked.
pub const EXIT_OK: i32 = 0;
/// Exit code for a failure with no more specific meaning.
pub const EXIT_GENERAL: i32 = 1;
/// Exit code for a command line Perch could not read as one. The argument
/// parser's own code, because a line Perch rejects itself and a line the parser
/// rejects are the same failure to the script wrapping it, and two codes for it
/// would be a distinction nobody could act on.
pub const EXIT_NOT_UNDERSTOOD: i32 = 2;
/// Exit code for a refused operation: an assumption about Claude Code failed.
pub const EXIT_PROBE_REFUSED: i32 = 10;
/// Exit code for a keychain that is locked, denied, or otherwise unavailable.
pub const EXIT_KEYCHAIN_UNAVAILABLE: i32 = 11;
/// Exit code for a target that does not exist — no login, no such Account.
pub const EXIT_NOT_FOUND: i32 = 12;
/// Exit code for a request that collides with something that is already there:
/// an Account added twice, a name already spoken for, a path an Export would
/// have written over.
pub const EXIT_CONFLICT: i32 = 13;
/// Exit code for a request Perch understood and refused on its own terms: a
/// name it will not accept, a configured value outside the range it means
/// something in, a command that needs a terminal run where there is none.
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
/// (ADR a-group-is-a-declaration).
pub const EXIT_NOT_INTERCHANGEABLE: i32 = 18;
/// Exit code for something asked of an Account whose Credential no longer works.
/// Distinct from every other refusal because the repair is distinct: no amount
/// of retrying, enabling or re-targeting repairs it, and `perch relogin` does.
pub const EXIT_QUARANTINED: i32 = 19;
/// Exit code for a check that decided nothing because it had no current figure
/// to decide on: `perch watcher check` held, and the Refresh it held for failed
/// (ADR a-watcher-knob-is-arithmetic). Distinct from having nowhere to go,
/// because a scheduler retrying shortly needs to tell the two apart — only one
/// of them resolves itself.
pub const EXIT_HELD: i32 = 20;

#[derive(Debug, thiserror::Error)]
pub enum PerchError {
    /// The probe does not recognize the installed Claude Code well enough to
    /// touch anything. Names the assumption that failed
    /// (ADR an-assumption-is-probed).
    #[error("Perch declined to act: {assumption} ({detail}), Claude Code {version}")]
    ProbeRefused {
        assumption: String,
        detail: String,
        version: String,
    },

    /// The keychain could not be consulted at all. Deliberately distinct from
    /// "not found", which reads as an Account having vanished
    /// (ADR claude-code-chooses-the-store).
    #[error("Keychain unavailable: {0}")]
    KeychainUnavailable(String),

    /// The command line could not be read as one — a flag Perch cannot tell
    /// apart from the launched program's. Distinct from [`PerchError::Invalid`],
    /// which is a request Perch read and declined: nothing here was understood
    /// well enough to decline, so the message has to end in the line that would
    /// have worked.
    #[error("{0}")]
    NotUnderstood(String),

    /// Something Perch was asked about does not exist.
    #[error("{0}")]
    NotFound(String),

    /// The request collides with something that is already there, and naming
    /// what is in the way is the whole of the answer.
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

    /// Another `perch` is holding a lock this one waited out. Nothing is wrong
    /// and nothing was changed — the answer is to ask again shortly, which is
    /// why it carries [`EXIT_HELD`] rather than a general failure: a scheduler
    /// and the watcher's own loop both need to tell "come back in a minute"
    /// from "this will fail the same way for ever".
    #[error("{0}")]
    Busy(String),

    /// A Cycle found nowhere worth landing. Says which Account frees up
    /// soonest, so waiting is a decision the user makes rather than one Perch
    /// makes for them by switching somewhere useless.
    #[error("{0}")]
    NoCandidate(String),

    /// A bare Cycle from an Account whose interchangeability nobody has
    /// declared. Names both ways to declare it.
    #[error("{0}")]
    NotInterchangeable(String),

    /// Something was asked of an Account that is Quarantined. The reason
    /// travels beside the sentence rather than only inside it, because the
    /// command that discovers a Quarantine has to record it and a reason
    /// inferred back out of prose goes wrong the day a second kind is raised
    /// nearby (ADR a-refusal-is-a-promise).
    #[error("{said}")]
    Quarantined {
        why: crate::registry::Quarantine,
        said: String,
    },

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

    /// A file Perch could not make sense of: which file, and what stopped it —
    /// deliberately not *which kind* of nonsense. Every way serde declines a
    /// well-formed document arrives here, as does an `import` of a Credential
    /// belonging to no Account, and naming a syntax error sends the reader past
    /// the half of the sentence that says what is wrong.
    #[error("Perch could not read {path}: {detail}")]
    Malformed { path: String, detail: String },

    #[error("{0}")]
    Other(String),
}

/// The refusal a build raises rather than half-reading something a newer Perch
/// wrote. Said once because both formats owe the reader the same three things:
/// what it was, how far ahead it is, and that upgrading is the way through. A
/// registry merely *older* migrates forward instead, and which of the two a
/// format gets turns on what a refusal costs (ADR the-holdings-outlive-a-perch).
pub fn written_by_a_newer_perch(what: &str, of: &str, version: u64, understood: u32) -> PerchError {
    PerchError::Other(format!(
        "{what} was written by a newer Perch ({of} version {version}, this build \
         understands {understood}). Upgrade Perch."
    ))
}

/// What version a document claims to be, asked ahead of the parse — because a
/// newer Perch is exactly what writes a value this build has no variant for, so
/// parsing first fails in serde's words about a document that is well-formed.
/// Nothing rather than an error where it will not parse at all: what to make of
/// nonsense is the caller's to say, about its own file.
pub fn claimed_version(contents: &str) -> Option<u64> {
    #[derive(serde::Deserialize)]
    struct Versioned {
        version: Option<serde_json::Value>,
    }

    // A `u64`, so a number above this build's ceiling reads as the newer Perch
    // it is rather than as serde's words about an integer that will not fit.
    // Anything that is no whole number claims no version at all.
    serde_json::from_str::<Versioned>(contents)
        .ok()?
        .version?
        .as_u64()
}

impl PerchError {
    /// A file that could not be read, from whatever said so.
    ///
    /// What fails is a [`HostError`](crate::host::HostError) and what the
    /// variant carries is an `io::Error`, so the laundering between them is
    /// said once rather than at each of the six places that report one.
    pub fn file_read(path: impl Into<PathBuf>, why: impl std::fmt::Display) -> PerchError {
        PerchError::FileRead {
            path: path.into(),
            source: std::io::Error::other(why.to_string()),
        }
    }

    /// The same, for a file that could not be written.
    pub fn file_write(path: impl Into<PathBuf>, why: impl std::fmt::Display) -> PerchError {
        PerchError::FileWrite {
            path: path.into(),
            source: std::io::Error::other(why.to_string()),
        }
    }

    /// The same failure, with a line about what it left behind.
    ///
    /// The failure belongs to whatever failed and what it left belongs to
    /// whatever was running the sequence, so both are said. The kind is kept,
    /// so the exit code a script branches on is the one the failure earned.
    pub fn with_note(mut self, note: &str) -> PerchError {
        // Where keeping the kind matters most is `Busy`: it is the one failure
        // that resolves on its own, and a note about what was left behind must
        // not cost a scheduler the fact that retrying works.
        match self.message_mut() {
            Some(message) => {
                message.push_str(&format!("\n\n{note}"));
                self
            }
            // The rest carry structure rather than a message. They all exit as
            // a general failure already, so folding them into one loses the
            // shape and nothing a caller could act on.
            None => PerchError::Other(format!("{self}\n\n{note}")),
        }
    }

    /// The one sentence a variant carries, or `None` for one built out of
    /// fields instead. Written once because thirteen arms rebuilding their own
    /// variant is thirteen chances for one of them to be spelled differently,
    /// and the difference would be an exit code changing under a note.
    fn message_mut(&mut self) -> Option<&mut String> {
        match self {
            PerchError::ProbeRefused { detail, .. } => Some(detail),
            PerchError::Quarantined { said, .. } => Some(said),
            PerchError::KeychainUnavailable(message)
            | PerchError::NotUnderstood(message)
            | PerchError::NotFound(message)
            | PerchError::Conflict(message)
            | PerchError::Invalid(message)
            | PerchError::NothingToDo(message)
            | PerchError::Busy(message)
            | PerchError::ProfileLive(message)
            | PerchError::NoCandidate(message)
            | PerchError::NotInterchangeable(message)
            | PerchError::Other(message) => Some(message),
            // Spelled out rather than caught by a `_`, and the same at
            // `exit_code`: a variant added later would otherwise be folded into
            // `Other` and exit as a general failure, silently.
            PerchError::FileRead { .. }
            | PerchError::FileWrite { .. }
            | PerchError::Malformed { .. } => None,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            PerchError::ProbeRefused { .. } => EXIT_PROBE_REFUSED,
            PerchError::KeychainUnavailable(_) => EXIT_KEYCHAIN_UNAVAILABLE,
            PerchError::NotUnderstood(_) => EXIT_NOT_UNDERSTOOD,
            PerchError::NotFound(_) => EXIT_NOT_FOUND,
            PerchError::Conflict(_) => EXIT_CONFLICT,
            PerchError::Invalid(_) => EXIT_INVALID,
            PerchError::NothingToDo(_) => EXIT_NOTHING_TO_DO,
            PerchError::ProfileLive(_) => EXIT_PROFILE_LIVE,
            PerchError::NoCandidate(_) => EXIT_NO_CANDIDATE,
            PerchError::NotInterchangeable(_) => EXIT_NOT_INTERCHANGEABLE,
            PerchError::Quarantined { .. } => EXIT_QUARANTINED,
            PerchError::Busy(_) => EXIT_HELD,
            PerchError::FileRead { .. }
            | PerchError::FileWrite { .. }
            | PerchError::Malformed { .. }
            | PerchError::Other(_) => EXIT_GENERAL,
        }
    }
}

impl From<KeychainError> for PerchError {
    fn from(err: KeychainError) -> Self {
        match err {
            KeychainError::NotFound { service, account } => PerchError::NotFound(format!(
                "No credential stored for {account} under {service}"
            )),
            // Spelled out rather than caught by a `_`, for the reason
            // `message_mut` gives: a variant added later would be folded into
            // "unavailable" silently.
            KeychainError::Unavailable { detail } => PerchError::KeychainUnavailable(detail),
        }
    }
}

pub type Result<T> = std::result::Result<T, PerchError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Quarantine;

    /// One of every variant, so the tables below are about the whole enum
    /// rather than whichever variants somebody remembered. A variant added
    /// without a line here is one nothing checks.
    fn one_of_each() -> Vec<(&'static str, PerchError)> {
        vec![
            (
                "ProbeRefused",
                PerchError::ProbeRefused {
                    assumption: "the credential lives in the keychain".to_string(),
                    detail: "it does not".to_string(),
                    version: "2.1.221".to_string(),
                },
            ),
            (
                "KeychainUnavailable",
                PerchError::KeychainUnavailable("locked".to_string()),
            ),
            (
                "NotUnderstood",
                PerchError::NotUnderstood("--json before --".to_string()),
            ),
            (
                "NotFound",
                PerchError::NotFound("no such Account".to_string()),
            ),
            (
                "Conflict",
                PerchError::Conflict("already added".to_string()),
            ),
            (
                "Invalid",
                PerchError::Invalid("not a percentage".to_string()),
            ),
            (
                "NothingToDo",
                PerchError::NothingToDo("already active".to_string()),
            ),
            (
                "ProfileLive",
                PerchError::ProfileLive("a client is running".to_string()),
            ),
            (
                "Busy",
                PerchError::Busy("another perch holds it".to_string()),
            ),
            (
                "NoCandidate",
                PerchError::NoCandidate("everything is exhausted".to_string()),
            ),
            (
                "NotInterchangeable",
                PerchError::NotInterchangeable("nobody said so".to_string()),
            ),
            (
                "Quarantined",
                PerchError::Quarantined {
                    why: Quarantine::RenewalRejected,
                    said: "it is Quarantined".to_string(),
                },
            ),
            (
                "FileRead",
                PerchError::file_read("/tmp/registry.json", "denied"),
            ),
            (
                "FileWrite",
                PerchError::file_write("/tmp/registry.json", "read-only"),
            ),
            (
                "Malformed",
                PerchError::Malformed {
                    path: "/tmp/registry.json".to_string(),
                    detail: "expected `,`".to_string(),
                },
            ),
            ("Other", PerchError::Other("something else".to_string())),
        ]
    }

    #[test]
    fn a_note_never_changes_the_exit_code_the_failure_earned() {
        for (name, error) in one_of_each() {
            let earned = error.exit_code();
            let noted = error.with_note("Nothing was removed.");
            assert_eq!(
                noted.exit_code(),
                earned,
                "{name} changed its exit code when a note was added"
            );
        }
    }

    /// Where the note lands differs by variant — [`PerchError::ProbeRefused`]
    /// renders its detail mid-sentence — so what is asserted is that nothing
    /// else about the message moved.
    #[test]
    fn a_note_is_said_alongside_what_failed_rather_than_instead_of_it() {
        for (name, error) in one_of_each() {
            let was = error.to_string();
            let noted = error.with_note("Nothing was removed.").to_string();

            assert!(
                noted.contains("Nothing was removed."),
                "{name} dropped the note: {noted}"
            );
            assert_eq!(
                noted.replace("\n\nNothing was removed.", ""),
                was,
                "{name} changed what it said for more than the note"
            );
        }
    }

    #[test]
    fn a_second_note_lands_beside_the_first_rather_than_swallowing_it() {
        let noted = PerchError::NotFound("no such Account".to_string())
            .with_note("first")
            .with_note("second");

        assert_eq!(noted.exit_code(), EXIT_NOT_FOUND);
        let said = noted.to_string();
        assert!(said.contains("first") && said.contains("second"), "{said}");
        assert!(
            said.find("first") < said.find("second"),
            "the notes are in the order they were added: {said}"
        );
    }

    #[test]
    fn a_noted_quarantine_still_carries_the_reason_it_was_raised_for() {
        let noted = PerchError::Quarantined {
            why: Quarantine::RotationLost,
            said: "it is Quarantined".to_string(),
        }
        .with_note("Nothing was switched.");

        match noted {
            PerchError::Quarantined { why, said } => {
                assert_eq!(why, Quarantine::RotationLost);
                assert!(said.contains("Nothing was switched."), "{said}");
            }
            other => panic!("a noted Quarantine is still one: {other:?}"),
        }
    }

    /// These fold into [`PerchError::Other`], which is the one case where what
    /// was said about the file has to survive a change of variant.
    #[test]
    fn a_note_on_a_structured_failure_keeps_what_it_said_about_the_file() {
        let noted = PerchError::file_write("/tmp/registry.json", "read-only")
            .with_note("Perch's registry is untouched.");

        assert_eq!(noted.exit_code(), EXIT_GENERAL);
        let said = noted.to_string();
        assert!(said.contains("/tmp/registry.json"), "{said}");
        assert!(said.contains("read-only"), "{said}");
        assert!(said.contains("Perch's registry is untouched."), "{said}");
    }

    #[test]
    fn every_kind_of_failure_earns_the_code_a_script_branches_on() {
        let expected = [
            (EXIT_PROBE_REFUSED, "ProbeRefused"),
            (EXIT_KEYCHAIN_UNAVAILABLE, "KeychainUnavailable"),
            (EXIT_NOT_UNDERSTOOD, "NotUnderstood"),
            (EXIT_NOT_FOUND, "NotFound"),
            (EXIT_CONFLICT, "Conflict"),
            (EXIT_INVALID, "Invalid"),
            (EXIT_NOTHING_TO_DO, "NothingToDo"),
            (EXIT_PROFILE_LIVE, "ProfileLive"),
            (EXIT_HELD, "Busy"),
            (EXIT_NO_CANDIDATE, "NoCandidate"),
            (EXIT_NOT_INTERCHANGEABLE, "NotInterchangeable"),
            (EXIT_QUARANTINED, "Quarantined"),
            (EXIT_GENERAL, "FileRead"),
            (EXIT_GENERAL, "FileWrite"),
            (EXIT_GENERAL, "Malformed"),
            (EXIT_GENERAL, "Other"),
        ];

        let actual: Vec<(i32, &str)> = one_of_each()
            .iter()
            .map(|(name, error)| (error.exit_code(), *name))
            .collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn the_codes_that_mean_different_things_are_different_numbers() {
        let mut codes = vec![
            EXIT_OK,
            EXIT_GENERAL,
            EXIT_NOT_UNDERSTOOD,
            EXIT_PROBE_REFUSED,
            EXIT_KEYCHAIN_UNAVAILABLE,
            EXIT_NOT_FOUND,
            EXIT_CONFLICT,
            EXIT_INVALID,
            EXIT_NOTHING_TO_DO,
            EXIT_PROFILE_LIVE,
            EXIT_NO_CANDIDATE,
            EXIT_NOT_INTERCHANGEABLE,
            EXIT_QUARANTINED,
            EXIT_HELD,
        ];
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count, "two failures share an exit code");
    }

    /// Different problems with different repairs, and this conversion is where
    /// the distinction is either kept or lost.
    #[test]
    fn a_missing_item_is_not_found_and_anything_else_is_the_keychain_being_unavailable() {
        let missing: PerchError = KeychainError::NotFound {
            service: "Claude Code-credentials".to_string(),
            account: "someone".to_string(),
        }
        .into();
        assert_eq!(missing.exit_code(), EXIT_NOT_FOUND);
        let said = missing.to_string();
        assert!(said.contains("someone"), "{said}");
        assert!(said.contains("Claude Code-credentials"), "{said}");

        let locked: PerchError = KeychainError::Unavailable {
            detail: "User interaction is not allowed".to_string(),
        }
        .into();
        assert_eq!(locked.exit_code(), EXIT_KEYCHAIN_UNAVAILABLE);
        assert!(
            locked
                .to_string()
                .contains("User interaction is not allowed"),
            "what the keychain said is what the user needs: {locked}"
        );
    }

    #[test]
    fn a_file_written_by_a_newer_perch_says_how_far_ahead_it_is_and_what_to_do() {
        let refused = written_by_a_newer_perch("The registry", "registry", 4, 2);

        assert_eq!(refused.exit_code(), EXIT_GENERAL);
        let said = refused.to_string();
        assert!(said.contains("The registry"), "{said}");
        assert!(said.contains("version 4"), "{said}");
        assert!(said.contains("understands 2"), "{said}");
        assert!(said.contains("Upgrade Perch."), "{said}");
    }
}
