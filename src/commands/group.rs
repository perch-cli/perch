//! `perch group` — declaring which Accounts are interchangeable (ADR 0002).
//!
//! A Group is the user's statement that Cycling may move them between these
//! Accounts and no others: another work subscription, never their personal one.
//! So it is recorded as something they said rather than inferred from where the
//! Accounts happen to sit — an empty Group is still a Group, and an Account is
//! never quietly dropped out of one.
//!
//! A Group also carries the configuration that would govern Cycling within it.
//! v1 stores and validates those fields and reads none of them; `perch group
//! list` shows them so the rules are visible without opening a config file.

use std::io::Write;

use crate::adopt;
use crate::commands::{say, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, GroupConfig, NO_GROUP, Registry};

/// What was asked of `perch group`. The help each of these is described by
/// lives with the command line that parses it.
#[derive(Debug, Clone)]
pub enum GroupCommand {
    Add {
        name: String,
    },
    Remove {
        name: String,
    },
    /// `group` is the Group to move into, or `none` to leave every Group.
    Move {
        target: String,
        group: String,
    },
    List,
}

const LABEL_WIDTH: usize = 13;

pub fn run(host: &dyn Host, command: GroupCommand, out: &mut dyn Write) -> Result<()> {
    let mut registry = adopt::ensure_adopted(host, out)?;

    match command {
        GroupCommand::Add { name } => {
            registry.declare_group(&name)?;
            registry::save(host, &registry)?;
            say(out, &format!("Declared the Group `{name}`."))?;
            describe_configuration(out, registry.group(&name).expect("just declared"))
        }
        GroupCommand::Remove { name } => {
            remove(&mut registry, &name)?;
            registry::save(host, &registry)?;
            say(out, &format!("Removed the Group `{name}`."))
        }
        GroupCommand::Move { target, group } => {
            let moved = move_account(&mut registry, &target, &group)?;
            registry::save(host, &registry)?;
            say(out, &moved)
        }
        GroupCommand::List => list(out, &registry),
    }
}

/// Forgets a Group, refusing to orphan the Accounts in it.
///
/// The Accounts standing in the way are listed, because which ones they are is
/// the whole of what the user has to decide about.
fn remove(registry: &mut Registry, name: &str) -> Result<()> {
    if registry.group(name).is_none() {
        return Err(no_such_group(registry, name));
    }

    let held: Vec<String> = registry
        .accounts_in(name)
        .iter()
        .map(|account| registry.named_for_the_user(account.email()))
        .collect();
    if !held.is_empty() {
        return Err(PerchError::Conflict(format!(
            "The Group `{name}` still holds {}:\n  {}\nMove them first with `perch group move <target> <group>`, or out of every Group with `perch group move <target> {NO_GROUP}`.",
            accounts_phrase(held.len()),
            held.join("\n  ")
        )));
    }

    registry.forget_group(name);
    Ok(())
}

/// Moves one Account between Groups, leaving everything else about it alone:
/// its Profile, its stored Credential and its Alias are what make removing and
/// re-adding a bad way to do this.
fn move_account(registry: &mut Registry, target: &str, group: &str) -> Result<String> {
    let email = account_named(registry, target)?;
    let destination = if registry::means_no_group(group) {
        None
    } else if registry.group(group).is_some() {
        Some(group.to_string())
    } else {
        return Err(no_such_group(registry, group));
    };

    let account = registry
        .account_mut(&email)
        .expect("the Account was just resolved");
    let previous = account.group.take();
    account.group = destination.clone();

    Ok(match (previous, destination) {
        (Some(from), Some(to)) if from == to => format!("{email} was already in `{to}`."),
        (Some(from), Some(to)) => format!("Moved {email} from `{from}` to `{to}`."),
        (None, Some(to)) => format!("Moved {email} into `{to}`."),
        (Some(from), None) => format!("Moved {email} out of `{from}`. It is now in no Group."),
        (None, None) => format!("{email} was already in no Group."),
    })
}

/// Which Account a target names.
///
/// An Alias first, then the email address itself. A Group is deliberately not
/// an answer here: the thing being moved has to be exactly one Account. The
/// full resolution order shared by every command, and near-match suggestions
/// for a target that resolves to nothing, land with Aliases.
fn account_named(registry: &Registry, target: &str) -> Result<String> {
    if let Some(email) = registry.aliases.get(target) {
        return Ok(email.clone());
    }
    if let Some(account) = registry.account(target) {
        return Ok(account.email().to_string());
    }
    Err(PerchError::NotFound(format!(
        "No Account called `{target}`. `perch group list` shows what Perch holds."
    )))
}

fn no_such_group(registry: &Registry, name: &str) -> PerchError {
    let declared = registry.group_names();
    let known = if declared.is_empty() {
        "No Groups have been declared yet.".to_string()
    } else {
        format!(
            "Groups Perch holds: {}.",
            declared.into_iter().collect::<Vec<_>>().join(", ")
        )
    };
    PerchError::NotFound(format!(
        "No Group called `{name}`. {known}\nDeclare it with `perch group add {name}`."
    ))
}

/// Every Group with what it holds and what governs it, so the rules Cycling
/// will follow are readable without opening the registry.
fn list(out: &mut dyn Write, registry: &Registry) -> Result<()> {
    if registry.groups.is_empty() {
        say(
            out,
            "No Groups yet. `perch group add <name>` declares one, and\n\
             `perch group move <target> <name>` puts an Account in it.\n",
        )?;
    }

    for (name, config) in &registry.groups {
        say(out, name)?;
        let members = registry.accounts_in(name);
        if members.is_empty() {
            write_line(out, "Accounts", "none yet")?;
        } else {
            for (index, account) in members.iter().enumerate() {
                let label = if index == 0 { "Accounts" } else { "" };
                write_line(out, label, &registry.named_for_the_user(account.email()))?;
            }
        }
        describe_configuration(out, config)?;
        say(out, "")?;
    }

    // Ungrouped Accounts are not a Group and are not shown as one, but leaving
    // them out would make this read as a list of every Account when it is not.
    let ungrouped = registry.ungrouped_accounts();
    if !ungrouped.is_empty() {
        say(out, "In no Group")?;
        for (index, account) in ungrouped.iter().enumerate() {
            let label = if index == 0 { "Accounts" } else { "" };
            write_line(out, label, &registry.named_for_the_user(account.email()))?;
        }
        write_line(
            out,
            "Cycling",
            "only moves between these when you say it may",
        )?;
    }

    Ok(())
}

fn describe_configuration(out: &mut dyn Write, config: &GroupConfig) -> Result<()> {
    write_line(out, "Strategy", config.strategy.as_str())?;
    let watcher = if config.watcher_may_act {
        format!(
            "may switch unattended at {}%",
            config.watcher_threshold_percent
        )
    } else {
        format!("off (would act at {}%)", config.watcher_threshold_percent)
    };
    write_line(out, "Watcher", &watcher)
}

fn write_line(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    writeln!(out, "  {label:LABEL_WIDTH$}{value}").map_err(write_failed)
}

fn accounts_phrase(count: usize) -> String {
    if count == 1 {
        "1 Account".to_string()
    } else {
        format!("{count} Accounts")
    }
}
