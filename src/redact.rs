//! Placeholders for everything Perch holds that names a person
//! (ADR a-trail-is-evidence).
//!
//! One door: every line `perch probe` prints goes through [`Redaction::text`],
//! so an email address that reaches the output is a line that went round it
//! rather than a rule somebody forgot at a site
//! (ADR an-invariant-gets-a-door).

use std::path::Path;

use crate::registry::Registry;

/// What an Account, a Group or a home directory is called in a rendering that
/// leaves this machine.
///
/// Numbered by the Account's place in the Registry rather than by the order
/// names are met, so `<account 3>` is the same Account in every rendering.
pub struct Redaction {
    /// Longest first, so a replacement never lands inside one already made:
    /// an Alias of `work` and a Group of `work-eu` overlap, and the short one
    /// replacing first would leave `<account 1>-eu`.
    named: Vec<Named>,
    home: Option<String>,
    /// `--raw`: every method answers what it was given. A flag rather than a
    /// second type, because the callers are renderers and a renderer that has
    /// to know which one it holds is one that can hold the wrong one.
    on: bool,
}

struct Named {
    raw: String,
    stands_for: String,
    kind: Kind,
}

/// What a Trail line keeps once the value is gone. Which kind matched is already
/// something Perch says when it acts on a Target, and it survives redaction
/// because it names no one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An Account's own address, which the placeholder already reads as.
    Email,
    Alias,
    Group,
}

/// What a name that reached the output without a placeholder becomes: an
/// Account the Registry no longer holds, most often a `remove` earlier in the
/// Trail. It cannot be numbered, because nothing on the machine still says
/// which Account it was.
const NOT_HELD: &str = "<an account no longer held>";

impl Redaction {
    pub fn of(registry: &Registry, home: Option<&Path>) -> Redaction {
        let mut named = Vec::new();
        for (at, account) in registry.accounts.iter().enumerate() {
            let stands_for = format!("<account {}>", at + 1);
            named.push(Named {
                raw: account.identity.email.clone(),
                stands_for: stands_for.clone(),
                kind: Kind::Email,
            });
            for (alias, email) in &registry.aliases {
                if email == &account.identity.email {
                    named.push(Named {
                        raw: alias.clone(),
                        stands_for: stands_for.clone(),
                        kind: Kind::Alias,
                    });
                }
            }
        }
        for (at, group) in registry.groups.keys().enumerate() {
            named.push(Named {
                raw: group.clone(),
                stands_for: format!("<group {}>", at + 1),
                kind: Kind::Group,
            });
        }
        named.sort_by_key(|named| std::cmp::Reverse(named.raw.len()));

        Redaction {
            named,
            home: home.map(|path| path.display().to_string()),
            on: true,
        }
    }

    /// `perch probe --raw`.
    pub fn none() -> Redaction {
        Redaction {
            named: Vec::new(),
            home: None,
            on: false,
        }
    }

    /// A line on its way out, with every name Perch holds taken out of it.
    pub fn text(&self, text: &str) -> String {
        if !self.on {
            return text.to_owned();
        }
        let mut said = text.to_owned();
        // Before the names, so a home directory carrying an Alias is not left
        // half replaced: `/home/you/.config/perch/profiles/work`.
        if let Some(home) = &self.home {
            said = said.replace(home.as_str(), "<home>");
        }
        for named in &self.named {
            said = replace_whole(&said, &named.raw, &named.stands_for);
        }
        anything_else_that_is_an_address(&said)
    }

    /// One word as the Trail holds it: the value goes and the kind stays, so
    /// `switch <account 3> (alias)` still says the two things that debug a
    /// Target — which Account it reached, and how it got there.
    pub fn word(&self, word: &str) -> String {
        if !self.on {
            return word.to_owned();
        }
        match self.named.iter().find(|named| named.raw == word) {
            Some(named) => match named.kind {
                Kind::Email => named.stands_for.clone(),
                Kind::Alias => format!("{} (alias)", named.stands_for),
                Kind::Group => named.stands_for.clone(),
            },
            None => self.text(word),
        }
    }

    pub fn path(&self, path: &Path) -> String {
        self.text(&path.display().to_string())
    }
}

/// Whether a character is one a name can carry, for either end of a match.
///
/// `-` and `_` are in it so an Alias of `work` is not replaced inside `work-eu`;
/// `.` and `@` are not, so an address at the end of a sentence still is.
fn part_of_a_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_')
}

/// Replaces whole names and nothing inside a longer word.
///
/// A bare substring replacement turns a Group called `us` loose in the sentence
/// around it: "a Switch to it ref`<group 1>`es, beca`<group 1>`e Perch
/// `<group 1>`es it". The name is what is redacted, not the letters in it.
fn replace_whole(haystack: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let mut said = String::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(at) = rest.find(needle) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + needle.len()..].chars().next();
        let whole = !before.is_some_and(part_of_a_name) && !after.is_some_and(part_of_a_name);

        said.push_str(&rest[..at]);
        match whole {
            true => said.push_str(with),
            false => said.push_str(needle),
        }
        rest = &rest[at + needle.len()..];
    }
    said.push_str(rest);
    said
}

