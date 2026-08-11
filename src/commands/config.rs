//! `perch config` — changing the rules Perch chooses Accounts by, from a
//! script.
//!
//! Perch has to be complete over SSH and in CI, so every capability it has is
//! reachable non-interactively (ADR 0011). This is the one that changes the
//! rules rather than the state: which Account a Cycle prefers, whether the
//! watcher may act, and whether the ungrouped Accounts may be Cycled among at
//! all.
//!
//! **Config is two layers deep** (ADR 0002, amended). Every Setting exists at
//! Global, where it is the value that applies until something narrower is said.
//! A Scope — a Group, or the Accounts in no Group — holds an Override for a
//! Setting it wants different and Inherits the rest. Nothing is three layers
//! deep and an Account carries nothing at all.
//!
//! Which layer is meant is read off how many words were said, and always has
//! been: three name a Scope and set an Override, two set Global's default.
//! Nothing else would do — a reserved Group name would make a Group mean two
//! contradictory things, and a flag would make the Global form the odd one out.
//! `unset` is the same vocabulary one word shorter, and at Global it is
//! refused: there is nothing above Global to Inherit from, so clearing there is
//! not a state that exists.
//!
//! `get` prints each Setting as the tail of the `set` that would restore it, so
//! reading the Config and writing it back are the same vocabulary and a script
//! needs no parser. That is also how it says where a value came from: **the
//! layer a value came from is the number of words that would set it again.** A
//! Setting a Group Inherits prints as `<key> <value>`, because Global is what
//! would set it; one it Overrides prints as `<group> <key> <value>`.
//!
//! The watcher's five fields say whether `perch watch` may Switch within a
//! Scope, at what Utilization it does, how often it may, how much emptier the
//! Account it moves to has to be, and whether the one it just left counts
//! (ADR 0013). Every message that describes the watcher *acting* says the same
//! thing about what it is not: a Scope that may be acted on is not a service
//! that has been switched on, because nothing acts on it unless somebody is
//! running the loop. The one message that need not is the one saying the
//! watcher may not act on this Scope at all.

use std::io::Write;

use crate::adopt;
use crate::commands::{group, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Overrides, Registry, Scope, Settings, Strategy, UNGROUPED};

/// What was asked of `perch config`, as the words that were typed.
///
/// The words are carried rather than resolved because which of them names a
/// Scope is not something the command line can know: the forms differ only in
/// how many there are, and telling the user which they seem to have meant is
/// part of what this command does.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum ConfigCommand {
    /// Set one Setting, and say what it now means.
    ///
    /// Every Setting exists at Global — `strategy`, and the watcher's
    /// `watcher-may-act`, `watcher-threshold-percent`,
    /// `watcher-cooldown-minutes`, `watcher-margin-percent` and
    /// `watcher-no-return` (ADR 0013) — where it is the value that applies
    /// until something narrower is said. Naming a Scope first sets that Scope's
    /// Override instead: a Group by name, or `ungrouped` for the Accounts in no
    /// Group. `cycle-ungrouped` is Global's alone.
    Set {
        /// `<scope> <key> <value>`, or `<key> <value>` for Global's default.
        #[arg(value_name = "WORDS", num_args = 1.., required = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },

    /// Clear a Scope's Override, so it Inherits Global again from then on.
    ///
    /// Inheriting is following rather than copying once: the Scope tracks
    /// Global as Global changes. Refused at Global, which has nothing above it
    /// to Inherit from.
    Unset {
        /// `<scope> <key>`.
        #[arg(value_name = "WORDS", num_args = 1.., required = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },

    /// Read Settings back, each one in the form that would set it again.
    ///
    /// With nothing named it prints Global's Config and then every Override
    /// there is. A Scope prints every Setting in force for it, each as the tail
    /// of the `set` that would restore it — so a two-word line is Inherited
    /// from Global and a three-word line is that Scope's own Override.
    Get {
        /// Nothing, `<scope>`, `<key>`, or `<scope> <key>`.
        #[arg(value_name = "WORDS", num_args = 0.., allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

pub fn run(host: &dyn Host, command: ConfigCommand, out: &mut dyn Write) -> Result<()> {
    // The lock is taken inside the match rather than above it, so it is taken
    // only by the halves that write. `perch config get` reads, and a reader that
    // takes the write lock waits out whatever holds it and then fails with
    // "another `perch` holds it" — `perch watch` takes that lock every round,
    // and `perch status --refresh` holds it across every network read. Same
    // rule `perch status` states for itself and `perch list` follows.
    match command {
        ConfigCommand::Set { words } => written(host, out, |registry| set(registry, &words)),
        ConfigCommand::Unset { words } => written(host, out, |registry| unset(registry, &words)),
        ConfigCommand::Get { words } => {
            let registry = adopt::ensure_adopted(host)?;
            for line in get(&registry, &words)? {
                say(out, &line)?;
            }
            Ok(())
        }
    }
}

/// The half that writes: under Perch's own lock, and saved only where the
/// change was accepted.
fn written(
    host: &dyn Host,
    out: &mut dyn Write,
    change: impl FnOnce(&mut Registry) -> Result<Vec<String>>,
) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
    let said = change(&mut registry)?;
    registry::save(host, &mut perch, &registry)?;
    for line in said {
        say(out, &line)?;
    }
    Ok(())
}

/// Sets one Setting, returning what to tell the user: what it is now, and what
/// that means for them.
fn set(registry: &mut Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [scope, key, value] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key)?;
            let was = key.in_force(registry, &scope);

            // Applied to a copy, so configuration that would not mean anything
            // never reaches the Scope it was meant for: a refused `set` leaves
            // every Setting exactly as it found it.
            let mut overrides = held(registry, &scope).clone();
            key.write(&mut overrides, value)?;
            // Asked again over the whole Scope, because the registry is the
            // boundary every Config crosses and this one is only a command
            // line. Both refusals name the same ranges, from the same constants.
            overrides.validate(&scope)?;
            *registry
                .overrides_mut(&scope)
                .expect("the Scope was just addressed") = overrides;

            let now = key.in_force(registry, &scope);
            Ok(vec![
                overridden(&key, &scope, &was, &now),
                key.what_that_means(&now.settings, &scope),
            ])
        }
        [key, value] => {
            // Two words address Global — unless the first is a Scope, in which
            // case what is missing is the value rather than the layer. A key
            // Perch owns wins over a Group that shares its name: naming no
            // Scope is the only way to reach Global's value, while the Group's
            // Override is still reachable with three words.
            if Key::parse_quietly(key).is_err()
                && let Ok(scope) = addressed(registry, key)
            {
                return Err(PerchError::Invalid(format!(
                    "`perch config set {key} {value}` names {} and a key, but nothing \
                     to set it to. `perch config set <scope> <key> <value>` sets one.",
                    scope.described(),
                )));
            }
            let key = Key::parse(key)?;
            let was = key.read(&registry.global.settings, &registry.global);
            let mut global = registry.global.clone();
            key.write(&mut global, value)?;
            Overrides::from(global.settings.clone()).validate(&Scope::Global)?;
            registry.global = global;
            let now = key.read(&registry.global.settings, &registry.global);

            let mut said = vec![changed(
                &format!("`{}` at Global", key.as_str()),
                &was,
                &now,
            )];
            if let Key::Setting(setting) = key
                && let Some(following) = inheriting(registry, setting)
            {
                said.push(following);
            }
            said.push(key.what_that_means(&registry.global.settings, &registry.global));
            Ok(said)
        }
        _ => Err(how_set_is_addressed(words)),
    }
}

