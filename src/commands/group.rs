//! `perch group` — declaring which Accounts are interchangeable (ADR 0002).
//!
//! A Group is the user's statement that Cycling may move them between these
//! Accounts and no others: another work subscription, never their personal one.
//! So it is recorded as something they said rather than inferred from where the
//! Accounts happen to sit — an empty Group is still a Group, and an Account is
//! never quietly dropped out of one.
//!
//! A Group can be renamed, and the rename keeps what the Group carries: its
//! Overrides, its Accounts, and what the last scheduled Check left behind. Doing
//! it by hand — an add, a move per Account and a remove — would lose every
//! Override the old Group held, which is precisely the part somebody
//! deliberately said.
//!
//! A Group also carries the configuration that would govern Cycling within it.
//! v1 stores and validates those fields and reads none of them; `perch group
//! list` shows them so the rules are visible without opening a config file.

use std::io::Write;

use crate::adopt;
use crate::commands::{CYCLING_AMONG_UNGROUPED, IN_NO_GROUP, config, say, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, NO_GROUP, Registry, Scope};
use crate::target::{self, AccountTarget};

/// What was asked of `perch group`. The help each of these is described by
/// lives with the command line that parses it.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum GroupCommand {
    /// Declare a Group. It starts empty and Inheriting every Setting from
    /// Global — an Override is something a Group has been told, and a new one
    /// has been told nothing.
    Add {
        /// The name, which shares one namespace with Aliases.
        name: String,
    },

    /// Forget a Group. Refused while it still holds Accounts, which are named.
    Remove { name: String },

    /// Rename a Group, keeping its Overrides, its Accounts and the cooldown the
    /// watcher is pacing it by. Nothing about it changes but what it is called.
    Rename {
        /// The Group as it is called now.
        from: String,
        /// What to call it instead. Refused if an Alias or another Group already
        /// answers to it, the same way declaring one is.
        to: String,
    },

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
    // Read-only where it reads. `perch group list` writes nothing, and taking
    // the write lock to read it means waiting out a `perch watch` round or a
    // `perch status --refresh` and then failing as though somebody else had
    // done something wrong.
    if let GroupCommand::List = command {
        return list(out, &adopt::ensure_adopted(host)?);
    }

    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    match command {
        GroupCommand::Add { name } => {
            registry.declare_group(&name)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &format!("Declared the Group `{name}`."))?;
            describe_configuration(out, &registry, &Scope::Group(name.clone()))
        }
        GroupCommand::Remove { name } => {
            let removed = remove(&mut registry, &name)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &format!("Removed the Group `{removed}`."))
        }
        GroupCommand::Rename { from, to } => {
            let renamed = rename(&mut registry, &from, &to)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &renamed)?;
            // What it carries, because keeping it is the whole of what this
            // command is for: the Overrides are the part a rename by hand loses.
            describe_configuration(out, &registry, &Scope::Group(to))
        }
        GroupCommand::Move { target, group } => {
            let account = target::resolve_account(&registry, &target)?;
            let moved = move_account(&mut registry, &account, &group)?;
            registry::save(host, &mut perch, &registry)?;
            say(out, &account.matched)?;
            say(out, &moved)
        }
        // Answered above, before the lock.
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
            crate::commands::accounts(held.len()),
            held.join("\n  ")
        )));
    }

    registry.forget_group(&declared);
    Ok(declared)
}

