//! `perch config` — changing how a Group behaves, from a script.
//!
//! Perch has to be complete over SSH and in CI, so every capability it has is
//! reachable non-interactively (ADR 0011). This is the one that changes the
//! rules rather than the state: which Account a Cycle prefers, and whether the
//! ungrouped Accounts may be Cycled among at all.
//!
//! Most configuration belongs to a Group, because a Group is what carries the
//! rules governing Cycling within it (ADR 0002). One setting cannot: whether
//! bare `perch switch` may Cycle among the Accounts in no Group is about
//! Accounts that have no Group to carry it, so it is global (ADR 0017). Both
//! are reached through this one command, and which is meant is read off how
//! many words were said — three name a Group, two do not. Nothing else would
//! do: a reserved Group name would make a Group mean two contradictory things,
//! and a flag would make the global form the odd one out.
//!
//! `get` prints each setting as the tail of the `set` that would restore it, so
//! reading the configuration and writing it back are the same vocabulary and a
//! script needs no parser.
//!
//! The watcher's two fields are stored and validated here and read by nothing:
//! the watcher itself is deferred (ADR 0013). They are settable now so a Group
//! configured today is one the watcher can be pointed at without migrating
//! anything — and every message about them says plainly that nothing is acting
//! on them yet, because a setting that looks like it took but does nothing is
//! worse than one that is refused.

use std::io::Write;

use crate::adopt;
use crate::commands::{group, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, GlobalConfig, GroupConfig, Registry, Strategy};

/// What was asked of `perch config`, as the words that were typed.
///
/// The words are carried rather than resolved because which of them names a
/// Group is not something the command line can know: the forms differ only in
/// how many there are, and telling the user which they seem to have meant is
/// part of what this command does.
#[derive(Debug, Clone)]
pub enum ConfigCommand {
    /// `perch config set [<group>] <key> <value>`.
    Set { words: Vec<String> },
    /// `perch config get [[<group>] <key>]`.
    Get { words: Vec<String> },
}

pub fn run(host: &dyn Host, command: ConfigCommand, out: &mut dyn Write) -> Result<()> {
    let mut registry = adopt::ensure_adopted(host, out)?;

    match command {
        ConfigCommand::Set { words } => {
            let said = set(&mut registry, &words)?;
            registry::save(host, &registry)?;
            for line in said {
                say(out, &line)?;
            }
            Ok(())
        }
        ConfigCommand::Get { words } => {
            for line in get(&registry, &words)? {
                say(out, &line)?;
            }
            Ok(())
        }
    }
}

/// Sets one setting, returning what to tell the user: what it is now, and what
/// that means for them.
fn set(registry: &mut Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [group, key, value] => {
            let declared = declared_group(registry, group)?;
            let key = GroupKey::parse(key)?;
            let config = registry
                .groups
                .get(&declared)
                .expect("the Group was just found")
                .clone();
            let was = key.read(&config);
            let config = validated(config, key, value, &declared)?;
            let now = key.read(&config);
            let meaning = key.what_that_means(&config, &declared);
            registry.groups.insert(declared.clone(), config);
            Ok(vec![
                changed(
                    &format!("`{}` on Group `{declared}`", key.as_str()),
                    &was,
                    &now,
                ),
                meaning,
            ])
        }
        [key, value] => {
            // Two words address a setting belonging to no Group — unless the
            // first is a Group, in which case what is missing is the value
            // rather than the meaning. A key Perch owns wins over a Group that
            // shares its name: naming no Group is the only way to reach that
            // setting, while the Group's is still reachable with three words.
            if GlobalKey::parse_quietly(key).is_err()
                && let Some(declared) = registry.declared_group(key)
            {
                return Err(PerchError::Invalid(format!(
                    "`perch config set {key} {value}` names the Group `{declared}` and \
                     a key, but nothing to set it to. `perch config set <group> <key> \
                     <value>` sets one."
                )));
            }
            let key = GlobalKey::parse(key)?;
            let was = key.read(&registry.global);
            key.write(&mut registry.global, value)?;
            let now = key.read(&registry.global);
            Ok(vec![
                changed(&format!("`{}`", key.as_str()), &was, &now),
                key.what_that_means(&registry.global),
            ])
        }
        _ => Err(how_set_is_addressed(words)),
    }
}

/// The Group's configuration with the value applied, or a refusal.
///
/// The value is applied to a copy, so configuration that would not mean
/// anything never reaches the Group it was meant for: a refused `set` leaves
/// every setting exactly as it found it.
fn validated(
    mut config: GroupConfig,
    key: GroupKey,
    value: &str,
    group: &str,
) -> Result<GroupConfig> {
    key.write(&mut config, value)?;
    // The range a percentage has to be in is stated once, where the registry
    // enforces it on the way in, rather than restated here where it could come
    // to disagree with it.
    config.validate(group)?;
    Ok(config)
}

