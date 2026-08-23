//! What a citation names, asserted against the directory it names.
//!
//! `ADR <slug>` is the one form in every file type: the slug is the tail of a
//! filename under `docs/adr/`, the numeric prefix is a sort key that appears in
//! no citation, and a document is found by globbing. None of that fails loudly
//! — a citation naming a document that merged away resolves to nothing, and
//! nothing reads it until somebody goes looking. So it is asserted here, on
//! every pull request.
//!
//! The form is `CLAUDE.md`'s; the reasoning is `docs/agents/comments.md`'s.

mod tree;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tree::{read, relative, repo};

/// At most this many characters, hard rather than a target: unlike a comment
/// cap there is no "write the ADR" escape hatch, so a slug that drifts past it
/// is a title to shorten.
const CAP: usize = 30;

/// Where a number is a record rather than a citation: `CHANGELOG.md` and the ADR
/// inventory both say what was true on a date, so a number in either may name a
/// document that is gone.
fn exempt(path: &Path) -> bool {
    let relative = path.strip_prefix(repo()).unwrap_or(path);
    relative == Path::new("CHANGELOG.md") || relative == Path::new("docs/research/adr-inventory.md")
}

fn sources() -> Vec<PathBuf> {
    tree::sources(|path| !exempt(path))
}

/// Every document there is, by the tail of its filename. The prefix is a sort
/// key, so the tail is the whole of what a citation may name.
fn documents() -> BTreeMap<String, usize> {
    let mut by_slug: BTreeMap<String, usize> = BTreeMap::new();
    for entry in std::fs::read_dir(repo().join("docs/adr")).expect("docs/adr is a directory") {
        let name = entry.expect("a readable entry").file_name();
        let name = name.to_str().expect("a UTF-8 filename");
        if let Some(stem) = name.strip_suffix(".md")
            && let Some((_, slug)) = stem.split_once('-')
        {
            *by_slug.entry(slug.to_owned()).or_default() += 1;
        }
    }
    by_slug
}

/// A number, or a slug: the two things `ADR` can turn out to be followed by.
/// Prose is neither, which is what hyphenation buys — it is how a citation is
/// told from the `ADR` in "this ADR declines".
fn cited(word: &str) -> bool {
    let numeric = word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit());
    let hyphenated = word.contains('-')
        && word
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()));
    numeric || hyphenated
}

/// Every citation in one file, as its line and what it names. A citation wraps
/// across two comment lines in this tree, so the scan reads the whole text and
/// steps over the marker a continuation line begins with.
fn citations(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (at, _) in text.match_indices("ADR") {
        if text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '-')
        {
            continue;
        }
        let mut cursor = at + "ADR".len();
        if !bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let gap = text[cursor..]
            .find(|c: char| !c.is_whitespace())
            .map_or(text.len() - cursor, |offset| offset);
        let wrapped = text[cursor..cursor + gap].contains('\n');
        cursor += gap;
        if wrapped {
            for marker in ["///", "//!", "//", "#", ">", "*"] {
                if let Some(rest) = text[cursor..].strip_prefix(marker) {
                    cursor += marker.len();
                    cursor += rest.len() - rest.trim_start_matches([' ', '\t']).len();
                    break;
                }
            }
        }
        let word: String = text[cursor..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if cited(&word) {
            found.push((text[..at].lines().count().max(1), word));
        }
    }
    found
}

fn every_citation() -> BTreeMap<PathBuf, Vec<(usize, String)>> {
    sources()
        .into_iter()
        .filter_map(|path| {
            let text = read(&path)?;
            let found = citations(&text);
            (!found.is_empty()).then_some((path, found))
        })
        .collect()
}

