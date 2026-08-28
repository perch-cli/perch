use std::io::Write;

use clap::{Parser, Subcommand};

use perch::commands::add::{self, AddArgs};
use perch::commands::alias::{self, AliasCommand};
use perch::commands::config::{self, ConfigCommand};
use perch::commands::enable::{self, EnableCommand};
use perch::commands::group::{self, GroupCommand};
use perch::commands::holdings::{self, HoldingsCommand};
use perch::commands::list::{self, ListArgs};
use perch::commands::probe::{self, ProbeArgs};
use perch::commands::relogin::{self, ReloginArgs};
use perch::commands::remove::{self, RemoveArgs};
use perch::commands::run::{self, RunArgs};
use perch::commands::status::{self, StatusArgs};
use perch::commands::switch::{self, SwitchArgs};
use perch::commands::upgrade::{self, UpgradeArgs};
use perch::commands::version;
use perch::commands::watcher::{self, WatcherCommand};
use perch::error::EXIT_OK;
use perch::host::RealHost;
use perch::report;
use perch::trail;

#[derive(Parser)]
#[command(
    name = "perch",
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
    Add(AddArgs),

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

    /// Show every Account with its Alias, Group, state and cached Utilization.
    ///
    /// The one place that answers "what do I have", at every breadth: bare it
    /// is every Account Perch holds, and a Scope narrows it to the Accounts you
    /// could Cycle between — where you would land before you switch. Renders
    /// from cache unless you ask it to fetch.
    List(ListArgs),

    /// Everything Perch can see of this machine, in one paste.
    ///
    /// What a bug report needs and nobody should have to gather by hand: which
    /// Perch and which Claude Code, what the Holdings hold, which of Perch's
    /// assumptions still hold, and what has been run here lately. Names and
    /// paths come out as placeholders unless `--raw` says otherwise, because
    /// what this is for is being pasted somewhere else.
    ///
    /// Reads and judges, and repairs nothing. It reaches no network, brings no
    /// registry forward and adds no line to the Trail, so running it never
    /// changes the machine it is describing. Exits `0` whatever it finds.
    Probe(ProbeArgs),

    /// Log an Account in again, in place.
    ///
    /// The way back from a Quarantine: the Account keeps its Alias, its Group,
    /// whether Cycling may choose it and its place in the listing, and only its
    /// Credential is replaced. The Account you are working in is untouched,
    /// unless it is the one being repaired — then its fresh Credential becomes
    /// the live one, because a repair nothing reads is not a repair.
    Relogin(ReloginArgs),

    /// Give up an Account: forget it, and delete the Credential Perch holds.
    ///
    /// The Account stops being listed and stops being a Cycle candidate, and
    /// the Alias it answered to is free again. Removing the Account you are on
    /// names the Account Perch will leave active, lands on it first, and asks
    /// before any of it happens.
    Remove(RemoveArgs),

    /// Launch Claude Code as one Account, without changing which one is active.
    ///
    /// The Account you are on stays active — in every other terminal, in the
    /// editor extension and in the desktop app — because a Run points one
    /// process at one Profile and touches nothing else. Two terminals can run
    /// two Accounts at once, and the client's exit code is Perch's.
    Run(RunArgs),

    /// Show the active Account and its cached Utilization.
    ///
    /// The Account you are on and nothing else — a set of Accounts is
    /// `perch list`, at whatever breadth. Renders from cache unless you ask it
    /// to fetch, so it is cheap enough for a shell prompt.
    Status(StatusArgs),

    /// Make an Account active everywhere, with no login flow.
    ///
    /// With no target, Perch picks for you: it Cycles within the current
    /// Account's Group, ranking each Account by its most constrained Quota
    /// Window, and never asks anything.
    ///
    /// The Credential you are leaving is Captured back into its own Profile
    /// first, so a Rotation that happened while it was active is not lost. Your
    /// memory, settings, plugins and project history are untouched.
    Switch(SwitchArgs),

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
    Upgrade(UpgradeArgs),

    /// Say which Perch is installed, and whether a newer Release exists.
    ///
    /// The line about a newer Release appears only at a terminal, is given two
    /// seconds, and is dropped in silence on any failure, so a machine with no
    /// network loses a line and nothing else. `PERCH_NO_UPGRADE_CHECK` switches
    /// the check off entirely.
    ///
    /// `perch upgrade --check` is the same question asked on purpose: it names
    /// the Channel this Installation came from, waits as long as the answer
    /// takes, and answers a script through `--json`.
    ///
    /// Nothing Perch holds is read or written: no registry, no Credential, no
    /// Profile.
    Version,

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
            // The third writer `commands::say` is the first of: a refusal quotes
            // the Claude Code version, a store's own words and a path read out of
            // a file, and none of the three is Perch's to vouch for.
            let _ = writeln!(
                stderr,
                "{}",
                perch::host::Shown::in_prose(&error.to_string())
            );
            error.exit_code()
        }
    }
}

