//! `perch config` — changing the rules Perch chooses Accounts by, from a
//! script.
//!
//! The **grammar** and the page: which word goes where, which form somebody
//! seems to have meant when the words were not one, and each Scope's Settings
//! laid out under its name. What a Setting *is* is [`crate::config`]'s,
//! because surfaces that are not this command name keys too.
//!
//! Every `set` is `<scope> <key> <value>` and reading is not writing
//! (ADR a-setting-names-its-scope).

use std::io::Write;

use crate::column::{self, Labeled};
use crate::commands::group;
use crate::config::Scope;
use crate::config::{NotAScope, SETTINGS, Setting};
use crate::error::{PerchError, Result};
use crate::host::{Host, Shown};
use crate::name::UNGROUPED;
use crate::registry::Registry;
use crate::say;

/// What was asked of `perch config`, as the words that were typed. Carried
/// rather than resolved, because telling somebody which form they seem to have
/// meant is part of what this command does, and a parser that had thrown the
/// words away could not.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ConfigCommand {
    /// Set one Setting on one Scope.
    #[command(long_about = how_a_setting_is_set())]
    Set {
        /// `<scope> <key> <value>`
        #[arg(value_name = "WORDS", num_args = 1.., required = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },

    /// Read Settings back.
    Get {
        /// Nothing, `<scope>`, or `<scope> <key>`
        #[arg(value_name = "WORDS", num_args = 0.., allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

pub fn run(host: &dyn Host, command: ConfigCommand, out: &mut dyn Write) -> Result<()> {
    // Taken inside the match, so only by the half that writes: a reader that
    // takes the write lock waits out whatever holds it and then fails with
    // "another `perch` holds it".
    match command {
        ConfigCommand::Set { words } => {
            let mut held = crate::holdings::lock(host)?;
            let mut registry = crate::registry::load(host)?.unwrap_or_default();
            let lines = set(&mut registry, &words)?;
            crate::registry::save(host, &mut held, &mut registry)?;
            for line in lines {
                say::line(out, &line)?;
            }
            Ok(())
        }
        ConfigCommand::Get { words } => {
            let registry = crate::registry::load(host)?.unwrap_or_default();
            for line in get(&registry, &words)? {
                say::line(out, &line)?;
            }
            Ok(())
        }
    }
}

/// Sets one Setting, returning what to tell the user: what it is now, and what
/// that means for them.
fn set(registry: &mut Registry, words: &[String]) -> Result<Vec<String>> {
    if let [scope, selector, provider, key, value] = words
        && selector == "--provider"
    {
        let scope = addressed(registry, scope)?;
        let provider = crate::providers::provider::Id::parse(provider)?;
        let mut changed = registry.clone();
        let settings = changed
            .scope_settings_mut(&scope)
            .ok_or_else(|| PerchError::NotFound("Scope disappeared".into()))?;
        let local = settings.providers.entry(provider).or_default();
        match key.as_str() {
            "strategy" => local.cycle.strategy = inherited(value, crate::config::strategy)?,
            "watcher-threshold-percent" => {
                local.watcher.threshold_percent =
                    inherited(value, |value| crate::config::percentage(key, value))?
            }
            "watcher-margin-percent" => {
                local.watcher.margin_percent =
                    inherited(value, |value| crate::config::margin(key, value))?
            }
            "watcher-may-act" => local.watcher.enabled = crate::config::yes_or_no(key, value)?,
            key if key.starts_with("option.") => {
                let key = key.trim_start_matches("option.");
                if value == "inherit" {
                    local.options.remove(key);
                } else {
                    local.options.insert(
                        key.into(),
                        serde_json::from_str(value).unwrap_or_else(|_| value.clone().into()),
                    );
                }
            }
            _ => return Err(PerchError::Invalid("A provider's Scope Settings are `strategy`, `watcher-threshold-percent`, `watcher-margin-percent`, `watcher-may-act` and `option.<name>`.".into())),
        }
        crate::registry::validate(&changed)?;
        *registry = changed;
        return Ok(vec![format!(
            "{} {} {key}: {value}",
            scope.word(),
            provider.word()
        )]);
    }
    if let [selector, key, value] = words
        && selector == "--defaults"
    {
        let mut changed = registry.clone();
        match key.as_str() {
            "strategy" => changed.scope_defaults.cycle.strategy = inherited(value, crate::config::strategy)?,
            "watcher-threshold-percent" => changed.scope_defaults.watcher.threshold_percent = inherited(value, |value| crate::config::percentage(key, value))?,
            "watcher-margin-percent" => changed.scope_defaults.watcher.margin_percent = inherited(value, |value| crate::config::margin(key, value))?,
            _ => return Err(PerchError::Invalid("`--defaults` takes `strategy`, `watcher-threshold-percent` or `watcher-margin-percent`. `perch config set <scope> watcher-may-act <value>` grants per Scope.".into())),
        }
        crate::registry::validate(&changed)?;
        *registry = changed;
        return Ok(vec![format!("Scope default {key}: {value}")]);
    }
    if words.first().is_some_and(|word| word == "--provider") {
        let [_, provider, key, value] = words else {
            return Err(PerchError::Invalid(
                "`perch config set --provider <name> <enabled|cli-path> <value>` sets a provider's Installation.".into(),
            ));
        };
        let provider = crate::providers::provider::Id::parse(provider)?;
        let settings = registry.provider_settings.entry(provider).or_default();
        match key.as_str() {
            "enabled" => {
                settings.enabled = value
                    .parse()
                    .map_err(|_| PerchError::Invalid("`enabled` takes `true` or `false`.".into()))?
            }
            "cli-path" => settings.cli_path = (value != "auto").then(|| value.into()),
            _ => {
                return Err(PerchError::Invalid(
                    "A provider's Installation Settings are `enabled` and `cli-path`.".into(),
                ));
            }
        }
        return Ok(vec![format!("{} {key}: {value}", provider.word())]);
    }

    if words.first().is_some_and(|word| word == "--global") {
        return match &words[1..] {
            [key, value] if key == "run-provider" => {
                registry.run_provider = crate::providers::provider::Id::parse(value)?;
                Ok(vec![format!(
                    "run-provider: {}",
                    registry.run_provider.word()
                )])
            }
            [key, value] if key == "run-fallback" => {
                registry.run_fallback = match value.as_str() {
                    "installed" => true,
                    "disabled" => false,
                    _ => {
                        return Err(PerchError::Invalid(
                            "`run-fallback` takes `installed` or `disabled`.".into(),
                        ));
                    }
                };
                Ok(vec![format!("run-fallback: {value}")])
            }
            [key, value] if key == "watcher-paused" => {
                registry.watcher_paused = crate::config::yes_or_no(key, value)?;
                Ok(vec![format!("watcher-paused: {value}")])
            }
            _ => Err(PerchError::Invalid(
                "The global Settings are `run-provider`, `run-fallback` and `watcher-paused`."
                    .into(),
            )),
        };
    }

    match words {
        [scope, key, value] => {
            let scope = addressed(registry, scope)?;
            if value == "inherit" {
                let mut changed = registry.clone();
                let settings = changed.scope_settings_mut(&scope).unwrap();
                match key.as_str() {
                    "strategy" => settings.cycle.strategy = None,
                    "watcher-threshold-percent" => settings.watcher.threshold_percent = None,
                    "watcher-margin-percent" => settings.watcher.margin_percent = None,
                    _ => {
                        return Err(PerchError::Invalid(format!(
                            "`{key}` takes no `inherit`. `perch config set {} {key} <value>` sets it.",
                            scope.word()
                        )));
                    }
                }
                crate::registry::validate(&changed)?;
                *registry = changed;
                return Ok(vec![format!("{} {key}: inherited", scope.word())]);
            }
            let key = Setting::parse(key, &scope)?;
            if key == Setting::WatcherMayAct {
                let providers: std::collections::BTreeSet<_> = registry
                    .accounts
                    .iter()
                    .filter(|account| registry.scope_of(account) == scope)
                    .map(|account| account.provider())
                    .collect();
                if providers.len() > 1 {
                    return Err(PerchError::Invalid(format!(
                        "{} holds both providers' Accounts, so the grant names one: `perch \
                         config set {} --provider <claude|codex> watcher-may-act <value>`.",
                        scope.described(),
                        scope.word()
                    )));
                }
                registry.select_provider(providers.into_iter().next().unwrap_or_default());
            }
            let was = key.of(registry, &scope);

            key.write(registry, &scope, value)?;

            let now = key.of(registry, &scope);
            Ok(vec![
                changed(
                    &format!("`{}` on {}", key.as_str(), scope.mentioned()),
                    &was,
                    &now,
                ),
                key.what_that_means(registry, &scope),
            ])
        }
        [first, second] => match addressed(registry, first) {
            // The key is parsed first, so a mistyped one is answered as what it
            // is: told only that the *value* is missing, somebody adds one, runs
            // it again, and only then learns what the mistake was.
            Ok(scope) => {
                Setting::parse(second, &scope)?;
                Err(PerchError::Invalid(format!(
                    "`perch config set {first} {second}` names no value. `perch \
                     config set {first} {second} <value>` sets one.",
                )))
            }
            // A key where the Scope goes is a Setting with no subject, and
            // there is no everywhere for it to have been about.
            Err(_) if Setting::parse_quietly(first).is_some() => {
                Err(no_scope_was_named(registry, first, second))
            }
            // Handed back as it came rather than recast as a key: `perch config
            // set wrok strategy` is a Group typo, and being offered `wrok` as a
            // Setting sends somebody looking for the wrong mistake.
            Err(refusal) => Err(refusal),
        },
        _ => Err(how_set_is_addressed(registry, words)),
    }
}

/// Reads Settings back: pages for a Scope or for all of them, and the bare
/// value where the words already name the rest of the line.
fn get(registry: &Registry, words: &[String]) -> Result<Vec<String>> {
    if let [selector, rest @ ..] = words
        && selector == "--effective"
    {
        let (scope, provider) = match rest {
            [scope, flag, provider] if flag == "--provider" => (
                addressed(registry, scope)?,
                crate::providers::provider::Id::parse(provider)?,
            ),
            [scope] => (addressed(registry, scope)?, registry.selected_provider()),
            _ => {
                return Err(PerchError::Invalid(
                    "`perch config get --effective <scope> [--provider <name>]` shows where each value comes from.".into(),
                ));
            }
        };
        let resolved = registry.resolved_policy(&scope, provider);
        let values = [
            ("strategy", resolved.settings.strategy.as_str().to_string()),
            (
                "watcher-threshold-percent",
                resolved.settings.watcher_threshold_percent.to_string(),
            ),
            (
                "watcher-margin-percent",
                resolved.settings.watcher_margin_percent.to_string(),
            ),
            (
                "watcher-may-act",
                resolved.settings.watcher_may_act.to_string(),
            ),
        ];
        let mut lines: Vec<_> = values
            .iter()
            .map(|(key, value)| format!("{key} {value} ({})", resolved.sources[key]))
            .collect();
        lines.push(format!(
            "watcher-paused {} (global)",
            registry.watcher_paused
        ));
        return Ok(lines);
    }
    if let [scope, selector, provider, rest @ ..] = words
        && selector == "--provider"
    {
        let scope = addressed(registry, scope)?;
        let mut contextual = registry.clone();
        contextual.select_provider(crate::providers::provider::Id::parse(provider)?);
        return match rest {
            [] => Ok(page(&contextual, &scope)),
            [key] if key.starts_with("option.") => {
                let provider = contextual.selected_provider();
                let option = contextual
                    .scope_settings(&scope)
                    .and_then(|settings| settings.providers.get(&provider))
                    .and_then(|settings| settings.options.get(key.trim_start_matches("option.")));
                Ok(vec![
                    option
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_else(|| "inherit".into()),
                ])
            }
            [key] => Ok(vec![Setting::parse(key, &scope)?.of(&contextual, &scope)]),
            _ => Err(PerchError::Invalid(
                "`perch config get <scope> [<key>]` takes one Setting at most.".into(),
            )),
        };
    }
    if words.first().is_some_and(|word| word == "--defaults") {
        return Ok(vec![
            serde_json::to_string_pretty(&registry.scope_defaults)
                .map_err(|e| PerchError::Other(e.to_string()))?,
        ]);
    }
    if words.first().is_some_and(|word| word == "--provider") {
        let [_, provider, rest @ ..] = words else {
            return Err(PerchError::Invalid(
                "`perch config get --provider <name> [enabled|cli-path]` reads a provider's Installation.".into(),
            ));
        };
        let provider = crate::providers::provider::Id::parse(provider)?;
        let settings = registry
            .provider_settings
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        let path = settings
            .cli_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "auto".into());
        return match rest {
            [] => Ok(vec![
                format!("enabled {}", settings.enabled),
                format!("cli-path {path}"),
            ]),
            [key] if key == "enabled" => Ok(vec![settings.enabled.to_string()]),
            [key] if key == "cli-path" => Ok(vec![path]),
            _ => Err(PerchError::Invalid(
                "A provider's Installation Settings are `enabled` and `cli-path`.".into(),
            )),
        };
    }

    if words.first().is_some_and(|word| word == "--global") {
        if let [key] = &words[1..] {
            if key == "run-fallback" {
                return Ok(vec![
                    if registry.run_fallback {
                        "installed"
                    } else {
                        "disabled"
                    }
                    .into(),
                ]);
            }
            if key == "watcher-paused" {
                return Ok(vec![registry.watcher_paused.to_string()]);
            }
        }
        return match &words[1..] {
            [] => Ok(vec![
                format!("run-provider: {}", registry.run_provider.word()),
                format!(
                    "run-fallback: {}",
                    if registry.run_fallback {
                        "installed"
                    } else {
                        "disabled"
                    }
                ),
                format!("watcher-paused: {}", registry.watcher_paused),
            ]),
            [key] if key == "run-provider" => Ok(vec![registry.run_provider.word().into()]),
            _ => Err(PerchError::Invalid(
                "`perch config get --global [run-provider|run-fallback|watcher-paused]` reads a global Setting.".into(),
            )),
        };
    }

    match words {
        [] => Ok(everything(registry)),
        [one] => {
            let scope = addressed(registry, one)?;
            Ok(page(registry, &scope))
        }
        [scope, key] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key, &scope)?;
            Ok(vec![key.of(registry, &scope)])
        }
        _ => Err(how_get_is_addressed(words)),
    }
}

