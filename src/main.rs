use std::io::Write;

use clap::{Parser, Subcommand};

use perch::commands::add::{self, AddArgs};
use perch::commands::status::{self, StatusArgs};
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

    /// Show the active Account and its cached Utilization.
    ///
    /// Renders from cache and never touches the network, so it is cheap enough
    /// for a shell prompt.
    Status {
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
        Command::Status { json } => status::run(&host, StatusArgs { json }, &mut out),
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