/// Whether this command comes after a registry migration.
///
/// Three do not: each is what somebody runs when the machine is already
/// misbehaving, and each promises at `--help` to touch nothing Perch holds. A
/// version carried forward on the way past a Probe is a finding it destroyed.
fn migrates(command: &Command) -> bool {
    !matches!(
        command,
        Command::Version | Command::Upgrade(_) | Command::Probe(_)
    )
}

/// Whether this command writes itself down.
///
/// One does not: a Probe renders the Trail, and a line of its own would push
/// what somebody wanted to see out of the window every time they re-ran it
/// (ADR a-trail-is-evidence).
fn leaves_a_trail(command: &Command) -> bool {
    !matches!(command, Command::Probe(_))
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

    let cli = Cli::parse();

    // Not the command's outcome, deliberately (ADR a-registry-comes-forward): an
    // older registry is read correctly either way, so a lock somebody else holds
    // costs the write-back alone and the next run takes it.
    if migrates(&cli.command) {
        let _ = perch::commands::bring_the_registry_forward(&host);
    }

    // After the parse, so a line that was never a command is not written down,
    // and before the dispatch, so a command that hangs has said it started.
    let invocation = leaves_a_trail(&cli.command).then(|| trail::began(&host, &typed));

    let outcome = match cli.command {
        Command::Add(args) => ok(add::run(&host, args, &mut out)),
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
        Command::List(args) => ok(list::run(&host, args, &mut out)),
        Command::Probe(args) => probe::run(&host, args, &mut out),
        Command::Relogin(args) => ok(relogin::run(&host, args, &mut out)),
        Command::Remove(args) => ok(remove::run(&host, args, &mut out)),
        // The one command whose exit code is not Perch's own: what the client
        // said is what a script reads.
        Command::Run(args) => run::run(&host, args, &mut out),
        Command::Status(args) => ok(status::run(&host, args, &mut out)),
        Command::Switch(args) => ok(switch::run(&host, args, &mut out)),
        // What `brew` or `npm` exited with, because a code of Perch's own
        // would lose which of their failures it was.
        Command::Upgrade(args) => upgrade::run(&host, args, &mut out),
        Command::Version => ok(version::run(&host, &mut out)),
        // A `check` reports what it decided, so a scheduler tells a Switch
        // from a figure it could not read without parsing the line
        // (ADR a-watcher-knob-is-arithmetic).
        Command::Watcher { action } => watcher::run(&host, action, &mut out),
    };

    let code = ended_as(outcome, &mut out);
    if let Some(invocation) = &invocation {
        trail::ended(&host, invocation, code);
    }

    let _ = out.flush();
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two exemptions a Probe carries, which are one idea: it changes
    /// nothing about the machine it is describing.
    #[test]
    fn a_probe_neither_migrates_the_registry_nor_writes_itself_down() {
        let probe = Cli::try_parse_from(["perch", "probe"])
            .expect("the line parses")
            .command;
        assert!(!migrates(&probe));
        assert!(!leaves_a_trail(&probe));

        let list = Cli::try_parse_from(["perch", "list"])
            .expect("the line parses")
            .command;
        assert!(migrates(&list));
        assert!(leaves_a_trail(&list), "every other command is written down");
    }

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
            Command::Run(RunArgs { command, .. }) => command,
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
    fn what_is_installed_is_asked_of_a_command_and_no_longer_of_a_flag() {
        assert!(matches!(
            Cli::try_parse_from(["perch", "version"])
                .expect("`perch version` parses")
                .command,
            Command::Version
        ));

        for typed in [["perch", "--version"], ["perch", "-V"]] {
            assert!(
                Cli::try_parse_from(typed).is_err(),
                "`{}` should not parse",
                typed.join(" ")
            );
        }
    }

    /// Both promise at `--help` to touch nothing Perch holds, and a migration is
    /// a read of the registry and a write of it under the lock.
    #[test]
    fn the_two_commands_for_a_misbehaving_machine_skip_the_migration() {
        assert!(!migrates(&Command::Version));
        assert!(!migrates(&Command::Upgrade(UpgradeArgs::default())));
        assert!(migrates(&Command::Status(StatusArgs::default())));
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

    /// The three flags an Add takes, and the pair that cannot be typed together:
    /// `--no-group` says put it nowhere, and a Group to put it in contradicts
    /// that rather than narrowing it.
    #[test]
    fn an_add_takes_a_group_or_no_group_and_never_both() {
        for line in [
            &["perch", "add"][..],
            &["perch", "add", "--group", "work"],
            &["perch", "add", "--no-group"],
            &["perch", "add", "--alias", "dev"],
            &["perch", "add", "--no-group", "--alias", "dev"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "add", "--group", "work", "--no-group"][..],
            &["perch", "add", "someone@example.com"],
            &["perch", "add", "--group"],
            &["perch", "add", "--alias"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// A listing is the one command that takes a Scope as a bare word, and the
    /// fixtures are the three breadths it has: every Account, a Group, and the
    /// Accounts in none. Two Scopes is not a wider listing, it is two questions.
    #[test]
    fn a_listing_takes_one_scope_and_the_two_flags() {
        for line in [
            &["perch", "list"][..],
            &["perch", "list", "work"],
            &["perch", "list", "ungrouped"],
            &["perch", "list", "--refresh"],
            &["perch", "list", "--json"],
            &["perch", "list", "work", "--refresh", "--json"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "list", "work", "ungrouped"][..],
            &["perch", "list", "--group", "work"],
            &["perch", "list", "--scope", "work"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// A status is about the active Account, so naming one is not a narrowing
    /// but a different question — `perch list <scope>` is where a set is asked
    /// for. The two flags are the whole of its surface.
    #[test]
    fn a_status_takes_no_target_and_the_two_flags() {
        for line in [
            &["perch", "status"][..],
            &["perch", "status", "--refresh"],
            &["perch", "status", "--json"],
            &["perch", "status", "--refresh", "--json"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "status", "someone@example.com"][..],
            &["perch", "status", "work"],
            &["perch", "status", "--group", "work"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// The three commands a Target is the whole of. A Switch's is optional
    /// because Perch picks when it is left out; the other two have nothing to
    /// pick from, so an absent Target is a line that does not parse.
    #[test]
    fn the_target_commands_take_one_and_only_a_switch_may_omit_it() {
        for line in [
            &["perch", "switch"][..],
            &["perch", "switch", "work"],
            &["perch", "relogin", "someone@example.com"],
            &["perch", "disable", "dev"],
            &["perch", "enable", "dev"],
            &["perch", "remove", "dev"],
            &["perch", "remove", "dev", "--yes"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "relogin"][..],
            &["perch", "disable"],
            &["perch", "enable"],
            &["perch", "remove"],
            &["perch", "switch", "work", "other"],
            &["perch", "remove", "dev", "other"],
            &["perch", "relogin", "dev", "--yes"],
        ] {
            assert!(
                Cli::try_parse_from(narrowed).is_err(),
                "`{}` should not parse",
                narrowed.join(" ")
            );
        }
    }

    /// The absence of a name *is* `--unset`, which is why the dispatch arm reads
    /// neither the flag nor both together: clap requires a name unless the flag
    /// was passed, and refuses the two of them at once.
    #[test]
    fn an_alias_takes_a_name_or_unset_and_never_neither_or_both() {
        for line in [
            &["perch", "alias", "someone@example.com", "dev"][..],
            &["perch", "alias", "dev", "--unset"],
        ] {
            assert!(
                Cli::try_parse_from(line).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }

        for narrowed in [
            &["perch", "alias", "someone@example.com"][..],
            &["perch", "alias", "someone@example.com", "dev", "--unset"],
            &["perch", "alias", "--unset"],
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
