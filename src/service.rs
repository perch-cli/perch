//! What a Service *is*: where its unit goes, what the unit says, and which
//! commands drive the machine's own service manager (ADR 0040).
//!
//! A Service is the Watcher, run for you by the machine's service manager,
//! started when you log in. Perch never backgrounds itself — it writes a unit
//! and hands the job over, which is the same "scheduling is the operating
//! system's job" ADR 0013 settled, with the one thing added that ADR 0013
//! refused: a unit file Perch will write and take back.
//!
//! Nothing here reaches the filesystem or spawns anything. What a Service *does*
//! is [`crate::commands::service`]'s; what one *says* is here, where three
//! platforms' worth of quoting can be argued with in a unit test.
//!
//! # The three arrangements
//!
//! | Platform | Unit | Started by |
//! |---|---|---|
//! | macOS | a LaunchAgent plist in `~/Library/LaunchAgents` | `launchctl bootstrap gui/<uid>` |
//! | Linux | a `systemd --user` unit in `~/.config/systemd/user` | `systemctl --user enable --now` |
//! | Windows | a Scheduled Task registered on logon | `schtasks /Create /SC ONLOGON` |
//!
//! All three are **per-user and start at login**, never at boot. Every Profile
//! Perch holds is under one person's home directory, and on macOS there is no
//! unlocked keychain before somebody logs in (ADR 0008) — so a system-wide
//! arrangement would be a Watcher with nothing it could read.
//!
//! [`Platform::Other`] is read as Linux throughout, which is a claim worth
//! making out loud rather than leaving to a `_` arm. Perch builds for five
//! Targets and two of them are `-unknown-linux-musl` (ADR 0029), so the set of
//! machines that are neither macOS nor Windows and are running a Perch is
//! exactly the set running Linux. The rest of the codebase already reads it that
//! way — `commands::a_store_that_held_nothing` decides a Credential Store on
//! macOS-or-not — and this module says `systemd` where that one says a file. A
//! sixth Target on a platform with a different service manager is the thing that
//! would make this wrong, and it would make [`Platform`] wrong first.

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{Host, Platform};
use crate::upgrade::Channel;

/// What the unit is called, on every platform that wants a name.
///
/// Reverse-DNS because launchd requires it of a Label and uses it as the
/// identity `bootout` is given; the other two take the same string in their own
/// shape so that one grep over a machine finds all three.
pub const LABEL: &str = "cli.perch.watch";

/// What the Windows task is called. A folder rather than a bare name, so
/// `schtasks /Query /TN Perch` lists what Perch put there and nothing else.
pub const TASK_NAME: &str = r"Perch\Watch";

/// What the systemd unit is called, which is also its file name.
pub const UNIT_NAME: &str = "perch-watch.service";

/// How long a service manager waits for a stopping Watcher to finish, in
/// seconds.
///
/// Pinned rather than inherited, and the same number on every platform. systemd
/// allows ninety seconds by default and launchd twenty, which would have given
/// one machine four times another's room to finish a Switch that was already
/// under way. What has to fit inside it is a Capture, a Credential write and an
/// Identity patch, each under Claude Code's locks — seconds, not tens of them —
/// so thirty is generous everywhere without leaving a wedged Watcher sitting
/// there for a minute and a half.
pub const STOP_GRACE_SECONDS: u32 = 30;

/// How long a supervisor leaves it before starting the Watcher again, in
/// seconds.
///
/// It should almost never be used: the Watcher holds rather than exiting (ADR
/// 0040), so the ordinary reasons a loop used to stop no longer stop it. What is
/// left is a genuine failure — a Switch that moved and then failed — and coming
/// straight back at a machine that is part way through one would be the
/// supervisor deciding what to do next about something nobody has looked at.
pub const RESTART_SECONDS: u32 = 30;

/// How many failed starts inside [`GIVE_UP_AFTER_SECONDS`] before systemd stops
/// trying and leaves the unit failed.
///
/// A Watcher holds rather than exiting on everything it can hold on (ADR 0040),
/// so a Service that will not *start* is a machine somebody has to look at: a
/// registry that will not parse, a Claude Code that cannot be probed, a home
/// directory that has gone. Restarting into that for ever is a loop nobody ever
/// sees, because the only place it is visible is a log nobody is reading.
///
/// systemd's own defaults would never trip here — five starts inside ten
/// seconds, against a `RestartSec` of thirty — so the window is set from the
/// restart interval rather than left to be inherited. Five tries spans two
/// minutes; five minutes of window catches them with room to spare, and
/// `systemctl --user status perch-watch` then says `failed` and why.
///
/// launchd has no equivalent and gets `ThrottleInterval` alone, which bounds how
/// fast it retries but not how long. That is a real difference between the two
/// platforms rather than something worth emulating with a wrapper.
pub const GIVE_UP_AFTER: u32 = 5;

