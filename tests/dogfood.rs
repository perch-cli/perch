//! Dogfood: the real binary, on a machine somebody works on, against Accounts
//! they actually hold (ADR 0037).
//!
//! Every other suite in this repository buys determinism by replacing
//! something — the filesystem, Perch itself, or the Account. This one replaces
//! nothing, which is why it is the only place `perch` is ever run as a process:
//! argv, the exit codes and every rendered line go unasserted everywhere else.
//!
//! The phases are in `perch::dogfood::phases`, because `dogfood-setup` reports
//! how many of them a machine could prove and a binary cannot link a test. What
//! is here is the driving, and it is **one test**, deliberately: phases run one
//! at a time and the suite never pauses — the human watching is the pause — so
//! there is nothing for a test harness to interleave.
//!
//! Every run opens with a Repair, which walks a browser login for each Account
//! another machine's run left Quarantined. On a machine with nothing Quarantined
//! it passes in a line, and on a runner holding no Accounts there is nothing to
//! repair — which is why this stays green in CI without a terminal.
//!
//! Held back by `required-features = ["dogfood"]`, so `cargo test` does not
//! reach it, and it refuses outright on a machine nothing has marked:
//!
//! ```text
//! cargo run --features dogfood --bin dogfood-setup
//! cargo test --features dogfood --test dogfood -- --nocapture
//!
//! PERCH_DOGFOOD_ATTENDED=1   # take on the phases that hand you the terminal
//! PERCH_DOGFOOD_PHASES=run,cycle   # only the phases whose names contain these
//! ```
//!
//! `--nocapture`, always — but not for the reason it looks like. The Preflight
//! and every phase write through a `Stdout` handle rather than through `print!`,
//! and libtest only captures the macro sink, so the run is legible either way.
//! What the flag buys is that a *passing* run is legible: libtest holds a
//! passing test's captured output back, and a suite that skipped itself and a
//! suite that asserted look identical when neither says anything. That is the
//! failure the Preflight figure exists to prevent.

use std::path::{Path, PathBuf};

use perch::dogfood::{self, phases::PHASES};
use perch::host::RealHost;

/// Where a run's report goes. Gitignored, because a report names real Accounts.
const REPORTS: &str = "dogfood-reports";

#[test]
fn a_dogfood_run() {
    let host = RealHost::new();
    let mut out = std::io::stdout();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(REPORTS);

    let run = match dogfood::dogfood(
        &host,
        env!("CARGO_BIN_EXE_perch"),
        PHASES,
        &reports,
        &mut out,
    ) {
        Ok(run) => run,
        // The unmarked-machine refusal arrives here, and it is the whole reason
        // the suite has one. Printed as itself rather than through a matcher,
        // because it is a paragraph telling somebody what to run next.
        Err(refused) => panic!("\n\n{refused}\n"),
    };

    if let Some((phase, setback)) = run.stopped() {
        panic!("\n\n{phase}\n\n{}\n", setback.said());
    }
}

/// A report names real Accounts, and an Export *is* those Accounts — so neither
/// the directory one goes in nor the other's extension may be committable.
/// Asserted rather than trusted: each is one line somebody could drop while
/// tidying, and the cost is an email address in the history of a public
/// repository at best and a working Credential at worst.
///
/// The Export earns its line here because `dogfood-setup` defaults to writing
/// one in the directory it was run from, and that directory is this repository.
///
/// Here rather than in `tests/publishing.rs` because it is about this suite —
/// and because a check that runs everywhere except on the machines that make the
/// file is a check about nothing.
#[test]
fn what_a_run_leaves_behind_names_real_accounts_or_is_them_so_neither_is_committable() {
    let gitignore =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".gitignore"))
            .expect("this repository has a .gitignore");
    let ignored = |entry: &str| gitignore.lines().any(|line| line.trim() == entry);

    assert!(
        ignored(&format!("/{REPORTS}")),
        "`/{REPORTS}` is not in .gitignore, and a report names real Accounts"
    );
    assert!(
        ignored("*.age"),
        "`*.age` is not in .gitignore, and an Export is a working Credential \
         for every Account this machine holds"
    );
}
