//! `perch switch [<target>]` — make an Account active everywhere.
//!
//! Every client reads the same Default Profile, so one Switch moves the
//! terminal you are in, the ones you are not, the editor extension and the
//! desktop app together. The work itself — and every refusal that protects it —
//! is [`crate::switch`] — including everything it owes the registry afterwards.
//! What lives here is deciding which Account was meant, declining to do it
//! again when it is already done, and saying where you landed.
//!
//! With no Target the Account is chosen rather than named — a Cycle within the
//! current Account's Group ([`crate::cycle`]), which is the command someone
//! types mid-task when quota just ran out. It asks nothing, under any
//! circumstances (ADR 0011): the interactive picker is a separate command.

use std::io::Write;

use crate::adopt;
use crate::commands::say;
use crate::cycle::{self, Scope};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::Installed;
use crate::registry::{Account, Registry};
use crate::switch::{self, Captured};
use crate::target::{self, Target};
use crate::utilization;

#[derive(Debug, Clone)]
pub struct SwitchArgs {
    /// What to switch to: an Account by Alias or email address, or a Group to
    /// Cycle within. Without one, Perch Cycles within the current Account's
    /// Group.
    pub target: Option<String>,
}

/// The Account to switch to, and what deciding on it left to be said.
struct Decision {
    incoming: Account,
    /// What the figures the choice was made on cannot promise, said once the
    /// Switch has landed (ADR 0015).
    caveat: Option<String>,
}

pub fn run(host: &dyn Host, args: SwitchArgs, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    let Decision { incoming, caveat } = decide(&registry, args.target.as_deref(), host.now(), out)?;
    let outgoing = registry.active_account().cloned();

    // Read once, for the whole command. Both the question below and the Switch
    // after it name the Claude Code they were reading in anything they refuse
    // (ADR 0007), and each used to ask it for itself — so one `perch switch`
    // ran `claude --version` twice, walking `PATH` and spawning a subprocess
    // each time, for a sentence neither of them usually prints.
    let installed = Installed::probed(host)?;

    already_there(host, &installed, &registry, &incoming)?;

    // Everything the Switch owes the registry — the Quarantine it may have
    // discovered, which Account is active now — is written by `record`, which
    // is the only way to reach what the Switch found. What is left here is
    // saying it.
    let landing = switch::perform(host, &mut perch, &installed, &incoming, outgoing.as_ref());
    let captured = landing.record(host, &mut perch, &mut registry)?;

    report(out, &registry, &incoming, &captured, host.now())?;
    match caveat {
        Some(caveat) => say(out, &caveat),
        None => Ok(()),
    }
}

/// Which Account this Switch is for, and how it was arrived at.
///
/// A Target that names a Group is not a Target that names nothing: it names a
/// set of Accounts declared interchangeable, which is exactly what a Cycle
/// needs. So the three forms — an Account, a Group, and no Target at all —
/// differ only in how the Account is arrived at, and the Switch that follows is
/// the same one.
fn decide(
    registry: &Registry,
    target: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
    out: &mut dyn Write,
) -> Result<Decision> {
    let scope = match target {
        Some(target) => {
            let found = target::resolve(registry, target)?;
            say(out, &found.matched())?;
            match found {
                Target::Group { name } => Scope::Group(name),
                Target::Alias { email, .. } | Target::Account { email } => {
                    let incoming = registry.held(&email)?.clone();
                    refuse_a_quarantined_account(registry, &incoming)?;
                    return Ok(Decision {
                        incoming,
                        caveat: None,
                    });
                }
            }
        }
        // Never outside the Group the current Account is in, so a work
        // subscription running dry does not land on a personal Account.
        None => cycle::scope_for(registry, leaving(registry)?)?,
    };

    say(out, &scope.announcement())?;
    // Nothing is set aside: a Cycle somebody asked for is one they get, and the
    // margin and the cooldown are the watcher's rules for acting unasked
    // (ADR 0013) rather than rules about where a Switch may land.
    let choice = cycle::choose(
        registry,
        &scope,
        registry.active.as_deref(),
        &cycle::SetAside::nothing(),
        now,
    )?;
    say(out, &choice.because)?;
    Ok(Decision {
        incoming: choice.account,
        caveat: choice.caveat,
    })
}

