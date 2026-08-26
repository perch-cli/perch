//! `perch config` — changing the rules Perch chooses Accounts by, from a
//! script.
//!
//! The **grammar** only: which word goes where, which form somebody seems to
//! have meant when the words were not one, and the line that reads back as the
//! `set` that would restore it. What a Setting *is* is [`crate::config`]'s,
//! because surfaces that are not this command name keys too.
//!
//! Every `set` is `<scope> <key> <value>` and reading is not writing
//! (ADR a-setting-names-its-scope).

use std::io::Write;

use crate::adopt;
use crate::commands::{group, only_the_registry, say};
use crate::config::{SETTINGS, Setting};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::name;
use crate::name::UNGROUPED;
use crate::registry::{Registry, Scope};

/// What was asked of `perch config`, as the words that were typed — carried
/// rather than resolved, because telling somebody which form they seem to have
/// meant is part of what this command does, and a parser that had thrown the
/// words away could not.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ConfigCommand {
    /// Set one Setting on one Scope, and say what it now means.
    ///
    /// A Scope is a Group by name, or `ungrouped` for the Accounts in no Group.
    /// Every Scope carries `strategy` and the watcher's `watcher-may-act` and
    /// `watcher-threshold-percent`; the Accounts in no Group also carry
    /// `interchangeable`, which is the declaration that they may be Cycled
    /// among at all.
    Set {
        /// `<scope> <key> <value>`.
        #[arg(value_name = "WORDS", num_args = 1.., required = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },

    /// Read Settings back, each one in the form that would set it again.
    ///
    /// With nothing named it prints every Scope's Config in full. A Scope prints
    /// every Setting it holds, and a Scope and a key print the one line.
    Get {
        /// Nothing, `<scope>`, or `<scope> <key>`.
        #[arg(value_name = "WORDS", num_args = 0.., allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

pub fn run(host: &dyn Host, command: ConfigCommand, out: &mut dyn Write) -> Result<()> {
    // Taken inside the match, so only by the half that writes: a reader that
    // takes the write lock waits out whatever holds it and then fails with
    // "another `perch` holds it".
    match command {
        // The half that writes changes the registry and reaches nothing else,
        // which is the whole of what `only_the_registry` is for
        // (ADR one-door-to-the-registry).
        ConfigCommand::Set { words } => {
            only_the_registry(host, out, |registry| set(registry, &words))
        }
        ConfigCommand::Get { words } => {
            let registry = adopt::ensure_adopted(host)?;
            for line in get(&registry, &words)? {
                say(out, &line)?;
            }
            Ok(())
        }
    }
}

/// Sets one Setting, returning what to tell the user: what it is now, and what
/// that means for them.
fn set(registry: &mut Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [scope, key, value] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key, &scope)?;
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
                    "`perch config set {first} {second}` names {} and a key, but \
                     nothing to set it to. `perch config set <scope> <key> \
                     <value>` sets one.",
                    scope.mentioned(),
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

/// Reads Settings back, in the form that would set them again.
fn get(registry: &Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [] => Ok(everything(registry)),
        [one] => {
            let scope = addressed(registry, one)?;
            Ok(scope_lines(registry, &scope))
        }
        [scope, key] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key, &scope)?;
            Ok(vec![line(&scope, key, &key.of(registry, &scope))])
        }
        _ => Err(how_get_is_addressed(words)),
    }
}

/// Every Setting Perch holds, Scope by Scope, and no shorter than that: a line
/// left out here is a line nothing else prints. [`scope_lines`] over every Scope
/// rather than a second idea of what a Config is.
fn everything(registry: &Registry) -> Vec<String> {
    registry
        .scopes()
        .iter()
        .flat_map(|scope| scope_lines(registry, scope))
        .collect()
}

/// Every Setting one Scope holds, each as the tail of the `set` that would
/// restore it.
fn scope_lines(registry: &Registry, scope: &Scope) -> Vec<String> {
    SETTINGS
        .into_iter()
        .filter(|key| key.carried_by(scope))
        .map(|key| line(scope, key, &key.of(registry, scope)))
        .collect()
}

