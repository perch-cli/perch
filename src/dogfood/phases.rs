//! The phases, in the order they are proved.
//!
//! Declared in the library rather than in `tests/dogfood.rs` because two things
//! need the list and only one of them is a test: the suite drives it, and
//! `dogfood-setup` reports how many of it this machine could prove. A list in a
//! test binary is a list a binary cannot link.
//!
//! Each phase says what it needs of a machine *before* it runs, so the Preflight
//! can turn "what can this machine prove" into a figure rather than into a pile
//! of skip lines somebody scrolls past. What it needs is counted against the
//! machine the *Repair* left behind, so an Account nobody could log back in is
//! not one this list is measured as able to prove anything with.
//!
//! ## Adding one
//!
//! - It steers **policy, never figures** (ADR 0037). A Threshold set below where
//!   an Account already sits fires a Cycle immediately; burning an Account down
//!   to reach a branch is what the fake suites are for.
//! - It reads `list --json`, `status --json` and exit codes. There is no `perch
//!   state --json` and there will not be one.
//! - It never unwinds itself. A phase that fails stops, says what is now true,
//!   and says what puts it back.
//! - It can tell a fault in Perch from news about something upstream. One that
//!   cannot does not belong here.

use super::{Needs, Perch, Phase, Proof, Setback};

/// The suite, whole.
///
/// One, for now, and chosen for being boring: what the first phase is for is
/// proving the skeleton around it, and a phase that spends quota or moves a
/// Credential proves that no better while costing more to get wrong. The list
/// grows once the skeleton has been proved wrong at least once.
///
/// **The Repair is not here, and never will be.** ADR 0037 opens every run with
/// one, and it is structural rather than an entry on this list: a Quarantine
/// yesterday's run on another machine caused is the ordinary starting state, so
/// it belongs before the phases and outside the stop-at-the-first-failure
/// sequence they run in. It lives in the run loop, and the ordering is the
/// driver's rather than a flag on an entry with exactly one true case forever.
pub const PHASES: &[Phase] = &[Phase {
    name: "the listing and the status agree on which Account is active",
    needs: Needs::THE_ACTIVE_ACCOUNT,
    prove: the_listing_and_the_status_agree,
}];

/// Two commands, one registry, no network and nothing changed.
///
/// Boring on purpose, and it is the boring part that carries the proof: that
/// `perch` runs as a process at all, that argv reaches it, that exit nought
/// means what it says, that `--json` prints something a script can parse, and
/// that two surfaces reading one registry through two renderers agree about
/// which Account somebody is on.
///
/// Every one of those is unasserted everywhere else, because every other suite
/// links the library and calls `run` directly.
fn the_listing_and_the_status_agree(perch: &Perch<'_>) -> Proof {
    let listing = perch.json(&["list", "--json"])?;
    let status = perch.json(&["status", "--json"])?;

    // Read as a script would read them: `perch list --json` names the active
    // Account as a bare address, and `perch status --json` names it inside the
    // Account it is about. The two are named apart deliberately —
    // `active_account` against `active` — so this is also the assertion that
    // they have not quietly been made one shape.
    let listed = listing["active_account"].as_str().ok_or_else(|| {
        Setback::perch(format!(
            "`perch list --json` did not name an active Account: {}",
            listing["active_account"]
        ))
    })?;
    let said = status["active"]["email"].as_str().ok_or_else(|| {
        Setback::perch(format!(
            "`perch status --json` did not name the Account it is about: {}",
            status["active"]
        ))
    })?;

    if listed != said {
        return Err(Setback::perch(format!(
            "`perch list --json` says {listed} is active and `perch status --json` \
             says {said} is. One registry, read twice, two answers."
        )));
    }

    // The same again from the other side of the listing: every Account carries
    // an `active` flag of its own, and exactly one of them may be true. None, or
    // two, is a registry that Cycling would rank from and nobody could read.
    let accounts = listing["accounts"].as_array().ok_or_else(|| {
        Setback::perch("`perch list --json` printed no `accounts` array".to_string())
    })?;
    let flagged: Vec<&str> = accounts
        .iter()
        .filter(|account| account["active"] == serde_json::Value::Bool(true))
        .filter_map(|account| account["email"].as_str())
        .collect();

    if flagged != vec![listed] {
        return Err(Setback::perch(format!(
            "`perch list --json` names {listed} as active, and the Accounts it \
             flags as active are {flagged:?}"
        )));
    }

    // Nothing here is a Setback for being upstream news: both commands render
    // from cache and never touch the network (ADR 0015), so there is no
    // upstream for this phase to be about.
    Ok(vec![
        format!(
            "`perch list --json` exited 0 and named {}",
            crate::commands::accounts(accounts.len())
        ),
        format!("`perch status --json` exited 0 and is about {said}"),
        format!("both agree {said} is active, and it is the only Account flagged"),
    ])
}

