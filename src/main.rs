use std::io::Write;

use clap::{Parser, Subcommand};

use perch::commands::add::{self, AddArgs};
use perch::commands::alias::{self, AliasCommand};
use perch::commands::config::{self, ConfigCommand};
use perch::commands::enable::{self, EnableCommand};
use perch::commands::group::{self, GroupCommand};
use perch::commands::list::{self, ListArgs};
use perch::commands::relogin::{self, ReloginArgs};
use perch::commands::remove::{self, RemoveArgs};
use perch::commands::status::{self, StatusArgs};
use perch::commands::switch::{self, SwitchArgs};
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

    /// Declare which Accounts are interchangeable.
    ///
    /// Cycling only ever moves between Accounts in one Group, so a Group is how
    /// you say that another work subscription is an acceptable landing place
    /// and your personal Account is not.
    Group {
        #[command(subcommand)]
        action: GroupCommand,
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
}

fn main() {
    report::install_panic_hook();

    let cli = Cli::parse();
    let host = RealHost::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let outcome = match cli.command {
        Command::Add {
            group,
            no_group,
            alias,
        } => add::run(
            &host,
            AddArgs {
                group,
                no_group,
                alias,
            },
            &mut out,
        ),
        // `--unset` needs no reading of its own: clap requires a Target unless
        // it was passed, and refuses both together, so the Target's absence is
        // exactly the flag.
        Command::Alias {
            name,
            target,
            unset: _,
        } => alias::run(
            &host,
            match target {
                Some(target) => AliasCommand::Set { name, target },
                None => AliasCommand::Unset { name },
            },
            &mut out,
        ),
        Command::Config { action } => config::run(&host, action, &mut out),
        Command::Disable { target } => {
            enable::run(&host, EnableCommand::Disable { target }, &mut out)
        }
        Command::Enable { target } => {
            enable::run(&host, EnableCommand::Enable { target }, &mut out)
        }
        Command::Group { action } => group::run(&host, action, &mut out),
        Command::List { json } => list::run(&host, ListArgs { json }, &mut out),
        Command::Relogin { target } => relogin::run(&host, ReloginArgs { target }, &mut out),
        Command::Remove { target, yes } => remove::run(&host, RemoveArgs { target, yes }, &mut out),
        Command::Status {
            group,
            refresh,
            json,
        } => status::run(
            &host,
            StatusArgs {
                group,
                refresh,
                json,
            },
            &mut out,
        ),
        Command::Switch { target } => switch::run(&host, SwitchArgs { target }, &mut out),
    };

    let code = match outcome {
        Ok(()) => EXIT_OK,
        Err(error) => {
            let _ = out.flush();
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "{error}");
            error.exit_code()
        }
    };

    let _ = out.flush();
    std::process::exit(code);
}