/// Reads settings back, in the form that would set them again.
fn get(registry: &Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [] => Ok(everything(registry)),
        [one] => {
            // A key Perch owns is answered as a key even if a Group shares its
            // name: the global form is the only way to read that setting, while
            // a Group's settings can still be read one key at a time.
            if let Ok(key) = GlobalKey::parse_quietly(one) {
                return Ok(vec![key.read(&registry.global)]);
            }
            let declared = match registry.declared_group(one) {
                Some(declared) => declared.to_string(),
                None => return Err(neither_a_key_nor_a_group(registry, one)),
            };
            Ok(group_lines(registry, &declared))
        }
        [group, key] => {
            let declared = declared_group(registry, group)?;
            let key = GroupKey::parse(key)?;
            Ok(vec![key.read(&registry.groups[&declared])])
        }
        _ => Err(how_get_is_addressed(words)),
    }
}

/// Every setting Perch holds: the ones belonging to no Group first, then each
/// Group's, so the shape of the answer matches the shape of the vocabulary.
fn everything(registry: &Registry) -> Vec<String> {
    let mut lines: Vec<String> = GLOBAL_KEYS
        .iter()
        .map(|key| format!("{} {}", key.as_str(), key.read(&registry.global)))
        .collect();
    for name in registry.groups.keys() {
        lines.extend(group_lines(registry, name));
    }
    lines
}

fn group_lines(registry: &Registry, group: &str) -> Vec<String> {
    let config = &registry.groups[group];
    GROUP_KEYS
        .iter()
        .map(|key| format!("{group} {} {}", key.as_str(), key.read(config)))
        .collect()
}

/// What a setting is now, said as a change or as something that was already so.
///
/// Asking for a value a setting already has is not a failure — a script that
/// runs twice has not done anything wrong — but it is worth saying, because it
/// is the difference between having changed something and having confirmed it.
fn changed(subject: &str, was: &str, now: &str) -> String {
    if was == now {
        format!("{subject} is already {now}.")
    } else {
        format!("{subject} is now {now}.")
    }
}

/// The Group as it was declared, or the refusal a mistyped Group name always
/// gets — the same one `perch group` gives, because a typo is a typo wherever
/// it is made.
fn declared_group(registry: &Registry, name: &str) -> Result<String> {
    match registry.declared_group(name) {
        Some(declared) => Ok(declared.to_string()),
        None => Err(group::no_such_group(registry, name)),
    }
}

/// One word that is neither of the two things one word can be.
fn neither_a_key_nor_a_group(registry: &Registry, word: &str) -> PerchError {
    let groups: Vec<&str> = registry.groups.keys().map(String::as_str).collect();
    let held = if groups.is_empty() {
        "No Groups have been declared yet.".to_string()
    } else {
        format!("Groups Perch holds: {}.", groups.join(", "))
    };
    PerchError::NotFound(format!(
        "`{word}` is neither a setting belonging to no Group nor a Group Perch \
         holds. Settings belonging to no Group: {}. {held}",
        GlobalKey::vocabulary(),
    ))
}

/// The forms `set` takes, said whenever the words said were none of them.
fn how_set_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set` was given {}. It takes a Group, a key and a value — \
         `perch config set <group> <key> <value>` — or a key and a value alone \
         for a setting that belongs to no Group: `perch config set <key> <value>`.",
        counted(words),
    ))
}

/// The forms `get` takes, which are not the forms `set` takes: naming fewer
/// words asks about more rather than being short of a value. One sentence
/// serving both would name a form that does not exist.
fn how_get_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config get` was given {}. It takes a Group and a key — `perch \
         config get <group> <key>` — or a key alone for a setting that belongs \
         to no Group. `perch config get <group>` reads everything one Group \
         carries, and `perch config get` on its own reads every setting there is.",
        counted(words),
    ))
}

fn counted(words: &[String]) -> String {
    match words.len() {
        1 => "1 word".to_string(),
        count => format!("{count} words"),
    }
}

/// The settings a Group carries (ADR 0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKey {
    Strategy,
    WatcherMayAct,
    WatcherThresholdPercent,
}

const GROUP_KEYS: [GroupKey; 3] = [
    GroupKey::Strategy,
    GroupKey::WatcherMayAct,
    GroupKey::WatcherThresholdPercent,
];

