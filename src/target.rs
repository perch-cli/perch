//! What the one Target a command is given means, decided in one place.
//!
//! Every command that takes a Target — `switch`, `run`, `remove`, `alias`,
//! `group move` — resolves it here, so a name means the same thing whichever
//! command it is typed at. The order is Alias, then Account email, then Group.
//!
//! The order can never actually break a tie: an Alias and a Group name cannot
//! collide because the registry refuses the second one, and neither may look
//! like an email address. It is fixed here anyway, because a resolution rule
//! that depends on no collisions existing is one that stops being true the day
//! a migration lets one through.
//!
//! A Target that matches nothing is the one case worth spending words on: what
//! was typed is repeated back with whatever it nearly matched, because a
//! mistyped name is far more common than an imagined one.

use crate::error::{PerchError, Result};
use crate::registry::{self, Registry};

/// What a Target turned out to name, and how it matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An Account, reached by the Alias that names it.
    Alias { name: String, email: String },
    /// An Account, reached by its own email address.
    Account { email: String },
    /// A Group, reached by its name.
    Group { name: String },
}

impl Target {
    /// How the Target matched, said the way the user would hear it and ready
    /// to print on its own line. Commands say this when they act, so "Perch
    /// understood me" is something the user reads rather than infers from the
    /// outcome.
    pub fn matched(&self) -> String {
        match self {
            Target::Alias { name, email } => format!("`{name}` is an Alias for {email}."),
            Target::Account { email } => format!("`{email}` is an Account."),
            Target::Group { name } => format!("`{name}` is a Group."),
        }
    }

    fn email(&self) -> Option<&str> {
        match self {
            Target::Alias { email, .. } | Target::Account { email } => Some(email),
            Target::Group { .. } => None,
        }
    }
}

/// A Target that named exactly one Account. Carrying the Account and how it
/// was reached together is what keeps every caller from having to ask a
/// [`Target`] for an email address it might not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountTarget {
    pub email: String,
    /// How it matched, for the command to say before it acts.
    pub matched: String,
}

/// The full order, for the commands that accept any kind of Target.
pub fn resolve(registry: &Registry, target: &str) -> Result<Target> {
    match matched(registry, target) {
        Some(target) => Ok(target),
        None => Err(nothing_called(target, every_name(registry))),
    }
}

/// The same order, for the commands whose Target has to be exactly one
/// Account. A Group is resolved rather than ignored, so naming one gets an
/// answer about the Group instead of a claim that it does not exist.
pub fn resolve_account(registry: &Registry, target: &str) -> Result<AccountTarget> {
    let found = match matched(registry, target) {
        Some(found) => found,
        None => return Err(nothing_called(target, account_names(registry))),
    };
    match found.email() {
        Some(email) => Ok(AccountTarget {
            email: email.to_string(),
            matched: found.matched(),
        }),
        None => Err(PerchError::Invalid(format!(
            "{} This acts on one Account, so name the Account itself: \
             its Alias, or its email address.",
            found.matched()
        ))),
    }
}

/// Matched however it was capitalised, because that is the rule the names were
/// made under.
///
/// The registry refuses an Alias or a Group that differs from a held name only
/// in case — "nobody remembers which way they capitalised a Group months ago",
/// as it puts it — so there is never more than one candidate to find. An exact
/// lookup here made every command that *resolves* a Target stricter than every
/// command that *sets* one: `perch group add Work` then `perch switch work`
/// said nothing was called `work`, while `perch config set work …` and `perch
/// group remove work` both accepted it. Emails diverged the same way, against
/// a `switch::already_landed` that compares them case-insensitively.
fn matched(registry: &Registry, target: &str) -> Option<Target> {
    if let Some((name, email)) = registry.declared_alias(target) {
        return Some(Target::Alias {
            name: name.to_string(),
            email: email.to_string(),
        });
    }
    // The same comparison Aliases and Group names get, and the same one a
    // Profile is derived under: `registry::slug` lowercases the whole of
    // Unicode, so `CAFÉ@example.com` and `café@example.com` already share one
    // Profile and `perch add` already refuses the second as a collision. Asked
    // in ASCII here, this was the one place that disagreed — a held
    // `café@example.com` could not be reached by typing it with a capital É,
    // and the refusal said Perch holds nothing by that name about an Account it
    // holds and would not let you have two of.
    if let Some(account) = registry
        .accounts
        .iter()
        .find(|held| registry::same_name(held.email(), target))
    {
        return Some(Target::Account {
            email: account.email().to_string(),
        });
    }
    if let Some(name) = registry.declared_group(target) {
        return Some(Target::Group {
            name: name.to_string(),
        });
    }
    None
}

/// Every name a Target could have been.
fn every_name(registry: &Registry) -> Vec<String> {
    let mut names = account_names(registry);
    names.extend(registry.groups.keys().cloned());
    names
}

