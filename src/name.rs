//! The namespace an Alias and a Group name share, and what may be typed as one.
//!
//! Below the Registry that stores them rather than beside it: a name rule names
//! nothing above `host`, and the migration reads the same rules this build
//! enforces (ADR code-lives-where-it-reaches).

use crate::error::{PerchError, Result};

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

/// The word that addresses "no Group at all" on `perch group move`, and the
/// answer `perch add` accepts to the Group it offers. A Group cannot be called
/// this, because then one of the two meanings would be unreachable.
pub const NO_GROUP: &str = "none";

/// Whether a name is the one that means no Group at all. Not on its own outside
/// this module: [`means_the_ungrouped_scope`] is the question every command has,
/// and half of it is what refused one spelling while naming the other.
fn means_no_group(name: &str) -> bool {
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
    /// rather than stating the rule: "the Registry holds an Alias `…`".
    pub fn article(self) -> &'static str {
        match self {
            NameKind::Alias => "an Alias",
            NameKind::Group => "a Group",
        }
    }
}

/// Whether two names the user chose are the same name, through the current row.
///
/// Case-insensitively, because nobody remembers how they capitalized a Group
/// months ago — and over the whole of Unicode, because ASCII alone would fold
/// `work` and `Work` but not `café` and `CAFÉ`.
pub fn same_name(one: &str, other: &str) -> bool {
    current().one_name(one, other)
}

/// The one spelling [`same_name`] holds every other spelling of a name to.
///
/// Two names are one name exactly where this is equal, so a map keyed on it
/// answers in a lookup what asking `same_name` of everything held is a scan for.
pub fn folded(name: &str) -> String {
    current().fold.spelling(name)
}

/// A Group name offered as a default, made from something that was never chosen
/// to be one — an organization name, which commonly has spaces in it.
///
/// Only the spaces are touched, and only into the separator chosen names already
/// use. Anything else wrong with it leaves no offer at all.
pub fn offerable_name(from: &str) -> Option<String> {
    let joined = from.split_whitespace().collect::<Vec<_>>().join("-");
    validate(NameKind::Group, &joined).ok()?;
    Some(joined)
}

/// One reason a name is refused, and what a version of the rules is made of.
///
/// A rule joining this build is a variant joining here, which every `match` over
/// it reports. Each carries the set or the words it refuses against, so a row is
/// one list and cannot name data no rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Nothing, or nothing but whitespace.
    Empty,
    /// A character the terminal acts on rather than draws, from the set this
    /// version held. Frozen per version below the current one: the live set
    /// grows, and a set that grew under a version that did not is a name that
    /// version accepted and no command can now repair.
    Unshowable(&'static [(char, char)]),
    /// A character outside the allow-list a name is made of.
    NotAnIdentifier,
    /// A first character that may only follow another.
    OpensWrong,
    /// Whitespace anywhere. Refused on its own before the allow-list, which
    /// carries no whitespace and so subsumes it.
    Whitespace,
    /// An `@`, which would make a name and an email address one question.
    /// Subsumed by the allow-list for the same reason.
    LikeAnAddress,
    /// A leading `-`. Subsumed by [`Rule::OpensWrong`].
    LeadingDash,
    /// One of the words that address the Accounts in no Group.
    AddressesTheUngrouped(&'static [&'static str]),
    /// The word people say every Scope at once by.
    MeansEveryScope(&'static [&'static str]),
}

impl Rule {
    /// The order the rules are asked in, low first, so a row is a set and
    /// reordering one changes nothing. Two positions are load-bearing:
    /// `Unshowable` ahead of `NotAnIdentifier`, which lets `dev\u{FE00}` and
    /// `dev\u{3164}` through, both well-formed identifiers drawing as `dev`; and
    /// `OpensWrong` after it, so a character wrong anywhere is named as that.
    fn precedence(self) -> u8 {
        match self {
            Rule::Empty => 0,
            Rule::Unshowable(_) => 1,
            Rule::Whitespace => 2,
            Rule::LikeAnAddress => 3,
            Rule::NotAnIdentifier => 4,
            Rule::LeadingDash => 5,
            Rule::OpensWrong => 6,
            Rule::AddressesTheUngrouped(_) => 7,
            Rule::MeansEveryScope(_) => 8,
        }
    }