/// Clears a Scope's Override, so it Inherits Global from then on.
fn unset(registry: &mut Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [scope, key] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key)?;
            let was = key.in_force(registry, &scope);
            key.clear(
                registry
                    .overrides_mut(&scope)
                    .expect("the Scope was just addressed"),
            );
            let now = key.in_force(registry, &scope);
            Ok(vec![
                inherited(&key, &scope, &was, &now),
                key.what_that_means(&now.settings, &scope),
            ])
        }
        // Global's values are always set. There is nothing above Global to
        // Inherit from, so this is refused rather than silently accepted: a
        // script told "done" would go on believing a Setting had gone back to
        // something, and there is no something for it to have gone back to.
        [key] => {
            let key = Key::parse(key)?;
            Err(PerchError::Invalid(format!(
                "`{}` cannot be unset at Global. Global is the value that applies \
                 where nothing narrower is said, so it has nothing above it to \
                 Inherit from — `perch config set {} <value>` is how it changes.\n\
                 `perch config unset <scope> {}` clears one Scope's Override so \
                 that Scope Inherits Global again.",
                key.as_str(),
                key.as_str(),
                key.as_str(),
            )))
        }
        _ => Err(how_unset_is_addressed(words)),
    }
}

/// Reads Settings back, in the form that would set them again.
fn get(registry: &Registry, words: &[String]) -> Result<Vec<String>> {
    match words {
        [] => Ok(everything(registry)),
        [one] => {
            // A key Perch owns is answered as a key even if a Group shares its
            // name: naming no Scope is the only way to read Global's own value,
            // while a Scope's Config can still be read one key at a time.
            if let Ok(key) = Key::parse_quietly(one) {
                return Ok(vec![format!(
                    "{} {}",
                    key.as_str(),
                    key.read(&registry.global.settings, &registry.global)
                )]);
            }
            let scope = addressed(registry, one).map_err(|refusal| match refusal {
                PerchError::NotFound(_) => neither_a_key_nor_a_scope(registry, one),
                other => other,
            })?;
            Ok(scope_lines(registry, &scope))
        }
        [scope, key] => {
            let scope = addressed(registry, scope)?;
            let key = Setting::parse(key)?;
            Ok(vec![key.in_force(registry, &scope).as_a_set()])
        }
        _ => Err(how_get_is_addressed(words)),
    }
}

