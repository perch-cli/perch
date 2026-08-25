//! What an Upgrade is: replacing this machine's Installation with a newer
//! Release, through the Channel that made it (ADR an-upgrade-asks-its-channel).
//!
//! The only place that knows a Channel can be told apart from another, that a
//! tag and a version are different spellings of one thing, and where the newest
//! Release is published. `commands::upgrade` decides what to *do* about the
//! answers; this decides what the answers are.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{Host, HttpRequest, Platform};

/// The repository every Channel points at.
pub const REPO: &str = "perch-cli/perch";

/// Where the newest Release is asked for. The same endpoint both installers
/// ask, which is the reason ADR this-repo-assembles-a-release refused to mark a
/// Release as a prerelease: doing so would empty this.
pub const LATEST_URL: &str = "https://api.github.com/repos/perch-cli/perch/releases/latest";

/// How long the check on `perch version` may take before it is abandoned.
///
/// Short because nobody asked for it: the line it prints is worth a pause
/// somebody would not notice, not a wait they would.
pub const CHECK_WITHIN_MILLIS: u64 = 2_000;

/// The variable that switches the check on `perch version` off.
pub const NO_CHECK: &str = "PERCH_NO_UPGRADE_CHECK";

/// What is installed, which is what the build says it is.
pub fn installed() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// One route by which this machine's Installation arrived, as far as its own
/// path can say.
///
/// Three rather than four: what the Release page leaves cannot be told from a
/// hand-placed binary, so it is no answer at all rather than a fourth variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// Installed by Homebrew, from the Tap. Carries the prefix above the
    /// Cellar, which is where its `brew` is.
    Homebrew { prefix: PathBuf },
    /// Installed by npm, as the platform package the `perch-cli` wrapper
    /// depends on.
    Npm,
    /// Installed by `install.sh` or `install.ps1`, which is the one Channel
    /// that leaves a binary nothing else manages.
    Installer,
}

impl Channel {
    /// What to call it in a sentence somebody reads.
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Homebrew { .. } => "Homebrew",
            Channel::Npm => "npm",
            Channel::Installer => "the installer",
        }
    }

    /// Whether the Channel works out which Release to install for itself. Two of
    /// the three do, so asking GitHub is only how Perch answers "there is
    /// nothing to do" — worth losing rather than the whole Upgrade.
    pub fn resolves_its_own(&self) -> bool {
        !matches!(self, Channel::Installer)
    }

    /// The word `--channel` takes for it.
    pub fn word(&self) -> &'static str {
        match self {
            Channel::Homebrew { .. } => "homebrew",
            Channel::Npm => "npm",
            Channel::Installer => "installer",
        }
    }

    /// The Channel a person named, for when detection got it wrong.
    ///
    /// Homebrew arrives with no prefix, because somebody typing `--channel
    /// homebrew` is saying which Channel this is and not where its `brew` is —
    /// so that half falls back to the search on `PATH`.
    pub fn spelled(word: &str) -> Option<Channel> {
        match word.to_lowercase().as_str() {
            "homebrew" | "brew" => Some(Channel::Homebrew {
                prefix: PathBuf::new(),
            }),
            "npm" => Some(Channel::Npm),
            "installer" => Some(Channel::Installer),
            _ => None,
        }
    }
}

/// Where the installer script puts the binary on this machine.
///
/// Three answers rather than one: the installers take `PERCH_INSTALL_DIR` above
/// everything, and their own defaults do not agree across platforms.
pub fn installer_dir(host: &dyn Host) -> Result<PathBuf> {
    // Filtered for the reason `LOCALAPPDATA` is below: set-but-empty is the
    // machine not saying, and taken at face value it makes `channel_at` compare
    // against `[]` and the refusal quote a path that is not there.
    if let Some(chosen) = host
        .env_var("PERCH_INSTALL_DIR")
        .filter(|chosen| !chosen.is_empty())
    {
        return Ok(PathBuf::from(chosen));
    }

    let home = host
        .home_dir()
        .map_err(|err| PerchError::Other(format!("could not find your home directory: {err}")))?;

    let home = home.display().to_string();

    Ok(match host.platform() {
        // `%LOCALAPPDATA%` is its own variable rather than a place under home,
        // so it is asked for first — and derived from home only where the machine
        // will not say, which is where the installer's own `Join-Path` fails too.
        Platform::Windows => {
            let local = host
                .env_var("LOCALAPPDATA")
                .filter(|local| !local.is_empty())
                .unwrap_or_else(|| [home.as_str(), "AppData", "Local"].join("/"));
            beneath(&[&local, "Perch", "bin"])
        }
        _ => beneath(&[&home, ".local", "bin"]),
    })
}

