//! What a Setting *is*: what it is called, which Scope carries it, what values
//! it takes, and what having set it means (ADR a-setting-names-its-scope).
//!
//! [`Registry`] holds what a Scope has been set to and refuses a value it
//! cannot mean. What `perch config` *does* with the words on a command line is
//! [`crate::commands::config`]'s.
//!
//! Here rather than there because surfaces that are not `perch config` name a
//! key too, and a key spelled as a literal at one of those sites is a surface
//! printing a word `perch config set` would refuse.

use crate::error::{PerchError, Result};
use crate::registry::{self, Registry, Scope, Strategy, UNGROUPED};

/// One Setting, as Perch names it.
///
/// One vocabulary, because every Setting is said about a Scope. One entry of it
/// is carried by one Scope alone (see [`Setting::carried_by`]).
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

    /// `interchangeable` is the Accounts in no Group's alone, because a Group
    /// **is** that declaration (ADR a-group-is-a-declaration). Printed against
    /// a Group and refused, it would break the invariant the command rests on:
    /// every line `get` prints is the tail of the `set` that restores it.
    pub fn carried_by(self, scope: &Scope) -> bool {
        self != Setting::Interchangeable || *scope == Scope::Ungrouped
    }

    /// What [`carried_by`](Self::carried_by) answering false comes to.
    ///
    /// Once, because it is refused twice: `parse` turns the word down and
    /// `write` turns it down again at the registry boundary, and one sentence
    /// spelled at two sites is how the two come to name different remedies.
    fn only_the_ungrouped_scope_carries_it() -> PerchError {
        PerchError::Invalid(format!(
            "`{}` is the declaration that the Accounts in no Group are \
             interchangeable at all, and only they carry it — a Group is \
             that declaration rather than something that holds \
             one. `perch config set {UNGROUPED} {} <value>` says it.",
            Setting::Interchangeable.as_str(),
            Setting::Interchangeable.as_str(),
        ))
    }

    /// The Setting a word names, asked of the Scope it was named about.
    ///
    /// Refused two ways, because they are two different mistakes: a word that
    /// is no key at all, and `interchangeable` asked of a Group, which is a
    /// real key said about the one Scope that cannot carry it.
    pub fn parse(name: &str, scope: &Scope) -> Result<Self> {
        match Self::parse_quietly(name) {
            Some(key) if key.carried_by(scope) => Ok(key),
            Some(_) => Err(Self::only_the_ungrouped_scope_carries_it()),
            None => Err(PerchError::Invalid(format!(
                "`{name}` is not a Setting {} carries. The ones it carries are {}.",
                scope.mentioned(),
                listed(&vocabulary(scope)),
            ))),
        }
    }

    /// The same lookup where failing is an answer rather than a refusal — for
    /// the forms that have a second thing to try — and without a Scope, because
    /// they are asking what a word *is* rather than what it may be said about.
    pub fn parse_quietly(name: &str) -> Option<Self> {
        SETTINGS
            .into_iter()
            .find(|key| name.eq_ignore_ascii_case(key.as_str()))
    }

    /// The value this Scope holds, as `get` prints it and `set` would take it
    /// back.
    pub fn of(self, registry: &Registry, scope: &Scope) -> String {
        let settings = registry.settings(scope);
        match self {
            Setting::Interchangeable => registry.ungrouped.interchangeable.to_string(),
            Setting::Strategy => settings.strategy.as_str().to_string(),
            Setting::WatcherMayAct => settings.watcher_may_act.to_string(),
            Setting::WatcherThresholdPercent => settings.watcher_threshold_percent.to_string(),
        }
    }

    /// Applied to a copy and checked over the whole Scope before anything
    /// lands, so a refused `set` leaves every Setting as it found it. Checked
    /// there rather than only here, because the registry is the boundary every
    /// Config crosses and this one is only a command line.
    pub fn write(self, registry: &mut Registry, scope: &Scope, value: &str) -> Result<()> {
        // Asked again here, for the reason `settings_mut` is asked about below:
        // a `pub fn` on a `pub` type returning a `Result` refuses rather than
        // writing a Setting onto a Scope nobody named.
        if !self.carried_by(scope) {
            return Err(Self::only_the_ungrouped_scope_carries_it());
        }

        let mut settings = registry.settings(scope);
        // Beside the Settings rather than in them, because a Group has no such
        // line.
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
        // A refusal rather than an abort: a signature that says it refuses is
        // not the thing that panics on the second caller.
        let Some(held) = registry.settings_mut(scope) else {
            let Scope::Group(name) = scope else {
                unreachable!("the Ungrouped Scope is always there to write to")
            };
            return Err(PerchError::NotFound(format!(
                "no Group is called `{name}`, so there is nothing to set on it."
            )));
        };
        *held = settings;
        registry.ungrouped.interchangeable = interchangeable;
        Ok(())
    }

    /// What the Scope now does, which is the half of the answer the value
    /// itself does not give — and never why Perch decided it should, which is a
    /// design defended to somebody who just typed the command that accepts it
    /// (ADR perch-says-what-it-did).
    pub fn what_that_means(self, registry: &Registry, scope: &Scope) -> String {
        let settings = registry.settings(scope);
        let within = scope.within();
        match self {
            Setting::Interchangeable if registry.ungrouped.interchangeable => {
                "A bare `perch switch` from an Account in no Group now Cycles \
                 among the other ungrouped Accounts. That declares every \
                 ungrouped Account interchangeable at once, present and future \
                 — including the next one `perch add` creates."
                    .to_string()
            }
            Setting::Interchangeable => {
                "A bare `perch switch` from an Account in no Group switches \
                 nowhere and says why. It is also what gates the watcher there, \
                 so nothing acts on those Accounts unasked while it is off."
                    .to_string()
            }
            Setting::Strategy => match settings.strategy {
                Strategy::MostHeadroom => format!(
                    "A Cycle {within} prefers the Account with the most \
                     room left, measured by its worst Quota Window."
                ),
                Strategy::SoonestReset => format!(
                    "A Cycle {within} prefers the Account whose fullest \
                     Quota Window resets soonest, so perishable quota is spent \
                     rather than wasted. Headroom is still measured by the worst \
                     window, so an exhausted Account is still never chosen \
                     however soon it comes back."
                ),
            },
            Setting::WatcherMayAct if settings.watcher_may_act => format!(
                "`perch watcher run` may Switch {within} on your behalf when \
                 the Account you are on reaches its threshold.{} {ONLY_WHILE_IT_RUNS}",
                gated(registry, scope),
            ),
            Setting::WatcherMayAct => format!(
                "`perch watcher run` will not act {within}: started on an \
                 Account there, it says so and exits rather than watching."
            ),
            Setting::WatcherThresholdPercent => format!(
                "`perch watcher run` Switches {within} once that much of the \
                 fullest Quota Window of the Account you are on has been used. \
                 {ONLY_WHILE_IT_RUNS}"
            ),
        }
    }
}