/// Every Setting Perch holds: Global's Config first, then every Override there
/// is, so the shape of the answer matches the shape of the vocabulary.
///
/// Overrides only, below Global. A Scope's Inherited Settings are already on
/// the page as Global's lines, and printing them again as three-word lines
/// would assert Overrides that are not there — after which replaying this
/// output would turn every Inheritance into an Override. What a Scope Inherits
/// is what `perch config get <scope>` is for.
fn everything(registry: &Registry) -> Vec<String> {
    let mut lines: Vec<String> = GLOBAL_KEYS
        .iter()
        .map(|key| {
            format!(
                "{} {}",
                key.as_str(),
                key.read(&registry.global.settings, &registry.global)
            )
        })
        .collect();
    for scope in registry.scopes() {
        let Some(overrides) = registry.overrides(&scope) else {
            continue;
        };
        lines.extend(
            SETTINGS
                .iter()
                .filter(|setting| setting.read(overrides).is_some())
                .map(|setting| setting.in_force(registry, &scope).as_a_set()),
        );
    }
    lines
}

/// Every Setting in force for one Scope, each as the tail of the `set` that
/// would restore it — so the word count says which layer it came from.
fn scope_lines(registry: &Registry, scope: &Scope) -> Vec<String> {
    let mut lines: Vec<String> = SETTINGS
        .iter()
        .map(|setting| setting.in_force(registry, scope).as_a_set())
        .collect();
    // The Setting that decides whether this Scope is Cycled within at all
    // belongs to Global and has no narrower form, so it is printed where it
    // takes effect rather than only where it lives (ADR 0017).
    if *scope == Scope::Ungrouped {
        lines.insert(
            0,
            format!(
                "{} {}",
                Key::CycleUngrouped.as_str(),
                registry.global.cycle_ungrouped
            ),
        );
    }
    lines
}

/// One Setting as it stands for one Scope: the value, and the Scope it came
/// from.
struct InForce {
    key: Setting,
    value: String,
    /// Where the value came from — the Scope itself when it Overrides, and
    /// Global when it Inherits.
    from: Scope,
    /// The whole of the Scope's Settings with this one in them, for the
    /// sentence that says what the value means.
    settings: Settings,
}

impl InForce {
    /// The tail of the `perch config set` that would restore it, which is also
    /// how the layer it came from is said: two words is Global's, three is an
    /// Override.
    fn as_a_set(&self) -> String {
        match self.from.word() {
            Some(word) => format!("{word} {} {}", self.key.as_str(), self.value),
            None => format!("{} {}", self.key.as_str(), self.value),
        }
    }

    fn inherits(&self) -> bool {
        self.from == Scope::Global
    }
}

/// What a Setting is now at a Scope that Overrides it.
fn overridden(key: &Setting, scope: &Scope, was: &InForce, now: &InForce) -> String {
    let subject = format!("`{}` on {}", key.as_str(), scope.described());
    if was.value == now.value && was.inherits() {
        return format!(
            "{subject} is {}, and is now an Override rather than Inherited from Global.",
            now.value
        );
    }
    changed(&subject, &was.value, &now.value)
}

/// What a Setting is now at a Scope that has just stopped Overriding it.
fn inherited(key: &Setting, scope: &Scope, was: &InForce, now: &InForce) -> String {
    let subject = format!("`{}` on {}", key.as_str(), scope.described());
    if was.inherits() {
        return format!(
            "{subject} was already Inherited from Global, which says {}.",
            now.value
        );
    }
    format!(
        "{subject} is Inherited from Global again, which says {}{}. It follows \
         Global from now on rather than holding a value of its own.",
        now.value,
        match was.value == now.value {
            true => " — the same value the Override held",
            false => "",
        },
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

/// Which Scopes a Setting just changed at Global reaches, where any do.
///
/// The whole point of Global carrying the defaults: somebody running four
/// Groups sets a threshold once rather than four times, and this is what tells
/// them it landed in all four.
fn inheriting(registry: &Registry, setting: Setting) -> Option<String> {
    let following: Vec<String> = registry
        .scopes()
        .into_iter()
        .filter(|scope| {
            registry
                .overrides(scope)
                .is_some_and(|held| setting.read(held).is_none())
        })
        .map(|scope| scope.described())
        .collect();
    match following.is_empty() {
        true => None,
        false => Some(format!(
            "{} Inherit it, and go on following Global as it changes.",
            listed_scopes(&following),
        )),
    }
}

/// "A, B and C" over Scopes already described.
fn listed_scopes(described: &[String]) -> String {
    match described {
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    }
}

/// The Scope a word addresses: the Accounts in no Group, or a Group as it was
/// declared.
///
/// Global is not among them, because Global is addressed by naming no Scope at
/// all — that is what makes the number of words the layer.
fn addressed(registry: &Registry, name: &str) -> Result<Scope> {
    if registry::means_ungrouped(name) {
        return Ok(Scope::Ungrouped);
    }
    match registry.declared_group(name) {
        Some(declared) => Ok(Scope::Group(declared.to_string())),
        None => Err(group::no_such_group(registry, name)),
    }
}

/// A Scope's Overrides, for a Scope that has just been addressed.
fn held<'a>(registry: &'a Registry, scope: &Scope) -> &'a Overrides {
    registry
        .overrides(scope)
        .expect("the Scope was just addressed")
}