/// A path built from its parts, spelled with `/`.
///
/// Rather than `Path::join`, for the reason [`crate::probe::on_path`] gives:
/// `join` follows the platform this build runs on, where everything here follows
/// the platform the *Host* reports. [`segments`] reads either separator.
fn beneath(parts: &[&str]) -> PathBuf {
    PathBuf::from(parts.join("/"))
}

/// Which Channel left the binary at this path, or `None` when nothing about the
/// path says.
///
/// The path is the whole of the evidence: every Channel installs the same bytes,
/// and a marker file beside one would be absent for Homebrew and npm both.
pub fn channel_at(host: &dyn Host, installer_dir: &Path, exe: &Path) -> Option<Channel> {
    let parts = segments(host.platform(), exe);

    // npm first: `brew install node` puts an npm prefix inside a Homebrew one,
    // so a path holding both is npm's Perch under Homebrew's Node.
    if parts.iter().any(|part| part == "node_modules") {
        return Some(Channel::Npm);
    }

    // The Cellar names the Channel and the prefix above it names the `brew` that
    // owns it. Homebrew is not a Windows Channel, so the capital is compared.
    if let Some(at) = parts.iter().position(|part| part == "Cellar") {
        return Some(Channel::Homebrew {
            prefix: PathBuf::from(format!("/{}", parts[..at].join("/"))),
        });
    }

    // Exactly where the installer puts it, and nowhere else. `/usr/local/bin` is
    // where a hand-unpacked Release lands, and is deliberately not here.
    let holding = &parts[..parts.len().saturating_sub(1)];
    let spelled_the_same = holding == segments(host.platform(), installer_dir);

    // The same question of the place rather than of the spelling: `current_exe`
    // resolves every link and `home_dir` reads `$HOME` verbatim, so a link above
    // `bin` hands this one directory under two names.
    let the_same_place = || {
        exe.parent()
            .is_some_and(|holding| crate::host::is_the_same_place(host, holding, installer_dir))
    };

    match spelled_the_same || the_same_place() {
        true => Some(Channel::Installer),
        false => None,
    }
}

/// A path as the parts that name it, compared as the platform compares them.
///
/// Not `Path::components`: on Windows `canonicalize` hands back a verbatim path
/// no comparison against `%LOCALAPPDATA%` matches, and two spellings differing
/// in case are one directory. Neither applies anywhere else.
fn segments(platform: Platform, path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let windows = platform == Platform::Windows;
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);

    text.split(|c| c == '/' || (windows && c == '\\'))
        .filter(|part| !part.is_empty())
        .map(|part| match windows {
            true => part.to_lowercase(),
            false => part.to_string(),
        })
        .collect()
}

/// The same, for the machine this is running on.
pub fn channel(host: &dyn Host) -> Result<Option<Channel>> {
    let exe = host
        .current_exe()
        .map_err(|err| PerchError::Other(format!("could not find Perch's own binary: {err}")))?;
    Ok(channel_at(host, &installer_dir(host)?, &exe))
}

