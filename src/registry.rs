//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned, and the version moves when the shape does
//! (ADR the-holdings-outlive-a-perch): a registry claiming more than this build
//! understands is refused rather than silently misread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::lock;
use crate::probe::{Identity, LockSpec};

/// The version this build writes.
///
/// A registry claiming a higher one is refused rather than silently misread, and
/// the guard is only worth having if this moves whenever the shape does.
pub const CURRENT_VERSION: u32 = 4;

/// One Quota Window's Utilization, as observed at a point in time
/// (ADR a-figure-carries-its-age).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowUtilization {
    /// The window this figure describes, e.g. `5-hour`, `7-day`, `7-day-opus`.
    pub window: String,
    /// How full the window is, 0-100.
    pub used_percent: f64,
    /// When the window next resets, if the observation carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}

/// Cached Utilization for one Account. What every surface renders, and what
/// only a `--refresh` ever goes and fetches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedUtilization {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<WindowUtilization>,
}

/// Why an Account's Credential can no longer be used and cannot be recovered
/// from anything Perch holds (ADR a-broken-account-is-repaired).
///
/// Recorded rather than merely counted: every one of these is terminal, and
/// which one it is implies the repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quarantine {
    /// Anthropic turned the refresh token down — retired, revoked, or belonging
    /// to a login that has been ended elsewhere.
    RenewalRejected,
    /// Anthropic Rotated the refresh token and the new one could not be stored,
    /// so the old one is retired and the new one is gone.
    RotationLost,
    /// The Credential carries no refresh token, so the access token that ran out
    /// was the last thing it could offer.
    NoRefreshToken,
    /// Neither of the Profile's Credential Stores holds anything at all.
    NoCredential,
}

impl Quarantine {
    /// What happened, as the middle of a sentence about the Account: "{named}
    /// is Quarantined: {because}."
    pub fn because(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "Anthropic would not renew its Credential",
            Quarantine::RotationLost => {
                "Anthropic Rotated its refresh token and the new one could not be stored, \
                 so the one Perch holds is retired"
            }
            Quarantine::NoRefreshToken => {
                "the Credential Perch holds carries no refresh token, so it cannot be renewed"
            }
            Quarantine::NoCredential => "Perch holds no Credential for it",
        }
    }

    /// Whether getting here cost a request to Anthropic, which is what the
    /// Watcher's Back-off paces. A property of what happened rather than of why
    /// the Renewal was wanted: both reasons reach both halves of this.
    pub fn reached_anthropic(&self) -> bool {
        match self {
            Quarantine::RenewalRejected | Quarantine::RotationLost => true,
            Quarantine::NoRefreshToken | Quarantine::NoCredential => false,
        }
    }

    /// The reason as a script reads it, which is the spelling the registry
    /// records.
    pub fn as_str(&self) -> &'static str {
        match self {
            Quarantine::RenewalRejected => "renewal-rejected",
            Quarantine::RotationLost => "rotation-lost",
            Quarantine::NoRefreshToken => "no-refresh-token",
            Quarantine::NoCredential => "no-credential",
        }
    }

    /// The whole of what is said about a Quarantined Account where nothing around
    /// it says any of it: which Account, what happened, and how to end it.
    ///
    /// `detail` is whatever the failure underneath said. The reason is what
    /// happened; the detail is how.
    pub fn said_of(&self, named: &str, target: &str, detail: Option<&str>) -> String {
        let how = match detail {
            Some(detail) => format!(" ({detail})"),
            None => String::new(),
        };
        format!(
            "{named} is Quarantined: {}{how}. {}",
            self.because(),
            how_to_repair(target)
        )
    }

    /// What is true of this Account and no other: which one it is and what
    /// happened to it, without the repair.
    ///
    /// For a surface that has already said the state and says the repair once
    /// beneath all of them (ADR perch-says-what-it-did).
    pub fn shown_of(&self, named: &str) -> String {
        format!("{named}: {}.", self.because())
    }

    /// The refusal a command raises rather than acting on a Quarantined Account,
    /// as opposed to [`said_of`](Quarantine::said_of), which is how one is
    /// *shown*.
    ///
    /// `consequence` is the caller's, and is the only part that differs.
    pub fn refusal(self, named: &str, target: &str, consequence: &str) -> PerchError {
        PerchError::Quarantined {
            why: self,
            said: format!(
                "{named} is Quarantined: {}.\n{consequence} {}",
                self.because(),
                how_to_repair(target),
            ),
        }
    }

    /// The same as a script reads it. Absent reads as false wherever a script
    /// asks whether it is set, so one already branching on the fact carries why.
    ///
    /// `said` rather than `detail`: the registry records a `Quarantine` and not
    /// the failure behind one, so there is no "how" here to carry.
    pub fn document(quarantine: Option<Quarantine>) -> serde_json::Value {
        match quarantine {
            Some(why) => {
                serde_json::json!({"reason": why.as_str(), "said": why.because()})
            }
            None => serde_json::Value::Null,
        }
    }
}

/// How a Quarantine is asked about and how it is put right, said the same way
/// wherever an Account is shown as broken.
pub fn how_to_repair(target: &str) -> String {
    format!(
        "`perch relogin {target}` logs it in again in place, keeping its Alias, \
         its Group and whether Cycling may choose it."
    )
}

/// The same repair, for however many Accounts are in that state — said once,
/// because it is the same repair.
///
/// Named where there is exactly one to name: "logs *it* in again" over a set
/// tells somebody holding three broken Accounts to repair the first.
pub fn how_to_repair_them(targets: &[impl AsRef<str>]) -> Option<String> {
    match targets {
        [] => None,
        [one] => Some(how_to_repair(one.as_ref())),
        _ => Some(
            "`perch relogin <target>` logs one in again in place, keeping its \
             Alias, its Group and whether Cycling may choose it."
                .to_string(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    /// Who this Account is. Its email address is also its identifier.
    pub identity: Identity,
    /// The subscription the Credential reports — `pro`, `max`, and so on. It
    /// comes from the Credential rather than the Identity, which is why it is
    /// not part of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Whether the Account has been taken out of Cycling.
    ///
    /// Said only when true: the positive state has no name to write down — it is
    /// the absence of this one (ADR a-command-names-its-noun).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Why this Account's Credential can no longer be used, when it cannot.
    ///
    /// Left out of the file entirely rather than written as a null: the registry
    /// is something a person may open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<CachedUtilization>,
}

/// How Cycling orders the Accounts in a Group.
///
/// Both readings measure headroom the same way — the worst Quota Window an
/// Account has (ADR headroom-is-the-worst-window) — so neither is a way round an
/// exhausted Account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    /// Prefer the Account with the most room left.
    #[default]
    MostHeadroom,
    /// Prefer the Account whose window resets soonest, so perishable quota is
    /// spent rather than wasted.
    SoonestReset,
}

impl Strategy {
    /// Every Strategy there is, so the vocabulary `perch config` accepts and
    /// names in a refusal cannot fall behind the ones Cycling implements.
    pub const ALL: [Strategy; 2] = [Strategy::MostHeadroom, Strategy::SoonestReset];

    pub fn as_str(&self) -> &'static str {
        match self {
            Strategy::MostHeadroom => "most-headroom",
            Strategy::SoonestReset => "soonest-reset",
        }
    }
}

/// The watcher's threshold when nobody has said otherwise: high enough that an
/// unattended Switch means the Account really is running out.
pub const DEFAULT_WATCHER_THRESHOLD_PERCENT: u8 = 80;

/// The most a Setting said as a share of something can be.
///
/// Not the bound on a Utilization figure, which `validate` checks separately:
/// that is what a *reading* may be and this is what a Setting may be *set to*.
/// The same number today, and two facts always.
pub const MAX_PERCENTAGE: u8 = 100;

/// What a percentage accepts, said once so a mistyped `perch config set` and a
/// hand-edited registry are refused in the same words. Built from the bound, so
/// the sentence and the number cannot disagree.
pub fn a_percentage() -> String {
    format!("a whole number between 0 and {MAX_PERCENTAGE}")
}

/// Every Setting there is, all of them set: what one Scope holds
/// (ADR a-setting-names-its-scope).
///
/// There is nothing above a Scope for a value to fall back to, so a Setting
/// nobody has said anything about is the compiled-in default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub strategy: Strategy,
    /// Whether the watcher may Switch within this Scope unattended. Off unless
    /// the user says otherwise: nothing changes underneath somebody because they
    /// did not say it could (ADR a-group-is-a-declaration). Said about the Scope
    /// it grants and nowhere else.
    pub watcher_may_act: bool,
    /// The Utilization the watcher would act at, as a percentage.
    pub watcher_threshold_percent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            strategy: Strategy::default(),
            watcher_may_act: false,
            watcher_threshold_percent: DEFAULT_WATCHER_THRESHOLD_PERCENT,
        }
    }
}

impl Settings {
    /// Refuses configuration that cannot mean what it says. Serde refuses a
    /// strategy Perch does not implement; what is left is the range.
    ///
    /// The refusal names the numbers that would have been accepted, because the
    /// script that mistyped one is the reader.
    pub fn validate(&self, scope: &Scope) -> Result<()> {
        if self.watcher_threshold_percent > MAX_PERCENTAGE {
            return Err(out_of_range(
                scope,
                "watcher-threshold-percent",
                self.watcher_threshold_percent,
                &a_percentage(),
            ));
        }
        Ok(())
    }
}

/// A number a setting cannot hold, refused with the ones it can.
fn out_of_range(
    scope: &Scope,
    key: &str,
    held: impl std::fmt::Display,
    accepted: &str,
) -> PerchError {
    PerchError::Invalid(format!(
        "{} has a `{key}` of {held}, and it takes {accepted}.",
        scope.described(),
    ))
}

/// The set of Accounts a Setting governs, and the set a Cycle may look within:
/// one Group, or the Accounts in no Group taken together.
///
/// One type for both, because they are one idea: a Cycle never leaves the Scope
/// it started in, and a Setting is said about exactly the Scope it governs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The Accounts in no Group, taken as one Scope. Not a Group and never one:
    /// a Group is a declaration somebody made, and this is the absence of one.
    Ungrouped,
    /// One Group, named as it was declared.
    Group(String),
}

impl Scope {
    /// The word that addresses this Scope on a command line, and the word it is
    /// recorded under wherever something is kept per Scope.
    ///
    /// A Group cannot be called `ungrouped` ([`validate_name`]), so the two can
    /// never collide.
    pub fn word(&self) -> &str {
        match self {
            Scope::Ungrouped => UNGROUPED,
            Scope::Group(name) => name,
        }
    }

    /// The Accounts this Scope holds.
    ///
    /// The same set whoever is asking. A second idea of which Accounts those are
    /// is how the figure on screen comes to describe a different set from the
    /// one that gets chosen.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Ungrouped => registry.ungrouped_accounts(),
            Scope::Group(name) => registry.accounts_in(name),
        }
    }

    /// The Scope as an adverbial phrase: "a Cycle {} prefers…". Said once here,
    /// because three spellings of "among the Accounts in no Group" is how two of
    /// them come to name the same set differently.
    pub fn within(&self) -> String {
        match self {
            Scope::Ungrouped => "among the Accounts in no Group".to_string(),
            Scope::Group(name) => format!("within Group `{name}`"),
        }
    }

    /// The Scope as the subject of a sentence about what it holds.
    pub fn described(&self) -> String {
        match self {
            Scope::Ungrouped => "The Ungrouped Scope".to_string(),
            Scope::Group(name) => format!("Group `{name}`"),
        }
    }

    /// The Scope in the middle of a sentence *about the Scope itself*: "a Setting
    /// {} carries", "`strategy` on {} is now …".
    ///
    /// [`Scope::described`] with the capital taken off: that one reads as a
    /// subject and this one does not. A Group is unaffected, being a name.
    pub fn mentioned(&self) -> String {
        match self {
            Scope::Ungrouped => "the Ungrouped Scope".to_string(),
            Scope::Group(_) => self.described(),
        }
    }

    /// The Scope as the middle of a sentence about the Accounts in it: "every
    /// Account in {}".
    ///
    /// Not [`Scope::described`], which is ungrammatical the moment "in" is said
    /// before it.
    pub fn place(&self) -> String {
        match self {
            Scope::Ungrouped => "no Group".to_string(),
            Scope::Group(_) => self.described(),
        }
    }
}

/// The word that addresses the Accounts in no Group as a Scope, on `perch
/// config`. A Group cannot be called this, because then the Scope would be
/// unreachable — the same rule [`NO_GROUP`] carries for `perch group move`.
pub const UNGROUPED: &str = "ungrouped";

/// Whether a name is the one that means the Ungrouped Scope.
pub fn means_ungrouped(name: &str) -> bool {
    same_name(name, UNGROUPED)
}

/// The word people reach for when they mean every Scope at once.
///
/// There is no such Scope, so this word addresses nothing and a Group may not
/// take it: `perch config set global …` would then take quietly and leave every
/// other Scope as it was. Reserved, so the refusal is where that is learned.
pub const GLOBAL: &str = "global";

/// Whether a name is the one people mean every Scope at once by.
pub fn means_global(name: &str) -> bool {
    same_name(name, GLOBAL)
}

/// The Switch a scheduled Check made within a Group, so the next can be paced by
/// it (ADR a-watcher-knob-is-arithmetic). Written down only because
/// `perch watcher check` is a fresh process each time; the loop keeps the same
/// fact in memory. Per Group: a Switch within `work` says nothing about how soon
/// `personal` may move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checked {
    /// When the Switch happened, which is what the cooldown counts from.
    pub switched_at: DateTime<Utc>,
}

/// What the Ungrouped Scope holds: the declaration that those Accounts are
/// interchangeable at all, and the Settings governing how they are Cycled.
///
/// The one Scope whose record is not a bare [`Settings`], because it is the one
/// that has to say it is a Scope at all. A Group **is** that declaration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UngroupedConfig {
    /// Whether the Accounts in no Group have been declared interchangeable —
    /// what a bare `perch switch` and the watcher both need first.
    ///
    /// Off unless the user says otherwise: being ungrouped is the absence of a
    /// declaration, not a weaker form of one.
    pub interchangeable: bool,
    /// The Settings this Scope holds, like every other Scope.
    pub settings: Settings,
}

