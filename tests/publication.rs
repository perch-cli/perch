//! What the site publishes, asserted against the repository that publishes it
//! (ADR 0062).
//!
//! The guide is written once, in `pages/src/content/docs/`, and read in two
//! places: on GitHub, and rendered by Starlight at
//! `https://perch-cli.github.io/perch/`. Nothing in either of those places fails
//! loudly — a link to a heading that has been renamed is a 404 somebody else
//! finds, and a page nobody linked to is one nobody reaches. So the things that
//! would go quietly wrong are asserted here, on every pull request, rather than
//! discovered on the deployed site.
//!
//! The other half is the constraint the site had before it had a guide: the
//! installers are pasted into terminals from a versionless URL, so they sit at
//! the root of what is deployed and the front page quotes them exactly.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where the guide is written. Starlight's `docsLoader()` reads this directory
/// and takes no option for another one (ADR 0062).
fn guide() -> PathBuf {
    repo().join("pages/src/content/docs")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("{} is readable: {err}", path.display()))
}

/// Every guide page there is, by file name. The splash page is excepted by its
/// extension: it is the site's front page and the guide's index rather than a
/// page of the guide, and it is the one file that is `.mdx`.
fn guide_pages() -> BTreeSet<String> {
    std::fs::read_dir(guide())
        .expect("pages/src/content/docs is a directory")
        .map(|entry| entry.expect("a readable entry").file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".md"))
        .collect()
}

/// The splash page, which is what the site serves at its root.
fn splash() -> PathBuf {
    guide().join("index.mdx")
}

/// The lines of a markdown document that are prose rather than fenced code. A
/// transcript is most of what these pages are, and every one of them quotes URLs
/// and prints `#` — so reading a link or a heading out of one would be reading
/// what Perch said as if the page had said it.
fn prose(markdown: &str) -> impl Iterator<Item = &str> {
    let mut fenced = false;
    markdown.lines().filter(move |line| {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            return false;
        }
        !fenced
    })
}

/// The `[text](destination)` of every link in a markdown document.
fn links(markdown: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in prose(markdown) {
        let mut rest = line;
        while let Some(open) = rest.find("](") {
            rest = &rest[open + 2..];
            match rest.find(')') {
                Some(close) => {
                    found.push(rest[..close].to_string());
                    rest = &rest[close..];
                }
                None => break,
            }
        }
    }
    found
}

/// The anchor a heading answers to, by the rule GitHub and Starlight agree on:
/// lower-cased, spaces hyphenated, and everything that is not a letter, a digit,
/// a hyphen or an underscore dropped.
fn slug(heading: &str) -> String {
    heading
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c.to_lowercase().next().unwrap_or(c)),
            _ => None,
        })
        .collect()
}

/// Every anchor a markdown document offers, taken from its ATX headings.
fn anchors(markdown: &str) -> BTreeSet<String> {
    prose(markdown)
        .filter(|line| line.starts_with('#'))
        .map(|line| slug(line.trim_start_matches('#')))
        .collect()
}

/// Asserts that one link out of `from` lands on a page that exists and, where it
/// names one, a heading that exists. Only the destination is checked here, so a
/// URL is somebody else's to keep working — but `#section` on its own is a link
/// into `from` itself, and is checked against it rather than skipped.
fn resolves(from: &Path, link: &str) {
    if link.starts_with("http") {
        return;
    }
    let (path, anchor) = match link.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (link, None),
    };
    let target = if path.is_empty() {
        from.to_path_buf()
    } else {
        from.parent().expect("a parent directory").join(path)
    };
    assert!(
        target.exists(),
        "{} links to {link}, which is not there",
        from.display()
    );
    if let Some(anchor) = anchor {
        let offered = anchors(&read(&target));
        assert!(
            offered.contains(anchor),
            "{} links to {link}, and {} has no such heading — it offers {offered:?}",
            from.display(),
            target.display()
        );
    }
}