    /// Whether the rule lets a character sit inside a name. `None` where it is
    /// about the whole name rather than any one character. No catch-all arm, so
    /// a rule joining the enum has to answer here before it builds.
    fn keeps(self, c: char) -> Option<bool> {
        match self {
            Rule::Unshowable(set) => Some(!crate::host::within(set, c)),
            Rule::NotAnIdentifier => Some(a_name_may_carry(c)),
            Rule::Whitespace => Some(!c.is_whitespace()),
            Rule::LikeAnAddress => Some(c != '@'),
            Rule::Empty
            | Rule::OpensWrong
            | Rule::LeadingDash
            | Rule::AddressesTheUngrouped(_)
            | Rule::MeansEveryScope(_) => None,
        }
    }

    /// Whether the rule lets a character open a name. `None` where it says
    /// nothing about the first character in particular.
    fn opens(self, c: char) -> Option<bool> {
        match self {
            Rule::OpensWrong => Some(a_name_may_open_with(c)),
            Rule::LeadingDash => Some(c != '-'),
            Rule::Empty
            | Rule::Unshowable(_)
            | Rule::NotAnIdentifier
            | Rule::Whitespace
            | Rule::LikeAnAddress
            | Rule::AddressesTheUngrouped(_)
            | Rule::MeansEveryScope(_) => None,
        }
    }

    /// Whether this name breaks the rule, folded as the version folds.
    fn broken_by(self, name: &str, fold: Fold) -> bool {
        match self {
            Rule::Empty => name.trim().is_empty(),
            Rule::Unshowable(set) => name.chars().any(|c| crate::host::within(set, c)),
            Rule::NotAnIdentifier => !name.chars().all(a_name_may_carry),
            Rule::OpensWrong => name
                .chars()
                .next()
                .is_some_and(|c| !a_name_may_open_with(c)),
            Rule::Whitespace => name.chars().any(char::is_whitespace),
            Rule::LikeAnAddress => name.contains('@'),
            Rule::LeadingDash => name.starts_with('-'),
            Rule::AddressesTheUngrouped(words) | Rule::MeansEveryScope(words) => {
                words.iter().any(|word| fold.one_name(name, word))
            }
        }
    }

    /// What a name that broke it is told, in the rule's own words
    /// (ADR a-refusal-is-a-promise). Every variant answers, including the three
    /// no current row holds: a rule is a rule, and one returning to the current
    /// row would otherwise arrive without its sentence.
    fn refusal(self, kind: NameKind, name: &str) -> PerchError {
        let said = |c: Option<char>| c.map(said_as).unwrap_or_default();
        PerchError::Invalid(match self {
            Rule::Empty => format!("{} cannot be empty.", kind.names()),
            Rule::Unshowable(set) => format!(
                "{} do not carry {}.",
                kind.names(),
                name.chars()
                    .find(|c| crate::host::within(set, *c))
                    .map(
                        |found| crate::host::unshowable_character_in(&found.to_string())
                            .unwrap_or_else(|| said_as(found))
                    )
                    .unwrap_or_default(),
            ),
            Rule::NotAnIdentifier => format!(
                "`{name}` carries {}, and {} are made of letters, digits, `_` \
                 and `-`.",
                said(name.chars().find(|c| !a_name_may_carry(*c))),
                kind.names()
            ),
            Rule::OpensWrong => format!(
                "`{name}` opens with {}, and {} open with a letter, a digit or `_`.",
                said(name.chars().next().filter(|c| !a_name_may_open_with(*c))),
                kind.names()
            ),
            Rule::Whitespace => format!(
                "`{name}` carries {}, and {} carry no whitespace.",
                said(name.chars().find(|c| c.is_whitespace())),
                kind.names()
            ),
            Rule::LikeAnAddress => format!("`{name}` carries `@`, and {} do not.", kind.names()),
            Rule::LeadingDash => format!("`{name}` opens with `-`, and {} do not.", kind.names()),
            // One sentence for both spellings, because they were reserved as
            // two: a Group called `ungrouped` or `none` is one no
            // `perch config set` could reach, and an Alias is the same collision.
            Rule::AddressesTheUngrouped(_) => format!(
                "`{name}` addresses the Accounts in no Group, so it cannot also be \
                 {}.",
                kind.article()
            ),
            // The word that means something Perch does not have. A Group by that
            // name would take every later `perch config set global …` quietly.
            Rule::MeansEveryScope(_) => format!(
                "`{name}` is how people say every Scope at once, so it cannot also \
                 be {}.",
                kind.article()
            ),
        })
    }
}

