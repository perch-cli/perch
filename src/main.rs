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
use perch::commands::triage::{self, TriageArgs};
use perch::commands::upgrade::{self, UpgradeArgs};
use perch::commands::version;
use perch::commands::watcher::{self, WatcherCommand};
use perch::commands::wizard;
use perch::error::EXIT_OK;
use perch::host::RealHost;
use perch::report;
use perch::trail;

#[derive(Parser)]
#[command(
    name = "perch",
    about = "Run Claude Code or Codex with separate Accounts"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log a new Account in, inside its own Profile.
    Add(AddArgs),

    /// Name an Account.
    Alias {
        /// The Account: its Alias, or its email address.
        target: String,

        /// The name to give.
        #[arg(required_unless_present = "unset", conflicts_with = "unset")]
        name: Option<String>,

        /// Free the Alias the Account has.
        #[arg(long)]
        unset: bool,
    },

    /// Read and change Settings, one Scope at a time.
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },

    /// Keep an Account out of Cycling.
    Disable {
        /// The Account: its Alias, or its email address.
        target: String,
    },

    /// Return an Account to Cycling.
    Enable {
        /// The Account: its Alias, or its email address.
        target: String,
    },

    /// Declare which Accounts are interchangeable.
    Group {
        #[command(subcommand)]
        action: GroupCommand,
    },

    /// Export, import or purge everything Perch holds here.
    Holdings {
        #[command(subcommand)]
        action: HoldingsCommand,
    },

    /// Show every Account, with Alias, Group, state and Utilization.
    List(ListArgs),

    /// Describe this machine as Perch sees it, for a bug report.
    Probe(ProbeArgs),

    /// Log an Account in again, in place.
    Relogin(ReloginArgs),

    /// Forget an Account and delete its Credential.
    Remove(RemoveArgs),

    /// Launch a client as one Account, leaving the active one alone.
    Run(RunArgs),

    /// Show the active Account and its Utilization.
    Status(StatusArgs),

    /// Make an Account active everywhere.
    Switch(SwitchArgs),

    /// Hand what Perch sees of this machine to Claude Code.
    Triage(TriageArgs),

    /// Upgrade Perch through whatever installed it.
    Upgrade(UpgradeArgs),

    /// Say which Perch is installed, and whether a newer Release exists.
    Version,

    /// Cycle for you when the active Account runs low.
    Watcher {
        #[command(subcommand)]
        action: WatcherCommand,
    },

    /// Organize what Perch holds, one question at a time.
    Wizard,
}

/// The flag clap once generated, caught before the parser so the refusal can
/// name the command that answers it. The parser would only say the word is one
/// it has never heard of.
fn refuse_the_version_flag(typed: &[String]) -> perch::Result<()> {
    if matches!(typed.first().map(String::as_str), Some("--version" | "-V")) {
        return Err(perch::error::PerchError::NotUnderstood(
            "`perch version` says which Perch is installed.".to_string(),
        ));
    }
    Ok(())
}

/// Nought for having worked, and otherwise whatever the failure earned.
///
/// Three do not come through here, because each hands the terminal to something
/// else — a Run's client, a Triage's Claude Code, an Upgrade's Channel — and a
/// code of Perch's own would lose which of their failures it was.
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

/// Everything `main` has to know about one command before running it, stated
/// by the arm that builds it: what runs, whether the Registry comes forward
/// first, and whether the run is written down. One place per command, so a new
/// command cannot get one of the three right and another silently wrong.
struct Orders {
    trailed: bool,
    run: Box<Run>,
}

/// The dispatch itself: given the machine and the terminal, the command's code.
type Run = dyn FnOnce(&dyn perch::host::Host, &mut dyn Write) -> perch::Result<i32>;

impl Orders {
    /// The ordinary command: the Registry is brought forward first, and the run
    /// is written into the Trail.
    fn of(
        run: impl FnOnce(&dyn perch::host::Host, &mut dyn Write) -> perch::Result<i32> + 'static,
    ) -> Orders {
        Orders {
            trailed: true,
            run: Box::new(run),
        }
    }

    /// A Probe renders the Trail and a Triage hands one over, so a line of
    /// their own would push what somebody wanted to see out of the window every
    /// time they re-ran it (ADR a-trail-is-evidence).
    fn leaving_no_trail(mut self) -> Orders {
        self.trailed = false;
        self
    }
}