/// The word that addresses "no Group at all" on `perch group move`, and the
/// answer `perch add` accepts to the Group it offers. A Group cannot be called
/// this, because then one of the two meanings would be unreachable.
pub const NO_GROUP: &str = "none";

/// Whether a name is the one that means no Group at all.
pub fn means_no_group(name: &str) -> bool {
    same_name(name, NO_GROUP)
}

/// Whether a name is either of the two words for the Accounts in no Group.
///
/// One predicate because they were reserved as two: every command refuses both
/// as a name, so a command that took only one refused the other with a sentence
/// naming the command that takes it, and never the spelling it takes itself.
pub fn means_the_ungrouped_scope(name: &str) -> bool {
    means_ungrouped(name) || means_no_group(name)
}

/// Which of the two things sharing the namespace is being named. A refusal
/// says which: being told `none` cannot be a name is less use than being told
/// what Perch was asked to call `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Alias,
    Group,
}

impl NameKind {
    /// The kind of name, plural, so a refusal states the rule that was broken
    /// rather than a remark about the one name that broke it.
    pub fn names(self) -> &'static str {
        match self {
            NameKind::Alias => "Alias names",
            NameKind::Group => "Group names",
        }
    }

    /// The singular with its article, for a refusal that names one of them
    /// rather than stating the rule: "the registry holds an Alias `…`".
    pub fn article(self) -> &'static str {
        match self {
            NameKind::Alias => "an Alias",
            NameKind::Group => "a Group",
        }
    }
}

/// Whether two names the user chose are the same name.
///
/// Case-insensitively, because nobody remembers how they capitalized a Group
/// months ago — and over the whole of Unicode, because ASCII alone would fold
/// `work` and `Work` but not `café` and `CAFÉ`.
pub fn same_name(one: &str, other: &str) -> bool {
    // A character at a time rather than through `to_lowercase`, which allocates
    // two `String`s per comparison — and `validate`'s quadratic collision check
    // asks this on every read and every write.
    fold(one).eq(fold(other))
}

/// The fold `same_name` compares on: lowercase, with both spellings of a Greek
/// sigma brought to one. `Σ` lowercases to `ς` ending a word and to `σ` inside
/// one, which is an orthographic rule about rendering Greek text — and a Group
/// is a name somebody typed rather than a word.
fn fold(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|character| if character == 'ς' { 'σ' } else { character })
}

/// A Group name offered as a default, made from something that was never chosen
/// to be one — an organization name, which commonly has spaces in it.
///
/// Only the spaces are touched, and only into the separator chosen names already
/// use. Anything else wrong with it leaves no offer at all.
pub fn offerable_name(from: &str) -> Option<String> {
    let joined = from.split_whitespace().collect::<Vec<_>>().join("-");
    validate_name(NameKind::Group, &joined).ok()?;
    Some(joined)
}

/// Refuses a name that could not be typed, or could not be told from something
/// else (ADR a-target-has-to-be-typeable).
///
/// An allow-list of characters, then the words that already address something.
/// No `@` is an identifier character, so an address stays tellable from a name.
pub fn validate_name(kind: NameKind, name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(PerchError::Invalid(format!(
            "{} cannot be empty.",
            kind.names()
        )));
    }
    // Ahead of the allow-list, which lets `dev\u{FE00}` and `dev\u{3164}`
    // through: both are well-formed identifiers and both draw as `dev`.
    if let Some(said) = crate::host::unshowable_character_in(name) {
        return Err(PerchError::Invalid(format!(
            "{} are drawn as they are held, and this one carries {said} — so two \
             names nothing on screen tells apart are one row in every listing, \
             and a character a terminal acts on moves the column and colors the \
             row.",
            kind.names()
        )));
    }
    if let Some(carried) = name.chars().find(|c| !a_name_may_carry(*c)) {
        return Err(PerchError::Invalid(format!(
            "`{name}` carries {}, and {} are made of letters, digits, `_` and \
             `-`. Every alphabet: `café` and `日本` are names. A Target is typed \
             at a shell prompt, often on a machine other than the one that \
             named it, so a name of symbols is one somebody has to produce from \
             a keyboard to reach it.",
            said_as(carried),
            kind.names()
        )));
    }
    // Asked second, so a character wrong wherever it sits is named as that
    // rather than as a bad opening.
    if let Some(opened) = name.chars().next().filter(|c| !a_name_may_open_with(*c)) {
        return Err(PerchError::Invalid(format!(
            "`{name}` opens with {}, and {} open with a letter, a digit or `_`. \
             A name opening with `-` is a Target `perch run` could never be \
             given, its program going after the `--` that would rescue one \
             anywhere else, and a name opening with a mark draws onto whatever \
             was already on the line.",
            said_as(opened),
            kind.names()
        )));
    }
    // One block for both spellings, through the predicate that exists because
    // they were reserved as two: a Group called `ungrouped` or `none` is one no
    // `perch config set` could reach, and an Alias is the same collision.
    if means_the_ungrouped_scope(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` addresses the Accounts in no Group, so it cannot also be \
             {}.",
            kind.article()
        )));
    }
    // The third word that already means something, and the one that means
    // something Perch does not have. A Group by that name would take every
    // later `perch config set global …` quietly, so it is refused here.
    if means_global(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` is how people say every Scope at once, so it cannot also \
             be {}. There is no such Scope: every Setting is said about the one \
             it governs, and `perch config set <scope> <key> <value>` says it.",
            kind.article()
        )));
    }
    Ok(())
}

/// Whether a character may open a name.
///
/// Unicode's `XID_Start`, `_`, and an ASCII digit, which `XID_Start` does not
/// carry and `2fa` needs.
pub(crate) fn a_name_may_open_with(c: char) -> bool {
    unicode_ident::is_xid_start(c) || c == '_' || c.is_ascii_digit()
}

/// Whether a character may sit later in a name.
///
/// Unicode's `XID_Continue`, and `-`, which is the separator chosen names
/// already use and the one [`offerable_name`] writes.
pub(crate) fn a_name_may_carry(c: char) -> bool {
    unicode_ident::is_xid_continue(c) || c == '-'
}

/// One character, named as it draws and as it is spelled. Both, because a space
/// quoted alone says nothing and the punctuation that draws alike is many
/// characters.
fn said_as(c: char) -> String {
    format!("`{c}` {}", crate::host::code_point_of(c))
}

/// Which Account is active — and, while a Switch is under way, that Perch cannot
/// yet say (ADR a-switch-is-written-down-first).
///
/// One field with three states rather than two, so a registry naming both a
/// settled active Account and a different in-flight one cannot be written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Active {
    /// Perch is on nobody: a machine that has never Switched, one a removal
    /// left with nowhere to land, or one a repair took off the Account it could
    /// not make live.
    #[default]
    Nobody,
    /// One Account is active, nothing is in flight, and the live Credential is
    /// theirs as far as anything Perch wrote is concerned.
    Settled(String),
    /// A **Landing**: a Switch that has been written down and not yet recorded.
    ///
    /// Written after the Capture and before the Credential moves, so a Perch that
    /// finds one knows the live Credential belongs to one of these two Accounts.
    /// Every Switch path settles one before it acts.
    Landing {
        /// The Account being left. `None` where Perch was on nobody, which is a
        /// Switch with no Capture to lose.
        leaving: Option<String>,
        /// The Account being switched to.
        arriving: String,
    },
}

impl Active {
    /// The Account to treat as active, which during a Landing is the one being
    /// left.
    ///
    /// Nothing has been recorded as having moved, and every path that could
    /// *lose* something by believing it settles the Landing first.
    pub fn whose(&self) -> Option<&str> {
        match self {
            Active::Nobody => None,
            Active::Settled(email) => Some(email),
            Active::Landing { leaving, .. } => leaving.as_deref(),
        }
    }

    /// Whether this address is named here in any role, case-folded like every
    /// other way the registry is asked about a name.
    pub fn names(&self, email: &str) -> bool {
        match self {
            Active::Nobody => false,
            Active::Settled(held) => same_name(held, email),
            Active::Landing { leaving, arriving } => {
                same_name(arriving, email)
                    || leaving
                        .as_deref()
                        .is_some_and(|leaving| same_name(leaving, email))
            }
        }
    }

    /// The Switch that was in flight and never recorded, said out loud.
    ///
    /// Never changes an exit code: the next Switch resolves this by itself. Here
    /// rather than in a command, because `perch status` and `perch list` both
    /// say it.
    pub fn a_switch_in_flight(&self) -> Option<String> {
        let Active::Landing { leaving, arriving } = self else {
            return None;
        };
        let was_on = match leaving {
            Some(leaving) => format!("Perch was on {leaving}"),
            None => "Perch was on no Account".to_string(),
        };
        Some(format!(
            "A Switch was in flight and was not recorded — {was_on} and was \
             switching to {arriving}, so which Credential is live is not \
             settled. The next Switch resolves it, and says so if it cannot."
        ))
    }

    /// The Switch that was in flight and never recorded, as a script reads it,
    /// and `null` on every machine that is not mid-Landing.
    ///
    /// Beside whichever key already says who is active rather than folded into
    /// it: *which Account* and *whether Perch can say* are different questions.
    pub fn document(&self) -> serde_json::Value {
        match self {
            Active::Landing { leaving, arriving } => {
                serde_json::json!({"leaving": leaving, "arriving": arriving})
            }
            Active::Nobody | Active::Settled(_) => serde_json::Value::Null,
        }
    }

    /// Being on the Account a Switch was leaving, or on nobody where it was
    /// leaving nobody. What a Landing comes back to when nothing moved.
    pub fn settled_on(leaving: Option<String>) -> Active {
        match leaving {
            Some(leaving) => Active::Settled(leaving),
            None => Active::Nobody,
        }
    }

    /// Absent from the file rather than written as a word, which is what the
    /// registry of a machine that has never Switched has always looked like.
    fn is_nobody(&self) -> bool {
        matches!(self, Active::Nobody)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    /// The active Account, or the Switch under way when this was last written.
    ///
    /// Private, and the only field here that is: assigning it directly is a
    /// Switch recorded without having been written down first. Reached through
    /// `begin_landing`, `settle` and `abandon_landing`.
    #[serde(default, skip_serializing_if = "Active::is_nobody")]
    active: Active,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Alias to Account email.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// The Groups the user has declared, with the Settings each one holds. A
    /// Group exists here even when it holds no Accounts: it is a statement
    /// somebody made, not a summary of where the Accounts happen to be.
    #[serde(default)]
    pub groups: BTreeMap<String, Settings>,
    /// What the Accounts in no Group hold, taken as one Scope. Not a Group and
    /// never one; here rather than under a reserved key in `groups` so that
    /// nothing can walk it as one.
    #[serde(default)]
    pub ungrouped: UngroupedConfig,
    /// What the last scheduled Check did in each Group. Written by `perch
    /// watcher check` and by nothing else, and absent from the file until one
    /// of them Switches.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, Checked>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            version: CURRENT_VERSION,
            active: Active::Nobody,
            accounts: Vec::new(),
            aliases: BTreeMap::new(),
            groups: BTreeMap::new(),
            ungrouped: UngroupedConfig::default(),
            checks: BTreeMap::new(),
        }
    }
}

impl Account {
    pub fn email(&self) -> &str {
        &self.identity.email
    }

    /// The Profile this Account's Credential lives in.
    ///
    /// Derived from the email address the registry already keys on rather than
    /// recorded beside it (ADR claude-code-chooses-the-store): two statements of
    /// one fact can disagree.
    pub fn profile_dir(&self, host: &dyn Host) -> Result<PathBuf> {
        profile_dir_for(host, self.email())
    }

    /// Where the installed Claude Code would keep this Account's configuration
    /// if it were pointed at its Profile.
    pub fn store(&self, host: &dyn Host) -> Result<crate::probe::Store> {
        crate::probe::store_for_profile(host, &self.profile_dir(host)?)
    }

    /// Whether this Account is Quarantined, for the places that only need the
    /// fact and not the reason.
    pub fn quarantined(&self) -> bool {
        self.quarantine.is_some()
    }

    /// The cached Utilization, if any figure has ever been observed. An empty
    /// set of windows is not an observation.
    pub fn observed_utilization(&self) -> Option<&CachedUtilization> {
        self.utilization
            .as_ref()
            .filter(|cached| !cached.windows.is_empty())
    }
}