/// Whatever still looks like an email address once the known names are gone.
///
/// The safety net rather than the rule: an Account removed last week is in the
/// Trail and in no Registry, so nothing above can name it. A run of the
/// characters an address is written with, holding an `@` with a `.` after it.
fn anything_else_that_is_an_address(text: &str) -> String {
    let addressy = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '@');

    let mut said = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        // By character rather than by byte: `rfind` answers where a character
        // starts, and one past that is inside it wherever it is not ASCII —
        // which a smart-quoted paste in the Trail is.
        let opens = rest[..at]
            .char_indices()
            .rev()
            .find(|(_, c)| !addressy(*c))
            .map_or(0, |(before, c)| before + c.len_utf8());
        let closes = rest[at..]
            .char_indices()
            .find(|(_, c)| !addressy(*c))
            .map_or(rest.len(), |(ends, _)| at + ends);

        match rest[at..closes].contains('.') && at > opens {
            true => {
                said.push_str(&rest[..opens]);
                said.push_str(NOT_HELD);
            }
            // An `@` with no domain after it is somebody's prose.
            false => said.push_str(&rest[..closes]),
        }
        rest = &rest[closes..];
    }
    said.push_str(rest);
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Identity;
    use crate::registry::{Account, Registry};

    fn holding(emails: &[&str], aliases: &[(&str, &str)], groups: &[&str]) -> Registry {
        let mut registry = Registry::default();
        for email in emails {
            registry.accounts.push(Account {
                identity: Identity {
                    email: (*email).to_string(),
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
        for (alias, email) in aliases {
            registry
                .aliases
                .insert((*alias).to_string(), (*email).to_string());
        }
        for group in groups {
            registry
                .groups
                .insert((*group).to_string(), Default::default());
        }
        registry
    }

    #[test]
    fn an_account_is_numbered_by_its_place_and_reached_by_either_name() {
        let registry = holding(
            &["one@example.com", "two@example.com"],
            &[("spare", "two@example.com")],
            &[],
        );
        let hidden = Redaction::of(&registry, None);

        assert_eq!(
            hidden.text("two@example.com is next"),
            "<account 2> is next"
        );
        assert_eq!(hidden.word("spare"), "<account 2> (alias)");
        assert_eq!(hidden.word("two@example.com"), "<account 2>");
    }

    /// A short name inside a longer one: replacing the short one first would
    /// leave `<account 1>-eu` where a Group name was.
    #[test]
    fn the_longer_name_is_replaced_first() {
        let registry = holding(
            &["one@example.com"],
            &[("work", "one@example.com")],
            &["work-eu"],
        );
        let hidden = Redaction::of(&registry, None);

        assert_eq!(hidden.text("work-eu"), "<group 1>");
    }

    #[test]
    fn an_address_nothing_can_number_is_still_taken_out() {
        let hidden = Redaction::of(&holding(&[], &[], &[]), None);

        assert_eq!(
            hidden.text("removed gone@example.com"),
            format!("removed {NOT_HELD}")
        );
    }

    /// An `@` that is not an address is somebody's prose, and prose is what a
    /// finding is made of.
    #[test]
    fn an_at_sign_with_no_domain_after_it_is_left_alone() {
        let hidden = Redaction::of(&holding(&[], &[], &[]), None);

        assert_eq!(hidden.text("held @ 80%"), "held @ 80%");
        assert_eq!(hidden.text("read @example"), "read @example");
    }

    /// A smart-quoted paste reaches the Trail, and every later Probe reads it.
    #[test]
    fn a_name_that_is_not_ascii_beside_an_address_is_not_sliced_through() {
        let hidden = Redaction::of(&holding(&[], &[], &[]), None);

        assert_eq!(
            hidden.text("switch \u{201c}gone@example.com\u{201d}"),
            format!("switch \u{201c}{NOT_HELD}\u{201d}")
        );
        assert_eq!(hidden.text("naïve@example.com"), NOT_HELD);
    }

    /// The name is redacted, not the letters in it.
    #[test]
    fn a_short_name_is_not_replaced_inside_a_longer_word() {
        let registry = holding(&["one@example.com"], &[], &["us"]);
        let hidden = Redaction::of(&registry, None);

        assert_eq!(
            hidden.text("a Switch to it refuses, because Perch uses it"),
            "a Switch to it refuses, because Perch uses it"
        );
        assert_eq!(hidden.text("the us group"), "the <group 1> group");
    }

    /// An Alias inside a longer name is not that Alias.
    #[test]
    fn a_name_a_hyphen_extends_is_left_alone() {
        let registry = holding(&["one@example.com"], &[("work", "one@example.com")], &[]);
        let hidden = Redaction::of(&registry, None);

        assert_eq!(hidden.text("work-eu"), "work-eu");
        assert_eq!(hidden.text("switch work."), "switch <account 1>.");
    }

    /// A Group reaches `word` the same way an Account does — a Target is either
    /// — and it keeps its own kind rather than borrowing the Alias's suffix.
    #[test]
    fn a_group_named_as_a_word_is_the_group_it_names() {
        let registry = holding(
            &["one@example.com"],
            &[("spare", "one@example.com")],
            &["work"],
        );
        let hidden = Redaction::of(&registry, None);

        assert_eq!(hidden.word("work"), "<group 1>");
        assert_eq!(hidden.word("spare"), "<account 1> (alias)");
    }

    /// A Registry an older Perch wrote can name a Group nothing: matched
    /// literally it stands between every pair of characters in the sentence.
    #[test]
    fn a_name_with_nothing_in_it_matches_nothing() {
        let hidden = Redaction::of(&holding(&[], &[], &[""]), None);

        assert_eq!(
            hidden.text("a Switch to it refuses"),
            "a Switch to it refuses"
        );
    }

    #[test]
    fn raw_answers_what_it_was_given() {
        let hidden = Redaction::none();

        assert_eq!(hidden.text("one@example.com"), "one@example.com");
        assert_eq!(hidden.word("spare"), "spare");
    }
}