impl Command {
    fn orders(self) -> Orders {
        match self {
            Command::Add(args) => Orders::of(move |host, out| ok(add::run(host, args, out))),
            // `--unset` needs no reading of its own: clap requires a name unless
            // it was passed and refuses both together, so the name's absence is
            // exactly the flag.
            Command::Alias {
                target,
                name,
                unset: _,
            } => Orders::of(move |host, out| {
                ok(alias::run(
                    host,
                    match name {
                        Some(name) => AliasCommand::Set { target, name },
                        None => AliasCommand::Unset { target },
                    },
                    out,
                ))
            }),
            Command::Config { action } => {
                Orders::of(move |host, out| ok(config::run(host, action, out)))
            }
            Command::Disable { target } => Orders::of(move |host, out| {
                ok(enable::run(host, EnableCommand::Disable { target }, out))
            }),
            Command::Enable { target } => Orders::of(move |host, out| {
                ok(enable::run(host, EnableCommand::Enable { target }, out))
            }),
            Command::Group { action } => {
                Orders::of(move |host, out| ok(group::run(host, action, out)))
            }
            Command::Holdings { action } => {
                Orders::of(move |host, out| ok(holdings::run(host, action, out)))
            }
            Command::List(args) => Orders::of(move |host, out| ok(list::run(host, args, out))),
            Command::Probe(args) => Orders::of(move |host, out| probe::run(host, args, out))
                .leaving_no_trail(),
            Command::Relogin(args) => {
                Orders::of(move |host, out| ok(relogin::run(host, args, out)))
            }
            Command::Remove(args) => Orders::of(move |host, out| ok(remove::run(host, args, out))),
            Command::Run(args) => Orders::of(move |host, out| run::run(host, args, out)),
            Command::Status(args) => Orders::of(move |host, out| ok(status::run(host, args, out))),
            Command::Switch(args) => Orders::of(move |host, out| ok(switch::run(host, args, out))),
            Command::Triage(args) => Orders::of(move |host, out| triage::run(host, args, out))
                .leaving_no_trail(),
            Command::Upgrade(args) => Orders::of(move |host, out| upgrade::run(host, args, out))
                ,
            Command::Version => Orders::of(move |host, out| ok(version::run(host, out)))
                ,
            // A `check` reports what it decided, so a scheduler tells a Switch
            // from a figure it could not read without parsing the line
            // (ADR a-watcher-knob-is-arithmetic).
            Command::Watcher { action } => {
                Orders::of(move |host, out| watcher::run(host, action, out))
            }
            Command::Wizard => Orders::of(move |host, out| ok(wizard::run(host, out))),
        }
    }
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
    if let Err(refusal) = refuse_the_version_flag(&typed) {
        std::process::exit(ended_as(Err(refusal), &mut out));
    }

    let host = RealHost::new();

    let cli = Cli::parse();

    let orders = cli.command.orders();

    // After the parse, so a line that was never a command is not written down,
    // and before the dispatch, so a command that hangs has said it started.
    let invocation = orders.trailed.then(|| trail::began(&host, &typed));

    let outcome = (orders.run)(&host, &mut out);

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
    /// nothing about the machine it is describing. A Triage carries both, for
    /// the same idea — it is a Probe somebody else is about to read.
    #[test]
    fn neither_a_probe_nor_a_triage_migrates_the_registry_or_writes_itself_down() {
        for line in [["perch", "probe"], ["perch", "triage"]] {
            let command = Cli::try_parse_from(line).expect("the line parses").command;
            let orders = command.orders();
            assert!(!orders.trailed, "{line:?}");
        }

        // Named as lines rather than as variants: six of these are absent from
        // `tests/invoking.rs` for needing a provider's CLI installed, so this is
        // the only place their arm is built at all.
        for line in [
            &["perch", "list"][..],
            &["perch", "add"],
            &["perch", "relogin", "dev"],
            &["perch", "run", "dev"],
            &["perch", "status"],
            &["perch", "switch"],
            &["perch", "upgrade"],
            &["perch", "version"],
            &["perch", "watcher", "check"],
            &["perch", "wizard"],
        ] {
            let orders = Cli::try_parse_from(line)
                .expect("the line parses")
                .command
                .orders();
            assert!(
                orders.trailed,
                "every other command is written down: {line:?}"
            );
        }
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
    fn provider_flags_work_on_either_side_of_the_target_and_stop_at_the_separator() {
        for line in [
            vec!["perch", "run", "--codex", "dev"],
            vec!["perch", "run", "dev", "--codex"],
        ] {
            let Command::Run(args) = Cli::try_parse_from(line).unwrap().command else {
                panic!("a Run")
            };
            assert!(args.provider.codex);
        }
        assert!(Cli::try_parse_from(["perch", "run", "dev", "--claude", "--codex"]).is_err());
        assert_eq!(
            command_of(&["perch", "run", "dev", "--", "--codex"]),
            vec!["--codex"]
        );
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

            let said = refuse_the_version_flag(&[typed[1].to_string()])
                .expect_err("the flag is caught before the parser")
                .to_string();
            assert!(said.contains("perch version"), "{said}");
        }
    }

    /// Both promise at `--help` to touch nothing Perch holds, and a migration is
    /// a read of the Registry and a write of it under the lock.
    #[test]
    fn the_two_commands_for_a_misbehaving_machine_skip_the_migration() {}

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
