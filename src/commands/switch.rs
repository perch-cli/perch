//! `perch switch [<target>]` — make an Account active everywhere.
//!
//! Every client reads the same Default Profile, so one Switch moves the terminal
//! you are in, the ones you are not, the editor extension and the desktop app
//! together. The work itself and every refusal that protects it is
//! [`crate::switch`]'s; what lives here is deciding which Account was meant,
//! declining to do it again, and saying where you landed. With no Target the
//! Account is chosen rather than named — a Cycle within the current Account's
//! Group ([`crate::cycle`]) — and it asks nothing, ever
//! (ADR perch-does-not-draw).

use std::io::Write;

use crate::adopt;
use crate::commands::say;
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::Installed;
use crate::registry::{Account, Registry, Scope};
use crate::switch::{self, Captured, Switched};
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
    /// What a Cycle chose this Account on, and the Scope it stayed inside, ready
    /// to go on the end of the landing line. Absent when somebody named the
    /// Account, because then nothing was chosen (ADR perch-says-what-it-did).
    chosen: Option<String>,
}

pub fn run(host: &dyn Host, args: SwitchArgs, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    // Before anything is decided, because everything after this reads which
    // Account is active and a registry holding a Landing does not know
    // (ADR a-switch-is-written-down-first).
    crate::commands::a_settled_landing(host, &mut perch, &mut registry)?;

    let Decision { incoming, chosen } = decide(&registry, args.target.as_deref(), host.now(), out)?;
    let outgoing = registry.active_account().cloned();

    // Read once, for the whole command: both the question below and the Switch
    // after it name the Claude Code they were reading in anything they refuse
    // (ADR an-assumption-is-probed).
    let installed = Installed::probed(host)?;

    already_there(host, &installed, &registry, &incoming)?;

    // Everything the Switch owes the registry is written by `switch_to`, which
    // is the only way to reach what the Switch found. `Reason::Asked` is all
    // this caller differs by: somebody typed it, so nothing paces anything.
    let Switched { captured, .. } = switch::switch_to(
        host,
        &mut perch,
        &mut registry,
        &installed,
        &incoming,
        outgoing.as_ref(),
        switch::Reason::Asked,
    )?;

    report(
        out,
        &registry,
        &incoming,
        chosen.as_deref(),
        &captured,
        host.now(),
    )
}

/// Which Account this Switch is for, and how it was arrived at.
///
/// A Target naming a Group names a set of Accounts declared interchangeable,
/// which is what a Cycle needs — so the three forms differ only in how the
/// Account is arrived at, and the Switch that follows is the same one.
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
                        chosen: None,
                    });
                }
            }
        }
        // Never outside the Group the current Account is in, so a work
        // subscription running dry does not land on a personal Account.
        None => cycle::scope_for(registry, leaving(registry)?)?,
    };

    // Nothing is set aside: a Cycle somebody asked for is one they get, and the
    // margin and the cooldown are the Watcher's rules for acting unasked
    // (ADR a-watcher-knob-is-arithmetic).
    let choice = cycle::choose(
        registry,
        &scope,
        registry.active().whose(),
        &cycle::SetAside::nothing(),
        now,
    )?;
    Ok(Decision {
        chosen: Some(choice.basis.in_the(&scope)),
        incoming: choice.account,
    })
}

/// Refuses to make a Credential live that is known not to work.
///
/// Cycling has never been able to choose a Quarantined Account; naming one is
/// where the user would otherwise find out by losing the session they were in. A
/// code of its own, because no other refusal is answered by logging in again.
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
        crate::commands::no_active_account(registry, ", so there is no Group to Cycle within")
    })
}

/// The refusal to rewrite Credentials for nothing, when there is nothing to do.
///
/// Perch's own record is not enough to establish that: a Switch interrupted
/// between writing the Credential and patching the Identity is recorded as
/// active while Claude Code still names somebody else.
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
    chosen: Option<&str>,
    captured: &Captured,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    match captured {
        // Said by nothing, because it happens before every Switch without
        // exception — the ordinary case announcing that it was ordinary. The
        // reassurance is the guide's to give once.
        Captured::Copied { .. } => {}
        // The one case where a Capture was declined rather than found
        // unnecessary, so it says what was live and what was spared.
        Captured::NotTheirs { outgoing, live } => say(
            out,
            &format!(
                "The live Credential names {live}, not {outgoing}, so it was not \
                 Captured — {outgoing}'s own Credential is untouched. A login \
                 made outside Perch is not kept: run `perch add` before \
                 switching to keep one."
            ),
        )?,
        // The one case where switching back to that Account needs a login
        // rather than just working.
        Captured::NothingLive => say(
            out,
            "There was no live Credential to Capture — Claude Code was logged out.",
        )?,
        // The live store held something that was not a Credential. Said rather
        // than swallowed, and not refused either: bytes nothing can read are not
        // a Rotation, and this Switch puts a usable Credential back in front.
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
        // had landed on. Nothing was Captured because nothing had moved on.
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

    // Where it landed, and — where the Account was chosen rather than named —
    // what it was chosen on and the Scope the Cycle stayed inside. One line,
    // because the ranking is not worth defending.
    let named = registry.named_for_the_user(incoming.email());
    say(
        out,
        &match chosen {
            Some(chosen) => format!("Switched to {named}, {chosen}."),
            None => format!("Switched to {named}."),
        },
    )?;

    // As of the cache and never from the network
    // (ADR a-figure-carries-its-age): the figures are shown with their age, so a
    // stale one reads as stale rather than as a promise.
    utilization::write_figures(out, incoming, now)
}