/// Every Setting Perch holds, Scope by Scope, and no shorter than that: a row
/// left out here is a row nothing else prints. [`page`] under every Scope's
/// name rather than a second idea of what a Config is.
fn everything(registry: &Registry) -> Vec<String> {
    let mut lines = vec![
        "--global:".into(),
        format!("  run-provider {}", registry.run_provider.word()),
        format!(
            "  run-fallback {}",
            if registry.run_fallback {
                "installed"
            } else {
                "disabled"
            }
        ),
        format!("  watcher-paused {}", registry.watcher_paused),
    ];
    for scope in registry.scopes() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("{}:", scope.word()));
        lines.extend(page(registry, &scope));
    }
    lines
}

/// Every Setting one Scope holds, each a row of key and value. The key is the
/// word `set` takes and the value is the one it would take back, so the page
/// still reads as the vocabulary that writes it.
fn page(registry: &Registry, scope: &Scope) -> Vec<String> {
    let column = key_column(0);
    SETTINGS
        .into_iter()
        .filter(|key| key.carried_by(scope))
        .map(|key| column.row(key.as_str(), &Shown::of(&key.of(registry, scope))))
        .collect()
}

/// The key column wherever Settings are laid out as a page: the widest key
/// there is, and two cells of gutter. Measured over the whole vocabulary rather
/// than over the Scope's own keys, so two Scopes' pages line up with each other.
fn key_column(indent: usize) -> Labeled {
    let widest = SETTINGS
        .into_iter()
        .map(|key| column::cells(&Shown::of(key.as_str())))
        .max()
        .unwrap_or(0);
    Labeled::of(indent, widest + 2)
}

