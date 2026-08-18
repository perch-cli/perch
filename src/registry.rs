//! Perch's own state: the Accounts it holds, the Profile each one lives in,
//! and which Account is active.
//!
//! Versioned, so that a registry written by a build that understands more than
//! this one is refused rather than silently misread. The version is a guard
//! against the future and not a migration story: nobody is running Perch yet,
//! so there is no past format to read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::lock;
use crate::probe::{Identity, LockSpec};

/// The version this build writes. A registry from the future is refused rather
/// than silently misread; one from the past is not read at all, because nobody
/// is running Perch yet and there is nothing to migrate.
///
/// Moved to `2` when every Scope came to hold its own Settings (ADR 0051):
/// `groups` stopped being a map of Overrides, `ungrouped` absorbed the record
/// that was `global`, and `cycle_ungrouped` became `interchangeable`. The guard
/// that refuses a newer registry is only worth having if the number moves when
/// the shape does.
pub const CURRENT_VERSION: u32 = 2;

/// One Quota Window's Utilization, as observed at a point in time (ADR 0015).
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
/// only a `--refresh` ever goes and fetches (ADR 0015).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedUtilization {
    pub observed_at: DateTime<Utc>,
    pub windows: Vec<WindowUtilization>,
}

/// Why an Account's Credential can no longer be used and cannot be recovered
/// from anything Perch holds.
///
/// Recorded rather than merely counted, because "this Account is broken" and
/// "this Account is broken because Anthropic retired its refresh token" are
/// different pieces of news: the first leaves the user guessing whether Perch
/// lost something, and the second says what happened and implies the repair.
/// Every one of these is terminal — none of them can be undone by trying again,
/// which is exactly what makes it a Quarantine rather than a failed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quarantine {
    /// Anthropic turned the refresh token down — retired, revoked, or belonging
    /// to a login that has been ended elsewhere.
    RenewalRejected,
    /// Anthropic Rotated the refresh token and the new one could not be stored,
    /// so the old one is retired and the new one is gone: ADR 0006's crash
    /// between two writes, arriving as a failed write instead of a crash.
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

    /// The whole of what is said about a Quarantined Account wherever one is
    /// shown as broken: which Account, what happened to it, and the one command
    /// that ends it.
    ///
    /// Said in one place because every surface owes the same three things. Two
    /// surfaces spelling this out separately would eventually offer two
    /// different repairs for one state.
    ///
    /// `detail` is whatever the failure underneath said, where there was one
    /// worth keeping — a keychain that would not take the Rotated Credential,
    /// say. The reason is what happened; the detail is how.
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

    /// The refusal a command raises rather than acting on a Quarantined
    /// Account, as opposed to [`said_of`](Quarantine::said_of), which is how one
    /// is *shown*.
    ///
    /// `consequence` is the caller's, and it is the only part that differs:
    /// what a Switch would have cost is not what a Run would have. Everything
    /// around it is shared, so the third command to refuse a Quarantine cannot
    /// come to offer a different repair from the first two.
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

    /// The same as a script reads it. Absent reads as false and present reads
    /// as true wherever a script asks whether it is set, so the fact a script
    /// already branches on branches the same way — and now carries why.
    pub fn document(quarantine: Option<Quarantine>) -> serde_json::Value {
        match quarantine {
            Some(why) => {
                serde_json::json!({"reason": why.as_str(), "detail": why.because()})
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
    /// Said only when it is true, for the reason the Quarantine below gives:
    /// the registry is something a person may open, and an Account nobody has
    /// done anything to reads more clearly for saying nothing. The positive
    /// state has no name to write down — it is the absence of this one
    /// (ADR 0052).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Why this Account's Credential can no longer be used, when it cannot.
    ///
    /// Absent is the ordinary case, and is left out of the file entirely rather
    /// than written as a null: the registry is something a person may open, and
    /// a healthy Account reads more clearly for saying nothing about its health.
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
/// Account has (ADR 0012) — and differ only in what they do with it. The
/// measurement is fixed and the Strategy is a separate axis on top of it, so
/// neither reading is a way round an exhausted Account.
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
/// `100` was written out in the sentence below and twice in the range check —
/// so what `perch config set` accepts and what a hand-edited registry is
/// refused for were two statements of one number.
///
/// Not the bound on a Utilization figure, which `validate` checks separately
/// and `anthropic::understand` clamps to. That one is what a *reading* may be;
/// this is what a Setting may be *set to*, which is a decision — a Perch that
/// declined to let the watcher act above ninety would move this and must not
/// move that. They are the same number today and two facts always.
pub const MAX_PERCENTAGE: u8 = 100;

/// What a percentage accepts, said once so that the refusal a mistyped `perch
/// config set` gets and the one a hand-edited registry gets are the same words.
///
/// Built from the bound rather than written out beside it, so the sentence and
/// the number it describes cannot come to disagree.
pub fn a_percentage() -> String {
    format!("a whole number between 0 and {MAX_PERCENTAGE}")
}

/// Every Setting there is, all of them set: what one Scope holds (ADR 0051).
///
/// A Scope — each Group, and the Accounts in no Group taken together — holds
/// its own full set. There is nothing above it for a value to fall back to, so
/// a Setting nobody has said anything about is the compiled-in default rather
/// than somebody else's value.
///
/// Two of these are the watcher's, and only one of the two is a pace: how full
/// is too full, which is the single question in the loop that a person's
/// appetite for risk answers rather than arithmetic (ADR 0046). The numbers the
/// loop paces itself by are not among them — the interval it Refreshes at, the
/// cooldown between two Switches and the margin under where one may land are
/// all derived rather than preferred, and live in [`crate::watch`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub strategy: Strategy,
    /// Whether the watcher may Switch within this Scope unattended. Off unless
    /// the user says otherwise: nothing changes underneath someone because they
    /// did not say it could (ADR 0002).
    ///
    /// Said about the Scope it grants and nowhere else (ADR 0051). A grant that
    /// reached a Scope by falling through from somewhere wider would authorize
    /// Groups nobody had said anything about — including ones not yet declared.
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
    /// Refuses configuration that cannot mean what it says. Serde already
    /// refuses a strategy Perch does not implement, and a `true`/`false` that
    /// is neither; what is left is the range the one number has to be in.
    ///
    /// The refusal names the numbers that would have been accepted, because
    /// the script that mistyped one is the reader, and being told only that it
    /// was wrong leaves it to guess twice.
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
/// The only levels at which a Setting means anything. One type for both ideas,
/// because they are one idea: a Cycle never leaves the Scope it started in
/// (ADR 0002), and a Setting is said about exactly the Scope it governs
/// (ADR 0051). They were two types while a Config had a layer above every
/// Scope, on the grounds that sharing one would put "every Account there is"
/// within reach of the ranking. There is no such value to be handed any more,
/// so that is a mistake nobody can make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The Accounts in no Group, taken as one Scope (ADR 0017, amended). Not a
    /// Group and never one: a Group is a declaration somebody made, and this is
    /// the absence of one.
    Ungrouped,
    /// One Group, named as it was declared.
    Group(String),
}

impl Scope {
    /// The word that addresses this Scope on a command line, and the word it is
    /// recorded under wherever something is kept per Scope — what the last
    /// scheduled Check did, for one.
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
    /// The same set whoever is asking: what a Cycle may land on, and what a
    /// Scope has left to draw on ([`crate::reserve`]) is measured over. A second
    /// idea of which Accounts those are is how the figure on screen comes to
    /// describe a different set from the one that gets chosen.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Ungrouped => registry.ungrouped_accounts(),
            Scope::Group(name) => registry.accounts_in(name),
        }
    }

    /// The Scope as an adverbial phrase: "a Cycle {} prefers…", "Nothing {} is
    /// worth Switching to". Said once here rather than per surface, because
    /// three spellings of "among the Accounts in no Group" is how two of them
    /// come to name the same set differently.
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

    /// The Scope as the middle of a sentence about the Accounts in it: "every
    /// Account in {}".
    ///
    /// Not [`Scope::described`], which is a subject and reads as "The Ungrouped
    /// Scope" — true standing alone and ungrammatical the moment "in" is said
    /// before it.
    pub fn place(&self) -> String {
        match self {
            Scope::Ungrouped => "no Group".to_string(),
            Scope::Group(_) => self.described(),
        }
    }

    /// What a Cycle is about to do, said before it does it.
    ///
    /// Built from [`Scope::within`] rather than spelled out arm for arm, which
    /// is that method's own argument applied to itself: it was the same two
    /// phrases with "Cycling" in front and a full stop after, in a second match
    /// that could come to disagree with the first.
    pub fn announcement(&self) -> String {
        format!("Cycling {}.", self.within())
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
/// There is no such Scope: every Setting is said about the one Scope it governs
/// and there is nothing above them (ADR 0051). So unlike [`UNGROUPED`] this
/// word addresses nothing, and a Group may not take it — `perch config set
/// global watcher-may-act true` is somebody saying *everywhere*, and a Group
/// answering to the name would take that quietly and leave every other Scope as
/// it was. Kept reserved so the refusal is where they find out Perch has no
/// everywhere-layer, which is a better place to learn it than from a Setting
/// that appeared to take.
pub const GLOBAL: &str = "global";

/// Whether a name is the one people mean every Scope at once by.
pub fn means_global(name: &str) -> bool {
    same_name(name, GLOBAL)
}

/// The Switch a scheduled Check made within a Group, kept so the next one can
/// be paced by it (ADR 0013).
///
/// The one thing about the watcher that is written down, and only because
/// `perch watcher check` is a fresh process every time: the cooldown is
/// measured from the last Switch, and a check that could not remember one would
/// be a check with no policy but the threshold. The loop carries the same fact
/// in memory and records nothing, because a loop is one process and a person
/// watching it — two of them would otherwise pace each other's decisions.
///
/// Per Group rather than per machine: a cooldown paces the Switches made within
/// a Group, and a Switch within `work` has nothing to say about how soon
/// `personal` may move. A constant still has to be paced somewhere (ADR 0046).
///
/// A stamp and nothing else. Which Account was Switched off was recorded here
/// too, for the no-return that read it — and when that went (ADR 0046) the field
/// was left behind, written every Check and read by nothing. It is not kept
/// against the day ADR 0046's guard fires and a no-return comes back: breaking
/// the registry's format is free (`CLAUDE.md`), so the day something needs to
/// know which Account was left is the day to record it again.
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
/// Scope that has to say it is a Scope at all. A Group carries no such line: a
/// Group **is** that declaration (ADR 0002), and printing one against it would
/// be a line `perch config set` could not take back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UngroupedConfig {
    /// Whether the Accounts in no Group have been declared interchangeable —
    /// what a bare `perch switch` and the watcher both need before either may
    /// move between them.
    ///
    /// Off unless the user says otherwise, and deliberately so: being ungrouped
    /// is the absence of a declaration that Accounts are interchangeable, not a
    /// weaker form of one. Cycling freely here would move someone from their
    /// work subscription onto their personal one without their ever having said
    /// the two were substitutable (ADR 0017).
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
/// Case-insensitively, because nobody remembers which way they capitalized a
/// Group months ago — and over the whole of Unicode, because
/// [`validate_name`] accepts the whole of Unicode. Comparing only ASCII would
/// make `café` and `CAFÉ` two different Groups while making `work` and `Work`
/// one, which is the ambiguity the rule exists to prevent, kept for exactly
/// the users whose language has accents in it.
pub fn same_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

/// A Group name offered as a default, made from something that was never
/// chosen to be one — an organization name, which is whatever Anthropic holds
/// and commonly has spaces in it.
///
/// Only the spaces are touched, and only into the separator the names people
/// pick already use. Anything else wrong with it — an `@`, or `none` — leaves
/// no offer at all, because a suggestion is a convenience and inventing a name
/// around a refusal is not.
pub fn offerable_name(from: &str) -> Option<String> {
    let joined = from.split_whitespace().collect::<Vec<_>>().join("-");
    validate_name(NameKind::Group, &joined).ok()?;
    Some(joined)
}

/// Refuses a name that could not be told from something else.
///
/// Aliases and Group names share one namespace and are both valid Targets for
/// `switch` and `run`, so a name has to be distinguishable from the other
/// things a Target can be: an email address, and the word that means no Group
/// at all.
pub fn validate_name(kind: NameKind, name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(PerchError::Invalid(format!(
            "{} cannot be empty.",
            kind.names()
        )));
    }
    // Any whitespace, not only at the ends. `perch config get` prints each
    // setting as the tail of the `perch config set` that would restore it —
    // `<group> <key> <value>`, read back by counting words — so a Group with a
    // space in its name prints a line that cannot be typed back in. A name that
    // breaks the round trip the output promises is worth refusing at the one
    // moment somebody can still choose another.
    if name.chars().any(char::is_whitespace) {
        return Err(PerchError::Invalid(format!(
            "`{name}` has a space in it. {} are read back a word at a time — \
             `perch config get` prints settings as the `perch config set` that \
             would restore them — so a name with a space in it is one no line \
             of that output could name.",
            kind.names()
        )));
    }
    // Both of the words that already address something are refused for either
    // half of the namespace, and both say which half they were asked about.
    // They were the two refusals in this function that did not: an Alias called
    // `ungrouped` was turned down with "so it cannot also name a Group", which
    // is a rule about something the user was not doing.
    if means_no_group(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` means no Group at all on `perch group move`, so it cannot \
             also be {}.",
            kind.article()
        )));
    }
    // The other word that already addresses something. `perch config` reads
    // which Scope is meant off the words that were typed, so a Group called
    // `ungrouped` would be a Group no `perch config set` could reach — the
    // Ungrouped Scope would answer to the name first. An Alias called
    // `ungrouped` is the same collision from the other side.
    if means_ungrouped(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` addresses the Accounts in no Group on `perch config`, so \
             it cannot also be {}.",
            kind.article()
        )));
    }
    // The third word that already means something, and the one that means
    // something Perch does not have. `perch config set global watcher-may-act
    // true` is somebody saying *everywhere* — and there is no everywhere, since
    // every Setting is said about the one Scope it governs (ADR 0051). Left to
    // fall through it would be answered with "Declare it with `perch group add
    // global`", and a Group by that name would then take every later `perch
    // config set global …` quietly, leaving every other Scope as it was.
    // Refused here so the offer can never be made, and so the refusal is where
    // somebody learns there is no such layer.
    if means_global(name) {
        return Err(PerchError::Invalid(format!(
            "`{name}` is how people say every Scope at once, so it cannot also \
             be {}. There is no such Scope: every Setting is said about the one \
             it governs, and `perch config set <scope> <key> <value>` says it.",
            kind.article()
        )));
    }
    if name.contains('@') {
        return Err(PerchError::Invalid(format!(
            "`{name}` looks like an email address. {} have to be tellable from one, because a Target that could be either has no single answer.",
            kind.names()
        )));
    }
    // Nothing that begins with `-` can name a program, and `perch run` is where
    // that matters: its `command` is `last = true`, so the `--` that rescues
    // such a Target everywhere else is already spoken for. `perch run -dev` is
    // read as flags and `perch run -- -dev` leaves no Target at all, so a name
    // like this is one the registry holds, `perch list` shows, `perch switch`
    // honors — and `perch run` can never be told. Refused at the one moment
    // somebody can still pick another.
    if name.starts_with('-') {
        return Err(PerchError::Invalid(format!(
            "`{name}` begins with `-`, and {} are typed where a flag could go. \
             `perch run` takes the program to launch after `--`, so a Target \
             spelled like a flag is one that command could never be given.",
            kind.names()
        )));
    }
    Ok(())
}

