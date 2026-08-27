//! The `/usr/bin/security` protocol, and none of the spawning: building a
//! command line, deciding which way it must travel, and reading an exit status.
//! The process itself is started in [`crate::host::real`], where every other
//! process Perch starts is.
//!
//! The four constraints that come with that binary are
//! ADR claude-code-chooses-the-store's, and this module is the only place they
//! are spelled.

use zeroize::Zeroize;

use crate::host::{Execution, write_double_quoted};
use crate::secret::Secret;

/// The `security` binary. Never a build of Perch, never a crate — see
/// ADR a-crate-must-not-cost-a-seam.
pub const SECURITY_BIN: &str = "/usr/bin/security";

/// `security` exit code meaning "the item is not in the keychain".
///
/// Reached through a lock: `security` matches an item's attributes before it
/// needs the keychain open, so a name nothing is stored under exits 44 whether
/// the keychain is locked or not.
pub const EXIT_ITEM_NOT_FOUND: i32 = 44;

/// The `-i` stdin buffer size. A command line at or above this is truncated
/// mid-argument, so writes that long take the argv path instead.
pub const STDIN_BUFFER_LIMIT: usize = 4096;

/// How much room is left below that limit before a write takes the argv path.
/// Nothing about the buffer is documented, and overflow corrupts the item
/// silently, so the fallback happens with headroom rather than at the edge.
pub const STDIN_SAFETY_MARGIN: usize = 256;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeychainError {
    #[error("no such item: {account} under {service}")]
    NotFound { service: String, account: String },

    /// Locked, access denied, or the keychain daemon is unreachable. Anything
    /// that is not exit 44.
    #[error("{detail}")]
    Unavailable { detail: String },
}

/// How a write reached `security`. Recorded so tests can assert that ordinary
/// Credentials never travel through `argv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePath {
    /// Hex-encoded, piped through `security -i`. The normal path.
    Stdin,
    /// Hex-encoded on the command line, taken only when the `-i` form would
    /// overflow the 4096-byte buffer and be silently truncated.
    Argv,
}

/// Turns a `security` exit status into the distinction that matters.
pub fn classify(execution: &Execution, service: &str, account: &str) -> KeychainError {
    if execution.status == EXIT_ITEM_NOT_FOUND {
        KeychainError::NotFound {
            service: service.to_string(),
            account: account.to_string(),
        }
    } else {
        let detail = execution.stderr.trim();
        let detail = if detail.is_empty() {
            format!("security exited {}", execution.status)
        } else {
            detail.to_string()
        };
        KeychainError::Unavailable { detail }
    }
}

/// The `-i` command line for a write, exactly as it is piped to `security`.
/// `-U` updates rather than failing; `-X` takes the secret as hex, so no byte
/// of it is quoted, escaped or logged. A control character is refused rather
/// than quoted: `-i` reads one sub-command per line, and reports a failed one
/// on stderr while still exiting 0.
pub fn add_command_line(
    service: &str,
    account: &str,
    secret: &str,
) -> Result<Secret, KeychainError> {
    storable(service, account)?;
    // A `Secret` because this line holds the Credential, and pushed into rather
    // than `format!`ed because a `format!` of one is a second buffer nothing
    // wipes — the width is what keeps this one from having to move.
    let mut line = Secret::with_room_for(
        FIXED_WIDTH + service.len() * 2 + account.len() * 2 + secret.len() * 2,
    );
    line.push_str("add-generic-password -U -s ");
    write_double_quoted(&mut line, service);
    line.push_str(" -a ");
    write_double_quoted(&mut line, account);
    line.push_str(" -X ");
    line.push_str(hex_encode(secret.as_bytes()).as_str());
    line.push('\n');
    Ok(line)
}

/// Room for everything in [`add_command_line`] that is not one of the three values,
/// over-counted: the sub-command name, the two flags and the newline come to 36.
const FIXED_WIDTH: usize = 48;

