//! `perch disable` / `perch enable` — reserving an Account for a purpose
//! without giving it up.
//!
//! Disabling is the narrowest thing Perch can be told about an Account: it is
//! no longer a Cycle candidate, and that is the whole of it. It stays listed,
//! keeps its Alias, its Group and its stored Credential, and naming it on
//! `perch switch` still switches to it. Removing the Account would be the blunt
//! instrument this exists to avoid — you keep the login you are keeping it for.
//!
//! So the pair is fully reversible and idempotent: `perch enable` puts it back,
//! and asking for a state the Account is already in says so rather than
//! failing, because a script that runs twice has not done anything wrong.
//!
//! Every outcome says what the state now means as well as what changed, because
//! the whole point of the command is a promise about what a disabled Account
//! still is — and that promise is not made about a Quarantined one, whose
//! Credential does not work whatever the Cycling pool says.

use std::io::Write;

use crate::adopt;
use crate::commands::say;
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
    let (_perch, mut registry) = adopt::ensure_adopted_exclusively(host, out)?;

    let account = target::resolve_account(&registry, command.target())?;
    let said = set(&mut registry, &account, &command);
    registry::save(host, &registry)?;

    say(out, &account.matched)?;
    say(out, &said)
}

/// Moves one Account in or out of the Cycling pool, touching nothing else about
/// it. Returns what to tell the user: what changed, and what the state it is
/// now in means for them.
fn set(registry: &mut Registry, target: &AccountTarget, command: &EnableCommand) -> String {
    let named = registry.named_for_the_user(&target.email);
    let candidate = matches!(command, EnableCommand::Enable { .. });

    let account = registry
        .account_mut(&target.email)
        .expect("the Account was just resolved");
    let was = std::mem::replace(&mut account.enabled, candidate);
    let quarantine = account.quarantine;

    let changed = match (was, candidate) {
        (false, false) => format!("{named} was already disabled."),
        (true, true) => format!("{named} was already enabled."),
        (_, false) => format!("Disabled {named}."),
        (_, true) => format!("Enabled {named}."),
    };
    format!(
        "{changed} {}",
        what_that_means(candidate, quarantine, &target.email)
    )
}

/// What the Account's state now means, which is where the honesty lives.
///
/// A disabled Account is promised everything Cycling is not. A Quarantined one
/// is promised nothing its Credential cannot deliver: whether it is in the
/// Cycling pool and whether it works at all are separate facts with separate
/// fixes, and enabling it is not one of them.
fn what_that_means(candidate: bool, quarantine: Option<Quarantine>, target: &str) -> String {
    match (candidate, quarantine) {
        (_, Some(why)) => format!(
            "It is Quarantined, though — {} — so nothing switches to it, Cycling \
             or you. {}",
            why.because(),
            registry::how_to_repair(target),
        ),
        (true, None) => "It is a Cycle candidate again.".to_string(),
        (false, None) => "Cycling will not choose it — it stays listed and named, and `perch \
             switch` still switches to it when you name it."
            .to_string(),
    }
}
