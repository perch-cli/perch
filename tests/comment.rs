//! The two clauses of the comment standard a script can hold.
//!
//! `AGENTS.md` states four. What a comment may say, and whether it says it in
//! the present tense, are judgments no text comparison reaches; the per-tier
//! caps and one citation per decision per file are text comparisons, and
//! eleven rewriting passes drifted on exactly those two — a two-paragraph
//! `///` is six lines once the separator counts, and a decision cited nine
//! times in one file reads apt at every site. So passing this is not passing
//! the standard. The reasoning is `docs/agents/comments.md`'s.

mod tree;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tree::{read, relative};

/// What a block of each role may spend, from `AGENTS.md`'s table. Read by role
/// rather than by marker, because a file with no `///` still has a header, a
/// block above a declaration, and a note at a site.
const CAPS: [(&str, usize); 3] = [("header", 10), ("declaration", 5), ("site", 3)];

/// How a file spells a comment: the line markers it uses, longest first, and
/// whether `/* … */` is one. A file type absent here is one the standard does
/// not reach — Markdown is a document, and JSON and a lockfile carry no comment.
fn syntax(path: &Path) -> Option<(&'static [&'static str], bool)> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".gitignore") {
        return Some((&["#"], false));
    }
    if name.ends_with("-lock.yaml") {
        return None;
    }
    match path.extension()?.to_str()? {
        "rs" => Some((&["//!", "///", "//"], false)),
        "toml" | "yml" | "yaml" | "sh" | "ps1" => Some((&["#"], false)),
        "ts" | "js" | "mjs" | "astro" => Some((&["//"], true)),
        "css" => Some((&[], true)),
        _ => None,
    }
}

/// Every file the standard binds, which is every file it knows a comment syntax
/// for.
fn sources() -> Vec<PathBuf> {
    tree::sources(|path| syntax(path).is_some())
}

struct Block {
    /// One-based, so a failure names a line an editor can be sent to.
    line: usize,
    marker: &'static str,
    role: &'static str,
    /// First line carrying text to last, inclusive. A blank `///` between two
    /// paragraphs counts, because two facts in one block is what the cap is
    /// against; a `/*` or `*/` on a line of its own does not, because that is
    /// what the syntax spends on itself rather than what the author spends.
    cost: usize,
    text: String,
}

/// What a line says with its markers off, for measuring and for citations.
fn stripped(line: &str, marker: &str) -> String {
    let bare = line.trim();
    let bare = bare.strip_prefix(marker).unwrap_or(bare);
    let bare = bare.trim_start_matches("/*").trim_end_matches("*/");
    bare.trim_start_matches('*').trim().to_owned()
}

fn measured(lines: &[String]) -> (usize, String) {
    let carries = |line: &String| !line.is_empty();
    let first = lines.iter().position(carries);
    let last = lines.iter().rposition(carries);
    match (first, last) {
        (Some(first), Some(last)) => (last - first + 1, lines[first..=last].join("\n")),
        _ => (0, String::new()),
    }
}

/// Which marker a line opens a block with, longest first so `///` is never read
/// as a `//`. A shebang is the kernel's line rather than a comment.
fn opens<'a>(line: &str, markers: &[&'a str]) -> Option<&'a str> {
    let bare = line.trim();
    if bare.starts_with("#!") {
        return None;
    }
    markers
        .iter()
        .find(|marker| bare.starts_with(**marker))
        .copied()
}

/// Every comment block in one file, in order. The role is Rust's marker where
/// there is one and the block's position where there is not: the file's first
/// block is its header, a block a line of code follows documents that
/// declaration, and anything else is a note at a site.
fn blocks(text: &str, markers: &[&'static str], paired: bool) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found: Vec<Block> = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let (marker, end) = match opens(lines[at], markers) {
            Some(marker) => {
                let mut end = at;
                while end < lines.len() && opens(lines[end], markers) == Some(marker) {
                    end += 1;
                }
                (marker, end)
            }
            None if paired && lines[at].trim().starts_with("/*") => {
                let mut end = at;
                while end < lines.len() && !lines[end].contains("*/") {
                    end += 1;
                }
                ("/*", (end + 1).min(lines.len()))
            }
            None => {
                at += 1;
                continue;
            }
        };
        let body: Vec<String> = lines[at..end].iter().map(|l| stripped(l, marker)).collect();
        let (cost, text) = measured(&body);
        let role = match marker {
            "//!" => "header",
            "///" => "declaration",
            "//" if markers.contains(&"//!") => "site",
            _ if found.is_empty() => "header",
            _ => match lines.get(end) {
                Some(next) if !next.trim().is_empty() => "declaration",
                _ => "site",
            },
        };
        found.push(Block {
            line: at + 1,
            marker,
            role,
            cost,
            text,
        });
        at = end;
    }
    found
}