impl GroupKey {
    fn as_str(self) -> &'static str {
        match self {
            GroupKey::Strategy => "strategy",
            GroupKey::WatcherMayAct => "watcher-may-act",
            GroupKey::WatcherThresholdPercent => "watcher-threshold-percent",
        }
    }

    fn parse(name: &str) -> Result<Self> {
        GROUP_KEYS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
            .ok_or_else(|| {
                PerchError::Invalid(format!(
                    "`{name}` is not a setting a Group carries. The ones it \
                     carries are {}.",
                    listed(GROUP_KEYS.map(GroupKey::as_str).as_slice()),
                ))
            })
    }

    /// The value as `get` prints it and `set` would take it back.
    fn read(self, config: &GroupConfig) -> String {
        match self {
            GroupKey::Strategy => config.strategy.as_str().to_string(),
            GroupKey::WatcherMayAct => config.watcher_may_act.to_string(),
            GroupKey::WatcherThresholdPercent => config.watcher_threshold_percent.to_string(),
        }
    }

    fn write(self, config: &mut GroupConfig, value: &str) -> Result<()> {
        match self {
            GroupKey::Strategy => config.strategy = strategy(value)?,
            GroupKey::WatcherMayAct => config.watcher_may_act = yes_or_no(self.as_str(), value)?,
            GroupKey::WatcherThresholdPercent => {
                config.watcher_threshold_percent = percentage(value)?
            }
        }
        Ok(())
    }

    /// What the Group now does, which is the half of the answer the value
    /// itself does not give. For the watcher's fields it is mostly what it
    /// still does not do.
    fn what_that_means(self, config: &GroupConfig, group: &str) -> String {
        match self {
            GroupKey::Strategy => match config.strategy {
                Strategy::MostHeadroom => format!(
                    "A Cycle within `{group}` prefers the Account with the most \
                     room left, measured by its worst Quota Window (ADR 0012)."
                ),
                Strategy::SoonestReset => format!(
                    "A Cycle within `{group}` prefers the Account whose fullest \
                     Quota Window resets soonest, so perishable quota is spent \
                     rather than wasted. Headroom is still measured by the worst \
                     window (ADR 0012), so an exhausted Account is still never \
                     chosen however soon it comes back."
                ),
            },
            GroupKey::WatcherMayAct if config.watcher_may_act => {
                format!("Nothing switches within `{group}` unattended yet: {WATCHER_IS_DEFERRED}")
            }
            GroupKey::WatcherMayAct => format!(
                "Nothing switches within `{group}` unattended — and nothing \
                 would have anyway: {WATCHER_IS_DEFERRED}"
            ),
            GroupKey::WatcherThresholdPercent => format!(
                "That is the Utilization the watcher would act at within \
                 `{group}`. Nothing acts on it: {WATCHER_IS_DEFERRED}"
            ),
        }
    }
}

/// Said of every watcher field, in one place, because two sentences about it
/// would sooner or later say two different things (ADR 0013).
const WATCHER_IS_DEFERRED: &str = "the watcher is not in this version. The \
     setting is stored and validated, and governs it when it lands.";

/// The settings that belong to no Group, because there is no Group for them to
/// belong to (ADR 0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalKey {
    CycleUngrouped,
}

const GLOBAL_KEYS: [GlobalKey; 1] = [GlobalKey::CycleUngrouped];

impl GlobalKey {
    fn as_str(self) -> &'static str {
        match self {
            GlobalKey::CycleUngrouped => "cycle-ungrouped",
        }
    }

    fn vocabulary() -> String {
        listed(GLOBAL_KEYS.map(GlobalKey::as_str).as_slice())
    }

    fn parse(name: &str) -> Result<Self> {
        Self::parse_quietly(name).map_err(|_| {
            PerchError::Invalid(format!(
                "`{name}` is not a setting that belongs to no Group. Naming no \
                 Group addresses {}. Everything else is a Group's, and is set \
                 with `perch config set <group> <key> <value>`.",
                Self::vocabulary(),
            ))
        })
    }

    /// The same lookup where failing is an answer rather than a refusal — for
    /// the one-word form of `get`, which has a second thing to try.
    fn parse_quietly(name: &str) -> std::result::Result<Self, ()> {
        GLOBAL_KEYS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
            .ok_or(())
    }

    fn read(self, config: &GlobalConfig) -> String {
        match self {
            GlobalKey::CycleUngrouped => config.cycle_ungrouped.to_string(),
        }
    }

    fn write(self, config: &mut GlobalConfig, value: &str) -> Result<()> {
        match self {
            GlobalKey::CycleUngrouped => config.cycle_ungrouped = yes_or_no(self.as_str(), value)?,
        }
        Ok(())
    }

    fn what_that_means(self, config: &GlobalConfig) -> String {
        match self {
            GlobalKey::CycleUngrouped if config.cycle_ungrouped => {
                "A bare `perch switch` from an Account in no Group now Cycles \
                 among the other ungrouped Accounts. That declares every \
                 ungrouped Account interchangeable at once, present and future \
                 — including the next one `perch add` creates (ADR 0017)."
                    .to_string()
            }
            GlobalKey::CycleUngrouped => {
                "A bare `perch switch` from an Account in no Group switches \
                 nowhere and says why. Being ungrouped is the absence of a \
                 declaration that Accounts are interchangeable, not a weaker \
                 form of one (ADR 0017)."
                    .to_string()
            }
        }
    }
}