/// Which Account is active — and, while a Switch is under way, that Perch
/// cannot yet say (ADR 0048).
///
/// One field with three states rather than two fields, so a registry naming
/// both a settled active Account and a different in-flight one cannot be
/// written at all. The registry already carries one dangling-pointer check for
/// what this names; a second field would need a second, held by nothing but
/// care.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    /// Written after the Capture and before the Credential moves, so a Perch
    /// that finds one knows the live Credential is one of these two Accounts'
    /// — or a Rotation of one of them — rather than knowing nothing at all.
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
    /// Nothing has been recorded as having moved, so the Account Perch was on
    /// is the last thing it established — and every path that could *lose*
    /// something by believing it settles the Landing first.
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

    /// The Switch that was in flight and never recorded, said out loud, or
    /// `None` on the machines where there was not one.
    ///
    /// Said at all because half of why this hazard survived is that a machine
    /// mid-Landing is indistinguishable from a healthy one, so nobody looks
    /// (ADR 0048). It never changes an exit code: Perch reports what it found
    /// rather than judging it (ADR 0018), and a state the next Switch resolves
    /// by itself should not fail somebody's shell prompt.
    ///
    /// Here rather than in the command, beside [`document`], because two
    /// commands say it and both have to say the same thing — `perch status`
    /// about the Account you are on, and `perch list` about the set it sits in,
    /// at whatever breadth that was asked for (ADR 0053). What it qualifies is
    /// whichever line says which Account is active, and there is one of those
    /// in each.
    ///
    /// [`document`]: Active::document
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
    /// Beside whichever key a document already uses for who is active rather
    /// than folded into it, and the same shape in every document that carries
    /// one. The two answer different questions — *which Account* against
    /// *whether Perch can say* — and a script that only ever wanted the first
    /// should not have to learn what a Landing is to go on getting it (ADR
    /// 0048). Absent reads as false wherever a script asks whether it is set,
    /// which is how [`Quarantine::document`] beside it is read.
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
    /// The active Account, or the Switch that was under way when Perch last
    /// wrote this down.
    ///
    /// Private, and the only field here that is. Every other field is a thing
    /// somebody declared; this one is a thing Perch *did*, and the three states
    /// it moves between are the whole of ADR 0048. A `= Active::Settled(…)`
    /// anywhere is a Switch recorded without having been written down first,
    /// which is precisely the write [`crate::switch::switch_to`] exists to be
    /// the one door for. So it is reached through [`Registry::begin_landing`],
    /// [`Registry::settle`] and [`Registry::abandon_landing`], each of which
    /// names a transition, and read through [`Registry::active`].
    #[serde(default, skip_serializing_if = "Active::is_nobody")]
    active: Active,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Alias to Account email. Empty until aliases land.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// The Groups the user has declared, with the Settings each one holds. A
    /// Group exists here even when it holds no Accounts: it is a statement the
    /// user made, not a summary of where the Accounts happen to be — and it
    /// exists here holding the compiled-in defaults, which is a Group nobody
    /// has said anything about yet.
    #[serde(default)]
    pub groups: BTreeMap<String, Settings>,
    /// What the Accounts in no Group hold, taken as one Scope (ADR 0017,
    /// amended). Not a Group and never one; it is here rather than under a
    /// reserved key in `groups` so that nothing can walk it as one.
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
    /// recorded beside it: two statements of one fact can disagree, and this is
    /// the fact every Credential Store is derived from in turn (ADR 0020).
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
    /// last written (ADR 0048).
    ///
    /// Reading is nobody's to get wrong, so it is open to everybody. Writing is
    /// three named transitions and nothing else.
    pub fn active(&self) -> &Active {
        &self.active
    }

    /// Writes down that a Switch is about to move the live Credential, naming
    /// both Accounts it could then belong to (ADR 0048).
    ///
    /// Hands back what it replaced, because the Landing has to reach disk
    /// before it means anything: a save that fails leaves a caller holding a
    /// registry claiming a Switch is in flight that never started, and this is
    /// what it puts back with [`Registry::abandon_landing`].
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
    /// Not the same transition as [`Registry::settle`] even though it leaves
    /// the same field holding the same kind of thing: this one is a Landing
    /// that never existed anywhere but in memory, and nothing has moved.
    pub fn abandon_landing(&mut self, before: Active) {
        self.active = before;
    }

    /// Records who is active now that a Switch is over — landed, refused, or
    /// resolved afterwards off the live Credential. `None` is a machine on
    /// nobody.
    ///
    /// Being active is a fact about which Credential is in the Default Profile
    /// rather than a wish, so what is passed is who the machine is holding the
    /// Credential of.
    ///
    /// It takes an address rather than an [`Active`], which is what makes
    /// "settled" true of what it leaves behind: handed the enum it would accept
    /// [`Active::Landing`], and a Landing written by anything but
    /// [`Registry::begin_landing`] is a Switch recorded without having been
    /// written down first — the one state ADR 0048 exists to keep impossible,
    /// walking back in through the door built to stop it.
    pub fn settle(&mut self, on: Option<String>) {
        self.active = Active::settled_on(on);
    }

    /// Whether this address is the one the registry records as active.
    ///
    /// One place, and case-folded like every other way the registry is asked
    /// about a name. A dozen call sites spelled this as
    /// `registry.active().whose() == Some(account.email())`, which is the one
    /// comparison in Perch that answered a question about an address by exact
    /// bytes — while [`account`] beside it has always answered the same question
    /// with [`same_name`].
    ///
    /// The two agree only while `active` holds the identical spelling of the
    /// entry it names, which is true today and is nothing that guarantees it:
    /// `upsert` matches an existing entry with `same_name` and stores the
    /// incoming spelling, so an Identity re-read with different capitalization
    /// replaces the Account and leaves `active` naming it the old way. From
    /// there `observe::holding` reads the Account's own Profile instead of the
    /// Default Profile, finds the store empty, and records `NoCredential` — a
    /// permanent Quarantine off a comparison every other part of the registry
    /// makes case-insensitively.
    ///
    /// [`account`]: Registry::account
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

    pub fn group(&self, name: &str) -> Option<&Settings> {
        self.groups.get(name)
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

    /// The Settings a Scope holds (ADR 0051).
    ///
    /// A lookup rather than a cascade: there is nothing above a Scope, so this
    /// walks no chain — there is no chain. A Group Perch does not hold is not a
    /// Scope at all, and answers with the compiled-in defaults rather than with
    /// somebody else's values.
    pub fn settings(&self, scope: &Scope) -> Settings {
        match scope {
            Scope::Ungrouped => self.ungrouped.settings,
            Scope::Group(name) => self.groups.get(name).copied().unwrap_or_default(),
        }
    }

    /// The same, to write through. A Group Perch does not hold has nothing to
    /// write to: declaring one is `declare_group`'s.
    pub fn settings_mut(&mut self, scope: &Scope) -> Option<&mut Settings> {
        match scope {
            Scope::Ungrouped => Some(&mut self.ungrouped.settings),
            Scope::Group(name) => self.groups.get_mut(name),
        }
    }

    /// The Scope an Account's Settings come from: its Group, or the Ungrouped
    /// Accounts.
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
            .filter(|account| account.group.as_deref() == Some(group))
            .collect()
    }

    /// The Accounts that are in no Group — the ordinary starting state, not an
    /// error (ADR 0017).
    pub fn ungrouped_accounts(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|account| account.group.is_none())
            .collect()
    }

    /// The Group declared under a name, whatever it was capitalized as. Two
    /// names that differ only in case are one name here (see
    /// [`refuse_taken_names`](Self::refuse_taken_names)), so this is how a
    /// Group typed in passing is matched to the one that exists.
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
        // somebody says otherwise about this Group. Nothing said elsewhere
        // reaches it — including a `watcher-may-act true` said about another
        // Scope, which is the whole point of a grant being said about the Scope
        // it grants (ADR 0051).
        self.groups.insert(name.to_string(), Settings::default());
        Ok(())
    }

    /// Refuses a name nothing may answer to: one that is not usable at all, one
    /// another name of the same kind already answers to, or one the other half
    /// of the shared namespace holds.
    ///
    /// One function for both halves. Every way a name enters the registry asks
    /// this — `perch group add`, `perch group rename`, `perch alias` and
    /// `perch add` — so what the two halves accept
    /// cannot come apart, and a caller cannot get the order wrong. The order is
    /// load-bearing: shape before collision, because `refuse_taken_names` opens
    /// by asking whether the Alias and the Group are the same name, and with it
    /// reversed `perch add --alias '' --group ''` was refused as "`` cannot be
    /// both an Alias and a Group name" — a Conflict about two names neither of
    /// which was usable in the first place.
    ///
    /// `instead_of` is the name this one is replacing, where it is replacing
    /// one: the Group's current name, or the Alias the Account already answers
    /// to. A name renaming itself does not collide with itself, and that
    /// includes recapitalizing it.
    ///
    /// Nothing else is waived by it. The shared namespace is still checked, so
    /// a recapitalization cannot walk into the other half — which the Group
    /// path has always done and the two Alias paths did not: both returned `Ok`
    /// on a self-rename without asking anything. That was sound, but only by an
    /// argument about what `declare_group` would have refused earlier, and an
    /// inference held in one head is the kind that stops being true when
    /// somebody adds a third way to make a name.
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
            // `refuse_taken_names` for an Alias, because that function is
            // asymmetric: given an Alias it checks both halves, and given a
            // Group only the Alias half.
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
                // cannot collide with the Alias it is giving up, and
                // `refuse_taken_names` would find exactly that.
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
    /// `held` is the name as this registry holds it — what
    /// [`declared_group`](Self::declared_group) answered, rather than what
    /// somebody typed. The caller establishes there is a Group under it, because
    /// what to say about a name nothing holds is the caller's: a typed one gets
    /// the sentence every mistyped Group name gets, and this is not the place
    /// that knows it.
    ///
    /// Three things move with the name, and they are the whole of the change:
    /// the Settings the Group holds, the Accounts that claim it, and what
    /// the last scheduled Check left behind. That last one is the difference
    /// between this and a remove and an add — [`forget_group`](Self::forget_group)
    /// drops the Check record because a Group declared under the same name later
    /// would be a different Group, while a rename is the *same* Group, so a
    /// rename that dropped it would be a way to make the watcher Switch again at
    /// once.
    pub fn rename_group(&mut self, held: &str, to: &str) -> Result<()> {
        self.refuse_a_name_nothing_may_answer_to(NameKind::Group, to, Some(held))?;

        let settings = self
            .groups
            .remove(held)
            .expect("the caller established the Group is declared");
        self.groups.insert(to.to_string(), settings);
        for account in &mut self.accounts {
            if account.group.as_deref() == Some(held) {
                account.group = Some(to.to_string());
            }
        }
        if let Some(checked) = self.checks.remove(held) {
            self.checks.insert(to.to_string(), checked);
        }
        Ok(())
    }

    /// Declares a Group unless it is already there, for the commands that name
    /// a Group in passing rather than to create one — `perch add --group`.
    ///
    /// Returns the name to record, which is the spelling the Group was
    /// declared under: naming `Work` in passing joins `work` rather than
    /// leaving the Account in a Group nothing else can see.
    pub fn ensure_group(&mut self, name: &str) -> Result<String> {
        if let Some(declared) = self.declared_group(name) {
            return Ok(declared.to_string());
        }
        self.declare_group(name)?;
        Ok(name.to_string())
    }

    /// Forgets a Group. The caller establishes that nothing is left in it:
    /// dropping the Group is not a way to empty it.
    ///
    /// What a scheduled Check left behind goes with it. A Group that is gone
    /// paces nothing, and a record kept past it would be a cooldown a Group
    /// declared under the same name later inherited from a Group it never was.
    pub fn forget_group(&mut self, name: &str) {
        self.groups.remove(name);
        self.checks.remove(name);
    }

    /// What the last scheduled Check did within a Group, if one has Switched
    /// there.
    pub fn checked(&self, group: &str) -> Option<&Checked> {
        self.checks.get(group)
    }

    /// Records a Switch a Check made, for the next one to be paced by.
    pub fn record_check(&mut self, group: &str, at: DateTime<Utc>) {
        self.checks
            .insert(group.to_string(), Checked { switched_at: at });
    }

    pub fn account_mut(&mut self, email: &str) -> Option<&mut Account> {
        self.accounts
            .iter_mut()
            .find(|account| same_name(account.email(), email))
    }

    /// The same, where the Account has to be there.
    ///
    /// Eight call sites reached for [`Self::account`] behind an `expect` saying
    /// resolution had named an Account Perch holds — the proof dropped at the
    /// seam and bought again, eight times, as an assertion. What they are
    /// asserting is real and [`validate`] owns it: an Alias or an `active`
    /// naming an Account that is not there is refused on the way in and, since
    /// `save` validates too, on the way out.
    ///
    /// So this is the same defense in depth `save` keeps: the state cannot
    /// happen, and if it does the answer is a refusal naming what could not be
    /// found rather than a panic. Nothing a person can act on — which is why it
    /// says so — but a wedged machine is worse than a bad sentence.
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
    /// Aliases and Group names share one namespace, so neither can shadow the
    /// other and the single Target on `switch` and `run` always has one
    /// answer. The pair is checked together as well as against what is already
    /// held: a command that sets both at once could otherwise plant the
    /// collision it is meant to prevent.
    ///
    /// Two names that differ only in case are the same name. Nobody remembers
    /// which way they capitalized a Group months ago, so `work` and `Work`
    /// reaching different Accounts is the ambiguity this exists to prevent
    /// even though a lookup could tell them apart.
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
            // The same lookup and the same sentence
            // [`Self::refuse_a_group_of_this_name`] is, which exists because an
            // Alias renaming itself needs this half without the one above it.
            // Two copies of one refusal is what the whole of
            // `refuse_a_name_nothing_may_answer_to` was written to end.
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
    /// something else. Hands back the Alias the Account gave up, where it had
    /// one.
    ///
    /// The check is not the caller's to remember, for the reason
    /// [`declare_group`](Self::declare_group) does not leave it to one either:
    /// this was three primitives — `validate_name`, then a self-rename waiver,
    /// then `refuse_taken_names` — reassembled at every call site, in an order
    /// that had already been got wrong once. The Group half has had one door
    /// since it was written and the Alias half had none.
    ///
    /// An Account answers to one Alias, so naming one that already had a name
    /// replaces it: a name the user has moved on from should not go on
    /// reaching the Account behind their back.
    ///
    /// An address is compared the way a name is, with [`same_name`], which is
    /// what every lookup in this module does — so `CAFÉ@example.com` reaches
    /// the Account held as `café@example.com` rather than quietly naming
    /// nothing.
    pub fn name_account(&mut self, alias: &str, email: &str) -> Result<Option<String>> {
        let previous = self.alias_of(email).map(str::to_string);
        self.refuse_a_name_nothing_may_answer_to(NameKind::Alias, alias, previous.as_deref())?;

        self.aliases.retain(|_, named| !same_name(named, email));
        self.aliases.insert(alias.to_string(), email.to_string());
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
    /// An Account is never dropped for this: it stays listed, keeps its Alias,
    /// its Group and its place, and goes on being named. An Account that
    /// vanishes reads as data loss, and a broken one reads as something needing
    /// attention — which is what it is.
    ///
    /// The first reason stands. A Quarantined Account asked a second question
    /// fails a second way, and the reason worth keeping is the one that says how
    /// it broke rather than the last thing that could not be done to it since.
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

    /// Forgets an Account: the entry, the Alias that reached it, and its place
    /// as the active one.
    ///
    /// Its Group is left declared. A Group is something the user said rather
    /// than a summary of where the Accounts happen to be, so emptying one is not
    /// a reason to withdraw the statement — and `perch group remove` is how it
    /// is withdrawn.
    ///
    /// The Credential is not this to delete: what a Profile holds is the
    /// caller's to take away before the row that names it goes, so a store that
    /// will not give it up is met while the Account can still be named.
    pub fn forget(&mut self, email: &str) {
        self.accounts
            .retain(|account| !same_name(account.email(), email));
        self.aliases.retain(|_, named| !same_name(named, email));
        // Either half of a Landing, and not only the Account being treated as
        // active: a Landing naming an Account Perch no longer holds is a
        // dangling pointer the registry refuses to load, and half a Switch is
        // not a thing to keep a record of once one of its two ends is gone.
        //
        // Through `settle`, because that is what the field being private is
        // for: `active` is reached through `begin_landing`, `settle` and
        // `abandon_landing`, each of which names a transition, and a fourth
        // writer inside the one module the rule is aimed at is the one a later
        // reader copies.
        if self.active.names(email) {
            self.settle(None);
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
/// because a machine that cannot say where home is gets a refusal rather than a
/// registry written into the filesystem root.
///
/// Under `~/.config` rather than directly in home. A tool that keeps its state
/// in the home directory adds a line to what somebody sees every time they list
/// it, and Perch's state is not something anybody reads by hand.
///
/// The same path on every platform, Windows included, rather than
/// `%APPDATA%`: one rule is easier to document, to support and to keep in the
/// Host port, which exposes a home directory and nothing else. It is a
/// preference rather than a constraint — nothing in the design breaks under a
/// platform-specific path — and `$PERCH_HOME` is there for anybody who wants
/// one.
///
/// `~/.config` is created if it is not there — at 0700, along with everything
/// below it, since what goes under it here is Credentials.
pub fn perch_home(host: &dyn Host) -> Result<PathBuf> {
    if let Some(overridden) = host.env_var("PERCH_HOME") {
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

/// The Default Profile, as everything that reads or writes the live Credential
/// means it: the directory Claude Code falls back to, and never a Profile.
///
/// `CLAUDE_CONFIG_DIR` is honored, because somebody who moved their
/// configuration directory moved the live Credential along with it — but never
/// when it names a Profile, and pointing it at one is exactly what a Run does.
/// The client a Run launches passes that variable on to everything it starts,
/// so a `perch` typed inside one is told a Profile is the default. It is not.
///
/// Here rather than in [`crate::probe`] because the question is which
/// directories are Perch's own, which is this module's to answer, and one place
/// because every surface that got this wrong got it wrong the same way. A
/// Switch taking `CLAUDE_CONFIG_DIR` at its word inside a Run wrote a third
/// Account's Credential into the running Account's Profile, superseded the copy
/// it had, and left the registry naming an Account the machine was not on — the
/// disagreement between Credential and Identity that ADR 0006 exists to keep
/// impossible, arriving by way of the environment.
pub fn the_default_profile(host: &dyn Host) -> Result<crate::probe::Store> {
    let told = crate::probe::default_store(host)?;
    if told.config_dir.starts_with(profiles_dir(host)?) {
        return crate::probe::default_profile_store(host);
    }
    Ok(told)
}

/// The Profile directory for an Account. The email is slugged because the path
/// is hashed into a keychain service name and has to be stable and printable.
///
/// An address with no alphanumeric character in it slugs to nothing, and
/// joining nothing onto a path gives back the path: the Profile of such an
/// Account would *be* `profiles/`, the directory holding every other Account's.
/// `perch remove` deletes a Profile directory whole, so that Account is one
/// removal away from taking every Credential Perch holds with it. Refused here,
/// at the one place every store and every keychain namespace is derived from,
/// rather than trusted to whatever wrote the address.
pub fn profile_dir_for(host: &dyn Host, email: &str) -> Result<PathBuf> {
    let profiles = profiles_dir(host)?;
    let slugged = slug(email);
    let dir = profiles.join(&slugged);

    // Two ways of asking the same question, because the answer is the whole
    // machine: an empty slug, and — for whatever a future slug might let
    // through — a path that is not one directory below the one it was joined to.
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
/// A Profile is named after the Account it holds, and which Account that is
/// only becomes knowable once the login has finished — so the login happens
/// here and its Credential is moved into a Profile afterwards. Nothing outlives
/// the command: this directory is removed whether the login worked or not.
pub fn pending_login_dir(host: &dyn Host, started_at: DateTime<Utc>) -> Result<PathBuf> {
    Ok(pending_logins_dir(host)?.join(format!("login-{}", started_at.timestamp_millis())))
}

/// Where every pending login lives, so the ones nobody came back from can be
/// found again.
pub fn pending_logins_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("pending"))
}

/// When the login that made this directory started, as its name records.
///
/// The name is `login-<millis>`, written by [`pending_login_dir`], and it is
/// the only account of the directory's age that nothing later moves.
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
/// lock: everything one of those waits for happens inside this one. Short
/// enough that a machine whose Perch was killed mid-command is usable again
/// within a minute rather than needing the directory removed by hand.
const REGISTRY_STALE_MILLIS: i64 = 90_000;
const REGISTRY_UPDATE_MILLIS: i64 = 5_000;

/// The lock one Perch takes so that no other Perch is changing the registry at
/// the same time.
///
/// A directory, taken with the same `mkdir`-or-fail primitive the Claude Code
/// locks use, for the same reason: the call both asks and answers, with nothing
/// in between. It excludes only other Perches — Claude Code has no interest in
/// this file — which is exactly the set that needs excluding.
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
/// Derived rather than chosen, and the derivation is the whole of the number: a
/// Watcher renews this once a round, so the longest it can go quiet while
/// perfectly healthy is the longest wait it ever takes between rounds — the
/// bounded back-off — plus the round that follows it. Anything shorter would
/// have a Watcher backed off against a failing endpoint declared dead by the
/// next `perch watcher run` to come along, and two Watchers is the state this
/// lock exists to prevent (ADR 0040).
///
/// It is deliberately long, and what pays for it is that nothing waits on it. A
/// Watcher that finds the lock held **holds and comes back** rather than
/// exiting, so a lock left behind by a `kill -9` costs a restarted Service some
/// held rounds it prints the reason for, and costs it nothing else. Exiting
/// would have made this number the length of a crash loop, which is the whole
/// of why it is not one.
const WATCHER_STALE_MILLIS: i64 =
    (crate::watch::LONGEST_WAIT_MILLIS + crate::watch::REFRESH_INTERVAL_MILLIS) as i64;

/// Comfortably inside a round, so the renewal a round makes always touches.
const WATCHER_UPDATE_MILLIS: i64 = 60_000;

/// The lock that makes a Watcher the only one on this machine (ADR 0040).
///
/// Two loops for one person watch the same active Account and each keeps its
/// Cooldown in memory, where neither can see the other's — so the pacing the
/// Cooldown exists to impose is undone by running the thing twice. A Check takes the same lock for the same reason: the
/// in-memory Cooldown and the one in `checks` cannot see each other either.
///
/// This is the one artifact a Watcher leaves behind, and it repeals the promise
/// the loop used to print on the way out. It is given back when the process
/// ends, however it ends, because [`crate::lock::Held`] gives its locks back
/// when it is dropped.
pub fn watcher_lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch watcher lock",
        // The Watcher rather than one of its three arrangements: whoever holds
        // this is a loop, a Check or a Service, and which of them it is neither
        // changes what to do about it nor is knowable from here (ADR 0047).
        held_by: "another Watcher",
        dir: perch_home(host)?.join(".watch.lock"),
        stale_millis: WATCHER_STALE_MILLIS,
        update_millis: WATCHER_UPDATE_MILLIS,
        lost_means: "Another Watcher has taken over watching this machine, so \
                     this one is no longer the only one deciding. It stops \
                     rather than deciding alongside it.",
    })
}

/// Shuts every other Perch out of the registry until the returned hold is
/// dropped.
///
/// Every command is a load, some changes made in memory, and a save of the
/// whole thing — so two of them overlapping does not merge, it discards. The
/// hold is taken for the span of the command rather than for the span of the
/// write, because it is the *read* that goes stale: a `perch add` that saved
/// its copy after a `perch switch` had landed would put `active` back to
/// whatever it was before the Switch, and the next Capture would then write the
/// live Credential into the wrong Account's Profile (ADR 0006).
///
/// Never held across a browser login. That is minutes of somebody else's time,
/// and the commands that spend it take this afterwards, against a registry read
/// fresh.
///
/// Perch's home comes into being on the way to the lock, and privately, because
/// on a fresh machine this is the first thing to need it. That is
/// [`lock::take_all`]'s doing rather than this function's: it creates every
/// lock's parent privately, and this lock's parent *is* Perch's home. A second
/// copy here was the same call with the same message, guarding the case the
/// first one already covers.
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

    // The version first, off a shape that is only the version, and before the
    // whole document is read as a Registry.
    //
    // This is the order the guard needs to be any use. A newer Perch is exactly
    // the thing that writes a value this build has no variant for — a Strategy
    // it added, a Quarantine reason — and reading the document first fails on
    // that with serde's own words: "unknown variant `least-recently-used`",
    // about a file that is perfectly well-formed, with nothing in the sentence
    // saying the build in front of them is simply too old. That is the
    // misdiagnosis the version field exists to prevent.
    if let Some(version) = crate::error::claimed_version(&contents)
        && version > CURRENT_VERSION
    {
        return Err(crate::error::written_by_a_newer_perch(
            &path.display().to_string(),
            "registry",
            version,
            CURRENT_VERSION,
        ));
    }

    // Strictly, so a key nobody recognizes is a refusal naming it rather than a
    // value that quietly did nothing. Every type here is Perch's own — Claude
    // Code's `.claude.json` is read through `probe`'s own lenient shapes, which
    // have to tolerate whatever Anthropic adds — so there is nothing upstream
    // for this to be brittle about. What it catches is a hand edit: one
    // transposed letter in `watcher_threshold_percent` used to deserialize as
    // Global's value, run the watcher at a threshold nobody set, and then be
    // erased by the next command that wrote the file, with nothing said at any
    // point. The version guard above runs first, so a genuinely newer Perch is
    // still diagnosed as one rather than as a typo.
    let registry: Registry = serde_json::from_str(&contents).map_err(|err| {
        PerchError::Malformed {
            path: path.display().to_string(),
            detail: err.to_string(),
        }
        .with_note(&the_file_to_edit(path))
    })?;

    validate(&registry).map_err(|refusal| refusal.with_note(&the_file_to_edit(path)))?;

    Ok(Some(with_every_claimed_group_declared(registry)))
}

