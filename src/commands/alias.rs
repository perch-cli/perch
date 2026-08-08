//! `perch alias` — naming an Account so no command needs an email address.
//!
//! An Alias shares one namespace with Group names, so a name is refused here
//! if a Group already has it, and refused there if an Account already answers
//! to it. Because the collision cannot exist, the single Target every command
//! takes always has exactly one meaning.
//!
//! An Account answers to one Alias at a time: naming an Account that already
//! has a name replaces it, and says which name it gave up.

use std::io::Write;

use crate::adopt;
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, NameKind, Registry};
use crate::target::{self, AccountTarget};

/// What was asked of `perch alias`.
#[derive(Debug, Clone)]
pub enum AliasCommand {
    /// `perch alias <name> <target>` — name an Account.
    Set { name: String, target: String },
    /// `perch alias <name> --unset` — free a name.
    Unset { name: String },
}

pub fn run(host: &dyn Host, command: AliasCommand, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    match command {
        AliasCommand::Set { name, target } => {
            registry::validate_name(NameKind::Alias, &name)?;
            let account = target::resolve_account(&registry, &target)?;
            let named = set(&mut registry, &name, &account)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &account.matched)?;
            say(out, &named)
        }
        AliasCommand::Unset { name } => {
            let (held, email) = unset(&mut registry, &name)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &format!("`{held}` no longer names {email}."))
        }
    }
}

/// Names an Account, refusing anything that would leave two things answering
/// to one name. Returns what to tell the user, which includes the name the
/// Account gave up when it had one.
fn set(registry: &mut Registry, name: &str, account: &AccountTarget) -> Result<String> {
    let previous = registry.alias_of(&account.email).map(str::to_string);

    // Renaming an Account by the name it already answers to is not a collision
    // with itself, so the name it is about to give up is not in the way — and
    // that includes recapitalising it, which is a rename like any other.
    let renaming_itself = previous
        .as_deref()
        .is_some_and(|held| registry::same_name(held, name));
    if !renaming_itself {
        registry.refuse_taken_names(Some(name), None)?;
    }
    registry.set_alias(name, &account.email);

    let email = &account.email;
    Ok(match previous {
        Some(previous) if previous == name => format!("`{name}` already names {email}."),
        Some(previous) => format!("`{name}` now names {email}, which `{previous}` no longer does."),
        None => format!("`{name}` now names {email}."),
    })
}

/// Frees a name. A name that reaches nothing is refused the same way any
/// unresolvable Target is, so a typo reads as a typo.
fn unset(registry: &mut Registry, name: &str) -> Result<(String, String)> {
    match registry.unset_alias(name) {
        Some(freed) => Ok(freed),
        None => Err(match target::resolve(registry, name) {
            Ok(matched) => PerchError::NotFound(format!(
                "There is no Alias called `{name}` — {}",
                matched.matched()
            )),
            Err(unresolvable) => unresolvable,
        }),
    }
}
