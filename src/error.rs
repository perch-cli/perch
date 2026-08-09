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
/// (ADR 0017).
pub const EXIT_NOT_INTERCHANGEABLE: i32 = 18;
/// Exit code for something asked of an Account whose Credential no longer works.
/// Distinct from every other refusal because the repair is distinct: no amount
/// of retrying, enabling or re-targeting repairs it, and `perch relogin` does.
pub const EXIT_QUARANTINED: i32 = 19;
/// Exit code for a check that decided nothing because it had no current figure
/// to decide on: `perch watch --once` held, and the Refresh it held for failed
/// (ADR 0013). Distinct from having nowhere to go, because a scheduler retrying
/// shortly needs to tell the two apart — only one of them resolves itself.
pub const EXIT_HELD: i32 = 20;

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
    /// declared. Names both ways to declare it (ADR 0017).
    #[error("{0}")]
    NotInterchangeable(String),

    /// Something was asked of an Account that is Quarantined. Says what it was
    /// Quarantined for and how to repair it, because those are the two things
    /// that turn a dead end into a next step.
    ///
    /// The reason travels beside the message rather than only inside it: the
    /// command that discovers a Quarantine is the one that has to record it,
    /// and a reason it had to infer from the failure would be a reason that
    /// goes wrong the day a second kind of Quarantine is raised nearby.
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

    #[error("{path} is not valid JSON: {detail}")]
    Malformed { path: String, detail: String },

    #[error("{0}")]
    Other(String),
}

/// The refusal a build raises rather than half-reading something a newer Perch
/// wrote.
///
/// Both of Perch's own formats are versioned, and the version is a guard against
/// the future rather than a migration story: nobody is running Perch yet, so
/// there is no past format to read. What there is, and what this exists for, is
/// the machine holding two builds — a registry written by the newer one, an
/// Export restored by the older — where the wrong answer is a file half-read
/// rather than refused.
///
/// Said once because the two formats owe the reader the same three things: what
/// it was, how far ahead it is, and that upgrading is the way through.
pub fn written_by_a_newer_perch(what: &str, of: &str, version: u32, understood: u32) -> PerchError {
    PerchError::Other(format!(
        "{what} was written by a newer Perch ({of} version {version}, this build \
         understands {understood}). Upgrade Perch."
    ))
}

impl PerchError {
    /// A file that could not be read, from whatever said so.
    ///
    /// Every caller of this used to spell out the same
    /// `std::io::Error::other(err.to_string())` laundering, because what fails
    /// is a [`HostError`](crate::host::HostError) and what the variant carries
    /// is an `io::Error`. Said once, so the six places that report a file
    /// failure report it the same way.
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
    /// A step that fails part way through a sequence has to say what happened
    /// *and* what the machine is holding now, and those are two different
    /// pieces of knowledge: the failure belongs to whatever failed, and what it
    /// left belongs to whatever was running the sequence. The kind is kept, so
    /// the exit code a script branches on is still the one the failure earned.
    pub fn with_note(mut self, note: &str) -> PerchError {
        // Every variant that carries its own sentence keeps its kind, so the
        // exit code a script branches on survives the note — including
        // [`PerchError::Busy`], where it matters most: a lock somebody else is
        // holding is the one failure that resolves on its own, and a Remove or
        // a Relogin noting what it left behind must not cost the scheduler
        // reading the code the fact that retrying works.
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

    /// The one sentence a variant carries, for the things that add to it rather
    /// than replace it.
    ///
    /// `None` is a variant built out of fields instead — a path and a reason,
    /// say — which has no single sentence to append to. Written once because
    /// thirteen arms spelling out `Variant(m) => Variant(format!("{m}…"))` is
    /// thirteen chances for one of them to be spelled differently, and the
    /// difference would be an exit code silently changing under a note.
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
            // `Other` by a note and exit as a general failure, silently, and
            // `one_of_each` would not be extended to notice. Three arms is the
            // price of the compiler asking.
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
            other => PerchError::KeychainUnavailable(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, PerchError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Quarantine;

    /// One of every variant, so the tables below are about the whole enum
    /// rather than about whichever variants somebody remembered. A variant
    /// added without a line here is one the note and the exit code say nothing
    /// about, and nothing else would notice.
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

    /// The property the whole of `with_note` rests on: a sequence that failed
    /// part way through can say what the machine is holding now without costing
    /// the script wrapping it the code it branches on.
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

    /// A note is added to what failed rather than said instead of it. Where it
    /// lands differs by variant — [`PerchError::ProbeRefused`] renders its
    /// detail mid-sentence, so the note goes inside that rather than after the
    /// version — but nothing else about the message moves either way.
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

    /// The variants that carry a message keep their shape, so a second note
    /// lands beside the first rather than folding the whole thing into
    /// [`PerchError::Other`] on the way.
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

    /// A Quarantine travels beside the message rather than only inside it, so
    /// the command that has to record why still can after a note is added.
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

    /// The variants that carry structure rather than a message exit as a
    /// general failure already, so folding them loses nothing a caller could
    /// act on — but what they said about the file has to survive the fold.
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

    /// No two codes mean the same thing, because telling one failure from
    /// another is the whole reason they are numbered rather than prose.
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

    /// A keychain with no such item and a keychain that will not answer are
    /// different problems with different repairs, and this conversion is where
    /// that distinction is either kept or lost (ADR 0008).
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
