//! The `/usr/bin/security` wrapper (ADR 0008).
//!
//! macOS anchors a keychain item's access control to the binary that created
//! it, so Perch drives the same binary Claude Code does rather than linking a
//! keychain crate. Four constraints come with that binary, and all four live
//! here and nowhere else:
//!
//! - writes hex-encode with `-X` and go in through `-i` so a Credential does
//!   not reach `argv`, where any process can read it off the process table;
//! - the `-i` stdin buffer is 4096 bytes and overflow truncates mid-argument,
//!   silently corrupting the item, so writes near the limit fall back to argv
//!   — the one exception to the line above, and one the user is told about as
//!   it happens rather than left to discover: a Credential that reaches `argv`
//!   is readable by anything running as them for as long as `security` does;
//! - exit code 44 means "not found"; every other non-zero exit means locked,
//!   denied, or unavailable, and is reported differently;
//! - `-w` returns hex for non-printable data, so this is safe for the ASCII
//!   JSON Claude Code stores and nothing else.
//!
//! What is here is the protocol and none of the spawning: building a command
//! line, deciding which way it must travel, and reading an exit status. The
//! process itself is started in [`crate::host::real`], where every other
//! process Perch starts is.

use crate::host::{Execution, double_quoted};
use zeroize::Zeroizing;

/// The `security` binary. Never a build of Perch, never a crate — see ADR 0008.
pub const SECURITY_BIN: &str = "/usr/bin/security";

/// `security` exit code meaning "the item is not in the keychain".
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
///
/// `-U` updates an existing item rather than failing; `-X` takes the secret as
/// hex so no byte of it is ever quoted, escaped, or logged.
///
/// Refuses a service or account carrying a control character. `security -i` is
/// line-oriented — one sub-command per line — so a newline in either of them
/// does not need escaping, it needs rejecting: it ends this command and starts
/// another, and `-i` reports a failed sub-command on stderr while still exiting
/// 0, so an injected `delete-generic-password` that *works* is silent. The
/// account name is `$USER` verbatim, which is somebody else's to set.
pub fn add_command_line(
    service: &str,
    account: &str,
    secret: &str,
) -> Result<Zeroizing<String>, KeychainError> {
    inert("the keychain service name", service)?;
    inert("the keychain account name", account)?;
    // `Zeroizing`, because this line holds the Credential — hex-encoded, which
    // is an encoding and not a protection. The read path goes to the trouble of
    // wiping `security`'s stdout and `StoredCredential` is `Zeroizing`
    // throughout, so the invariant is explicit everywhere except the write that
    // puts a Credential *there*, which is the one direction that had none.
    Ok(Zeroizing::new(format!(
        "add-generic-password -U -s {} -a {} -X {}\n",
        double_quoted(service),
        double_quoted(account),
        hex_encode(secret.as_bytes()).as_str(),
    )))
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
/// ADR 0008 says writes *near* the limit fall back, not writes past it: the
/// failure is silent corruption, and the exact byte at which `security` starts
/// truncating is an observation about one build of it, not a promise.
pub fn write_path_for(command_line: &str) -> WritePath {
    if command_line.len() >= STDIN_BUFFER_LIMIT - STDIN_SAFETY_MARGIN {
        WritePath::Argv
    } else {
        WritePath::Stdin
    }
}

pub fn hex_encode(bytes: &[u8]) -> Zeroizing<String> {
    use std::fmt::Write;

    // Written into the buffer rather than formatted into a `String` per byte,
    // which is a transient allocation for every one of the several thousand a
    // Credential runs to — and every one of those would have been a fragment of
    // the Credential in freed heap.
    //
    // The buffer is reserved at the full width for the same reason, so it never
    // grows: a `Vec` that reallocates copies and frees the old block untouched,
    // which is what `export::Wiping` exists to avoid one module over.
    let mut out = Zeroizing::new(String::with_capacity(bytes.len() * 2));
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
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// `security -w` prints hex when the stored data is not printable, so a reply
/// that is entirely hex digits is decoded before it is believed.
///
/// Exactly one newline is taken off, because exactly one is `security`'s: the
/// rest are the Credential's. `trim_end_matches` took them all, and a Credential
/// ending in a newline is ordinary — `~/.claude/.credentials.json` restored from
/// a backup or touched by an editor has one. The write is `-X`, so the bytes
/// stored are exact; only the read was lossy, which made `write_and_read_back`
/// compare a Credential against itself minus a byte and conclude the keychain
/// could not be written to. It then deleted the item it had just written and
/// fell back to the plaintext store, saying so in a note that named the wrong
/// cause — and every later write repeated it, so on macOS that Account's
/// Credential stayed in a file on disk for good.
pub fn decode_password_output(stdout: &str) -> String {
    let trimmed = stdout.strip_suffix('\n').unwrap_or(stdout);
    match hex_decode(trimmed) {
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => trimmed.to_string(),
        },
        None => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let secret = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01"}}"#;
        let hex = hex_encode(secret.as_bytes());
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(hex_decode(&hex).unwrap(), secret.as_bytes());
    }

    /// One newline is `security`'s and the rest are the Credential's. A
    /// Credential ending in one is ordinary — a `.credentials.json` restored
    /// from a backup or touched by an editor has one — and taking it off made
    /// `write_and_read_back` compare a Credential against itself minus a byte,
    /// conclude the keychain could not be written to, delete the item it had
    /// just written, and fall back to storing it in plaintext for good.
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

    /// `security -i` reads one sub-command per line, so a newline in a value is
    /// not something to escape — it is the end of this command and the start of
    /// another. The account name is `$USER` verbatim, which is somebody else's
    /// to set, and `-i` reports a failed sub-command on stderr while still
    /// exiting 0: an injected `delete-generic-password` that worked would be
    /// silent.
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
}
