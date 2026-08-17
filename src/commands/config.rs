//! `perch config` — changing the rules Perch chooses Accounts by, from a
//! script.
//!
//! Perch has to be complete over SSH and in CI, so every capability it has is
//! reachable non-interactively (ADR 0011). This is the one that changes the
//! rules rather than the state: which Account a Cycle prefers, whether the
//! watcher may act, and whether the ungrouped Accounts may be Cycled among at
//! all.
//!
//! **A Setting is said about the Scope it governs** (ADR 0051). A Scope — each
//! Group, and the Accounts in no Group taken together — holds its own full
//! Settings, and there is nothing above it: what nobody has said anything about
//! is the compiled-in default rather than somebody else's value. Nothing is two
//! layers deep and an Account carries nothing at all.
//!
//! So every `set` is `<scope> <key> <value>`, and a `set` that names no Scope is
//! refused rather than landing somewhere. There is no word for "everywhere" —
//! `global` is reserved so that the refusal, rather than a Setting that appeared
//! to take, is where somebody finds that out.
//!
//! **Reading is not writing.** Bare `perch config get` survives and prints every
//! Scope's Config in full: a read has no subject to be wrong about, and a write
//! does. Every line it prints is the tail of the `perch config set` that would
//! restore it, so reading the Config and writing it back are the same
//! vocabulary and a script needs no parser.
//!
//! The watcher's two fields say whether `perch watcher run` may Switch within a
//! Scope and at what Utilization it does (ADR 0046). Every message that
//! describes the watcher *acting* says the same thing about what it is not: a
//! Scope that may be acted on is not a service that has been switched on,
//! because nothing acts on it unless somebody is running the loop. The one
//! message that need not is the one saying the watcher may not act on this
//! Scope at all.

use std::io::Write;

use crate::adopt;
use crate::commands::{group, only_the_registry, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Registry, Scope, Strategy, UNGROUPED};

/// What was asked of `perch config`, as the words that were typed.
///
/// The words are carried rather than resolved because telling somebody which
/// form they seem to have meant is part of what this command does, and a parser
/// that had already thrown the words away could not.
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
    // The lock is taken inside the match rather than above it, so it is taken
    // only by the half that writes. `perch config get` reads, and a reader
    // that takes the write lock waits out whatever holds it and then fails with
    // "another `perch` holds it" — `perch watcher run` takes that lock every
    // round, and `perch status --refresh` holds it across every network read.
    // Same rule `perch status` states for itself and `perch list` follows.
    match command {
        // The half that writes changes the registry and reaches nothing else,
        // which is the whole of what `only_the_registry` is for (ADR 0057). The
        // shape was written here first and lived here alone; `enable`, `alias`
        // and `group` were spelling it out by hand.
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
                    &format!("`{}` on {}", key.as_str(), scope.described()),
                    &was,
                    &now,
                ),
                key.what_that_means(registry, &scope),
            ])
        }
        [first, second] => match addressed(registry, first) {
            // A Scope and a key with nothing to set them to. The key is parsed
            // first so that a mistyped one is answered as what it is: told only
            // that the *value* was missing, somebody who mistyped the key adds a
            // value, runs it again, and only then learns what the mistake was —
            // and the first message pointed away from it.
            Ok(scope) => {
                Setting::parse(second, &scope)?;
                Err(PerchError::Invalid(format!(
                    "`perch config set {first} {second}` names {} and a key, but \
                     nothing to set it to. `perch config set <scope> <key> \
                     <value>` sets one.",
                    scope.described(),
                )))
            }
            // A key where the Scope goes is the form that set a value
            // everywhere, and there is no everywhere (ADR 0051) — answered as
            // the missing subject it is.
            Err(_) if Setting::parse_quietly(first).is_some() => {
                Err(no_scope_was_named(registry, first, second))
            }
            // Anything else is a word that was meant to name a Scope and does
            // not, which is the mistake the three-word form is already answered
            // for. Handed back as it came rather than recast as a key: `perch
            // config set wrok strategy` is a Group typo, and being offered
            // `wrok` as a Setting sends somebody looking for the wrong mistake.
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

/// Every Setting Perch holds, Scope by Scope.
///
/// The whole of it, and no shorter than that: with no layer above a Scope,
/// every value a Scope holds is a value said about that Scope, and a line left
/// out here would be a line nothing else prints. Which is also why this is
/// [`scope_lines`] over every Scope rather than a second idea of what a Config
/// is — the two used to differ, and the difference was the layer.
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
    if registry::means_ungrouped(name) {
        return Ok(Scope::Ungrouped);
    }
    // The one word that has to be answered here rather than left to fall
    // through. `global` is what somebody types when they mean every Scope at
    // once, and `no_such_group` would answer it with "Declare it with `perch
    // group add global`" — advice the registry refuses, and which would be
    // worse if it did not: a Group by that name would take every later `perch
    // config set global …` quietly and leave every other Scope as it was.
    if registry::means_global(name) {
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

/// A key typed where a Scope goes, which is what the two-word form used to be.
///
/// `None` for a word that is not a key either, which is an ordinary mistyped
/// Group name and is `group::no_such_group`'s to answer. Kept apart because the
/// two send somebody to different places: one to the spelling of a Group, and
/// one to the form that has a subject in it.
fn a_setting_is_not_a_scope(word: &str) -> Option<PerchError> {
    let key = Setting::parse_quietly(word)?.as_str();
    Some(PerchError::NotFound(format!(
        "`{key}` is a Setting rather than a Scope, and a Setting is said about \
         the Scope it governs: `perch config set <scope> {key} <value>` sets one \
         and `perch config get <scope> {key}` reads it."
    )))
}

/// Two words with no Scope among them, which is the form that set a value
/// everywhere until there was no everywhere to set it at (ADR 0051).
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
        counted(words),
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
        counted(words),
    ))
}

