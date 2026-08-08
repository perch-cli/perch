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
//! The watcher's five fields say whether `perch watch` may Switch within a
//! Group, at what Utilization it does, how often it may, how much emptier the
//! Account it moves to has to be, and whether the one it just left counts
//! (ADR 0013). Every message that describes the watcher *acting* says the same
//! thing about what it is not: a Group that may be acted on is not a service
//! that has been switched on, because nothing acts on it unless somebody is
//! running the loop. The one message that need not is the one saying the
//! watcher may not act on this Group at all.

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
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ConfigCommand {
    /// Set one setting, and say what it now means.
    ///
    /// A Group carries `strategy` — `most-headroom` or `soonest-reset` — along
    /// with the watcher's policy: `watcher-may-act`,
    /// `watcher-threshold-percent`, `watcher-cooldown-minutes`,
    /// `watcher-margin-percent` and `watcher-no-return` (ADR 0013). Naming no
    /// Group addresses `cycle-ungrouped`, which is whether bare `perch switch`
    /// may Cycle among the Accounts in no Group at all.
    Set {
        /// `<group> <key> <value>`, or `<key> <value>` for a setting that
        /// belongs to no Group.
        #[arg(value_name = "WORDS", num_args = 1.., required = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },

    /// Read settings back, each one in the form that would set it again.
    ///
    /// With nothing named it prints every setting there is. A Group prints
    /// what that Group carries, and a Group and a key — or a key alone, for a
    /// setting belonging to no Group — prints the one value on its own, for a
    /// script to read without parsing prose.
    Get {
        /// Nothing, `<group>`, `<key>`, or `<group> <key>`.
        #[arg(value_name = "WORDS", num_args = 0.., allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

pub fn run(host: &dyn Host, command: ConfigCommand, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    match command {
        ConfigCommand::Set { words } => {
            let said = set(&mut registry, &words)?;
            registry::save(host, &mut perch, &registry)?;
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
    // Asked again over the whole Group, because the registry is the boundary
    // every configuration crosses and this one is only a command line. Both
    // refusals name the same ranges, from the same constants.
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
    WatcherCooldownMinutes,
    WatcherMarginPercent,
    WatcherNoReturn,
}

const GROUP_KEYS: [GroupKey; 6] = [
    GroupKey::Strategy,
    GroupKey::WatcherMayAct,
    GroupKey::WatcherThresholdPercent,
    GroupKey::WatcherCooldownMinutes,
    GroupKey::WatcherMarginPercent,
    GroupKey::WatcherNoReturn,
];

impl GroupKey {
    fn as_str(self) -> &'static str {
        match self {
            GroupKey::Strategy => "strategy",
            GroupKey::WatcherMayAct => "watcher-may-act",
            GroupKey::WatcherThresholdPercent => "watcher-threshold-percent",
            GroupKey::WatcherCooldownMinutes => "watcher-cooldown-minutes",
            GroupKey::WatcherMarginPercent => "watcher-margin-percent",
            GroupKey::WatcherNoReturn => "watcher-no-return",
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
            GroupKey::WatcherCooldownMinutes => config.watcher_cooldown_minutes.to_string(),
            GroupKey::WatcherMarginPercent => config.watcher_margin_percent.to_string(),
            GroupKey::WatcherNoReturn => config.watcher_no_return.to_string(),
        }
    }

    fn write(self, config: &mut GroupConfig, value: &str) -> Result<()> {
        match self {
            GroupKey::Strategy => config.strategy = strategy(value)?,
            GroupKey::WatcherMayAct => config.watcher_may_act = yes_or_no(self.as_str(), value)?,
            GroupKey::WatcherThresholdPercent => {
                config.watcher_threshold_percent = percentage(self.as_str(), value)?
            }
            GroupKey::WatcherCooldownMinutes => {
                config.watcher_cooldown_minutes = minutes(self.as_str(), value)?
            }
            GroupKey::WatcherMarginPercent => {
                config.watcher_margin_percent = percentage(self.as_str(), value)?
            }
            GroupKey::WatcherNoReturn => {
                config.watcher_no_return = yes_or_no(self.as_str(), value)?
            }
        }
        Ok(())
    }

    /// What the Group now does, which is the half of the answer the value
    /// itself does not give.
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
            GroupKey::WatcherMayAct if config.watcher_may_act => format!(
                "`perch watch` may Switch within `{group}` on your behalf when \
                 the Account you are on reaches its threshold. {ONLY_WHILE_IT_RUNS}"
            ),
            GroupKey::WatcherMayAct => format!(
                "`perch watch` will not act on `{group}`: started on an Account \
                 in it, it says so and exits rather than watching. A Group only \
                 ever changes underneath you because you said it could."
            ),
            GroupKey::WatcherThresholdPercent => format!(
                "`perch watch` Switches within `{group}` once that much of the \
                 fullest Quota Window of the Account you are on has been used. \
                 {ONLY_WHILE_IT_RUNS}"
            ),
            GroupKey::WatcherCooldownMinutes if config.watcher_cooldown_minutes == 0 => format!(
                "`perch watch` will Switch within `{group}` as often as the \
                 figures say to, with no wait between one Switch and the next — \
                 and `watcher-no-return` goes with it, because a no-return of no \
                 minutes bars nothing. The margin is then all that stands \
                 between two Accounts either side of the threshold and a \
                 ping-pong. {ONLY_WHILE_IT_RUNS}"
            ),
            GroupKey::WatcherCooldownMinutes => format!(
                "`perch watch` leaves at least that long between two Switches \
                 within `{group}`, however the figures move in between. \
                 {}{ONLY_WHILE_IT_RUNS}",
                match config.watcher_no_return {
                    true =>
                        "It is also how long the Account it just left stays \
                             barred from being Switched back to. ",
                    false => "",
                },
            ),
            GroupKey::WatcherMarginPercent => format!(
                "`perch watch` Switches within `{group}` only to an Account with \
                 no more than {}% of its fullest Quota Window used — that much \
                 clear of the {}% it moves you at. A candidate barely emptier \
                 than the Account you are on is what a ping-pong is made of. \
                 {ONLY_WHILE_IT_RUNS}",
                crate::watch::Policy::of(config).ceiling(),
                config.watcher_threshold_percent,
            ),
            // A no-return is measured in cooldowns, so a Group with no cooldown
            // has no no-return either however this reads. Said rather than left
            // for somebody to find out, because "will not Switch back until the
            // cooldown of 0 minutes has passed" is a sentence that means the
            // opposite of what it appears to promise.
            GroupKey::WatcherNoReturn
                if config.watcher_no_return && config.watcher_cooldown_minutes == 0 =>
            {
                format!(
                    "`perch watch` bars nothing within `{group}`: no-return lasts \
                     one cooldown, and `watcher-cooldown-minutes` is 0. Setting a \
                     cooldown is what gives this something to measure. \
                     {ONLY_WHILE_IT_RUNS}"
                )
            }
            GroupKey::WatcherNoReturn if config.watcher_no_return => format!(
                "`perch watch` will not Switch back to the Account it just left \
                 within `{group}` until the cooldown of {} minutes has passed, \
                 whatever the figures say in between. {ONLY_WHILE_IT_RUNS}",
                config.watcher_cooldown_minutes,
            ),
            GroupKey::WatcherNoReturn => format!(
                "`perch watch` may Switch straight back to the Account it just \
                 left within `{group}`. Only the cooldown and the margin then \
                 stand between two Accounts either side of the threshold and a \
                 ping-pong. {ONLY_WHILE_IT_RUNS}"
            ),
        }
    }
}

/// Said of both of the watcher's fields, in one place, because two sentences
/// about it would sooner or later say two different things: nothing here is a
/// service that has been switched on (ADR 0013).
///
/// Both ways of running one are named, because a setting that only governed the
/// loop would be a setting somebody scheduling `--once` had no reason to read.
const ONLY_WHILE_IT_RUNS: &str = "Only while a watcher is running — the loop in \
     the terminal you started it in, or a `perch watch --once` your scheduler \
     runs. It is not a daemon, and nothing here switches one on.";

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

/// A percentage, refused with the numbers that would have been accepted.
///
/// The range is the registry's to state (`A_PERCENTAGE`), so the refusal a
/// number too large for the field gets and the one a number the field can hold
/// but the policy cannot gets are the same sentence. To the script that
/// mistyped, `300` and `101` are the same mistake.
fn percentage(key: &str, value: &str) -> Result<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|percent| *percent <= 100)
        .ok_or_else(|| not_a_value(key, value, registry::A_PERCENTAGE))
}

