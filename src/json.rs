//! Replacing one key of a JSON document and leaving every other byte of it
//! alone — and writing a whole one into a buffer that is wiped ([`sealed`]).
//!
//! Perch patches files it does not own — `.claude.json` holds project history,
//! MCP configuration and settings beside the one block belonging to an Account
//! (ADR everything-but-the-account). Parsing such a file and writing it back
//! would reorder keys, reformat numbers and drop the shape its owner wrote, all
//! invisibly, so the value is found as a span of text and spliced.
//!
//! Narrow, and not a JSON library: one top-level key, and deeper is this twice.

use crate::secret::Secret;
use zeroize::Zeroize;

/// `document` as JSON text, in a buffer wiped on drop and reserved at the width
/// the finished text takes. `to_string` starts at zero and doubles, so every
/// growth abandons a prefix of what it wrote un-wiped — and each document that
/// comes through here holds the only copy there is of a refresh token.
pub fn sealed(document: &serde_json::Value) -> Secret {
    let mut width = Width(0);
    // A failure here costs the reserve and nothing else, so it is not a reason
    // to refuse: the pass below writes the same bytes either way.
    let _ = serde_json::to_writer(&mut width, document);

    let mut bytes = Vec::with_capacity(width.0);
    if serde_json::to_writer(&mut bytes, document).is_err() {
        // Unreachable — a `Value` and a `Vec<u8>` have nothing between them
        // that can fail — and truncated bytes would be worse than a lost wipe.
        bytes.zeroize();
        return Secret::copied(&document.to_string());
    }
    // Borrowed over what `to_writer` wrote, and `into_owned` of a borrowed one
    // reserves exactly: one copy, no growth.
    let text = Secret::taken_over(String::from_utf8_lossy(&bytes).into_owned());
    bytes.zeroize();
    text
}

/// Counts the bytes a serialization writes without keeping one of them.
struct Width(usize);

impl std::io::Write for Width {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The value of a top-level `key`, exactly as it is written, whatever kind of
/// value it is.
pub fn value_at<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    let (start, end) = span_of(contents, key)?;
    Some(&contents[start..end])
}

/// The same, where only an object will do.
///
/// A key that holds something else is no answer rather than the wrong one: the
/// callers that ask this way are about a block of fields, and a string where
/// one belongs is a file that has stopped being the shape they believe in.
pub fn object_at<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    value_at(contents, key).filter(|value| value.starts_with('{'))
}

/// The same document with `key` holding `value` — replacing what it held, or
/// writing it as the document's first member where it held nothing. Every byte
/// outside that value is identical, and the value is written at the indentation
/// of the key that introduces it, so a block copied between two files does not
/// step further right each time.
pub fn set_value_at(contents: &str, key: &str, value: &str) -> Option<Secret> {
    let Some((span, indented)) = replacement(contents, key, value) else {
        return insert(contents, key, value);
    };
    Some(splice(contents, span, &indented))
}

/// The same, and nothing at all where the document already holds what the splice
/// would write. The splice copies the whole document and `.claude.json` grows
/// with the person's history, so the caller that may have nothing to do asks
/// this one — both of whose `None`s say the same thing to it: keep what you have.
pub fn changed_value_at(contents: &str, key: &str, value: &str) -> Option<Secret> {
    let Some((span, indented)) = replacement(contents, key, value) else {
        return insert(contents, key, value);
    };
    (contents[span.0..span.1] != *indented.as_str()).then(|| splice(contents, span, &indented))
}

/// The document with one span of it replaced, in a buffer wiped on drop.
fn splice(contents: &str, span: (usize, usize), indented: &Secret) -> Secret {
    spliced(&[&contents[..span.0], indented.as_str(), &contents[span.1..]])
}

/// The span a splice of `key` replaces and the text it writes there, or nothing
/// where the document holds no such key. Split from the join so the comparison
/// above can be made against a value rather than against a whole document.
fn replacement(contents: &str, key: &str, value: &str) -> Option<((usize, usize), Secret)> {
    let (start, end) = span_of(contents, key)?;
    Some((
        (start, end),
        indent_to_match(value, indentation_of_the_line(contents, start)),
    ))
}