fn counted(words: &[String]) -> String {
    match words.len() {
        1 => "1 word".to_string(),
        count => format!("{count} words"),
    }
}

/// One Setting, as `perch config` names it.
///
/// One vocabulary rather than two. There were two — the keys a Scope carried and
/// the keys addressed by naming no Scope — and the split was the layer: with
/// every Setting said about a Scope, what is left is a single list, of which one
/// entry is carried by one Scope alone (see [`Setting::carried_by`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Interchangeable,
    Strategy,
    WatcherMayAct,
    WatcherThresholdPercent,
}

/// Every Setting there is, in the order every surface offers them. The
/// declaration a Scope is Cycled within at all comes first, because the rest of
/// the page says how it is Cycled.
pub const SETTINGS: [Setting; 4] = [
    Setting::Interchangeable,
    Setting::Strategy,
    Setting::WatcherMayAct,
    Setting::WatcherThresholdPercent,
];

impl Setting {
    pub fn as_str(self) -> &'static str {
        match self {
            Setting::Interchangeable => "interchangeable",
            Setting::Strategy => "strategy",
            Setting::WatcherMayAct => "watcher-may-act",
            Setting::WatcherThresholdPercent => "watcher-threshold-percent",
        }
    }

    /// Whether this Scope carries the Setting at all.
    ///
    /// One Scope carries a key the others do not, and it is the Accounts in no
    /// Group: `interchangeable` is the declaration that they are a set worth
    /// Cycling within, and a Group **is** that declaration (ADR 0002). Printing
    /// it against a Group and then refusing to set it would break the invariant
    /// the whole command rests on — every line `get` prints is the tail of the
    /// `set` that would restore it — so the honest form is silence.
    pub fn carried_by(self, scope: &Scope) -> bool {
        self != Setting::Interchangeable || *scope == Scope::Ungrouped
    }

    /// The Setting a word names, asked of the Scope it was named about.
    ///
    /// Refused two ways, because they are two different mistakes: a word that
    /// is no key at all, and `interchangeable` asked of a Group, which is a
    /// real key said about the one Scope that cannot carry it.
    fn parse(name: &str, scope: &Scope) -> Result<Self> {
        match Self::parse_quietly(name) {
            Some(key) if key.carried_by(scope) => Ok(key),
            Some(_) => Err(PerchError::Invalid(format!(
                "`{}` is the declaration that the Accounts in no Group are \
                 interchangeable at all, and only they carry it — a Group is \
                 that declaration rather than something that holds one (ADR \
                 0002). `perch config set {UNGROUPED} {} <value>` says it.",
                Setting::Interchangeable.as_str(),
                Setting::Interchangeable.as_str(),
            ))),
            None => Err(PerchError::Invalid(format!(
                "`{name}` is not a Setting {} carries. The ones it carries are {}.",
                scope.described(),
                listed(&vocabulary(scope)),
            ))),
        }
    }

    /// The same lookup where failing is an answer rather than a refusal — for
    /// the forms that have a second thing to try — and without a Scope, because
    /// they are asking what a word *is* rather than what it may be said about.
    fn parse_quietly(name: &str) -> Option<Self> {
        SETTINGS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
    }

    /// The value this Scope holds, as `get` prints it and `set` would take it
    /// back.
    fn of(self, registry: &Registry, scope: &Scope) -> String {
        let settings = registry.settings(scope);
        match self {
            Setting::Interchangeable => registry.ungrouped.interchangeable.to_string(),
            Setting::Strategy => settings.strategy.as_str().to_string(),
            Setting::WatcherMayAct => settings.watcher_may_act.to_string(),
            Setting::WatcherThresholdPercent => settings.watcher_threshold_percent.to_string(),
        }
    }

    /// Sets this Setting on one Scope.
    ///
    /// Applied to a copy of what the Scope holds and checked over the whole of
    /// it before anything lands, so configuration that would not mean anything
    /// never reaches the Scope it was meant for: a refused `set` leaves every
    /// Setting exactly as it found it. Asked again over the whole Scope after
    /// the value parses, because the registry is the boundary every Config
    /// crosses and this one is only a command line — and both refusals name the
    /// same ranges, from the same constants.
    fn write(self, registry: &mut Registry, scope: &Scope, value: &str) -> Result<()> {
        let mut settings = registry.settings(scope);
        // Carried beside the Settings rather than in them, because a Group has
        // no such line: `parse` has already refused this key on any Scope but
        // the Accounts in no Group.
        let mut interchangeable = registry.ungrouped.interchangeable;
        match self {
            Setting::Interchangeable => interchangeable = yes_or_no(self.as_str(), value)?,
            Setting::Strategy => settings.strategy = strategy(value)?,
            Setting::WatcherMayAct => settings.watcher_may_act = yes_or_no(self.as_str(), value)?,
            Setting::WatcherThresholdPercent => {
                settings.watcher_threshold_percent = percentage(self.as_str(), value)?
            }
        }
        settings.validate(scope)?;
        *registry
            .settings_mut(scope)
            .expect("the Scope was just addressed") = settings;
        registry.ungrouped.interchangeable = interchangeable;
        Ok(())
    }

    /// What the Scope now does, which is the half of the answer the value
    /// itself does not give.
    fn what_that_means(self, registry: &Registry, scope: &Scope) -> String {
        let settings = registry.settings(scope);
        let within = scope.within();
        match self {
            Setting::Interchangeable if registry.ungrouped.interchangeable => {
                "A bare `perch switch` from an Account in no Group now Cycles \
                 among the other ungrouped Accounts. That declares every \
                 ungrouped Account interchangeable at once, present and future \
                 — including the next one `perch add` creates (ADR 0017)."
                    .to_string()
            }
            Setting::Interchangeable => {
                "A bare `perch switch` from an Account in no Group switches \
                 nowhere and says why. Being ungrouped is the absence of a \
                 declaration that Accounts are interchangeable, not a weaker \
                 form of one (ADR 0017). It is also what gates the watcher \
                 there, so nothing acts on those Accounts unasked while it is \
                 off."
                    .to_string()
            }
            Setting::Strategy => match settings.strategy {
                Strategy::MostHeadroom => format!(
                    "A Cycle {within} prefers the Account with the most \
                     room left, measured by its worst Quota Window (ADR 0012)."
                ),
                Strategy::SoonestReset => format!(
                    "A Cycle {within} prefers the Account whose fullest \
                     Quota Window resets soonest, so perishable quota is spent \
                     rather than wasted. Headroom is still measured by the worst \
                     window (ADR 0012), so an exhausted Account is still never \
                     chosen however soon it comes back."
                ),
            },
            Setting::WatcherMayAct if settings.watcher_may_act => format!(
                "`perch watcher run` may Switch {within} on your behalf when \
                 the Account you are on reaches its threshold.{} {ONLY_WHILE_IT_RUNS}",
                gated(registry, scope),
            ),
            Setting::WatcherMayAct => format!(
                "`perch watcher run` will not act {within}: started on an \
                 Account there, it says so and exits rather than watching. \
                 Nothing only ever changes underneath you because you said it \
                 could."
            ),
            Setting::WatcherThresholdPercent => format!(
                "`perch watcher run` Switches {within} once that much of the \
                 fullest Quota Window of the Account you are on has been used. \
                 {ONLY_WHILE_IT_RUNS}"
            ),
        }
    }
}

