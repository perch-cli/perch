//! What an Upgrade is: replacing this machine's Installation with a newer
//! Release, through the Channel that made it (ADR 0039).
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
/// ask, which is the reason ADR 0031 refused to mark a Release as a prerelease:
/// doing so would empty this.
pub const LATEST_URL: &str = "https://api.github.com/repos/perch-cli/perch/releases/latest";

/// How long the check on `perch --version` may take before it is abandoned.
///
/// Short because nobody asked for it. `perch --version` was instant and silent
/// on the network before this existed, and the line it now prints is worth a
/// pause somebody would not notice — not a wait they would (ADR 0039).
pub const CHECK_WITHIN_MILLIS: u64 = 2_000;

/// The variable that switches the check on `perch --version` off.
pub const NO_CHECK: &str = "PERCH_NO_UPGRADE_CHECK";

/// What is installed, which is what the build says it is.
pub fn installed() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// One route by which this machine's Installation arrived, as far as its own
/// path can say.
///
/// Three rather than four: the Release page is a Channel and leaves an
/// Installation indistinguishable from a hand-placed binary, so it arrives here
/// as no answer at all rather than as a fourth variant that would have to be
/// guessed at.
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
/// Three answers rather than one, because the installers have three: they take
/// `PERCH_INSTALL_DIR` above everything, and their defaults do not agree
/// across platforms. A single hard-coded `~/.local/bin` would have read every
/// Windows Installation as one Perch did not make, and refused to upgrade the
/// Channel it is the only one able to upgrade.
pub fn installer_dir(host: &dyn Host) -> Result<PathBuf> {
    if let Some(chosen) = host.env_var("PERCH_INSTALL_DIR") {
        return Ok(PathBuf::from(chosen));
    }

    let home = host
        .home_dir()
        .map_err(|err| PerchError::Other(format!("could not find your home directory: {err}")))?;

    let home = home.display().to_string();

    Ok(match host.platform() {
        // `%LOCALAPPDATA%` is its own variable rather than a place under the
        // home directory, so it is asked for first — and derived from home only
        // where the machine will not say, which is where the installer's own
        // `Join-Path $env:LOCALAPPDATA` would have failed anyway.
        Platform::Windows => {
            let local = host
                .env_var("LOCALAPPDATA")
                .unwrap_or_else(|| [home.as_str(), "AppData", "Local"].join("/"));
            beneath(&[&local, "Perch", "bin"])
        }
        _ => beneath(&[&home, ".local", "bin"]),
    })
}

/// A path built from its parts, spelled with `/`.
///
/// Rather than `Path::join`, for the reason [`crate::probe::on_path`] gives
/// about the same choice: `join` picks the separator of whatever platform this
/// build runs on, where every other decision in this module follows the
/// platform the *Host* reports. The two disagree in exactly one place that
/// matters — a Windows build driving a Host that says it is anything else, and
/// the reverse — and there the directory Perch computed and the directory it
/// compared against were spelled differently and never matched, so no
/// Installation was ever the installer's.
///
/// Windows accepts either separator and [`segments`] reads both, so this costs
/// nothing on the platform it looks foreign on.
fn beneath(parts: &[&str]) -> PathBuf {
    PathBuf::from(parts.join("/"))
}

/// Which Channel left the binary at this path, or `None` when nothing about the
/// path says.
///
/// The path is the whole of the evidence. Every Channel installs the same bytes
/// (ADR 0039), so there is nothing inside the binary to read and no marker file
/// to trust — one placed beside the binary would be absent for Homebrew and npm
/// both, making its absence mean two different things.
pub fn channel_at(platform: Platform, installer_dir: &Path, exe: &Path) -> Option<Channel> {
    let parts = segments(platform, exe);

    // npm first, because an npm prefix can sit anywhere — including under a
    // Homebrew prefix, which is where `brew install node` puts it. A path
    // holding both a Cellar and a `node_modules` is an npm Installation of
    // Perch under a Homebrew installation of Node, and reading it as Homebrew's
    // would route `brew upgrade perch` at a formula that is not installed.
    if parts.iter().any(|part| part == "node_modules") {
        return Some(Channel::Npm);
    }

    // The Cellar names the Channel, and the prefix above it names the `brew`
    // that owns it — `/opt/homebrew` on Apple silicon, `/usr/local` on Intel,
    // and anywhere at all for a prefix somebody chose. Homebrew is not a
    // Windows Channel, so the capital is the one this compares.
    if let Some(at) = parts.iter().position(|part| part == "Cellar") {
        return Some(Channel::Homebrew {
            prefix: PathBuf::from(format!("/{}", parts[..at].join("/"))),
        });
    }

    // Exactly where the installer puts it, and nowhere else. `/usr/local/bin`
    // is deliberately not here: that is where somebody who unpacked the Release
    // page's archive by hand would have put it, and Perch overwriting a binary
    // it did not place is the one irreversible thing this command can do.
    let holding = &parts[..parts.len().saturating_sub(1)];
    match holding == segments(platform, installer_dir) {
        true => Some(Channel::Installer),
        false => None,
    }
}