/// Refuses to make a Credential live that is known not to work.
///
/// Cycling has never been able to choose a Quarantined Account; naming one is
/// where the user would otherwise find out by losing the session they were in.
/// The refusal is a code of its own because the answer is one of its own: no other
/// refusal in Perch is answered by logging in again, and none of them is
/// answered by trying the same command a second time.
///
/// Reached by the TUI as well, which refuses the keystroke in the frame rather
/// than letting the Switch below discover it: the picker names the Account by
/// cursor, so the refusal has to be the one the command would have given —
/// character for character, from here, rather than a second sentence about the
/// same state.
pub(crate) fn refuse_a_quarantined_account(registry: &Registry, incoming: &Account) -> Result<()> {
    crate::commands::refuse_a_quarantined_account(
        registry,
        incoming.email(),
        "Nothing was changed — switching to it would make a Credential live \
         that no longer works, and cost you the Account you are on.",
    )
}

/// The Account a bare `perch switch` would be leaving, which is the one whose
/// Group decides where it may look.
fn leaving(registry: &Registry) -> Result<&Account> {
    registry.active_account().ok_or_else(|| {
        PerchError::NotFound(
            "No active Account, so there is no Group to Cycle within. Run \
             `claude` and log in, then run Perch again."
                .to_string(),
        )
    })
}

/// The refusal to rewrite Credentials for nothing, when there is nothing to do.
///
/// Perch's own record is not enough to establish that: a Switch interrupted
/// between writing the Credential and patching the Identity is recorded as
/// active while Claude Code still names somebody else, and running the same
/// command again is how that is repaired.
fn already_there(
    host: &dyn Host,
    installed: &Installed,
    registry: &Registry,
    incoming: &Account,
) -> Result<()> {
    if !registry.is_active(incoming.email()) {
        return Ok(());
    }
    if !switch::already_landed(host, installed, incoming)? {
        return Ok(());
    }

    Err(PerchError::NothingToDo(format!(
        "{} is already the active Account. Nothing was changed.",
        registry.named_for_the_user(incoming.email())
    )))
}

fn report(
    out: &mut dyn Write,
    registry: &Registry,
    incoming: &Account,
    captured: &Captured,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    match captured {
        Captured::Copied { from } => say(
            out,
            &format!("Captured {from}'s live Credential into its own Profile."),
        )?,
        // The one case where a Capture was declined rather than found
        // unnecessary, so it says both what was live and what was spared: the
        // Account Perch believed was active keeps the Credential it already
        // held, and the login somebody made outside Perch is about to be
        // replaced without ever having been filed anywhere.
        Captured::NotTheirs { outgoing, live } => say(
            out,
            &format!(
                "The live Credential names {live}, not {outgoing}, so it was not \
                 Captured — {outgoing}'s own Credential is untouched. A login \
                 made outside Perch is not kept: run `perch add` before \
                 switching to keep one."
            ),
        )?,
        // Worth saying, because it is the one case where switching back to that
        // Account will need a login rather than just working.
        Captured::NothingLive => say(
            out,
            "There was no live Credential to Capture — Claude Code was logged out.",
        )?,
        // The live store held something and it was not a Credential. Said
        // rather than swallowed, because the Account being left is now relying
        // on whatever its Profile already held — but not refused either: bytes
        // nothing can read are not a Rotation, and this Switch is what puts a
        // Credential Claude Code can use back in front of it.
        Captured::Unreadable { outgoing, why } => say(
            out,
            &format!(
                "The live Credential could not be read, so it was not Captured \
                 and {outgoing}'s own Credential is untouched: {why}"
            ),
        )?,
        // Also worth saying: whatever was live belonged to no Account Perch
        // holds, so it was replaced rather than kept anywhere.
        Captured::NoOutgoing => say(
            out,
            "Perch held no active Account, so there was nothing to Capture.",
        )?,
        // The repair for a Switch that stopped before it named the Account it
        // had landed on. Nothing was Captured because nothing had moved on:
        // saying so keeps the report honest about a Switch that only patched
        // `.claude.json`.
        Captured::NothingToSave => say(
            out,
            &format!(
                "{}'s Credential was already the live one, so there was nothing \
                 to Capture — this finished a Switch that had stopped before \
                 naming it.",
                incoming.email(),
            ),
        )?,
    }

    say(
        out,
        &format!(
            "Switched to {}.",
            registry.named_for_the_user(incoming.email())
        ),
    )?;

    // What the Switch bought, as of the cache and never from the network
    // (ADR 0015): the figures are shown with their age so a stale one reads as
    // stale rather than as a promise.
    utilization::write_figures(out, incoming, now)
}
