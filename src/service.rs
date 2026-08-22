//! What a Service *is*: where its unit goes, what the unit says, and which commands
//! drive the machine's own service manager.
//!
//! A LaunchAgent on macOS, a `systemd --user` unit on Linux, a Scheduled Task on
//! Windows, all per-user and started at login (ADR the-machine-runs-the-watcher).
//! Nothing here reaches the filesystem: what a Service *does* is
//! [`crate::commands::service`]'s.
//!
//! [`Platform::Other`] is read as Linux throughout, because two of Perch's five Targets
//! are `-unknown-linux-musl` (ADR a-linux-build-is-static).

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{Host, Platform};
use crate::upgrade::Channel;

/// What the unit is called, on every platform that wants a name.
///
/// Reverse-DNS because launchd requires it of a Label; the other two take the same
/// string in their own shape, so one grep over a machine finds all three.
pub const LABEL: &str = "cli.perch.watch";

/// What the Windows task is called. A folder rather than a bare name, so `schtasks
/// /Query /TN Perch` lists what Perch put there and nothing else.
pub const TASK_NAME: &str = r"Perch\Watch";

/// What the systemd unit is called, which is also its file name.
pub const UNIT_NAME: &str = "perch-watch.service";

/// How long a service manager waits for a stopping Watcher, in seconds.
///
/// Pinned rather than inherited, because systemd allows ninety and launchd twenty. What
/// has to fit inside it is a Capture, a Credential write and an Identity patch under
/// Claude Code's locks.
pub const STOP_GRACE_SECONDS: u32 = 30;

/// How long a supervisor leaves it before starting the Watcher again.
///
/// Rarely reached, because the Watcher holds rather than exiting. What is left is a
/// genuine failure, and coming straight back at a machine part way through a Switch
/// would be deciding about something nobody has seen.
pub const RESTART_SECONDS: u32 = 30;

/// How many failed starts inside [`GIVE_UP_AFTER_SECONDS`] leave the unit failed.
///
/// A Service that will not *start* is a machine somebody has to look at. The window is
/// derived from [`RESTART_SECONDS`], because systemd's own five starts inside ten
/// seconds could never trip against a restart of thirty.
pub const GIVE_UP_AFTER: u32 = 5;

/// The window those failures have to fall inside, in seconds.
pub const GIVE_UP_AFTER_SECONDS: u32 = 300;

/// The relationship between the three that makes the limit reachable at all.
///
/// A compile-time assertion rather than a test, because nothing runs to discover it: a
/// window narrower than the tries it counts is a limit systemd can never reach, which
/// is a silent return to restarting for ever.
const _: () = assert!(
    GIVE_UP_AFTER_SECONDS > RESTART_SECONDS * GIVE_UP_AFTER,
    "the start-limit window has to be wider than the restarts it counts"
);

/// The environment a unit carries over from the shell that installed it.
///
/// Named rather than "everything that is set", which is [`crate::carry`]'s bargain
/// about `.claude.json`: a unit that captured the whole environment would bake whatever
/// secret the installing shell held into a file on disk.
pub const CARRIED: [&str; 2] = ["PERCH_HOME", "CLAUDE_CONFIG_DIR"];

/// Where the decision log goes on a platform whose service manager will not keep one.
///
/// Inside Perch's home, which is what makes it Perch's to remove. Linux has none:
/// systemd captures standard output into the journal.
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
        // A Scheduled Task is registered rather than written: it lives in Windows' own
        // store, so there is no file for Perch to put anywhere and every caller has to
        // answer `None` rather than assuming a path.
        Platform::Windows => None,
    })
}

/// The binary a unit should name, which is not always the running one.
///
/// The most stable path that runs without a shell, per Channel
/// (ADR an-upgrade-asks-its-channel) rather than a `canonicalize`: Homebrew's is a
/// Cellar path gone at the next upgrade, npm's a shim that needs `node`.
pub fn binary_for_the_unit(exe: &Path, channel: Option<&Channel>) -> PathBuf {
    match channel {
        Some(Channel::Homebrew { prefix }) => prefix.join("bin").join("perch"),
        _ => exe.to_path_buf(),
    }
}

