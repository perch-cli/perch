//! The repository's own files, as the two suites that assert about text walk
//! them.
//!
//! Both walk the whole tree and differ only in which files they keep, so the
//! walk is here and the filter stays with the suite. A list of areas instead
//! would be a list to keep in step with the tree, and a file on nobody's list
//! is the gap eleven green pull requests hide.

use std::path::{Path, PathBuf};

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A path as a failure names it, so a reader can be sent straight to it.
pub fn relative(path: &Path) -> String {
    path.strip_prefix(repo())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Build output and a dependency tree nobody here wrote.
pub fn skipped(name: &str) -> bool {
    matches!(name, ".git" | "target" | "node_modules" | "dist" | ".astro")
}

/// Every file under the repository that `keep` says its suite binds, sorted so
/// a failure lists them in the order somebody reads a tree.
pub fn sources(keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![repo()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable directory") {
            let path = entry.expect("a readable entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if skipped(name) {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if keep(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// A file's text, or nothing where it is not text at all.
pub fn read(path: &Path) -> Option<String> {
    String::from_utf8(std::fs::read(path).ok()?).ok()
}
