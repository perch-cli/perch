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
//! Deliberately narrow: it finds the object value of one top-level key. It is
//! not a JSON library, and nothing here decides which key matters — that is
//! [`crate::probe`]'s business, and this module never names one.

/// The object value of a top-level `key`, exactly as it is written.
pub fn object_at<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    let (start, end) = span_of(contents, key)?;
    Some(&contents[start..end])
}

/// The same document with the object value of `key` replaced by `block`, and
/// every byte outside that value identical.
///
/// The block is written at the indentation of the key that introduces it,
/// whatever indentation it arrived with, so a block copied between two files
/// does not step further right each time.
pub fn replace_object_at(contents: &str, key: &str, block: &str) -> Option<String> {
    let (start, end) = span_of(contents, key)?;
    let indented = indent_to_match(block, indentation_of_the_line(contents, start));
    Some(format!(
        "{}{indented}{}",
        &contents[..start],
        &contents[end..]
    ))
}

/// Where the object value of a top-level key starts and ends.
fn span_of(contents: &str, key: &str) -> Option<(usize, usize)> {
    let bytes = contents.as_bytes();
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
                    && &contents[at + 1..end - 1] == key
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

/// The span of the object value following a key, given where the key ended.
fn value_after_key(bytes: &[u8], key_end: usize) -> Option<(usize, usize)> {
    let mut at = skip_whitespace(bytes, key_end);
    if bytes.get(at) != Some(&b':') {
        return None;
    }
    at = skip_whitespace(bytes, at + 1);
    if bytes.get(at) != Some(&b'{') {
        return None;
    }

    let mut depth = 0usize;
    let start = at;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(bytes, at)?,
            b'{' => {
                depth += 1;
                at += 1;
            }
            b'}' => {
                depth -= 1;
                at += 1;
                if depth == 0 {
                    return Some((start, at));
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
fn indent_to_match(block: &str, indentation: usize) -> String {
    let own = block
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
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
                format!("{padding}{}", &line[own.min(line.len())..])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
            replace_object_at(DOCUMENT, "block", "{\n  \"name\": \"someone-else\"\n}").unwrap();

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
        let patched = replace_object_at(
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
        assert_eq!(replace_object_at(r#"{"a": 1}"#, "block", "{}"), None);
    }
}