/// What a unit is being written from: everything that has to be true of it months after
/// the command that wrote it has been forgotten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The binary the service manager will run, absolute and stable.
    pub binary: PathBuf,
    /// The environment carried over from the shell that installed it, in [`CARRIED`]
    /// order and only where actually set.
    pub environment: Vec<(String, String)>,
    /// Where standard output goes, or `None` where the service manager keeps it (which
    /// is Linux, and the journal).
    pub log: Option<PathBuf>,
    /// Which session a LaunchAgent is bootstrapped into.
    pub user_id: Option<u32>,
    /// Whose Scheduled Task this is, and `None` where the machine will not say.
    ///
    /// Read off the machine rather than left as `%USERNAME%`, because nothing expands
    /// it: `%VAR%` is `cmd.exe`'s notation, and these arguments go to `CreateProcess`
    /// with no shell between (ADR a-crate-must-not-cost-a-seam).
    pub user_name: Option<String>,
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
    /// `KeepAlive` rather than `RunAtLoad` alone, because `RunAtLoad` starts it once
    /// and the point of a Service is that it is still there tomorrow. Safe because the
    /// Watcher holds rather than exiting.
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
        // Both streams to one file: a refusal that landed in a second one would be the
        // line missing from the sequence somebody is reading.
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

    /// Refuses a Unit carrying something the platform's format cannot hold.
    ///
    /// Each of these formats is line-oriented or shell-parsed, so a `PERCH_HOME` with a
    /// newline in it would write further directives into a file systemd loads at every
    /// login. Windows is refused more widely.
    pub fn refuse_what_the_format_cannot_hold(&self, platform: Platform) -> Result<()> {
        let mut values: Vec<(String, String)> = vec![(
            "Perch's own path".to_string(),
            self.binary.to_string_lossy().to_string(),
        )];
        if let Some(log) = &self.log {
            values.push((
                "the log's path".to_string(),
                log.to_string_lossy().to_string(),
            ));
        }
        for (key, value) in &self.environment {
            values.push((format!("`{key}`"), value.clone()));
        }

        for (what, value) in values {
            crate::host::inert(&what, &value).map_err(|err| {
                PerchError::Invalid(format!(
                    "{err}, so the Service cannot be described.\nNothing was installed."
                ))
            })?;
            if platform == Platform::Windows
                && let Some(character) = value.chars().find(|c| *c == '"' || *c == '%')
            {
                return Err(PerchError::Invalid(format!(
                    "{what} carries `{character}`, which a Scheduled Task's \
                     command line has no way to hold as part of a value: \
                     `schtasks` hands it to `cmd.exe`, which reads `\"` as the \
                     end of one and expands `%…%` when the task runs.\n\
                     Nothing was installed. `perch watcher run` in a terminal \
                     works whatever the path is."
                )));
            }
        }
        Ok(())
    }

    /// A `systemd --user` unit.
    ///
    /// `Type=simple` because the Watcher *is* the process: it does not fork, does not
    /// write a pid file, and does not background itself. Standard output is left alone,
    /// so systemd captures it into the journal.
    fn systemd_unit(&self) -> String {
        let environment = self
            .environment
            .iter()
            .map(|(key, value)| {
                format!(
                    "Environment={}\n",
                    systemd_quoted(&format!("{key}={value}"))
                )
            })
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
            binary = systemd_quoted(&self.binary.to_string_lossy()),
            restart = RESTART_SECONDS,
            grace = STOP_GRACE_SECONDS,
            window = GIVE_UP_AFTER_SECONDS,
            tries = GIVE_UP_AFTER,
        )
    }

    /// What a Windows Scheduled Task is told to run.
    ///
    /// Wrapped in a `cmd /c` for the two things `schtasks` cannot express: the
    /// environment, which a task has no field for, and the redirection, which is the
    /// only way its output is kept. Appending, so a log survives a logout.
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
        // Quoted once more around the whole of it: `cmd /c` strips the first quote
        // and the last, and bare it takes them off the binary and the log, leaving
        // the `>>` inside a quoted run as text rather than as a redirection.
        format!(
            "cmd /c \"{environment}\"{binary}\" watcher run{log}\"",
            binary = self.binary.display(),
        )
    }
}

/// One command run against the machine's service manager.
///
/// A type rather than a tuple: these are the calls that make a machine start running
/// something at every login, and each is asserted against as the shell line it stands
/// for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driven {
    pub program: String,
    pub args: Vec<String>,
    /// Whether the whole act fails when this one does.
    ///
    /// The tidying-up steps are not: `systemctl --user disable` on a unit that was
    /// never enabled and `launchctl bootout` on one that is not loaded both fail, and
    /// both mean "it is already not running".
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

    /// The command as somebody would have typed it, for the line that says what failed
    /// — the sentence `perch upgrade` prints about the installer it drives, and so the
    /// same function.
    pub fn as_typed(&self) -> String {
        crate::host::as_typed(Path::new(&self.program), &self.args)
    }
}

