//! `perch switch [<target>]` — make an Account active everywhere.
//!
//! Every client reads the same Default Profile, so one Switch moves the
//! terminal you are in, the ones you are not, the editor extension and the
//! desktop app together. The work itself — and every refusal that protects it —
//! is [`crate::switch`]; what lives here is deciding which Account was meant,
//! declining to do it again when it is already done, recording who is active
//! afterwards, and saying where you landed.
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
use crate::registry::{self, Account, Registry};
use crate::switch::{self, Captured, Interrupted};
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
    let mut registry = adopt::ensure_adopted(host, out)?;

    let Decision { incoming, caveat } = decide(&registry, args.target.as_deref(), host.now(), out)?;
    let outgoing = registry.active_account().cloned();

    if let Some(nothing_to_do) = already_there(host, &registry, &incoming)? {
        return Err(nothing_to_do);
    }

    match switch::perform(host, &incoming, outgoing.as_ref()) {
        Ok(captured) => {
            record_active(host, &mut registry, &incoming)?;
            report(out, &registry, &incoming, &captured, host.now())?;
            match caveat {
                Some(caveat) => say(out, &caveat),
                None => Ok(()),
            }
        }
        Err(Interrupted {
            error,
            incoming_is_live,
        }) => {
            // Which Account is active is a fact about which Credential is in
            // the Default Profile. Recording anything else would send the next
            // Switch to Capture this Credential into the wrong Profile, which
            // is the one mistake this design cannot recover from (ADR 0006).
            //
            // Failing to record it is worth saying and is not worth losing the
            // failure that got us here over: the user is told both, and the
            // exit code stays the one the original failure earned.
            if incoming_is_live
                && let Err(unrecorded) = record_active(host, &mut registry, &incoming)
            {
                return Err(error.with_note(&unrecorded.to_string()));
            }
            Err(error)
        }
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
                    return Ok(Decision {
                        incoming: registry
                            .account(&email)
                            .cloned()
                            .expect("resolution named an Account Perch holds"),
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
    let choice = cycle::choose(registry, &scope, registry.active.as_deref(), now)?;
    say(out, &choice.because)?;
    Ok(Decision {
        incoming: choice.account,
        caveat: choice.caveat,
    })
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
    registry: &Registry,
    incoming: &Account,
) -> Result<Option<PerchError>> {
    if registry.active.as_deref() != Some(incoming.email()) {
        return Ok(None);
    }

    Ok(switch::already_landed(host, incoming)?.then(|| {
        PerchError::NothingToDo(format!(
            "{} is already the active Account. Nothing was changed.",
            registry.named_for_the_user(incoming.email())
        ))
    }))
}

fn record_active(host: &dyn Host, registry: &mut Registry, incoming: &Account) -> Result<()> {
    registry.active = Some(incoming.email().to_string());
    registry::save(host, registry).map_err(|error| {
        error.with_note(&format!(
            "The Switch itself worked: {}'s Credential is the live one. \
             Perch could not record that, so its own view of which Account is \
             active is behind until this is fixed.",
            incoming.email()
        ))
    })
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
        // Worth saying, because it is the one case where switching back to that
        // Account will need a login rather than just working.
        Captured::NothingLive => say(
            out,
            "There was no live Credential to Capture — Claude Code was logged out.",
        )?,
        // Also worth saying: whatever was live belonged to no Account Perch
        // holds, so it was replaced rather than kept anywhere.
        Captured::NoOutgoing => say(
            out,
            "Perch held no active Account, so there was nothing to Capture.",
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