/// The pieces of a document, joined in a buffer wiped on drop and reserved at
/// the width the whole comes to.
///
/// `format!` would grow into it and free every prefix it outgrew, and what comes
/// through here is a `.claude.json` — which carries an MCP server's `env` block.
fn spliced(pieces: &[&str]) -> Secret {
    let mut written = Secret::with_room_for(pieces.iter().map(|piece| piece.len()).sum());
    for piece in pieces {
        written.push_str(piece);
    }
    written
}

/// Writes a key the document does not have yet, as the first member of it.
///
/// First rather than last because it is the one position needing no commas
/// moved: the member the file already opens with keeps its comma, and everything
/// after it is untouched. A document that is not an object has nowhere to write.
fn insert(contents: &str, key: &str, value: &str) -> Option<Secret> {
    let bytes = contents.as_bytes();
    let open = skip_whitespace(bytes, 0);
    if bytes.get(open) != Some(&b'{') {
        return None;
    }
    let end = value_end(bytes, open)?;
    let inside = &contents[open + 1..end - 1];

    let column = member_indentation(contents, open, inside);
    let padding = " ".repeat(column);
    let indented = indent_to_match(value, column);
    let member = spliced(&["\n", &padding, &quoted(key), ": ", &indented]);

    // An object holding nothing has no first member to keep, and needs its
    // closing brace put back on a line of its own.
    if inside.trim().is_empty() {
        let brace = " ".repeat(indentation_of_the_line(contents, open));
        return Some(spliced(&[
            &contents[..open],
            "{",
            &member,
            "\n",
            &brace,
            "}",
            &contents[end..],
        ]));
    }
    Some(spliced(&[
        &contents[..open],
        "{",
        &member,
        ",",
        &contents[open + 1..],
    ]))
}

/// The column an object's members are written at: whatever the first one uses,
/// and two past the brace where there is no member to copy.
fn member_indentation(contents: &str, open: usize, inside: &str) -> usize {
    inside
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .unwrap_or_else(|| indentation_of_the_line(contents, open) + 2)
}

/// A key as it is written in a document, quotes, escapes and all.
///
/// Compared and written in this form rather than decoded, because a key here is
/// routinely a path: `projects` is keyed by directory, and a Windows one is a
/// string of backslashes that only means what it says once escaped.
fn quoted(key: &str) -> String {
    serde_json::to_string(key).expect("a string serializes")
}

/// Where the value of a top-level key starts and ends.
fn span_of(contents: &str, key: &str) -> Option<(usize, usize)> {
    let bytes = contents.as_bytes();
    let sought = quoted(key);
    let mut at = 0;
    let mut depth = 0usize;

    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                let end = string_end(bytes, at)?;
                // Only the top-level key. The same name nested inside something
                // else — a project's own entry, say — is a different thing that
                // happens to be spelled the same.
                if depth == 1
                    && contents[at..end] == sought
                    && let Some(value) = value_after_key(bytes, end)
                {
                    return Some(value);
                }
                at = end;
            }
            b'{' | b'[' => {
                depth += 1;
                at += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                at += 1;
            }
            _ => at += 1,
        }
    }
    None
}

/// The span of the value following a key, given where the key ended. Nothing
/// but a `:` may follow a key, so anything else is a string that was a value
/// rather than a name, and no span at all.
fn value_after_key(bytes: &[u8], key_end: usize) -> Option<(usize, usize)> {
    let at = skip_whitespace(bytes, key_end);
    if bytes.get(at) != Some(&b':') {
        return None;
    }
    let start = skip_whitespace(bytes, at + 1);
    Some((start, value_end(bytes, start)?))
}

/// One past the last byte of the value starting at `start`, whichever of the
/// six kinds of JSON value it is.
fn value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'"' => string_end(bytes, start),
        b'{' => balanced_end(bytes, start, b'{', b'}'),
        b'[' => balanced_end(bytes, start, b'[', b']'),
        // A number, `true`, `false` or `null`: whatever runs up to the comma or
        // the brace that ends it.
        _ => {
            let mut at = start;
            while at < bytes.len() && !matches!(bytes[at], b',' | b'}' | b']') {
                at += 1;
            }
            Some(at).filter(|end| *end > start).map(|end| {
                // Trailing whitespace belongs to the document rather than to
                // the value, so replacing the value leaves the line as it was.
                let mut end = end;
                while end > start && bytes[end - 1].is_ascii_whitespace() {
                    end -= 1;
                }
                end
            })
        }
    }
}