/// Where to put right something only a hand edit could have put wrong.
///
/// The rule [`validate`] states is about the registry; this is about the file it
/// happens to be written in, and only a caller that read one can say which file
/// that is. Kept apart because the other caller — [`save`] — is holding a
/// registry nobody hand-edited, and telling somebody to go and edit a value that
/// is not in the file yet is the one sentence that would make it worse.
///
/// Named as one sentence rather than spelled at each refusal, which is where it
/// was: seven copies, of which one had already drifted to the singular.
pub fn the_file_to_edit(path: &Path) -> String {
    format!(
        "It is in {}, and every Perch command reads that file — including the \
         ones that would set it. Edit the value there.",
        path.display(),
    )
}

/// The refusal for an Account that was named and is not there.
///
/// Unreachable by construction — [`validate`] refuses every registry that could
/// produce it, on the way in and on the way out — so this is worded as what it
/// is rather than as something to go and fix.
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
/// Checked on the way in rather than where each value is read, because the
/// thing that reads them is a loop nobody is watching: a value that means
/// nothing would otherwise sit in the file until the watcher next went round
/// and surprise somebody by acting on it.
///
/// Checked here means every command meets it, including `perch config set` —
/// the one that would otherwise be the repair. A value only a hand edit can
/// produce is a value only a hand edit can take back out, so a caller that read
/// the registry off a disk says where to do that, with
/// [`the_file_to_edit`]. The rule itself names no file, because the other
/// caller is holding a registry that came from nowhere but Perch.
///
/// Public because an Import writes a registry without reading one first, and
/// was running a narrower check of its own — so a file `perch holdings import`
/// accepted could be one every later command refused to read, which leaves a
/// machine with no working command on it and no `perch holdings purge` either.
/// One function, so what an Import will accept and what a load will accept
/// cannot differ.
pub fn validate(registry: &Registry) -> Result<()> {
    // Every Scope, and every Scope is all of them: with no layer above, a
    // Setting is read from the Scope that holds it and nowhere else, so one
    // walk over the Scopes is the whole of the check.
    for scope in registry.scopes() {
        registry.settings(&scope).validate(&scope)?;
    }

    // The Group *names* an Account claims, for the same reason.
    // `with_every_claimed_group_declared` repairs a hand-edited registry by
    // declaring what it finds — and a hand-edited registry is exactly where a
    // name nothing would have accepted comes from. Declared by a raw insert, a
    // claim of `none` became a Group `move_account` can never move an Account
    // into, because `means_no_group` is asked first; a claim of `my work`
    // became one whose `perch config get` line cannot be typed back into
    // `perch config set`, which is the round trip whitespace is refused to
    // protect; and a claim colliding with an Alias planted the namespace
    // collision `refuse_taken_names` exists to make impossible, after which
    // `target::matched` resolves the name to the Alias and the Group is
    // unreachable.
    //
    // Declared Groups are walked as well as claimed ones, and Aliases with
    // them. Only claims were checked, which left the two other halves of the
    // same namespace to be hand-edited freely: a declared Group nobody is in
    // never reached the loop at all, so `my work` sat in the file printing a
    // `perch config get` line that cannot be typed back in. And the `aliases`
    // map was never looked at, so an Alias keyed by an email address resolved
    // ahead of the Account of that name — the Target with two answers the `@`
    // rule exists to make impossible.
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

    // What each Alias points *at*, which the loop above does not look at: it
    // asks whether the key is a name Perch would have given, and never whether
    // the value names an Account Perch holds.
    //
    // A dangling one is not a refusal anywhere downstream — it is a panic.
    // `target::matched` builds a `Target::Alias` straight out of the map with no
    // existence check, and `perch switch`, `perch remove`, `perch relogin` and
    // `perch run` all then reach for `registry.account(&email)` behind an
    // `expect` that says resolution named an Account Perch holds. The `active`
    // pointer has never had this problem, because `active_account` resolves
    // through `and_then(account)` and its callers turn `None` into a refusal;
    // the Alias half had no equivalent.
    let mut named: Vec<(&str, &str)> = Vec::new();
    for (alias, email) in &registry.aliases {
        if registry.account(email).is_none() {
            return Err(PerchError::Invalid(format!(
                "The registry gives the Alias `{alias}` to {email}, which is not \
                 an Account Perch holds.",
            )));
        }
        // One Account, one Alias. `set_alias` enforces it by dropping the old key
        // — "a name the user has moved on from should not go on reaching the
        // Account behind their back" — and nothing was asking it of a file.
        //
        // With two, `alias_of` returns whichever the map yields first, so `perch
        // list` and every sentence Perch writes show one of them while `perch
        // switch` answers to both, and `perch alias <the other> --unset` reports
        // a name freed that was never the one being shown. That is the same "which
        // one a Target finds is not decided by anything" harm as two names that
        // differ only in case, one map value away.
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

    // The other pointer into the Accounts, and the one the Alias check above was
    // written for: "a dangling one is not a refusal anywhere downstream — it is
    // a panic". `active` has exactly the same shape and had no such check. It
    // survives today because `active_account` resolves through `and_then` and
    // its callers turn `None` into a refusal — which means a dangling pointer
    // reads as "no Account is active", and the repair somebody is offered is to
    // switch to one, on a machine where the Account they are on is right there
    // in the file.
    //
    // Holding nothing is a state and not a fault: a machine that has never
    // switched has no active Account.
    //
    // Both ends of a Landing, because both are pointers into the Accounts and
    // resolving one reads the Credential of the other.
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

    // The third member of the namespace, which nothing was checking. A Target is
    // an Alias, a Group name or an Account's address, and `validate_name` keeps
    // the first two tellable from the third by refusing an `@` in them —
    // "because a Target that could be either has no single answer". The mirror
    // rule, that an address actually looks like one, was never stated anywhere:
    // `probe::read_identity` asks only for one alphanumeric character, and
    // `refuse_taken_names` consults the Aliases and the Groups but never the
    // Accounts.
    //
    // So an Account called `work` beside a Group called `work` resolved to the
    // Account, leaving the Group reachable from `perch group list` and `perch
    // config set` and unreachable from `perch switch` and `perch run`; beside an
    // *Alias* called `work` it left the Account reachable by no Target at all.
    // Refused here, one rule is what makes `refuse_taken_names`' two-way check
    // correct without it needing a third lookup.
    for account in &registry.accounts {
        if !account.email().contains('@') {
            return Err(PerchError::Invalid(format!(
                "The registry holds an Account called `{}`, which is not an \
                 address an Alias or a Group name could be told from — and a \
                 Target that could be either has no single answer.",
                account.email(),
            )));
        }
    }

    // One entry per Account, for the same reason and with a worse ending.
    // `upsert` is what every command writes an Account through and it replaces
    // the matching entry, so two entries for one address are something only a
    // hand edit produces — after which `account` and `account_mut` silently act
    // on the first of them, `perch list` renders one Account as two rows, and a
    // Cycle counts it twice when it ranks the Group.
    let mut held: Vec<&str> = Vec::new();
    for account in &registry.accounts {
        if let Some(already) = held.iter().find(|seen| same_name(seen, account.email())) {
            return Err(PerchError::Invalid(format!(
                "The registry holds two Accounts spelled `{already}` and `{}`, \
                 which are one Account — so which entry a command reads, and \
                 which one it writes, is not decided by anything.",
                account.email(),
            )));
        }
        held.push(account.email());
    }

    // Two names in the *same* half of the namespace that differ only in case.
    // The loop above catches the collision across the two halves, and
    // `declare_group` and `refuse_taken_names` refuse both kinds at creation —
    // but nothing was asking it of a file somebody had edited, and `target`
    // states the answer as an assumption: "the registry refuses an Alias or a
    // Group that differs from a held name only in case, so there is never more
    // than one candidate to find".
    //
    // With two, which one is found is which one a `BTreeMap` yields first.
    // `aliases` holding both `work` and `Work` renders one of them in `perch
    // list` and resolves `perch switch work` to the other, and freeing `work`
    // frees neither reliably.
    refuse_two_names_that_differ_only_in_case(NameKind::Group, registry.groups.keys())?;
    refuse_two_names_that_differ_only_in_case(NameKind::Alias, registry.aliases.keys())?;

    // The percentages a Cycle ranks on, checked the way a Group's are and for
    // the reason `GroupConfig::validate` states: the thing that reads them is a
    // loop nobody is watching. `watcher_threshold_percent` was range-checked
    // here and `used_percent` — the figure that loop compares *against* the
    // threshold — was not, though `anthropic::understand` clamps it to 0–100 on
    // the way in, so the invariant is real and was simply enforced in one of the
    // two places a figure can enter the registry.
    //
    // The other place is a hand edit or a restored Export, which is what every
    // check above is written against. `"used_percent": -50` gives
    // `cycle::headroom_of` 150% of headroom, so the Account outranks a genuinely
    // untouched one and the watcher's `used_percent >= threshold` never fires:
    // an Account that can never be moved off, and cannot be seen to be wrong
    // from `perch list` either, which renders it as nothing but an odd
    // percentage. `150` is the mirror: negative headroom, so an Account with
    // room to spare is ranked below one that has none.
    //
    // A range and not a finiteness check as well: serde_json refuses a literal
    // it cannot hold in an `f64` — `1e400` is "number out of range" before this
    // is reached — and JSON has no way to spell a NaN. The range is the whole of
    // what is left to ask, and it answers `false` for either anyway.
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
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if let Some(already) = seen.iter().find(|held| same_name(held, name)) {
            return Err(PerchError::Invalid(format!(
                "The registry holds {} `{already}` and `{name}`, which differ \
                 only in case — so which one a Target finds is not decided by \
                 anything.",
                kind.article(),
            )));
        }
        seen.push(name);
    }
    Ok(())
}