/// What no adapter may store, whichever one is answering.
///
/// `security -i` reads one sub-command per line, so a name carrying a control
/// character would end the `add-generic-password` line and begin whatever the
/// rest of it spelled. Beside [`add_command_line`] so the fake asks it too.
pub fn storable(service: &str, account: &str) -> Result<(), KeychainError> {
    inert("the keychain service name", service)?;
    inert("the keychain account name", account)
}

/// Refuses a value that would be punctuation rather than a value.
fn inert(what: &str, value: &str) -> Result<(), KeychainError> {
    match crate::host::control_character_in(value) {
        Some(said) => Err(KeychainError::Unavailable {
            detail: format!(
                "{what} carries {said}, which `security` would read as the end \
                 of one command and the start of another"
            ),
        }),
        None => Ok(()),
    }
}

/// Which path a write of this size must take.
///
/// Writes *near* the limit fall back, not writes past it: the byte at which
/// `security` starts truncating is an observation about one build of it.
pub fn write_path_for(command_line: &str) -> WritePath {
    if command_line.len() >= STDIN_BUFFER_LIMIT - STDIN_SAFETY_MARGIN {
        WritePath::Argv
    } else {
        WritePath::Stdin
    }
}

pub fn hex_encode(bytes: &[u8]) -> Secret {
    use std::fmt::Write;

    // One buffer, at the full width: a `String` per byte is a fragment of the
    // Credential in freed heap several thousand times over.
    let mut out = Secret::with_room_for(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02X}");
    }
    out
}

pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    // Reserved at full width for `hex_encode`'s reason: collecting an
    // `Option<Vec<_>>` starts at capacity zero, so every doubling frees a
    // fragment of the decoded Credential — and this is the macOS read path.
    let mut out = Vec::with_capacity(text.len() / 2);
    for at in (0..text.len()).step_by(2) {
        out.push(u8::from_str_radix(&text[at..at + 2], 16).ok()?);
    }
    Some(out)
}