/// One word that is neither of the two things one word can be.
fn neither_a_key_nor_a_scope(registry: &Registry, word: &str) -> PerchError {
    let groups: Vec<&str> = registry.groups.keys().map(String::as_str).collect();
    let held = if groups.is_empty() {
        "No Groups have been declared yet.".to_string()
    } else {
        format!("Groups Perch holds: {}.", groups.join(", "))
    };
    PerchError::NotFound(format!(
        "`{word}` is neither a Setting nor a Scope Perch holds. Settings: {}. \
         `{UNGROUPED}` addresses the Accounts in no Group. {held}",
        Key::vocabulary(),
    ))
}

/// The forms `set` takes, said whenever the words said were none of them.
fn how_set_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config set` was given {}. It takes a Scope, a key and a value — \
         `perch config set <scope> <key> <value>`, where a Scope is a Group or \
         `{UNGROUPED}` — or a key and a value alone to set Global's default: \
         `perch config set <key> <value>`.",
        counted(words),
    ))
}

/// The forms `unset` takes. It has exactly one, because Global is the layer
/// with nothing above it and every other layer is named.
fn how_unset_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config unset` was given {}. It takes a Scope and a key — `perch \
         config unset <scope> <key>` — which clears that Scope's Override so it \
         Inherits Global again.",
        counted(words),
    ))
}

/// The forms `get` takes, which are not the forms `set` takes: naming fewer
/// words asks about more rather than being short of a value. One sentence
/// serving both would name a form that does not exist.
fn how_get_is_addressed(words: &[String]) -> PerchError {
    PerchError::Invalid(format!(
        "`perch config get` was given {}. It takes a Scope and a key — `perch \
         config get <scope> <key>` — or a key alone to read Global's value. \
         `perch config get <scope>` reads every Setting in force for one Scope, \
         and `perch config get` on its own reads Global and every Override there \
         is.",
        counted(words),
    ))
}

fn counted(words: &[String]) -> String {
    match words.len() {
        1 => "1 word".to_string(),
        count => format!("{count} words"),
    }
}

/// One Setting: a value at Global, and something a narrower Scope may Override
/// (ADR 0002, amended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Strategy,
    WatcherMayAct,
    WatcherThresholdPercent,
    WatcherCooldownMinutes,
    WatcherMarginPercent,
    WatcherNoReturn,
}

/// Every Setting there is, in the order every surface offers them.
pub const SETTINGS: [Setting; 6] = [
    Setting::Strategy,
    Setting::WatcherMayAct,
    Setting::WatcherThresholdPercent,
    Setting::WatcherCooldownMinutes,
    Setting::WatcherMarginPercent,
    Setting::WatcherNoReturn,
];

/// What a value can be stepped through, for the surface that has arrow keys
/// rather than a keyboard full of digits.
///
/// Here rather than in the TUI because it is a fact about the Setting — a bool
/// has two readings, a Strategy has the ones Perch implements, and a percentage
/// has a range — and a second statement of it in the view is how the panel
/// comes to offer a value `set` would refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// `true` or `false`.
    YesOrNo,
    /// One of the readings [`Strategy::ALL`] holds.
    OneOf(Vec<String>),
    /// A whole number between the two, stepped by the third.
    Range { least: u32, most: u32, step: u32 },
}

