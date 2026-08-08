//! Replacing one key of a JSON document and leaving every other byte of it
//! alone.
//!
//! Perch patches files it does not own — `.claude.json` holds project history,
//! MCP configuration and settings beside the one block that belongs to an
//! Account (ADR 0001). Parsing such a file and writing it back would reorder
//! keys, reformat numbers and drop the shape its owner wrote, all invisibly. So
//! the value is found as a span of text and spliced, and the rest of the file
//! is never touched.
//!
//! Deliberately narrow: it finds the value of one top-level key. Anything
//! deeper is this applied twice — the value of `projects` is itself a document
//! with keys of its own, so reading one entry out of it and writing one entry
//! into it is the same pair of calls a second time. It is not a JSON library,
//! and nothing here decides which key matters: that is [`crate::probe`]'s
//! business and [`crate::carry`]'s, and this module never names one.

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
/// outside that value is identical.
///
/// The value is written at the indentation of the key that introduces it,
/// whatever indentation it arrived with, so a block copied between two files
/// does not step further right each time.
///
/// Written first rather than last because that is the one position needing no
/// commas moved: the member the file already opens with keeps its comma, and
/// everything after it is untouched. Where the document is not an object at
/// all there is nowhere to write, and nothing is.
pub fn set_value_at(contents: &str, key: &str, value: &str) -> Option<String> {
    let Some((start, end)) = span_of(contents, key) else {
        return insert(contents, key, value);
    };
    let indented = indent_to_match(value, indentation_of_the_line(contents, start));
    Some(format!(
        "{}{indented}{}",
        &contents[..start],
        &contents[end..]
    ))
}

/// Writes a key the document does not have yet, as the first member of it.
fn insert(contents: &str, key: &str, value: &str) -> Option<String> {
    let bytes = contents.as_bytes();
    let open = skip_whitespace(bytes, 0);
    if bytes.get(open) != Some(&b'{') {
        return None;
    }
    let end = value_end(bytes, open)?;
    let inside = &contents[open + 1..end - 1];

    let column = member_indentation(contents, open, inside);
    let member = format!(
        "\n{}{}: {}",
        " ".repeat(column),
        quoted(key),
        indent_to_match(value, column)
    );

    // An object holding nothing has no first member to keep, and needs its
    // closing brace put back on a line of its own.
    if inside.trim().is_empty() {
        let brace = " ".repeat(indentation_of_the_line(contents, open));
        return Some(format!(
            "{}{{{member}\n{brace}}}{}",
            &contents[..open],
            &contents[end..]
        ));
    }
    Some(format!(
        "{}{{{member},{}",
        &contents[..open],
        &contents[open + 1..]
    ))
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
/// routinely a path — `projects` is keyed by directory, and a Windows one is a
/// string of backslashes that only means what it says once escaped. Matching
/// the text is exact for a document written by `JSON.stringify`, which is what
/// Claude Code writes.
fn quoted(key: &str) -> String {
    serde_json::to_string(key).expect("a string serialises")
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
                depth -= 1;
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

/// Where a JSON string ends, one past its closing quote, honouring escapes.
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

/// Rewrites a block at a given indentation, whatever it was written at before.
///
/// Counted in characters, as [`indentation_of_the_line`] counts the
/// indentation it is being matched to. The block is read verbatim out of a
/// `.claude.json` Perch does not own, so its whitespace can be anything Unicode
/// calls whitespace — and a width measured in bytes and then used to cut a
/// string would slice one of those characters in half and panic. Not somewhere
/// a panic can be afforded: this runs inside `patch_identity`, after the
/// incoming Credential is already live, where what the user needs is the
/// recovery instructions rather than a backtrace.
fn indent_to_match(block: &str, indentation: usize) -> String {
    let width_of = |line: &str| line.chars().take_while(|c| c.is_whitespace()).count();
    let own = block
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(width_of)
        .min()
        .unwrap_or(0);

    let padding = " ".repeat(indentation);
    block
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line.to_string()
            } else {
                let undented: String = line.chars().skip(own).collect();
                format!("{padding}{undented}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block is read verbatim out of a `.claude.json` Perch does not own,
    /// so its whitespace is whatever is in that file. A width measured in bytes
    /// and then used to cut a string slices a multi-byte character in half —
    /// and this runs after the incoming Credential is already live, where a
    /// panic replaces the recovery instructions the user actually needs.
    #[test]
    fn a_block_indented_with_multi_byte_whitespace_is_re_indented_rather_than_split() {
        // U+00A0 NO-BREAK SPACE: two bytes, one character, and whitespace as
        // far as `trim_start` is concerned.
        let block = "{\n\u{a0}\u{a0}\"a\": 1,\n\u{a0}\u{a0}\"b\": 2\n\u{a0}\u{a0}}";

        assert_eq!(
            indent_to_match(block, 2),
            "{\n  \"a\": 1,\n  \"b\": 2\n  }",
            "two characters of indentation are two characters, not four bytes"
        );
    }

    /// The case that used to panic outright: the narrowest line's indentation,
    /// measured in bytes, falls inside a character of a wider one.
    #[test]
    fn a_block_whose_lines_use_different_multi_byte_whitespace_does_not_panic() {
        // Two of U+00A0 is four bytes; two of U+2028 is six, and byte four is
        // the middle of the second one.
        let block = "{\n\u{a0}\u{a0}\"a\": 1,\n\u{2028}\u{2028}\"b\": 2\n\u{a0}\u{a0}}";

        let indented = indent_to_match(block, 0);

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
        let patched =
            set_value_at(DOCUMENT, "block", "{\n  \"name\": \"someone-else\"\n}").unwrap();

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
        let patched = set_value_at(
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

        let written = set_value_at(fresh, "hasCompletedOnboarding", "true").unwrap();

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
        let written =
            set_value_at("{}", "/Users/someone/work", "{\n  \"allowedTools\": []\n}").unwrap();

        assert_eq!(
            written,
            "{\n  \"/Users/someone/work\": {\n    \"allowedTools\": []\n  }\n}"
        );
        serde_json::from_str::<serde_json::Value>(&written).expect("still JSON");
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
        let projects = set_value_at(
            value_at(destination, "projects").unwrap(),
            "/Users/someone/work",
            entry,
        )
        .unwrap();
        let written = set_value_at(destination, "projects", &projects).unwrap();

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

        let written = set_value_at("{}", here, entry).expect("written into an empty object");
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
        assert_eq!(set_value_at("[1, 2]", "seen", "true"), None);
        assert_eq!(set_value_at("not json at all", "seen", "true"), None);
        assert_eq!(set_value_at("", "seen", "true"), None);
    }
}