/// The lines clap renders as `--help`, as a half-open range. A `///` inside an
/// item deriving one of clap's traits is read by a person at a terminal rather
/// than by an agent in the tree, which is what makes the tier caps the wrong
/// measure for it and a citation there wrong outright.
fn printed_by_clap(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut spans = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let derives = line.trim_start().starts_with("#[derive(")
            && ["Parser", "Subcommand", "Args"]
                .iter()
                .any(|trait_name| line.contains(trait_name));
        if !derives {
            continue;
        }
        spans.push((at, closes(&lines, at)));
    }
    spans
}

/// Where the item under a derive ends: the line its brace depth returns to
/// nought. Counted rather than matched against a `}` in column nought, because
/// a span that overshoots its item exempts every `///` below it from the cap in
/// silence, and a silently widened exemption is the one failure here nothing
/// downstream would report.
fn closes(lines: &[&str], from: usize) -> usize {
    let (mut depth, mut opened) = (0usize, false);
    for (at, line) in lines.iter().enumerate().skip(from) {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let (mut quoted, mut escaped) = (false, false);
        for character in line.chars() {
            match character {
                _ if escaped => escaped = false,
                '\\' if quoted => escaped = true,
                '"' => quoted = !quoted,
                '{' if !quoted => (depth, opened) = (depth + 1, true),
                '}' if !quoted => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if opened && depth == 0 {
            return at + 1;
        }
    }
    lines.len()
}

fn within(spans: &[(usize, usize)], line: usize) -> bool {
    spans
        .iter()
        .any(|(from, to)| (*from..*to).contains(&(line - 1)))
}

#[test]
fn every_comment_is_within_the_cap_of_its_tier() {
    let mut over = Vec::new();
    for path in sources() {
        let Some((markers, paired)) = syntax(&path) else {
            continue;
        };
        let Some(text) = read(&path) else { continue };
        let printed = printed_by_clap(&text);
        for block in blocks(&text, markers, paired) {
            let cap = CAPS
                .iter()
                .find_map(|(role, cap)| (*role == block.role).then_some(*cap))
                .expect("every role has a cap");
            if block.cost > cap && !(block.marker == "///" && within(&printed, block.line)) {
                over.push(format!(
                    "{}:{} a {}-line {} where a {} may spend {cap}",
                    relative(&path),
                    block.line,
                    block.cost,
                    block.marker,
                    block.role,
                ));
            }
        }
    }

    assert!(
        over.is_empty(),
        "over the cap is a decision with no ADR, not a comment to shorten: \
         write the ADR, cite it, cut the comment to the fact. Two facts in one \
         block is the commonest shape and one of them belongs elsewhere. \
         Reflowing to fit is the one answer the standard forbids.\n{}",
        over.join("\n")
    );
}

/// The caps make a citation the cheapest thing a comment can carry, so unbounded
/// they trade prose for slugs and a decision named nine times in one file marks
/// nothing. Comments only: an assertion message is read on a failure rather than
/// in the tree, and repeating a citation across four asserts costs nothing.
#[test]
fn no_decision_is_cited_twice_in_one_file() {
    let mut twice = Vec::new();
    for path in sources() {
        let Some((markers, paired)) = syntax(&path) else {
            continue;
        };
        let Some(text) = read(&path) else { continue };
        let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for block in blocks(&text, markers, paired) {
            for slug in cited(&block.text) {
                sites.entry(slug).or_default().push(block.line);
            }
        }
        for (slug, lines) in sites {
            if lines.len() > 1 {
                let lines: Vec<String> = lines.iter().map(usize::to_string).collect();
                twice.push(format!(
                    "{} cites {slug} at {}",
                    relative(&path),
                    lines.join(", ")
                ));
            }
        }
    }

    assert!(
        twice.is_empty(),
        "a decision is cited once per file, at the widest scope that owns it:\n{}",
        twice.join("\n")
    );
}

/// Every slug one block of comment text names. The markers are already off, so
/// a citation wrapped across two comment lines is `ADR` and a slug with
/// whitespace between them, which is what `split_whitespace` reads.
fn cited(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(2)
        .filter(|pair| pair[0] == "ADR" || pair[0] == "(ADR")
        .map(|pair| {
            pair[1]
                .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_owned()
        })
        .filter(|slug| slug.contains('-'))
        .collect()
}

/// The inverse of the exemption above, and a hard failure rather than a
/// judgment: `perch watcher install --help` printing a slug names a document
/// its reader has no tree to glob and did not ask for.
#[test]
fn nothing_clap_prints_cites_a_decision() {
    let mut said = Vec::new();
    for path in sources() {
        let Some((markers, paired)) = syntax(&path) else {
            continue;
        };
        let Some(text) = read(&path) else { continue };
        let printed = printed_by_clap(&text);
        if printed.is_empty() {
            continue;
        }
        for block in blocks(&text, markers, paired) {
            if block.marker == "///" && within(&printed, block.line) {
                said.extend(
                    cited(&block.text)
                        .into_iter()
                        .map(|slug| format!("{}:{} cites {slug}", relative(&path), block.line)),
                );
            }
        }
    }

    assert!(
        said.is_empty(),
        "a doc comment clap renders is what a person reads, and says the fact \
         rather than pointing at a decision:\n{}",
        said.join("\n")
    );
}