/// What a Scope that has just grown still needs said about it, as a statement
/// of what is now true rather than a question: consent is said and never asked.
/// `None` below two Accounts, because a rule for choosing has nothing to say to
/// a set of one — and no [`ONLY_WHILE_IT_RUNS`] caveat, because this is the
/// sentence saying the Setting is off.
pub fn what_the_scope_still_needs(registry: &Registry, scope: &Scope) -> Option<String> {
    let held = scope.accounts(registry).len();
    if held < 2 {
        return None;
    }

    // The declaration before the grant, which is the order it has to be said
    // in. `may_cycle_within` rather than a second reading of `interchangeable`:
    // it is the one place answering what a Group is exempt from.
    let mut needed = Vec::new();
    if !crate::cycle::may_cycle_within(registry, scope) {
        needed.push(Setting::Interchangeable);
    }
    if !registry.settings(scope).watcher_may_act {
        needed.push(Setting::WatcherMayAct);
    }
    if needed.is_empty() {
        return None;
    }

    // Named from the vocabulary rather than spelled here, for the reason at the
    // top of this module.
    let says: Vec<String> = needed
        .iter()
        .map(|key| format!("`perch config set {} {} true`", scope.word(), key.as_str()))
        .collect();
    Some(format!(
        "{} now holds {}, and nothing Cycles between them unasked: {} {} it may.",
        scope.described(),
        crate::commands::accounts(held),
        says.join(" and "),
        if says.len() == 1 { "says" } else { "say" },
    ))
}