/// A count of minutes, refused the same way.
fn minutes(key: &str, value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|minutes| *minutes <= registry::MAX_WATCHER_COOLDOWN_MINUTES)
        .ok_or_else(|| not_a_value(key, value, &registry::a_cooldown()))
}

/// A value refused for the value it is, said to somebody who just typed it —
/// which is why it is not the registry's `out_of_range`, whose reader is
/// somebody looking at a file and needs to be told which Group it is in.
fn not_a_value(key: &str, value: &str, accepted: &str) -> PerchError {
    PerchError::Invalid(format!(
        "`{value}` is not a value `{key}` takes. It takes {accepted}."
    ))
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

    /// Every message about a watcher setting that describes the watcher acting
    /// says what it is not — a Group that may be acted on is not a service that
    /// has been switched on (ADR 0013). Asserted over every key in every shape
    /// its message branches on, because the branch that forgets is always the
    /// one somebody added last.
    #[test]
    fn every_message_about_the_watcher_acting_says_it_only_acts_while_the_loop_runs() {
        let shapes = [
            GroupConfig::default(),
            GroupConfig {
                watcher_may_act: true,
                ..GroupConfig::default()
            },
            // The two settings that read differently at zero, and the one that
            // reads differently when no-return is off.
            GroupConfig {
                watcher_cooldown_minutes: 0,
                ..GroupConfig::default()
            },
            GroupConfig {
                watcher_margin_percent: 0,
                ..GroupConfig::default()
            },
            GroupConfig {
                watcher_no_return: false,
                ..GroupConfig::default()
            },
        ];

        for config in shapes {
            for key in GROUP_KEYS {
                if key == GroupKey::Strategy {
                    continue;
                }
                let said = key.what_that_means(&config, "work");
                assert!(
                    said.contains(ONLY_WHILE_IT_RUNS) || said.contains("will not act on"),
                    "`{}` at {config:?} says nothing about being a loop rather \
                     than a service: {said}",
                    key.as_str(),
                );
            }
        }
    }

    /// A no-return is measured in cooldowns, so a Group with no cooldown has no
    /// no-return. Saying otherwise would be a promise the loop does not keep.
    #[test]
    fn no_return_without_a_cooldown_says_it_bars_nothing() {
        let mut registry = holding_a_group();
        set(
            &mut registry,
            &words(&["work", "watcher-cooldown-minutes", "0"]),
        )
        .unwrap();

        let said = set(
            &mut registry,
            &words(&["work", "watcher-no-return", "true"]),
        )
        .unwrap();

        assert!(
            said.iter().any(|line| line.contains("bars nothing")),
            "{said:?}"
        );
        assert!(
            !said
                .iter()
                .any(|line| line.contains("will not Switch back")),
            "a sentence promising it will not Switch back is one the loop does \
             not keep at a cooldown of zero: {said:?}"
        );
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