impl Registry {
    pub fn account(&self, email: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| same_name(account.email(), email))
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.active.whose().and_then(|email| self.account(email))
    }

    /// Which Account is active, or the Switch that was in flight when this was
    /// last written.
    ///
    /// Reading is nobody's to get wrong. Writing is three named transitions.
    pub fn active(&self) -> &Active {
        &self.active
    }

    /// Writes down that a Switch is about to move the live Credential, naming
    /// both Accounts it could then belong to.
    ///
    /// Hands back what it replaced, because the Landing has to reach disk before
    /// it means anything — see [`Registry::abandon_landing`].
    pub fn begin_landing(&mut self, leaving: Option<String>, arriving: &str) -> Active {
        std::mem::replace(
            &mut self.active,
            Active::Landing {
                leaving,
                arriving: arriving.to_string(),
            },
        )
    }

    /// Puts back what [`Registry::begin_landing`] replaced, where the save that
    /// would have carried it did not happen.
    ///
    /// Not [`Registry::settle`]: this Landing never existed anywhere but in
    /// memory, and nothing has moved.
    pub fn abandon_landing(&mut self, before: Active) {
        self.active = before;
    }

    /// Records who is active now that a Switch is over. `None` is a machine on
    /// nobody, and what is passed is whose Credential the machine is holding.
    ///
    /// An address rather than an [`Active`], which is what makes "settled" true
    /// of what it leaves: handed the enum it would accept a Landing.
    pub fn settle(&mut self, on: Option<String>) {
        self.active = Active::settled_on(on);
    }

    /// Whether this address is the one the registry records as active.
    ///
    /// Case-folded, like every other way the registry is asked about a name:
    /// `upsert` stores the incoming spelling, so an Identity re-read with
    /// different capitalization would leave an exact `==` answering wrongly.
    pub fn is_active(&self, email: &str) -> bool {
        self.active
            .whose()
            .is_some_and(|active| same_name(active, email))
    }

    /// Every Group name in use. A Group an Account claims is always declared
    /// too — [`load`] sees to that — so this is the declared set.
    pub fn group_names(&self) -> impl Iterator<Item = &str> {
        self.groups.keys().map(String::as_str)
    }

    /// The Settings of the Group declared under a name, whatever it was
    /// capitalized as.
    ///
    /// Through [`declared_group`](Self::declared_group), because that is how
    /// every other question about a Group name is answered here.
    pub fn group(&self, name: &str) -> Option<&Settings> {
        self.groups.get(self.declared_group(name)?)
    }

    /// Every Scope a Setting can be said at, in the order they are offered: the
    /// Ungrouped Accounts, then each Group as it was declared.
    pub fn scopes(&self) -> Vec<Scope> {
        let mut every = vec![Scope::Ungrouped];
        every.extend(
            self.group_names()
                .map(|name| Scope::Group(name.to_string())),
        );
        every
    }

    /// The Settings a Scope holds.
    ///
    /// A lookup rather than a cascade: there is no chain. A Group Perch does not
    /// hold is not a Scope, and answers with the compiled-in defaults.
    pub fn settings(&self, scope: &Scope) -> Settings {
        match scope {
            Scope::Ungrouped => self.ungrouped.settings,
            Scope::Group(name) => self.group(name).copied().unwrap_or_default(),
        }
    }

    /// The same, to write through. A Group Perch does not hold has nothing to
    /// write to: declaring one is `declare_group`'s.
    pub fn settings_mut(&mut self, scope: &Scope) -> Option<&mut Settings> {
        match scope {
            Scope::Ungrouped => Some(&mut self.ungrouped.settings),
            Scope::Group(name) => {
                let declared = self.declared_group(name)?.to_string();
                self.groups.get_mut(&declared)
            }
        }
    }

    /// The Scope an Account's Settings come from: its Group, or the Ungrouped
    /// Accounts. One place, because there is nothing to the rule but this match,
    /// which is exactly what gets written out again at a call site.
    pub fn scope_of(&self, account: &Account) -> Scope {
        match &account.group {
            Some(name) => Scope::Group(name.clone()),
            None => Scope::Ungrouped,
        }
    }

    /// The Accounts in a Group, in the order they were added.
    pub fn accounts_in(&self, group: &str) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| {
                account
                    .group
                    .as_deref()
                    .is_some_and(|held| same_name(held, group))
            })
            .collect()
    }

    /// The Accounts that are in no Group — the ordinary starting state, not an
    /// error.
    pub fn ungrouped_accounts(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.is_none())
            .collect()
    }

    /// The Group declared under a name, whatever it was capitalized as. Two
    /// names that differ only in case are one name here, so this is how a Group
    /// typed in passing is matched to the one that exists.
    pub fn declared_group(&self, name: &str) -> Option<&str> {
        self.groups
            .keys()
            .find(|declared| same_name(declared, name))
            .map(String::as_str)
    }

    /// Declares a Group, refusing a name that is not usable or already means
    /// something else.
    pub fn declare_group(&mut self, name: &str) -> Result<()> {
        self.refuse_a_name_nothing_may_answer_to(NameKind::Group, name, None)?;
        // At the compiled-in defaults, which is what every Setting means until
        // somebody says otherwise about this Group.
        self.groups.insert(name.to_string(), Settings::default());
        Ok(())
    }

    /// Refuses a name nothing may answer to: one that is not usable, one another
    /// name of the same kind holds, or one the other namespace half holds.
    ///
    /// Shape before collision. `instead_of` is the name this replaces, and
    /// renaming itself waives only the same-kind collision.
    pub fn refuse_a_name_nothing_may_answer_to(
        &self,
        kind: NameKind,
        name: &str,
        instead_of: Option<&str>,
    ) -> Result<()> {
        validate_name(kind, name)?;

        let renaming_itself = instead_of.is_some_and(|held| same_name(held, name));
        if !renaming_itself {
            // The same-kind collision. Asked here for a Group and inside
            // `refuse_taken_names` for an Alias, which is asymmetric: given an
            // Alias it checks both halves, and given a Group only the Alias one.
            if let (NameKind::Group, Some(declared)) = (kind, self.declared_group(name)) {
                return Err(PerchError::Conflict(format!(
                    "There is already a Group called `{declared}`."
                )));
            }
        }

        match kind {
            NameKind::Group => self.refuse_taken_names(None, Some(name)),
            NameKind::Alias => match renaming_itself {
                // An Account keeping its own Alias under another capitalization
                // cannot collide with the Alias it is giving up.
                true => self.refuse_a_group_of_this_name(name),
                false => self.refuse_taken_names(Some(name), None),
            },
        }
    }

    /// The half of [`Self::refuse_taken_names`] that still applies to an Alias
    /// renaming itself: the other side of the shared namespace.
    fn refuse_a_group_of_this_name(&self, name: &str) -> Result<()> {
        match self.declared_group(name) {
            Some(declared) => Err(PerchError::Conflict(format!(
                "`{declared}` is already a Group name, and a name cannot be both."
            ))),
            None => Ok(()),
        }
    }

    /// Renames a Group, keeping everything it carries.
    ///
    /// `held` is the name as this registry holds it. Three things move with it:
    /// the Settings, the Accounts that claim it, and what the last scheduled
    /// Check left — dropping that would let the watcher Switch again at once.
    pub fn rename_group(&mut self, held: &str, to: &str) -> Result<()> {
        // Resolved before the new name is judged, or
        // `perch group rename nosuchgroup work` exits as a name collision. And
        // through `declared_group`, as every question about a Group name is.
        let Some(declared) = self.declared_group(held).map(str::to_string) else {
            return Err(PerchError::NotFound(format!(
                "no Group is called `{held}`."
            )));
        };
        self.refuse_a_name_nothing_may_answer_to(NameKind::Group, to, Some(held))?;

        // `declared` is one of this map's own keys, `declared_group` having
        // just read it out — so the refusal above is the only one there is.
        let settings = self.groups.remove(&declared).unwrap_or_default();
        self.groups.insert(to.to_string(), settings);
        for account in &mut self.accounts {
            if account
                .group
                .as_deref()
                .is_some_and(|held| same_name(held, &declared))
            {
                account.group = Some(to.to_string());
            }
        }
        if let Some(checked) = self.checks.remove(&declared) {
            self.checks.insert(to.to_string(), checked);
        }
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name a
    /// Group in passing rather than to create one — `perch add --group`.
    ///
    /// Returns the spelling the Group was declared under: naming `Work` in
    /// passing joins `work`.
    pub fn ensure_group(&mut self, name: &str) -> Result<String> {
        if let Some(declared) = self.declared_group(name) {
            return Ok(declared.to_string());
        }
        self.declare_group(name)?;
        Ok(name.to_string())
    }

    /// Forgets a Group. The caller establishes nothing is left in it: dropping
    /// the Group is not a way to empty it.
    ///
    /// What a scheduled Check left goes with it, or a Group declared under the
    /// same name later would inherit a cooldown from a Group it never was.
    pub fn forget_group(&mut self, name: &str) {
        let Some(declared) = self.declared_group(name).map(str::to_string) else {
            return;
        };
        self.groups.remove(&declared);
        self.checks.remove(&declared);
    }

    /// What the last scheduled Check did within a Group, if one has Switched
    /// there.
    pub fn checked(&self, group: &str) -> Option<&Checked> {
        self.checks
            .iter()
            .find(|(declared, _)| same_name(declared, group))
            .map(|(_, checked)| checked)
    }

    /// Records a Switch a Check made, for the next one to be paced by.
    ///
    /// Filed under the spelling the Group was declared under, so a Check naming
    /// it in another case does not leave a second record pacing nothing.
    pub fn record_check(&mut self, group: &str, at: DateTime<Utc>) {
        let under = self.declared_group(group).unwrap_or(group).to_string();
        self.checks.insert(under, Checked { switched_at: at });
    }

    pub fn account_mut(&mut self, email: &str) -> Option<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| same_name(account.email(), email))
    }

    /// The same, where the Account has to be there.
    ///
    /// The state cannot happen — [`validate`] refuses every registry that could
    /// produce it, on the way in and, since `save` validates too, on the way out
    /// — so a refusal naming what could not be found beats a panic.
    pub fn held(&self, email: &str) -> Result<&Account> {
        self.account(email).ok_or_else(|| no_such_account(email))
    }

    /// [`Self::held`], for the callers that go on to change what they find.
    pub fn held_mut(&mut self, email: &str) -> Result<&mut Account> {
        match self.account_mut(email) {
            Some(account) => Ok(account),
            None => Err(no_such_account(email)),
        }
    }

    /// The Alias an Account answers to, if it has been given one.
    pub fn alias_of(&self, email: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(_, target)| same_name(target, email))
            .map(|(alias, _)| alias.as_str())
    }

    /// An Account as the user names it: by its Alias when it has one, so a
    /// message about it reads the way they would say it.
    pub fn named_for_the_user(&self, email: &str) -> String {
        match self.alias_of(email) {
            Some(alias) => format!("{email} (as `{alias}`)"),
            None => email.to_string(),
        }
    }

    /// The Alias held under a name, whatever it was capitalized as, and the
    /// Account it reaches.
    pub fn declared_alias(&self, name: &str) -> Option<(&str, &str)> {
        self.aliases
            .iter()
            .find(|(alias, _)| same_name(alias, name))
            .map(|(alias, email)| (alias.as_str(), email.as_str()))
    }

    /// Refuses an Alias and a Group name that would not both be free.
    ///
    /// The pair is checked against each other as well as against what is held: a
    /// command setting both at once could otherwise plant the collision it is
    /// meant to prevent. Two names differing only in case are one name.
    pub fn refuse_taken_names(&self, alias: Option<&str>, group: Option<&str>) -> Result<()> {
        if let (Some(alias), Some(group)) = (alias, group)
            && same_name(alias, group)
        {
            return Err(PerchError::Conflict(format!(
                "`{alias}` cannot be both an Alias and a Group name."
            )));
        }

        if let Some(alias) = alias {
            if let Some((held, target)) = self.declared_alias(alias) {
                return Err(PerchError::Conflict(format!(
                    "`{held}` already names {target}. Free it with `perch alias {held} --unset` first."
                )));
            }
            // The same lookup and sentence `refuse_a_group_of_this_name` is,
            // which an Alias renaming itself needs without the one above it.
            self.refuse_a_group_of_this_name(alias)?;
        }

        if let Some(group) = group
            && let Some((held, target)) = self.declared_alias(group)
        {
            return Err(PerchError::Conflict(format!(
                "`{held}` is already an Alias for {target}, and a name cannot be both."
            )));
        }

        Ok(())
    }

    /// Names an Account, refusing a name that is not usable or already means
    /// something else. Hands back the Alias it gave up, where it had one.
    ///
    /// An Account answers to one Alias, so naming one that already had a name
    /// replaces it rather than adding to it.
    pub fn name_account(&mut self, alias: &str, email: &str) -> Result<Option<String>> {
        let previous = self.alias_of(email).map(str::to_string);
        self.refuse_a_name_nothing_may_answer_to(NameKind::Alias, alias, previous.as_deref())?;

        // The address as the registry *holds* it, not as it was typed: the
        // lookup above folds case, and storing the typed spelling would point
        // the Alias at a string no `accounts` entry has.
        let held = self
            .account(email)
            .map_or_else(|| email.to_string(), |account| account.email().to_string());
        self.aliases.retain(|_, named| !same_name(named, email));
        self.aliases.insert(alias.to_string(), held);
        Ok(previous)
    }

    /// Frees a name, returning the name as it was held and the Account it used
    /// to reach.
    pub fn unset_alias(&mut self, alias: &str) -> Option<(String, String)> {
        let held = self.declared_alias(alias)?.0.to_string();
        let email = self.aliases.remove(&held)?;
        Some((held, email))
    }

    /// Records that an Account's Credential can no longer be used, and says
    /// whether that is news.
    ///
    /// The first reason stands: a Quarantined Account asked a second question
    /// fails a second way, and the reason worth keeping is how it broke.
    pub fn quarantine(&mut self, email: &str, why: Quarantine) -> bool {
        match self.account_mut(email) {
            Some(account) if account.quarantine.is_none() => {
                account.quarantine = Some(why);
                true
            }
            _ => false,
        }
    }

    /// Returns an Account to the pool of ones that work, reporting what it was
    /// Quarantined for. Only a login can do this: nothing else produces a
    /// Credential to replace the one that stopped working.
    pub fn release(&mut self, email: &str) -> Option<Quarantine> {
        self.account_mut(email)?.quarantine.take()
    }

    /// Forgets an Account: the entry, the Alias, and its place as the active one.
    ///
    /// Its Group is left declared, and the Credential is not this to delete —
    /// what a Profile holds is the caller's to take away first, while the Account
    /// can still be named.
    pub fn forget(&mut self, email: &str) {
        self.accounts
            .retain(|account| !same_name(account.email(), email));
        self.aliases.retain(|_, named| !same_name(named, email));
        // Either half of a Landing, and it comes back to whichever half is
        // still held: a Landing naming an Account Perch no longer holds is a
        // dangling pointer `load` refuses. Through `settle`, like every writer.
        if self.active.names(email) {
            let comes_back_to = self
                .active
                .whose()
                .filter(|whose| !same_name(whose, email))
                .map(str::to_string);
            self.settle(comes_back_to);
        }
    }

    pub fn upsert(&mut self, account: Account) {
        match self
            .accounts
            .iter_mut()
            .find(|existing| same_name(existing.email(), account.email()))
        {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }
}

/// `$PERCH_HOME`, or `~/.config/perch` — an error when neither is knowable,
/// rather than a registry written into the filesystem root.
///
/// The same path on every platform rather than `%APPDATA%`, because one rule is
/// easier to keep in a Host port exposing only a home directory. A preference.
pub fn perch_home(host: &dyn Host) -> Result<PathBuf> {
    // Set-but-empty is the machine not saying: taken at face value it makes the
    // registry a relative path, so Perch would read and write the Holdings
    // wherever it happened to be invoked from.
    if let Some(overridden) = host
        .env_var("PERCH_HOME")
        .filter(|overridden| !overridden.is_empty())
    {
        return Ok(PathBuf::from(overridden));
    }
    Ok(home_dir(host)?.join(".config").join("perch"))
}

fn home_dir(host: &dyn Host) -> Result<PathBuf> {
    host.home_dir()
        .map_err(|err| PerchError::Other(err.to_string()))
}

pub fn registry_path(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("registry.json"))
}

pub fn profiles_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("profiles"))
}