/// A phase only ever runs against a real machine, which is the one place its own
/// reading cannot be checked: a phase that quietly agreed with anything would
/// pass on every machine in the matrix and prove none of them.
///
/// So the reading is asserted here, against two canned documents. What is being
/// tested is the phase, not Perch — the documents stand in for what `perch list
/// --json` and `perch status --json` print, and the point is that a disagreement
/// between them is caught rather than passed over.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dogfood::Fault;
    use crate::host::{Execution, FakeHost, Host};

    const BIN: &str = "/build/perch";

    fn printing(listing: &str, status: &str) -> FakeHost {
        let said = |stdout: &str| Execution {
            status: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        };
        FakeHost::new()
            .with_exec(BIN, &["list", "--json"], said(listing))
            .with_exec(BIN, &["status", "--json"], said(status))
    }

    fn listing(active: &str, accounts: &[(&str, bool)]) -> String {
        let listed: Vec<String> = accounts
            .iter()
            .map(|(email, flagged)| format!(r#"{{"email": "{email}", "active": {flagged}}}"#))
            .collect();
        format!(
            r#"{{"active_account": "{active}", "accounts": [{}]}}"#,
            listed.join(", ")
        )
    }

    fn status(email: &str) -> String {
        format!(r#"{{"active": {{"email": "{email}"}}}}"#)
    }

    fn proving(host: &dyn Host) -> Proof {
        the_listing_and_the_status_agree(&Perch::under_test(host, BIN))
    }

    #[test]
    fn two_surfaces_reading_one_registry_and_agreeing_is_what_is_proved() {
        let host = printing(
            &listing(
                "one@example.com",
                &[("one@example.com", true), ("two@example.com", false)],
            ),
            &status("one@example.com"),
        );

        let proved = proving(&host).expect("they agree");

        assert!(proved.iter().any(|line| line.contains("named 2 Accounts")));
        assert!(proved.iter().any(|line| line.contains("one@example.com")));
    }

    #[test]
    fn two_answers_to_which_account_is_active_is_a_fault_in_perch() {
        let host = printing(
            &listing("one@example.com", &[("one@example.com", true)]),
            &status("two@example.com"),
        );

        let setback = proving(&host).expect_err("they do not agree");

        assert_eq!(setback.fault, Fault::Perch);
        assert!(setback.because.contains("One registry, read twice"));
    }

    /// None flagged, or two, is the failure the flag exists to make visible: it
    /// is what Cycling ranks from, and a listing that agrees with `status` while
    /// flagging the wrong row is a listing somebody would act on.
    #[test]
    fn a_listing_that_flags_no_account_or_two_is_caught_even_though_the_two_surfaces_agree() {
        for accounts in [
            &[("one@example.com", false)][..],
            &[("one@example.com", true), ("two@example.com", true)][..],
        ] {
            let host = printing(
                &listing("one@example.com", accounts),
                &status("one@example.com"),
            );

            let setback = proving(&host).expect_err("the flags do not add up");

            assert!(setback.because.contains("flags as active"), "{setback:?}");
        }
    }

    #[test]
    fn a_command_that_failed_is_reported_as_what_it_exited_with() {
        let host = FakeHost::new().with_exec(
            BIN,
            &["list", "--json"],
            Execution {
                status: crate::error::EXIT_NOT_FOUND,
                stdout: String::new(),
                stderr: "Perch holds no Accounts.".to_string(),
            },
        );

        let setback = proving(&host).expect_err("nothing to list");

        assert!(setback.because.contains("exited 12"), "{setback:?}");
        assert!(setback.because.contains("Perch holds no Accounts."));
    }

    #[test]
    fn output_that_is_not_json_is_said_to_be_that_rather_than_read_past() {
        let host = printing("not json at all", &status("one@example.com"));

        let setback = proving(&host).expect_err("that is not a document");

        assert!(setback.because.contains("not JSON"), "{setback:?}");
    }
}