/// What has to be run to start the Service, after the unit is in place.
pub fn starting(platform: Platform, unit: &Unit, at: Option<&Path>) -> Vec<Driven> {
    match platform {
        // Bootstrapped into the logged-in session rather than `load`ed, the deprecated
        // spelling since 10.10. `bootout` first, because bootstrapping over something
        // loaded fails — and allowed to fail on a first install.
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
        // `daemon-reload` first, because systemd has not read a unit that did not exist
        // when it last looked; `--now` makes an install something that has happened;
        // `restart`, because `enable --now` leaves a running unit alone.
        Platform::Other => vec![
            Driven::must("systemctl", &["--user", "daemon-reload"]),
            Driven::must("systemctl", &["--user", "enable", "--now", UNIT_NAME]),
            Driven::may_fail("systemctl", &["--user", "restart", UNIT_NAME]),
        ],
        // `/NP` keeps a console window off the desktop by registering the task without
        // a stored password, `/RU` names the user whose home the Profiles are under,
        // and `/F` makes a re-install a replacement.
        Platform::Windows => {
            let command = unit.windows_command();
            let mut args = vec!["/Create", "/TN", TASK_NAME, "/SC", "ONLOGON"];
            if let Some(user) = &unit.user_name {
                args.extend(["/RU", user, "/NP"]);
            }
            args.extend(["/TR", &command, "/F"]);
            // And run it, because `/Create /SC ONLOGON` starts nothing, so nothing
            // would hold the watcher lock `status` asks until the next logon. `/End`
            // first, as macOS `bootout`s before it bootstraps.
            vec![
                Driven::must("schtasks", &args),
                Driven::may_fail("schtasks", &["/End", "/TN", TASK_NAME]),
                Driven::may_fail("schtasks", &["/Run", "/TN", TASK_NAME]),
            ]
        }
    }
}

/// What has to be run to stop the Service and take it back.
///
/// Every step may fail, because each one is "make sure this is not running". What
/// decides whether an `uninstall` succeeded is whether the unit is gone afterwards,
/// which the caller asks separately.
pub fn stopping(platform: Platform, user_id: Option<u32>) -> Vec<Driven> {
    match platform {
        Platform::MacOs => vec![Driven::may_fail(
            "launchctl",
            &[
                "bootout",
                &format!("gui/{}/{LABEL}", user_id.unwrap_or_default()),
            ],
        )],
        Platform::Other => vec![Driven::may_fail(
            "systemctl",
            &["--user", "disable", "--now", UNIT_NAME],
        )],
        // `/End` before `/Delete`, which unregisters the task without terminating the
        // instance the scheduler started — and makes `schtasks /Query` answer "no such
        // task", which is how the callers ask whether it worked.
        Platform::Windows => vec![
            Driven::may_fail("schtasks", &["/End", "/TN", TASK_NAME]),
            Driven::may_fail("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"]),
        ],
    }
}

/// What has to be run once the unit file is gone, so the service manager stops
/// holding a unit that no longer exists. Apart from [`stopping`] because of
/// where it sits: a `daemon-reload` driven before the file is deleted re-reads
/// the unit still on disk, and `systemctl --user list-units` goes on showing it.
pub fn forgetting(platform: Platform) -> Vec<Driven> {
    match platform {
        Platform::Other => vec![Driven::may_fail("systemctl", &["--user", "daemon-reload"])],
        Platform::MacOs | Platform::Windows => Vec::new(),
    }
}

/// What has to be run to ask whether the Service is running right now.
///
/// `None` where the question is answered by the unit file being there, which is every
/// platform Perch does not support a service manager on.
pub fn asking(platform: Platform, user_id: Option<u32>) -> Option<Driven> {
    // Every platform has one, so this is `Option` for the caller's sake rather than for
    // a platform's: `status` asks before it knows whether anything is installed.
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

/// What to call the arrangement when saying it out loud, in the platform's own word.
///
/// Service is the domain term, and the sentence telling somebody where to look has to
/// use the word their machine uses.
pub fn described(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "a LaunchAgent",
        Platform::Other => "a systemd user unit",
        Platform::Windows => "a Scheduled Task that runs at logon",
    }
}