/// The Default Profile, as everything reading or writing the live Credential
/// means it: the directory Claude Code falls back to, and never a Profile.
///
/// `CLAUDE_CONFIG_DIR` is honored, but no directory under Perch's own home is
/// ever the Default Profile — and both a Run and a login point it at one.
pub fn the_default_profile(host: &dyn Host) -> Result<crate::probe::Store> {
    let told = crate::probe::default_store(host)?;
    let home = perch_home(host)?;
    if crate::host::is_inside(host, &told.config_dir, &home) {
        return crate::probe::default_profile_store(host);
    }
    Ok(told)
}

/// The Profile directory for an Account. The email is slugged because the path is
/// hashed into a keychain service name and has to be stable and printable.
///
/// An address that slugs to nothing is refused here, at the one place every store
/// is derived from.
pub fn profile_dir_for(host: &dyn Host, email: &str) -> Result<PathBuf> {
    let profiles = profiles_dir(host)?;
    let slugged = slug(email);
    let dir = profiles.join(&slugged);

    // Two ways of asking one question, because the answer is the whole machine:
    // an empty slug, and a path that is not one directory below `profiles/`.
    if slugged.is_empty() || dir.parent() != Some(profiles.as_path()) {
        return Err(PerchError::Invalid(format!(
            "`{email}` has no character a Profile directory can be named after, \
             so Perch cannot say where its Credential would be kept.\n\
             An Account recorded under that address has to be removed from \
             {} by hand.",
            registry_path(host)?.display(),
        )));
    }
    Ok(dir)
}

/// Where a login lives while Perch is running it.
///
/// Named after the moment it started, because a Profile is named after the
/// Account it holds and which Account that is only becomes knowable once the
/// login has finished (ADR a-login-perch-does-not-need).
pub fn pending_login_dir(host: &dyn Host, started_at: DateTime<Utc>) -> Result<PathBuf> {
    Ok(pending_logins_dir(host)?.join(format!("login-{}", started_at.timestamp_millis())))
}

/// Where every pending login lives, so the ones nobody came back from can be
/// found again.
pub fn pending_logins_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("pending"))
}

/// When the login that made this directory started, as its name records.
/// `login-<millis>`, written by [`pending_login_dir`], and the only account of
/// the directory's age that nothing later moves.
pub fn pending_login_started_at(dir: &Path) -> Option<DateTime<Utc>> {
    let millis: i64 = dir
        .file_name()?
        .to_str()?
        .strip_prefix("login-")?
        .parse()
        .ok()?;
    DateTime::from_timestamp_millis(millis)
}

/// Whether two Accounts derive the same Profile directory. The derivation is
/// `profiles_dir` joined with the slugged email, so sharing a slug is sharing
/// a Profile — kept here beside the derivation so the two cannot drift apart.
pub fn same_profile(one: &str, other: &str) -> bool {
    slug(one) == slug(other)
}

/// The other Account a Profile belongs to as well, where there is one.
///
/// Three commands ask it — a Switch, a Renewal and a Remove — and each spelled
/// its own scan, two of them comparing addresses by bytes where the third folded
/// case. Beside [`same_profile`], for the reason that already lives there.
pub fn sharing_a_profile_with<'a>(
    registry: &'a Registry,
    account: &Account,
) -> Option<&'a Account> {
    // Slugged once rather than once per comparison. `is_a_candidate` asks this
    // of every Account, and every Account asks `is_a_candidate`, so a `perch
    // list` over n Accounts pays for it n² times.
    let mine = slug(account.email());
    registry
        .accounts
        .iter()
        .find(|held| !same_name(held.email(), account.email()) && slug(held.email()) == mine)
}

pub fn slug(email: &str) -> String {
    let slugged: String = email
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slugged.trim_matches('-').to_string()
}

/// How long a Perch that died holding the registry lock keeps it.
///
/// Longer than the Claude Code locks a Switch takes, because it is the outer
/// lock; short enough that a killed Perch leaves a usable machine within a
/// minute.
const REGISTRY_STALE_MILLIS: i64 = 90_000;
const REGISTRY_UPDATE_MILLIS: i64 = 5_000;

/// The lock one Perch takes so that no other Perch is changing the registry at
/// the same time.
///
/// A directory, taken with the same `mkdir`-or-fail primitive the Claude Code
/// locks use: the call both asks and answers, with nothing in between.
pub fn lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch registry lock",
        held_by: "the other `perch`",
        dir: perch_home(host)?.join(".registry.lock"),
        stale_millis: REGISTRY_STALE_MILLIS,
        update_millis: REGISTRY_UPDATE_MILLIS,
        lost_means: "Another `perch` has been changing the registry since this \
                     command read it, so what this one holds in memory is behind \
                     what is on disk. Nothing of it will be written over theirs.",
    })
}

/// How long a Watcher that died holding the watcher lock keeps it.
///
/// Derived rather than chosen: the longest a healthy Watcher goes quiet is its
/// longest wait between rounds plus the round after it. Deliberately long, and
/// what pays for it is that a Watcher finding the lock held holds and comes back.
const WATCHER_STALE_MILLIS: i64 =
    (crate::watch::LONGEST_WAIT_MILLIS + crate::watch::REFRESH_INTERVAL_MILLIS) as i64;

/// Comfortably inside a round, so the renewal a round makes always touches.
const WATCHER_UPDATE_MILLIS: i64 = 60_000;

/// The lock that makes a Watcher the only one on this machine
/// (ADR the-machine-runs-the-watcher).
///
/// Two loops each keep their Cooldown in memory, where neither can see the
/// other's, so running the thing twice undoes the pacing. A Check too.
pub fn watcher_lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch watcher lock",
        // The Watcher rather than one of its three arrangements: which of them
        // holds this neither changes what to do about it nor is knowable here.
        held_by: "another Watcher",
        dir: perch_home(host)?.join(".watch.lock"),
        stale_millis: WATCHER_STALE_MILLIS,
        update_millis: WATCHER_UPDATE_MILLIS,
        lost_means: "Another Watcher has taken over watching this machine, so \
                     this one is no longer the only one deciding. It stops \
                     rather than deciding alongside it.",
    })
}

/// Shuts every other Perch out of the registry until the hold is dropped.
///
/// The hold spans the command rather than the write, because it is the *read*
/// that goes stale: a copy saved after somebody else's Switch landed would put
/// `active` back and send the next Capture to the wrong Profile.
pub fn lock(host: &dyn Host) -> Result<lock::Held<'_>> {
    lock::take_all(host, vec![lock_spec(host)?])
}

/// Reads the registry, or `None` when Perch has never run here.
pub fn load(host: &dyn Host) -> Result<Option<Registry>> {
    let path = &registry_path(host)?;
    let contents = match host.read_file(path) {
        Ok(contents) => contents,
        Err(HostError::NotFound { .. }) => return Ok(None),
        Err(err) => {
            return Err(PerchError::Other(format!(
                "could not read {}: {err}",
                path.display()
            )));
        }
    };

    // The version first, off a shape that is only the version. A newer Perch is
    // exactly the thing that writes a value this build has no variant for, and
    // reading the document first fails on that with serde's own words.
    match crate::error::claimed_version(&contents) {
        Some(version) if version > u64::from(CURRENT_VERSION) => {
            return Err(crate::error::written_by_a_newer_perch(
                &path.display().to_string(),
                "registry",
                version,
                CURRENT_VERSION,
            ));
        }
        Some(version) if version < u64::from(crate::migration::EARLIEST_VERSION) => {
            return Err(no_perch_wrote(path, Some(version)));
        }
        None if crate::migration::says_no_version(&contents) => {
            return Err(no_perch_wrote(path, None));
        }
        _ => {}
    }

    // In memory here and written back by `migration::bring_forward`, because
    // every path that writes holds the lock before it reads. Decorated as every
    // other refusal here is: a step that names a field names no file otherwise.
    let forwarded = crate::migration::forward(&contents)
        .map_err(|refused| refused.with_note(&the_file_to_edit(path)))?;

    // Strictly, so a key nobody recognizes is a refusal naming it rather than a
    // value that quietly did nothing. Every type here is Perch's own — Claude
    // Code's `.claude.json` is read through `probe`'s lenient shapes instead.
    let registry: Registry = serde_json::from_str(forwarded.as_deref().unwrap_or(&contents))
        .map_err(|err| {
            PerchError::Malformed {
                path: path.display().to_string(),
                detail: err.to_string(),
            }
            .with_note(&the_file_to_edit(path))
        })?;

    // Normalized first, so `load` and `save` judge one shape: `validate` asks
    // `declared_group` about the `checks` key, and would otherwise refuse a
    // registry this line repairs — leaving no command able to read the file.
    let registry = with_every_claimed_group_declared(registry);
    validate(&registry).map_err(|refusal| refusal.with_note(&the_file_to_edit(path)))?;

    Ok(Some(registry))
}

/// The refusal for a registry claiming a version no Perch has stamped, or none
/// (ADR a-registry-comes-forward).
///
/// Neither names a shape, and a document whose shape is unstated half-parses
/// rather than refusing.
fn no_perch_wrote(path: &Path, claimed: Option<u64>) -> PerchError {
    // Without the path, which `Malformed` has already said.
    let what = match claimed {
        Some(version) => format!(
            "it says it is registry version {version}, and no Perch has written \
             that."
        ),
        None => "it does not say which registry version it is, and every Perch \
                 has written one."
            .to_string(),
    };
    PerchError::Malformed {
        path: path.display().to_string(),
        detail: what,
    }
    .with_note(&format!(
        "The version says which shape the rest of the file is in, so Perch will \
         not guess at it. This build reads versions {} through {CURRENT_VERSION}.\n{}",
        crate::migration::EARLIEST_VERSION,
        the_file_to_edit(path),
    ))
}

/// Where to put right something only a hand edit could have put wrong.
///
/// Kept apart from [`validate`]'s rule because [`save`] is holding a registry
/// nobody hand-edited, and telling somebody to edit a value that is not in the
/// file yet is the one sentence that would make it worse.
pub fn the_file_to_edit(path: &Path) -> String {
    format!(
        "It is in {}, and every Perch command reads that file — including the \
         ones that would set it. Edit the value there.",
        path.display(),
    )
}

/// The refusal for an Account that was named and is not there.
///
/// Unreachable by construction, so it is worded as what it is rather than as
/// something to go and fix.
fn no_such_account(email: &str) -> PerchError {
    PerchError::Other(format!(
        "Perch was asked for {email}, which it does not hold, by something that \
         had already established it did.\n\
         {}\n\
         Nothing was changed.",
        crate::report::this_is_a_bug(),
    ))
}

/// Everything a registry has to be true of before any command acts on it.
///
/// Checked on the way in rather than where each value is read, because the thing
/// that reads them is a loop nobody is watching. Public because an Import writes
/// a registry without reading one, and what it accepts must not differ.
pub fn validate(registry: &Registry) -> Result<()> {
    // Every Scope, and every Scope is all of them: with no layer above, one
    // walk over the Scopes is the whole of the check.
    for scope in registry.scopes() {
        registry.settings(&scope).validate(&scope)?;
    }

    // The Group *names* an Account claims, the declared Groups, and the Aliases
    // with them: a hand-edited registry is exactly where a name nothing would
    // have accepted comes from, in any of the three.
    let claimed = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.as_deref());
    for name in claimed.chain(registry.groups.keys().map(String::as_str)) {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Group, name)?;
    }
    for name in registry.aliases.keys() {
        refuse_a_name_nothing_would_have_accepted(registry, NameKind::Alias, name)?;
    }

    // What each Alias points *at*, which the loop above does not look at. A
    // dangling one is not a refusal downstream — it is the `expect` in every
    // command that resolves a Target.
    let mut named: Vec<(&str, &str)> = Vec::new();
    for (alias, email) in &registry.aliases {
        if registry.account(email).is_none() {
            return Err(PerchError::Invalid(format!(
                "The registry gives the Alias `{alias}` to {email}, which is not \
                 an Account Perch holds.",
            )));
        }
        // One Account, one Alias. With two, `alias_of` returns whichever the map
        // yields first, so `perch list` shows one while `perch switch` answers to
        // both — the same undecided answer as two names differing only in case.
        if let Some((already, _)) = named.iter().find(|(_, held)| same_name(held, email)) {
            return Err(PerchError::Invalid(format!(
                "The registry gives {email} both the Alias `{already}` and the \
                 Alias `{alias}`, and an Account answers to one Alias at a time \
                 — so which of them Perch shows it under is not decided by \
                 anything.",
            )));
        }
        named.push((alias, email));
    }

    // The other pointer into the Accounts, and both ends of a Landing, because
    // resolving one reads the Credential of the other. Holding nothing is a
    // state and not a fault.
    match &registry.active {
        Active::Nobody => {}
        Active::Settled(active) => refuse_a_dangling_pointer(
            registry,
            active,
            &format!("says {active} is the active Account"),
        )?,
        Active::Landing { leaving, arriving } => {
            if let Some(leaving) = leaving {
                refuse_a_dangling_pointer(
                    registry,
                    leaving,
                    &format!("says a Switch away from {leaving} was under way"),
                )?;
            }
            refuse_a_dangling_pointer(
                registry,
                arriving,
                &format!("says a Switch to {arriving} was under way"),
            )?;
        }
    }

    // The third member of the namespace. `validate_name` keeps an Alias and a
    // Group name tellable from an address, no `@` being an identifier
    // character; the mirror rule, that an address looks like one, is this.
    for account in &registry.accounts {
        if !account.email().contains('@') {
            return Err(PerchError::Invalid(format!(
                "The registry holds an Account called `{}`, which is not an \
                 address an Alias or a Group name could be told from — and a \
                 Target that could be either has no single answer.",
                account.email(),
            )));
        }
        // Nothing here about a character a terminal would act on. An address is
        // Claude Code's rather than anybody's choice, so it is refused where it
        // enters and drawn through `Shown` (ADR nothing-drawn-is-obeyed).
    }

    // One entry per Account. `upsert` replaces the matching entry, so two for
    // one address is a hand edit — after which `account` acts on the first,
    // `perch list` renders two rows, and a Cycle counts it twice.
    if let Some((already, again)) = first_collision(registry.accounts.iter().map(Account::email)) {
        return Err(PerchError::Invalid(format!(
            "The registry holds two Accounts spelled `{already}` and `{again}`, \
             which are one Account — so which entry a command reads, and which \
             one it writes, is not decided by anything."
        )));
    }

    // What `checks` is keyed on, the one pointer into the Group namespace with no
    // rule of its own: `record_check` keeps the name it was handed when it cannot
    // resolve one, and `forget_group` only clears what it can.
    for named in registry.checks.keys() {
        if same_name(named, UNGROUPED) || registry.declared_group(named).is_some() {
            continue;
        }
        return Err(PerchError::Invalid(format!(
            "The registry records a Check against `{named}`, which is neither a \
             Group Perch holds nor the Accounts in no Group — so the Cooldown it \
             carries paces nothing."
        )));
    }

    // `checked` answers with the first match in `BTreeMap` order and
    // `record_check` writes under the declared spelling, so two keys that fold
    // to one pace the next Check off a record nothing is keeping.
    if let Some((already, name)) = first_collision(registry.checks.keys().map(String::as_str)) {
        return Err(PerchError::Invalid(format!(
            "The registry records a Check against `{already}` and one against \
             `{name}`, which are one Group — so which Cooldown paces the next \
             one is not decided by anything."
        )));
    }

    refuse_two_names_that_differ_only_in_case(NameKind::Group, registry.groups.keys())?;
    refuse_two_names_that_differ_only_in_case(NameKind::Alias, registry.aliases.keys())?;

    // The percentages a Cycle ranks on. A negative figure gives
    // `cycle::headroom_of` over 100% of headroom, so `used_percent >= threshold`
    // never fires; serde_json already refuses a literal too large for an `f64`.
    for account in &registry.accounts {
        for window in account
            .utilization
            .iter()
            .flat_map(|cached| &cached.windows)
        {
            if !(0.0..=100.0).contains(&window.used_percent) {
                return Err(PerchError::Invalid(format!(
                    "The registry says {} is {}% through its {} window, and a \
                     window is between 0 and 100 percent full — so what a Cycle \
                     would rank it on, and what the watcher would compare \
                     against a threshold, is not a figure at all.\n\
                     Deleting the Account's `utilization` lets a `perch status \
                     --refresh` read it again.",
                    account.email(),
                    window.used_percent,
                    window.window,
                )));
            }
        }
    }

    Ok(())
}