/// The guide is written once and indexed twice, which is one fewer than it used
/// to be: the sidebar is autogenerated and so cannot omit a page, and the table
/// that was the guide's own index is now the splash page's card grid, asserted by
/// `the_landing_page_leads_into_the_guide` below (ADR 0062).
///
/// What remains here is `README.md`, which is what npm shows, and the reason the
/// README was not cut down to a link. Its links are markdown paths rather than
/// site URLs, so a page it stops naming is one an offline clone cannot find.
///
/// What it asserts is that the README *mentions* every page, not that any
/// particular list within it does. It names most pages twice — once in the
/// command table and once under Guides — and requiring a page in both would be
/// asserting the shape of the README rather than that it points at the guide.
#[test]
fn every_guide_page_is_named_by_every_index_of_the_guide() {
    let indexes = [("README.md", "the README would not point at it")];

    for (index, consequence) in indexes {
        let named: BTreeSet<String> = links(&read(&repo().join(index)))
            .into_iter()
            .filter_map(|link| {
                link.rsplit('/')
                    .next()
                    .filter(|name| name.ends_with(".md"))
                    .map(str::to_string)
            })
            .collect();

        for page in guide_pages() {
            assert!(
                named.contains(&page),
                "{page} is a guide page that {index} does not name, so {consequence}"
            );
        }
    }
}

/// A relative link is resolved by the reader — GitHub against the repository, a
/// browser against the deployed site — and only the ones that stay inside the
/// guide mean the same thing to both. A link up out of the guide's directory
/// reaches `CONTEXT.md` on GitHub and a 404 on the site, so those are written as
/// URLs.
#[test]
fn a_guide_page_links_out_of_the_guide_only_by_url() {
    for page in guide_pages() {
        let path = guide().join(&page);
        for link in links(&read(&path)) {
            assert!(
                !link.starts_with("../"),
                "{page} links to {link}, which leaves the guide — the site cannot follow it, so it wants an https:// URL"
            );
            resolves(&path, &link);
        }
    }
}

/// The README's command table is a column of links into the guide, and its
/// anchors are the part that rots: a heading reworded three files away breaks it
/// without touching the README at all.
#[test]
fn the_readme_links_into_the_guide_land() {
    let readme = repo().join("README.md");
    for link in links(&read(&readme)) {
        resolves(&readme, &link);
    }
}

/// `pages/public/` is copied to the root of what Astro emits, so an installer
/// there is an installer at `https://perch-cli.github.io/perch/install.sh` —
/// which is the URL that is already pasted into terminals and in every guide.
/// Neither the guide moving nor the renderer changing must have moved these.
#[test]
fn the_installers_stay_at_the_root_of_the_site() {
    for installer in ["install.sh", "install.ps1"] {
        assert!(
            repo().join("pages/public").join(installer).is_file(),
            "pages/public/{installer} is what https://perch-cli.github.io/perch/{installer} serves"
        );
    }
}

/// Every place that quotes an installer URL quotes the same one, and no place
/// quotes any other URL on the site. Both halves matter: the first catches an
/// installer that stopped being mentioned, and the second catches one that grew
/// a version or moved under a prefix — which is a command somebody has already
/// pasted into a shell, now fetching nothing.
///
/// What the site serves is the splash page, these two files, and one page per
/// guide page, so the set below is exhaustive by construction rather than by
/// having been kept up to date.
#[test]
fn every_url_on_the_site_is_one_the_site_serves() {
    const SITE: &str = "https://perch-cli.github.io/perch/";

    let mut served: BTreeSet<String> = ["", "install.sh", "install.ps1"]
        .into_iter()
        .map(str::to_string)
        .collect();
    for page in guide_pages() {
        served.insert(format!("{}/", page.trim_end_matches(".md")));
    }

    let quoting = [
        "README.md",
        "pages/src/content/docs/installing.md",
        "pages/src/content/docs/index.mdx",
        "pages/public/install.sh",
        "pages/public/install.ps1",
    ];

    for file in quoting {
        let text = read(&repo().join(file));

        let mut rest = text.as_str();
        while let Some(at) = rest.find(SITE) {
            rest = &rest[at + SITE.len()..];
            // Whatever follows the site's root, up to whatever ended the URL.
            let path: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && !"\"'`)>|".contains(*c))
                .collect();
            assert!(
                served.contains(&path),
                "{file} quotes {SITE}{path}, which is not one of the {served:?} the site serves"
            );
        }
    }

    // And the two that are pasted into terminals are still quoted where somebody
    // reading would look for them.
    for file in ["README.md", "pages/src/content/docs/installing.md"] {
        let text = read(&repo().join(file));
        for installer in ["install.sh", "install.ps1"] {
            assert!(
                text.contains(&format!("{SITE}{installer}")),
                "{file} should quote {SITE}{installer}, and does not"
            );
        }
    }
}