/// The same arrangement as a word a script branches on rather than reads.
///
/// Names the arrangement and not the operating system: what a caller does with
/// this answer turns on which thing is running the Watcher, and `Platform::Other`
/// says nothing (ADR a-command-names-its-noun).
pub fn arrangement(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "launchagent",
        Platform::Other => "systemd",
        Platform::Windows => "scheduled-task",
    }
}

/// Where somebody reads the decision log on this platform, as the line that tells them.
pub fn log_is_at(platform: Platform, log: Option<&Path>) -> String {
    match (platform, log) {
        (Platform::Other, _) => "journalctl --user -u perch-watch -f".to_string(),
        (_, Some(path)) => format!("{}", path.display()),
        (_, None) => "nowhere — nothing names a log file".to_string(),
    }
}

/// The five characters a property list cannot carry raw.
///
/// A plist is XML, and a home directory really can hold an `&`. Escaped rather than
/// refused, because the refusal would be about a machine somebody cannot change.
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
/// Beside the functions that wrote it, because the reader is the only thing that can
/// establish the writer was right. `None` for a unit Perch does not recognize, and
/// always for Windows, which keeps no file.
pub fn binary_in(platform: Platform, unit: &str) -> Option<PathBuf> {
    match platform {
        Platform::Other => unit
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .and_then(|line| line.strip_suffix(" watcher run"))
            .and_then(systemd_unquoted)
            .map(PathBuf::from),
        // The first `<string>` inside `ProgramArguments` is the program, and the two
        // after it are required as the systemd arm requires its ` watcher run`. Bounded
        // by `</array>`, so a later key is never read.
        Platform::MacOs => {
            let array = unit.split("<key>ProgramArguments</key>").nth(1)?;
            let array = array.split("</array>").next()?;
            let mut arguments = array
                .split("<string>")
                .skip(1)
                .map(|rest| rest.split("</string>").next());
            let binary = arguments.next()??;
            if arguments.next()?? != "watcher" || arguments.next()?? != "run" {
                return None;
            }
            Some(PathBuf::from(unescaped(binary)))
        }
        Platform::Windows => None,
    }
}

/// Where a unit actually sends the decision log, read out of the text of one.
///
/// Beside [`binary_in`] and for its reason: [`log_path`] derives its answer from
/// `PERCH_HOME` as it stands now, so a `status` run under another would name a file
/// nothing writes to. `None` for Windows, Linux, and a unit naming none.
pub fn log_in(platform: Platform, unit: &str) -> Option<PathBuf> {
    match platform {
        Platform::MacOs => {
            // Bounded by the next `<key>`, so a `StandardOutPath` somebody deleted the
            // value of never reads the following key's string as the log's path.
            let value = unit.split("<key>StandardOutPath</key>").nth(1)?;
            let value = value.split("<key>").next()?;
            let path = value.split("<string>").nth(1)?.split("</string>").next()?;
            Some(PathBuf::from(unescaped(path)))
        }
        Platform::Other | Platform::Windows => None,
    }
}

/// A value as a systemd unit can hold one, which is quoted.
///
/// Three characters and not one: `"` and `\` end and escape the quoted string, and `%`
/// is a *specifier* systemd expands before it runs anything. Unquoted, a path with a
/// space in it installs cleanly and never comes up.
fn systemd_quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// The inverse of [`systemd_quoted`]. `None` for anything not written by it, which
/// [`binary_in`] answers as a unit Perch does not recognize.
fn systemd_unquoted(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut read = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => read.push(characters.next()?),
            '%' => match characters.next()? {
                '%' => read.push('%'),
                // A lone `%` is a specifier, which is not something this wrote.
                _ => return None,
            },
            other => read.push(other),
        }
    }
    Some(read)
}