/// Every citation there is, gathered once and reported as a list of sites so a
/// failure names all of them rather than the first.
fn sites(wrong: impl Fn(&str) -> bool + Copy) -> Vec<String> {
    every_citation()
        .iter()
        .flat_map(|(path, found)| {
            found
                .iter()
                .filter(|(_, word)| wrong(word))
                .map(|(line, word)| format!("{}:{line} cites {word}", relative(path)))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn nothing_cites_a_number() {
    let numeric = sites(|word| word.bytes().all(|b| b.is_ascii_digit()));

    assert!(
        numeric.is_empty(),
        "a citation names a slug, and the prefix is a sort key:\n{}",
        numeric.join("\n")
    );
}

#[test]
fn every_cited_slug_names_exactly_one_document() {
    let documents = documents();
    let unresolved = sites(|slug| documents.get(slug) != Some(&1));

    assert!(
        unresolved.is_empty(),
        "every citation globs docs/adr/*-<slug>.md to exactly one file:\n{}",
        unresolved.join("\n")
    );
}

#[test]
fn every_cited_slug_is_within_the_cap_and_hyphenated() {
    let over: BTreeSet<String> = every_citation()
        .values()
        .flatten()
        .map(|(_, slug)| slug.clone())
        .filter(|slug| slug.len() > CAP || !slug.contains('-'))
        .collect();

    assert!(
        over.is_empty(),
        "a slug is at most {CAP} characters and never one word:\n{}",
        Vec::from_iter(over).join("\n")
    );
}

/// A citation addresses a reader with the tree in front of them. Three places
/// face one who has not: the guide, and the two files GitHub and the npm
/// registry put in front of somebody deciding whether to install. Each loses
/// the word rather than gaining slugs, and the word is what is asserted —
/// "the numbered ADRs" is not a citation and is the same defect.
#[test]
fn nothing_facing_a_reader_without_the_tree_says_adr() {
    let guide = repo().join("pages/src/content/docs");
    let mut faced = vec![repo().join("README.md"), repo().join("SECURITY.md")];
    for entry in std::fs::read_dir(&guide).expect("the guide is a directory") {
        faced.push(entry.expect("a readable entry").path());
    }

    let mut said = Vec::new();
    for path in faced {
        let text = std::fs::read_to_string(&path).expect("a readable page");
        for (number, line) in text.lines().enumerate() {
            if line
                .split(|c: char| !c.is_ascii_alphabetic())
                .any(|word| word == "ADR")
            {
                said.push(format!("{}:{}", relative(&path), number + 1));
            }
        }
    }

    assert!(
        said.is_empty(),
        "a page a reader arrives at says what it does rather than citing a \
         decision they have no tree to glob:\n{}",
        said.join("\n")
    );
}

/// The other half of a citation resolving: a slug globs to one file, and that
/// file is the document whose title it is. Held back until the set was closed,
/// because a document that merges away keeps the title it had.
#[test]
fn every_document_is_named_by_its_own_title() {
    let mut wrong = Vec::new();
    for entry in std::fs::read_dir(repo().join("docs/adr")).expect("docs/adr is a directory") {
        let path = entry.expect("a readable entry").path();
        let text = std::fs::read_to_string(&path).expect("a readable document");
        let title = text.lines().next().unwrap_or("").trim_start_matches("# ");
        let slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let slug = slug
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let tail = stem.split_once('-').map_or("", |(_, tail)| tail);
        if slug != tail {
            wrong.push(format!("{}: titled {title:?}", relative(&path)));
        }
    }

    assert!(
        wrong.is_empty(),
        "a filename's tail is its document's title, kebab-cased:\n{}",
        wrong.join("\n")
    );
}

/// The prefix's own half. Band `N` occupies `N01` through `N99`, so lexicographic
/// sort is the intended order and a new document appends inside its band. Which
/// band a document belongs to is a judgment; that a band runs from `01` with no
/// gap and no number twice is not, and a gap is what a merge leaves behind.
#[test]
fn every_band_is_contiguous_from_its_base() {
    let mut positions: BTreeMap<u32, BTreeMap<u32, Vec<String>>> = BTreeMap::new();
    let mut malformed = Vec::new();
    for entry in std::fs::read_dir(repo().join("docs/adr")).expect("docs/adr is a directory") {
        let path = entry.expect("a readable entry").path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let prefix = stem.split('-').next().unwrap_or("");
        let number = (prefix.len() == 4 && prefix.bytes().all(|b| b.is_ascii_digit()))
            .then(|| prefix.parse::<u32>().expect("four digits"));
        match number {
            Some(number) => positions
                .entry(number / 100)
                .or_default()
                .entry(number % 100)
                .or_default()
                .push(relative(&path)),
            None => malformed.push(relative(&path)),
        }
    }

    let mut wrong = malformed;
    for (band, within) in &positions {
        for (offset, (at, documents)) in within.iter().enumerate() {
            let expected = offset as u32 + 1;
            if documents.len() > 1 {
                wrong.push(format!(
                    "{band:02}{at:02} names {}",
                    documents.join(" and ")
                ));
            } else if *at != expected {
                wrong.push(format!(
                    "{} sits at {at:02}, not {expected:02}",
                    documents[0]
                ));
            }
        }
    }
    wrong.sort();

    assert!(
        wrong.is_empty(),
        "band N runs N01 upward, four digits, no gap and no number twice:\n{}",
        wrong.join("\n")
    );
}