/// The keys one Scope carries, in the order they are offered.
fn vocabulary(scope: &Scope) -> Vec<&'static str> {
    SETTINGS
        .into_iter()
        .filter(|key| key.carried_by(scope))
        .map(Setting::as_str)
        .collect()
}

/// The second yes the Accounts in no Group need, said wherever permission for
/// the watcher to act is (ADR 0017).
///
/// Two statements rather than one said twice: `interchangeable` declares those
/// Accounts a set worth moving between, and `watcher-may-act` lets something
/// move between them unasked. A Group needs only the second, because being a
/// Group is the first. Named here rather than left to be discovered by the
/// watcher declining.
fn gated(registry: &Registry, scope: &Scope) -> String {
    match scope {
        Scope::Ungrouped if !registry.ungrouped.interchangeable => format!(
            " It does not act there yet: `{}` is false, and that is a separate \
             declaration that those Accounts are interchangeable at all (ADR \
             0017) — `perch config set {UNGROUPED} {} true` makes it.",
            Setting::Interchangeable.as_str(),
            Setting::Interchangeable.as_str(),
        ),
        Scope::Ungrouped => format!(
            " Those Accounts have also been declared interchangeable, which is \
             the other half of it: the watcher acts here only where `{}` is on \
             too (ADR 0017).",
            Setting::Interchangeable.as_str(),
        ),
        Scope::Group(_) => String::new(),
    }
}

