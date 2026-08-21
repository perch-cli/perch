//! `perch group` — declaring which Accounts are interchangeable
//! (ADR a-group-is-a-declaration).
//!
//! A Group is recorded as something the user said rather than inferred: an
//! empty Group is still a Group, and an Account is never quietly dropped out of
//! one. A rename keeps everything it carries — its Settings, its Accounts and
//! the cooldown the watcher paces it by — because doing it by hand would lose
//! every Setting somebody said. `perch group list` shows what governs Cycling
//! in a Scope, so the rules are readable without opening a file.

use std::io::Write;

use crate::adopt;
use crate::commands::{IN_NO_GROUP, cycling_among_ungrouped, only_the_registry, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, NO_GROUP, Registry, Scope};
use crate::target::{self, AccountTarget};

/// What was asked of `perch group`. The help each of these is described by
/// lives with the command line that parses it.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum GroupCommand {
    /// Declare a Group. It starts empty and at the compiled-in defaults —
    /// nothing said about another Scope reaches it, including a grant the
    /// watcher already holds somewhere else.
    Add {
        /// The name, which shares one namespace with Aliases.
        name: String,
    },

    /// Forget a Group. Refused while it still holds Accounts, which are named.
    Remove { name: String },

    /// Rename a Group, keeping its Settings, its Accounts and the cooldown the
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
    // `perch group list` writes nothing, and taking the write lock to read
    // means waiting out a `perch watcher run` round and then failing as though
    // somebody else had done something wrong.
    if let GroupCommand::List = command {
        return list(out, &adopt::ensure_adopted(host)?);
    }

    only_the_registry(host, out, |registry| match command {
        // What it declared, and not what governs it: a Group is declared at
        // the compiled-in defaults every time, so those rows are the same on
        // every run (ADR perch-says-what-it-did).
        GroupCommand::Add { name } => {
            registry.declare_group(&name)?;
            Ok(vec![format!("Declared the Group `{name}`.")])
        }
        GroupCommand::Remove { name } => {
            let removed = remove(registry, &name)?;
            Ok(vec![format!("Removed the Group `{removed}`.")])
        }
        // The Accounts it still holds are said by `rename` itself, because a
        // rename could have lost them and did not. Its Settings are not: a
        // rename never touches one.
        GroupCommand::Rename { from, to } => Ok(vec![rename(registry, &from, &to)?]),
        GroupCommand::Move { target, group } => {
            let account = target::resolve_account(registry, &target)?;
            let moved = move_account(registry, &account, &group)?;
            Ok(vec![account.matched, moved])
        }
        // Answered above, before the lock, because it writes nothing and taking
        // the write lock to read is the wait this command refuses to make.
        GroupCommand::List => unreachable!("`perch group list` is answered before the lock"),
    })
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
/// [`Registry::rename_group`](Registry::rename_group)'s to say; the two things
/// here are the command layer's — a typo answered the way every mistyped Group
/// name is, and a report saying what the Group still holds, which is the same
/// question `remove` answers from the other side.
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
/// `perch config` and `perch list` address Groups too, and answer a typo with
/// this same sentence rather than one of their own.
pub(crate) fn no_such_group(registry: &Registry, name: &str) -> PerchError {
    // A word that could never be a Group is told so rather than offered a
    // `perch group add` the next command refuses — `global` above all. Asked
    // here rather than at each caller, in `validate_name`'s own words.
    if let Err(why) = registry::validate_name(registry::NameKind::Group, name) {
        return PerchError::NotFound(format!("No Group called `{name}`. {why}"));
    }

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

    // Borrowed rather than collected: nothing here mutates the registry, so the
    // only clone left is the one `Scope::Group` genuinely needs.
    for name in registry.groups.keys() {
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
        // The rule and then what it currently answers, said by the one function
        // all three surfaces that show this Scope ask — a second spelling of it
        // offers `on`/`off`, which is not a value the Setting takes.
        write_line(out, "Cycling", &cycling_among_ungrouped(registry))?;
        // Shown for the same reason a Group's is: the rules Cycling will follow
        // should be readable without opening the registry.
        describe_configuration(out, registry, &Scope::Ungrouped)?;
    }

    Ok(())
}

/// The Settings a Scope holds, which are the rules Cycling there will follow.
///
/// No line naming which of them the Scope declared itself, because it declared
/// all of them: a Scope holds its own full Settings and there is nothing above
/// it for one to have come from (ADR a-setting-names-its-scope).
fn describe_configuration(out: &mut dyn Write, registry: &Registry, scope: &Scope) -> Result<()> {
    let settings = registry.settings(scope);
    let strategy = labeled("Strategy", settings.strategy.as_str());
    // The whole policy rather than the threshold alone, two of whose three
    // numbers are constants: a summary naming only when the watcher acts would
    // read as the whole of what it does (ADR a-watcher-knob-is-arithmetic).
    let policy = crate::watch::Policy::of(&settings);
    let acting = format!(
        "at {}%, onto {}% or better, at most every {}m",
        policy.threshold,
        policy.ceiling(),
        crate::watch::COOLDOWN_MINUTES,
    );
    // Being allowed to act is not the whole of whether it does: among the
    // Accounts in no Group there is nowhere to Switch to until `interchangeable`
    // says so, and `perch watcher run` refuses outright and names both.
    let interchangeable = crate::cycle::may_cycle_within(registry, scope);
    let watcher = match (settings.watcher_may_act, interchangeable) {
        (true, true) => format!("may switch unattended {acting}"),
        // The Setting's own value, not `off`: `on`/`off` is not a value
        // `interchangeable` takes, so a reader typing back what they were shown
        // would be refused.
        (true, false) => format!(
            "off — `{}` is {}, so there is nowhere to Switch to (would act \
             {acting})",
            crate::config::Setting::Interchangeable.as_str(),
            registry.ungrouped.interchangeable
        ),
        (false, _) => format!("off (would act {acting})"),
    };
    say(out, &strategy)?;
    say(out, &labeled("Watcher", &watcher))
}

fn write_line(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    say(out, &labeled(label, value))
}

fn labeled(label: &str, value: &str) -> String {
    // Measured in cells rather than `char`s, for the reason
    // `utilization::padded` was written: a column counted in characters steps
    // out of line the first time something wide goes through it.
    format!(
        "  {}{value}",
        crate::utilization::padded(label, LABEL_WIDTH)
    )
}
