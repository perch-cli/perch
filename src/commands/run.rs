//! `perch run <target>` — a client against one Account's Profile, without a
//! Switch (ADR 0010).
//!
//! One process is pointed at one Profile by setting `CLAUDE_CONFIG_DIR` for it
//! and nothing else. The active Account, every other terminal, the editor
//! extension and the desktop app go on as they were: nothing is Captured,
//! nothing is written to the Default Profile, and no Identity is patched. A Run
//! is not a Switch and shares none of its machinery.
//!
//! Several Runs coexist. Two terminals running two Accounts is what the command
//! is for rather than an edge case, so nothing here may hold anything for as
//! long as the client lives — the registry least of all, which is read and let
//! go before the launch.
//!
//! A Run is the one path where a Profile is a live configuration directory
//! rather than storage, so it is the one path that has to Reconcile
//! ([`crate::reconcile`]) — and it does so before every launch, because Shared
//! State moves underneath it between Runs. A Run that cannot Reconcile does not
//! launch: a client served a Profile it cannot see the person's memory,
//! settings and plugins through is worse than one that did not start.

use std::io::Write;
use std::path::PathBuf;

use crate::adopt;
use crate::commands::{self, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Registry};
use crate::{probe, reconcile, target};

#[derive(Debug, Clone)]
pub struct RunArgs {
    /// The Account to run as: its Alias, or its email address.
    pub target: String,
}

/// Launches a client against the named Account's Profile and reports the status
/// it exited with.
///
/// The status comes back rather than being folded into success or failure: a
/// Run is a way of launching a program, and a wrapper that flattened what the
/// program said would break every script that branches on it.
pub fn run(host: &dyn Host, args: RunArgs, out: &mut dyn Write) -> Result<i32> {
    // Read and let go, never held. A Run lasts as long as somebody's session,
    // and a registry lock held across that would shut every other Perch on the
    // machine out for the afternoon — including the second Run this command
    // exists to make possible.
    let registry = adopt::ensure_adopted(host, out)?;

    // A Group names a set of Accounts declared interchangeable, which is what a
    // Cycle needs and nothing a Run can act on: there is no one Profile to point
    // a process at. Refused here, in the one place every command that acts on
    // exactly one Account refuses it.
    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    refuse_a_quarantined_account(&registry, &found.email)?;

    // Found before anything is linked. A machine with no Claude Code on it is a
    // refusal that should cost the filesystem nothing, and the launch is the
    // only thing Reconcile is preparation for.
    let claude = probe::claude_bin(host)?;
    let profile = registry::profile_dir_for(host, &found.email)?;
    reconcile::reconcile(host, &shared_state(host)?, &profile)?;

    say(out, &launching(&registry, &found.email))?;
    // Flushed before the client is handed the terminal. Everything Perch has to
    // say about a Run is said in front of it, and a buffer that had not been
    // emptied would deliver those lines after the output of the thing they were
    // announcing.
    out.flush().map_err(commands::write_failed)?;

    // The environment of this one process, and the whole of what makes the Run
    // a Run.
    host.exec_interactive(
        &claude.to_string_lossy(),
        &[("CLAUDE_CONFIG_DIR", &profile.to_string_lossy())],
    )
    .map_err(|err| PerchError::Other(format!("could not launch Claude Code: {err}")))
}

/// Where a Run reads Shared State from.
///
/// `CLAUDE_CONFIG_DIR` is honoured, because somebody who moved their
/// configuration directory moved their Shared State along with it — but never
/// when it names a Profile, and pointing it at one is exactly what a Run does.
/// The client a Run launches passes that variable on to everything it starts, so
/// a `perch run` typed inside one inherits it. Reading Shared State out of the
/// Profile it is already in would share links to links, and would share nothing
/// whatever where that Profile is fresh — silently, since a Profile that holds
/// nothing is not an error to enumerate.
///
/// A Profile is where Shared State is made reachable and never where it is read
/// from. That is the whole of the rule.
fn shared_state(host: &dyn Host) -> Result<PathBuf> {
    let told = probe::default_store(host)?.config_dir;
    if told.starts_with(registry::profiles_dir(host)?) {
        return probe::default_config_dir(host);
    }
    Ok(told)
}

/// Refuses to launch a client against an Account whose Credential is known not
/// to work.
///
/// The remedy is the one every Quarantine has and no other refusal in Perch has,
/// so it carries the same exit code: no amount of re-running repairs it, and
/// `perch relogin` does. Without this the user finds out by being asked to log
/// in by a Claude Code that has already taken the terminal.
fn refuse_a_quarantined_account(registry: &Registry, email: &str) -> Result<()> {
    let account = registry
        .account(email)
        .expect("resolution named an Account Perch holds");
    let Some(why) = account.quarantine else {
        return Ok(());
    };
    Err(why.refusal(
        &registry.named_for_the_user(email),
        email,
        "Nothing was launched — the client would open on an Account it cannot \
         authenticate as and ask you to log in.",
    ))
}

/// What is about to happen, and what is not.
///
/// The second half is the whole point of the command, and the difference from
/// the command next to it: somebody who typed `run` where they meant `switch`
/// should be able to see that nothing moved before the client takes the screen.
fn launching(registry: &Registry, email: &str) -> String {
    let named = registry.named_for_the_user(email);
    match registry.active.as_deref() {
        // Both Accounts are named the way every other command names one, so the
        // sentence that contrasts them does not hand one of them its Alias and
        // take the other's away.
        Some(active) if active != email => format!(
            "Running Claude Code as {named}, in this terminal alone. {} stays \
             the active Account everywhere else.",
            registry.named_for_the_user(active)
        ),
        // Running the Account that is already active is not a mistake worth
        // refusing: the Run still gets a Profile of its own, and the session it
        // launches is not the one a later Switch moves out from under.
        _ => format!("Running Claude Code as {named}, in this terminal alone."),
    }
}