/// `set`'s long help, built from the vocabulary the refusals read, so one
/// binary has one account of what a Scope carries and what each Setting takes.
fn how_a_setting_is_set() -> String {
    let column = key_column(2);
    let rows: Vec<String> = SETTINGS
        .into_iter()
        .map(|key| column.row(key.as_str(), &Shown::of(&key.takes())))
        .collect();
    format!(
        "Set one Setting on one Scope.\n\
         \n\
         `<scope>` is a Group by name, or `{UNGROUPED}`. `<key>` and `<value>`:\n\
         {rows}\n\
         \n\
         The other forms:\n\
         \x20 perch config set --global <run-provider|run-fallback|watcher-paused> <value>\n\
         \x20 perch config set --defaults <key> <value>\n\
         \x20 perch config set <scope> --provider <name> <key> <value>\n\
         \x20 perch config set --provider <name> <enabled|cli-path> <value>",
        rows = rows.join("\n"),
    )
}

/// What a Setting is now, said as a change or as something that was already so.
///
/// Asking for a value a Setting already has is not a failure — a script that
/// runs twice has not done anything wrong — but it is worth saying, because it
/// is the difference between having changed something and having confirmed it.
fn changed(subject: &str, was: &str, now: &str) -> String {
    if was == now {
        format!("{subject} is already {now}.")
    } else {
        format!("{subject} is now {now}.")
    }
}