/// One Setting as `get` prints it and `set` would take it back: the whole of the
/// command, minus the command.
fn line(scope: &Scope, key: Setting, value: &str) -> String {
    format!("{} {} {value}", scope.word(), key.as_str())
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
    if name::means_the_ungrouped_scope(name) {
        return Ok(Scope::Ungrouped);
    }
    // Answered here rather than left to fall through, which would offer
    // "Declare it with `perch group add global`" — advice the registry refuses,
    // and which would be worse if it did not.
    if name::means_global(name) {
        return Err(PerchError::NotFound(format!(
            "There is no Scope every other one falls back to, so there is no \
             `{name}` to name: every Setting is said about the Scope it governs. \
             A Scope is a Group by name, or `{UNGROUPED}` for the Accounts in no \
             Group, and `perch config get` prints every one of them."
        )));
    }
    match registry.declared_group(name) {
        Some(declared) => Ok(Scope::Group(declared.to_string())),
        None => {
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
        "`{key}` is a Setting rather than a Scope, and a Setting is said about \
         the Scope it governs: `perch config set <scope> {key} <value>` sets one \
         and `perch config get <scope> {key}` reads it."
    )))
}

/// Two words with no Scope among them: a Setting with no subject.
fn no_scope_was_named(registry: &Registry, key: &str, value: &str) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set {key} {value}` names no Scope, and every Setting is \
         said about the Scope it governs — there is nothing above them for a \
         value to be set at. `perch config set <scope> {key} {value}` sets one. \
         {}",
        the_scopes(registry),
    ))
}

/// The Scopes there are to name, said as a sentence. Every refusal about a
/// missing Scope ends with it, because "name a Scope" is no use to somebody who
/// does not know what theirs are called.
fn the_scopes(registry: &Registry) -> String {
    let groups: Vec<&str> = registry.groups.keys().map(String::as_str).collect();
    let held = match groups.is_empty() {
        true => "No Groups have been declared yet.".to_string(),
        false => format!("Groups Perch holds: {}.", groups.join(", ")),
    };
    format!("`{UNGROUPED}` addresses the Accounts in no Group. {held}")
}

/// The form `set` takes, said whenever the words said were not it.
fn how_set_is_addressed(registry: &Registry, words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set` was given {}. It takes a Scope, a key and a value — \
         `perch config set <scope> <key> <value>`, where a Scope is a Group or \
         `{UNGROUPED}`. {}",
        super::words(words.len()),
        the_scopes(registry),
    ))
}

/// The forms `get` takes, which are not the form `set` takes: naming fewer
/// words asks about more rather than being short of a value. One sentence
/// serving both would name a form that does not exist.
fn how_get_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config get` was given {}. It takes a Scope and a key — `perch \
         config get <scope> <key>` — or a Scope alone to read every Setting it \
         holds. `perch config get` on its own reads every Scope there is.",
        super::words(words.len()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Strategy;

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
        assert!(said.contains("Groups Perch holds: work."), "{said}");
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
    fn every_line_get_prints_is_a_set_that_would_restore_it() {
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

        let printed = get(&registry, &[]).unwrap();

        let mut restored = Registry::default();
        restored.declare_group("work").unwrap();
        for line in &printed {
            set(&mut restored, &words(&line.split(' ').collect::<Vec<_>>())).unwrap();
        }
        assert_eq!(restored.groups, registry.groups);
        assert_eq!(restored.ungrouped, registry.ungrouped);
    }

    #[test]
    fn every_line_names_the_scope_it_is_about() {
        let registry = holding_a_group();

        assert_eq!(
            get(&registry, &words(&["work", "strategy"])).unwrap(),
            vec!["work strategy most-headroom".to_string()],
        );
        assert_eq!(
            get(&registry, &words(&["ungrouped", "strategy"])).unwrap(),
            vec!["ungrouped strategy most-headroom".to_string()],
        );
    }

    #[test]
    fn only_the_ungrouped_accounts_carry_the_declaration_that_they_are_a_set() {
        let mut registry = holding_a_group();

        let refused = set(&mut registry, &words(&["work", "interchangeable", "true"]))
            .expect_err("a Group is that declaration rather than holding one");
        let said = refused.to_string();
        assert!(said.contains("only they carry it"), "{said}");
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
                .contains(&"ungrouped interchangeable false".to_string()),
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
            vec!["strategy strategy most-headroom".to_string()],
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
        assert!(said.contains("rather than a Scope"), "{said}");
        assert!(!said.contains("No Group called"), "{said}");
    }

    #[test]
    fn a_scope_and_a_key_with_nothing_to_set_it_to_says_which_form_was_meant() {
        let mut registry = holding_a_group();

        let refused = set(&mut registry, &words(&["work", "strategy"]))
            .expect_err("that is a Scope and a key, with no value");

        let said = refused.to_string();
        assert!(said.contains("Group `work`"), "{said}");
        assert!(said.contains("<scope> <key> <value>"), "{said}");
    }
}
