use std::io::Write;

use clap::{Parser, Subcommand};

use perch::commands::add::{self, AddArgs};
use perch::commands::alias::{self, AliasCommand};
use perch::commands::config::{self, ConfigCommand};
use perch::commands::enable::{self, EnableCommand};
use perch::commands::group::{self, GroupCommand};
use perch::commands::holdings::{self, HoldingsCommand};
use perch::commands::list::{self, ListArgs};
use perch::commands::relogin::{self, ReloginArgs};
use perch::commands::remove::{self, RemoveArgs};
use perch::commands::run::{self, RunArgs};
use perch::commands::status::{self, StatusArgs};
use perch::commands::switch::{self, SwitchArgs};
use perch::commands::upgrade::{self, UpgradeArgs};
use perch::commands::watcher::{self, WatcherCommand};
use perch::error::EXIT_OK;
use perch::host::RealHost;
use perch::report;

#[derive(Parser)]
#[command(
    name = "perch",
    version,
    about = "Run Claude Code as whichever Claude account you want"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add an Account by logging in inside a new Profile.
    ///
    /// The Account you are on stays active and its session is untouched, so
    /// gaining an Account never costs you the one you are using.
    Add {
        /// The Group to put the new Account in. Without it, the Account's
        /// organization is offered as a default for you to confirm.
        #[arg(long, value_name = "NAME")]
        group: Option<String>,

        /// Put the new Account in no Group, and ask nothing.
        #[arg(long, conflicts_with = "group")]
        no_group: bool,

        /// A short name to reach the Account by, instead of its email address.
        #[arg(long, value_name = "NAME")]
        alias: Option<String>,
    },

    /// Name an Account, so no command needs its email address.
    ///
    /// Aliases and Group names share one namespace, so a name is refused if
    /// the other half already has it and a Target is never ambiguous.
    Alias {
        /// The Account to name: its current Alias, or its email address.
        target: String,

        /// The name to give, unless `--unset` is freeing the one it has.
        #[arg(required_unless_present = "unset", conflicts_with = "unset")]
        name: Option<String>,

        /// Free the name the Account answers to, instead of giving it one.
        #[arg(long)]
        unset: bool,
    },

    /// Read and change how Cycling behaves, one Scope at a time.
    ///
    /// Every Setting is reachable from a script, because Perch has to be
    /// complete over SSH and in CI. A Setting is said about the Scope it
    /// governs and there is nothing above them, so a `set` is always
    /// `<scope> <key> <value>` — where a Scope is a Group by name, or
    /// `ungrouped` for the Accounts in no Group.
    ///
    /// A bare `perch config get` reads every Scope there is; a `set` that names
    /// none is refused, because a rule with no subject is a rule about
    /// nothing.
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },

    /// Keep an Account out of Cycling, without giving it up.
    ///
    /// It stays listed, keeps its Alias, its Group and its Credential, and
    /// `perch switch <target>` still switches to it. Only Cycling stops
    /// choosing it, and `perch enable` puts it back.
    Disable {
        /// The Account: its Alias, or its email address.
        target: String,
    },

    /// Return an Account to the Cycling pool.
    ///
    /// The other half of `perch disable`, and all it takes to undo one: the
    /// Account never lost its Profile or its Credential, so nothing has to be
    /// logged into again.
    Enable {
        /// The Account: its Alias, or its email address.
        target: String,
    },

    /// Declare which Accounts are interchangeable.
    ///
    /// Cycling only ever moves between Accounts in one Group, so a Group is how
    /// you say that another work subscription is an acceptable landing place
    /// and your personal Account is not.
    Group {
        #[command(subcommand)]
        action: GroupCommand,
    },

    /// Everything Perch holds on this machine: write it out, put it back, or
    /// give it up.
    ///
    /// Every Profile, every Credential Perch holds, the registry naming them
    /// and what each Group carries — the counterpart to an Installation, which
    /// is what a Channel left. None of the three takes a Target, because none
    /// of them is about one Account.
    Holdings {
        #[command(subcommand)]
        action: HoldingsCommand,
    },

    /// Log an Account in again, in place.
    ///
    /// The way back from a Quarantine: the Account keeps its Alias, its Group,
    /// whether Cycling may choose it and its place in the listing, and only its
    /// Credential is replaced. The Account you are working in is untouched,
    /// unless it is the one being repaired — then its fresh Credential becomes
    /// the live one, because a repair nothing reads is not a repair.
    Relogin {
        /// The Account: its Alias, or its email address.
        target: String,
    },

    /// Give up an Account: forget it, and delete the Credential Perch holds.
    ///
    /// The Account stops being listed and stops being a Cycle candidate, and
    /// the Alias it answered to is free again. Removing the Account you are on
    /// names the Account Perch will leave active, lands on it first, and asks
    /// before any of it happens.
    Remove {
        /// The Account: its Alias, or its email address.
        target: String,

        /// Remove it without being asked to confirm.
        #[arg(long)]
        yes: bool,
    },

    /// Launch Claude Code as one Account, without changing which one is active.
    ///
    /// The Account you are on stays active — in every other terminal, in the
    /// editor extension and in the desktop app — because a Run points one
    /// process at one Profile and touches nothing else. Two terminals can run
    /// two Accounts at once, and the client's exit code is Perch's.
    Run {
        /// The Account to run as: its Alias, or its email address. A Group has
        /// no single meaning here, so naming one is refused.
        target: String,

        /// What to run, after a mandatory `--`: a program and its arguments, or
        /// Claude Code's own arguments where the first word is a flag. Nothing
        /// here is read by Perch, so a `--json` after `--` is the program's.
        #[arg(last = true, allow_hyphen_values = true, num_args = .., value_name = "COMMAND")]
        command: Vec<String>,
    },

    /// Show every Account with its Alias, Group, state and cached Utilization.
    ///
    /// The one place that answers "what do I have", at every breadth: bare it
    /// is every Account Perch holds, and a Scope narrows it to the Accounts you
    /// could Cycle between — where you would land before you switch. Renders
    /// from cache unless you ask it to fetch.
    List {
        /// Which Accounts to show: a Group by name, or `ungrouped` for the
        /// Accounts in no Group. Without one, every Account Perch holds.
        #[arg(value_name = "SCOPE")]
        scope: Option<String>,

        /// Read current Utilization from Anthropic first.
        ///
        /// Exactly the Accounts about to be shown and no others, so narrowing
        /// the listing narrows the reads with it. Roughly 28-30 reads an hour
        /// are allowed per Account and the allowance does not refill early, so
        /// a figure that cannot be read falls back to the cached one rather
        /// than failing.
        #[arg(long)]
        refresh: bool,

        /// Emit machine-readable output, with an observation time on every
        /// Utilization figure.
        #[arg(long)]
        json: bool,
    },

    /// Make an Account active everywhere, with no login flow.
    ///
    /// With no target, Perch picks for you: it Cycles within the current
    /// Account's Group, ranking each Account by its most constrained Quota
    /// Window, and never asks anything.
    ///
    /// The Credential you are leaving is Captured back into its own Profile
    /// first, so a Rotation that happened while it was active is not lost. Your
    /// memory, settings, plugins and project history are untouched.
    Switch {
        /// The Account to switch to — its Alias or its email address — or a
        /// Group to Cycle within.
        target: Option<String>,
    },

    /// Show the active Account and its cached Utilization.
    ///
    /// The Account you are on and nothing else — a set of Accounts is
    /// `perch list`, at whatever breadth. Renders from cache unless you ask it
    /// to fetch, so it is cheap enough for a shell prompt.
    Status {
        /// Read current Utilization from Anthropic first.
        ///
        /// The one Account this command is about, and no others. Asking for a
        /// refresh is the only thing in Perch that touches the network, here or
        /// on a listing. Roughly 28-30 reads an hour are allowed per Account
        /// and the allowance does not refill early, so a figure that cannot be
        /// read falls back to the cached one rather than failing.
        #[arg(long)]
        refresh: bool,

        /// Emit machine-readable output, with an observation time on every
        /// Utilization figure.
        #[arg(long)]
        json: bool,
    },

    /// Replace this Perch with a newer Release.
    ///
    /// Through whatever Channel installed it: a Homebrew Installation is
    /// handed to `brew upgrade perch` and an npm one
    /// to `npm update -g perch-cli`, because their binaries are theirs to
    /// replace and writing over one is reverted or thrown away at the next
    /// thing they do. Only a binary the installer script put where it puts them
    /// — `~/.local/bin`, `%LOCALAPPDATA%\Perch\bin` on Windows, or
    /// `$PERCH_INSTALL_DIR` — is replaced by Perch itself, using that same
    /// installer.
    ///
    /// A binary anywhere else — unpacked from the Release page by hand, most
    /// likely — is refused rather than written over, and `--channel` says which
    /// Channel it really is when the path does not.
    ///
    /// Nothing Perch holds is touched: no registry, no Credential, no Profile.
    Upgrade {
        /// The Release to install, with or without its leading `v`. Without
        /// this, the newest.
        ///
        /// Not with `--check`, which asks what the newest Release is and has no
        /// use for a named one. Refused rather than ignored, because a named
        /// Release that is quietly thrown away is one nobody is told about.
        #[arg(long, value_name = "TAG", conflicts_with = "check")]
        release: Option<String>,

        /// Say what is installed and what is newest, and install nothing.
        #[arg(long)]
        check: bool,

        /// That answer as a document.
        #[arg(long, requires = "check")]
        json: bool,

        /// Which Channel installed this Perch, for when its path does not say.
        #[arg(long, value_name = "NAME")]
        channel: Option<String>,

        /// Agree ahead of time to a Release older than the one installed.
        ///
        /// Nothing is installed by a check, so there is nothing for it to agree
        /// to.
        #[arg(long, conflicts_with = "check")]
        yes: bool,
    },

    /// Cycle on your behalf when the Account you are on runs low.
    ///
    /// Three arrangements and one behavior: `run` is a loop you can see and
    /// kill, `install` hands that same loop to the machine's own service
    /// manager, and `check` is one round for a scheduler to fire. One of them
    /// at a time, and the policy is the same in all three.
    ///
    /// Only the active Account is read, and only within a Scope that has been
    /// told the watcher may act on it — `perch config set <group>
    /// watcher-may-act true` for a Group, or the same for `ungrouped` where
    /// `interchangeable` is on as well, because being interchangeable at all is
    /// its own yes.
    Watcher {
        #[command(subcommand)]
        action: WatcherCommand,
    },
}