/// `security -w` prints hex for data that is not printable, so a reply that is
/// entirely hex digits is decoded before it is believed. Exactly one newline
/// comes off, because exactly one is `security`'s and the rest are the
/// Credential's — one restored from a backup ends in a newline, and the `-X`
/// write stores it exactly.
pub fn decode_password_output(stdout: &str) -> String {
    let trimmed = stdout.strip_suffix('\n').unwrap_or(stdout);
    match hex_decode(trimmed) {
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            // The error owns the decoded bytes, so dropping it frees a copy of
            // the Credential — the one way out of here that does.
            Err(refused) => {
                let mut decoded = refused.into_bytes();
                decoded.zeroize();
                trimmed.to_string()
            }
        },
        None => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reservation may not come up short, because growing it abandons a
    /// prefix of the hex-encoded Credential in freed heap — and the largest
    /// Credentials are exactly the ones this line is measured for.
    #[test]
    fn the_reservation_covers_a_write_of_every_shape() {
        let secret = "x".repeat(STDIN_BUFFER_LIMIT);
        for (service, account) in [
            ("Claude Code-credentials", "someone"),
            // Nothing but characters quoting doubles, in both names.
            (r#"\"\"\"\""#, r#"\"\"\"\""#),
        ] {
            let line = add_command_line(service, account, &secret).expect("storable");
            assert!(
                line.len()
                    <= FIXED_WIDTH + service.len() * 2 + account.len() * 2 + secret.len() * 2,
                "{} written into the width reserved for it",
                line.len()
            );
        }
    }

    #[test]
    fn hex_round_trips() {
        let secret = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01"}}"#;
        let hex = hex_encode(secret.as_bytes());
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(hex_decode(&hex).unwrap(), secret.as_bytes());
    }

    #[test]
    fn only_the_newline_security_added_is_taken_off_a_reply() {
        let credential = "{\"claudeAiOauth\":{}}";
        assert_eq!(
            decode_password_output(&format!("{credential}\n")),
            credential
        );
        assert_eq!(
            decode_password_output(&format!("{credential}\n\n")),
            format!("{credential}\n"),
            "a Credential that ends in a newline still does when it is read back"
        );
        assert_eq!(
            decode_password_output(credential),
            credential,
            "and a reply with no newline at all is left alone"
        );
    }

    /// The same for the hex form, which is what `security -w` answers with when
    /// the stored bytes are not printable.
    #[test]
    fn a_hex_reply_carries_its_trailing_newline_through_the_decode() {
        let credential = "{\"a\":1}\n";
        let encoded = hex_encode(credential.as_bytes());
        assert_eq!(
            decode_password_output(&format!("{}\n", encoded.as_str())),
            credential
        );
    }

    /// The `-i` line for an ordinary write, which every test below is about.
    fn line(service: &str, account: &str, secret: &str) -> String {
        add_command_line(service, account, secret)
            .expect("ordinary names")
            .to_string()
    }

    #[test]
    fn a_short_credential_goes_through_stdin() {
        let written = line("Claude Code-credentials", "someone", "{\"a\":1}");
        assert_eq!(write_path_for(&written), WritePath::Stdin);
    }

    /// The account name is `$USER` verbatim, which is somebody else's to set.
    #[test]
    fn a_name_carrying_a_control_character_is_refused_rather_than_quoted() {
        let injected = "someone\ndelete-generic-password -s \"Claude Code-credentials\"";
        let refused = add_command_line("Claude Code-credentials", injected, "{}")
            .expect_err("that is two commands, not a name");
        assert!(refused.to_string().contains("account name"), "{refused}");

        add_command_line("svc\r", "someone", "{}").expect_err("a carriage return too");
        add_command_line("svc", "someone", "{}").expect("and an ordinary pair is fine");
    }

    #[test]
    fn a_credential_near_the_buffer_limit_falls_back_to_argv() {
        let big = "x".repeat(STDIN_BUFFER_LIMIT / 2);
        let written = line("Claude Code-credentials", "someone", &big);
        assert_eq!(write_path_for(&written), WritePath::Argv);
    }

    #[test]
    fn the_fallback_happens_with_headroom_rather_than_at_the_edge() {
        let just_inside = "y".repeat(STDIN_BUFFER_LIMIT - STDIN_SAFETY_MARGIN + 1);
        assert_eq!(write_path_for(&just_inside), WritePath::Argv);

        let comfortably_clear = "y".repeat(STDIN_BUFFER_LIMIT - STDIN_SAFETY_MARGIN - 1);
        assert_eq!(write_path_for(&comfortably_clear), WritePath::Stdin);
    }

    #[test]
    fn the_secret_never_appears_in_plain_text_on_the_command_line() {
        let written = line("svc", "acct", "sk-ant-oat01-secret");
        assert!(!written.contains("sk-ant-oat01-secret"));
    }

    #[test]
    fn exit_44_is_not_found_and_everything_else_is_unavailable() {
        let not_found = Execution {
            status: 44,
            stdout: String::new(),
            stderr: "The specified item could not be found".into(),
        };
        assert!(matches!(
            classify(&not_found, "svc", "acct"),
            KeychainError::NotFound { .. }
        ));

        let denied = Execution {
            status: 51,
            stdout: String::new(),
            stderr: "User interaction is not allowed".into(),
        };
        assert!(matches!(
            classify(&denied, "svc", "acct"),
            KeychainError::Unavailable { .. }
        ));
    }

    #[test]
    fn printable_output_is_returned_as_is() {
        assert_eq!(decode_password_output("{\"a\":1}\n"), "{\"a\":1}");
    }

    #[test]
    fn hex_output_is_decoded() {
        assert_eq!(decode_password_output("7B2261223A317D\n"), "{\"a\":1}");
    }

    /// Hex that decodes to bytes that are not text: the reply is handed back as
    /// it came, and the decoded copy is wiped rather than dropped, being the
    /// Credential in the one branch that holds it after the `String` refused it.
    #[test]
    fn hex_that_is_not_text_is_returned_as_it_came() {
        assert_eq!(decode_password_output("FFFE\n"), "FFFE");
    }
}