/// The window those failures have to fall inside, in seconds.
pub const GIVE_UP_AFTER_SECONDS: u32 = 300;

/// The relationship between the three that makes the limit reachable at all.
///
/// A compile-time assertion rather than a test, because it is a fact about three
/// constants and nothing runs to discover it: a window narrower than the tries
/// it is counting is a limit systemd can never reach, which is the *default*
/// this exists to override and would be a silent return to restarting for ever.
/// Changing `RESTART_SECONDS` without changing this fails the build, which is
/// the whole point of putting it here.
const _: () = assert!(
    GIVE_UP_AFTER_SECONDS > RESTART_SECONDS * GIVE_UP_AFTER,
    "the start-limit window has to be wider than the restarts it counts"
);

/// The environment a unit carries over from the shell that installed it.
///
/// Both are read from the process environment and are typically set in a shell
/// profile that no service manager will ever source (ADR 0040). A Service
/// silently watching `~/.config/perch` while its owner works out of
/// `PERCH_HOME=~/work/perch` would be reporting, correctly and uselessly, that
/// there is nothing to do.
///
/// Named rather than "everything that is set", which is the same bargain
/// [`crate::carry`] makes about `.claude.json`: a unit that captured the whole
/// environment would bake a `PATH`, an `SSH_AUTH_SOCK` and whatever secret the
/// shell happened to be holding into a file on disk.
pub const CARRIED: [&str; 2] = ["PERCH_HOME", "CLAUDE_CONFIG_DIR"];

/// Where the decision log goes on a platform whose service manager will not
/// keep one.
///
/// Inside Perch's home, which is what makes it Perch's to remove: a Purge sweeps
/// it with everything else Perch holds (ADR 0040). On Linux there is no such
/// file — systemd captures standard output into the journal, which rotates it,
/// retains it and answers `journalctl --user -u perch-watch -f` without Perch
/// knowing the word journal.
pub fn log_path(host: &dyn Host) -> Result<Option<PathBuf>> {
    match host.platform() {
        Platform::Other => Ok(None),
        _ => Ok(Some(crate::registry::perch_home(host)?.join("watch.log"))),
    }
}

/// Where the unit file lives.
pub fn unit_path(host: &dyn Host) -> Result<Option<PathBuf>> {
    let home = host
        .home_dir()
        .map_err(|err| PerchError::Other(format!("could not find your home directory: {err}")))?;
    Ok(match host.platform() {
        Platform::MacOs => Some(
            home.join("Library")
                .join("LaunchAgents")
                .join(format!("{LABEL}.plist")),
        ),
        Platform::Other => Some(
            home.join(".config")
                .join("systemd")
                .join("user")
                .join(UNIT_NAME),
        ),
        // A Scheduled Task is registered rather than written: it lives in
        // Windows' own store, and `schtasks` is the whole of the interface to
        // it. There is no file for Perch to put anywhere, which is why every
        // caller has to answer `None` rather than assuming a path.
        Platform::Windows => None,
    })
}

/// The binary a unit should name, which is not always the one that is running.
///
/// A unit names an absolute path and is read months later by a service manager
/// that will not search a `PATH`. Two Channels make that path a question rather
/// than a lookup (ADR 0039, ADR 0040):
///
/// **Homebrew**: `current_exe` resolves symlinks, so the running binary is
/// `…/Cellar/perch/0.2.0/bin/perch` — **version-stamped**, and gone the next
/// time `brew upgrade` runs. The symlink in the prefix's `bin` is the stable
/// name across every Release, so that is what the unit gets. This is the one
/// place where resolving the path is exactly the wrong thing to do.
///
/// **npm**: the opposite, and it needs no work. What is on `PATH` is a
/// JavaScript shim that execs the platform package, and a unit naming it would
/// need `node` on a `PATH` no `systemd --user` unit has — but `current_exe` is
/// already the platform binary the shim exec'd, so the running path is the right
/// one and resolving is what got us there.
///
/// Everything else — the installer, an unpacked archive, a `cargo build` — is
/// wherever it is, and wherever it is, is what the unit says.
pub fn binary_for_the_unit(exe: &Path, channel: Option<&Channel>) -> PathBuf {
    match channel {
        Some(Channel::Homebrew { prefix }) => prefix.join("bin").join("perch"),
        _ => exe.to_path_buf(),
    }
}

