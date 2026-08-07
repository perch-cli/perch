use std::io::Write;

use clap::{Parser, Subcommand};

use perch::commands::add::{self, AddArgs};
use perch::commands::alias::{self, AliasCommand};
use perch::commands::config::{self, ConfigCommand};
use perch::commands::enable::{self, EnableCommand};
use perch::commands::export::{self, ExportArgs};
use perch::commands::group::{self, GroupCommand};
use perch::commands::import::{self, ImportArgs};
use perch::commands::list::{self, ListArgs};
use perch::commands::relogin::{self, ReloginArgs};
use perch::commands::remove::{self, RemoveArgs};
use perch::commands::run::{self, RunArgs};
use perch::commands::status::{self, StatusArgs};
use perch::commands::switch::{self, SwitchArgs};
use perch::commands::watch::{self, WatchArgs};
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
        /// The name to give, or to free with `--unset`.
        name: String,

        /// The Account to name: its current Alias, or its email address.
        #[arg(required_unless_present = "unset", conflicts_with = "unset")]
        target: Option<String>,

        /// Free the name instead of giving it.
        #[arg(long)]
        unset: bool,
    },

    /// Read and change how a Group behaves.
    ///
    /// Every setting is reachable from a script, because Perch has to be
    /// complete over SSH and in CI (ADR 0011). Most belong to a Group; the one
    /// about the Accounts in no Group has no Group to belong to, and is
    /// addressed by naming none (ADR 0017).
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },

    /// Keep an Account out of Cycling, without giving it up.
    ///
    /// For reserving an Account for one purpose: it stays listed, keeps its
    /// Alias, its Group and its Credential, and `perch switch <target>` still
    /// switches to it. Only Cycling stops choosing it, and `perch enable` puts
    /// it back.
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

    /// Write everything Perch holds to one encrypted file.
    ///
    /// The registry and every Credential, in the `age` format, so a dead
    /// machine or a new laptop does not cost a login for every subscription
    /// (ADR 0014). There is no per-Account form: a selective export is a
    /// partial restore.
    ///
    /// The passphrase is prompted and confirmed, and cannot be passed as an
    /// argument — an argument sits in the process table for anything on this
    /// machine to read. Without a terminal to type it at, the export is
    /// refused.
    Export {
        /// Where to write the file. Nothing is written over: a path that is
        /// already taken is refused.
        path: std::path::PathBuf,
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

    /// Put a whole machine back from a file `perch export` wrote.
    ///
    /// The exact inverse of an export: the registry and every Credential, so a
    /// new machine arrives with the setup the old one had rather than a pile of
    /// nameless logins (ADR 0014). Credentials land wherever this machine's
    /// Claude Code keeps one, whatever store the file was written from.
    ///
    /// It refuses a Perch that already holds an Account and names `perch purge`
    /// as the way to make room — merging two machines is a different feature.
    /// Nothing is made active by an import; `perch switch` is what lands.
    Import {
        /// The file to restore from. The passphrase is prompted, and a wrong
        /// one fails before anything is written.
        path: std::path::PathBuf,
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
    /// The one place that answers "what do I have". Renders from cache and
    /// never touches the network.
    List {
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
    /// Renders from cache unless you ask it to fetch, so it is cheap enough for
    /// a shell prompt.
    Status {
        /// Show every Account in the active Account's Group, so you can see
        /// where you would land before switching.
        #[arg(long)]
        group: bool,

        /// Read current Utilization from Anthropic first.
        ///
        /// The only thing in Perch that touches the network. Roughly 28-30
        /// reads an hour are allowed per Account and the allowance does not
        /// refill early, so a figure that cannot be read falls back to the
        /// cached one rather than failing.
        #[arg(long)]
        refresh: bool,

        /// Emit machine-readable output, with an observation time on every
        /// Utilization figure.
        #[arg(long)]
        json: bool,
    },

    /// Watch the Account you are on, and Cycle when it runs low.
    ///
    /// A loop in this terminal rather than a daemon (ADR 0013): it runs until
    /// you stop it with Ctrl-C, and leaves nothing behind when you do. Only the
    /// active Account is read, and only within a Group that has been told the
    /// watcher may act on it — `perch config set <group> watcher-may-act true`.
    ///
    /// Every decision is printed as it is made, including the ones where
    /// nothing happens, which are most of them. They go to standard output, so
    /// redirecting them to a file is yours to do — or `--once` takes a single
    /// check for cron or a systemd timer to run, and says what it decided in
    /// its exit code.
    Watch {
        /// Take one check and exit, saying what it decided in the exit code.
        ///
        /// For cron or a systemd timer, which is where an unattended watcher
        /// belongs: scheduling is the operating system's job. The policy is the
        /// loop's, run once — and the cooldown and no-return that pace it
        /// survive between invocations, so a check every minute still switches
        /// no more often than the Group allows.
        #[arg(long)]
        once: bool,
    },
}

/// The exit code a command that either did what it was asked or failed earns.
///
/// Every command but `run` is one of those: it worked, or it reported why not
/// and the failure decides the code. A Run is the exception because it is a way
/// of launching a program, and what that program said is what Perch says.
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

fn main() {
    report::install_panic_hook();

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Before the parser rather than after it, for the reason
    // `refuse_a_flag_without_the_separator` is written down at. Lossily, because
    // it only reads the shape of the words: one that is not text is clap's to
    // complain about, in clap's words.
    let typed: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    if let Err(refusal) = run::refuse_a_flag_without_the_separator(&typed) {
        std::process::exit(ended_as(Err(refusal), &mut out));
    }

    let cli = Cli::parse();
    let host = RealHost::new();

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
        // `--unset` needs no reading of its own: clap requires a Target unless
        // it was passed, and refuses both together, so the Target's absence is
        // exactly the flag.
        Command::Alias {
            name,
            target,
            unset: _,
        } => ok(alias::run(
            &host,
            match target {
                Some(target) => AliasCommand::Set { name, target },
                None => AliasCommand::Unset { name },
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
        Command::Export { path } => ok(export::run(&host, ExportArgs { path }, &mut out)),
        Command::Group { action } => ok(group::run(&host, action, &mut out)),
        Command::Import { path } => ok(import::run(&host, ImportArgs { path }, &mut out)),
        Command::List { json } => ok(list::run(&host, ListArgs { json }, &mut out)),
        Command::Relogin { target } => ok(relogin::run(&host, ReloginArgs { target }, &mut out)),
        Command::Remove { target, yes } => {
            ok(remove::run(&host, RemoveArgs { target, yes }, &mut out))
        }
        // The one command whose exit code is not Perch's own: what the client
        // said is what a script reads.
        Command::Run { target, command } => run::run(&host, RunArgs { target, command }, &mut out),
        Command::Status {
            group,
            refresh,
            json,
        } => ok(status::run(
            &host,
            StatusArgs {
                group,
                refresh,
                json,
            },
            &mut out,
        )),
        Command::Switch { target } => ok(switch::run(&host, SwitchArgs { target }, &mut out)),
        // The other command whose exit code is not simply "it worked": a check
        // reports what it decided, so a scheduler can tell a Switch from a
        // figure that could not be read without parsing the line (ADR 0013).
        Command::Watch { once } => watch::run(&host, WatchArgs { once }, &mut out),
    };

    let code = ended_as(outcome, &mut out);

    let _ = out.flush();
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The parser stops reading at `--` and hands the rest over whole: flags it
    /// has of its own, a word with a space in it, and a second `--` that belongs
    /// to the program's own argument parser rather than to Perch.
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

    /// The separator is mandatory, and the parser holds that line too: the
    /// refusal Perch writes itself is a better message for the same rule rather
    /// than the only thing enforcing it.
    #[test]
    fn a_command_without_the_separator_is_not_a_command_line() {
        assert!(Cli::try_parse_from(["perch", "run", "dev", "--resume"]).is_err());
        assert!(Cli::try_parse_from(["perch", "run", "dev", "npm", "test"]).is_err());
    }

    /// An Import is the exact inverse, so its surface is the same one: a path,
    /// and nothing that would narrow the restore or answer the passphrase ahead
    /// of time. There is no `--force` either — a machine that already holds an
    /// Account is refused rather than merged, and a flag would be the merge
    /// wearing a shortcut's clothes (ADR 0014).
    #[test]
    fn an_import_takes_a_path_and_nothing_else() {
        assert!(Cli::try_parse_from(["perch", "import", "/tmp/perch.age"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "import"]).is_err());

        for narrowed in [
            &["perch", "import", "/tmp/perch.age", "someone@example.com"][..],
            &["perch", "import", "/tmp/perch.age", "--account", "work"],
            &["perch", "import", "/tmp/perch.age", "--group", "work"],
            &[
                "perch",
                "import",
                "/tmp/perch.age",
                "--passphrase",
                "hunter2",
            ],
            &["perch", "import", "/tmp/perch.age", "--force"],
            &["perch", "import", "/tmp/perch.age", "--merge"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// An Export takes everything and has no target: a selective one is a
    /// partial restore, which is the failure the file exists to prevent wearing
    /// a feature's clothes. So a path is the whole of the surface — and there is
    /// no flag carrying a passphrase either, because an argument sits in the
    /// process table for anything on the machine to read (ADR 0014).
    #[test]
    fn an_export_takes_a_path_and_nothing_else() {
        assert!(Cli::try_parse_from(["perch", "export", "/tmp/perch.age"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "export"]).is_err());

        for narrowed in [
            &["perch", "export", "/tmp/perch.age", "someone@example.com"][..],
            &["perch", "export", "/tmp/perch.age", "--account", "work"],
            &["perch", "export", "/tmp/perch.age", "--group", "work"],
            &[
                "perch",
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
}