fn account_names(registry: &Registry) -> Vec<String> {
    let mut names: Vec<String> = registry.aliases.keys().cloned().collect();
    names.extend(
        registry
            .accounts
            .iter()
            .map(|account| account.email().to_string()),
    );
    names
}

/// Every suggestion is a name that would have worked, so it can be typed
/// straight back rather than translated first.
fn nothing_called(target: &str, candidates: Vec<String>) -> PerchError {
    let help = match suggestion(&candidates, target) {
        Some(suggestion) => suggestion,
        None => "`perch group list` shows every Account Perch holds, and the Groups.".to_string(),
    };
    PerchError::NotFound(format!("Nothing Perch holds is called `{target}`. {help}"))
}

/// What the user probably meant, if anything they hold is close enough to be
/// worth guessing at. Shared with the commands that take a name of one
/// particular kind, so a typo reads the same wherever it is made.
pub fn suggestion(candidates: &[String], typed: &str) -> Option<String> {
    let near = near_matches(candidates, typed);
    (!near.is_empty()).then(|| {
        format!(
            "Did you mean {}?",
            near.iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(" or ")
        )
    })
}

/// How many single-character mistakes a name of this length may be out by and
/// still be worth suggesting. Short names are held to a stricter standard,
/// because at three characters almost everything is two edits from everything.
fn allowed_mistakes(name: &str) -> usize {
    match name.chars().count() {
        0..=3 => 1,
        4..=7 => 2,
        _ => 3,
    }
}

/// The names close enough to what was typed to be worth offering, closest
/// first — at most three, so the refusal stays readable.
fn near_matches(candidates: &[String], target: &str) -> Vec<String> {
    let typed = target.to_lowercase();
    let mut scored: Vec<((bool, usize), &String)> = candidates
        .iter()
        .filter_map(|candidate| {
            let name = candidate.to_lowercase();
            let distance = edit_distance(&name, &typed);
            // A name the typed Target starts is a suggestion whatever the
            // distance says: somebody typing `over` for `overflow@example.com`
            // has not made a mistake so much as stopped early.
            let started = name.starts_with(&typed) && typed.chars().count() >= 3;
            (started || distance <= allowed_mistakes(&typed))
                .then_some(((!started, distance), candidate))
        })
        .collect();
    // In its own bucket, ahead of everything, because the sort is what the rule
    // above needed and did not have. Ranked on the raw distance, `over` put
    // `over1`, `over2` and `over3` — one edit each — ahead of
    // `overflow@example.com` at sixteen, and the list is cut at three: the one
    // candidate the rule was written for was the one it dropped.
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate.clone())
        .collect()
}

/// Levenshtein distance, one row of the matrix at a time. The strings compared
/// here are names somebody typed, so the quadratic cost is on nothing.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (row, left_char) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, &right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            let deletion = previous[column + 1] + 1;
            let insertion = current[column] + 1;
            current[column + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_distance_is_the_number_of_single_character_mistakes() {
        assert_eq!(edit_distance("work", "work"), 0);
        assert_eq!(edit_distance("overflow", "overflw"), 1);
        assert_eq!(edit_distance("work", "wrok"), 2);
        assert_eq!(edit_distance("", "work"), 4);
    }

    #[test]
    fn a_near_match_is_offered_and_an_unrelated_name_is_not() {
        let names = vec![
            "overflow".to_string(),
            "work".to_string(),
            "someone@example.com".to_string(),
        ];
        assert_eq!(near_matches(&names, "overflw"), vec!["overflow"]);
        assert_eq!(near_matches(&names, "Work"), vec!["work"]);
        assert!(near_matches(&names, "zzzzzz").is_empty());
    }

    #[test]
    fn a_name_the_target_starts_is_offered_however_long_it_is() {
        let names = vec!["someone@example.com".to_string()];
        assert_eq!(near_matches(&names, "someone"), vec!["someone@example.com"]);
        assert!(
            near_matches(&names, "so").is_empty(),
            "two characters is not enough of a start to guess from"
        );
    }

    /// "However long it is" was a claim the filter made and the sort took back.
    /// Ranked on the raw edit distance, a name the Target starts sinks in
    /// proportion to how much of it is left to type, and the list is cut at
    /// three — so `over` offered `over1`, `over2` and `over3`, one edit each,
    /// and dropped the `overflow@example.com` the rule exists for at sixteen.
    #[test]
    fn a_name_the_target_starts_outranks_a_shorter_name_it_does_not() {
        let names = vec![
            "overflow@example.com".to_string(),
            "aver".to_string(),
            "oves".to_string(),
            "ever".to_string(),
        ];

        let offered = near_matches(&names, "over");

        assert!(
            offered.contains(&"overflow@example.com".to_string()),
            "somebody typing `over` has stopped early rather than made sixteen \
             mistakes, and three one-edit names must not crowd it out: \
             {offered:?}"
        );
    }
}
