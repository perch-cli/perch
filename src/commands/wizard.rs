//! `perch wizard` — organizing the Holdings one question at a time.
//!
//! Five steps, each of them a command that exists on its own: the Listing, an
//! Add, a Group move, a Setting, the Service. A step shows what is there and
//! Enter keeps it, so a run that keeps every default changes nothing; a step
//! that wrote prints the typed form it stood for. Steps land as they are
//! answered, so end of input leaves what was done and rolls back nothing.
//!
//! Questions, never a screen: the Wizard is the onboarding moment named and
//! answered without drawing (ADR perch-does-not-draw).

use std::io::Write;

use crate::adopt;
use crate::ask;
use crate::commands::config::{self, ConfigCommand};
use crate::commands::group::{self, GroupCommand};
use crate::commands::list::{self, ListArgs};
use crate::commands::{add, service};
use crate::config::{Scope, Setting};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::name::{self, NO_GROUP};
use crate::registry::Registry;
use crate::say;

/// The Settings a step asks about, in the order the page offers them. The two
/// percentages are left at their defaults: a first run should not ask for a
/// number (ADR a-watcher-knob-is-arithmetic).
const ASKED: [Setting; 4] = [
    Setting::Interchangeable,
    Setting::Strategy,
    Setting::PreferFable,
    Setting::WatcherMayAct,
];

/// Whether the person at the terminal is still there. End of input is the one
/// answer that ends the Wizard, and it ends it between steps rather than
/// undoing one.
enum Step {
    Next,
    Stop,
}

pub fn run(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    needs_a_terminal(host)?;

    list::run(host, ListArgs::default(), out)?;
    for step in [adding, grouping, setting, installing] {
        say::line(out, "")?;
        if let Step::Stop = step(host, out)? {
            say::line(
                out,
                "\nNo answer, so the Wizard stops here. Every step answered so far is kept.",
            )?;
            return Ok(());
        }
    }
    Ok(())
}

/// The refusal names the steps, because a script reading it has the whole job.
fn needs_a_terminal(host: &dyn Host) -> Result<()> {
    if host.is_interactive() {
        return Ok(());
    }
    Err(PerchError::Invalid(
        "There is no terminal to answer questions on. The Wizard's steps, each a \
         command of its own:\n  perch list\n  perch add\n  perch group move <target> \
         <group>\n  perch config set <scope> <key> <value>\n  perch watcher install"
            .to_string(),
    ))
}

