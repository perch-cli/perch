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
use crate::commands::{CYCLING_AMONG_UNGROUPED, IN_NO_GROUP, say, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, GroupConfig, NO_GROUP, Registry};
use crate::target::{self, AccountTarget};

/// What was asked of `perch group`. The help each of these is described by
/// lives with the command line that parses it.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum GroupCommand {
    /// Declare a Group. It starts empty, with the configuration a Group carries
    /// by default: the most-headroom strategy, and the watcher switched off.
    Add {
        /// The name, which shares one namespace with Aliases.
        name: String,
    },

    /// Forget a Group. Refused while it still holds Accounts, which are named.
    Remove { name: String },

    /// Move an Account into a Group, keeping its Profile, Credential and Alias.
    Move {
        /// The Account: its Alias, or its email address.
        target: String,
        /// The Group to move it into, or `none` to leave every Group.
        group: String,
    },

    /// Show every Group with its Accounts and its configuration.
    List,
}

const LABEL_WIDTH: usize = 13;

pub fn run(host: &dyn Host, command: GroupCommand, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    match command {
        GroupCommand::Add { name } => {
            registry.declare_group(&name)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &format!("Declared the Group `{name}`."))?;
            describe_configuration(out, registry.group(&name).expect("just declared"))
        }
        GroupCommand::Remove { name } => {
            let removed = remove(&mut registry, &name)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &format!("Removed the Group `{removed}`."))
        }
        GroupCommand::Move { target, group } => {
            let account = target::resolve_account(&registry, &target)?;
            let moved = move_account(&mut registry, &account, &group)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &account.matched)?;
            say(out, &moved)
        }
        GroupCommand::List => list(out, &registry),
    }
}

/// Forgets a Group, refusing to orphan the Accounts in it.
///
/// The Accounts standing in the way are listed, because which ones they are is
/// the whole of what the user has to decide about.
fn remove(registry: &mut Registry, name: &str) -> Result<String> {
    let declared = match registry.declared_group(name) {
        Some(declared) => declared.to_string(),
        None => return Err(no_such_group(registry, name)),
    };

    let held: Vec<String> = registry
        .accounts_in(&declared)
        .iter()
        .map(|account| registry.named_for_the_user(account.email()))
        .collect();
    if !held.is_empty() {
        return Err(PerchError::Conflict(format!(
            "The Group `{declared}` still holds {}:\n  {}\nMove them first with `perch group move <target> <group>`, or out of every Group with `perch group move <target> {NO_GROUP}`.",
            accounts_phrase(held.len()),
            held.join("\n  ")
        )));
    }

    registry.forget_group(&declared);
    Ok(declared)
}

/// Moves one Account between Groups, leaving everything else about it alone:
/// its Profile, its stored Credential and its Alias are what make removing and
/// re-adding a bad way to do this.
fn move_account(registry: &mut Registry, target: &AccountTarget, group: &str) -> Result<String> {
    let email = target.email.clone();
    let destination = if registry::means_no_group(group) {
        None
    } else if let Some(declared) = registry.declared_group(group) {
        Some(declared.to_string())
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

/// A Group name is not a Target — only a Group will do here — but a typo is a
/// typo wherever it is made, so it is answered the same way one in a Target
/// is: with what the user probably meant, and otherwise with what they hold.
/// `perch config` addresses Groups too, and answers a typo with this same
/// sentence rather than one of its own.
pub(crate) fn no_such_group(registry: &Registry, name: &str) -> PerchError {
    let declared: Vec<String> = registry.groups.keys().cloned().collect();
    let help = match target::suggestion(&declared, name) {
        // A near miss is a typo far more often than it is a Group somebody
        // meant to declare, so it is not also handed a way to create the typo.
        Some(suggestion) => suggestion,
        None if declared.is_empty() => {
            format!("No Groups have been declared yet. Declare it with `perch group add {name}`.")
        }
        None => format!(
            "Groups Perch holds: {}.\nDeclare it with `perch group add {name}`.",
            declared.join(", ")
        ),
    };
    PerchError::NotFound(format!("No Group called `{name}`. {help}"))
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
        say(out, IN_NO_GROUP)?;
        for (index, account) in ungrouped.iter().enumerate() {
            let label = if index == 0 { "Accounts" } else { "" };
            write_line(out, label, &registry.named_for_the_user(account.email()))?;
        }
        write_line(out, "Cycling", CYCLING_AMONG_UNGROUPED)?;
    }

    Ok(())
}

fn describe_configuration(out: &mut dyn Write, config: &GroupConfig) -> Result<()> {
    write_line(out, "Strategy", config.strategy.as_str())?;
    // The whole policy rather than the threshold alone: a summary that named
    // only when the watcher acts would read as the whole of what it does, and
    // the margin is what decides where it lands (ADR 0013).
    let policy = crate::watch::Policy::of(config);
    let acting = format!(
        "at {}%, onto {}% or better, at most every {}m",
        policy.threshold,
        policy.ceiling(),
        policy.cooldown_minutes,
    );
    let watcher = match config.watcher_may_act {
        true => format!("may switch unattended {acting}"),
        false => format!("off (would act {acting})"),
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
