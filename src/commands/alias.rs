//! `perch alias` — naming an Account so no command needs an email address.
//!
//! An Alias shares one namespace with Group names, so a name is refused here
//! if a Group already has it, and refused there if an Account already answers
//! to it. Because the collision cannot exist, the single Target every command
//! takes always has exactly one meaning.
//!
//! An Account answers to one Alias at a time: naming an Account that already
//! has a name replaces it, and says which name it gave up.
//!
//! Both forms are told the Account first and the name second, because the thing
//! a command acts on is its first argument (ADR a-command-names-its-noun). So
//! `--unset` frees whatever Alias the Account it names answers to, which is the
//! same act reached by the Alias itself or by the email address behind it.

use std::io::Write;

use crate::commands::only_the_registry;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, NameKind, Registry};
use crate::target::{self, AccountTarget};

/// What was asked of `perch alias`.
#[derive(Debug, Clone)]
pub enum AliasCommand {
    /// `perch alias <target> <name>` — name an Account.
    Set { target: String, name: String },
    /// `perch alias <target> --unset` — free the name it answers to.
    Unset { target: String },
}

pub fn run(host: &dyn Host, command: AliasCommand, out: &mut dyn Write) -> Result<()> {
    only_the_registry(host, out, |registry| match command {
        AliasCommand::Set { target, name } => {
            registry::validate_name(NameKind::Alias, &name)?;
            let account = target::resolve_account(registry, &target)?;
            let named = set(registry, &name, &account)?;
            Ok(vec![account.matched, named])
        }
        AliasCommand::Unset { target } => {
            let account = target::resolve_account(registry, &target)?;
            let held = unset(registry, &account)?;
            let email = &account.email;
            Ok(vec![
                account.matched.clone(),
                format!("`{held}` no longer names {email}."),
            ])
        }
    })
}

/// Names an Account, refusing anything that would leave two things answering
/// to one name. Returns what to tell the user, which includes the name the
/// Account gave up when it had one.
fn set(registry: &mut Registry, name: &str, account: &AccountTarget) -> Result<String> {
    let previous = registry.name_account(name, &account.email)?;

    let email = &account.email;
    Ok(match previous {
        Some(previous) if previous == name => format!("`{name}` already names {email}."),
        Some(previous) => format!("`{name}` now names {email}, which `{previous}` no longer does."),
        None => format!("`{name}` now names {email}."),
    })
}

/// Frees whatever Alias an Account answers to, and says which name that was —
/// the user need not have typed it. A typo reads as a typo because the Target
/// was resolved before this was reached, so the only thing left to refuse is an
/// Account with no name to give up.
fn unset(registry: &mut Registry, account: &AccountTarget) -> Result<String> {
    match registry.alias_of(&account.email).map(str::to_string) {
        Some(held) => {
            registry.unset_alias(&held);
            Ok(held)
        }
        None => Err(PerchError::NotFound(format!(
            "{} answers to no Alias, so there is none to free.",
            account.email
        ))),
    }
}