/// A Release as somebody typed it, as the number Perch compares.
///
/// `v0.2.0` and `0.2.0` both arrive: the tag carries a `v` and no other Channel
/// does. Checked here rather than at the download, because a typo that reaches
/// the network comes back as a 404 about an archive.
pub fn version_typed(typed: &str) -> Result<String> {
    let bare = typed.strip_prefix('v').unwrap_or(typed);
    let plausible = {
        let mut parts = bare.splitn(3, '.');
        let numeric = |part: Option<&str>| {
            part.is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        };
        // The third part carries any pre-release or build suffix —
        // `0.2.0-rc.1`, `0.2.0+build.3` — so it is digits and then an optional
        // suffix, spelled out rather than left as "starts with a digit".
        let major = numeric(parts.next());
        let minor = numeric(parts.next());
        let patch = parts.next().is_some_and(|part| {
            let (digits, suffix) = match part.find(['-', '+']) {
                Some(at) => (&part[..at], Some(&part[at + 1..])),
                None => (part, None),
            };
            let semver_ish = |part: &str| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            };
            !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
                && suffix.is_none_or(semver_ish)
        });
        major && minor && patch
    };

    match plausible {
        true => Ok(bare.to_string()),
        false => Err(PerchError::Invalid(format!(
            "`{typed}` is not a Release. They are numbered `0.2.0`, with or \
             without the leading `v`.\n\
             `perch upgrade` without `--release` takes the newest."
        ))),
    }
}

/// The tag a version is published under.
pub fn tag_of(version: &str) -> String {
    format!("v{version}")
}

/// Which of two Releases is newer, by semver's ordering: numbers part by part,
/// then a pre-release suffix as text once they agree — an empty suffix sorts
/// last, which is what puts `0.2.0-rc.1` before `0.2.0` — with build metadata
/// dropped first, because semver gives it no bearing on precedence.
pub fn compare(a: &str, b: &str) -> Ordering {
    let split = |version: &str| -> (Vec<u64>, Vec<String>) {
        let version = match version.find('+') {
            Some(at) => &version[..at],
            None => version,
        };
        let (numbers, pre_release) = match version.find('-') {
            Some(at) => (&version[..at], &version[at + 1..]),
            None => (version, ""),
        };
        (
            numbers
                .split('.')
                // Saturating rather than zero: every version here has been
                // through `version_typed`, so the only way the parse fails is a
                // number too big for a `u64` — which is newer, not oldest.
                .map(|part| part.parse::<u64>().unwrap_or(u64::MAX))
                .collect(),
            match pre_release.is_empty() {
                true => Vec::new(),
                false => pre_release.split('.').map(str::to_string).collect(),
            },
        )
    };

    let (a_numbers, a_pre_release) = split(a);
    let (b_numbers, b_pre_release) = split(b);
    let ordered = a_numbers.cmp(&b_numbers);
    if ordered != Ordering::Equal {
        return ordered;
    }
    match (a_pre_release.is_empty(), b_pre_release.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => by_identifier(&a_pre_release, &b_pre_release),
    }
}

/// Two pre-release suffixes, compared the way semver says: field by field,
/// numerically wherever both fields are numbers.
///
/// A number sorts below a word, which is what keeps `1.0.0-alpha` above
/// `1.0.0-1`; where one suffix runs out first the longer one is the newer.
fn by_identifier(a: &[String], b: &[String]) -> Ordering {
    for (ours, theirs) in a.iter().zip(b) {
        let ordered = match (a_number(ours), a_number(theirs)) {
            (Some(ours), Some(theirs)) => ours.cmp(&theirs),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => ours.cmp(theirs),
        };
        if ordered != Ordering::Equal {
            return ordered;
        }
    }
    a.len().cmp(&b.len())
}

/// One field of a pre-release suffix as the number it is, or nothing where it
/// is a word. Saturating for the reason the version's own components are.
fn a_number(field: &str) -> Option<u64> {
    match !field.is_empty() && field.chars().all(|c| c.is_ascii_digit()) {
        true => Some(field.parse().unwrap_or(u64::MAX)),
        false => None,
    }
}