/// Step two: Accounts, added one login at a time until the answer is no.
fn adding(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    loop {
        match asked_yes(host, out, "Add an Account? [y/N]: ")? {
            None => return Ok(Step::Stop),
            Some(false) => return Ok(Step::Next),
            Some(true) => {}
        }
        // A login walked away from is said and asked past rather than ending
        // the Wizard: nothing landed, and the next question is the same one.
        match add::run(host, add::AddArgs::default(), out) {
            Ok(()) => typed(out, &["add"])?,
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// Step three: each Account's Group, the current one kept on Enter. A name no
/// Group is declared under declares it on the way, as `perch add --group` does.
fn grouping(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    let held: Vec<(String, Option<String>)> = adopt::ensure_adopted(host)?
        .accounts
        .iter()
        .map(|account| (account.email().to_string(), account.group.clone()))
        .collect();

    for (email, current) in held {
        let question = format!(
            "Group for {email} [{}] (Enter keeps, `{NO_GROUP}` for no Group): ",
            current.as_deref().unwrap_or(NO_GROUP)
        );
        loop {
            let Some(answer) = ask::line(host, out, &question)? else {
                return Ok(Step::Stop);
            };
            let answer = answer.trim();
            if answer.is_empty() {
                break;
            }
            match move_into(host, out, &email, answer) {
                Ok(()) => break,
                Err(refused) => say::line(out, &refused.to_string())?,
            }
        }
    }
    Ok(Step::Next)
}

/// `perch group add` where the name is new, then `perch group move`.
fn move_into(host: &dyn Host, out: &mut dyn Write, email: &str, group: &str) -> Result<()> {
    let undeclared = !name::means_the_ungrouped_scope(group)
        && adopt::ensure_adopted(host)?.declared_group(group).is_none();
    if undeclared {
        group::run(
            host,
            GroupCommand::Add {
                name: group.to_string(),
            },
            out,
        )?;
        typed(out, &["group", "add", group])?;
    }
    group::run(
        host,
        GroupCommand::Move {
            target: email.to_string(),
            group: group.to_string(),
        },
        out,
    )?;
    typed(out, &["group", "move", email, group])
}

/// Step four: the Settings of every Scope that holds an Account.
fn setting(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    let registry = adopt::ensure_adopted(host)?;
    for scope in registry.scopes() {
        if scope.accounts(&registry).is_empty() {
            continue;
        }
        for key in ASKED.into_iter().filter(|key| key.carried_by(&scope)) {
            if let Step::Stop = one_setting(host, out, &registry, &scope, key)? {
                return Ok(Step::Stop);
            }
        }
    }
    Ok(Step::Next)
}

/// One Setting asked, and written through `perch config set` when answered.
fn one_setting(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    key: Setting,
) -> Result<Step> {
    if key == Setting::WatcherMayAct {
        say::line(
            out,
            &format!(
                "The Watcher only observes {} until `{}` is true. Most people turn it on.",
                scope.within(),
                key.as_str()
            ),
        )?;
    }
    let question = format!(
        "`{}` {} [{}] (Enter keeps; {}): ",
        key.as_str(),
        scope.within(),
        key.of(registry, scope),
        key.takes()
    );
    loop {
        let Some(answer) = ask::line(host, out, &question)? else {
            return Ok(Step::Stop);
        };
        let answer = answer.trim();
        if answer.is_empty() {
            return Ok(Step::Next);
        }
        let words = [scope.word(), key.as_str(), answer];
        match config::run(
            host,
            ConfigCommand::Set {
                words: words.iter().map(|word| word.to_string()).collect(),
            },
            out,
        ) {
            Ok(()) => {
                typed(out, &["config", "set", scope.word(), key.as_str(), answer])?;
                return Ok(Step::Next);
            }
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// Step five: the Service. What still gates it is said first, so nobody
/// installs a Service believing it will Switch for them.
fn installing(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    if service::is_there(host) {
        return say::line(
            out,
            "The Watcher runs as a Service. `perch watcher status` says what it is doing.",
        )
        .map(|()| Step::Next);
    }

    let registry = adopt::ensure_adopted(host)?;
    for scope in registry.scopes() {
        if scope.accounts(&registry).is_empty() {
            continue;
        }
        let needed = crate::config::grants_still_needed(&registry, &scope);
        if !needed.is_empty() {
            say::line(
                out,
                &format!(
                    "{} does not let the Watcher act: {} first.",
                    scope.described(),
                    needed.join(" and ")
                ),
            )?;
        }
    }

    match asked_yes(
        host,
        out,
        "Install the Watcher as a Service, starting when you log in? [y/N]: ",
    )? {
        None => Ok(Step::Stop),
        Some(false) => Ok(Step::Next),
        Some(true) => {
            service::install(host, out)?;
            typed(out, &["watcher", "install"])?;
            Ok(Step::Next)
        }
    }
}

/// A yes-or-no with end of input kept apart from no, because here the two mean
/// different things: no is the next step, and nobody is the end of the Wizard.
fn asked_yes(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<bool>> {
    Ok(ask::a_word(host, out, question)?.map(|word| matches!(word.as_str(), "y" | "yes")))
}

/// The command a step stood for, indented and alone, so it can be copied.
fn typed(out: &mut dyn Write, words: &[&str]) -> Result<()> {
    say::line(out, &format!("  perch {}", words.join(" ")))
}
