//! `perch group` — declaring which Accounts are interchangeable (ADR 0002).
//!
//! A Group is the user's statement that Cycling may move them between these
//! Accounts and no others: another work subscription, never their personal one.
//! So it is recorded as something they said rather than inferred from where the
//! Accounts happen to sit — an empty Group is still a Group, and an Account is
//! never quietly dropped out of one.
//!
//! A Group can be renamed, and the rename keeps what the Group carries: its
//! Settings, its Accounts, and what the last scheduled Check left behind. Doing
//! it by hand — an add, a move per Account and a remove — would lose every
//! Setting the old Group held, which is precisely the part somebody
//! deliberately said.
//!
//! A Group also carries the configuration that would govern Cycling within it.
//! v1 stores and validates those fields and reads none of them; `perch group
//! list` shows them so the rules are visible without opening a config file.

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
    /// watcher already holds somewhere else (ADR 0051).
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
    // Read-only where it reads. `perch group list` writes nothing, and taking
    // the write lock to read it means waiting out a `perch watcher run` round
    // or a `perch status --refresh` and then failing as though somebody else
    // had done something wrong.
    if let GroupCommand::List = command {
        return list(out, &adopt::ensure_adopted(host)?);
    }

    only_the_registry(host, out, |registry| match command {
        GroupCommand::Add { name } => {
            registry.declare_group(&name)?;
            let mut said = vec![format!("Declared the Group `{name}`.")];
            said.extend(configuration_lines(registry, &Scope::Group(name)));
            Ok(said)
        }
        GroupCommand::Remove { name } => {
            let removed = remove(registry, &name)?;
            Ok(vec![format!("Removed the Group `{removed}`.")])
        }
        GroupCommand::Rename { from, to } => {
            let renamed = rename(registry, &from, &to)?;
            let mut said = vec![renamed];
            // What it carries, because keeping it is the whole of what this
            // command is for: the Settings are the part a rename by hand loses.
            said.extend(configuration_lines(registry, &Scope::Group(to)));
            Ok(said)
        }
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
/// `perch config` and `perch list` address Groups too, and answer a typo with
/// this same sentence rather than one of their own.
pub(crate) fn no_such_group(registry: &Registry, name: &str) -> PerchError {
    // A word that could never be a Group is told so, rather than offered a
    // `perch group add` that the next command refuses. `global` is the sharpest
    // case: `registry::validate_name` reserves it *specifically* so that
    // "Declare it with `perch group add global`" can never be made as an
    // offer — and this function, which every one of `group remove`, `group
    // rename` and `group move` reaches, was making it.
    //
    // Asked here rather than at each caller, which is where the two ad-hoc
    // `means_global` guards in `list` and `config` came from: one door, and
    // whatever `validate_name` refuses is refused in its own words.
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

    // Borrowed rather than collected into a `Vec<String>`: nothing here mutates
    // the registry, so the map can be walked directly and the only clone left is
    // the one `Scope::Group` genuinely needs.
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
        // The rule, and then what it currently answers — said by the one
        // function all three surfaces that show this Scope now ask, because
        // three spellings of it had grown and one of them offered `on`/`off`,
        // which is not a value the Setting takes.
        write_line(out, "Cycling", &cycling_among_ungrouped(registry))?;
        // The Accounts in no Group are a Scope (ADR 0017, amended), so what
        // governs Cycling among them is a thing with an answer rather than a
        // constant compiled into Perch. It is shown here for the same reason a
        // Group's is: the rules Cycling will follow should be readable without
        // opening the registry.
        describe_configuration(out, registry, &Scope::Ungrouped)?;
    }

    Ok(())
}

/// The Settings a Scope holds, which are the rules Cycling there will follow.
///
/// No line naming which of them the Scope declared itself, because it declared
/// all of them: a Scope holds its own full Settings and there is nothing above
/// it for one to have come from (ADR 0051).
fn describe_configuration(out: &mut dyn Write, registry: &Registry, scope: &Scope) -> Result<()> {
    for line in configuration_lines(registry, scope) {
        say(out, &line)?;
    }
    Ok(())
}

/// The same two rows as lines, for the callers that cannot write them
/// themselves.
///
/// `perch group add` and `perch group rename` go through
/// [`crate::commands::only_the_registry`] (ADR 0057), which hands the change no
/// writer — so what they have to say comes back as words and is said after the
/// save. The two readers above still write theirs directly, because they have a
/// writer and nothing to save.
fn configuration_lines(registry: &Registry, scope: &Scope) -> Vec<String> {
    let settings = registry.settings(scope);
    let strategy = labeled("Strategy", settings.strategy.as_str());
    // The whole policy rather than the threshold alone: a summary that named
    // only when the watcher acts would read as the whole of what it does, and
    // the margin is what decides where it lands (ADR 0046). Two of the three
    // are constants now, and they are still shown — what the watcher will do
    // here should be readable without knowing which of the numbers is anyone's.
    let policy = crate::watch::Policy::of(&settings);
    let acting = format!(
        "at {}%, onto {}% or better, at most every {}m",
        policy.threshold,
        policy.ceiling(),
        crate::watch::COOLDOWN_MINUTES,
    );
    // Being allowed to act is not the whole of whether it does. Among the
    // Accounts in no Group, `interchangeable` is a separate declaration that
    // they are a set at all (ADR 0017), and without it there is nowhere for the
    // watcher to Switch to — `perch watcher run` refuses outright and names
    // both. Read from `watcher-may-act` alone, this line claimed unattended
    // switching that the watcher declines, and said the same thing whichever way
    // the gate was set, so it was unfalsifiable in both directions.
    // `config::scope_lines` already answers this correctly.
    let interchangeable = crate::cycle::may_cycle_within(registry, scope);
    let watcher = match (settings.watcher_may_act, interchangeable) {
        (true, true) => format!("may switch unattended {acting}"),
        // The Setting's own value, not `off`. `on`/`off` is not a value
        // `interchangeable` takes, so a reader who typed back what they were
        // shown was refused — which is the whole of why
        // `commands::cycling_among_ungrouped` exists, and this line was
        // printing the other spelling two rows below one that uses it.
        (true, false) => format!(
            "off — `{}` is {}, so there is nowhere to Switch to (would act \
             {acting})",
            crate::config::Setting::Interchangeable.as_str(),
            registry.ungrouped.interchangeable
        ),
        (false, _) => format!("off (would act {acting})"),
    };
    vec![strategy, labeled("Watcher", &watcher)]
}

fn write_line(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    say(out, &labeled(label, value))
}

fn labeled(label: &str, value: &str) -> String {
    format!("  {label:LABEL_WIDTH$}{value}")
}