/// The Scope a word addresses: the Accounts in no Group, or a Group as it was
/// declared.
fn addressed(registry: &Registry, name: &str) -> Result<Scope> {
    match Scope::named(registry, name) {
        Ok(scope) => Ok(scope),
        Err(NotAScope::MeansEveryScope) => Err(PerchError::NotFound(format!(
            "There is no Scope called `{name}`. `perch config get` reads every \
             Scope there is."
        ))),
        Err(NotAScope::NoSuchGroup) => {
            Err(a_setting_is_not_a_scope(name)
                .unwrap_or_else(|| group::no_such_group(registry, name)))
        }
    }
}

/// A key typed where a Scope goes. `None` for a word that is not a key either,
/// which is an ordinary mistyped Group name and `group::no_such_group`'s to
/// answer — kept apart because the two send somebody to different places, one
/// to the spelling of a Group and one to the form that has a subject in it.
fn a_setting_is_not_a_scope(word: &str) -> Option<PerchError> {
    let key = Setting::parse_quietly(word)?.as_str();
    Some(PerchError::NotFound(format!(
        "`{key}` is a Setting, not a Scope. `perch config set <scope> {key} \
         <value>` sets it."
    )))
}

/// Two words with no Scope among them: a Setting with no subject.
fn no_scope_was_named(registry: &Registry, key: &str, value: &str) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set {key} {value}` names no Scope. `perch config set \
         <scope> {key} {value}` does. {}",
        the_scopes(registry),
    ))
}

/// The Scopes there are to name, said as a sentence. Every refusal about a
/// missing Scope ends with it, because "name a Scope" is no use to somebody who
/// does not know what theirs are called.
fn the_scopes(registry: &Registry) -> String {
    let mut scopes = vec![format!("`{UNGROUPED}`")];
    scopes.extend(registry.groups.keys().map(|group| format!("`{group}`")));
    format!("The Scopes are {}.", scopes.join(", "))
}

/// The form `set` takes, said whenever the words said were not it.
fn how_set_is_addressed(registry: &Registry, words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set` takes `perch config set <scope> <key> <value>`, \
         not {}. {}",
        say::words(words.len()),
        the_scopes(registry),
    ))
}