/// One past the brace or bracket that closes the one at `start`, counting only
/// its own kind and skipping strings — which is exact for a well-formed
/// document, where every other pair inside it is balanced too.
fn balanced_end(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut at = start;
    let mut depth = 0usize;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(bytes, at)?,
            byte if byte == open => {
                depth += 1;
                at += 1;
            }
            byte if byte == close => {
                // Saturating, as its sibling in `span_of` is. Only the two
                // callers above reach this and both enter on `open`, so a close
                // first is unreachable — and a `-=` is how it stops being so.
                depth = depth.saturating_sub(1);
                at += 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => at += 1,
        }
    }
    None
}

/// Where a JSON string ends, one past its closing quote, honoring escapes.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return Some(at + 1),
            _ => at += 1,
        }
    }
    None
}

fn skip_whitespace(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// The indentation of the line a position falls on. A block sits at the
/// indentation of the key that introduces it, not at the column its own first
/// brace happens to land on.
fn indentation_of_the_line(contents: &str, at: usize) -> usize {
    let line_start = match contents[..at].rfind('\n') {
        Some(newline) => newline + 1,
        None => 0,
    };
    contents[line_start..at]
        .chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

/// Where a line's own indentation ends, in bytes, given its width in characters
/// — or the line's end, for one narrower than the block's narrowest.
fn own_ends_at(line: &str, width: usize) -> usize {
    line.char_indices()
        .nth(width)
        .map_or(line.len(), |(offset, _)| offset)
}

/// Rewrites a block at a given indentation, whatever it was written at before.
///
/// Counted in characters, as [`indentation_of_the_line`] counts what it is being
/// matched to: the block is read verbatim out of a file Perch does not own, so a
/// width measured in bytes would slice a character in half and panic.
fn indent_to_match(block: &str, indentation: usize) -> Secret {
    let width_of = |line: &str| line.chars().take_while(|c| c.is_whitespace()).count();
    let own = block
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(width_of)
        .min()
        .unwrap_or(0);

    let padding = " ".repeat(indentation);
    // Into one buffer for [`spliced`]'s reason, and by hand rather than through
    // `join`, which builds every line as a `String` of its own first.
    let mut written = Secret::with_room_for(block.len() + block.lines().count() * indentation);
    for (index, line) in block.lines().enumerate() {
        if index > 0 {
            written.push('\n');
            written.push_str(&padding);
        }
        match index {
            0 => written.push_str(line),
            // Resolved to a byte offset once and copied in one go. A `projects`
            // block runs to megabytes, and a byte at a time is the difference
            // between a `memcpy` and a bounds check per character.
            _ => written.push_str(&line[own_ends_at(line, own)..]),
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two splices as text a case can compare. Both answer a `Secret`, which
    /// neither prints nor compares — deliberately, since one holds a document
    /// this module exists to keep out of freed heap.
    fn text_at(contents: &str, key: &str, value: &str) -> Option<String> {
        set_value_at(contents, key, value).map(|written| written.as_str().to_string())
    }

    fn indented_to(block: &str, indentation: usize) -> String {
        indent_to_match(block, indentation).as_str().to_string()
    }

    /// The whole of what [`spliced`] is for, said as capacity because that is
    /// the property: a buffer that never grew handed no prefix of what it holds
    /// back to the allocator. `carry` chains eleven of these over one file, and
    /// `mcpServers` — the key holding an API key — is one of the eleven.
    #[test]
    fn a_patched_document_is_written_into_a_buffer_that_never_grew() {
        let carrying = r#"{
  "numStartups": 4,
  "projects": {
    "/Users/someone/work": {
      "mcpServers": {
        "billing": { "env": { "API_KEY": "sk-live-a-third-party-secret" } }
      }
    }
  }
}"#;

        for (key, value) in [
            ("hasCompletedOnboarding", "true"),
            ("numStartups", "9"),
            ("projects", r#"{"/elsewhere": {}}"#),
        ] {
            let written = set_value_at(carrying, key, value).expect("it is an object");
            assert_eq!(
                written.capacity(),
                written.as_str().len(),
                "`{key}` was written into a buffer that grew"
            );
        }
    }

    /// The steady state of a Run: every key that crosses is already in the
    /// Profile, and `.claude.json` is the largest file Perch touches.
    #[test]
    fn a_key_already_holding_what_would_be_written_is_no_change_and_no_copy() {
        assert!(changed_value_at(DOCUMENT, "numStartups", "41").is_none());
        assert!(
            changed_value_at(
                DOCUMENT,
                "block",
                "{\n    \"name\": \"someone\",\n    \"role\": \"admin\"\n  }"
            )
            .is_none()
        );
        assert_eq!(
            changed_value_at(DOCUMENT, "numStartups", "42")
                .map(|written| written.as_str().to_string()),
            text_at(DOCUMENT, "numStartups", "42"),
            "and a value that differs is spliced exactly as it always was"
        );
    }

    /// A block written at one indentation and read into a document expecting
    /// another is a change, however equal the two texts look.
    #[test]
    fn a_value_differing_only_in_indentation_is_still_a_change() {
        let outdented = "{\n\"name\": \"someone\",\n\"role\": \"admin\"\n}";

        assert!(changed_value_at(DOCUMENT, "block", outdented).is_some());
    }

    /// This runs after the incoming Credential is already live, where a panic
    /// replaces the recovery instructions the user actually needs.
    #[test]
    fn a_block_indented_with_multi_byte_whitespace_is_re_indented_rather_than_split() {
        // U+00A0 NO-BREAK SPACE: two bytes, one character, and whitespace as
        // far as `trim_start` is concerned.
        let block = "{\n\u{a0}\u{a0}\"a\": 1,\n\u{a0}\u{a0}\"b\": 2\n\u{a0}\u{a0}}";

        assert_eq!(
            indented_to(block, 2),
            "{\n  \"a\": 1,\n  \"b\": 2\n  }",
            "two characters of indentation are two characters, not four bytes"
        );
    }

    /// A blank line is left out of the narrowest-indentation reckoning, so it
    /// is the one line the cut can be asked for past its own end.
    #[test]
    fn a_line_narrower_than_the_block_it_sits_in_is_cut_at_its_end() {
        // The blank line is one character wide where the block's own
        // indentation is two, and is left out of the reckoning that says so.
        let block = "{\n    \"a\": 1,\n \n    \"b\": 2\n  }";

        assert_eq!(indented_to(block, 0), "{\n  \"a\": 1,\n\n  \"b\": 2\n}");
    }

    /// The case that used to panic outright: the narrowest line's indentation,
    /// measured in bytes, falls inside a character of a wider one.
    #[test]
    fn a_block_whose_lines_use_different_multi_byte_whitespace_does_not_panic() {
        // Two of U+00A0 is four bytes; two of U+2028 is six, and byte four is
        // the middle of the second one.
        let block = "{\n\u{a0}\u{a0}\"a\": 1,\n\u{2028}\u{2028}\"b\": 2\n\u{a0}\u{a0}}";

        let indented = indented_to(block, 0);

        assert_eq!(indented, "{\n\"a\": 1,\n\"b\": 2\n}");
    }

    const DOCUMENT: &str = r#"{
  "numStartups": 41,
  "block": {
    "name": "someone",
    "role": "admin"
  },
  "projects": {
    "/Users/someone/work": {
      "allowedTools": ["Bash(git status)"],
      "block": "not the one"
    }
  }
}"#;

    #[test]
    fn everything_outside_the_value_survives_byte_for_byte() {
        let patched = text_at(DOCUMENT, "block", "{\n  \"name\": \"someone-else\"\n}").unwrap();

        assert!(patched.contains(r#""name": "someone-else""#));
        assert!(!patched.contains(r#""name": "someone""#));
        assert!(patched.contains(r#"  "numStartups": 41,"#));
        assert!(patched.contains(r#"      "allowedTools": ["Bash(git status)"],"#));
        assert!(
            patched.contains(r#"      "block": "not the one""#),
            "a nested key spelled the same is a different key: {patched}"
        );
        serde_json::from_str::<serde_json::Value>(&patched).expect("still JSON");
    }

    #[test]
    fn a_block_is_written_at_the_indentation_of_the_key_that_introduces_it() {
        // What taking a block out of another file hands over: indented already,
        // for a document that is not this one.
        let patched = text_at(
            DOCUMENT,
            "block",
            "{\n        \"name\": \"someone-else\"\n      }",
        )
        .unwrap();

        assert!(
            patched.contains("  \"block\": {\n    \"name\": \"someone-else\"\n  },"),
            "a block copied between documents does not step further right each \
             time: {patched}"
        );
    }

    #[test]
    fn the_value_is_read_back_exactly_as_it_is_written() {
        let block = object_at(DOCUMENT, "block").expect("there is one");
        assert!(block.starts_with('{') && block.ends_with('}'));
        assert!(block.contains(r#""role": "admin""#));
        assert!(!block.contains("not the one"));
    }

    #[test]
    fn a_key_that_is_not_there_or_holds_something_else_is_no_span_at_all() {
        assert_eq!(object_at(r#"{"projects": {}}"#, "block"), None);
        assert_eq!(object_at(r#"{"block": "a string"}"#, "block"), None);
        assert_eq!(object_at("not json at all", "block"), None);
    }

    /// `.claude.json` keeps the person's state in values of every kind — a flag
    /// for onboarding, a version string, an object of tips — so a reader that
    /// only knew objects could carry a third of what belongs to the person.
    #[test]
    fn a_value_of_any_kind_is_read_exactly_as_it_is_written() {
        const DOCUMENT: &str = r#"{
  "hasCompletedOnboarding": true,
  "lastOnboardingVersion": "2.1.221",
  "tipsHistory": {
    "ide-hotkey": 5
  },
  "seenNotifications": ["one", "two"],
  "numStartups": 41,
  "nothing": null
}"#;

        assert_eq!(value_at(DOCUMENT, "hasCompletedOnboarding"), Some("true"));
        assert_eq!(
            value_at(DOCUMENT, "lastOnboardingVersion"),
            Some(r#""2.1.221""#)
        );
        assert_eq!(
            value_at(DOCUMENT, "tipsHistory"),
            Some("{\n    \"ide-hotkey\": 5\n  }")
        );
        assert_eq!(
            value_at(DOCUMENT, "seenNotifications"),
            Some(r#"["one", "two"]"#)
        );
        assert_eq!(value_at(DOCUMENT, "numStartups"), Some("41"));
        assert_eq!(value_at(DOCUMENT, "nothing"), Some("null"));
    }

    /// The last member of an object closes with a brace rather than a comma,
    /// and a span that swallowed it would take the document's own punctuation
    /// with the value.
    #[test]
    fn the_last_value_in_an_object_ends_where_the_object_does() {
        assert_eq!(value_at("{\n  \"seen\": true\n}", "seen"), Some("true"));
        assert_eq!(value_at(r#"{"seen":true}"#, "seen"), Some("true"));
    }

    /// A Profile that has never been run holds an identity file and nothing
    /// else, so every key that crosses to it is one it does not have yet.
    #[test]
    fn a_key_the_document_does_not_have_is_written_as_its_first_member() {
        let fresh =
            "{\n  \"oauthAccount\": {\n    \"emailAddress\": \"someone@example.com\"\n  }\n}";

        let written = text_at(fresh, "hasCompletedOnboarding", "true").unwrap();

        assert_eq!(
            written,
            "{\n  \"hasCompletedOnboarding\": true,\n  \"oauthAccount\": {\n    \
             \"emailAddress\": \"someone@example.com\"\n  }\n}"
        );
        serde_json::from_str::<serde_json::Value>(&written).expect("still JSON");
    }

    /// The `projects` key of a Profile nothing has run in yet: an object with
    /// no members, whose closing brace has to end up on a line of its own.
    #[test]
    fn a_key_written_into_an_empty_object_keeps_the_document_valid() {
        let written = text_at("{}", "/Users/someone/work", "{\n  \"allowedTools\": []\n}").unwrap();

        assert_eq!(
            written,
            "{\n  \"/Users/someone/work\": {\n    \"allowedTools\": []\n  }\n}"
        );
        serde_json::from_str::<serde_json::Value>(&written).expect("still JSON");
    }

    /// A document written on one line, which is what a hand edit or anything
    /// that minified it leaves. The member it displaces shares a line with it,
    /// and that is the contract rather than a shortcoming: reflowing the tail
    /// would mean rewriting bytes outside the value.
    #[test]
    fn a_key_written_into_a_document_on_one_line_leaves_the_rest_of_it_alone() {
        let written = text_at(r#"{"numStartups": 4}"#, "hasSeenTasksHint", "true").unwrap();

        assert_eq!(
            written,
            "{\n  \"hasSeenTasksHint\": true,\"numStartups\": 4}"
        );
        let back: serde_json::Value = serde_json::from_str(&written).expect("still JSON");
        assert_eq!(back["hasSeenTasksHint"], true);
        assert_eq!(back["numStartups"], 4, "and nothing else moved");
    }

    /// One entry of `projects`, read out of one document and written into
    /// another: the same pair of calls twice, and the whole of how a nested key
    /// is reached.
    #[test]
    fn one_entry_of_a_nested_object_crosses_without_the_rest_of_it() {
        const SOURCE: &str = r#"{
  "projects": {
    "/Users/someone/work": {
      "allowedTools": ["Bash(git status)"]
    },
    "/Users/someone/elsewhere": {
      "allowedTools": ["Bash(rm -rf /)"]
    }
  }
}"#;
        let destination = "{\n  \"projects\": {}\n}";

        let source_projects = value_at(SOURCE, "projects").unwrap();
        let entry = value_at(source_projects, "/Users/someone/work").unwrap();
        let projects = text_at(
            value_at(destination, "projects").unwrap(),
            "/Users/someone/work",
            entry,
        )
        .unwrap();
        let written = text_at(destination, "projects", &projects).unwrap();

        assert!(written.contains(r#""Bash(git status)""#), "{written}");
        assert!(
            !written.contains("elsewhere"),
            "the directory the Run is not about stays behind: {written}"
        );
        serde_json::from_str::<serde_json::Value>(&written).expect("still JSON");
    }

    /// `projects` is keyed by directory, and on Windows a directory is a string
    /// of backslashes. A key matched or written without its escapes would find
    /// nothing on that platform, and would write a document nothing could read.
    #[test]
    fn a_key_that_needs_escaping_is_read_and_written_as_it_is_spelled_in_the_file() {
        const WINDOWS: &str = r#"{
  "projects": {
    "C:\\Users\\someone\\work": {
      "allowedTools": ["Bash(git status)"]
    }
  }
}"#;
        let here = r"C:\Users\someone\work";

        let projects = value_at(WINDOWS, "projects").expect("there is one");
        let entry = value_at(projects, here).expect("the directory is in it");
        assert!(entry.contains("Bash(git status)"));

        let written = text_at("{}", here, entry).expect("written into an empty object");
        assert!(
            written.contains(r#""C:\\Users\\someone\\work""#),
            "{written}"
        );
        let read_back: serde_json::Value =
            serde_json::from_str(&written).expect("what was written is JSON");
        assert!(!read_back[here]["allowedTools"].is_null(), "{read_back}");
    }

    /// Something that is not an object has no member to write, and a file that
    /// has stopped being a JSON object is not one to guess at.
    #[test]
    fn there_is_nowhere_to_write_a_key_in_a_document_that_is_not_an_object() {
        assert_eq!(text_at("[1, 2]", "seen", "true"), None);
        assert_eq!(text_at("not json at all", "seen", "true"), None);
        assert_eq!(text_at("", "seen", "true"), None);
    }

    /// The whole of what `sealed` is for: the buffer never grows, so no prefix
    /// of the token it holds is handed back to the allocator un-wiped. Said as
    /// capacity, because that is the property — the text being right is what
    /// every other caller of `serde_json` already gets.
    #[test]
    fn a_sealed_document_is_written_into_a_buffer_that_never_grew() {
        let document = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": "sk-ant-ort01-a-refresh-token-of-a-realistic-length",
            "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        });

        let written = sealed(&document);

        assert_eq!(
            written.as_str(),
            serde_json::to_string(&document).expect("a Value serializes"),
            "the same bytes `to_string` would have written"
        );
        assert_eq!(
            written.capacity(),
            written.len(),
            "and in exactly the room they take: anything larger is a reserve \
             that was guessed, anything smaller is impossible"
        );
    }
}