/// What a unit is being written from: everything that has to be true of it
/// months after the command that wrote it has been forgotten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The binary the service manager will run, absolute and stable.
    pub binary: PathBuf,
    /// The environment carried over from the shell that installed it, in
    /// [`CARRIED`] order and only where actually set.
    pub environment: Vec<(String, String)>,
    /// Where standard output goes, or `None` where the service manager keeps it
    /// (which is Linux, and the journal).
    pub log: Option<PathBuf>,
    /// Which session a LaunchAgent is bootstrapped into.
    pub user_id: Option<u32>,
}

impl Unit {
    /// The unit file's text, in the format the platform's service manager reads.
    ///
    /// `None` on Windows, where there is no file: a Scheduled Task is registered
    /// through `schtasks` and Windows keeps it.
    pub fn rendered(&self, platform: Platform) -> Option<String> {
        match platform {
            Platform::MacOs => Some(self.plist()),
            Platform::Other => Some(self.systemd_unit()),
            Platform::Windows => None,
        }
    }

    /// A LaunchAgent, which launchd reads as a property list.
    ///
    /// `KeepAlive` rather than `RunAtLoad` alone, because the point of a Service
    /// is that it is still there tomorrow: `RunAtLoad` starts it once, and
    /// `KeepAlive` is what starts it again. The Watcher holds rather than
    /// exiting on everything that is not a genuine failure (ADR 0040), so this
    /// is not the respawn-forever loop it would have been against ADR 0013's
    /// watcher.
    fn plist(&self) -> String {
        let environment = match self.environment.is_empty() {
            true => String::new(),
            false => format!(
                "\n\t<key>EnvironmentVariables</key>\n\t<dict>\n{}\t</dict>",
                self.environment
                    .iter()
                    .map(|(key, value)| format!(
                        "\t\t<key>{}</key>\n\t\t<string>{}</string>\n",
                        xml_escaped(key),
                        xml_escaped(value)
                    ))
                    .collect::<String>()
            ),
        };
        // Both streams to one file, and deliberately: the decision log is the
        // evidence the policy works, and a refusal that landed in a second file
        // would be the one line missing from the sequence somebody is reading.
        let log = match &self.log {
            Some(path) => format!(
                "\n\t<key>StandardOutPath</key>\n\t<string>{path}</string>\n\
                 \t<key>StandardErrorPath</key>\n\t<string>{path}</string>",
                path = xml_escaped(&path.to_string_lossy()),
            ),
            None => String::new(),
        };

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{binary}</string>
		<string>watcher</string>
		<string>run</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>ThrottleInterval</key>
	<integer>{restart}</integer>
	<key>ExitTimeOut</key>
	<integer>{grace}</integer>{environment}{log}
</dict>
</plist>
"#,
            label = LABEL,
            binary = xml_escaped(&self.binary.to_string_lossy()),
            restart = RESTART_SECONDS,
            grace = STOP_GRACE_SECONDS,
        )
    }

    /// A `systemd --user` unit.
    ///
    /// `Type=simple` because the Watcher is the process: it does not fork, does
    /// not write a pid file, and does not background itself — which is the whole
    /// of ADR 0040's arrangement, said in one word.
    ///
    /// Standard output is left alone, so systemd captures it into the journal.
    /// That is the one platform where ADR 0013's "the decision log goes to
    /// standard output" needs nothing added to it at all.
    fn systemd_unit(&self) -> String {
        let environment = self
            .environment
            .iter()
            .map(|(key, value)| format!("Environment=\"{key}={}\"\n", value.replace('"', "\\\"")))
            .collect::<String>();

        format!(
            "[Unit]\n\
             Description=Perch — Cycles the Claude account you are on when it runs low\n\
             Documentation=https://github.com/perch-cli/perch\n\
             StartLimitIntervalSec={window}\n\
             StartLimitBurst={tries}\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={binary} watcher run\n\
             Restart=always\n\
             RestartSec={restart}\n\
             TimeoutStopSec={grace}\n\
             {environment}\n\
             [Install]\n\
             WantedBy=default.target\n",
            binary = self.binary.display(),
            restart = RESTART_SECONDS,
            grace = STOP_GRACE_SECONDS,
            window = GIVE_UP_AFTER_SECONDS,
            tries = GIVE_UP_AFTER,
        )
    }

    /// What a Windows Scheduled Task is told to run.
    ///
    /// Wrapped in a `cmd /c` for two things `schtasks` cannot express on its
    /// own: the environment, which a task has no field for at all, and the
    /// redirection, which is the only way a task's standard output is kept
    /// anywhere.
    ///
    /// Appending rather than truncating, so a log survives a logout.
    pub fn windows_command(&self) -> String {
        let environment = self
            .environment
            .iter()
            .map(|(key, value)| format!("set \"{key}={value}\" && "))
            .collect::<String>();
        let log = match &self.log {
            Some(path) => format!(" >> \"{}\" 2>&1", path.display()),
            None => String::new(),
        };
        format!(
            "cmd /c {environment}\"{binary}\" watcher run{log}",
            binary = self.binary.display(),
        )
    }
}