impl Setting {
    pub fn as_str(self) -> &'static str {
        match self {
            Setting::Strategy => "strategy",
            Setting::WatcherMayAct => "watcher-may-act",
            Setting::WatcherThresholdPercent => "watcher-threshold-percent",
            Setting::WatcherCooldownMinutes => "watcher-cooldown-minutes",
            Setting::WatcherMarginPercent => "watcher-margin-percent",
            Setting::WatcherNoReturn => "watcher-no-return",
        }
    }

    fn parse(name: &str) -> Result<Self> {
        SETTINGS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
            .ok_or_else(|| {
                PerchError::Invalid(format!(
                    "`{name}` is not a Setting a Scope carries. The ones it \
                     carries are {}. `{}` is Global's alone and has no \
                     per-Scope form.",
                    listed(SETTINGS.map(Setting::as_str).as_slice()),
                    Key::CycleUngrouped.as_str(),
                ))
            })
    }

    /// The same lookup where failing is an answer rather than a refusal.
    pub fn named(name: &str) -> Option<Self> {
        SETTINGS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
    }

    /// What this Setting can be stepped through.
    pub fn shape(self) -> Shape {
        match self {
            Setting::Strategy => Shape::OneOf(
                Strategy::ALL
                    .iter()
                    .map(|strategy| strategy.as_str().to_string())
                    .collect(),
            ),
            Setting::WatcherMayAct | Setting::WatcherNoReturn => Shape::YesOrNo,
            // Five at a time, so crossing a useful range is a dozen keystrokes
            // rather than eighty.
            Setting::WatcherThresholdPercent | Setting::WatcherMarginPercent => Shape::Range {
                least: 0,
                most: 100,
                step: 5,
            },
            Setting::WatcherCooldownMinutes => Shape::Range {
                least: 0,
                most: registry::MAX_WATCHER_COOLDOWN_MINUTES,
                step: 5,
            },
        }
    }

    /// The value as `get` prints it and `set` would take it back.
    pub fn of(self, settings: &Settings) -> String {
        match self {
            Setting::Strategy => settings.strategy.as_str().to_string(),
            Setting::WatcherMayAct => settings.watcher_may_act.to_string(),
            Setting::WatcherThresholdPercent => settings.watcher_threshold_percent.to_string(),
            Setting::WatcherCooldownMinutes => settings.watcher_cooldown_minutes.to_string(),
            Setting::WatcherMarginPercent => settings.watcher_margin_percent.to_string(),
            Setting::WatcherNoReturn => settings.watcher_no_return.to_string(),
        }
    }

    /// What a Scope Overrides this with, or `None` where it Inherits.
    pub fn read(self, overrides: &Overrides) -> Option<String> {
        match self {
            Setting::Strategy => overrides
                .strategy
                .map(|strategy| strategy.as_str().to_string()),
            Setting::WatcherMayAct => overrides.watcher_may_act.map(|held| held.to_string()),
            Setting::WatcherThresholdPercent => overrides
                .watcher_threshold_percent
                .map(|held| held.to_string()),
            Setting::WatcherCooldownMinutes => overrides
                .watcher_cooldown_minutes
                .map(|held| held.to_string()),
            Setting::WatcherMarginPercent => overrides
                .watcher_margin_percent
                .map(|held| held.to_string()),
            Setting::WatcherNoReturn => overrides.watcher_no_return.map(|held| held.to_string()),
        }
    }

    /// The value in force for a Scope, and which Scope it came from.
    fn in_force(self, registry: &Registry, scope: &Scope) -> InForce {
        let settings = registry.in_force(scope);
        let from = match registry.overrides(scope) {
            Some(held) if self.read(held).is_some() => scope.clone(),
            _ => Scope::Global,
        };
        InForce {
            key: self,
            value: self.of(&settings),
            from,
            settings,
        }
    }

    /// Where the value in force for a Scope came from, for the surfaces that
    /// show provenance beside it.
    pub fn source(self, registry: &Registry, scope: &Scope) -> Scope {
        self.in_force(registry, scope).from
    }

    /// Sets a Scope's Override.
    pub fn write(self, overrides: &mut Overrides, value: &str) -> Result<()> {
        match self {
            Setting::Strategy => overrides.strategy = Some(strategy(value)?),
            Setting::WatcherMayAct => {
                overrides.watcher_may_act = Some(yes_or_no(self.as_str(), value)?)
            }
            Setting::WatcherThresholdPercent => {
                overrides.watcher_threshold_percent = Some(percentage(self.as_str(), value)?)
            }
            Setting::WatcherCooldownMinutes => {
                overrides.watcher_cooldown_minutes = Some(minutes(self.as_str(), value)?)
            }
            Setting::WatcherMarginPercent => {
                overrides.watcher_margin_percent = Some(percentage(self.as_str(), value)?)
            }
            Setting::WatcherNoReturn => {
                overrides.watcher_no_return = Some(yes_or_no(self.as_str(), value)?)
            }
        }
        Ok(())
    }

    /// Sets Global's value, which is never absent.
    fn write_global(self, settings: &mut Settings, value: &str) -> Result<()> {
        match self {
            Setting::Strategy => settings.strategy = strategy(value)?,
            Setting::WatcherMayAct => settings.watcher_may_act = yes_or_no(self.as_str(), value)?,
            Setting::WatcherThresholdPercent => {
                settings.watcher_threshold_percent = percentage(self.as_str(), value)?
            }
            Setting::WatcherCooldownMinutes => {
                settings.watcher_cooldown_minutes = minutes(self.as_str(), value)?
            }
            Setting::WatcherMarginPercent => {
                settings.watcher_margin_percent = percentage(self.as_str(), value)?
            }
            Setting::WatcherNoReturn => {
                settings.watcher_no_return = yes_or_no(self.as_str(), value)?
            }
        }
        Ok(())
    }

    /// Clears a Scope's Override, so it Inherits Global from then on.
    pub fn clear(self, overrides: &mut Overrides) {
        match self {
            Setting::Strategy => overrides.strategy = None,
            Setting::WatcherMayAct => overrides.watcher_may_act = None,
            Setting::WatcherThresholdPercent => overrides.watcher_threshold_percent = None,
            Setting::WatcherCooldownMinutes => overrides.watcher_cooldown_minutes = None,
            Setting::WatcherMarginPercent => overrides.watcher_margin_percent = None,
            Setting::WatcherNoReturn => overrides.watcher_no_return = None,
        }
    }

    /// What the Scope now does, which is the half of the answer the value
    /// itself does not give.
    fn what_that_means(self, settings: &Settings, scope: &Scope) -> String {
        let within = within(scope);
        match self {
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
                "`perch watch` may Switch {within} on your behalf when \
                 the Account you are on reaches its threshold.{} {ONLY_WHILE_IT_RUNS}",
                gated(scope),
            ),
            Setting::WatcherMayAct => format!(
                "`perch watch` will not act {within}: started on an Account \
                 there, it says so and exits rather than watching. Nothing \
                 only ever changes underneath you because you said it could."
            ),
            Setting::WatcherThresholdPercent => format!(
                "`perch watch` Switches {within} once that much of the \
                 fullest Quota Window of the Account you are on has been used. \
                 {ONLY_WHILE_IT_RUNS}"
            ),
            Setting::WatcherCooldownMinutes if settings.watcher_cooldown_minutes == 0 => format!(
                "`perch watch` will Switch {within} as often as the \
                 figures say to, with no wait between one Switch and the next — \
                 and `watcher-no-return` goes with it, because a no-return of no \
                 minutes bars nothing. The margin is then all that stands \
                 between two Accounts either side of the threshold and a \
                 ping-pong. {ONLY_WHILE_IT_RUNS}"
            ),
            Setting::WatcherCooldownMinutes => format!(
                "`perch watch` leaves at least that long between two Switches \
                 {within}, however the figures move in between. \
                 {}{ONLY_WHILE_IT_RUNS}",
                match settings.watcher_no_return {
                    true =>
                        "It is also how long the Account it just left stays \
                             barred from being Switched back to. ",
                    false => "",
                },
            ),
            Setting::WatcherMarginPercent => format!(
                "`perch watch` Switches {within} only to an Account with \
                 no more than {}% of its fullest Quota Window used — that much \
                 clear of the {}% it moves you at. A candidate barely emptier \
                 than the Account you are on is what a ping-pong is made of. \
                 {ONLY_WHILE_IT_RUNS}",
                crate::watch::Policy::of(settings).ceiling(),
                settings.watcher_threshold_percent,
            ),
            // A no-return is measured in cooldowns, so a Scope with no cooldown
            // has no no-return either however this reads. Said rather than left
            // for somebody to find out, because "will not Switch back until the
            // cooldown of 0 minutes has passed" is a sentence that means the
            // opposite of what it appears to promise.
            Setting::WatcherNoReturn
                if settings.watcher_no_return && settings.watcher_cooldown_minutes == 0 =>
            {
                format!(
                    "`perch watch` bars nothing {within}: no-return lasts \
                     one cooldown, and `watcher-cooldown-minutes` is 0. Setting a \
                     cooldown is what gives this something to measure. \
                     {ONLY_WHILE_IT_RUNS}"
                )
            }
            Setting::WatcherNoReturn if settings.watcher_no_return => format!(
                "`perch watch` will not Switch back to the Account it just left \
                 {within} until the cooldown of {} minutes has passed, \
                 whatever the figures say in between. {ONLY_WHILE_IT_RUNS}",
                settings.watcher_cooldown_minutes,
            ),
            Setting::WatcherNoReturn => format!(
                "`perch watch` may Switch straight back to the Account it just \
                 left {within}. Only the cooldown and the margin then \
                 stand between two Accounts either side of the threshold and a \
                 ping-pong. {ONLY_WHILE_IT_RUNS}"
            ),
        }
    }
}