/// A path as the parts that name it, compared the way the platform compares
/// them.
///
/// Not `Path::components`, for two reasons that only show up on Windows and
/// both of which would make an installer Installation there unrecognizable.
/// `std::fs::canonicalize` hands back a verbatim path — `\\?\C:\Users\...` —
/// which no plain comparison against `%LOCALAPPDATA%` can match; and Windows
/// paths are case-insensitive, so `C:\Users` and `c:\users` are one directory
/// that two `PathBuf`s call different. Neither applies anywhere else, so
/// neither is done anywhere else.
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
    Ok(channel_at(host.platform(), &installer_dir(host)?, &exe))
}

/// A Release as somebody typed it, as the number Perch compares.
///
/// `v0.2.0` and `0.2.0` both arrive, because the tag carries a `v` and every
/// other Channel does not — Homebrew's formula strips it, npm never had it. So
/// both are accepted and one is kept.
///
/// Checked here rather than left to the download, because a typo that reaches
/// the network comes back as a 404 about an archive, which is a sentence about
/// the wrong thing.
pub fn version_typed(typed: &str) -> Result<String> {
    let bare = typed.strip_prefix('v').unwrap_or(typed);
    let plausible = {
        let mut parts = bare.splitn(3, '.');
        let numeric = |part: Option<&str>| {
            part.is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        };
        // The third part carries any pre-release or build suffix —
        // `0.2.0-rc.1`, `0.2.0+build.3` — so it is digits followed by an
        // optional suffix rather than digits alone.
        //
        // The suffix is spelled out rather than left unchecked. Requiring only
        // that the part *start* with a digit accepted every byte after the
        // first: `0.2.0/../../whatever` and `0.2.0 && x` both passed, went into
        // `PERCH_VERSION`, and came back from the installer as the
        // 404-about-an-archive this function exists to turn into a sentence
        // about the thing that is actually wrong.
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

/// Which of two Releases is newer.
///
/// Numeric part by numeric part, so `0.10.0` is newer than `0.9.0` where a
/// string comparison would have it the other way. A pre-release suffix is
/// compared as text after the numbers agree, which puts `0.2.0-rc.1` before
/// `0.2.0` — the ordering semver gives it, arrived at from the other direction:
/// an empty suffix sorts last rather than first.
///
/// Build metadata is dropped before any of that, which is the other half of the
/// same rule: semver says `+build.3` has no bearing on precedence, so `0.2.0`
/// and `0.2.0+build.3` are one Release. Kept, it was compared as a pre-release
/// suffix — and since `+` sorts below `-`, `--release 0.2.0+build.3` against an
/// installed `0.2.0` took the "older" arm and asked "Install the older
/// Release?" about the version that was already there. `version_typed` accepts
/// the spelling deliberately, so it is reachable rather than hypothetical.
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
                // Saturating rather than zero. Every version that reaches here
                // has been through `version_typed`, on both paths — what
                // somebody typed and the `tag_name` `newest` reads — so each
                // component is a run of ASCII digits and the only way this
                // parse fails is a number too big for a `u64`. Read as zero, a
                // `--release 99999999999999999999.0.0` compared as `0.0.0` and
                // was offered as "the older Release", which is the
                // wrong-direction prompt the `+build.3` rule above is about.
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
/// Compared as one string it was the same trap the numbers above are split for:
/// `-rc.10` against `-rc.9` is `'1'` against `'9'`, so `rc.10` came out *older*
/// than `rc.9` and `perch upgrade --release 0.2.0-rc.10` asked "Install the
/// older Release?" about the newer one — and non-interactively refused it
/// outright. `version_typed` accepts the spelling deliberately, so it is
/// reachable by typing rather than hypothetical.
///
/// A number always sorts below a word, which is semver's rule and not an
/// arbitrary one: it is what keeps `1.0.0-alpha` above `1.0.0-1`. And where one
/// suffix runs out first, the longer one is the newer — `rc.1.1` after `rc.1`.
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
/// the machinery for holding an answer and reasoning about its age is the thing
/// ADR 0032 refused and ADR 0039 did not build.
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

/// The line `perch --version` adds when there is a newer Release, and nothing
/// at all otherwise.
///
/// Every reason not to ask is checked before asking, and the ask itself
/// swallows whatever went wrong. This is a line nobody requested printed
/// underneath one they did, so a machine that is offline, has no `curl`, sits
/// behind a proxy, or is being rate-limited loses the line and nothing else
/// (ADR 0039).
pub fn notice(host: &dyn Host) -> Option<String> {
    if host.env_var(NO_CHECK).is_some() {
        return None;
    }
    // Not a terminal is not a person: a script parsing `perch --version` and
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

/// The whole of what `perch --version` says.
///
/// Here rather than in `main`, because `main` is the one place no test reaches:
/// what is printed, whether the second line appears at all, and the fact that
/// the first is spelled exactly as clap spelled it — `perch <version>`, which
/// the Homebrew formula's test block asserts on — are the parts worth holding
/// still, and none of them can be held still inside a function that only runs
/// as a process.
pub fn version_report(host: &dyn Host) -> String {
    let installed = installed();
    match notice(host) {
        Some(line) => format!("perch {installed}\n{line}\n"),
        None => format!("perch {installed}\n"),
    }
}

/// The command that hands the work back to Homebrew, as program and arguments.
///
/// The `brew` beside the Cellar rather than the first one on `PATH`, for the
/// reason ADR 0021 gives about every program Perch runs: a machine with two
/// Homebrew prefixes has two `brew`s, and the one that owns this Installation is
/// the one that can replace it. A Channel named by `--channel` carries no
/// prefix, so that case falls back to the search.
pub fn homebrew_command(host: &dyn Host, prefix: &Path) -> Result<(PathBuf, Vec<String>)> {
    let brew = match prefix.as_os_str().is_empty() {
        true => crate::probe::on_path(host, "brew"),
        false => Some(beneath(&[&prefix.display().to_string(), "bin", "brew"])),
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
    let args = match version {
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
    };
    Ok((npm, args))
}

/// The installer this platform is upgraded by, embedded rather than fetched.
///
/// `include_str!` at build time, because the alternative is downloading a shell
/// script at run time and executing it, which is a sentence a program built
/// around being careful with Credentials should not have to defend (ADR 0039).
/// What the copy is pinned to costs nothing: the only thing passed into it is a
/// tag.
pub fn installer_for(platform: Platform) -> (&'static str, &'static str) {
    match platform {
        Platform::Windows => (
            "perch-upgrade.ps1",
            include_str!("../packaging/pages/install.ps1"),
        ),
        _ => (
            "perch-upgrade.sh",
            include_str!("../packaging/pages/install.sh"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLER_DIR: &str = "/home/someone/.local/bin";

    fn channel_of(exe: &str) -> Option<Channel> {
        channel_at(Platform::Other, Path::new(INSTALLER_DIR), Path::new(exe))
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

    /// A machine with Node installed by Homebrew puts npm's prefix *inside* a
    /// Homebrew one, so the path holds a Cellar and a `node_modules` both.
    /// Reading it as Homebrew would run `brew upgrade perch` against a formula
    /// that is not installed, and report success having changed nothing.
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

    /// The Release page is a Channel too, and what it leaves is a binary
    /// somebody moved into place themselves. There is no telling it from any
    /// other hand-placed binary, so it is not told: overwriting a file at a path
    /// Perch never wrote is the one thing here that cannot be taken back.
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

    /// The installers do not agree on where they put a binary, and both take
    /// `PERCH_INSTALL_DIR` above their own default. A single hard-coded
    /// `~/.local/bin` read every Windows Installation as one Perch had not
    /// made, and refused to upgrade the one Channel it can upgrade itself.
    #[test]
    fn the_installers_directory_is_the_one_that_installer_would_have_used() {
        use crate::host::FakeHost;

        // Compared as text rather than as a `PathBuf`, which is the whole
        // point: a `PathBuf` comparison is separator-agnostic on Windows and so
        // said nothing about *spelling*. This is built with `Path::join` and
        // spelled with the build's separator, while `segments` reads it against
        // the platform the Host reports — and on a Windows build driving a Host
        // that says otherwise the two never matched, so no Installation was
        // ever the installer's and every upgrade was refused.
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

        // Derived from home only where the machine will not say — which is
        // where the installer's own `Join-Path $env:LOCALAPPDATA` would have
        // failed too.
        let quiet = FakeHost::new().with_platform(Platform::Windows);
        assert_eq!(spelling(&quiet), "/Users/someone/AppData/Local/Perch/bin");

        for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
            let chosen = FakeHost::new()
                .with_platform(platform)
                .with_env("PERCH_INSTALL_DIR", "/opt/mine");
            assert_eq!(
                installer_dir(&chosen).expect("a home"),
                PathBuf::from("/opt/mine"),
                "{platform:?} — the installers take it above their own default"
            );
        }
    }

    /// Both of the Windows-only readings, neither of which any other platform
    /// needs: a canonicalized path there is verbatim — `\\?\C:\Users\...` —
    /// and the filesystem does not distinguish case. Either one left alone made
    /// every Windows installer Installation unrecognizable.
    #[test]
    fn a_windows_path_is_read_the_way_windows_reads_it() {
        let installer = Path::new("C:\\Users\\someone\\AppData\\Local\\Perch\\bin");

        for exe in [
            "C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe",
            "\\\\?\\C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe",
            "c:\\users\\someone\\appdata\\local\\perch\\bin\\PERCH.EXE",
        ] {
            assert_eq!(
                channel_at(Platform::Windows, installer, Path::new(exe)),
                Some(Channel::Installer),
                "{exe}"
            );
        }

        assert_eq!(
            channel_at(
                Platform::Windows,
                installer,
                Path::new("C:\\Program Files\\perch\\perch.exe")
            ),
            None,
            "and somewhere else is still somewhere else"
        );
    }

    /// The same folding must not happen off Windows, where two paths differing
    /// in case are two files.
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

    /// Refused here rather than at the download, where the answer is a 404
    /// about an archive nobody asked for by name.
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
            // Everything after the first digit of the patch component used to
            // go unread, so all of these were accepted, put into
            // `PERCH_VERSION` and handed to the installer — which built an
            // archive name and a download URL out of them and came back with
            // the 404 this refusal exists to replace.
            "0.2.0/../../whatever",
            "0.2.0 && echo",
            "0.2.0\nv0.3.0",
            "0.2.0;rm",
            "0.2.0 ",
        ] {
            let refused = version_typed(typed).expect_err("{typed} is not a Release");
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

    /// An unfinished Release comes before the finished one of the same number,
    /// which falls out of an empty suffix sorting last rather than first.
    #[test]
    fn a_pre_release_comes_before_the_release_it_is_a_run_up_to() {
        assert_eq!(compare("0.2.0-rc.1", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.2.0", "0.2.0-rc.1"), Ordering::Greater);
        assert_eq!(compare("0.2.0-rc.2", "0.2.0-rc.1"), Ordering::Greater);
    }

    /// A run-up is numbered, and a number is not spelled: `rc.10` follows
    /// `rc.9`.
    ///
    /// Compared as one string it was `'1'` against `'9'`, so `perch upgrade
    /// --release 0.2.0-rc.10` on an installed `0.2.0-rc.9` asked "Install the
    /// older Release?" about the newer one, and refused it outright where
    /// nobody was there to answer.
    #[test]
    fn a_run_up_is_ordered_by_its_number_rather_than_by_its_spelling() {
        assert_eq!(compare("0.2.0-rc.10", "0.2.0-rc.9"), Ordering::Greater);
        assert_eq!(compare("0.2.0-rc.9", "0.2.0-rc.10"), Ordering::Less);
        assert_eq!(compare("0.2.0-rc.10", "0.2.0-rc.10"), Ordering::Equal);
        // Semver's other two rules about a suffix, which fall out of the same
        // walk: a number sorts below a word, and where one suffix runs out
        // first the longer one is the newer.
        assert_eq!(compare("0.2.0-alpha", "0.2.0-1"), Ordering::Greater);
        assert_eq!(compare("0.2.0-1", "0.2.0-alpha"), Ordering::Less);
        assert_eq!(compare("0.2.0-rc.1.1", "0.2.0-rc.1"), Ordering::Greater);
    }

    /// Build metadata has no bearing on precedence, which semver states and
    /// `version_typed` deliberately accepts the spelling of.
    ///
    /// Compared as a suffix it was worse than ignored: `+` sorts below `-`, so
    /// `0.2.0+build.3` came out below both `0.2.0` and `0.2.0-rc.1`, and
    /// `perch upgrade --release 0.2.0+build.3` on an installed `0.2.0` asked
    /// "Install the older Release?" about the version already there.
    #[test]
    fn build_metadata_does_not_decide_which_release_is_newer() {
        assert_eq!(compare("0.2.0+build.3", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("0.2.0", "0.2.0+build.3"), Ordering::Equal);
        assert_eq!(compare("0.2.0+build.3", "0.2.0-rc.1"), Ordering::Greater);
        assert_eq!(compare("0.2.0+a", "0.2.0+b"), Ordering::Equal);
        assert_eq!(compare("0.3.0+build.3", "0.2.0"), Ordering::Greater);
    }

    /// A component too big for a `u64` is bigger than any Release, not smaller
    /// than all of them.
    ///
    /// `version_typed` accepts any run of ASCII digits, so this is reachable by
    /// typing rather than hypothetical — and read as zero it compared as
    /// `0.0.0` and was offered as "the older Release", the wrong-direction
    /// prompt the build-metadata rule above is about.
    #[test]
    fn a_number_too_big_to_read_is_the_newer_release_rather_than_the_oldest_possible() {
        let enormous = "99999999999999999999.0.0";
        assert_eq!(compare(enormous, "0.2.0"), Ordering::Greater);
        assert_eq!(compare("0.2.0", enormous), Ordering::Less);
    }
}