/// Said of both of the watcher's fields, in one place, because two sentences
/// about it would sooner or later say two different things: nothing here is a
/// service that has been switched on (ADR 0013).
///
/// All three ways of running one are named, because a setting that only governed
/// the loop would be a setting somebody with a Service, or somebody scheduling
/// a Check, had no reason to read (ADR 0040).
const ONLY_WHILE_IT_RUNS: &str = "Only while a Watcher is running — the loop in \
     the terminal you started it in, a Service `perch watcher install` set up, \
     or a `perch watcher check` your scheduler runs. Nothing here starts one.";

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
/// The range is the registry's to state (`a_percentage`), so the refusal a
/// number too large for the field gets and the one a number the field can hold
/// but the policy cannot gets are the same sentence. To the script that
/// mistyped, `300` and `101` are the same mistake.
fn percentage(key: &str, value: &str) -> Result<u8> {
    value
        .parse::<u8>()
        .ok()
        .filter(|percent| *percent <= registry::MAX_PERCENTAGE)
        .ok_or_else(|| not_a_value(key, value, &registry::a_percentage()))
}

/// A value refused for the value it is, said to somebody who just typed it —
/// which is why it is not the registry's `out_of_range`, whose reader is
/// somebody looking at a file and needs to be told which Scope it is in.
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
    use crate::registry::Settings;

    fn holding_a_group() -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
    }

    /// Every surface agrees what a percentage is.
    ///
    /// The bound is stated in three places — in `Settings::validate`, in the
    /// parser here, and inside the sentence both of them quote — and a value
    /// one of them takes and another refuses is a Setting somebody can write
    /// and then not be allowed to keep.
    #[test]
    fn every_surface_agrees_what_a_percentage_is() {
        let most = registry::MAX_PERCENTAGE;
        let past_it = u32::from(most) + 1;

        let setting = Setting::WatcherThresholdPercent;
        // What `perch config set` accepts.
        percentage(setting.as_str(), &most.to_string()).expect("the top of the range");
        percentage(setting.as_str(), &past_it.to_string()).expect_err("and one past it");

        // And what a registry somebody edited by hand is refused for.
        let scope = work();
        Settings {
            watcher_threshold_percent: most,
            ..Settings::default()
        }
        .validate(&scope)
        .expect("the top of the range is a value the registry holds");

        let refused = Settings {
            watcher_threshold_percent: most.saturating_add(1),
            ..Settings::default()
        }
        .validate(&scope)
        .expect_err("and one past it is not");
        assert!(
            refused.to_string().contains(&registry::a_percentage()),
            "refused in the words every other surface uses: {refused}"
        );
    }

    fn words(said: &[&str]) -> Vec<String> {
        said.iter().map(|word| word.to_string()).collect()
    }

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    /// Every Setting has a subject, and a `set` that names none is refused
    /// rather than landing somewhere (ADR 0051).
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

    /// A Setting said about one Scope reaches that Scope and no other. There is
    /// no layer for it to arrive by, which is the whole of the decision.
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

    /// The grant is the one that matters most: a Group declared after somebody
    /// let the watcher into another one is a Group nobody has said anything
    /// about (ADR 0051).
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
    fn a_value_that_means_nothing_leaves_the_scope_as_it_was() {
        let mut registry = holding_a_group();
        set(&mut registry, &words(&["work", "watcher-may-act", "true"])).unwrap();

        // Refused by the range the registry enforces, after the value parsed.
        set(
            &mut registry,
            &words(&["work", "watcher-threshold-percent", "101"]),
        )
        .expect_err("a Utilization threshold is a percentage");

        let settings = registry.settings(&work());
        assert_eq!(settings.watcher_threshold_percent, 80);
        assert!(
            settings.watcher_may_act,
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

    /// Every line names the Scope it is about, because that is what would set it
    /// again — there is no second reading in the word count any more.
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

    /// The one key one Scope carries. A Group is the declaration that its
    /// Accounts are interchangeable, so it neither shows the line nor takes it.
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

    /// Every message about a watcher setting that describes the watcher acting
    /// says what it is not — a Scope that may be acted on is not a service that
    /// has been switched on (ADR 0013). Asserted over every key in every shape
    /// its message branches on, because the branch that forgets is always the
    /// one somebody added last.
    #[test]
    fn every_message_about_the_watcher_acting_says_it_only_acts_while_the_loop_runs() {
        for granted in [false, true] {
            let mut registry = holding_a_group();
            for scope in [Scope::Ungrouped, work()] {
                set(
                    &mut registry,
                    &words(&[scope.word(), "watcher-may-act", &granted.to_string()]),
                )
                .unwrap();
                for key in [Setting::WatcherMayAct, Setting::WatcherThresholdPercent] {
                    let said = key.what_that_means(&registry, &scope);
                    assert!(
                        said.contains(ONLY_WHILE_IT_RUNS) || said.contains("will not act"),
                        "`{}` granted={granted} on {scope:?} says nothing about being a \
                         loop rather than a service: {said}",
                        key.as_str(),
                    );
                }
            }
        }
    }

    /// ADR 0017: the Accounts in no Group need two independent yeses, and the
    /// message that grants the watcher names the other one. A Group needs only
    /// the grant, because being a Group is the declaration.
    #[test]
    fn granting_the_watcher_names_the_second_yes_the_ungrouped_accounts_need() {
        let mut registry = holding_a_group();
        for scope in ["ungrouped", "work"] {
            set(&mut registry, &words(&[scope, "watcher-may-act", "true"])).unwrap();
        }

        let said = Setting::WatcherMayAct.what_that_means(&registry, &Scope::Ungrouped);
        assert!(said.contains("interchangeable"), "{said}");
        assert!(
            !Setting::WatcherMayAct
                .what_that_means(&registry, &work())
                .contains("interchangeable"),
            "a named Group needs no second yes",
        );
    }

    #[test]
    fn a_vocabulary_is_named_as_a_sentence() {
        assert_eq!(listed(&["one"]), "`one`");
        assert_eq!(listed(&["one", "two"]), "`one` and `two`");
        assert_eq!(listed(&["one", "two", "three"]), "`one`, `two` and `three`");
    }

    /// A Group named after a key is now an ordinary Scope: with every Setting
    /// said about one, a word in the Scope's place can only be a Scope.
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

    /// A word in the Scope's place that is a key is a `set` missing its subject,
    /// not a Group nobody declared — and being sent to check the spelling of a
    /// Group is being sent to look for a mistake that is not the problem.
    #[test]
    fn a_key_where_a_scope_goes_says_a_setting_needs_a_subject() {
        let registry = holding_a_group();

        let refused = get(&registry, &words(&["watcher-may-act"]))
            .expect_err("a Setting on its own is about nothing");

        let said = refused.to_string();
        assert!(said.contains("rather than a Scope"), "{said}");
        assert!(!said.contains("No Group called"), "{said}");
    }

    /// The refusal for a Scope named with a key and nothing to set it to.
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