/// How a version tells two names apart. History as well as a rule: what folds
/// together decides which names one Registry holds as two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// `str::to_lowercase`, which applies Greek's final-sigma rule and so holds
    /// `ΟΔΟΣ` and `οδοσ` as two names.
    Lowercase,
    /// The same, with both spellings of a Greek sigma brought to one. `Σ`
    /// lowercases to `ς` ending a word and to `σ` inside one, which is an
    /// orthographic rule about rendering Greek text — and a name is something
    /// somebody typed rather than a word.
    OneSigma,
}

impl Fold {
    /// The spelling this fold brings a name to, which is the fold as a value:
    /// [`Fold::one_name`] is two of these compared.
    fn spelling(self, name: &str) -> String {
        match self {
            Fold::Lowercase => name.to_lowercase(),
            Fold::OneSigma => one_sigma(name).collect(),
        }
    }

    /// Whether two names are one name under this fold. [`Fold::Lowercase`] is
    /// `str::to_lowercase` rather than a per-character spelling of it: the two
    /// part company at the final sigma and at `İ`, which are the characters a
    /// version turns on. [`Fold::OneSigma`] is per character, being the hot path.
    fn one_name(self, one: &str, other: &str) -> bool {
        // Ahead of both folds, because both agree with it: an ASCII character
        // lowercases to one ASCII character, and neither character the two rows
        // part company over is ASCII.
        if one.is_ascii() && other.is_ascii() {
            return one.eq_ignore_ascii_case(other);
        }
        match self {
            Fold::Lowercase => one.to_lowercase() == other.to_lowercase(),
            Fold::OneSigma => one_sigma(one).eq(one_sigma(other)),
        }
    }
}

/// Lowercase with both spellings of a Greek sigma brought to one.
fn one_sigma(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ς' { 'σ' } else { c })
}

/// The rules one version of Perch enforced, and the fold it told names apart by.
///
/// A row below the newest names nothing this build can change: it is what a
/// published Perch did, and a predicate reading live code answers for what this
/// build does instead (ADR a-registry-comes-forward).
#[derive(Debug)]
pub struct Rules {
    rules: &'static [Rule],
    fold: Fold,
}

const THE_UNGROUPED_WORDS: &[&str] = &[UNGROUPED, NO_GROUP];

const CURRENT_RULES: Rules = Rules {
    rules: &[
        Rule::Empty,
        Rule::Unshowable(crate::host::UNSHOWABLE),
        Rule::NotAnIdentifier,
        Rule::OpensWrong,
        Rule::AddressesTheUngrouped(THE_UNGROUPED_WORDS),
        Rule::MeansEveryScope(&[GLOBAL]),
    ],
    fold: Fold::OneSigma,
};

pub fn current() -> &'static Rules {
    &CURRENT_RULES
}

impl Rules {
    /// Refuses a name that could not be typed, or could not be told from
    /// something else (ADR a-target-has-to-be-typeable).
    ///
    /// The first rule broken by precedence, so which sentence a name gets does
    /// not depend on the order its row happens to be written in.
    pub fn validate(&self, kind: NameKind, name: &str) -> Result<()> {
        match self
            .rules
            .iter()
            .copied()
            .filter(|rule| rule.broken_by(name, self.fold))
            .min_by_key(|rule| rule.precedence())
        {
            Some(rule) => Err(rule.refusal(kind, name)),
            None => Ok(()),
        }
    }

    /// Whether a Perch of this version would have accepted the name — the
    /// question the rename pass asks of a Registry it is bringing forward.
    pub fn accepts(&self, name: &str) -> bool {
        !self
            .rules
            .iter()
            .any(|rule| rule.broken_by(name, self.fold))
    }

    /// Whether two names are one name to a Perch of this version.
    pub fn one_name(&self, one: &str, other: &str) -> bool {
        self.fold.one_name(one, other)
    }

    /// Whether every rule that has a view lets the character sit in a name.
    fn keeps(&self, c: char) -> bool {
        self.rules.iter().all(|rule| rule.keeps(c).unwrap_or(true))
    }

    /// The same, of the first character.
    fn opens(&self, c: char) -> bool {
        self.rules.iter().all(|rule| rule.opens(c).unwrap_or(true))
    }
}

/// Refuses a name this build could not hold, through the current row.
pub fn validate(kind: NameKind, name: &str) -> Result<()> {
    current().validate(kind, name)
}