/// Refuses one of `active`'s pointers into the Accounts naming somebody Perch
/// does not hold. `said` is what the registry claims about that address, so
/// each of the three pointers a Landing can carry says which one it was.
fn refuse_a_dangling_pointer(registry: &Registry, email: &str, said: &str) -> Result<()> {
    if registry.account(email).is_some() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "The registry {said}, which is not an Account Perch holds."
    )))
}

/// Refuses a pair of names in one half of the namespace that only case tells
/// apart. Both halves, because the namespace is shared and one copy of the rule
/// is how the two cannot come to disagree about it.
fn refuse_two_names_that_differ_only_in_case<'a>(
    kind: NameKind,
    names: impl Iterator<Item = &'a String>,
) -> Result<()> {
    match first_collision(names.map(String::as_str)) {
        None => Ok(()),
        Some((already, name)) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{already}` and `{name}`, which differ only \
             in case — so which one a Target finds is not decided by anything.",
            kind.article(),
        ))),
    }
}

/// The first pair of names in a sequence that [`same_name`] cannot tell apart,
/// earlier one first.
///
/// Quadratic, deliberately: it is a registry, and the alternative is a map keyed
/// on a lowercased copy of every name.
fn first_collision<'a>(names: impl Iterator<Item = &'a str>) -> Option<(&'a str, &'a str)> {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if let Some(already) = seen.iter().find(|held| same_name(held, name)) {
            return Some((already, name));
        }
        seen.push(name);
    }
    None
}

/// Refuses a name in the registry that nothing would have accepted.
///
/// Named rather than repaired: a value only a hand edit can produce is one only a
/// hand edit can take out. The cross-half collision is asked from the Group side
/// only, because every Group is walked either way.
fn refuse_a_name_nothing_would_have_accepted(
    registry: &Registry,
    kind: NameKind,
    name: &str,
) -> Result<()> {
    // `none` is not a case of its own here: `validate_name` already refuses it
    // for both kinds, in words true of a claim and a declaration alike — and
    // this loop walks *declared* Groups too.
    let refused = validate_name(kind, name)
        .err()
        .map(|refusal| refusal.to_string())
        .or_else(|| {
            (kind == NameKind::Group)
                .then(|| registry.declared_alias(name).map(|(held, _)| held))
                .flatten()
                .map(|alias| {
                    format!(
                        "`{alias}` is already an Alias, and Aliases and Group \
                         names share one namespace"
                    )
                })
        });

    match refused {
        None => Ok(()),
        Some(why) => Err(PerchError::Invalid(format!(
            "The registry holds {} `{name}`, which is not a name Perch would \
             have accepted: {why}.",
            kind.article(),
        ))),
    }
}

/// Declares any Group an Account claims but nothing declared.
///
/// One nothing declares falls out of `perch list`, which walks the declared
/// Groups and then the Accounts in none (ADR the-listing-owns-the-set). A claim
/// differing only in case joins rather than becoming a second key.
pub(crate) fn with_every_claimed_group_declared(mut registry: Registry) -> Registry {
    let claimed: Vec<String> = registry
        .accounts
        .iter()
        .filter_map(|account| account.group.clone())
        .collect();
    for name in claimed {
        match registry.declared_group(&name) {
            Some(declared) if declared != name => {
                let declared = declared.to_string();
                for account in &mut registry.accounts {
                    if account.group.as_deref().is_some_and(|of| of == name) {
                        account.group = Some(declared.clone());
                    }
                }
            }
            Some(_) => {}
            None => {
                registry.groups.insert(name, Settings::default());
            }
        }
    }
    with_every_check_under_the_declared_spelling(registry)
}

/// The same for what `checks` is keyed on. `checked` and `validate` fold the
/// key and the two mutators remove it exactly, so a key differing only in case
/// outlives its Group and leaves `validate` refusing what `save` just built.
fn with_every_check_under_the_declared_spelling(mut registry: Registry) -> Registry {
    let keyed: Vec<String> = registry.checks.keys().cloned().collect();
    for name in keyed {
        // The Ungrouped Scope has no declaration to be brought to, so it is
        // brought to the constant `record_check` writes. Without this the key is
        // the only one in the map that can outlive its own spelling.
        let declared = match means_ungrouped(&name) {
            true => Some(UNGROUPED.to_string()),
            false => registry.declared_group(&name).map(str::to_string),
        };
        let Some(declared) = declared else {
            continue;
        };
        if declared == name {
            continue;
        }
        if let Some(checked) = registry.checks.remove(&name) {
            // The later of the two, where both spellings carry a record: byte
            // order would otherwise decide, and the older one winning is a
            // Check free to Switch inside a Cooldown still running.
            let later = match registry.checks.get(&declared) {
                Some(held) if held.switched_at > checked.switched_at => held.clone(),
                _ => checked,
            };
            registry.checks.insert(declared, later);
        }
    }
    registry
}

/// Writes the registry, under the hold the caller took to read it.
///
/// The hold is a parameter because a registry is only ever written by the Perch
/// that read it, and is where [`validate`] is asked on the way out, so what this
/// writes and what [`load`] accepts cannot differ.
pub fn save(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &mut Registry) -> Result<()> {
    perch.renew();
    if !perch.still_held() {
        // A general failure rather than `Busy`, deliberately
        // (ADR a-refusal-is-a-promise): `Busy` promises nothing was changed, and
        // this save is reached as often after a Credential moved as before.
        return Err(PerchError::Other(
            "Another `perch` took the registry lock over while this command was \
             working, and has changed the registry since this one read it. \
             Nothing was written, because writing would have undone whatever it \
             did. Run this command again."
                .to_string(),
        ));
    }

    // What `load` will accept, asked before `load` has to refuse it: a command
    // writing a file every later command declined to read would leave a machine
    // with no working `perch` on it, and no `perch holdings purge` either.
    validate(registry).map_err(|invalid| {
        PerchError::Other(format!(
            "{invalid}\n\n{}\n\
             Nothing was written, and the registry on disk is as it was.",
            crate::report::this_is_a_bug(),
        ))
    })?;

    let path = registry_path(host)?;
    // In place, so the caller's field cannot disagree with the file — and because
    // cloning the Holdings to set one `u32` is a deep copy of every Account and
    // its figures per write, which a Watcher pays every round.
    registry.version = CURRENT_VERSION;
    let body = serde_json::to_string_pretty(&*registry)
        .map_err(|err| PerchError::Other(format!("could not serialize the registry: {err}")))?;
    write(host, &path, &format!("{body}\n"))
}

/// Replaces the registry in one step, or not at all, and for its owner alone.
///
/// One step because every command reads this file first, and a crash mid-write
/// would leave it half written for good. Its owner alone because it holds no
/// Credential and everything else about every Account.
fn write(host: &dyn Host, path: &Path, contents: &str) -> Result<()> {
    host.write_private_file(path, contents)
        .map_err(|err| PerchError::file_write(path, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::prelude::*;
    use chrono::TimeZone;

    #[test]
    fn an_email_slugs_to_a_stable_directory_name() {
        assert_eq!(slug("Someone@Example.com"), "someone-example-com");
    }

    #[test]
    fn an_address_that_names_no_directory_is_refused_rather_than_naming_them_all() {
        let host = crate::host::FakeHost::new();
        let profiles = profiles_dir(&host).unwrap();

        for degenerate in ["@", "-", "...", "@.-@"] {
            assert_eq!(slug(degenerate), "", "the case this is about: {degenerate}");
            let refused =
                profile_dir_for(&host, degenerate).expect_err("no Profile can be named after this");
            assert!(refused.to_string().contains(degenerate), "{refused}");
        }

        assert_eq!(
            profile_dir_for(&host, "someone@example.com").unwrap(),
            profiles.join("someone-example-com"),
            "and an ordinary address is unaffected"
        );
    }

    #[test]
    fn one_perch_changes_the_registry_at_a_time() {
        let host = crate::host::FakeHost::new();

        let held = lock(&host).expect("the first Perch takes it");
        let refused = match lock(&host) {
            Err(refused) => refused,
            Ok(_) => panic!("the second Perch must wait, then give up"),
        };
        assert!(
            refused.to_string().contains("the Perch registry lock"),
            "{refused}"
        );

        drop(held);
        lock(&host).expect("a lock given back can be taken again");
    }

    /// The fold is the whole of the identity a name has, so it is stated here
    /// rather than only through the callers that ask it. `İ` lowercases to two
    /// characters, which is where a naive pairwise fold parts company with
    /// `to_lowercase`; `ΟΔΟΣ` is where `to_lowercase` parts company with a name.
    #[test]
    fn two_names_are_one_name_whenever_the_only_difference_is_case() {
        for (one, other) in [
            ("work", "Work"),
            ("café", "CAFÉ"),
            ("İ", "İ"),
            ("straße", "STRAßE"),
            ("ΟΔΟΣ", "οδος"),
            ("ΟΔΟΣ", "οδοσ"),
        ] {
            assert!(same_name(one, other), "{one} and {other} are one name");
        }
        for (one, other) in [("work", "works"), ("café", "cafe"), ("", "a")] {
            assert!(!same_name(one, other), "{one} and {other} are two names");
        }
    }

    /// The one place the fold is deliberately not `to_lowercase`'s, stated as a
    /// case rather than left to the reader to derive from the sigma rule.
    #[test]
    fn a_greek_name_is_one_name_where_to_lowercase_makes_it_two() {
        assert_ne!(
            "ΟΔΟΣ".to_lowercase(),
            "οδοσ".to_lowercase(),
            "`to_lowercase` writes a final sigma as `ς`"
        );
        assert!(same_name("ΟΔΟΣ", "οδοσ"), "and a name is not a Greek word");
    }

    #[test]
    fn an_account_is_found_however_its_address_is_capitalized() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "café@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the same Account, spelled the way somebody typed it");

        assert!(registry.account("CAFÉ@example.com").is_some());
        assert!(registry.account_mut("CAFÉ@EXAMPLE.COM").is_some());
        assert_eq!(registry.alias_of("Café@Example.com"), Some("work"));

        registry.settle(Some("CAFÉ@EXAMPLE.COM".into()));
        assert!(
            registry.is_active("café@example.com"),
            "and which Account is active is the same question, asked the same \
             way — a dozen call sites compared these by exact bytes, which is \
             the one place in Perch an address was not case-folded"
        );
        assert!(!registry.is_active("someone@example.com"));

        registry.forget("CAFÉ@example.com");
        assert!(registry.accounts.is_empty(), "and it is the one that goes");
        assert!(registry.aliases.is_empty(), "with the name it answered to");
    }

    /// Two Accounts, which is the fewest a Landing needs.
    fn holding_two() -> Registry {
        let mut registry = Registry::default();
        for email in ["one@example.com", "two@example.com"] {
            registry.upsert(Account {
                identity: Identity {
                    email: email.into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        registry
    }

    #[test]
    fn forgetting_the_arriving_half_of_a_landing_comes_back_to_the_one_being_left() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("two@example.com");

        assert_eq!(
            registry.active().whose(),
            Some("one@example.com"),
            "the Account still held is the one the registry is on"
        );
        assert!(
            registry.active().a_switch_in_flight().is_none(),
            "and the Landing is gone with the half that went"
        );
    }

    #[test]
    fn forgetting_the_leaving_half_of_a_landing_settles_on_nobody() {
        let mut registry = holding_two();
        registry.begin_landing(Some("one@example.com".into()), "two@example.com");

        registry.forget("one@example.com");

        assert_eq!(*registry.active(), Active::Nobody);
    }

    #[test]
    fn an_account_that_was_named_and_is_not_there_is_refused_rather_than_panicked_on() {
        let registry = Registry::default();

        let refused = registry
            .held("nobody@example.com")
            .expect_err("Perch does not hold it");

        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("bug in Perch"),
            "whose fault it is, because there is nothing for a person to do: {refused}"
        );
        assert_eq!(refused.exit_code(), crate::error::EXIT_GENERAL);

        // The mutable half answers the same way: a caller that goes on to change
        // what it finds needs the other borrow.
        let mut registry = Registry::default();
        assert_eq!(
            registry
                .held_mut("nobody@example.com")
                .expect_err("Perch does not hold it")
                .to_string(),
            refused.to_string()
        );
    }

    /// The `active` states here are written to the field rather than through the
    /// transitions, and this is the one place that is right: a dangling pointer
    /// is a state no transition can produce.
    #[test]
    fn an_active_pointer_naming_nothing_is_refused_like_a_dangling_alias() {
        let mut registry = Registry {
            active: Active::Settled("nobody@example.com".to_string()),
            ..Default::default()
        };

        let refused = validate(&registry).expect_err("it names an Account Perch does not hold");
        assert!(
            refused.to_string().contains("nobody@example.com"),
            "{refused}"
        );

        registry.active = Active::Nobody;
        validate(&registry).expect("holding nothing is a state rather than a fault");
    }

    #[test]
    fn both_ends_of_a_landing_are_refused_when_they_name_nothing() {
        let held = "someone@example.com";
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: held.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        for (what, active, names, says) in [
            (
                "the Account it was switching to",
                Active::Landing {
                    leaving: Some(held.to_string()),
                    arriving: "nobody@example.com".to_string(),
                },
                "nobody@example.com",
                "a Switch to nobody@example.com was under way",
            ),
            (
                "the Account it was leaving",
                Active::Landing {
                    leaving: Some("nobody@example.com".to_string()),
                    arriving: held.to_string(),
                },
                "nobody@example.com",
                "a Switch away from nobody@example.com was under way",
            ),
        ] {
            registry.active = active;

            let refused = validate(&registry).expect_err("it names an Account Perch does not hold");

            assert!(refused.to_string().contains(names), "{what}: {refused}");
            assert!(
                refused.to_string().contains(says),
                "{what}: and says which end of the Landing dangles: {refused}"
            );
        }

        // A Landing naming Accounts Perch does hold is a state, not a fault: it
        // is what every interrupted Switch leaves, and the next one resolves it.
        registry.active = Active::Landing {
            leaving: None,
            arriving: held.to_string(),
        };
        validate(&registry).expect("a Switch left in flight is a machine to load, not to refuse");
    }

    /// A table, because the rules are one fact with two spellings — asked of the
    /// one function all four callers go through.
    #[test]
    fn what_a_name_may_be_is_one_rule_for_both_halves_of_the_namespace() {
        /// An Account answering to `work`, and a Group called `personal`.
        fn held() -> Registry {
            let mut registry = Registry::default();
            registry.upsert(Account {
                identity: Identity {
                    email: "someone@example.com".into(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
            registry
                .name_account("work", "someone@example.com")
                .expect("the name is free");
            registry.declare_group("personal").expect("so is this one");
            registry
        }

        let cases: &[(NameKind, &str, Option<&str>, Option<&str>)] = &[
            // kind, name, instead_of, the refusal it earns (None = accepted)
            (NameKind::Group, "spare", None, None),
            (NameKind::Alias, "spare", None, None),
            // Its own half.
            (NameKind::Group, "personal", None, Some("already a Group")),
            (NameKind::Alias, "work", None, Some("already names")),
            // Two names that differ only in case are one name.
            (NameKind::Group, "PERSONAL", None, Some("already a Group")),
            (NameKind::Alias, "WORK", None, Some("already names")),
            // The other half, which is what makes the namespace shared.
            (NameKind::Group, "work", None, Some("already an Alias")),
            (
                NameKind::Alias,
                "personal",
                None,
                Some("already a Group name"),
            ),
            // Renaming itself is not colliding with itself, recapitalization
            // included, and the same waiver on both halves.
            (NameKind::Group, "Personal", Some("personal"), None),
            (NameKind::Alias, "Work", Some("work"), None),
            // Shape before collision.
            (NameKind::Group, "", None, Some("cannot be empty")),
            (NameKind::Alias, "", None, Some("cannot be empty")),
            (
                NameKind::Alias,
                "has a space",
                None,
                Some("carries ` ` (U+0020)"),
            ),
            (
                NameKind::Group,
                "none",
                None,
                Some("addresses the Accounts in no Group"),
            ),
            // Not whitespace, and so not caught by the clause above — and the
            // one a terminal reads as an instruction rather than as a name.
            (
                NameKind::Group,
                "\u{1b}[31mred",
                None,
                Some("a control character (U+001B)"),
            ),
            (
                NameKind::Alias,
                "bell\u{7}",
                None,
                Some("a control character (U+0007)"),
            ),
            // `char::is_control` is `Cc` alone, so neither of these was caught
            // by the clause above: the first reverses the rest of the line it is
            // drawn on, and the second is a name drawn identically to `work`.
            (
                NameKind::Group,
                "\u{202e}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+202E)"),
            ),
            (
                NameKind::Alias,
                "wo\u{200b}rk",
                None,
                Some("a character a terminal does not draw as itself (U+200B)"),
            ),
            // The word joiner and the bidi isolates, which are the same harm
            // under two more blocks of the table.
            (
                NameKind::Group,
                "wo\u{2060}rk",
                None,
                Some("a character a terminal does not draw as itself (U+2060)"),
            ),
            (
                NameKind::Group,
                "\u{2066}gpj.exe",
                None,
                Some("a character a terminal does not draw as itself (U+2066)"),
            ),
        ];

        for (kind, name, instead_of, refusal) in cases {
            let asked = held().refuse_a_name_nothing_may_answer_to(*kind, name, *instead_of);
            match refusal {
                None => asked.unwrap_or_else(|err| {
                    panic!("{kind:?} `{name}` should be free: {err}");
                }),
                Some(said) => {
                    let refused = asked.expect_err(&format!("{kind:?} `{name}` is not free"));
                    assert!(
                        refused.to_string().contains(said),
                        "{kind:?} `{name}`: expected {said:?}, got {refused}"
                    );
                }
            }
        }
    }

    /// The registry here is built by hand because nothing reachable produces it:
    /// a Group cannot be declared under a name an Alias already holds. It is what
    /// a third way of making a name would walk into.
    #[test]
    fn recapitalizing_an_alias_still_cannot_walk_into_a_group() {
        let mut registry = Registry::default();
        registry
            .aliases
            .insert("work".to_string(), "someone@example.com".to_string());
        registry
            .groups
            .insert("Work".to_string(), Settings::default());

        let refused = registry
            .refuse_a_name_nothing_may_answer_to(NameKind::Alias, "Work", Some("work"))
            .expect_err("the shared namespace is still checked");

        assert!(
            refused.to_string().contains("already a Group name"),
            "{refused}"
        );
    }

    /// Nothing reachable produces one, so what is asserted is the guard rather
    /// than the property: what happens on the day a command gets it wrong, and
    /// that the file on disk is left alone.
    #[test]
    fn a_registry_load_would_not_read_is_one_save_declines_to_write() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");
        let path = registry_path(&host).unwrap();
        save(&host, &mut perch, &mut Registry::default()).expect("an empty one is fine");
        let before = host.file(&path).expect("it was written");

        // A dangling Alias is not a refusal downstream — it is the `expect` in
        // every command that resolves a Target.
        let mut broken = Registry::default();
        broken
            .aliases
            .insert("work".to_string(), "nobody@example.com".to_string());

        let refused = save(&host, &mut perch, &mut broken).expect_err("load would not read it");

        let said = refused.to_string();
        assert!(said.contains("nobody@example.com"), "the rule: {said}");
        assert!(said.contains("bug in Perch"), "whose fault it is: {said}");
        assert!(
            !said.contains("Edit the value there"),
            "and not an instruction to edit a file it did not write: {said}"
        );
        assert_eq!(
            refused.exit_code(),
            crate::error::EXIT_GENERAL,
            "a script told 14 would read it as its own input being wrong"
        );
        assert_eq!(
            host.file(&path).as_deref(),
            Some(before.as_str()),
            "and the registry on disk is untouched, which is the whole of it"
        );
    }

    #[test]
    fn a_command_that_takes_its_time_keeps_the_lock_it_took() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // Past the staleness window several times over: the shape of a
        // `perch remove` waiting on somebody who walked away.
        for _ in 0..4 {
            host.sleep(REGISTRY_STALE_MILLIS as u64 - 10_000);
            save(&host, &mut perch, &mut Registry::default())
                .expect("it is still Perch's to write");
        }

        assert!(perch.still_held());
        assert!(
            lock(&host).is_err(),
            "and no other Perch could have taken it in the meantime"
        );
    }

    #[test]
    fn a_registry_read_before_somebody_elses_command_is_not_written_over_theirs() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // The stall, and another Perch finding the lock abandoned and taking it.
        host.sleep(REGISTRY_STALE_MILLIS as u64 + 1_000);
        let theirs = lock(&host).expect("an abandoned lock is taken over");
        save(&host, &mut { theirs }, &mut Registry::default()).expect("theirs is the live hold");
        let before = load(&host).expect("it reads").expect("they wrote one");

        let mut stale = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        let refused =
            save(&host, &mut perch, &mut stale).expect_err("this one may no longer write");

        assert!(
            refused.to_string().contains("Run this command again"),
            "{refused}"
        );
        assert_eq!(
            load(&host).expect("it reads").expect("a registry is there"),
            before,
            "what the other Perch wrote is what is on disk"
        );
    }

    /// On a fresh machine the *lock* is what brings Perch's home into being,
    /// before any registry has been written into it.
    #[test]
    fn the_home_the_lock_creates_is_the_owners_alone() {
        let host = crate::host::FakeHost::new();

        let _perch = lock(&host).expect("the registry lock is free");

        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE)
        );
    }

    #[test]
    fn a_profile_reached_through_a_link_is_still_not_the_default_profile() {
        let home = "/Users/someone/.config/perch";
        let host = crate::host::FakeHost::new()
            // How somebody comes to have this: a shorter name for the Profiles
            // directory, and a `CLAUDE_CONFIG_DIR` pointing inside it.
            .with_link(
                crate::host::Link::Symbolic,
                format!("{home}/profiles"),
                "/Users/someone/claude",
            )
            .with_env("CLAUDE_CONFIG_DIR", "/Users/someone/claude/work");

        let store = the_default_profile(&host).expect("a Default Profile is known");

        assert!(
            !crate::host::is_inside(
                &host,
                &store.config_dir,
                std::path::Path::new("/Users/someone/claude")
            ),
            "a Profile is never the Default Profile, whichever name reaches it: {:?}",
            store.config_dir
        );
        assert_eq!(
            store.config_dir,
            crate::probe::default_profile_store(&host)
                .expect("the real Default Profile")
                .config_dir,
        );
    }

    #[test]
    fn a_check_against_a_group_nothing_declares_is_not_a_registry() {
        let mut registry = Registry::default();
        registry.checks.insert(
            "a-group-nobody-declared".to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );

        let refused = validate(&registry).expect_err("the Cooldown paces nothing");
        assert!(
            refused.to_string().contains("a-group-nobody-declared"),
            "it names the entry: {refused}"
        );

        // The Ungrouped Scope keeps one and is not a Group, which is why the
        // fallback that produces these entries exists at all.
        registry.checks.clear();
        registry.checks.insert(
            UNGROUPED.to_string(),
            Checked {
                switched_at: Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap(),
            },
        );
        validate(&registry).expect("the Accounts in no Group Cycle too");
    }

    /// `checked` answers with the first in `BTreeMap` order and `record_check`
    /// writes under the declared spelling, so the record read is not the record
    /// kept — and a Cooldown read off a stale one Switches sooner than 15
    /// minutes.
    #[test]
    fn two_checks_against_one_group_are_not_a_registry() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        for spelling in ["Work", "work"] {
            registry
                .checks
                .insert(spelling.to_string(), Checked { switched_at: at });
        }

        let refused = validate(&registry).expect_err("one of the two paces nothing");
        let said = refused.to_string();
        assert!(
            said.contains("Work") && said.contains("work"),
            "it names both spellings: {said}"
        );
    }

    /// `checked` and `validate` fold the key and the two mutators remove it
    /// exactly, so a `checks` key that outlives its Group leaves `validate`
    /// refusing what `save` has just built — under a sentence saying the fault
    /// is Perch's, on a registry no command can then read.
    #[test]
    fn a_check_keyed_in_another_case_than_its_group_is_brought_to_one_on_the_way_in() {
        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        registry
            .checks
            .insert("Work".to_string(), Checked { switched_at: at });

        let mut registry = with_every_claimed_group_declared(registry);
        validate(&registry).expect("one Group, one Check");
        assert_eq!(
            registry.checks.keys().collect::<Vec<_>>(),
            vec!["work"],
            "the Check is filed under the spelling the Group was declared under"
        );

        registry.forget_group("work");
        validate(&registry).expect("and it goes when the Group it paces goes");
        assert!(registry.checks.is_empty(), "{:?}", registry.checks);
    }

    /// The fold happens before `validate`, so the collision it refuses is one
    /// `load` never reaches — it merges instead, and merging by insertion order
    /// lets `"Work"` (0x57) write over `"work"` whichever is fresher.
    #[test]
    fn two_checks_folding_to_one_group_keep_the_later_switch() {
        let noon = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        let january = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut registry = Registry::default();
        registry
            .groups
            .insert("work".to_string(), Settings::default());
        registry
            .checks
            .insert("Work".to_string(), Checked { switched_at: noon });
        registry.checks.insert(
            "work".to_string(),
            Checked {
                switched_at: january,
            },
        );

        let registry = with_every_claimed_group_declared(registry);

        assert_eq!(
            registry.checked("work").map(|it| it.switched_at),
            Some(noon),
            "the older record winning is a Check free to Switch at once"
        );
    }

    #[test]
    fn an_alias_points_at_the_address_as_the_registry_holds_it() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: crate::probe::Identity {
                email: "café@example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        registry
            .name_account("work", "CAFÉ@example.com")
            .expect("the name is free and the Account is there");

        assert_eq!(
            registry.aliases.get("work").map(String::as_str),
            Some("café@example.com"),
            "the Alias names the Account, and the Account has one spelling"
        );
    }

    #[test]
    fn a_key_the_registry_does_not_know_is_refused_rather_than_ignored() {
        let path = "/Users/someone/.config/perch/registry.json";
        for (written, key) in [
            (
                format!(
                    "{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"groups\":\
                     {{\"work\":{{\"watcher_treshold_percent\":50}}}}}}"
                ),
                "watcher_treshold_percent",
            ),
            (
                format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"aliasses\":{{}}}}"),
                "aliasses",
            ),
            // A transposed `leaving` deserializes as `None`, which is not
            // nothing: it is the Landing saying Perch had been on nobody, so the
            // Capture that resumes it has no Profile to file into.
            (
                format!(
                    "{{\"version\":{CURRENT_VERSION},\"accounts\":[],\"active\":\
                     {{\"landing\":{{\"leavign\":\"one@example.com\",\
                     \"arriving\":\"two@example.com\"}}}}}}"
                ),
                "leavign",
            ),
        ] {
            let host = crate::host::FakeHost::new().with_file(path, &written);

            let refused = load(&host).expect_err("a key Perch does not know is not a registry");

            let said = refused.to_string();
            assert!(
                said.contains(key),
                "it names the key it could not read: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the file to put it right in: {said}"
            );
        }
    }

    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.config/perch/registry.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_unwritable_file(path, "No space left on device (os error 28)");

        // Holding the Account its `active` names, so `validate` passes and the
        // unwritable file is what fails the save: refused at the first step, the
        // two assertions below are true of a write that was never attempted.
        let mut registry = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        registry.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        let mut perch = lock(&host).expect("the registry lock is free");
        let refused = save(&host, &mut perch, &mut registry).expect_err("the write cannot land");
        assert!(
            refused.to_string().contains("No space left on device"),
            "the file is what refused it, rather than the shape: {refused}"
        );

        assert_eq!(
            host.file(path).as_deref(),
            Some(before.as_str()),
            "a reader still sees the registry that was there"
        );
        assert_eq!(
            host.file(crate::host::temp_beside(&host, Path::new(path))),
            None,
            "and the half-written copy is not left beside it"
        );
    }

    #[test]
    fn the_registry_is_written_for_its_owner_alone() {
        let host = crate::host::FakeHost::new();

        let mut perch = lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &mut Registry::default()).expect("it is written");

        let path = registry_path(&host).unwrap();
        assert_eq!(host.mode_of(&path), Some(crate::host::PRIVATE_FILE_MODE));
        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE),
            "a directory others may enter is a directory whose contents others \
             may open"
        );
    }

    #[test]
    fn a_registry_round_trips_through_json() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: Some("Acme".into()),
                organization_uuid: None,
            },
            plan: Some("pro".into()),
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });
        registry.settle(Some("someone@example.com".into()));

        let json = serde_json::to_string(&registry).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
        assert_eq!(back.active_account().unwrap().plan.as_deref(), Some("pro"));
    }

    #[test]
    fn what_a_check_recorded_survives_the_file_and_is_absent_until_one_switches() {
        let mut registry = Registry::default();
        assert!(
            !serde_json::to_string(&registry).unwrap().contains("checks"),
            "a machine nothing has been scheduled on records no checks"
        );

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_check("work", at);
        let back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();

        let recorded = back.checked("work").expect("the Group it Switched within");
        assert_eq!(recorded.switched_at, at);
        assert_eq!(
            back.checked("personal"),
            None,
            "a cooldown is paced per Group, and a Switch within one says \
             nothing about how soon another may move"
        );
    }

    #[test]
    fn forgetting_a_group_forgets_what_a_check_recorded_against_it() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.record_check("work", Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

        registry.forget_group("work");

        assert_eq!(registry.checked("work"), None);
    }

    #[test]
    fn the_version_is_recorded_so_an_older_build_can_refuse_the_file() {
        let json = serde_json::to_string(&Registry::default()).unwrap();
        assert!(json.contains(&format!("\"version\":{CURRENT_VERSION}")));
    }

    #[test]
    fn a_healthy_account_records_no_quarantine_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("quarantine"), "{json}");

        assert!(registry.quarantine("someone@example.com", Quarantine::RotationLost));
        assert!(
            !registry.quarantine("someone@example.com", Quarantine::NoCredential),
            "the reason kept is how it broke, not the last thing that could not \
             be done to it since"
        );
        let mut back: Registry =
            serde_json::from_str(&serde_json::to_string(&registry).unwrap()).unwrap();
        assert_eq!(
            back.release("someone@example.com"),
            Some(Quarantine::RotationLost),
            "the reason survives the round trip, and a login is what ends it"
        );
        assert!(!back.account("someone@example.com").unwrap().quarantined());
    }

    #[test]
    fn an_account_nobody_has_disabled_records_no_disable_at_all() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        assert!(!json.contains("disabled"), "{json}");

        registry
            .account_mut("someone@example.com")
            .unwrap()
            .disabled = true;
        let written = serde_json::to_string(&registry).unwrap();
        assert!(written.contains(r#""disabled":true"#), "{written}");

        let back: Registry = serde_json::from_str(&written).unwrap();
        assert!(
            back.account("someone@example.com").unwrap().disabled,
            "and it survives the round trip, because it is the half that was said"
        );
    }

    /// `enabled` is `disabled` spelled the other way round.
    #[test]
    fn a_registry_that_still_says_enabled_is_refused_rather_than_read() {
        let held: std::result::Result<Registry, _> = serde_json::from_str(
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"enabled":true}]}"#,
        );

        assert!(held.is_err(), "{held:?}");
    }

    #[test]
    fn nothing_about_where_a_credential_is_kept_is_written_down() {
        let mut registry = Registry::default();
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        });

        let json = serde_json::to_string(&registry).unwrap();
        for derived in ["keychain_service", "keychain_account", "profile", "dir"] {
            assert!(
                !json.contains(derived),
                "a registry that records `{derived}` can disagree with the \
                 derivation it restates: {json}"
            );
        }
    }

    #[test]
    fn a_group_carries_its_configuration_through_json() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry.groups.get_mut("work").unwrap().strategy = Strategy::SoonestReset;

        let json = serde_json::to_string(&registry).unwrap();
        assert!(json.contains("soonest-reset"), "{json}");
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, registry);
    }

    #[test]
    fn cycling_among_ungrouped_accounts_is_off_until_it_is_asked_for() {
        assert!(!Registry::default().ungrouped.interchangeable);
        let says_nothing_about_it: Registry =
            serde_json::from_str(&format!("{{\"version\":{CURRENT_VERSION}}}"))
                .expect("a registry with no settings in it");
        assert!(
            !says_nothing_about_it.ungrouped.interchangeable,
            "a registry that says nothing about it reads as off, not as a \
             declaration nobody made (ADR a-group-is-a-declaration)"
        );
    }

    #[test]
    fn a_new_group_holds_the_defaults_and_leaves_the_watcher_alone() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        let work = Scope::Group("work".to_string());

        let settings = registry.settings(&work);
        assert!(!settings.watcher_may_act);
        assert_eq!(settings.strategy, Strategy::MostHeadroom);
        assert_eq!(
            settings.watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT
        );

        registry.ungrouped.settings.watcher_threshold_percent = 55;
        assert_eq!(
            registry.settings(&work).watcher_threshold_percent,
            DEFAULT_WATCHER_THRESHOLD_PERCENT,
            "a Setting said about one Scope is said about that Scope: there is \
             no layer for it to arrive at another by"
        );
    }

    #[test]
    fn a_group_declared_later_is_not_reached_by_a_grant_made_earlier() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        registry
            .settings_mut(&Scope::Group("work".to_string()))
            .expect("declared")
            .watcher_may_act = true;
        registry.ungrouped.settings.watcher_may_act = true;

        registry.declare_group("personal").unwrap();

        assert!(
            !registry
                .settings(&Scope::Group("personal".to_string()))
                .watcher_may_act,
            "a Group nobody has said anything about is not one the watcher may \
             act on"
        );
    }

    /// Asserted as the number rather than as the constant, because a default is
    /// a promise made in the docs and a test reading the constant back cannot
    /// notice it change.
    #[test]
    fn the_watchers_policy_has_the_default_it_is_documented_with() {
        assert_eq!(Settings::default().watcher_threshold_percent, 80);
    }

    #[test]
    fn a_number_out_of_range_is_refused_with_the_range() {
        let cases: [(Settings, &str, &str); 1] = [(
            Settings {
                watcher_threshold_percent: 101,
                ..Settings::default()
            },
            "watcher-threshold-percent",
            "100",
        )];

        let work = Scope::Group("work".to_string());
        for (config, key, accepted) in cases {
            let refusal = config.validate(&work).expect_err("out of range");
            let message = refusal.to_string();
            assert_eq!(refusal.exit_code(), crate::error::EXIT_INVALID, "{message}");
            assert!(message.contains("work"), "{message}");
            assert!(message.contains(key), "{message}");
            assert!(
                message.contains(accepted),
                "a refusal that does not say what would be accepted leaves the \
                 script to guess twice: {message}"
            );
        }

        assert!(Settings::default().validate(&work).is_ok());
        assert!(Settings::default().validate(&Scope::Ungrouped).is_ok());
    }

    #[test]
    fn a_name_that_would_be_ambiguous_is_refused_whichever_half_it_is_for() {
        for kind in [NameKind::Alias, NameKind::Group] {
            for name in [
                "",
                " ",
                "none",
                "None",
                // A Group called `ungrouped` is one no `perch config set` could
                // reach; an Alias is the same collision from the other side.
                "ungrouped",
                "Ungrouped",
                " work",
                "work ",
                // Not only at the ends: a `perch config get` line is read back
                // a word at a time, so no line of it could name this.
                "my work",
                "Overflow Ltd",
                "two\twords",
                "someone@example.com",
                // The word people mean every Scope at once by. There is no such
                // Scope, so a Group taking the name would take every
                // `perch config set global …` quietly.
                "global",
                "Global",
                // Spelled like a flag: `perch run`'s program goes after `--`,
                // so this is a Target that command can never be given.
                "-dev",
                "--work",
                "-",
            ] {
                let refused = validate_name(kind, name)
                    .expect_err(&format!("`{name}` should not be usable as a {kind:?} name"));
                // And the refusal states the rule that was broken rather than
                // one about the other half of the namespace.
                assert!(
                    refused.to_string().contains(kind.names())
                        || refused.to_string().contains(kind.article()),
                    "a {kind:?} refused `{name}` in words about something else: {refused}"
                );
            }
            for name in ["work", "overflow-ltd", "personal-2"] {
                assert!(validate_name(kind, name).is_ok(), "`{name}` should be fine");
            }
        }
    }

    /// The allow-list, on both halves of the namespace, and the refusal naming
    /// the character it turned on rather than the rule in the abstract.
    #[test]
    fn a_name_is_made_of_identifier_characters_in_whatever_alphabet() {
        for kind in [NameKind::Alias, NameKind::Group] {
            for name in [
                "dev",
                "my-group",
                "my_group",
                "_dev",
                "v2",
                // A digit opens a name, which `XID_Start` alone would refuse.
                "2fa",
                // Every alphabet, which is the whole of what XID buys over
                // ASCII: the person naming a Group is naming it for themselves.
                "café",
                "日本",
                "дом",
                "한국",
                "العربية",
                "日本-dev",
            ] {
                assert!(
                    validate_name(kind, name).is_ok(),
                    "`{name}` should be usable as {} name",
                    kind.article()
                );
            }

            for (name, said) in [
                ("🚀", "U+1F680"),
                ("dev★", "U+2605"),
                ("dev.ops", "U+002E"),
                ("dev+1", "U+002B"),
                ("dev/qa", "U+002F"),
                ("-dev", "U+002D"),
                ("dev ops", "U+0020"),
                ("dev@x", "U+0040"),
                // A combining mark may follow a letter and may not open a name:
                // there would be nothing for it to combine with but the prompt.
                ("\u{301}dev", "U+0301"),
                // The two XID does not answer. Both are well-formed identifiers
                // and both draw as `dev`, which is the other rule's to refuse.
                ("dev\u{FE00}", "U+FE00"),
                ("dev\u{3164}", "U+3164"),
            ] {
                let refused = validate_name(kind, name)
                    .expect_err(&format!("`{name}` should not be usable as a {kind:?} name"));
                assert!(
                    refused.to_string().contains(said),
                    "a {kind:?} refused `{name}` without naming {said}: {refused}"
                );
            }
        }
    }

    #[test]
    fn naming_an_account_that_is_already_named_replaces_the_name() {
        let mut registry = Registry::default();
        registry
            .name_account("overflow", "someone@example.com")
            .expect("the name is free");
        registry
            .name_account("work", "someone@example.com")
            .expect("the Account renames itself");

        assert_eq!(registry.alias_of("someone@example.com"), Some("work"));
        assert!(!registry.aliases.contains_key("overflow"));
        assert_eq!(
            registry.unset_alias("Work"),
            Some(("work".to_string(), "someone@example.com".to_string())),
            "a name is freed however it is capitalized, and says how it was held"
        );
        assert!(registry.unset_alias("work").is_none());
    }

    #[test]
    fn a_group_is_declared_once() {
        let mut registry = Registry::default();
        registry.declare_group("work").unwrap();
        assert!(registry.declare_group("work").is_err());
        registry
            .ensure_group("work")
            .expect("naming it again in passing is not a conflict");
    }

    /// A script branching on why an Account is Quarantined branches on this
    /// string and nothing else, so every kind needs one and no two may share it.
    #[test]
    fn every_quarantine_has_its_own_machine_readable_name() {
        let every = [
            Quarantine::RenewalRejected,
            Quarantine::RotationLost,
            Quarantine::NoRefreshToken,
            Quarantine::NoCredential,
        ];

        let named: Vec<&str> = every.iter().map(Quarantine::as_str).collect();
        assert_eq!(
            named,
            [
                "renewal-rejected",
                "rotation-lost",
                "no-refresh-token",
                "no-credential"
            ]
        );

        let mut unique = named.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), named.len(), "two Quarantines share a name");

        for why in every {
            assert!(
                !why.because().is_empty(),
                "{why:?} says nothing about itself"
            );
        }
    }

    #[test]
    fn perch_home_is_taken_from_the_environment_verbatim_when_it_is_set() {
        let host = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("PERCH_HOME", "/tmp/somewhere-else");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else")
        );
        assert_eq!(
            registry_path(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else/registry.json"),
            "and everything under it moves with it"
        );
    }

    #[test]
    fn without_the_override_perch_keeps_its_registry_under_the_config_directory() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/Users/someone/.config/perch")
        );
    }

    /// `export PERCH_HOME=$SOMETHING_UNSET` is the ordinary way to arrive here,
    /// and a relative registry path is the Holdings following the working
    /// directory around.
    #[test]
    fn a_perch_home_set_to_nothing_is_the_machine_not_saying_rather_than_the_working_directory() {
        let host = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("PERCH_HOME", "");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/Users/someone/.config/perch")
        );
    }

    #[test]
    fn a_registry_that_cannot_be_read_is_a_failure_rather_than_an_empty_perch() {
        let absent = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        assert_eq!(
            load(&absent).expect("a machine Perch has never run on holds nothing"),
            None
        );

        let path = registry_path(&absent).unwrap();
        let unreadable = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_file(&path, "{}")
            .with_unreadable_file(&path, "permission denied");

        let failed = load(&unreadable).expect_err("a registry that is there must be readable");
        let said = failed.to_string();
        assert!(said.contains("permission denied"), "{said}");
        assert!(
            said.contains(&path.display().to_string()),
            "and it names the file: {said}"
        );
    }

    #[test]
    fn a_groups_settings_are_found_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("Work").expect("a usable name");

        assert!(registry.group("work").is_some());
        assert!(registry.group("WORK").is_some());
        assert!(registry.group("play").is_none());
    }

    #[test]
    fn every_question_about_a_group_is_answered_however_the_name_was_capitalized() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.upsert(Account {
            identity: Identity {
                email: "someone@example.com".into(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: Some("work".to_string()),
            utilization: None,
        });

        registry
            .settings_mut(&Scope::Group("WORK".to_string()))
            .expect("the Group is declared, whatever it was typed as")
            .watcher_threshold_percent = 65;
        assert_eq!(
            registry
                .settings(&Scope::Group("Work".to_string()))
                .watcher_threshold_percent,
            65,
            "what was written is what is read back"
        );
        assert_eq!(registry.accounts_in("WORK").len(), 1);

        let at = Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap();
        registry.record_check("WORK", at);
        assert!(
            registry.checked("work").is_some(),
            "one Cooldown record, under the spelling the Group was declared as"
        );

        registry.rename_group("WORK", "office").expect("it renames");
        assert!(registry.group("work").is_none());
        assert_eq!(registry.accounts_in("office").len(), 1);
        assert!(
            registry.checked("office").is_some(),
            "the Cooldown came too"
        );

        registry.forget_group("OFFICE");
        assert!(registry.group("office").is_none());
        assert!(registry.checked("office").is_none());
        // And forgetting one nothing declared is nothing to do.
        registry.forget_group("office");
    }

    #[test]
    fn renaming_a_group_nothing_declared_is_refused_rather_than_panicked_on() {
        let mut registry = Registry::default();

        let error = registry
            .rename_group("work", "office")
            .expect_err("there is no such Group");

        assert!(error.to_string().contains("work"), "{error}");
    }

    #[test]
    fn a_group_an_account_claims_is_declared_by_the_time_anything_reads_it() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"work"}],"groups":{}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        let declared: Vec<&str> = registry.group_names().collect();
        assert!(
            declared.contains(&"work"),
            "the Group the Account claims is in the declared set: {declared:?}"
        );
        assert_eq!(
            registry.group("work"),
            Some(&Settings::default()),
            "carrying what a freshly declared Group carries"
        );
        assert_eq!(registry.accounts_in("work").len(), 1);
    }

    /// The `checks` rule asks `declared_group`, so it has to be asked after the
    /// claim is declared and not before. Judged the other way round it refused
    /// every command on the file, `holdings purge` among them.
    #[test]
    fn a_check_against_a_group_only_an_account_claims_is_read_rather_than_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"work"}],"groups":{},"checks":{"work":{"switched_at":"2026-01-01T00:00:00Z"}}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        assert!(
            registry.checked("work").is_some(),
            "the Check is still there"
        );
    }

    /// The other half: normalizing first must not make the rule toothless.
    #[test]
    fn a_check_against_a_group_nobody_claims_is_still_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"}}],"groups":{},"checks":{"ghost":{"switched_at":"2026-01-01T00:00:00Z"}}}"#,
        );

        let refused = load(&host).expect_err("that Cooldown paces nothing");

        assert!(refused.to_string().contains("ghost"), "{refused}");
    }

    #[test]
    fn a_group_an_account_claims_in_another_case_joins_the_one_that_is_declared() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"group":"Work"}],"groups":{"work":{"watcher_threshold_percent":65}}}"#,
        );

        let registry = load(&host).expect("it reads").expect("it is there");

        let declared: Vec<&str> = registry.group_names().collect();
        assert_eq!(declared.len(), 1, "one Group, not two: {declared:?}");
        assert_eq!(
            registry.accounts_in("work").len(),
            1,
            "and the Account is in it, rather than in a namesake beside it"
        );
        assert_eq!(
            registry.group("work").unwrap().watcher_threshold_percent,
            65,
            "the declared Group keeps the policy it was declared with"
        );
    }

    #[test]
    fn a_claim_declare_group_would_have_refused_is_named_rather_than_declared() {
        let claims = [
            (r#""none""#, "{}", "addresses the Accounts in no Group"),
            (r#""my work""#, "{}", "carries ` ` (U+0020)"),
            (r#""overflow""#, r#"{}"#, "already an Alias"),
        ];

        for (claimed, groups, expected) in claims {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            let aliases = if expected == "already an Alias" {
                r#","aliases":{"overflow":"someone@example.com"}"#
            } else {
                ""
            };
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}},"group":{claimed}}}],"groups":{groups}{aliases}}}"#
                ),
            );

            let refused = load(&host).expect_err("that is not a Group name");
            let said = refused.to_string();
            assert!(
                said.contains(expected),
                "`{claimed}` should be refused for `{expected}`: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal names the file, because every command reads it \
                 — including the ones that would set this: {said}"
            );
        }
    }

    #[test]
    fn a_declared_group_or_an_alias_nothing_would_have_accepted_is_named_too() {
        let holdings = [
            (r#""groups":{"my work":{}}"#, "carries ` ` (U+0020)"),
            (
                r#""groups":{"none":{}}"#,
                "addresses the Accounts in no Group",
            ),
            (
                r#""aliases":{"someone@example.com":"other@example.com"}"#,
                "carries `@` (U+0040)",
            ),
            (
                r#""groups":{"work":{}},"aliases":{"work":"someone@example.com"}"#,
                "share one namespace",
            ),
            (
                r#""groups":{"\u001b[31mred":{}}"#,
                "a control character (U+001B)",
            ),
        ];

        for (held, expected) in holdings {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            host.set_file(&path, &format!(r#"{{"version":2,"accounts":[],{held}}}"#));

            let refused = load(&host).expect_err("that is not a name Perch would have given");
            let said = refused.to_string();
            assert!(
                said.contains(expected),
                "`{held}` should be refused for `{expected}`: {said}"
            );
            // These registries hold no Accounts at all, so a refusal explaining
            // itself in terms of what an Account can be in says nothing.
            assert!(
                !said.contains("an Account cannot be in it"),
                "a declared Group is refused in words about the name rather \
                 than about Accounts this registry does not hold: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal names the file to edit: {said}"
            );
        }
    }

    /// `add` picks a different question depending on whether there is an offer,
    /// so a `None` that stopped being one would put a name in front of somebody
    /// that `validate_name` refuses a keystroke later — after the browser round
    /// trip has been spent.
    #[test]
    fn a_group_name_is_offered_only_where_it_is_one_perch_would_accept() {
        assert_eq!(
            offerable_name("Overflow Ltd").as_deref(),
            Some("Overflow-Ltd"),
            "the spaces are what is wrong with it, and nothing else is touched \
             — Group names are compared case-insensitively, so there is nothing \
             to gain by rewriting how somebody's organization spells itself"
        );
        assert_eq!(
            offerable_name("  Overflow   Ltd  ").as_deref(),
            Some("Overflow-Ltd"),
            "whitespace is what is being fixed, wherever it is"
        );
        assert_eq!(offerable_name("Acme").as_deref(), Some("Acme"));

        for refused in ["none", "None", "NONE", "someone@example.com", "   ", ""] {
            assert_eq!(
                offerable_name(refused),
                None,
                "`{refused}` is not a name Perch would accept, so it is not one \
                 to offer either"
            );
        }
    }

    /// Both files parse as JSON perfectly well — a Strategy this build has no
    /// variant for, and a missing `version`. Told "not valid JSON", somebody goes
    /// looking for a syntax error that is not there.
    #[test]
    fn a_registry_that_is_json_and_still_unreadable_is_not_called_bad_json() {
        let files = [
            r#"{"version":2,"accounts":[],"groups":{"work":{"strategy":"round-robin"}}}"#,
            r#"{"accounts":[],"groups":{}}"#,
        ];

        for contents in files {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            host.set_file(&path, contents);

            let refused = load(&host).expect_err("this build cannot read it");
            let said = refused.to_string();
            assert!(
                !said.contains("not valid JSON"),
                "the file parses as JSON perfectly well: {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the refusal still names it: {said}"
            );
        }
    }

    #[test]
    fn a_percentage_that_is_not_one_is_refused_rather_than_ranked_on() {
        for figure in ["-50", "150"] {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}},"utilization":{{"observed_at":"2025-01-01T00:00:00Z","windows":[{{"window":"5-hour","used_percent":{figure}}}]}}}}],"groups":{{}}}}"#
                ),
            );

            let refused = load(&host).expect_err("that is not a percentage");
            assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
            let said = refused.to_string();
            assert!(
                said.contains("5-hour") && said.contains("someone@example.com"),
                "`{figure}` should be refused naming the window and the Account: \
                 {said}"
            );
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }

        // And the ends of the range are figures, not refusals: a window that has
        // just reset and one that is completely spent are both ordinary.
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"},"utilization":{"observed_at":"2025-01-01T00:00:00Z","windows":[{"window":"5-hour","used_percent":0},{"window":"7-day","used_percent":100}]}}],"groups":{}}"#,
        );
        load(&host).expect("0 and 100 are both percentages");
    }

    #[test]
    fn an_alias_for_an_account_perch_does_not_hold_is_refused_and_names_both() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"someone@example.com"}}],"aliases":{"overflow":"gone@example.com"}}"#,
        );

        let refused = load(&host).expect_err("the Alias names nobody");

        assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
        let said = refused.to_string();
        assert!(said.contains("overflow"), "which Alias: {said}");
        assert!(
            said.contains("gone@example.com"),
            "and who it names: {said}"
        );
        assert!(
            said.contains("registry.json"),
            "and the file to edit: {said}"
        );
    }

    #[test]
    fn two_names_in_one_half_of_the_namespace_that_differ_only_in_case_are_refused() {
        let holdings = [
            r#""groups":{"work":{},"Work":{}}"#,
            // Two Accounts, one Alias each: giving both names to one Account is
            // a different refusal.
            r#""aliases":{"work":"someone@example.com","Work":"other@example.com"}"#,
        ];

        for held in holdings {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(
                    r#"{{"version":2,"accounts":[{{"identity":{{"email":"someone@example.com"}}}},{{"identity":{{"email":"other@example.com"}}}}],{held}}}"#
                ),
            );

            let refused = load(&host).expect_err("which of the two a Target finds is undecided");
            let said = refused.to_string();
            assert!(said.contains("differ only in case"), "`{held}`: {said}");
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }
    }

    #[test]
    fn an_account_address_a_name_could_be_confused_with_is_refused() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"work"}},{"identity":{"email":"real@example.com"}}],"groups":{"work":{}}}"#,
        );

        let refused = load(&host).expect_err("that Target has two answers");
        let said = refused.to_string();
        assert!(said.contains("Account called `work`"), "{said}");
        assert!(
            said.contains("could be told from"),
            "it says what is wrong with it: {said}"
        );
        assert!(
            said.contains("registry.json"),
            "and the file to edit: {said}"
        );
    }

    /// An address is Claude Code's rather than anybody's choice, so it is
    /// refused where it enters and drawn stripped. Refused here it would be
    /// refused at `load`, which takes every command with it — including
    /// `perch remove`, which is the only way such an Account could ever go.
    #[test]
    fn an_account_address_a_terminal_would_obey_reads_rather_than_bricking() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[{"identity":{"email":"bad\u001brow@example.com"}}]}"#,
        );

        let registry = load(&host)
            .expect("a registry a published Perch wrote is readable")
            .expect("it is there");
        assert_eq!(registry.accounts.len(), 1);
    }

    #[test]
    fn one_account_reached_twice_over_is_refused() {
        let cases = [
            (
                r#""aliases":{"spare":"someone@example.com","work":"someone@example.com"}"#,
                "one Alias at a time",
            ),
            (
                r#""aliases":{}"#,
                // Two entries for one address, spelled differently.
                "which are one Account",
            ),
        ];

        for (index, (held, expected)) in cases.iter().enumerate() {
            let accounts = if index == 0 {
                r#"[{"identity":{"email":"someone@example.com"}}]"#
            } else {
                r#"[{"identity":{"email":"someone@example.com"}},{"identity":{"email":"SOMEONE@example.com"}}]"#
            };
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let path = registry_path(&host).unwrap();
            host.set_file(
                &path,
                &format!(r#"{{"version":2,"accounts":{accounts},{held}}}"#),
            );

            let refused = load(&host).expect_err("one Account reached twice over");
            let said = refused.to_string();
            assert!(said.contains(expected), "`{held}`: {said}");
            assert!(
                said.contains("registry.json"),
                "and the file to edit: {said}"
            );
        }
    }

    #[test]
    fn a_number_out_of_range_in_the_file_is_refused_by_the_read_and_names_the_file() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let path = registry_path(&host).unwrap();
        host.set_file(
            &path,
            r#"{"version":2,"accounts":[],"groups":{"work":{"watcher_threshold_percent":101}}}"#,
        );

        let refused = load(&host).expect_err("101 is not a percentage");

        assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
        let said = refused.to_string();
        assert!(said.contains("work"), "which Group: {said}");
        assert!(
            said.contains("watcher-threshold-percent"),
            "which setting, spelled the way it is set: {said}"
        );
        assert!(
            said.contains(&path.display().to_string()),
            "and where to go and change it, because no command can: {said}"
        );
    }

    #[test]
    fn a_registry_this_build_writes_says_so() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut perch = lock(&host).expect("the registry lock is free");
        let mut stale = Registry {
            version: 0,
            ..Registry::default()
        };

        save(&host, &mut perch, &mut stale).expect("it writes");

        assert_eq!(
            load(&host).expect("it reads").expect("it is there").version,
            CURRENT_VERSION
        );
    }
}