/// Refuses a name in the registry that `declare_group` or `perch alias` would
/// not have allowed in the first place.
///
/// Named rather than repaired, for the reason the configuration check above
/// gives: a value only a hand edit can produce is a value only a hand edit can
/// take back out, and every command reads this file — including the ones that
/// would otherwise be the repair.
///
/// One function for both halves of the namespace, because the namespace is
/// shared and what makes a name unacceptable is the same list either way. Two
/// copies is how one of them comes to allow what the other refuses, which is
/// the state this exists to find.
///
/// The collision between the two halves is asked from the Group side only. It
/// is one fact about a pair, and every Group is walked — declared and claimed
/// alike — so asking again from the Alias side would be a branch nothing could
/// reach.
fn refuse_a_name_nothing_would_have_accepted(
    registry: &Registry,
    kind: NameKind,
    name: &str,
) -> Result<()> {
    // `none` is not a case of its own here. `validate_name` already refuses it,
    // for both kinds, in words true of a claim and a declaration alike — "means
    // no Group at all on `perch group move`, so it cannot also name one" —
    // whereas the sentence this used to carry said "an Account cannot be in
    // it", which is a statement about Accounts and this loop walks *declared*
    // Groups too. A registry of no Accounts and a Group called `none` was
    // refused with a claim about the Accounts it does not hold, and two
    // sentences for one rule is how the two come to disagree about it.
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
/// The invariant `group_names` states — "a Group an Account claims is always
/// declared too, `load` sees to that" — and which nothing was enforcing. An
/// Account claiming an undeclared Group falls out of `perch list` entirely,
/// because the listing walks the declared Groups and then the Accounts in none
/// of them (ADR 0049), so it becomes an Account nothing shows; and `perch
/// switch <that group>` refuses while `perch list` shows the Group.
///
/// Declared rather than refused, because the Group's settings are what is
/// missing and the defaults are what a freshly declared Group carries anyway.
///
/// A claim that differs from a declaration only in case is the *same* Group,
/// and is rewritten to the declared spelling rather than declared a second
/// time. Everywhere else in this module two names differing in case are one
/// name — [`same_name`] is what `declare_group` refuses on and what
/// [`Registry::ensure_group`] returns the held spelling for — so a second key
/// here would be a Group nothing but this function believes in: an empty
/// section in the listing, an `accounts_in` that matches nobody, and a
/// `declared_group` answering with whichever the map happened to order first.
fn with_every_claimed_group_declared(mut registry: Registry) -> Registry {
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
    registry
}

/// Writes the registry, under the hold the caller took to read it.
///
/// The hold is asked for rather than assumed, and it is the reason this
/// signature is the shape it is: the invariant the whole module turns on is
/// that a registry is only ever written by the Perch that read it, and a
/// parameter is the only way to say that once rather than in every caller.
///
/// It is also the one place every command reliably passes through after however
/// long its work took, which makes it where the hold is renewed. A hold that
/// was lost — Perch stalled past the staleness window and another `perch` took
/// the lock over and ran a whole command under it — is a hold whose registry is
/// behind the one on disk, and writing it back would revert that command
/// wholesale. So it is refused, and the caller says what that cost.
///
/// It is also where [`validate`] is asked on the way out, so that what this
/// writes and what [`load`] will accept cannot come apart.
pub fn save(host: &dyn Host, perch: &mut lock::Held<'_>, registry: &Registry) -> Result<()> {
    perch.renew();
    if !perch.still_held() {
        // A general failure rather than `Busy`, and deliberately (ADR 0036).
        // `Busy` promises that nothing was changed, and `perch watcher run`
        // branches on that promise by going round again — but this save is
        // reached after a Switch has already moved a Credential as often as
        // before anything has been written, and from here there is no telling
        // which.
        return Err(PerchError::Other(
            "Another `perch` took the registry lock over while this command was \
             working, and has changed the registry since this one read it. \
             Nothing was written, because writing would have undone whatever it \
             did. Run this command again."
                .to_string(),
        ));
    }

    // What `load` will accept, asked before `load` has to refuse it.
    //
    // Eight invariants were enforced on the way in and none on the way out, so
    // a command could write a file every later command declined to read — which
    // leaves a machine with no working `perch` on it and no `perch holdings
    // purge` either. That gap has been found twice from the other side: `perch
    // holdings import` ran a narrower check of its own until `validate` was
    // made public for it, and `used_percent` was range-checked in one of the
    // two places a figure can enter the registry. Both repairs added an
    // obligation to a caller. This one removes the need for it.
    //
    // Nothing reachable trips it today — the whole suite is green without this
    // line — which is what it is for: the failure it catches is a bug in Perch,
    // and the value of catching it is that the file is never written. So it is
    // a refusal in the shipped binary rather than a `debug_assert`, and it says
    // what it is rather than telling somebody to hand-edit a value that is not
    // in the file. `Other` rather than the `Invalid` the rule raises, because a
    // script reading exit 14 is being told its input was wrong, and it was not.
    validate(registry).map_err(|invalid| {
        PerchError::Other(format!(
            "{invalid}\n\n{}\n\
             Nothing was written, and the registry on disk is as it was.",
            crate::report::this_is_a_bug(),
        ))
    })?;

    let path = registry_path(host)?;
    // Stamped rather than carried through. `load` returns whatever version the
    // file claimed and this writes it back, so a document claiming something
    // else kept claiming it through every write this build made — and the field
    // is documented as "the version this build writes". A guard that describes
    // the last writer rather than this one is a guard about nothing.
    let body = serde_json::to_string_pretty(&Registry {
        version: CURRENT_VERSION,
        ..registry.clone()
    })
    .map_err(|err| PerchError::Other(format!("could not serialize the registry: {err}")))?;
    write(host, &path, &format!("{body}\n"))
}

/// Replaces the registry in one step, or not at all, and for its owner alone.
///
/// **In one step**, with the same care `.claude.json` gets and for a sharper
/// reason: this file is the whole of Perch's state, and every command reads it
/// before it does anything. A truncate-then-write leaves a window in which a
/// reader — `perch status`, which is advertised for shell prompts and may run
/// several times a minute — sees half a file and reports it as malformed; and a
/// crash inside that window leaves it half-written for good, with no command
/// able to run until somebody edits it by hand.
///
/// **For its owner alone**, because the registry holds no Credential but holds
/// everything else: every Account's email address, organization, plan, Alias,
/// Group and Quarantine reason, and the Utilization history behind them. That
/// is a full picture of somebody's Anthropic relationships, and the Profile
/// directories it sits beside are already 0700 (ADR 0020) — this file was the
/// gap. A `~/.config/perch` that already exists keeps the mode it has, as `mkdir -p`
/// does everywhere else in Perch, but the file is replaced on every save and so
/// comes back narrow from the first one.
/// The directory above it is not made here. `write_private_file` is documented
/// as creating a file "and any directory above it" with that mode, and both
/// Hosts do exactly that — so the call this used to make first was the same call
/// against the same path with the same mode. That is the duplicate
/// [`lock`] records having already been removed once; this was the copy that
/// survived it.
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

    /// `join("")` gives back the path it was joined to, so an address with
    /// nothing nameable in it would put an Account's Profile *at* the
    /// directory holding every other Account's — and `perch remove` deletes a
    /// Profile directory whole.
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

    /// Every command is a load, some changes made in memory and a save of the
    /// whole thing, so two of them overlapping does not merge — it discards.
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

    /// An address is a name, and is compared the way every other name here is.
    ///
    /// It was not: the lookups compared with `==` while everything feeding them
    /// — `target::matched`, `add`, `relogin`, `remove` and `validate` — compared
    /// with [`same_name`]. Sound, but only because resolution always hands back
    /// the spelling the registry holds, which was prose in five places and the
    /// thing eight `expect`s downstream rested on. `validate` refuses two
    /// Accounts whose addresses differ only in case, on the way in and now on
    /// the way out, so inside a registry Perch will load there is never more
    /// than one to find.
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

    /// An Account that was named and is not there is a refusal, not a panic.
    ///
    /// Eight call sites asserted this with `expect`, each having thrown away
    /// the proof resolution gave them and bought it back as a lookup. The state
    /// cannot happen — `validate` refuses every registry that could produce it,
    /// on the way in and, since `save` validates too, on the way out — which is
    /// exactly why the answer to it happening anyway should be a machine that
    /// still works.
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

        // The mutable half answers the same way. Two functions rather than one
        // because a caller that goes on to change what it finds needs the other
        // borrow, and a refusal that differed between them would be the two
        // halves of one rule disagreeing.
        let mut registry = Registry::default();
        assert_eq!(
            registry
                .held_mut("nobody@example.com")
                .expect_err("Perch does not hold it")
                .to_string(),
            refused.to_string()
        );
    }

    /// The pointer the Alias check was written for, asked of the other one.
    ///
    /// `validate` refuses an Alias naming an Account Perch does not hold,
    /// because "a dangling one is not a refusal anywhere downstream — it is a
    /// panic". `active` is the same shape and had no check: it reads as "no
    /// Account is active", so the repair somebody is offered is to switch to
    /// one, on a machine where the Account they are on is right there in the
    /// file.
    ///
    /// The `active` states here are written to the field rather than through the
    /// transitions, and this is the one place that is right: `validate` guards a
    /// registry Perch is *reading*, and a dangling pointer is a state no
    /// transition can produce — a hand-edited file is what the rule is written
    /// against.
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

    /// Both ends of a Landing, because both are pointers into the Accounts and
    /// resolving one reads the Credential of the other (ADR 0048).
    ///
    /// The end that dangles is named, rather than the pair reported as one
    /// broken `active`: a hand-edited registry is what this rule is written
    /// against, and which of the two addresses to put right is the whole of
    /// what the person editing it needs to be told.
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

    /// Every rule the shared namespace has, asked of both halves of it.
    ///
    /// A table because the rules are one fact with two spellings, and they had
    /// come apart: the Group half funneled through a single private check and
    /// the Alias half was three primitives reassembled at each of its three
    /// call sites, in an order one of them had already got wrong. Asked here of
    /// the one function all four callers now go through, so a fifth cannot
    /// reassemble it differently.
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
            // included — the same waiver on both halves, which is new for the
            // Alias one.
            (NameKind::Group, "Personal", Some("personal"), None),
            (NameKind::Alias, "Work", Some("work"), None),
            // Shape before collision. `perch add --alias '' --group ''` was
            // refused as "`` cannot be both an Alias and a Group name" — a
            // Conflict, about two names neither of which was usable at all.
            (NameKind::Group, "", None, Some("cannot be empty")),
            (NameKind::Alias, "", None, Some("cannot be empty")),
            (
                NameKind::Alias,
                "has a space",
                None,
                Some("has a space in it"),
            ),
            (NameKind::Group, "none", None, Some("means no Group at all")),
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

    /// The half of the check an Alias renaming itself used to skip.
    ///
    /// Both Alias paths returned `Ok` the moment the new name matched the held
    /// one, without asking the other half of the namespace anything. That was
    /// sound — a Group cannot be declared under a name an Alias already holds —
    /// but only by an argument about what `declare_group` would have refused
    /// earlier, which is why the registry here is built by hand: nothing
    /// reachable can produce it. It is what a third way of making a name would
    /// walk into, and it now has one answer rather than an inference.
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

    /// A registry `load` would refuse is one `save` declines to write.
    ///
    /// Nothing reachable produces one — every command in the suite passes with
    /// this guard in place, and `registry_of` runs `load` after each of them —
    /// so what is asserted here is the guard rather than the property. The
    /// property is already covered; the part that is not is what happens on the
    /// day a command gets it wrong, and the whole value of that is that the file
    /// on disk is left alone.
    ///
    /// The message is asserted too. A refusal that told somebody to edit a value
    /// in a file it just declined to write would send them looking for something
    /// that is not there, which is how the wording drifts back.
    #[test]
    fn a_registry_load_would_not_read_is_one_save_declines_to_write() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");
        let path = registry_path(&host).unwrap();
        save(&host, &mut perch, &Registry::default()).expect("an empty one is fine");
        let before = host.file(&path).expect("it was written");

        // An Alias naming an Account Perch does not hold: a dangling one is not
        // a refusal downstream, it is the `expect` in every command that
        // resolves a Target.
        let mut broken = Registry::default();
        broken
            .aliases
            .insert("work".to_string(), "nobody@example.com".to_string());

        let refused = save(&host, &mut perch, &broken).expect_err("load would not read it");

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

    /// The hold spans the whole command, and a command can stall for as long as
    /// somebody takes to answer a `[y/N]`. Renewed at every write, so the
    /// ordinary long command keeps the lock it took rather than letting it
    /// expire silently underneath itself.
    #[test]
    fn a_command_that_takes_its_time_keeps_the_lock_it_took() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // Comfortably past the staleness window, several times over: exactly
        // the shape of a `perch remove` waiting on somebody who walked away.
        for _ in 0..4 {
            host.sleep(REGISTRY_STALE_MILLIS as u64 - 10_000);
            save(&host, &mut perch, &Registry::default()).expect("it is still Perch's to write");
        }

        assert!(perch.still_held());
        assert!(
            lock(&host).is_err(),
            "and no other Perch could have taken it in the meantime"
        );
    }

    /// The other direction, and the reason the hold is checked rather than
    /// assumed: a Perch that did stall past the window has had its lock taken
    /// over, and the registry it read before that is however many commands out
    /// of date. Writing it back would revert every one of them wholesale — so
    /// the write is refused, and the user is told to run the command again.
    #[test]
    fn a_registry_read_before_somebody_elses_command_is_not_written_over_theirs() {
        let host = crate::host::FakeHost::new();
        let mut perch = lock(&host).expect("the registry lock is free");

        // The stall, and another Perch finding the lock abandoned and taking it.
        host.sleep(REGISTRY_STALE_MILLIS as u64 + 1_000);
        let theirs = lock(&host).expect("an abandoned lock is taken over");
        save(&host, &mut { theirs }, &Registry::default()).expect("theirs is the live hold");
        let before = load(&host).expect("it reads").expect("they wrote one");

        let stale = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        let refused = save(&host, &mut perch, &stale).expect_err("this one may no longer write");

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

    /// Perch's home holds Profile directories full of Credentials, and on a
    /// fresh machine the *lock* is what brings it into being — before any
    /// registry has been written into it. Created privately there rather than
    /// at whatever the umask happens to be, or the narrow modes below it guard
    /// files inside a directory anybody may walk into.
    #[test]
    fn the_home_the_lock_creates_is_the_owners_alone() {
        let host = crate::host::FakeHost::new();

        let _perch = lock(&host).expect("the registry lock is free");

        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE)
        );
    }

    /// A key nobody recognizes is a refusal naming it, rather than a value that
    /// quietly did nothing and was then erased.
    ///
    /// Every other way of getting this file wrong by hand has a named refusal —
    /// a bad version, a bad Strategy, a percentage out of range, a dangling
    /// Alias, an address with no `@`. A transposed letter had neither a refusal
    /// nor an effect: `watcher_treshold_percent` deserialized as the default,
    /// so the Group went on running at a threshold nobody set, and the next
    /// command that wrote the file re-serialized the Group without it — the
    /// edit gone, with nothing said at any point in between.
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

    /// The registry is the whole of Perch's state and every command reads it
    /// first, so a write that stops half way must not be visible: a reader
    /// would call the file malformed, and a crash inside that window would
    /// leave it malformed for good.
    #[test]
    fn a_save_that_fails_leaves_the_registry_exactly_as_it_was() {
        let path = "/Users/someone/.config/perch/registry.json";
        let before = format!("{{\"version\":{CURRENT_VERSION},\"accounts\":[]}}");
        let host = crate::host::FakeHost::new()
            .with_file(path, &before)
            .with_unwritable_file(path, "No space left on device (os error 28)");

        let registry = Registry {
            active: Active::Settled("someone@example.com".into()),
            ..Registry::default()
        };
        let mut perch = lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &registry).expect_err("the write cannot land");

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

    /// No Credential, but every Account's address, organization, plan, Alias,
    /// Group, Quarantine reason and Utilization history — a full picture of
    /// somebody's Anthropic relationships, beside Profile directories that are
    /// already 0700.
    #[test]
    fn the_registry_is_written_for_its_owner_alone() {
        let host = crate::host::FakeHost::new();

        let mut perch = lock(&host).expect("the registry lock is free");
        save(&host, &mut perch, &Registry::default()).expect("it is written");

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

    /// What a scheduled Check leaves for the next one, and nothing where none
    /// has Switched: a key in every registry on every machine would be a
    /// promise the watcher had run, which for nearly all of them it has not.
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

    /// A Group that is gone paces nothing, and a record kept past it would be a
    /// cooldown inherited by a Group declared under the same name later.
    #[test]
    fn forgetting_a_group_forgets_what_a_check_recorded_against_it() {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.record_check("work", Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap());

        registry.forget_group("work");

        assert_eq!(registry.checked("work"), None);
    }

    /// Not so that anything can be migrated — nothing is running this yet, so
    /// there is nothing to migrate from. It is there so a build that
    /// understands less than the file it is handed says so.
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

    /// The positive state has no name (ADR 0052), so the file says nothing
    /// about an Account nobody has taken out of Cycling — the same shape, and
    /// the same reason, as the Quarantine above it.
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

    /// `enabled` was the same bool spelled the other way round, and ADR 0052
    /// renamed it rather than teaching this build to read both. A registry
    /// carrying it is refused, which is what `deny_unknown_fields` is for.
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
             declaration nobody made (ADR 0017)"
        );
    }

    /// A freshly declared Group holds the compiled-in defaults, and nothing
    /// said about another Scope reaches it (ADR 0051).
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

    /// The grant that has to be said about the Scope it grants: a Group
    /// declared after somebody let the watcher into another one is a Group
    /// nobody has said anything about (ADR 0051).
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

    /// The one number the watcher's policy still carries (ADR 0046). Asserted as
    /// the number rather than as the constant, because a default is a promise
    /// made in the docs and a test that reads the constant back cannot notice it
    /// change.
    #[test]
    fn the_watchers_policy_has_the_default_it_is_documented_with() {
        assert_eq!(Settings::default().watcher_threshold_percent, 80);
    }

    /// A number a setting cannot hold is refused with the ones it can — from a
    /// hand-edited registry as much as from a mistyped command, because both
    /// readers have the same next question.
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
                // The other word that already addresses something. A Group
                // called `ungrouped` is one no `perch config set` could reach;
                // an Alias called `ungrouped` is the same collision from the
                // other side, and was the case nothing asked about.
                "ungrouped",
                "Ungrouped",
                " work",
                "work ",
                // Not only at the ends: `perch config get` prints settings as
                // the `perch config set` that would restore them, read back a
                // word at a time, so a name with a space in it is one no line
                // of that output could name.
                "my work",
                "Overflow Ltd",
                "two\twords",
                "someone@example.com",
                // The word people mean Global by. Not a Scope anything
                // addresses — Global is addressed by naming none — which is
                // precisely why a Group taking the name is dangerous: `perch
                // config set global strategy …` would land on the Group and
                // leave Global as it was.
                "global",
                "Global",
                // Spelled like a flag. `perch run`'s program goes after `--`,
                // so a Target beginning with `-` is one that command can never
                // be given, however it is quoted.
                "-dev",
                "--work",
                "-",
            ] {
                let refused = validate_name(kind, name)
                    .expect_err(&format!("`{name}` should not be usable as a {kind:?} name"));
                // And the refusal states the rule the user broke rather than a
                // rule about the other half of the namespace. `none` and
                // `ungrouped` both told somebody naming an Account that their
                // name could not also name a Group.
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

    /// The machine-readable name of a Quarantine, which is what `--json` puts in
    /// front of a script (`reason`, beside the prose `detail`). Every kind needs
    /// one, and no two may share it: a script branching on why an Account is
    /// Quarantined branches on this string and nothing else.
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

    /// `PERCH_HOME` moves everything Perch keeps, which is what lets a test —
    /// or a second Perch on one machine — run without touching the real one.
    /// It passes through verbatim: no `.config/perch` is appended to it.
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

    /// A registry that is not there is a Perch that holds nothing, and a
    /// registry that is there and will not be read is a failure. Saying "no
    /// Accounts" for the second would be a Perch that quietly forgot everything
    /// the moment a permission went wrong.
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

    /// The invariant `group_names` states, now that something holds it.
    ///
    /// An Account claiming a Group nothing declared falls out of `perch list`
    /// entirely — the listing walks the declared Groups and then the Accounts
    /// in none of them (ADR 0049) — which makes it an Account nothing shows,
    /// while `perch switch <that group>` refuses.
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

    /// The other half of the same repair, and the reason it is a repair rather
    /// than a bare insert: two names differing only in case are one name
    /// everywhere else in this module, so a claim spelled `Work` against a
    /// declared `work` must join it rather than become a second Group nothing
    /// else believes in — an empty section in the listing, an `accounts_in` that
    /// matches nobody, and a `declared_group` answering with whichever the map
    /// ordered first.
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

    /// The repair declares what it finds, and a hand-edited registry is exactly
    /// where a name nothing would have accepted comes from. Inserted raw, each
    /// of these produced a Group that exists and cannot be used: `none` is one
    /// `move_account` can never move an Account into, because `means_no_group`
    /// is asked first; `my work` is one whose `perch config get` line cannot be
    /// typed back into `perch config set`, which is the round-trip whitespace is
    /// refused to protect; and a claim colliding with an Alias plants the
    /// namespace collision `refuse_taken_names` exists to make impossible, after
    /// which the name resolves to the Alias and the Group is unreachable.
    #[test]
    fn a_claim_declare_group_would_have_refused_is_named_rather_than_declared() {
        let claims = [
            (r#""none""#, "{}", "means no Group"),
            (r#""my work""#, "{}", "has a space in it"),
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

    /// The other two halves of the same namespace, which only a *claim* being
    /// walked left unguarded. A declared Group nobody is in was never looked at,
    /// so `my work` sat in the file printing a `perch config get` line that
    /// cannot be typed back into `perch config set` — the round trip whitespace
    /// is refused to protect. And the `aliases` map was never looked at at all,
    /// so an Alias keyed by an email address resolved ahead of the Account of
    /// that name: `perch switch someone@example.com` landing on somebody else,
    /// which is the Target with two answers the `@` rule exists to prevent.
    #[test]
    fn a_declared_group_or_an_alias_nothing_would_have_accepted_is_named_too() {
        let holdings = [
            (r#""groups":{"my work":{}}"#, "has a space in it"),
            (r#""groups":{"none":{}}"#, "means no Group"),
            (
                r#""aliases":{"someone@example.com":"other@example.com"}"#,
                "looks like an email address",
            ),
            (
                r#""groups":{"work":{}},"aliases":{"work":"someone@example.com"}"#,
                "share one namespace",
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
            // These registries hold no Accounts at all, so a refusal that
            // explains itself in terms of what an Account can be in is a
            // statement about nothing. `validate_name`'s own wording is true of
            // a declaration and a claim alike, which is why there is one of it.
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

    /// What `perch add` may offer as a Group name, and — the part that had no
    /// test at all — what it must decline to offer.
    ///
    /// The offer is made from an organization name, which is whatever Anthropic
    /// holds rather than anything anybody chose. `add` picks a different
    /// question depending on whether there is an offer, so a `None` that
    /// stopped being a `None` would put a name in front of somebody that
    /// `validate_name` refuses one keystroke later — at the one moment the
    /// browser round trip has already been spent, which is the cost the code
    /// there is written around. A personal-plan organization rendered from an
    /// email address is the likeliest of these, and it is exactly the shape the
    /// `@` rule exists to refuse.
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

    /// A registry this build cannot read is never reported as one that is not
    /// JSON, because most of the time it *is* JSON.
    ///
    /// The version guard above closes this for a document claiming a version
    /// from the future, which is one of the ways a build meets a value it has no
    /// variant for. It is not the only one: a hand edit picks the same wrong
    /// spelling, and every other way serde declines a well-formed document —
    /// a missing `version`, a number that will not fit — arrives the same way.
    /// Told "not valid JSON", somebody goes looking for a syntax error that is
    /// not there, past the half of the sentence that says what is wrong.
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

    /// The figure a Cycle ranks on, checked the way a Group's thresholds are.
    ///
    /// `GroupConfig::validate` refuses a `watcher-threshold-percent` outside
    /// 0–100 because "the thing that reads them is a loop nobody is watching".
    /// `used_percent` is the number that same loop compares *against* that
    /// threshold, and it went unchecked — clamped on the way in by
    /// `anthropic::understand` and by nothing at all on the way a hand edit or a
    /// restored Export takes.
    ///
    /// Both of these are silent rather than loud. A negative figure gives
    /// `cycle::headroom_of` more than 100% of headroom, so the Account outranks
    /// one that has genuinely never been touched and the watcher's `>=
    /// threshold` never fires — an Account nothing will move off, showing in
    /// `perch list` as an odd percentage and nothing more. An overflowing
    /// literal deserializes to infinity, and makes an Account no Cycle will ever
    /// choose.
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

    /// What an Alias points at, which walking the keys never asked.
    ///
    /// Downstream this is not a refusal but a panic: `target::matched` builds a
    /// `Target::Alias` straight out of the map, and `perch switch`, `perch
    /// remove`, `perch relogin` and `perch run` all then reach for the Account
    /// behind an `expect` saying resolution named one Perch holds. A file that
    /// `load` accepts and every command panics on is the state `validate` exists
    /// to turn into a sentence.
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

    /// Two names in one half of the namespace that only case tells apart.
    ///
    /// `declare_group` and `refuse_taken_names` refuse both kinds at creation,
    /// and the collision *across* the halves is already caught — but nothing
    /// asked it of a hand-edited file, while `target` states the answer as an
    /// assumption it relies on: "the registry refuses an Alias or a Group that
    /// differs from a held name only in case, so there is never more than one
    /// candidate to find". With two, which one is found is whichever a
    /// `BTreeMap` happens to yield first, so `perch list` renders one and `perch
    /// switch` lands on the other.
    #[test]
    fn two_names_in_one_half_of_the_namespace_that_differ_only_in_case_are_refused() {
        let holdings = [
            r#""groups":{"work":{},"Work":{}}"#,
            // Two Accounts, one Alias each: giving both names to one Account is a
            // different refusal, and this one is about the pair of names.
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

    /// The namespace has three members and only two of them were guarded.
    ///
    /// `validate_name` refuses an `@` in an Alias or a Group name so that a
    /// Target is never ambiguous; nothing said that an Account's address has to
    /// look like one. An Account called `work` beside a Group called `work`
    /// resolved to the Account, so `perch group list` and `perch config set` went
    /// on showing and editing a Group that `perch switch` and `perch run` could
    /// no longer reach — and beside an *Alias* of that name it was the Account
    /// that became reachable by no Target at all.
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

    /// One entry per Account and one Alias per Account, asked of a file.
    ///
    /// `upsert` and `set_alias` both enforce these — `set_alias` drops the old key
    /// so "a name the user has moved on from should not go on reaching the Account
    /// behind their back", and `upsert` replaces the matching entry rather than
    /// adding a second. Nothing was asking either of a registry somebody edited,
    /// and both land in the same place as two names that differ only in case:
    /// which one a command reads is whichever a `BTreeMap` or a `Vec` happens to
    /// yield first.
    ///
    /// Two Aliases showed one of them in `perch list` while `perch switch`
    /// answered to both, and freeing either reported a name freed that was not the
    /// one being shown. Two entries had `perch list` render one Account as two
    /// rows and a Cycle count it twice when ranking the Group.
    #[test]
    fn one_account_reached_twice_over_is_refused() {
        let cases = [
            (
                r#""aliases":{"spare":"someone@example.com","work":"someone@example.com"}"#,
                "one Alias at a time",
            ),
            (
                r#""aliases":{}"#,
                // Two entries for one address, spelled differently, which is what
                // `same_name` decides over the whole of Unicode.
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

    /// A number that means nothing is refused where the file is read, so a
    /// watcher nobody is looking at never acts on one. That is the right place
    /// for it, and it has one consequence worth writing down: every command
    /// reads the registry, so a hand-edited value out of range turns all of
    /// them away — `perch config set` among them, which is otherwise the
    /// repair. The refusal has to name the file, or it is a dead end.
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

    /// What this build writes is what the file says it was written by.
    ///
    /// `load` hands back whatever version the document claimed and `save` used
    /// to write it straight back, so a file claiming something else kept
    /// claiming it through every write — about a document this build had just
    /// produced.
    #[test]
    fn a_registry_this_build_writes_says_so() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut perch = lock(&host).expect("the registry lock is free");
        let stale = Registry {
            version: 0,
            ..Registry::default()
        };

        save(&host, &mut perch, &stale).expect("it writes");

        assert_eq!(
            load(&host).expect("it reads").expect("it is there").version,
            CURRENT_VERSION
        );
    }
}