/// The nearest name to this one that this build accepts and nothing else in the
/// namespace answers to. `None` leaves the name as it is, for the refusal at
/// `load` to describe. Here rather than in the migration that asks for it: what
/// a name may be is this module's, and `taken` is all the caller brings.
pub fn acceptable(kind: NameKind, name: &str, taken: &[String]) -> Option<String> {
    let row = current();
    // The per-character rules are per character, and no suffix rescues one, so a
    // name breaking one loses the character rather than gaining a number; the
    // reserved words are whole words and take one.
    let kept: String = name.chars().filter(|c| row.keeps(*c)).collect();
    // What is left may still open with something that may only follow: a `-`, a
    // combining mark, a digit of another script.
    let opened = kept.trim_start_matches(|c| !row.opens(c));
    let base = match opened.is_empty() {
        true => match kind {
            NameKind::Group => "group",
            NameKind::Alias => "alias",
        },
        false => opened,
    };
    (0..ENOUGH_SUFFIXES)
        .map(|at| match at {
            0 => base.to_string(),
            _ => format!("{base}-{at}"),
        })
        .find(|candidate| {
            current().accepts(candidate) && !taken.iter().any(|held| same_name(held, candidate))
        })
}

/// How many spellings of a name are tried before the rename gives up.
///
/// Bounded rather than open, so a name no suffix rescues is a refusal at `load`
/// rather than a command that never returns.
const ENOUGH_SUFFIXES: u32 = 100;

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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Both rows of `spelling`, because only the current one is reachable
    /// through [`folded`] and a row below it still has to answer for the
    /// registries it wrote. The two part company at the final sigma, which is
    /// the whole of what tells them apart.
    #[test]
    fn each_fold_brings_a_name_to_its_own_one_spelling() {
        assert_eq!(Fold::Lowercase.spelling("ΟΔΟΣ"), "οδος");
        assert_eq!(Fold::OneSigma.spelling("ΟΔΟΣ"), "οδοσ");
        for fold in [Fold::Lowercase, Fold::OneSigma] {
            assert_eq!(fold.spelling("Work"), "work");
            assert_eq!(fold.spelling("CAFÉ"), "café");
        }
    }

    /// What every map keyed on [`folded`] rests on: two names are one name
    /// exactly where their folded spellings are equal. `K` is U+212A, the
    /// Kelvin sign, which is the crossing case — not ASCII, and folding to a
    /// spelling that is.
    #[test]
    fn one_name_and_one_folded_spelling_are_the_same_question() {
        let corpus = [
            "work", "Work", "WORK", "café", "CAFÉ", "cafe", "ΟΔΟΣ", "οδος", "οδοσ", "İ", "i",
            "straße", "STRAßE", "\u{212A}", "k", "日本", "", "-", "2fa",
        ];
        for one in corpus {
            for other in corpus {
                assert_eq!(
                    same_name(one, other),
                    folded(one) == folded(other),
                    "{one:?} and {other:?}"
                );
            }
        }
    }

    /// What lets [`Fold::one_name`] answer ASCII without either fold. Exhaustive
    /// over one character, which is where a case mapping that disagreed would
    /// have to live: every ASCII character against every other, under both rows.
    #[test]
    fn both_folds_answer_ascii_exactly_as_a_bytewise_case_compare_does() {
        for one in 0u8..=127 {
            for other in 0u8..=127 {
                let (one, other) = (String::from(one as char), String::from(other as char));
                let bytewise = one.eq_ignore_ascii_case(&other);
                assert_eq!(
                    one.to_lowercase() == other.to_lowercase(),
                    bytewise,
                    "{one:?} and {other:?} under Lowercase"
                );
                assert_eq!(
                    one_sigma(&one).eq(one_sigma(&other)),
                    bytewise,
                    "{one:?} and {other:?} under OneSigma"
                );
            }
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
                // Not only at the ends: a `perch config set` is read a word
                // at a time, so no `set` could address this.
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
                let refused = validate(kind, name)
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
                assert!(validate(kind, name).is_ok(), "`{name}` should be fine");
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
                    validate(kind, name).is_ok(),
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
                let refused = validate(kind, name)
                    .expect_err(&format!("`{name}` should not be usable as a {kind:?} name"));
                assert!(
                    refused.to_string().contains(said),
                    "a {kind:?} refused `{name}` without naming {said}: {refused}"
                );
            }
        }
    }

    /// `add` picks a different question depending on whether there is an offer,
    /// so a `None` that stopped being one would put a name in front of somebody
    /// that [`validate`] refuses a keystroke later — after the browser round
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

    /// The three rules the current row does not hold, each asked of a row of its
    /// own: a rule is a rule, and one returning to the current row would
    /// otherwise arrive without the sentence it refuses in.
    #[test]
    fn a_rule_no_current_row_holds_still_refuses_in_its_own_words() {
        let row = Rules {
            rules: &[Rule::Whitespace, Rule::LikeAnAddress, Rule::LeadingDash],
            fold: Fold::Lowercase,
        };

        for (name, said) in [
            ("my work", "carry no whitespace"),
            ("someone@example.com", "carries `@`"),
            ("-dev", "opens with `-`"),
        ] {
            let refused = row
                .validate(NameKind::Group, name)
                .expect_err("the row refuses this name");
            assert!(refused.to_string().contains(said), "{refused}");
        }
        assert!(
            row.accepts("dev.ops"),
            "and no rule here is the allow-list, which is the current row's"
        );
    }

    /// The other fold, kept as a live alternative. It is `str::to_lowercase`'s,
    /// which applies Greek's final-sigma rule — an orthographic rule about
    /// rendering Greek text, where a name is something somebody typed.
    #[test]
    fn the_other_fold_holds_two_spellings_of_a_greek_name_as_two_names() {
        assert!(!Fold::Lowercase.one_name("ΟΔΟΣ", "οδοσ"));
        assert!(Fold::OneSigma.one_name("ΟΔΟΣ", "οδοσ"));
        assert!(
            Fold::Lowercase.one_name("CAFÉ", "café"),
            "and away from the sigma the two agree"
        );
    }

    /// What [`acceptable`] rests on: a rule about one character answers about a
    /// character, and a rule about the whole name answers nothing rather than
    /// `false` — which the `unwrap_or(true)` reading it would take for a refusal
    /// of every character there is.
    #[test]
    fn a_rule_answers_about_one_character_exactly_where_it_is_about_one() {
        for rule in [
            Rule::Empty,
            Rule::AddressesTheUngrouped(THE_UNGROUPED_WORDS),
            Rule::MeansEveryScope(&[GLOBAL]),
        ] {
            assert_eq!(rule.keeps('a'), None, "{rule:?}");
            assert_eq!(rule.opens('a'), None, "{rule:?}");
        }

        for (rule, refused) in [
            (Rule::Unshowable(crate::host::UNSHOWABLE), '\u{200B}'),
            (Rule::NotAnIdentifier, '.'),
            (Rule::Whitespace, ' '),
            (Rule::LikeAnAddress, '@'),
        ] {
            assert_eq!(rule.keeps(refused), Some(false), "{rule:?}");
            assert_eq!(rule.keeps('a'), Some(true), "{rule:?}");
            assert_eq!(rule.opens(refused), None, "{rule:?}");
        }

        for (rule, refused) in [(Rule::OpensWrong, '-'), (Rule::LeadingDash, '-')] {
            assert_eq!(rule.opens(refused), Some(false), "{rule:?}");
            assert_eq!(rule.opens('a'), Some(true), "{rule:?}");
            assert_eq!(rule.keeps(refused), None, "{rule:?}");
        }
    }

    /// The nearest name this build would hold. A character no rule keeps goes,
    /// because no suffix rescues one; a reserved word is whole and takes a
    /// number instead; and a name with nothing left is named for its kind.
    #[test]
    fn an_unacceptable_name_is_brought_to_the_nearest_one_perch_would_hold() {
        let free: &[String] = &[];

        assert_eq!(
            acceptable(NameKind::Group, "dev.ops", free).as_deref(),
            Some("devops")
        );
        assert_eq!(
            acceptable(NameKind::Group, "-dev", free).as_deref(),
            Some("dev"),
            "a character that may only follow is trimmed rather than kept"
        );
        assert_eq!(
            acceptable(NameKind::Group, "...", free).as_deref(),
            Some("group")
        );
        assert_eq!(
            acceptable(NameKind::Alias, "...", free).as_deref(),
            Some("alias"),
            "and which kind it is names it"
        );
        assert_eq!(
            acceptable(NameKind::Group, "none", free).as_deref(),
            Some("none-1"),
            "a reserved word is a whole word, so it gains a number"
        );
        assert_eq!(
            acceptable(NameKind::Group, "dev.ops", &["DEVOPS".to_string()]).as_deref(),
            Some("devops-1"),
            "and so does one something in the namespace already answers to"
        );
    }
}