/// The Scope as the middle of a sentence about where a Cycle happens.
fn within(scope: &Scope) -> String {
    match scope {
        Scope::Global => "in any Scope that Inherits this".to_string(),
        Scope::Ungrouped => "among the Accounts in no Group".to_string(),
        Scope::Group(name) => format!("within Group `{name}`"),
    }
}

/// The one place the layering is deliberately not uniform, said wherever
/// permission for the watcher to act is (ADR 0017, amended).
///
/// A Global "yes" is about work Groups, and Inheriting it straight through
/// would authorise moving somebody off a work Account onto their personal
/// subscription — precisely the failure Groups exist to prevent, arriving by a
/// route nobody typed. So it is two independent yeses, and the second one is
/// named here rather than left to be discovered by the watcher declining.
fn gated(scope: &Scope) -> &'static str {
    match scope {
        Scope::Ungrouped | Scope::Global => {
            " Among the Accounts in no Group it also takes `cycle-ungrouped`, \
             which is a separate declaration that those Accounts are \
             interchangeable at all (ADR 0017) — the watcher acts there only \
             where both are on."
        }
        Scope::Group(_) => "",
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

/// A key as the two-word form addresses it: any Setting, and the one thing that
/// is Global's alone (ADR 0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Setting(Setting),
    CycleUngrouped,
}

