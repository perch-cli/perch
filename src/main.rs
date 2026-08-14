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
use perch::commands::purge::{self, PurgeArgs};
use perch::commands::relogin::{self, ReloginArgs};
use perch::commands::remove::{self, RemoveArgs};
use perch::commands::run::{self, RunArgs};
use perch::commands::status::{self, StatusArgs};
use perch::commands::switch::{self, SwitchArgs};
use perch::commands::tui;
use perch::commands::upgrade::{self, UpgradeArgs};
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

    /// Read and change how Cycling behaves, at Global or for one Scope.
    ///
    /// Every setting is reachable from a script, because Perch has to be
    /// complete over SSH and in CI (ADR 0011). Config is two layers: Global is
    /// what applies where nothing narrower is said, and a Scope — a Group by
    /// name, or `ungrouped` for the Accounts in no Group — Overrides it.
    ///
    /// So two words set Global's default and three set one Scope's Override,
    /// and a Scope that has been told nothing goes on Inheriting Global.
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

    /// Give the machine back the state it had before Perch.
    ///
    /// Every Profile, every Credential Perch holds and its own registry, gone in
    /// one act — the exact inverse of an import, and what makes room for one
    /// (ADR 0014). It takes no target: giving up one Account is `perch remove`.
    ///
    /// It offers to write an export first, lists the Accounts that will go by
    /// email address, and wants the word `purge` typed rather than a letter.
    /// Whatever Claude Code is logged in as is left exactly where it is.
    Purge {
        /// Purge without being asked, and write no export.
        ///
        /// An export is a path you name and a passphrase you type, neither of
        /// which a script can be asked for — so this answers both questions at
        /// once. Without a terminal and without this flag, a purge is refused.
        #[arg(long)]
        yes: bool,
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

    /// Open the interactive view: the Accounts and their Utilization, side by
    /// side.
    ///
    /// For when the choice wants making by eye rather than by rule. Nothing
    /// here is only here — every capability has a plain command form, because
    /// Perch has to be complete over SSH and in scripts (ADR 0011).
    ///
    /// Two tabs. `Status` answers "where am I and what governs me" — the active
    /// Account, the table of Accounts in the order `perch switch` would rank
    /// them, and the Settings in force for the Scope you are in. `Config`
    /// answers "what does each Scope declare" and lets you change it.
    ///
    /// It writes what it can unwrite (ADR 0034): Settings, Aliases, whether
    /// Cycling may choose an Account, which Group it is in, declaring a Group,
    /// Enter to Switch and `x` to Run. `add`, `remove`, `relogin`, `purge`,
    /// `export`, `import` and deleting a Group have no key here. There is no
    /// save button — a change is written when it is made.
    ///
    /// The first frame is drawn from cache and never waits on the network, with
    /// the age of every figure on it; `r` Refreshes, and the display keeps
    /// answering while it does. `q` or Ctrl-C leaves.
    Tui,

    /// Replace this Perch with a newer Release.
    ///
    /// Through whatever Channel installed it (ADR 0039): a Homebrew
    /// Installation is handed to `brew upgrade perch` and an npm one to `npm
    /// update -g perch-cli`, because their binaries are theirs to replace and
    /// writing over one is reverted or thrown away at the next thing they do.
    /// Only a binary the installer script put where it puts them — `~/.local/bin`,
    /// `%LOCALAPPDATA%\Perch\bin` on Windows, or `$PERCH_INSTALL_DIR` — is
    /// replaced by Perch itself, using that same installer.
    ///
    /// A binary anywhere else — unpacked from the Release page by hand, most
    /// likely — is refused rather than written over, and `--channel` says which
    /// Channel it really is when the path does not.
    ///
    /// Nothing Perch holds is touched: no registry, no Credential, no Profile.
    Upgrade {
        /// The Release to install, with or without its leading `v`. Without
        /// this, the newest.
        #[arg(long, value_name = "TAG")]
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
        #[arg(long)]
        yes: bool,
    },

    /// Watch the Account you are on, and Cycle when it runs low.
    ///
    /// A loop in this terminal rather than a daemon (ADR 0013): it runs until
    /// you stop it with Ctrl-C, and leaves nothing behind when you do. Only the
    /// active Account is read, and only within a Scope that has been told the
    /// watcher may act on it — `perch config set <group> watcher-may-act true`
    /// for a Group, or the same for `ungrouped` where `cycle-ungrouped` is on
    /// as well, because being interchangeable at all is its own yes (ADR 0017).
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

/// Whether this command line is the bare question "what is installed?".
///
/// Only the bare form, and deliberately: `perch --version` is what a person
/// types, what a bug report carries and what a formula asserts on. Anything
/// with a subcommand in it — `perch add --version` — is still clap's to answer
/// in clap's words, and taking those over here would mean owning a parser
/// beside the one Perch already has.
fn version_asked_for(typed: &[String]) -> bool {
    matches!(typed, [only] if only == "--version" || only == "-V")
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

    let host = RealHost::new();

    // Before the parser, because clap answers `--version` by printing and
    // exiting, and the line underneath it has to come from somewhere with a
    // Host to ask (ADR 0039). What it says is `upgrade::version_report`'s, so
    // that the shape the Homebrew formula asserts on is held still somewhere a
    // test can reach.
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
        Command::Purge { yes } => ok(purge::run(&host, PurgeArgs { yes }, &mut out)),
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
        // A third whose exit code is not simply Perch's own: the picker can
        // hand the terminal to a client, and what that client said is what a
        // script reads.
        Command::Tui => tui::run(&host, &mut out),
        // And a fourth: what `brew` or `npm` exited with is what a script
        // reads, because a failed `brew upgrade` is a failed upgrade and a code
        // of Perch's own would lose which of `brew`'s failures it was.
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

    /// A Run is the exception every other command is measured against: what the
    /// client said is what a script reads. Everything else earns nought for
    /// having worked, and the failure decides the code when it did not.
    #[test]
    fn a_command_that_worked_is_nought_and_one_that_failed_is_its_own_code() {
        assert_eq!(ok(Ok(())).expect("it worked"), EXIT_OK);

        let refused = ok(Err(perch::error::PerchError::NotFound("gone".to_string())))
            .expect_err("it did not");
        assert_eq!(refused.exit_code(), perch::error::EXIT_NOT_FOUND);
    }

    /// What Perch exits with, and where a failure is said. Standard output is
    /// flushed first: a refusal on stderr that overtook the lines the command
    /// had already printed would read as being about the wrong thing.
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

    /// A Purge is never about one Account — that is `perch remove`, which is
    /// deliberately narrow, and two verbs for one act is exactly the ambiguity
    /// the shared Alias and Group namespace exists to prevent. So it takes no
    /// Target, in any of the shapes one could arrive in. `--yes` is the whole of
    /// the surface, because it is the only question a script can answer.
    #[test]
    fn a_purge_takes_no_target() {
        assert!(Cli::try_parse_from(["perch", "purge"]).is_ok());
        assert!(Cli::try_parse_from(["perch", "purge", "--yes"]).is_ok());

        for narrowed in [
            &["perch", "purge", "someone@example.com"][..],
            &["perch", "purge", "work"],
            &["perch", "purge", "--account", "work"],
            &["perch", "purge", "--group", "work"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// The interactive view has no arguments at all. What it shows is every
    /// Account, and how it shows them is a keystroke away rather than a flag —
    /// and a `--json` here would be `perch list --json`, which is the command
    /// ADR 0011 requires to exist anyway.
    #[test]
    fn the_interactive_view_takes_no_arguments() {
        assert!(Cli::try_parse_from(["perch", "tui"]).is_ok());

        for narrowed in [
            &["perch", "tui", "work"][..],
            &["perch", "tui", "--json"],
            &["perch", "tui", "--refresh"],
            &["perch", "tui", "--group", "work"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// An Upgrade is about the Installation and never about an Account, so
    /// nothing that would narrow it to one parses. `--json` says what a check
    /// found, so it does not appear without one — clap enforces that rather
    /// than the command, because a flag that parses and is then refused is a
    /// flag the `--help` still advertises as free-standing.
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

    /// The bare question and nothing else. `perch --version` is what a person
    /// types, what a bug report carries and what the Homebrew formula asserts
    /// on; anything with a subcommand in it stays clap's to answer, because the
    /// alternative is owning a second parser beside the one Perch has.
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