/// The inverse of [`xml_escaped`], for reading a path back out of a plist.
fn unescaped(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        // Last, so an `&amp;lt;` in somebody's path survives the round trip rather than
        // becoming a `<` on the way back.
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
            user_name: Some("someone".to_string()),
        }
    }

    #[test]
    fn a_path_written_into_a_unit_is_the_path_read_back_out_of_it() {
        for path in [
            "/usr/local/bin/perch",
            "/Users/some & one/bin/perch",
            "/Users/o'brien/<perch>/\"bin\"/perch",
            // An escape sequence spelled out in somebody's own path, which comes back
            // as the five characters they typed rather than as the one it names.
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
            binary_in(
                Platform::MacOs,
                "<key>ProgramArguments</key>\n<array>\n<string>/bin/perch</string>\n\
                 <string>list</string>\n</array>\n",
            ),
            None,
            "and a plist running something else names no Perch either",
        );
        assert_eq!(
            binary_in(
                Platform::MacOs,
                "<key>ProgramArguments</key>\n<array>\n<string>/bin/perch</string>\n\
                 </array>\n<key>StandardOutPath</key>\n<string>watcher</string>\n",
            ),
            None,
            "and a `<string>` belonging to a later key is not read as an argument",
        );
        assert_eq!(
            binary_in(Platform::Windows, &a_unit().plist()),
            None,
            "Windows keeps no file, so there is nothing to have read",
        );
    }

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

    #[test]
    fn a_systemd_unit_gives_up_rather_than_restarting_into_a_failure_for_ever() {
        let systemd = a_unit()
            .rendered(Platform::Other)
            .expect("Linux writes a file");

        assert!(systemd.contains("StartLimitBurst=5"), "{systemd}");
        assert!(systemd.contains("StartLimitIntervalSec=300"), "{systemd}");
    }

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

    #[test]
    fn the_log_a_plist_names_is_read_back_out_of_the_plist_that_names_it() {
        let at = PathBuf::from("/Users/someone/R&D/perch/watch.log");
        let unit = Unit {
            log: Some(at.clone()),
            ..a_unit()
        };
        let plist = unit.rendered(Platform::MacOs).expect("macOS writes a file");

        assert_eq!(log_in(Platform::MacOs, &plist), Some(at));
    }

    #[test]
    fn a_plist_that_names_no_log_is_answered_with_none_rather_than_the_next_value() {
        let emptied = "<key>StandardOutPath</key>\n\t<key>Program</key>\n\t\
             <string>/usr/local/bin/perch</string>";

        assert_eq!(log_in(Platform::MacOs, emptied), None);
        assert_eq!(log_in(Platform::MacOs, "<dict></dict>"), None);
    }

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
        assert!(
            line.contains("/RU someone"),
            "and the account is named by a value read off the machine: {line}"
        );
        assert!(
            !line.contains('%'),
            "`%USERNAME%` is `cmd.exe`'s notation, and there is no shell in \
             front of `schtasks` to expand it: {line}"
        );
    }

    #[test]
    fn a_windows_task_names_no_user_where_the_machine_will_not_say_who_it_is() {
        let unit = Unit {
            user_name: None,
            ..a_unit()
        };

        let line = starting(Platform::Windows, &unit, None)[0].as_typed();

        assert!(line.contains("/SC ONLOGON"), "{line}");
        assert!(!line.contains("/RU"), "{line}");
        assert!(!line.contains("/NP"), "{line}");
    }

    #[test]
    fn a_windows_task_carries_its_environment_and_its_log_in_the_command() {
        let unit = Unit {
            binary: PathBuf::from(r"C:\Users\someone\perch.exe"),
            environment: vec![("PERCH_HOME".to_string(), r"C:\work\perch".to_string())],
            log: Some(PathBuf::from(r"C:\work\perch\watch.log")),
            user_id: None,
            user_name: Some("someone".to_string()),
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

    /// `cmd /c` removes the first quote and the last one whenever the line
    /// carries more than two, so what it is handed has to be able to spare a
    /// pair — including on the default install, where nothing sets the
    /// environment and the line would otherwise begin at the binary's own quote.
    #[test]
    fn a_windows_task_survives_cmds_stripping_of_the_outer_quotes() {
        let unit = Unit {
            binary: PathBuf::from(r"C:\Users\someone\perch.exe"),
            environment: Vec::new(),
            log: Some(PathBuf::from(r"C:\work\perch\watch.log")),
            user_id: None,
            user_name: Some("someone".to_string()),
        };

        let command = unit.windows_command();
        let after = command
            .strip_prefix("cmd /c ")
            .expect("the wrapper is what `schtasks` is handed");
        // What cmd does to it: the leading quote off, and the last quote on the
        // line off, whatever sits after that last quote left alone.
        let stripped = after
            .strip_prefix('"')
            .expect("there is an outer quote to spare");
        let (before, tail) = stripped
            .rsplit_once('"')
            .expect("and a closing one to spend");
        let survives = format!("{before}{tail}");

        assert_eq!(
            survives,
            r#""C:\Users\someone\perch.exe" watcher run >> "C:\work\perch\watch.log" 2>&1"#,
            "the binary and the log keep their quotes and the `>>` is a \
             redirection rather than text: {command}"
        );
    }

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
