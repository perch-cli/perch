//! `dogfood-setup` — what marks a machine for Dogfood runs (ADR 0037).
//!
//! Rust rather than a shell script, and for one reason: a bash wizard would need
//! a PowerShell twin for the Windows leg, and twins drift. The Windows
//! credential store is the least-proven belief in the design and therefore the
//! machine that matters most, so it is the last place to run a second
//! implementation of the setup that decides whether it may be tested at all.
//!
//! Held back by `required-features = ["dogfood"]`, so it never reaches the
//! shipped binary — nothing in a release archive has to explain it.
//!
//! ```text
//! cargo run --features dogfood --bin dogfood-setup
//! cargo run --features dogfood --bin dogfood-setup -- --unattended   # CI
//! ```
//!
//! It is thin on purpose. Everything it decides is in `perch::dogfood`, where
//! it can be driven against a fake machine; what is here is the command line and
//! the exit code.

use std::io::Write;
use std::path::PathBuf;

use clap::Parser;
use perch::dogfood::{self, SetupArgs, phases::PHASES};
use perch::error::EXIT_OK;
use perch::host::RealHost;

#[derive(Parser)]
#[command(
    name = "dogfood-setup",
    about = "Take an Export and mark this machine for Dogfood runs"
)]
struct Cli {
    /// Where the Export goes. Asked for at the terminal when it is not given.
    #[arg(long, value_name = "PATH")]
    export_to: Option<PathBuf>,

    /// Ask nothing, take no Export and walk no login.
    ///
    /// For CI, and refused on any machine holding an Account: a Dogfood run
    /// moves real Credentials around, so the machine it runs on is one somebody
    /// set up while watching.
    #[arg(long)]
    unattended: bool,
}

/// The `perch` this same `cargo` invocation built, which is the one the suite
/// will drive.
///
/// `CARGO_BIN_EXE_perch` is set for tests, benches and examples, and not for a
/// binary — so the sibling is found rather than named. Cargo puts every binary
/// of a package in one directory, so `perch` is beside this executable, under
/// whatever profile and target triple built it.
///
/// The sibling is named whether or not it is there, and `set_up` refuses when it
/// is not. It deliberately does **not** fall back to whatever `perch` is on
/// `PATH`: that is very often an installed Perch of another version, and a
/// wizard that quietly set the machine up against a different binary from the
/// one the suite will drive is the failure the marker exists to prevent, wearing
/// a convenience's clothes. `$PERCH_DOGFOOD_BIN` is how somebody asks for that
/// on purpose, in the wizard exactly as in the suite.
///
/// A bare `perch` only where the running executable cannot be located at all,
/// which leaves it to `PATH` because there is nothing better left to say.
fn built_beside_me() -> PathBuf {
    let named = if cfg!(windows) { "perch.exe" } else { "perch" };
    std::env::current_exe()
        .ok()
        .and_then(|me| Some(me.parent()?.join(named)))
        .unwrap_or_else(|| PathBuf::from(named))
}

fn main() {
    perch::report::install_panic_hook();

    let cli = Cli::parse();
    let host = RealHost::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let code = match dogfood::set_up(
        &host,
        &built_beside_me().to_string_lossy(),
        &SetupArgs {
            export_to: cli.export_to,
            unattended: cli.unattended,
        },
        PHASES,
        &mut out,
    ) {
        Ok(_) => EXIT_OK,
        Err(refused) => {
            let _ = out.flush();
            let _ = writeln!(std::io::stderr(), "{refused}");
            refused.exit_code()
        }
    };

    let _ = out.flush();
    std::process::exit(code);
}