/// One command run against the machine's service manager, as the program and
/// the arguments to give it.
///
/// A type rather than a tuple, because these are the calls that make a machine
/// start running something at every login, and every one of them is asserted
/// against in a test that reads like the shell line it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driven {
    pub program: String,
    pub args: Vec<String>,
    /// Whether the whole act fails when this one does.
    ///
    /// The tidying-up steps are not. `systemctl --user disable` on a unit that
    /// was never enabled, and `launchctl bootout` on one that is not loaded,
    /// both fail — and both mean "it is already not running", which is what an
    /// `uninstall` was asked for. A step that must work says so.
    pub required: bool,
}

impl Driven {
    fn must(program: &str, args: &[&str]) -> Driven {
        Driven {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            required: true,
        }
    }

    fn may_fail(program: &str, args: &[&str]) -> Driven {
        Driven {
            required: false,
            ..Driven::must(program, args)
        }
    }

    /// The command as somebody would have typed it, for the line that says what
    /// failed.
    pub fn as_typed(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What has to be run to start the Service, after the unit is in place.
pub fn starting(platform: Platform, unit: &Unit, at: Option<&Path>) -> Vec<Driven> {
    match platform {
        // Bootstrapped into the logged-in session rather than `load`ed, which
        // has been the deprecated spelling since 10.10. `bootout` first, because
        // bootstrapping over something already loaded fails — and it is allowed
        // to fail, because "not loaded" is the ordinary case on a first install.
        Platform::MacOs => {
            let domain = format!("gui/{}", unit.user_id.unwrap_or_default());
            let path = at
                .map(|at| at.to_string_lossy().to_string())
                .unwrap_or_default();
            vec![
                Driven::may_fail("launchctl", &["bootout", &format!("{domain}/{LABEL}")]),
                Driven::must("launchctl", &["bootstrap", &domain, &path]),
            ]
        }
        // `daemon-reload` before `enable`, because systemd will not have read a
        // unit file that did not exist when it last looked. `--now` is what
        // makes an install something that has already happened rather than
        // something that happens next time you log in.
        Platform::Other => vec![
            Driven::must("systemctl", &["--user", "daemon-reload"]),
            Driven::must("systemctl", &["--user", "enable", "--now", UNIT_NAME]),
        ],
        // `/NP` is what keeps a console window off the desktop at every login:
        // it registers the task to run without a stored password, which Windows
        // runs non-interactively. `/RU` names the user whose home the Profiles
        // are under, and `/F` is what makes a re-install a replacement rather
        // than a collision.
        Platform::Windows => vec![Driven::must(
            "schtasks",
            &[
                "/Create",
                "/TN",
                TASK_NAME,
                "/SC",
                "ONLOGON",
                "/RU",
                "%USERNAME%",
                "/NP",
                "/TR",
                &unit.windows_command(),
                "/F",
            ],
        )],
    }
}

/// What has to be run to stop the Service and take it back.
///
/// Every step may fail, and that is the shape of an `uninstall` rather than an
/// oversight: each one is "make sure this is not running", and a machine where
/// it is already not running is a machine where the command has nothing to do.
/// What decides whether an `uninstall` succeeded is whether the unit is gone
/// afterwards, which the caller asks separately.
pub fn stopping(platform: Platform, user_id: Option<u32>) -> Vec<Driven> {
    match platform {
        Platform::MacOs => vec![Driven::may_fail(
            "launchctl",
            &[
                "bootout",
                &format!("gui/{}/{LABEL}", user_id.unwrap_or_default()),
            ],
        )],
        Platform::Other => vec![
            Driven::may_fail("systemctl", &["--user", "disable", "--now", UNIT_NAME]),
            Driven::may_fail("systemctl", &["--user", "daemon-reload"]),
        ],
        Platform::Windows => vec![Driven::may_fail(
            "schtasks",
            &["/Delete", "/TN", TASK_NAME, "/F"],
        )],
    }
}

/// What has to be run to ask whether the Service is running right now.
///
/// `None` where the question is answered by the unit file being there, which is
/// every platform Perch does not support a service manager on.
pub fn asking(platform: Platform, user_id: Option<u32>) -> Option<Driven> {
    // Every platform has one, so this is `Option` for the caller's sake rather
    // than for a platform's: `status` asks before it knows whether anything is
    // installed, and an answer of "nothing to run" is a shape it has to handle
    // either way.
    match platform {
        Platform::MacOs => Some(Driven::may_fail(
            "launchctl",
            &[
                "print",
                &format!("gui/{}/{LABEL}", user_id.unwrap_or_default()),
            ],
        )),
        Platform::Other => Some(Driven::may_fail(
            "systemctl",
            &["--user", "is-active", UNIT_NAME],
        )),
        Platform::Windows => Some(Driven::may_fail("schtasks", &["/Query", "/TN", TASK_NAME])),
    }
}

/// What to call the arrangement when saying it out loud, in the platform's own
/// word.
///
/// "Service" is the domain term and is what the command is called, but the
/// sentence telling somebody where to look has to use the word their machine
/// uses — `journalctl` will not help a mac and `launchctl` means nothing on
/// Linux.
pub fn described(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "a LaunchAgent",
        Platform::Other => "a systemd user unit",
        Platform::Windows => "a Scheduled Task that runs at logon",
    }
}

/// Where somebody reads the decision log on this platform, as the line that
/// tells them.
pub fn log_is_at(platform: Platform, log: Option<&Path>) -> String {
    match (platform, log) {
        (Platform::Other, _) => "journalctl --user -u perch-watch -f".to_string(),
        (_, Some(path)) => format!("{}", path.display()),
        (_, None) => "nowhere — this platform keeps no log".to_string(),
    }
}

/// The five characters a property list cannot carry raw.
///
/// A plist is XML, and a home directory really can hold an `&`. Escaped rather
/// than refused, because what is being escaped is somebody's own path and the
/// refusal would be about a machine they cannot change.
fn xml_escaped(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The binary a unit names, read back out of the text of one.
///
/// Beside the two functions that wrote it, because reading a unit and writing
/// one are the same format twice and the reader is the only thing that can
/// establish the writer was right. Apart, the escaping had a test and the
/// unescaping had none, and no test anywhere could put a path in one end and
/// take it out of the other.
///
/// The text rather than the file: what a unit *says* is here and reaching the
/// filesystem is [`crate::commands::service`]'s, so a path with an `&` in it
/// can be argued with in a unit test instead of on somebody's machine.
///
/// `None` for a unit Perch does not recognize — one somebody has edited into
/// another shape — which it declines to make claims about rather than guessing
/// at. Windows is `None` always, and never reaches here: it keeps no file.
pub fn binary_in(platform: Platform, unit: &str) -> Option<PathBuf> {
    match platform {
        Platform::Other => unit
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .and_then(|line| line.strip_suffix(" watcher run"))
            .map(PathBuf::from),
        // The first `<string>` inside `ProgramArguments`, which is the program.
        Platform::MacOs => {
            let array = unit.split("<key>ProgramArguments</key>").nth(1)?;
            let opened = array.find("<string>")? + "<string>".len();
            let closed = array[opened..].find("</string>")? + opened;
            Some(PathBuf::from(unescaped(&array[opened..closed])))
        }
        Platform::Windows => None,
    }
}

/// The inverse of [`xml_escaped`], for reading a path back out of a plist.
fn unescaped(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        // Last, so that an `&amp;lt;` in somebody's path survives the round trip
        // rather than becoming a `<` on the way back.
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_unit() -> Unit {
        Unit {
            binary: PathBuf::from("/usr/local/bin/perch"),
            environment: Vec::new(),
            log: None,
            user_id: Some(501),
        }
    }

    /// A path written into a unit is the path read back out of it, on both
    /// platforms that keep one.
    ///
    /// The pair is what `perch service status` rests on: it asks whether the
    /// installed unit and the machine have come apart, and a reader that
    /// disagreed with the writer would answer that question wrongly in the one
    /// direction nobody would check — reporting drift on a machine where
    /// nothing had moved. The `&` is the case that made this worth asserting:
    /// it is the character a home directory really can hold, it is escaped on
    /// the way in, and it is the one the unescaping has to undo last.
    #[test]
    fn a_path_written_into_a_unit_is_the_path_read_back_out_of_it() {
        for path in [
            "/usr/local/bin/perch",
            "/Users/some & one/bin/perch",
            "/Users/o'brien/<perch>/\"bin\"/perch",
            // An escape sequence spelled out in somebody's own path, which
            // comes back as the five characters they typed rather than as the
            // one it names.
            "/Users/someone/&amp;lt;/perch",
        ] {
            for platform in [Platform::MacOs, Platform::Other] {
                let unit = Unit {
                    binary: PathBuf::from(path),
                    ..a_unit()
                };
                let written = unit
                    .rendered(platform)
                    .expect("both of these platforms keep a file");
                assert_eq!(
                    binary_in(platform, &written),
                    Some(PathBuf::from(path)),
                    "{platform:?} did not read back the path it wrote: {written}"
                );
            }
        }
    }

    /// A unit somebody has edited into a shape Perch does not recognize is one
    /// it declines to make a claim about. `status` reads the answer as "cannot
    /// say", which is the honest one — a guess here would report drift against
    /// a binary nobody named.
    #[test]
    fn a_unit_perch_does_not_recognize_is_not_guessed_at() {
        assert_eq!(binary_in(Platform::MacOs, "<plist></plist>"), None);
        assert_eq!(binary_in(Platform::Other, "[Service]\nExecStart=\n"), None);
        assert_eq!(
            binary_in(Platform::Other, "[Service]\nExecStart=/bin/perch serve\n"),
            None,
            "a unit running something other than the Watcher loop names no Perch",
        );
        assert_eq!(
            binary_in(Platform::Windows, &a_unit().plist()),
            None,
            "Windows keeps no file, so there is nothing to have read",
        );
    }

    /// The correction that matters most, and the one a reasonable
    /// implementation gets backwards.
    ///
    /// `current_exe` resolves symlinks, so under Homebrew the running binary is
    /// inside a Cellar directory named after the version installed today. A unit
    /// pointing there works until the next `brew upgrade` and then names a path
    /// that is not there — a Service that silently stops coming up, months after
    /// anybody typed anything.
    #[test]
    fn a_homebrew_unit_names_the_stable_symlink_rather_than_the_versioned_cellar() {
        let named = binary_for_the_unit(
            Path::new("/opt/homebrew/Cellar/perch/0.2.0/bin/perch"),
            Some(&Channel::Homebrew {
                prefix: PathBuf::from("/opt/homebrew"),
            }),
        );

        assert_eq!(named, PathBuf::from("/opt/homebrew/bin/perch"));
        assert!(
            !named.to_string_lossy().contains("0.2.0"),
            "a unit naming a version is a unit that breaks on the next Upgrade"
        );
    }

    /// The mirror, and the reason this is a rule per Channel rather than one
    /// call to `canonicalize` or one refusal to make one.
    ///
    /// npm puts a JavaScript shim on `PATH` and the shim execs the platform
    /// package — so the *running* binary is already the real one, and naming it
    /// is right. Naming what is on `PATH` instead would need `node` on a `PATH`
    /// no `systemd --user` unit has.
    #[test]
    fn an_npm_unit_names_the_platform_binary_the_shim_already_exec_d() {
        let running = Path::new(
            "/home/someone/.nvm/versions/node/v20.11.0/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
        );

        assert_eq!(
            binary_for_the_unit(running, Some(&Channel::Npm)),
            running.to_path_buf(),
            "the shim has already been resolved through by the time this runs"
        );
    }

    #[test]
    fn a_binary_no_channel_claims_is_named_exactly_where_it_is() {
        let running = Path::new("/home/someone/.local/bin/perch");
        assert_eq!(binary_for_the_unit(running, None), running.to_path_buf());
    }

    /// The stop grace is the same on both platforms that have one, because what
    /// has to fit inside it is the same Switch.
    #[test]
    fn both_unit_formats_pin_the_same_stop_grace_rather_than_inheriting_one() {
        let unit = a_unit();
        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");
        let systemd = unit.rendered(Platform::Other).expect("Linux writes a file");

        assert!(
            plist.contains("<key>ExitTimeOut</key>\n\t<integer>30</integer>"),
            "launchd would otherwise allow twenty: {plist}"
        );
        assert!(
            systemd.contains("TimeoutStopSec=30"),
            "systemd would otherwise allow ninety: {systemd}"
        );
    }

    /// A Service that cannot *start* is a machine somebody has to look at, and
    /// restarting into that for ever is a loop nobody ever sees. systemd's own
    /// defaults would never trip it — five starts inside ten seconds against a
    /// `RestartSec` of thirty — so the window is derived from the restart
    /// interval rather than inherited.
    #[test]
    fn a_systemd_unit_gives_up_rather_than_restarting_into_a_failure_for_ever() {
        let systemd = a_unit()
            .rendered(Platform::Other)
            .expect("Linux writes a file");

        assert!(systemd.contains("StartLimitBurst=5"), "{systemd}");
        assert!(systemd.contains("StartLimitIntervalSec=300"), "{systemd}");
    }

    /// The two the unit carries, and nothing else. A unit that captured the
    /// whole environment would bake whatever secret the installing shell was
    /// holding into a file on disk.
    #[test]
    fn only_the_named_environment_is_carried_into_a_unit() {
        let unit = Unit {
            environment: vec![("PERCH_HOME".to_string(), "/home/someone/work".to_string())],
            ..a_unit()
        };

        let systemd = unit.rendered(Platform::Other).expect("Linux writes a file");
        assert!(
            systemd.contains(r#"Environment="PERCH_HOME=/home/someone/work""#),
            "{systemd}"
        );

        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");
        assert!(plist.contains("<key>PERCH_HOME</key>"), "{plist}");
        assert!(
            plist.contains("<string>/home/someone/work</string>"),
            "{plist}"
        );
    }

    /// A unit with nothing to carry says nothing about the environment, rather
    /// than carrying an empty block that a reader would take for a statement.
    #[test]
    fn a_unit_with_nothing_to_carry_carries_no_environment_block() {
        let unit = a_unit();
        assert!(
            !unit
                .rendered(Platform::MacOs)
                .expect("macOS writes a file")
                .contains("EnvironmentVariables"),
        );
        assert!(
            !unit
                .rendered(Platform::Other)
                .expect("Linux writes a file")
                .contains("Environment="),
        );
    }

    /// A plist is XML and a home directory can hold an `&`. Unescaped, the unit
    /// is not a property list at all and launchd refuses the whole thing.
    #[test]
    fn a_path_holding_xml_is_escaped_rather_than_written_raw() {
        let unit = Unit {
            binary: PathBuf::from("/Users/some & one/bin/perch"),
            ..a_unit()
        };
        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");

        assert!(plist.contains("/Users/some &amp; one/bin/perch"), "{plist}");
        assert!(
            !plist.contains("/Users/some & one/bin/perch"),
            "the raw ampersand would make this not a plist: {plist}"
        );
    }

    /// Linux writes no log path at all: standard output goes to the journal,
    /// which is the one platform where ADR 0013's decision needs nothing added.
    #[test]
    fn linux_keeps_no_logfile_because_the_journal_already_does() {
        assert!(
            !a_unit()
                .rendered(Platform::Other)
                .expect("Linux writes a file")
                .contains("StandardOutput"),
        );
        assert_eq!(
            log_is_at(Platform::Other, None),
            "journalctl --user -u perch-watch -f"
        );
    }

    /// macOS keeps one, and both streams go to the same file so that a refusal
    /// and the decision it interrupted read in order.
    #[test]
    fn macos_sends_both_streams_to_one_file_inside_perchs_own_home() {
        let unit = Unit {
            log: Some(PathBuf::from("/Users/someone/.config/perch/watch.log")),
            ..a_unit()
        };
        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");

        assert!(plist.contains("<key>StandardOutPath</key>"), "{plist}");
        assert!(plist.contains("<key>StandardErrorPath</key>"), "{plist}");
        assert_eq!(
            plist
                .matches("/Users/someone/.config/perch/watch.log")
                .count(),
            2,
            "one file, named twice: {plist}"
        );
    }

    /// What every unit format is told to run, asserted on all three at once.
    ///
    /// The one string a Service is for, and the one no platform's own test was
    /// claiming: the plist carries it as a second `<string>`, systemd as the
    /// tail of `ExecStart`, and Windows inside a `cmd /c`. Three spellings of
    /// one fact is how two of them come to name a verb the binary stopped
    /// answering to (ADR 0047), which is a Service that installs cleanly and
    /// then fails at every login.
    #[test]
    fn every_unit_format_runs_the_watcher_loop_by_the_name_the_binary_answers_to() {
        let unit = a_unit();

        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");
        assert!(
            plist.contains("<string>watcher</string>\n\t\t<string>run</string>"),
            "the two words are two arguments, which is what a plist array is: {plist}"
        );

        let systemd = unit.rendered(Platform::Other).expect("Linux writes a file");
        assert!(
            systemd
                .lines()
                .any(|line| line.ends_with(" watcher run") && line.starts_with("ExecStart=")),
            "{systemd}"
        );

        assert!(
            unit.windows_command().contains(r#"" watcher run"#),
            "{}",
            unit.windows_command()
        );
    }

    /// The flag that keeps a console window off somebody's desktop at every
    /// login, which is the whole of why a Scheduled Task is registered this way
    /// rather than as an interactive one.
    #[test]
    fn a_windows_task_is_registered_to_run_without_a_console() {
        let driven = starting(Platform::Windows, &a_unit(), None);
        let line = driven[0].as_typed();

        assert!(line.contains("/SC ONLOGON"), "{line}");
        assert!(
            line.contains("/NP"),
            "without this it runs interactively and shows a window: {line}"
        );
        assert!(
            line.contains("/F"),
            "so that a re-install replaces rather than collides: {line}"
        );
    }

    /// Windows has no field for an environment and no capture of standard
    /// output, so both are folded into the command the task runs.
    #[test]
    fn a_windows_task_carries_its_environment_and_its_log_in_the_command() {
        let unit = Unit {
            binary: PathBuf::from(r"C:\Users\someone\perch.exe"),
            environment: vec![("PERCH_HOME".to_string(), r"C:\work\perch".to_string())],
            log: Some(PathBuf::from(r"C:\work\perch\watch.log")),
            user_id: None,
        };

        let command = unit.windows_command();
        assert!(
            command.contains(r#"set "PERCH_HOME=C:\work\perch""#),
            "{command}"
        );
        assert!(
            command.contains(r#">> "C:\work\perch\watch.log" 2>&1"#),
            "appending rather than truncating, so a log survives a logout: {command}"
        );
    }

    /// A LaunchAgent is bootstrapped into the logged-in session, which is what
    /// makes it start at login and never at boot.
    #[test]
    fn a_launchagent_goes_into_the_logged_in_session_and_boots_out_of_it_first() {
        let unit = a_unit();
        let driven = starting(Platform::MacOs, &unit, Some(Path::new("/tmp/x.plist")));

        assert_eq!(
            driven[0].as_typed(),
            "launchctl bootout gui/501/cli.perch.watch"
        );
        assert!(
            !driven[0].required,
            "nothing is loaded on a first install, and that is not a failure"
        );
        assert_eq!(
            driven[1].as_typed(),
            "launchctl bootstrap gui/501 /tmp/x.plist"
        );
        assert!(driven[1].required, "this one is the install");
    }

    /// Every step of an uninstall may fail, because every one of them is "make
    /// sure this is not running" and a machine where it already is not is one
    /// where there was nothing to do.
    #[test]
    fn nothing_an_uninstall_runs_is_required_to_succeed() {
        for platform in [Platform::MacOs, Platform::Other, Platform::Windows] {
            assert!(
                stopping(platform, Some(501))
                    .iter()
                    .all(|step| !step.required),
                "{platform:?} has a step that would fail an uninstall of \
                 something already uninstalled"
            );
        }
    }

    /// systemd is reloaded before it is asked to enable something, because it
    /// has not read a unit file that did not exist when it last looked.
    #[test]
    fn systemd_is_reloaded_before_it_is_asked_about_a_unit_that_is_new() {
        let driven = starting(Platform::Other, &a_unit(), None);

        assert_eq!(driven[0].as_typed(), "systemctl --user daemon-reload");
        assert_eq!(
            driven[1].as_typed(),
            "systemctl --user enable --now perch-watch.service",
        );
        assert!(
            driven[1].as_typed().contains("--now"),
            "an install that did nothing until the next login reads as broken"
        );
    }

    /// Never a system unit, on any platform. Every Profile is under one
    /// person's home directory, and on macOS there is no unlocked keychain
    /// before somebody logs in.
    #[test]
    fn nothing_is_ever_installed_outside_the_users_own_session() {
        for platform in [Platform::MacOs, Platform::Other, Platform::Windows] {
            for step in starting(platform, &a_unit(), Some(Path::new("/tmp/x")))
                .into_iter()
                .chain(stopping(platform, Some(501)))
            {
                let line = step.as_typed();
                assert!(
                    !line.contains("--system") && !line.contains("system/"),
                    "{platform:?} reaches outside the user's session: {line}"
                );
            }
        }
    }
}