/// The newest published Release, asked for now.
///
/// No cache and no interval: this happens because somebody typed a command, and
/// the machinery for holding an answer and reasoning about its age is not built
/// a second time for a notification.
pub fn newest(host: &dyn Host, within_millis: Option<u64>) -> Result<String> {
    // GitHub answers an unidentified caller with a 403, and `curl`'s default
    // agent is enough on its own — but a request that says which program made
    // it is the one whose rate-limiting somebody can explain later.
    let agent = format!("perch/{}", installed());
    let headers = [
        ("Accept", "application/vnd.github+json"),
        ("User-Agent", agent.as_str()),
    ];
    let mut request = HttpRequest::get(LATEST_URL, &headers);
    if let Some(millis) = within_millis {
        request = request.within(millis);
    }

    // The one caller outside [`crate::anthropic`]: this asks GitHub which Release
    // is newest, spends no Account's allowance and is reached by a command a
    // person typed, so there is no watch to be still holding.
    #[allow(
        clippy::disallowed_methods,
        reason = "not an Anthropic request, and no Watcher is running behind it"
    )]
    let answered = host.http(&request).map_err(|err| {
        PerchError::Other(format!("could not ask which Release is newest: {err}"))
    })?;

    if answered.status != 200 {
        return Err(PerchError::Other(format!(
            "asking which Release is newest came back {} rather than 200.\n\
             The Releases are at https://github.com/{REPO}/releases.",
            answered.status
        )));
    }

    let document: serde_json::Value = serde_json::from_str(&answered.body).map_err(|err| {
        PerchError::Other(format!(
            "could not read the answer about which Release is newest: {err}"
        ))
    })?;
    let tag = document
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            PerchError::Other("the answer about which Release is newest names no tag".to_string())
        })?;

    version_typed(tag)
}

/// The line `perch version` adds when there is a newer Release, and nothing
/// at all otherwise.
///
/// Every reason not to ask is checked before asking, and the ask swallows
/// whatever went wrong: an offline machine loses a line nobody requested.
pub fn notice(host: &dyn Host) -> Option<String> {
    if host.env_var(NO_CHECK).is_some() {
        return None;
    }
    // Not a terminal is not a person: a script parsing `perch version` and
    // the Homebrew formula's test block both read this output, and neither
    // wants a second line or a network request.
    if !host.is_interactive() {
        return None;
    }

    let newest = newest(host, Some(CHECK_WITHIN_MILLIS)).ok()?;
    match compare(&newest, installed()) {
        Ordering::Greater => Some(format!(
            "a newer Release is available: {newest} — `perch upgrade`"
        )),
        _ => None,
    }
}

/// The whole of what `perch version` says.
///
/// Here rather than in `main`, the one place no test reaches: what is printed,
/// whether the second line appears, and that the first is spelled `perch
/// <version>` as clap spelled it — which the formula's test block asserts on.
pub fn version_report(host: &dyn Host) -> String {
    let installed = installed();
    match notice(host) {
        Some(line) => format!("perch {installed}\n{line}\n"),
        None => format!("perch {installed}\n"),
    }
}

/// The command that hands the work back to Homebrew, as program and arguments.
///
/// The `brew` beside the Cellar rather than the first one on `PATH`
/// (ADR a-crate-must-not-cost-a-seam): a machine with two Homebrew prefixes has
/// two `brew`s, and only the one that owns this Installation can replace it.
pub fn homebrew_command(host: &dyn Host, prefix: &Path) -> Result<(PathBuf, Vec<String>)> {
    let brew = match prefix.as_os_str().is_empty() {
        true => crate::probe::on_path(host, "brew"),
        // Asked for rather than assumed, so a prefix whose `bin/brew` has gone
        // reaches the refusal below — which names the command to type — rather
        // than a "No such file or directory" from running it.
        false => Some(beneath(&[&prefix.display().to_string(), "bin", "brew"]))
            .filter(|brew| host.is_file(brew)),
    };
    let brew = brew.ok_or_else(|| {
        PerchError::NotFound(
            "this Installation came from Homebrew, and no `brew` was found to \
             hand it back to.\n\
             `brew upgrade perch` is the command, once `brew` is on PATH."
                .to_string(),
        )
    })?;
    Ok((brew, vec!["upgrade".to_string(), "perch".to_string()]))
}