const GLOBAL_KEYS: [Key; 7] = [
    Key::CycleUngrouped,
    Key::Setting(Setting::Strategy),
    Key::Setting(Setting::WatcherMayAct),
    Key::Setting(Setting::WatcherThresholdPercent),
    Key::Setting(Setting::WatcherCooldownMinutes),
    Key::Setting(Setting::WatcherMarginPercent),
    Key::Setting(Setting::WatcherNoReturn),
];

impl Key {
    fn as_str(self) -> &'static str {
        match self {
            Key::Setting(setting) => setting.as_str(),
            Key::CycleUngrouped => "cycle-ungrouped",
        }
    }

    fn vocabulary() -> String {
        listed(GLOBAL_KEYS.map(Key::as_str).as_slice())
    }

    fn parse(name: &str) -> Result<Self> {
        Self::parse_quietly(name).map_err(|_| {
            PerchError::Invalid(format!(
                "`{name}` is not a Setting Perch holds. The ones it holds are {}.",
                Self::vocabulary(),
            ))
        })
    }

    /// The same lookup where failing is an answer rather than a refusal — for
    /// the forms that have a second thing to try.
    fn parse_quietly(name: &str) -> std::result::Result<Self, ()> {
        GLOBAL_KEYS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
            .ok_or(())
    }

    fn read(self, settings: &Settings, global: &registry::GlobalConfig) -> String {
        match self {
            Key::Setting(setting) => setting.of(settings),
            Key::CycleUngrouped => global.cycle_ungrouped.to_string(),
        }
    }

    fn write(self, global: &mut registry::GlobalConfig, value: &str) -> Result<()> {
        match self {
            Key::Setting(setting) => setting.write_global(&mut global.settings, value),
            Key::CycleUngrouped => {
                global.cycle_ungrouped = yes_or_no(self.as_str(), value)?;
                Ok(())
            }
        }
    }

    fn what_that_means(self, settings: &Settings, global: &registry::GlobalConfig) -> String {
        match self {
            Key::Setting(setting) => setting.what_that_means(settings, &Scope::Global),
            Key::CycleUngrouped if global.cycle_ungrouped => {
                "A bare `perch switch` from an Account in no Group now Cycles \
                 among the other ungrouped Accounts. That declares every \
                 ungrouped Account interchangeable at once, present and future \
                 — including the next one `perch add` creates (ADR 0017)."
                    .to_string()
            }
            Key::CycleUngrouped => "A bare `perch switch` from an Account in no Group switches \
                 nowhere and says why. Being ungrouped is the absence of a \
                 declaration that Accounts are interchangeable, not a weaker \
                 form of one (ADR 0017). It is also what gates the watcher \
                 there, so nothing acts on those Accounts unasked while it is \
                 off."
                .to_string(),
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
    fn three_words_address_a_scope_and_two_address_global() {
        let mut registry = holding_a_group();

        set(
            &mut registry,
            &words(&["work", "strategy", "soonest-reset"]),
        )
        .unwrap();
        set(&mut registry, &words(&["cycle-ungrouped", "true"])).unwrap();

        assert_eq!(
            registry.in_force(&work()).strategy,
            Strategy::SoonestReset,
            "the Group's own Override"
        );
        assert_eq!(
            registry.global.settings.strategy,
            Strategy::MostHeadroom,
            "and Global is untouched by it"
        );
        assert!(registry.global.cycle_ungrouped);
    }

    /// The whole point of the layer: one edit reaches every Scope that has not
    /// said otherwise, and goes on reaching it.
    #[test]
    fn a_scope_that_overrides_nothing_follows_global_as_global_changes() {
        let mut registry = holding_a_group();
        registry.declare_group("personal").unwrap();
        set(
            &mut registry,
            &words(&["personal", "watcher-threshold-percent", "50"]),
        )
        .unwrap();

        set(&mut registry, &words(&["watcher-threshold-percent", "90"])).unwrap();

        assert_eq!(registry.in_force(&work()).watcher_threshold_percent, 90);
        assert_eq!(
            registry
                .in_force(&Scope::Group("personal".to_string()))
                .watcher_threshold_percent,
            50,
            "the one that said otherwise keeps saying it"
        );
        assert_eq!(
            registry
                .in_force(&Scope::Ungrouped)
                .watcher_threshold_percent,
            90
        );
    }

    /// Inherit is a state and not an absence: clearing an Override makes the
    /// Scope track Global rather than copy it once.
    #[test]
    fn clearing_an_override_makes_the_scope_track_global_from_then_on() {
        let mut registry = holding_a_group();
        set(
            &mut registry,
            &words(&["work", "watcher-threshold-percent", "50"]),
        )
        .unwrap();

        unset(
            &mut registry,
            &words(&["work", "watcher-threshold-percent"]),
        )
        .unwrap();
        set(&mut registry, &words(&["watcher-threshold-percent", "90"])).unwrap();

        assert_eq!(registry.in_force(&work()).watcher_threshold_percent, 90);
    }

    /// Global's values are always set, so there is no clearing there to do.
    #[test]
    fn unset_at_global_is_refused_rather_than_silently_accepted() {
        let mut registry = holding_a_group();

        let refused = unset(&mut registry, &words(&["watcher-threshold-percent"]))
            .expect_err("Global has nothing above it to Inherit from");

        let said = refused.to_string();
        assert!(said.contains("cannot be unset at Global"), "{said}");
        assert!(said.contains("perch config set"), "{said}");
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

        let settings = registry.in_force(&work());
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
        set(&mut registry, &words(&["watcher-threshold-percent", "90"])).unwrap();
        set(
            &mut registry,
            &words(&["ungrouped", "watcher-cooldown-minutes", "45"]),
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
        assert_eq!(restored.global, registry.global);
    }

    /// The layer a value came from is the number of words that would set it
    /// again — which is the whole of how provenance is said.
    #[test]
    fn a_line_says_which_scope_the_value_came_from_by_how_long_it_is() {
        let mut registry = holding_a_group();
        set(
            &mut registry,
            &words(&["work", "strategy", "soonest-reset"]),
        )
        .unwrap();

        assert_eq!(
            get(&registry, &words(&["work", "strategy"])).unwrap(),
            vec!["work strategy soonest-reset".to_string()],
            "an Override names the Scope, because that is what would set it again"
        );
        assert_eq!(
            get(&registry, &words(&["work", "watcher-no-return"])).unwrap(),
            vec!["watcher-no-return true".to_string()],
            "and an Inheritance does not, because Global is what would"
        );
    }

    /// Every message about a watcher setting that describes the watcher acting
    /// says what it is not — a Scope that may be acted on is not a service that
    /// has been switched on (ADR 0013). Asserted over every key in every shape
    /// its message branches on, because the branch that forgets is always the
    /// one somebody added last.
    #[test]
    fn every_message_about_the_watcher_acting_says_it_only_acts_while_the_loop_runs() {
        let shapes = [
            Settings::default(),
            Settings {
                watcher_may_act: true,
                ..Settings::default()
            },
            // The two settings that read differently at zero, and the one that
            // reads differently when no-return is off.
            Settings {
                watcher_cooldown_minutes: 0,
                ..Settings::default()
            },
            Settings {
                watcher_margin_percent: 0,
                ..Settings::default()
            },
            Settings {
                watcher_no_return: false,
                ..Settings::default()
            },
        ];

        for settings in shapes {
            for scope in [Scope::Global, Scope::Ungrouped, work()] {
                for key in SETTINGS {
                    if key == Setting::Strategy {
                        continue;
                    }
                    let said = key.what_that_means(&settings, &scope);
                    assert!(
                        said.contains(ONLY_WHILE_IT_RUNS) || said.contains("will not act"),
                        "`{}` at {settings:?} in {scope:?} says nothing about being a \
                         loop rather than a service: {said}",
                        key.as_str(),
                    );
                }
            }
        }
    }

    /// ADR 0017, amended: `watcher-may-act` does not reach the Ungrouped Scope
    /// by Inheritance, and the message that grants it says so.
    #[test]
    fn granting_the_watcher_names_the_second_yes_the_ungrouped_accounts_need() {
        let granted = Settings {
            watcher_may_act: true,
            ..Settings::default()
        };

        for scope in [Scope::Global, Scope::Ungrouped] {
            let said = Setting::WatcherMayAct.what_that_means(&granted, &scope);
            assert!(said.contains("cycle-ungrouped"), "{scope:?}: {said}");
        }
        assert!(
            !Setting::WatcherMayAct
                .what_that_means(&granted, &work())
                .contains("cycle-ungrouped"),
            "a named Group needs no second yes",
        );
    }

    /// A no-return is measured in cooldowns, so a Scope with no cooldown has no
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
    fn a_group_that_shares_a_keys_name_does_not_hide_the_setting() {
        let mut registry = Registry::default();
        registry.declare_group("cycle-ungrouped").unwrap();
        set(&mut registry, &words(&["cycle-ungrouped", "true"])).unwrap();

        assert_eq!(
            get(&registry, &words(&["cycle-ungrouped"])).unwrap(),
            vec!["cycle-ungrouped true".to_string()],
            "one word is the key, which has no other way to be read"
        );
        assert_eq!(
            get(&registry, &words(&["cycle-ungrouped", "strategy"])).unwrap(),
            vec!["strategy most-headroom".to_string()],
            "and the Group is still reachable a key at a time"
        );
    }

    /// The refusal for a Scope named with a key and nothing to set it to is
    /// still reachable — for every Scope not named after a key, which is all of
    /// them anybody declares.
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
