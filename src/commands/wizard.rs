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

use std::collections::BTreeSet;
use std::io::Write;

use crate::adopt;
use crate::ask;
use crate::commands::config::{self, ConfigCommand};
use crate::commands::group::{self, GroupCommand};
use crate::commands::list::{self, ListArgs};
use crate::commands::selection::Selection;
use crate::commands::{add, service};
use crate::config::{Scope, Setting};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::name::{self, NO_GROUP};
use crate::providers::provider::{Id, catalog};
use crate::registry::Registry;
use crate::say;

/// The Settings a step asks about, in the order the page offers them. The two
/// percentages are left at their defaults: a first run should not ask for a
/// number (ADR a-watcher-knob-is-arithmetic).
const ASKED: [Setting; 4] = [
    Setting::Interchangeable,
    Setting::Strategy,
    Setting::PreferredWorkload,
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
        let Some(provider) = provider_to_add(host, out)? else {
            return Ok(Step::Stop);
        };
        let args = add::AddArgs {
            provider: Selection {
                provider: Some(provider),
                ..Selection::default()
            },
            ..add::AddArgs::default()
        };
        // A login walked away from is said and asked past rather than ending
        // the Wizard: nothing landed, and the next question is the same one.
        match add::run(host, args, out) {
            Ok(()) if provider == Id::default() => typed(out, &["add"])?,
            Ok(()) => typed(out, &["add", &format!("--{}", provider.word())])?,
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// Which provider the login is for, asked only where more than one CLI could
/// take it. Enter is the bare `perch add`, which is Claude whatever
/// `run-provider` says, so the typed form and the kept default agree.
fn provider_to_add(host: &dyn Host, out: &mut dyn Write) -> Result<Option<Id>> {
    let installed = installed_providers(host)?;
    if installed.len() < 2 {
        return Ok(Some(installed.first().copied().unwrap_or_default()));
    }
    let question = format!(
        "Provider for the new Account [{}] (Enter keeps; {}): ",
        Id::default().word(),
        one_of(&installed)
    );
    loop {
        let Some(answer) = ask::a_word(host, out, &question)? else {
            return Ok(None);
        };
        if answer.is_empty() {
            return Ok(Some(Id::default()));
        }
        match Id::parse(&answer) {
            Ok(provider) => return Ok(Some(provider)),
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// The providers whose CLI is on this machine and enabled, in catalog order.
fn installed_providers(host: &dyn Host) -> Result<Vec<Id>> {
    // A provider whose configuration cannot be read is not offered a login,
    // rather than stopping the Wizard before its first question.
    Ok(catalog()
        .iter()
        .map(|adapter| adapter.id())
        .filter(|provider| {
            provider
                .adapter()
                .configured(host)
                .is_ok_and(|configured| configured.installation(host).is_ok())
        })
        .collect())
}

/// Step three: each Account's Group, the current one kept on Enter. A name no
/// Group is declared under declares it on the way, as `perch add --group` does.
fn grouping(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    let registry = adopt::ensure_adopted(host)?;
    let held: Vec<(String, Option<String>)> = registry
        .accounts
        .iter()
        .map(|account| {
            (
                registry.target_of(account.key()).to_string(),
                account.group.clone(),
            )
        })
        .collect();

    for (target, current) in held {
        let question = format!(
            "Group for {target} [{}] (Enter keeps, `{NO_GROUP}` for no Group): ",
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
            match move_into(host, out, &target, answer) {
                Ok(()) => break,
                Err(refused) => say::line(out, &refused.to_string())?,
            }
        }
    }
    Ok(Step::Next)
}

/// `perch group add` where the name is new, then `perch group move`.
fn move_into(host: &dyn Host, out: &mut dyn Write, target: &str, group: &str) -> Result<()> {
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
            target: target.to_string(),
            group: group.to_string(),
        },
        out,
    )?;
    typed(out, &["group", "move", target, group])
}

/// Step four: `run-provider` where both providers' Accounts are held, then the
/// Settings of every Scope a Cycle can choose within. A Scope held only by
/// providers that never Switch live is passed over: no Setting there is read.
fn setting(host: &dyn Host, out: &mut dyn Write) -> Result<Step> {
    let registry = adopt::ensure_adopted(host)?;
    if providers_among(&registry.accounts.iter().collect::<Vec<_>>()).len() > 1
        && let Step::Stop = run_provider(host, out, &registry)?
    {
        return Ok(Step::Stop);
    }
    for scope in registry.scopes() {
        let held = providers_among(&scope.accounts(&registry));
        let cycled: Vec<Id> = held
            .iter()
            .copied()
            .filter(|provider| provider.adapter().capabilities().live_switch)
            .collect();
        if cycled.is_empty() {
            continue;
        }
        for key in ASKED.into_iter().filter(|key| key.carried_by(&scope)) {
            if key != Setting::WatcherMayAct {
                if let Step::Stop = one_setting(host, out, &registry, &scope, key, None)? {
                    return Ok(Step::Stop);
                }
                continue;
            }
            say::line(
                out,
                &format!(
                    "The Watcher only observes {} until `{}` is true. Most people turn it on.",
                    scope.within(),
                    key.as_str()
                ),
            )?;
            // The grant is one provider's in a Scope holding two, and is
            // written with `--provider`, as `perch config set` requires there.
            for provider in &cycled {
                let named = (held.len() > 1).then_some(*provider);
                if let Step::Stop = one_setting(host, out, &registry, &scope, key, named)? {
                    return Ok(Step::Stop);
                }
            }
        }
    }
    Ok(Step::Next)
}

/// The providers these Accounts belong to, each once.
fn providers_among(accounts: &[&crate::registry::Account]) -> BTreeSet<Id> {
    accounts.iter().map(|account| account.provider()).collect()
}

/// The one global the Wizard asks: which CLI a bare `perch run` launches.
fn run_provider(host: &dyn Host, out: &mut dyn Write, registry: &Registry) -> Result<Step> {
    let all: Vec<Id> = catalog().iter().map(|adapter| adapter.id()).collect();
    let question = format!(
        "`run-provider` for a bare `perch run` [{}] (Enter keeps; {}): ",
        registry.run_provider.word(),
        one_of(&all)
    );
    loop {
        let Some(answer) = ask::a_word(host, out, &question)? else {
            return Ok(Step::Stop);
        };
        if answer.is_empty() {
            return Ok(Step::Next);
        }
        let words = ["--global", "run-provider", answer.as_str()];
        match config_set(host, out, &words) {
            Ok(()) => return Ok(Step::Next),
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// One Setting asked, and written through `perch config set` when answered.
/// `provider` is set only in a Scope holding more than one, where the grant
/// has to name whose it is.
fn one_setting(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    key: Setting,
    provider: Option<Id>,
) -> Result<Step> {
    let (subject, current) = match provider {
        Some(provider) => (
            format!("for {} {}", provider.adapter().name(), scope.within()),
            registry
                .resolved_policy(scope, provider)
                .settings
                .watcher_may_act
                .to_string(),
        ),
        None => (scope.within(), key.of(registry, scope)),
    };
    let question = format!(
        "`{}` {subject} [{current}] (Enter keeps; {}): ",
        key.as_str(),
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
        let words: Vec<&str> = match provider {
            Some(provider) => vec![
                scope.word(),
                "--provider",
                provider.word(),
                key.as_str(),
                answer,
            ],
            None => vec![scope.word(), key.as_str(), answer],
        };
        match config_set(host, out, &words) {
            Ok(()) => return Ok(Step::Next),
            Err(refused) => say::line(out, &refused.to_string())?,
        }
    }
}

/// `perch config set` with these words, and the typed form once it landed.
fn config_set(host: &dyn Host, out: &mut dyn Write, words: &[&str]) -> Result<()> {
    config::run(
        host,
        ConfigCommand::Set {
            words: words.iter().map(|word| word.to_string()).collect(),
        },
        out,
    )?;
    let mut typed_words = vec!["config", "set"];
    typed_words.extend_from_slice(words);
    typed(out, &typed_words)
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

/// The providers as the clause a question offers them in: "`claude` or `codex`".
fn one_of(providers: &[Id]) -> String {
    providers
        .iter()
        .map(|provider| format!("`{}`", provider.word()))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The command a step stood for, indented and alone, so it can be copied.
fn typed(out: &mut dyn Write, words: &[&str]) -> Result<()> {
    say::line(out, &format!("  perch {}", words.join(" ")))
}