/// The same for npm, which unlike Homebrew can be pointed at a Release.
///
/// `npm update` only ever goes to the newest, so naming one is `npm install`
/// with the version attached — a different command rather than a flag on the
/// same one.
pub fn npm_command(host: &dyn Host, version: Option<&str>) -> Result<(PathBuf, Vec<String>)> {
    let npm = crate::probe::on_path(host, "npm").ok_or_else(|| {
        PerchError::NotFound(
            "this Installation came from npm, and no `npm` was found to hand it \
             back to.\n\
             `npm update -g perch-cli` is the command, once `npm` is on PATH."
                .to_string(),
        )
    })?;
    Ok((npm, npm_arguments(version)))
}

/// What `npm` is told, whichever way the command is reached.
///
/// Its own function because Windows prints this command rather than running it,
/// and does so before any `npm` has been found: two spellings of `perch-cli`
/// are two that come to disagree.
pub fn npm_arguments(version: Option<&str>) -> Vec<String> {
    match version {
        Some(version) => vec![
            "install".to_string(),
            "-g".to_string(),
            format!("perch-cli@{version}"),
        ],
        None => vec![
            "update".to_string(),
            "-g".to_string(),
            "perch-cli".to_string(),
        ],
    }
}

/// The installer this platform is upgraded by, embedded rather than fetched.
///
/// `include_str!` at build time: downloading a script at run time and executing
/// it is a sentence a program that holds Credentials should not have to defend.
/// Pinning the copy to the build costs nothing — a tag is all that goes in.
pub fn installer_for(platform: Platform) -> (&'static str, &'static str) {
    match platform {
        Platform::Windows => (
            "perch-upgrade.ps1",
            include_str!("../pages/public/install.ps1"),
        ),
        _ => (
            "perch-upgrade.sh",
            include_str!("../pages/public/install.sh"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLER_DIR: &str = "/home/someone/.local/bin";

    /// A machine of the platform being asked about and nothing else on it, so
    /// the two paths settle to themselves and the spelling is the whole answer.
    fn machine(platform: Platform) -> crate::host::FakeHost {
        crate::host::FakeHost::new().with_platform(platform)
    }

    fn channel_of(exe: &str) -> Option<Channel> {
        channel_at(
            &machine(Platform::Other),
            Path::new(INSTALLER_DIR),
            Path::new(exe),
        )
    }

    #[test]
    fn a_cellar_names_homebrew_and_the_prefix_above_it_names_its_brew() {
        assert_eq!(
            channel_of("/opt/homebrew/Cellar/perch/0.2.0/bin/perch"),
            Some(Channel::Homebrew {
                prefix: PathBuf::from("/opt/homebrew")
            })
        );
        assert_eq!(
            channel_of("/usr/local/Cellar/perch/0.2.0/bin/perch"),
            Some(Channel::Homebrew {
                prefix: PathBuf::from("/usr/local")
            })
        );
    }

    #[test]
    fn a_node_modules_names_npm_wherever_the_prefix_is() {
        for exe in [
            "/usr/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
            "/home/someone/.nvm/versions/node/v20.11.0/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
        ] {
            assert_eq!(channel_of(exe), Some(Channel::Npm), "{exe}");
        }
    }

    #[test]
    fn an_npm_installation_under_a_homebrew_prefix_is_npms() {
        assert_eq!(
            channel_of(
                "/opt/homebrew/Cellar/node/21.6.1/lib/node_modules/perch-cli/node_modules/@perch-cli/darwin-arm64/bin/perch"
            ),
            Some(Channel::Npm)
        );
    }

    #[test]
    fn exactly_where_the_installer_puts_it_is_the_installers() {
        assert_eq!(
            channel_of("/home/someone/.local/bin/perch"),
            Some(Channel::Installer)
        );
    }

    #[test]
    fn a_binary_perch_did_not_place_is_no_channel_at_all() {
        for exe in [
            "/usr/local/bin/perch",
            "/usr/bin/perch",
            "/home/someone/bin/perch",
            "/home/someone/.local/lib/perch",
            "/opt/perch/perch",
        ] {
            assert_eq!(channel_of(exe), None, "{exe}");
        }
    }

    #[test]
    fn the_installers_directory_is_the_one_that_installer_would_have_used() {
        use crate::host::FakeHost;

        // Compared as text rather than as a `PathBuf`, which is the whole point:
        // a `PathBuf` comparison is separator-agnostic on Windows, and so says
        // nothing about the spelling this is here to hold still.
        let spelling = |host: &FakeHost| installer_dir(host).expect("a home").display().to_string();

        assert_eq!(spelling(&FakeHost::new()), "/Users/someone/.local/bin");

        let windows = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("LOCALAPPDATA", "C:\\Users\\someone\\AppData\\Local");
        assert_eq!(
            spelling(&windows),
            "C:\\Users\\someone\\AppData\\Local/Perch/bin",
            "the part Perch adds is spelled the same on every build; the part \
             Windows handed it is left as Windows spelled it, and `segments` \
             reads both"
        );

        // Derived from home only where the machine will not say. Set-but-empty
        // is the machine not saying: taken at face value it gives `/Perch/bin`,
        // which no installer wrote and `channel_at` recognizes as no Channel.
        for quiet in [
            FakeHost::new().with_platform(Platform::Windows),
            FakeHost::new()
                .with_platform(Platform::Windows)
                .with_env("LOCALAPPDATA", ""),
        ] {
            assert_eq!(spelling(&quiet), "/Users/someone/AppData/Local/Perch/bin");
        }

        for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
            let chosen = FakeHost::new()
                .with_platform(platform)
                .with_env("PERCH_INSTALL_DIR", "/opt/mine");
            assert_eq!(
                installer_dir(&chosen).expect("a home"),
                PathBuf::from("/opt/mine"),
                "{platform:?} — the installers take it above their own default"
            );

            // Set to nothing is the machine not saying, the same as `LOCALAPPDATA`
            // above: taken at face value the comparison is against `[]`.
            let quiet = FakeHost::new()
                .with_platform(platform)
                .with_env("PERCH_INSTALL_DIR", "");
            assert_ne!(
                installer_dir(&quiet).expect("a home"),
                PathBuf::new(),
                "{platform:?} — an empty override is no override"
            );
        }
    }

    #[test]
    fn a_windows_path_is_read_the_way_windows_reads_it() {
        let installer = Path::new("C:\\Users\\someone\\AppData\\Local\\Perch\\bin");

        for exe in [
            "C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe",
            "\\\\?\\C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe",
            "c:\\users\\someone\\appdata\\local\\perch\\bin\\PERCH.EXE",
        ] {
            assert_eq!(
                channel_at(&machine(Platform::Windows), installer, Path::new(exe)),
                Some(Channel::Installer),
                "{exe}"
            );
        }

        assert_eq!(
            channel_at(
                &machine(Platform::Windows),
                installer,
                Path::new("C:\\Program Files\\perch\\perch.exe")
            ),
            None,
            "and somewhere else is still somewhere else"
        );
    }

    /// `current_exe` canonicalizes — it has to, or a Homebrew binary is a
    /// `<prefix>/bin/perch` saying nothing about a Cellar — and `home_dir` reads
    /// `$HOME` verbatim. Compared as spellings, a link above `bin` says a binary
    /// the installer placed was placed by hand, and `perch upgrade` refuses it
    /// naming both paths as though they were two places.
    #[test]
    fn the_installers_directory_reached_through_a_link_is_still_the_installers() {
        let host = machine(Platform::Other).with_link(
            crate::host::Link::Symbolic,
            "/export/home/someone",
            "/home/someone",
        );

        assert_eq!(
            channel_at(
                &host,
                Path::new(INSTALLER_DIR),
                Path::new("/export/home/someone/.local/bin/perch"),
            ),
            Some(Channel::Installer),
            "one directory under the two names the two Host answers spell it"
        );
        assert_eq!(
            channel_at(
                &host,
                Path::new(INSTALLER_DIR),
                Path::new("/export/home/someone/elsewhere/perch"),
            ),
            None,
            "and somewhere else under the link is still somewhere else"
        );
    }

    #[test]
    fn case_is_not_folded_where_the_filesystem_does_not_fold_it() {
        assert_eq!(channel_of("/home/someone/.local/BIN/perch"), None);
    }

    #[test]
    fn a_release_is_taken_with_or_without_the_v() {
        assert_eq!(version_typed("0.2.0").expect("bare"), "0.2.0");
        assert_eq!(version_typed("v0.2.0").expect("tagged"), "0.2.0");
        assert_eq!(version_typed("v0.2.0-rc.1").expect("pre"), "0.2.0-rc.1");
        assert_eq!(
            version_typed("0.2.0+build.3").expect("build"),
            "0.2.0+build.3"
        );
        assert_eq!(tag_of("0.2.0"), "v0.2.0");
    }

    #[test]
    fn something_that_is_not_a_release_is_refused_before_the_network() {
        for typed in [
            "latest",
            "newest",
            "0.2",
            "v",
            "",
            "banana",
            "0.x.0",
            // Everything after the patch component's first digit, which is what
            // reaches `PERCH_VERSION` and then the installer's download URL.
            "0.2.0/../../whatever",
            "0.2.0 && echo",
            "0.2.0\nv0.3.0",
            "0.2.0;rm",
            "0.2.0 ",
        ] {
            let refused = version_typed(typed)
                .err()
                .unwrap_or_else(|| panic!("{typed} is not a Release"));
            assert!(
                matches!(refused, PerchError::Invalid(_)),
                "{typed}: {refused}"
            );
        }
    }

    #[test]
    fn releases_are_ordered_by_number_rather_than_by_spelling() {
        assert_eq!(compare("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare("1.0.0", "0.99.99"), Ordering::Greater);
        assert_eq!(compare("0.2.0", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("0.1.0", "0.2.0"), Ordering::Less);
    }

    #[test]
    fn a_pre_release_comes_before_the_release_it_is_a_run_up_to() {
        assert_eq!(compare("0.2.0-rc.1", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.2.0", "0.2.0-rc.1"), Ordering::Greater);
        assert_eq!(compare("0.2.0-rc.2", "0.2.0-rc.1"), Ordering::Greater);
    }

    #[test]
    fn a_run_up_is_ordered_by_its_number_rather_than_by_its_spelling() {
        assert_eq!(compare("0.2.0-rc.10", "0.2.0-rc.9"), Ordering::Greater);
        assert_eq!(compare("0.2.0-rc.9", "0.2.0-rc.10"), Ordering::Less);
        assert_eq!(compare("0.2.0-rc.10", "0.2.0-rc.10"), Ordering::Equal);
        // Semver's other two rules about a suffix, from the same walk.
        assert_eq!(compare("0.2.0-alpha", "0.2.0-1"), Ordering::Greater);
        assert_eq!(compare("0.2.0-1", "0.2.0-alpha"), Ordering::Less);
        assert_eq!(compare("0.2.0-rc.1.1", "0.2.0-rc.1"), Ordering::Greater);
    }

    /// `version_typed` accepts the spelling deliberately, so this is reachable
    /// by typing rather than hypothetical.
    #[test]
    fn build_metadata_does_not_decide_which_release_is_newer() {
        assert_eq!(compare("0.2.0+build.3", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("0.2.0", "0.2.0+build.3"), Ordering::Equal);
        assert_eq!(compare("0.2.0+build.3", "0.2.0-rc.1"), Ordering::Greater);
        assert_eq!(compare("0.2.0+a", "0.2.0+b"), Ordering::Equal);
        assert_eq!(compare("0.3.0+build.3", "0.2.0"), Ordering::Greater);
    }

    #[test]
    fn a_number_too_big_to_read_is_the_newer_release_rather_than_the_oldest_possible() {
        let enormous = "99999999999999999999.0.0";
        assert_eq!(compare(enormous, "0.2.0"), Ordering::Greater);
        assert_eq!(compare("0.2.0", enormous), Ordering::Less);
    }
}