/// The keys one Scope carries, in the order they are offered.
pub fn vocabulary(scope: &Scope) -> Vec<&'static str> {
    SETTINGS
        .into_iter()
        .filter(|key| key.carried_by(scope))
        .map(Setting::as_str)
        .collect()
}

/// The second yes the Accounts in no Group need, said wherever permission for
/// the watcher to act is.
///
/// A Group needs only the grant, because being a Group is the declaration.
/// Named here rather than left to be discovered by the watcher declining.
fn gated(registry: &Registry, scope: &Scope) -> String {
    match scope {
        Scope::Ungrouped if !registry.ungrouped.interchangeable => format!(
            " It does not act there yet: `{}` is false, and that is a separate \
             declaration that those Accounts are interchangeable at all — \
             `perch config set {UNGROUPED} {} true` makes it.",
            Setting::Interchangeable.as_str(),
            Setting::Interchangeable.as_str(),
        ),
        Scope::Ungrouped => format!(
            " Those Accounts have also been declared interchangeable, which is \
             the other half of it: the watcher acts here only where `{}` is on \
             too.",
            Setting::Interchangeable.as_str(),
        ),
        Scope::Group(_) => String::new(),
    }
}

/// Said of both of the watcher's fields in one place, because two sentences
/// about it would sooner or later say two different things: nothing here is a
/// service that has been switched on (ADR a-watcher-knob-is-arithmetic). All
/// three ways of running one are named, or this would be a Setting somebody
/// with a Service had no reason to read (ADR the-machine-runs-the-watcher).
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
/// The range is the registry's to state (`a_percentage`), so a number too large
/// for the field and one the field holds but the policy will not are refused in
/// one sentence: to the script that mistyped, `300` and `101` are one mistake.
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
pub fn listed(names: &[&str]) -> String {
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

    fn work() -> Scope {
        Scope::Group("work".to_string())
    }

    /// The bound is stated in three places — `Settings::validate`, the parser
    /// here, and the sentence both of them quote — and a value one takes and
    /// another refuses is a Setting somebody can write and not keep.
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

    #[test]
    fn a_setting_a_group_cannot_carry_is_refused_rather_than_written_elsewhere() {
        let mut registry = holding_a_group();
        registry.ungrouped.interchangeable = false;

        let refused = Setting::Interchangeable
            .write(&mut registry, &work(), "true")
            .expect_err("a Group is that declaration rather than one that holds it");

        assert!(
            refused.to_string().contains("interchangeable"),
            "it names the key: {refused}"
        );
        assert!(
            !registry.ungrouped.interchangeable,
            "and the Ungrouped Scope, which nobody named, is as it was"
        );
    }

    /// A value that would not mean anything never reaches the Scope it was meant
    /// for, and takes nothing already there with it.
    #[test]
    fn a_value_that_means_nothing_leaves_the_scope_as_it_was() {
        let mut registry = holding_a_group();
        Setting::WatcherMayAct
            .write(&mut registry, &work(), "true")
            .unwrap();

        // Refused by the range the registry enforces, after the value parsed.
        Setting::WatcherThresholdPercent
            .write(&mut registry, &work(), "101")
            .expect_err("a Utilization threshold is a percentage");

        let settings = registry.settings(&work());
        assert_eq!(settings.watcher_threshold_percent, 80);
        assert!(
            settings.watcher_may_act,
            "the failed write is the only thing rolled back"
        );
    }

    /// Asserted over every key in every shape its message branches on, because
    /// the branch that forgets the caveat is the one somebody added last.
    #[test]
    fn every_message_about_the_watcher_acting_says_it_only_acts_while_the_loop_runs() {
        for granted in [false, true] {
            let mut registry = holding_a_group();
            for scope in [Scope::Ungrouped, work()] {
                Setting::WatcherMayAct
                    .write(&mut registry, &scope, &granted.to_string())
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

    #[test]
    fn granting_the_watcher_names_the_second_yes_the_ungrouped_accounts_need() {
        let mut registry = holding_a_group();
        for scope in [Scope::Ungrouped, work()] {
            Setting::WatcherMayAct
                .write(&mut registry, &scope, "true")
                .unwrap();
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

    /// `commands::config` resolves the Scope before this is reached, so the CLI
    /// cannot get here — and a `pub fn` returning a `Result` still refuses.
    #[test]
    fn setting_a_scope_nothing_declared_is_refused_rather_than_panicked_on() {
        let mut registry = Registry::default();

        let error = Setting::Strategy
            .write(&mut registry, &work(), "most-headroom")
            .expect_err("there is no such Group");

        assert!(error.to_string().contains("work"), "{error}");
    }

    /// One Setting missing for a Group, two for the Accounts in no Group, and
    /// the declaration named before the grant.
    #[test]
    fn a_scope_of_two_is_told_the_yeses_it_is_still_missing() {
        let mut registry = holding_a_group();
        registry.upsert(in_group("one@example.com", Some("work")));
        registry.upsert(in_group("two@example.com", Some("work")));
        registry.upsert(in_group("three@example.com", None));
        registry.upsert(in_group("four@example.com", None));

        let said = what_the_scope_still_needs(&registry, &work()).expect("a Group of two");
        assert!(said.contains("2 Accounts"), "{said}");
        assert!(
            said.contains(&format!(
                "`perch config set work {} true`",
                Setting::WatcherMayAct.as_str()
            )),
            "{said}"
        );
        assert!(
            !said.contains(Setting::Interchangeable.as_str()),
            "a Group *is* that declaration: {said}"
        );

        let said =
            what_the_scope_still_needs(&registry, &Scope::Ungrouped).expect("and a pair with none");
        let declaration = said
            .find(Setting::Interchangeable.as_str())
            .expect("the declaration is named");
        let grant = said
            .find(Setting::WatcherMayAct.as_str())
            .expect("and the grant beside it");
        assert!(
            declaration < grant,
            "the declaration has to come first, so it is said first: {said}"
        );
    }

    #[test]
    fn a_scope_that_is_permitted_or_holds_one_account_is_told_nothing() {
        let mut registry = holding_a_group();
        registry.upsert(in_group("one@example.com", Some("work")));
        assert_eq!(
            what_the_scope_still_needs(&registry, &work()),
            None,
            "one Account is not a set worth Cycling within"
        );

        registry.upsert(in_group("two@example.com", Some("work")));
        Setting::WatcherMayAct
            .write(&mut registry, &work(), "true")
            .unwrap();
        assert_eq!(
            what_the_scope_still_needs(&registry, &work()),
            None,
            "nothing is missing, so nothing is said"
        );
    }

    /// An Account held by nothing but its address and its Group, which is all
    /// the question above asks about.
    fn in_group(email: &str, group: Option<&str>) -> crate::registry::Account {
        crate::registry::Account {
            identity: crate::probe::Identity {
                email: email.to_string(),
                organization_name: None,
                organization_uuid: None,
                account_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: group.map(str::to_string),
            utilization: None,
        }
    }

    #[test]
    fn a_vocabulary_is_named_as_a_sentence() {
        assert_eq!(listed(&["one"]), "`one`");
        assert_eq!(listed(&["one", "two"]), "`one` and `two`");
        assert_eq!(listed(&["one", "two", "three"]), "`one`, `two` and `three`");
    }

    /// A key is named from one place, so a surface that prints it cannot be
    /// left printing a word `perch config set` would refuse.
    #[test]
    fn the_key_the_other_surfaces_name_is_the_key_this_one_takes() {
        let key = Setting::Interchangeable.as_str();
        assert_eq!(Setting::parse_quietly(key), Some(Setting::Interchangeable));
        assert!(
            crate::commands::cycling_among_ungrouped(&Registry::default()).contains(key),
            "the clause `perch list` and `perch group list` share names it",
        );
    }
}