/// Nought for having worked, and otherwise whatever the failure earned.
///
/// A Run is the one command this is wrong for: it launches a program, and what
/// that program said is what Perch says.
fn ok(outcome: perch::Result<()>) -> perch::Result<i32> {
    outcome.map(|()| EXIT_OK)
}

/// What Perch exits with, and where a failure is said: to stderr, after
/// everything the command had already printed has been let out.
fn ended_as(outcome: perch::Result<i32>, out: &mut dyn Write) -> i32 {
    match outcome {
        Ok(code) => code,
        Err(error) => {
            let _ = out.flush();
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "{error}");
            error.exit_code()
        }
    }
}

/// Whether this command line is the bare question "what is installed?".
///
/// Only the bare form. Anything with a subcommand in it stays clap's to answer
/// in clap's words, because taking those over means owning a second parser
/// beside the one Perch has.
fn version_asked_for(typed: &[String]) -> bool {
    matches!(typed, [only] if only == "--version" || only == "-V")
}

fn main() {
    report::install_panic_hook();

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Before the parser, for the reason `refuse_a_flag_without_the_separator`
    // is written down at. Lossily: a word that is not text is clap's to
    // complain about rather than this line's.
    let typed: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    if let Err(refusal) = run::refuse_a_flag_without_the_separator(&typed) {
        std::process::exit(ended_as(Err(refusal), &mut out));
    }

    let host = RealHost::new();

    // Before the parser, because clap answers `--version` by printing and
    // exiting, and the line underneath it has to come from somewhere with a
    // Host to ask (ADR an-upgrade-asks-its-channel).
    if version_asked_for(&typed) {
        let _ = write!(out, "{}", perch::upgrade::version_report(&host));
        let _ = out.flush();
        std::process::exit(EXIT_OK);
    }

    let cli = Cli::parse();

    let outcome = match cli.command {
        Command::Add {
            group,
            no_group,
            alias,
        } => ok(add::run(
            &host,
            AddArgs {
                group,
                no_group,
                alias,
            },
            &mut out,
        )),
        // `--unset` needs no reading of its own: clap requires a name unless
        // it was passed and refuses both together, so the name's absence is
        // exactly the flag.
        Command::Alias {
            target,
            name,
            unset: _,
        } => ok(alias::run(
            &host,
            match name {
                Some(name) => AliasCommand::Set { target, name },
                None => AliasCommand::Unset { target },
            },
            &mut out,
        )),
        Command::Config { action } => ok(config::run(&host, action, &mut out)),
        Command::Disable { target } => ok(enable::run(
            &host,
            EnableCommand::Disable { target },
            &mut out,
        )),
        Command::Enable { target } => ok(enable::run(
            &host,
            EnableCommand::Enable { target },
            &mut out,
        )),
        Command::Group { action } => ok(group::run(&host, action, &mut out)),
        Command::Holdings { action } => ok(holdings::run(&host, action, &mut out)),
        Command::List {
            scope,
            refresh,
            json,
        } => ok(list::run(
            &host,
            ListArgs {
                scope,
                refresh,
                json,
            },
            &mut out,
        )),
        Command::Relogin { target } => ok(relogin::run(&host, ReloginArgs { target }, &mut out)),
        Command::Remove { target, yes } => {
            ok(remove::run(&host, RemoveArgs { target, yes }, &mut out))
        }
        // The one command whose exit code is not Perch's own: what the client
        // said is what a script reads.
        Command::Run { target, command } => run::run(&host, RunArgs { target, command }, &mut out),
        Command::Status { refresh, json } => {
            ok(status::run(&host, StatusArgs { refresh, json }, &mut out))
        }
        Command::Switch { target } => ok(switch::run(&host, SwitchArgs { target }, &mut out)),
        // What `brew` or `npm` exited with, because a code of Perch's own
        // would lose which of their failures it was.
        Command::Upgrade {
            release,
            check,
            json,
            channel,
            yes,
        } => upgrade::run(
            &host,
            UpgradeArgs {
                release,
                check,
                json,
                channel,
                yes,
            },
            &mut out,
        ),
        // A `check` reports what it decided, so a scheduler tells a Switch
        // from a figure it could not read without parsing the line
        // (ADR a-watcher-knob-is-arithmetic).
        Command::Watcher { action } => watcher::run(&host, action, &mut out),
    };

    let code = ended_as(outcome, &mut out);

    let _ = out.flush();
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_worked_is_nought_and_one_that_failed_is_its_own_code() {
        assert_eq!(ok(Ok(())).expect("it worked"), EXIT_OK);

        let refused = ok(Err(perch::error::PerchError::NotFound("gone".to_string())))
            .expect_err("it did not");
        assert_eq!(refused.exit_code(), perch::error::EXIT_NOT_FOUND);
    }

    #[test]
    fn what_a_command_ended_as_is_its_code_and_a_failure_says_why_on_the_way_out() {
        let mut out = Vec::new();
        assert_eq!(
            ended_as(Ok(3), &mut out),
            3,
            "a Run's code is passed through"
        );
        assert!(out.is_empty(), "and nothing is added to what it said");

        let mut out = Vec::new();
        let code = ended_as(
            Err(perch::error::PerchError::Invalid("no".to_string())),
            &mut out,
        );
        assert_eq!(code, perch::error::EXIT_INVALID);
        assert!(
            out.is_empty(),
            "a failure is said on stderr, not on the stream a script is parsing"
        );
    }

    fn command_of(line: &[&str]) -> Vec<String> {
        match Cli::try_parse_from(line).expect("the line parses").command {
            Command::Run { command, .. } => command,
            _ => panic!("`{}` is not a Run", line.join(" ")),
        }
    }

    #[test]
    fn nothing_after_the_target_is_a_run_with_no_command() {
        assert!(command_of(&["perch", "run", "dev"]).is_empty());
        assert!(command_of(&["perch", "run", "dev", "--"]).is_empty());
    }

    /// The fixtures are the three shapes that could be read as Perch's: a flag
    /// it has of its own, a word with a space in it, and a second `--`.
    #[test]
    fn everything_after_the_separator_arrives_as_it_was_typed() {
        assert_eq!(
            command_of(&["perch", "run", "dev", "--", "--json", "-p", "two words"]),
            vec!["--json", "-p", "two words"]
        );
        assert_eq!(
            command_of(&["perch", "run", "dev", "--", "npm", "test", "--", "--watch"]),
            vec!["npm", "test", "--", "--watch"]
        );
    }

    /// The parser holds this line too, so the refusal Perch writes itself is a
    /// better message for the same rule rather than the only thing enforcing it.
    #[test]
    fn a_command_without_the_separator_is_not_a_command_line() {
        assert!(Cli::try_parse_from(["perch", "run", "dev", "--resume"]).is_err());
        assert!(Cli::try_parse_from(["perch", "run", "dev", "npm", "test"]).is_err());
    }

    /// An Import is the exact inverse, so its surface is the same one: a path,
    /// and nothing that would narrow the restore, answer the passphrase ahead of
    /// time, or turn the refusal to merge into a flag
    /// (ADR the-holdings-go-out-sealed).
    #[test]
    fn an_import_takes_a_path_and_nothing_else() {
        assert!(Cli::try_parse_from(["perch", "holdings", "import", "/tmp/perch.age"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "holdings", "import"]).is_err());

        for narrowed in [
            &[
                "perch",
                "holdings",
                "import",
                "/tmp/perch.age",
                "someone@example.com",
            ][..],
            &[
                "perch",
                "holdings",
                "import",
                "/tmp/perch.age",
                "--account",
                "work",
            ],
            &[
                "perch",
                "holdings",
                "import",
                "/tmp/perch.age",
                "--group",
                "work",
            ],
            &[
                "perch",
                "holdings",
                "import",
                "/tmp/perch.age",
                "--passphrase",
                "hunter2",
            ],
            &["perch", "holdings", "import", "/tmp/perch.age", "--force"],
            &["perch", "holdings", "import", "/tmp/perch.age", "--merge"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// The fixtures are the four shapes a Target could arrive in. `--yes` is the
    /// whole of the surface, because it is the only question a script can
    /// answer.
    #[test]
    fn a_purge_takes_no_target() {
        assert!(Cli::try_parse_from(["perch", "holdings", "purge"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "holdings", "purge", "--yes"]).is_ok());

        for narrowed in [
            &["perch", "holdings", "purge", "someone@example.com"][..],
            &["perch", "holdings", "purge", "work"],
            &["perch", "holdings", "purge", "--account", "work"],
            &["perch", "holdings", "purge", "--group", "work"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// `--json` says what a check found, so clap refuses it without one rather
    /// than the command doing so: a flag that parses and is then refused is a
    /// flag `--help` still advertises as free-standing.
    #[test]
    fn an_upgrade_takes_a_release_and_never_a_target() {
        for line in [
            &["perch", "upgrade"][..],
            &["perch", "upgrade", "--release", "v0.2.0"],
            &["perch", "upgrade", "--release", "0.2.0", "--yes"],
            &["perch", "upgrade", "--check"],
            &["perch", "upgrade", "--check", "--json"],
            &["perch", "upgrade", "--channel", "npm"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "upgrade", "someone@example.com"][..],
            &["perch", "upgrade", "--account", "work"],
            &["perch", "upgrade", "--group", "work"],
            &["perch", "upgrade", "--json"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    #[test]
    fn only_the_bare_version_question_is_answered_here() {
        for typed in [vec!["--version"], vec!["-V"]] {
            let typed: Vec<String> = typed.into_iter().map(str::to_string).collect();
            assert!(version_asked_for(&typed), "{typed:?}");
        }

        for typed in [
            vec!["add", "--version"],
            vec!["--version", "extra"],
            vec!["status"],
            vec![],
        ] {
            let typed: Vec<String> = typed.into_iter().map(str::to_string).collect();
            assert!(!version_asked_for(&typed), "{typed:?}");
        }
    }

    /// The fixtures are a Target and the three flags that would narrow an
    /// Export or answer for it.
    #[test]
    fn an_export_takes_a_path_and_nothing_else() {
        assert!(Cli::try_parse_from(["perch", "holdings", "export", "/tmp/perch.age"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "holdings", "export"]).is_err());

        for narrowed in [
            &[
                "perch",
                "holdings",
                "export",
                "/tmp/perch.age",
                "someone@example.com",
            ][..],
            &[
                "perch",
                "holdings",
                "export",
                "/tmp/perch.age",
                "--account",
                "work",
            ],
            &[
                "perch",
                "holdings",
                "export",
                "/tmp/perch.age",
                "--group",
                "work",
            ],
            &[
                "perch",
                "holdings",
                "export",
                "/tmp/perch.age",
                "--passphrase",
                "hunter2",
            ],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// The half of *one capability, one name, one place* no other test can
    /// make: that the spelling a name moved off is **gone**
    /// (ADR a-command-names-its-noun). Nothing is aliased.
    #[test]
    fn the_watcher_is_five_verbs_and_the_names_they_moved_off_are_not_commands() {
        for line in [
            &["perch", "watcher", "run"][..],
            &["perch", "watcher", "check"],
            &["perch", "watcher", "install"],
            &["perch", "watcher", "uninstall"],
            &["perch", "watcher", "status"],
            &["perch", "watcher", "status", "--json"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for moved in [
            &["perch", "export", "/tmp/perch.age"][..],
            &["perch", "import", "/tmp/perch.age"],
            &["perch", "purge"],
            &["perch", "watch"],
            &["perch", "service", "install"],
            // A Check changes both the exit code's meaning and the command's
            // lifetime, so it is a verb rather than a flag on the loop.
            &["perch", "watcher", "run", "--once"],
            // A noun on its own is not a command, and neither is a verb under
            // the wrong one.
            &["perch", "holdings"],
            &["perch", "watcher"],
            &["perch", "holdings", "run"],
            &["perch", "watcher", "export", "/tmp/perch.age"],
        ] {
            assert!(
                Cli::try_parse_from(moved).is_err(),
                "`{}` should not parse",
                moved.join(" ")
            );
        }
    }
}
