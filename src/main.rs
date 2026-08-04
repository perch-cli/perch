use std::io::Write;

use clap::{Parser, Subcommand};

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