/// The forms `get` takes, which are not the form `set` takes: naming fewer
/// words asks about more rather than being short of a value. One sentence
/// serving both would name a form that does not exist.
fn how_get_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config get` takes `perch config get [<scope> [<key>]]`, not {}.",
        say::words(words.len()),
    ))
}

fn inherited<T>(value: &str, parse: impl FnOnce(&str) -> Result<T>) -> Result<Option<T>> {
    if value == "inherit" {
        Ok(None)
    } else {
        parse(value).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Strategy;

    fn holding_a_group() -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
    }

    fn words(said: &[&str]) -> Vec<String> {
        said.iter().map(|word| word.to_string()).collect()
    }

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    #[test]
    fn a_set_that_names_no_scope_is_refused_and_names_the_scopes() {
        let mut registry = holding_a_group();

        let refused = set(&mut registry, &words(&["strategy", "soonest-reset"]))
            .expect_err("there is no Scope for that to be about");

        let said = refused.to_string();
        assert!(said.contains("names no Scope"), "{said}");
        assert!(
            said.contains("perch config set <scope> strategy soonest-reset"),
            "the form with a subject in it is named, with the words they typed \
             already in it: {said}"
        );
        assert!(
            said.contains("The Scopes are `ungrouped`, `work`."),
            "{said}"
        );
        assert_eq!(
            registry.settings(&work()).strategy,
            Strategy::MostHeadroom,
            "and nothing was written"
        );
    }

    #[test]
    fn a_setting_said_about_one_scope_reaches_no_other() {
        let mut registry = holding_a_group();
        registry.declare_group("personal").unwrap();

        set(
            &mut registry,
            &words(&["work", "watcher-threshold-percent", "50"]),
        )
        .unwrap();

        assert_eq!(registry.settings(&work()).watcher_threshold_percent, 50);
        assert_eq!(
            registry
                .settings(&Scope::Group("personal".to_string()))
                .watcher_threshold_percent,
            80,
            "the Group nobody said anything about is at the compiled default"
        );
        assert_eq!(
            registry
                .settings(&Scope::Ungrouped)
                .watcher_threshold_percent,
            80
        );
    }

    /// The grant is the case this matters most for: a Group declared after
    /// somebody let the watcher into another one.
    #[test]
    fn a_group_declared_later_is_not_reached_by_a_grant_made_earlier() {
        let mut registry = holding_a_group();
        set(&mut registry, &words(&["work", "watcher-may-act", "true"])).unwrap();

        registry.declare_group("personal").unwrap();

        assert!(
            !registry
                .settings(&Scope::Group("personal".to_string()))
                .watcher_may_act,
            "consent is said about the Scope it grants, so a Group that did not \
             exist when it was said cannot have been included in it"
        );
    }

    #[test]
    fn a_scopes_page_and_its_name_are_the_set_that_restores_it() {
        let mut registry = holding_a_group();
        set(
            &mut registry,
            &words(&["work", "strategy", "soonest-reset"]),
        )
        .unwrap();
        set(
            &mut registry,
            &words(&["ungrouped", "watcher-threshold-percent", "90"]),
        )
        .unwrap();
        set(
            &mut registry,
            &words(&["ungrouped", "interchangeable", "true"]),
        )
        .unwrap();

        let mut restored = Registry::default();
        restored.declare_group("work").unwrap();
        for scope in registry.scopes() {
            for line in page(&registry, &scope) {
                let row: Vec<&str> = line.split_whitespace().collect();
                let [key, value] = row[..] else {
                    panic!("a row is a key and a value: {line}")
                };
                set(&mut restored, &words(&[scope.word(), key, value])).unwrap();
            }
        }
        for scope in registry.scopes() {
            assert_eq!(restored.settings(&scope), registry.settings(&scope));
        }
        assert_eq!(
            restored.ungrouped.interchangeable,
            registry.ungrouped.interchangeable
        );
    }

    /// The header is what carries the Scope where the words did not name one,
    /// so a bare `get` is the one form whose rows do not say what they are about.
    #[test]
    fn a_bare_get_names_every_scope_above_its_page() {
        let registry = holding_a_group();

        let printed = get(&registry, &[]).unwrap();

        for scope in registry.scopes() {
            assert!(
                printed.contains(&format!("{}:", scope.word())),
                "{scope:?} is missing its header: {printed:?}"
            );
        }
        assert!(
            !get(&registry, &words(&["work"]))
                .unwrap()
                .iter()
                .any(|line| line.ends_with(':')),
            "while a Scope that was named carries no header of its own"
        );
    }

    #[test]
    fn a_scope_and_a_key_read_back_the_value_alone() {
        let registry = holding_a_group();

        assert_eq!(
            get(&registry, &words(&["work", "strategy"])).unwrap(),
            vec!["most-headroom".to_string()],
        );
        assert_eq!(
            get(&registry, &words(&["ungrouped", "strategy"])).unwrap(),
            vec!["most-headroom".to_string()],
        );
    }

    #[test]
    fn only_the_ungrouped_accounts_carry_the_declaration_that_they_are_a_set() {
        let mut registry = holding_a_group();

        let refused = set(&mut registry, &words(&["work", "interchangeable", "true"]))
            .expect_err("a Group is that declaration rather than holding one");
        let said = refused.to_string();
        assert!(said.contains("of `ungrouped` alone"), "{said}");
        assert!(
            said.contains("perch config set ungrouped interchangeable"),
            "{said}"
        );

        assert!(
            !get(&registry, &words(&["work"]))
                .unwrap()
                .iter()
                .any(|line| line.contains("interchangeable")),
            "and a Group's page does not print a line it would refuse to take back"
        );
        assert!(
            get(&registry, &words(&["ungrouped"]))
                .unwrap()
                .iter()
                .any(|line| {
                    line.split_whitespace().collect::<Vec<_>>() == ["interchangeable", "false"]
                }),
            "while the Scope that does carry it prints it"
        );
    }

    /// A word in the Scope's place can only be a Scope, so a Group may be named
    /// after a key.
    #[test]
    fn a_group_named_after_a_key_is_addressed_like_any_other() {
        let mut registry = Registry::default();
        registry.declare_group("strategy").unwrap();

        set(
            &mut registry,
            &words(&["strategy", "watcher-may-act", "true"]),
        )
        .unwrap();

        assert!(
            registry
                .settings(&Scope::Group("strategy".to_string()))
                .watcher_may_act,
            "the Group took it, because the first word is where a Scope goes"
        );
        assert_eq!(
            get(&registry, &words(&["strategy", "strategy"])).unwrap(),
            vec!["most-headroom".to_string()],
            "and the key is still reachable, in the place a key goes"
        );
    }

    /// Being sent to check the spelling of a Group is being sent to look for a
    /// mistake that is not the problem.
    #[test]
    fn a_key_where_a_scope_goes_says_a_setting_needs_a_subject() {
        let registry = holding_a_group();

        let refused = get(&registry, &words(&["watcher-may-act"]))
            .expect_err("a Setting on its own is about nothing");

        let said = refused.to_string();
        assert!(said.contains("is a Setting, not a Scope"), "{said}");
        assert!(!said.contains("No Group called"), "{said}");
    }

    #[test]
    fn a_scope_and_a_key_with_nothing_to_set_it_to_says_which_form_was_meant() {
        let mut registry = holding_a_group();

        let refused = set(&mut registry, &words(&["work", "strategy"]))
            .expect_err("that is a Scope and a key, with no value");

        let said = refused.to_string();
        assert!(
            said.contains("perch config set work strategy <value>"),
            "{said}"
        );
    }
}
