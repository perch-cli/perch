//! `perch disable` / `perch enable` — keeping an Account out of Cycling
//! without giving it up.
//!
//! The narrowest thing Perch can be told about an Account: no longer a Cycle
//! candidate, and that is the whole of it. It stays listed, keeps its Alias,
//! its Group and its Credential, and naming it on `perch switch` still switches
//! to it. The pair is reversible and idempotent.
//!
//! What either half says is what it changed (ADR perch-says-what-it-did) —
//! except a Quarantine, which is promised nothing whatever the pool says.

use std::io::Write;

use crate::commands::only_the_registry;
use crate::error::Result;
use crate::host::Host;
use crate::registry::{self, Quarantine, Registry};
use crate::target::{self, AccountTarget};

/// What was asked. The help each of these is described by lives with the
/// command line that parses it.
#[derive(Debug, Clone)]
pub enum EnableCommand {
    /// `perch enable <target>` — return an Account to the Cycling pool.
    Enable { target: String },
    /// `perch disable <target>` — take one out of it.
    Disable { target: String },
}

impl EnableCommand {
    fn target(&self) -> &str {
        match self {
            EnableCommand::Enable { target } | EnableCommand::Disable { target } => target,
        }
    }
}

pub fn run(host: &dyn Host, command: EnableCommand, out: &mut dyn Write) -> Result<()> {
    only_the_registry(host, out, |registry| {
        let account = target::resolve_account(registry, command.target())?;
        let said = set(registry, &account, &command)?;
        Ok(vec![account.matched, said])
    })
}

/// Moves one Account in or out of the Cycling pool, touching nothing else about
/// it. Returns what to tell the user, which is what changed — and, where the
/// Account is Quarantined, the one thing that makes the change not mean what it
/// says.
fn set(registry: &mut Registry, target: &AccountTarget, command: &EnableCommand) -> Result<String> {
    let named = registry.named_for_the_user(&target.email);
    let candidate = matches!(command, EnableCommand::Enable { .. });

    let account = registry.held_mut(&target.email)?;
    let was_disabled = std::mem::replace(&mut account.disabled, !candidate);
    let quarantine = account.quarantine;

    let changed = match (was_disabled, candidate) {
        (true, false) => format!("{named} was already disabled."),
        (false, true) => format!("{named} was already enabled."),
        (_, false) => format!("Disabled {named}."),
        (_, true) => format!("Enabled {named}."),
    };
    Ok(format!(
        "{changed}{}",
        what_the_quarantine_still_denies(quarantine, &target.email)
    ))
}

/// The one thing about the Account's state the verb did not already say. Whether
/// an Account is in the Cycling pool and whether it works at all are separate
/// facts with separate fixes, so a Quarantine is said whichever way the pair was
/// asked and said in full: an Account that will not be switched to at all is a
/// refusal wearing an outcome's clothes.
fn what_the_quarantine_still_denies(quarantine: Option<Quarantine>, target: &str) -> String {
    match quarantine {
        Some(why) => format!(
            " It is Quarantined, though — {} — so nothing switches to it, \
             Cycling or you. {}",
            why.because(),
            registry::how_to_repair(target),
        ),
        None => String::new(),
    }
}
