//! The namespace an Alias and a Group name share, and what may be typed as one.
//!
//! Below the registry that stores them rather than beside it: a name rule names
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
    // two `String`s per comparison — and `registry::validate`'s quadratic collision check
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
    validate(NameKind::Group, &joined).ok()?;
    Some(joined)
}

/// Refuses a name that could not be typed, or could not be told from something
/// else (ADR a-target-has-to-be-typeable).
///
/// An allow-list of characters, then the words that already address something.
/// No `@` is an identifier character, so an address stays tellable from a name.
pub fn validate(kind: NameKind, name: &str) -> Result<()> {
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
}
