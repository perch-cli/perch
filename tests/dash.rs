//! The one clause of the register a script can hold: no em dash in what Perch
//! says.
//!
//! Under ADR perch-says-what-it-did the dash is what a sentence reaches for
//! when it has a verdict and wants to append its reasoning, so cutting the
//! reasoning takes almost all of them. This holds the rest, under the caveat
//! `tests/comment.rs` carries: a character's presence is a fact, and whether a
//! sentence earned its place still needs a reader. One exception, and it is a
//! shape: a labeled row puts the dash between a value and its qualifier.

mod tree;

use std::path::PathBuf;

use tree::{read, relative};

/// What a row's value may run to before the dash after it is a sentence's
/// rather than a table's. `Reserve: none ` is the longest Perch has.
const VALUE: usize = 20;

/// `src/` alone: the guide, `docs/adr/` and `tests/` keep their dashes, and a
/// test asserting what Perch says has to be free to quote it. Read off the
/// relative path rather than `Path::starts_with`, which follows no links and is
/// disallowed here for the reads that need to.
fn sources() -> Vec<PathBuf> {
    tree::sources(|path| {
        path.extension().is_some_and(|kind| kind == "rs")
            && relative(path).split(['/', '\\']).next() == Some("src")
    })
}

/// The file with its comments blanked and its test module dropped, so what is
/// left is what Perch says. Lines rather than bytes, to keep the numbering a
/// failure reports.
fn what_perch_says(text: &str) -> String {
    let text = match text.find("\n#[cfg(test)]") {
        Some(at) => &text[..at],
        None => text,
    };
    text.lines()
        .map(|line| match line.trim_start().starts_with("//") {
            true => "",
            false => line,
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Every string literal, as one line each with the line it opens on. A literal
/// carrying a real newline is the scan running past a char literal rather than
/// a sentence, and is dropped.
fn literals(text: &str) -> Vec<(usize, String)> {
    let bytes: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut line = 1;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            '\n' => line += 1,
            '"' => {
                let opened = line;
                let mut said = String::new();
                at += 1;
                while at < bytes.len() && bytes[at] != '"' {
                    if bytes[at] == '\n' {
                        line += 1;
                    }
                    if bytes[at] == '\\' && at + 1 < bytes.len() {
                        at += 1;
                        if bytes[at] == '\n' {
                            line += 1;
                        }
                    }
                    said.push(bytes[at]);
                    at += 1;
                }
                if !said.contains('\n') || said.contains("\\\n") {
                    found.push((opened, said));
                }
            }
            _ => {}
        }
        at += 1;
    }
    found
}

/// Whether the dash in this literal is a row's rather than a sentence's: one
/// of them, and what stands before it is a value rather than a clause.
fn is_a_row(said: &str) -> bool {
    let parts: Vec<&str> = said.split('—').collect();
    let [value, _] = parts[..] else {
        return false;
    };
    let value = value.replace("\\\n", " ");
    let value = value.trim();
    value.chars().count() <= VALUE && !value.contains('.')
}

#[test]
fn nothing_perch_says_carries_an_em_dash() {
    let mut said = Vec::new();
    for path in sources() {
        let Some(text) = read(&path) else { continue };
        for (line, literal) in literals(&what_perch_says(&text)) {
            if literal.contains('—') && !is_a_row(&literal) {
                said.push(format!("{}:{line}", relative(&path)));
            }
        }
    }

    assert!(
        said.is_empty(),
        "an em dash is a sentence appending its reasoning, and Perch states the \
         verdict and the next command (ADR perch-says-what-it-did). A labeled \
         row may put one between a value and its qualifier:\n{}",
        said.join("\n")
    );
}

/// The other half: a rule with an exception nothing takes is a rule stated
/// twice, and one every site takes is not a rule.
#[test]
fn the_row_is_the_only_exception_and_something_takes_it() {
    let taken: Vec<String> = sources()
        .iter()
        .filter_map(|path| Some((path, read(path)?)))
        .flat_map(|(path, text)| {
            literals(&what_perch_says(&text))
                .into_iter()
                .filter(|(_, said)| said.contains('—'))
                .map(|(line, _)| format!("{}:{line}", relative(path)))
                .collect::<Vec<String>>()
        })
        .collect();

    assert!(
        !taken.is_empty(),
        "no labeled row carries a dash any more, so the exception is a clause \
         with nothing under it and goes"
    );
}

/// Named so a failure above can be read: what the scan takes a literal to be.
#[test]
fn a_literal_is_read_to_its_closing_quote() {
    let said = literals("let a = \"one — two\"; let b = '\"'; let c = \"three\";");
    assert_eq!(said[0].1, "one — two");
    assert!(is_a_row("off — `interchangeable` is false"));
    assert!(is_a_row(" — {}"));
    assert!(!is_a_row(
        "PERCH_HOME is not an absolute path — so where Perch holds them moves"
    ));
    assert!(
        !is_a_row("a — b — c"),
        "two dashes is prose whatever stands before the first",
    );
}