fn strategy(value: &str) -> Result<Strategy> {
    Strategy::ALL
        .into_iter()
        .find(|candidate| value.eq_ignore_ascii_case(candidate.as_str()))
        .ok_or_else(|| {
            PerchError::Invalid(format!(
                "`{value}` is not a Strategy Perch implements. The ones it \
                 implements are:\n  {}",
                Strategy::ALL
                    .map(|strategy| format!("{} — {}", strategy.as_str(), gloss(strategy)))
                    .join("\n  "),
            ))
        })
}

/// What each Strategy prefers, in a clause. Built by matching every Strategy
/// rather than written out once as prose, so a Strategy added to the enum
/// cannot ship with a refusal that fails to mention it — the match stops
/// compiling instead.
fn gloss(strategy: Strategy) -> &'static str {
    match strategy {
        Strategy::MostHeadroom => "prefers the Account with the most room left",
        Strategy::SoonestReset => {
            "prefers the Account whose quota is about to be thrown away, so it \
             is spent rather than wasted"
        }
    }
}

fn yes_or_no(key: &str, value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(PerchError::Invalid(format!(
            "`{value}` is not a value `{key}` takes. It is either `true` or `false`."
        ))),
    }
}

fn percentage(value: &str) -> Result<u8> {
    value.parse::<u8>().map_err(|_| {
        PerchError::Invalid(format!(
            "`{value}` is not a percentage. A Utilization threshold is a whole \
             number between 0 and 100."
        ))
    })
}

/// "a, b and c" — a vocabulary said as a sentence, because a refusal that names
/// what is valid is read rather than parsed.
fn listed(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => format!("`{only}`"),
        [rest @ .., last] => format!(
            "{} and `{last}`",
            rest.iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding_a_group() -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
    }

    fn words(said: &[&str]) -> Vec<String> {
        said.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn three_words_address_a_group_and_two_address_no_group() {
        let mut registry = holding_a_group();

        set(
            &mut registry,
            &words(&["work", "strategy", "soonest-reset"]),
        )
        .unwrap();
        set(&mut registry, &words(&["cycle-ungrouped", "true"])).unwrap();

        assert_eq!(
            registry.group("work").unwrap().strategy,
            Strategy::SoonestReset
        );
        assert!(registry.global.cycle_ungrouped);
    }

    #[test]
    fn a_value_that_means_nothing_leaves_the_group_as_it_was() {
        let mut registry = holding_a_group();
        set(&mut registry, &words(&["work", "watcher-may-act", "true"])).unwrap();

        // Refused by the range the registry enforces, after the value parsed.
        set(
            &mut registry,
            &words(&["work", "watcher-threshold-percent", "101"]),
        )
        .expect_err("a Utilization threshold is a percentage");

        let config = registry.group("work").unwrap();
        assert_eq!(config.watcher_threshold_percent, 80);
        assert!(
            config.watcher_may_act,
            "the failed write is the only thing rolled back"
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
            &words(&["work", "watcher-threshold-percent", "90"]),
        )
        .unwrap();

        let printed = get(&registry, &[]).unwrap();

        let mut restored = Registry::default();
        restored.declare_group("work").unwrap();
        for line in &printed {
            set(&mut restored, &words(&line.split(' ').collect::<Vec<_>>())).unwrap();
        }
        assert_eq!(restored.groups, registry.groups);
        assert_eq!(restored.global, registry.global);
    }

    #[test]
    fn a_vocabulary_is_named_as_a_sentence() {
        assert_eq!(listed(&["one"]), "`one`");
        assert_eq!(listed(&["one", "two"]), "`one` and `two`");
        assert_eq!(listed(&["one", "two", "three"]), "`one`, `two` and `three`");
    }

    #[test]
    fn a_group_that_shares_a_global_keys_name_does_not_hide_the_setting() {
        let mut registry = Registry::default();
        registry.declare_group("cycle-ungrouped").unwrap();
        set(&mut registry, &words(&["cycle-ungrouped", "true"])).unwrap();

        assert_eq!(
            get(&registry, &words(&["cycle-ungrouped"])).unwrap(),
            vec!["true".to_string()],
            "one word is the setting, which has no other way to be read"
        );
        assert_eq!(
            get(&registry, &words(&["cycle-ungrouped", "strategy"])).unwrap(),
            vec!["most-headroom".to_string()],
            "and the Group is still reachable a key at a time"
        );
    }
}
