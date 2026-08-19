//! What a Setting *is*: what it is called, which Scope carries it, what values
//! it takes, and what having set it means.
//!
//! A Setting is one named rule governing how Perch chooses between Accounts,
//! said about the Scope it governs and nowhere else (ADR 0051). [`Registry`]
//! holds what a Scope has been set to and refuses a value it cannot mean;
//! everything about how that value is *named*, *typed* and *explained* is here.
//!
//! What `perch config` *does* — which words go where on a command line, which
//! form somebody seems to have meant, how a line is printed so it reads back as
//! the `set` that would restore it — is [`crate::commands::config`]'s.
//!
//! Here rather than there because a key's name is printed by surfaces that are
//! not `perch config`. `perch group list` names `interchangeable` in the row
//! explaining why the watcher will not act, and the clause `perch list` and
//! `perch group list` share names it again. Both used to spell it as a literal,
//! so the key could be renamed — as it was once already, from `cycle_ungrouped`
//! — and leave two surfaces confidently printing a word `perch config set`
//! would refuse.

use crate::error::{PerchError, Result};
use crate::registry::{self, Registry, Scope, Strategy, UNGROUPED};

/// One Setting, as Perch names it.
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
    pub fn parse(name: &str, scope: &Scope) -> Result<Self> {
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

    /// Sets this Setting on one Scope.
    ///
    /// Applied to a copy of what the Scope holds and checked over the whole of
    /// it before anything lands, so configuration that would not mean anything
    /// never reaches the Scope it was meant for: a refused `set` leaves every
    /// Setting exactly as it found it. Asked again over the whole Scope after
    /// the value parses, because the registry is the boundary every Config
    /// crosses and this one is only a command line — and both refusals name the
    /// same ranges, from the same constants.
    pub fn write(self, registry: &mut Registry, scope: &Scope, value: &str) -> Result<()> {
        // The same question `parse` asks, asked again here — and for the reason
        // this function already gives about `settings_mut` a few lines down: a
        // `pub fn` on a `pub` type returning a `Result` should refuse rather
        // than do something nobody asked for.
        //
        // What it did without this was write `interchangeable` onto the
        // Ungrouped Scope when it was handed a *Group*: the Group's `Settings`
        // were left alone, and the line at the end of this function assigned
        // `registry.ungrouped.interchangeable` unconditionally. A Setting
        // applied to a Scope nobody named. Unreachable through
        // `commands::config`, which calls `parse` first, and unreachable is
        // where a second caller comes from.
        if !self.carried_by(scope) {
            return Err(PerchError::Invalid(format!(
                "`{}` is the declaration that the Accounts in no Group are \
                 interchangeable at all, and only they carry it — a Group is \
                 that declaration rather than something that holds one (ADR \
                 0002). `perch config set {UNGROUPED} {} <value>` says it.",
                Setting::Interchangeable.as_str(),
                Setting::Interchangeable.as_str(),
            )));
        }

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
        // A refusal rather than an abort. `commands::config` resolves the Scope
        // through `declared_group` before it gets here, so today's one caller
        // cannot reach this — but this is a `pub fn` on a `pub` type returning a
        // `Result`, and a signature that says it refuses should not be the thing
        // that panics on the second caller.
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
    /// itself does not give.
    pub fn what_that_means(self, registry: &Registry, scope: &Scope) -> String {
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
                 Anything that changes underneath you only ever does so \
                 because you said it could."
            ),
            Setting::WatcherThresholdPercent => format!(
                "`perch watcher run` Switches {within} once that much of the \
                 fullest Quota Window of the Account you are on has been used. \
                 {ONLY_WHILE_IT_RUNS}"
            ),
        }
    }
}