/// Renames a Group. What moves with the name is
/// [`Registry::rename_group`](Registry::rename_group)'s to say; what is here is
/// the two things the command layer owns.
///
/// The old name is resolved the way every Group named in passing is, so a typo
/// gets the sentence every mistyped Group name gets rather than one of its own.
/// And the report says what the Group still holds, because "did my Accounts come
/// too" is the other thing somebody renaming one wants to know and would
/// otherwise need a second command to find out — said the way `remove` says it,
/// since it is the same question answered from the other side.
fn rename(registry: &mut Registry, from: &str, to: &str) -> Result<String> {
    let held = match registry.declared_group(from) {
        Some(declared) => declared.to_string(),
        None => return Err(no_such_group(registry, from)),
    };

    registry.rename_group(&held, to)?;

    Ok(match registry.accounts_in(to).len() {
        0 => format!("Renamed the Group `{held}` to `{to}`."),
        still_holds => format!(
            "Renamed the Group `{held}` to `{to}`, which still holds {}.",
            crate::commands::accounts(still_holds)
        ),
    })
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

    let account = registry.held_mut(&email)?;
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

    for name in registry.groups.keys().cloned().collect::<Vec<String>>() {
        say(out, &name)?;
        let members = registry.accounts_in(&name);
        if members.is_empty() {
            write_line(out, "Accounts", "none yet")?;
        } else {
            for (index, account) in members.iter().enumerate() {
                let label = if index == 0 { "Accounts" } else { "" };
                write_line(out, label, &registry.named_for_the_user(account.email()))?;
            }
        }
        describe_configuration(out, registry, &Scope::Group(name.clone()))?;
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
        // The rule, and then what it currently answers. The constant alone
        // printed the same words whether the declaration had been made or not,
        // so the one Setting gating the whole Scope was never readable here.
        write_line(
            out,
            "Cycling",
            &format!(
                "{CYCLING_AMONG_UNGROUPED} — `cycle-ungrouped` is {}",
                if registry.global.cycle_ungrouped {
                    "on"
                } else {
                    "off"
                }
            ),
        )?;
        // The Accounts in no Group are a Scope (ADR 0017, amended), so what
        // governs Cycling among them is a thing with an answer rather than a
        // constant compiled into Perch. It is shown here for the same reason a
        // Group's is: the rules Cycling will follow should be readable without
        // opening the registry.
        describe_configuration(out, registry, &Scope::Ungrouped)?;
    }

    Ok(())
}

/// The Settings in force for a Scope, and which of them it declares itself.
///
/// The values are the resolved ones — what Cycling would actually follow — with
/// a line naming what this Scope Overrides, because an Override and an
/// Inheritance that happens to hold the same value are different things: one
/// tracks Global as Global changes and the other does not (ADR 0002, amended).
fn describe_configuration(out: &mut dyn Write, registry: &Registry, scope: &Scope) -> Result<()> {
    let settings = registry.in_force(scope);
    write_line(out, "Strategy", settings.strategy.as_str())?;
    // The whole policy rather than the threshold alone: a summary that named
    // only when the watcher acts would read as the whole of what it does, and
    // the margin is what decides where it lands (ADR 0013).
    let policy = crate::watch::Policy::of(&settings);
    let acting = format!(
        "at {}%, onto {}% or better, at most every {}m",
        policy.threshold,
        policy.ceiling(),
        policy.cooldown_minutes,
    );
    // Being allowed to act is not the whole of whether it does. Among the
    // Accounts in no Group, `cycle-ungrouped` is a separate declaration that
    // they are interchangeable at all (ADR 0017), and without it there is
    // nowhere for the watcher to Switch to — `perch watch` refuses outright and
    // names both. Read from `watcher-may-act` alone, this line claimed
    // unattended switching that the watcher declines, and said the same thing
    // whichever way the gate was set, so it was unfalsifiable in both
    // directions. `config::scope_lines` already answers this correctly.
    let interchangeable = *scope != Scope::Ungrouped || registry.global.cycle_ungrouped;
    let watcher = match (settings.watcher_may_act, interchangeable) {
        (true, true) => format!("may switch unattended {acting}"),
        (true, false) => format!(
            "off — `cycle-ungrouped` is off, so there is nowhere to Switch to \
             (would act {acting})"
        ),
        (false, _) => format!("off (would act {acting})"),
    };
    write_line(out, "Watcher", &watcher)?;

    let declared: Vec<&str> = config::SETTINGS
        .iter()
        .filter(|setting| setting.overridden_at(registry, scope))
        .map(|setting| setting.as_str())
        .collect();
    let overrides = match declared.is_empty() {
        true => "none — every Setting Inherited from Global".to_string(),
        false => declared.join(", "),
    };
    write_line(out, "Overrides", &overrides)
}

fn write_line(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    writeln!(out, "  {label:LABEL_WIDTH$}{value}").map_err(write_failed)
}