/// The front page's job is to say what Perch does. It has to show that before it
/// asks for a paste into a terminal.
#[test]
fn the_landing_page_shows_perch_before_it_asks_you_to_install() {
    let landing = read(&splash());
    let install = landing
        .find("curl -fsSL")
        .expect("the landing page offers the installer");

    for shown in ["perch switch", "perch watcher run"] {
        let at = landing
            .find(shown)
            .unwrap_or_else(|| panic!("the landing page shows `{shown}`"));
        assert!(
            at < install,
            "the landing page asks for an install before it shows `{shown}`"
        );
    }
}

/// The splash page is the guide's index as well as the site's front page
/// (ADR 0062), so this is asserted in both directions: every page it names is one
/// the guide has, and every page the guide has is one it names. The links are
/// written as the paths the site serves — `/perch/accounts/` — which is a
/// spelling nothing else in the repository uses and nothing else would catch.
#[test]
fn the_landing_page_leads_into_the_guide() {
    const ROOT: &str = "/perch/";
    let landing = read(&splash());
    let mut led_to = BTreeSet::new();

    // Only a link written from the root of the site: an installer URL contains
    // `/perch/` too, and it is an absolute URL rather than a path on the site.
    let mut rest = landing.as_str();
    let mut opened = 0;
    while let Some(at) = rest.find(ROOT) {
        let before = landing[..opened + at].chars().next_back();
        rest = &rest[at + ROOT.len()..];
        opened += at + ROOT.len();
        if !matches!(before, Some('"') | Some('\'') | Some('(')) {
            continue;
        }
        let href: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && !"\"')".contains(*c))
            .collect();

        let (page, anchor) = match href.split_once('#') {
            Some((page, anchor)) => (page, Some(anchor)),
            None => (href.as_str(), None),
        };
        // `/perch/` on its own is the splash page itself.
        let page = page.trim_end_matches('/');
        if page.is_empty() {
            continue;
        }

        let file = format!("{page}.md");
        let source = guide().join(&file);
        assert!(
            source.is_file(),
            "the landing page links to {ROOT}{href}, and there is no {} to render it from",
            source.display()
        );
        if let Some(anchor) = anchor {
            let offered = anchors(&read(&source));
            assert!(
                offered.contains(anchor),
                "the landing page links to {ROOT}{href}, and {} has no such heading — it offers {offered:?}",
                source.display()
            );
        }
        led_to.insert(file);
    }

    for page in guide_pages() {
        assert!(
            led_to.contains(&page),
            "{page} is a guide page the front page does not lead to, and the front page is the guide's index"
        );
    }
}

/// Starlight requires `title` in frontmatter and renders it as the page's `h1`
/// (ADR 0062). A page without one does not build; a page with one *and* an `#`
/// heading of its own says its title twice, which builds and is wrong.
#[test]
fn every_page_says_its_title_in_frontmatter() {
    for page in pages_of_the_site() {
        let text = read(&guide().join(&page));
        let frontmatter = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .map(|(frontmatter, _)| frontmatter)
            .unwrap_or_else(|| panic!("{page} opens with frontmatter, and Starlight needs it to"));
        assert!(
            frontmatter
                .lines()
                .any(|line| line.starts_with("title:") && line.len() > "title:".len() + 1),
            "{page} has frontmatter that does not say a title, and Starlight renders the title as the h1"
        );
    }
}

/// The other half of the same rule.
#[test]
fn no_page_says_its_title_a_second_time_as_a_heading() {
    for page in pages_of_the_site() {
        let text = read(&guide().join(&page));
        // Past the frontmatter, whose `---` fence is not a heading of any level.
        let body = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .map(|(_, body)| body)
            .unwrap_or(&text);
        for line in prose(body) {
            assert!(
                !line.starts_with("# "),
                "{page} opens a level-one heading — Starlight renders the frontmatter title as the h1, so this one is the title said twice"
            );
        }
    }
}

/// Every page the site renders: the guide, and the splash page that indexes it.
fn pages_of_the_site() -> BTreeSet<String> {
    let mut pages = guide_pages();
    pages.insert("index.mdx".to_string());
    pages
}