/// What a Scope that has just grown still needs said about it before anything
/// Cycles within it unasked, or `None` where it needs nothing.
///
/// A second Account in one Scope is the moment two deliberate defaults start to
/// matter: `watcher-may-act` is false on every Scope, and the Accounts in no
/// Group need `interchangeable` as well, because being ungrouped is the absence
/// of a declaration that they are interchangeable rather than a weaker form of
/// one (ADR 0017). Both are correct, and neither used to be said anywhere near
/// the command that makes them relevant — the Scope simply held two
/// interchangeable-looking Accounts and quietly did nothing with them.
///
/// **Said and never asked.** `watcher-may-act` is a consent gate, and a yes
/// collected in the middle of adding an Account is not the yes Perch promises
/// when it says nothing changes underneath you until you say it may. So this
/// returns a statement of what is now true, which is [`crate::commands::add`]'s
/// to print beside the rest of what it did (ADR 0061).
///
/// Asked of a Scope rather than of a Group with the Ungrouped case bolted on:
/// they are the same question, and the only difference is how many Settings
/// come back.
///
/// Silent below two Accounts, because a rule for choosing has nothing to say to
/// a set of one — which is the same reason no Setting is carried by an Account.
///
/// **It does not repeat [`ONLY_WHILE_IT_RUNS`], and that is deliberate.** Every
/// sentence saying what `watcher-may-act` *does* carries that caveat, because a
/// Scope that may be acted on is not a service that has been switched on (ADR
/// 0013). This sentence says the opposite — that the Setting is off and nothing
/// is happening — and the sentence about what turning it on means is
/// [`Setting::what_that_means`], which is printed by the very `perch config
/// set` named here. So the caveat arrives at the moment it becomes true rather
/// than three clauses early, in a command whose report ADR 0061 keeps to what
/// it did.
///
/// **And it counts what the Scope holds rather than what a Cycle could choose.**
/// [`Scope::accounts`] is the one idea of that set, and the narrower count would
/// be a second: a Disabled or Quarantined Account beside the new one is still a
/// pair a Cycle can move *between*, since either can be the one being left, and
/// both states are reversible by a command that says nothing about Settings.
pub fn what_the_scope_still_needs(registry: &Registry, scope: &Scope) -> Option<String> {
    let held = scope.accounts(registry).len();
    if held < 2 {
        return None;
    }

    // The declaration before the grant, which is the order `perch watcher run`
    // refuses in and for the same reason: somebody who has said neither is told
    // about the declaration first, because that is the one that has to come
    // first. `may_cycle_within` rather than a second reading of
    // `interchangeable` — it is the one place that answers what a Group is
    // exempt from, and a copy of it here would be a second idea of that.
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
    // top of this module: `interchangeable` has been renamed once already, and
    // a surface printing the old word is a surface telling somebody to type
    // something `perch config set` refuses.
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

    /// A Setting the Scope cannot carry is refused rather than written
    /// somewhere else.
    ///
    /// `interchangeable` is the Ungrouped Scope's alone, and `write` never
    /// asked. Handed a Group it left the Group's Settings untouched and then
    /// assigned `registry.ungrouped.interchangeable` on the way out — a Setting
    /// applied to a Scope nobody named. `commands::config` calls `parse` first
    /// and so cannot reach it, which is the argument for the check rather than
    /// against it: this function already makes it, one branch further down,
    /// about `settings_mut`.
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

    /// ADR 0017: the Accounts in no Group need two independent yeses, and the
    /// message that grants the watcher names the other one. A Group needs only
    /// the grant, because being a Group is the declaration.
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

    /// Writing a Setting onto a Group nothing declared is a refusal, which is
    /// what the signature says. It used to be an abort.
    ///
    /// `commands::config` resolves the Scope through `declared_group` before it
    /// gets here, so the CLI cannot reach this — but `Setting::write` is a
    /// `pub fn` on a `pub` type returning a `Result`, and a signature that says
    /// it refuses should not be the thing that panics on the second caller.
    #[test]
    fn setting_a_scope_nothing_declared_is_refused_rather_than_panicked_on() {
        let mut registry = Registry::default();

        let error = Setting::Strategy
            .write(&mut registry, &work(), "most-headroom")
            .expect_err("there is no such Group");

        assert!(error.to_string().contains("work"), "{error}");
    }

    /// A Scope that has grown to two Accounts is told what it still cannot do,
    /// and what it is told is the Settings it is actually missing — one for a
    /// Group, two for the Accounts in no Group, and the declaration named
    /// before the grant (ADR 0017).
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
            "a Group *is* that declaration (ADR 0002): {said}"
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

    /// The line says what is missing, so a Scope missing nothing says nothing —
    /// and neither does a set of one, which is not a set a rule for choosing has
    /// anything to say to.
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

    /// An Account held by nothing but its email address and the Group it is in,
    /// which is all the question above asks about.
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
    /// left printing a word `perch config set` would refuse. It has been
    /// renamed once already — `cycle_ungrouped` became `interchangeable` — and
    /// the two surfaces that spell it outside this module are the ones that
    /// would have been missed.
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
