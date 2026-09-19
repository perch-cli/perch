//! What the one Target a command is given means, decided in one place.
//!
//! Every command that takes a Target — `switch`, `run`, `remove`, `alias`,
//! `group move` — resolves it here, so a name means the same thing whichever
//! command it is typed at. The order is Alias, then Account email, then Group.
//!
//! That order can never break a tie: an Alias and a Group name cannot collide
//! because the Registry refuses the second, and neither may look like an email
//! address. It is fixed anyway, because a resolution rule that depends on no
//! collisions existing stops being true the day a migration lets one through.

use crate::error::{PerchError, Result};
use crate::registry::Registry;

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
    /// How the Target matched, on its own line: which of three kinds a name
    /// turned out to be is the part nobody could have predicted
    /// (ADR perch-says-what-it-did). The *held* spelling, not what was typed.
    pub fn matched(&self) -> String {
        match self {
            Target::Alias { name, email } => format!("`{name}` is an Alias for {email}."),
            Target::Account { email } => format!("`{email}` is an Account."),
            Target::Group { name } => format!("`{name}` is a Group."),
        }
    }

    fn key(&self) -> Option<&str> {
        match self {
            Target::Alias { email, .. } | Target::Account { email } => Some(email),
            Target::Group { .. } => None,
        }
    }
}

/// A Target that named exactly one Account. Carrying the Account and how it was
/// reached together is what keeps every caller from asking a [`Target`] for an
/// email address it might not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountTarget {
    pub email: String,
    /// How it matched, for the command to say before it acts.
    pub matched: String,
}

/// The full order, for the commands that accept any kind of Target.
pub fn resolve(registry: &Registry, target: &str) -> Result<Target> {
    match matched(registry, target)? {
        Some(target) => Ok(target),
        None => Err(nothing_called(target, every_name(registry))),
    }
}

/// The same order, for the commands whose Target has to be exactly one Account.
/// A Group is resolved rather than ignored, so naming one gets an answer about
/// the Group instead of a claim that it does not exist.
pub fn resolve_account(registry: &Registry, target: &str) -> Result<AccountTarget> {
    let found = match matched(registry, target)? {
        Some(found) => found,
        None => return Err(nothing_called(target, account_names(registry))),
    };
    match found.key() {
        Some(email) => Ok(AccountTarget {
            email: email.to_string(),
            matched: match &found {
                Target::Alias { name, .. } => format!(
                    "`{name}` is an Alias for {}.",
                    registry.held(email)?.email()
                ),
                _ => format!(
                    "`{target}` is an Account: {}.",
                    registry.named_for_the_user(email)
                ),
            },
        }),
        None => Err(PerchError::Invalid(format!(
            "{} Name one Account: its Alias, or its email address.",
            found.matched()
        ))),
    }
}

/// Matched however it was capitalized, because that is the rule the names were
/// made under: the Registry refuses an Alias or a Group differing from a held
/// name only in case, so there is never more than one candidate to find. An
/// exact lookup here would make resolving a Target stricter than setting one.
fn matched(registry: &Registry, target: &str) -> Result<Option<Target>> {
    if let Some((name, email)) = registry.declared_alias(target) {
        return Ok(Some(Target::Alias {
            name: name.to_string(),
            email: email.to_string(),
        }));
    }
    let accounts: Vec<_> = registry
        .accounts
        .iter()
        .filter(|account| crate::name::same_name(account.email(), target))
        .collect();
    if accounts.len() > 1 {
        return Err(PerchError::Invalid(format!(
            "{target} names more than one Account. Name one by its Alias: {}.",
            accounts
                .iter()
                .map(|account| registry.alias_of(account.key()).unwrap_or(account.key()))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(account) = accounts
        .first()
        .copied()
        .or_else(|| registry.account(target))
    {
        return Ok(Some(Target::Account {
            email: account.key().to_string(),
        }));
    }
    if let Some(name) = registry.declared_group(target) {
        return Ok(Some(Target::Group {
            name: name.to_string(),
        }));
    }
    Ok(None)
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
            .map(|account| account.key().to_string()),
    );
    names.extend(
        registry
            .accounts
            .iter()
            .filter(|account| {
                registry
                    .accounts
                    .iter()
                    .filter(|peer| crate::name::same_name(peer.email(), account.email()))
                    .count()
                    == 1
            })
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

/// What the user probably meant, where anything they hold is close enough to be
/// worth guessing at. Shared with the commands that take a name of one kind, so
/// a typo reads the same wherever it is made.
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
    // A started name sorts in its own bucket first, because on raw distance a
    // long address loses to any three-letter near-miss and the list is cut at
    // three.
    scored.sort();
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate.clone())
        .collect()
}

/// Levenshtein distance, one row of the matrix at a time. The strings are names
/// somebody typed, so the quadratic cost is on nothing.
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

/// A Target under a provider flag. An email can name one Account per provider
/// (ADR an-account-has-a-workspace), so the flag picks among them; an Alias
/// names one Account whatever the flag says, and a flag it disagrees with is a
/// refusal on both paths rather than a Switch to the other provider.
pub fn resolve_for(
    registry: &Registry,
    target: &str,
    provider: Option<crate::providers::provider::Id>,
) -> Result<AccountTarget> {
    let found = match within_provider(registry, target, provider)? {
        Some(found) => found,
        None => resolve_account(registry, target)?,
    };
    let account = registry.held(&found.email)?;
    if let Some(selected) = provider
        && selected != account.provider()
    {
        return Err(PerchError::Invalid(format!(
            "{target} is a {} Account. `--{}` selects it.",
            account.provider().adapter().name(),
            account.provider().word()
        )));
    }
    Ok(found)
}

/// The one Account this email names among the provider's, or nothing where the
/// Target is an Alias or no Account of that provider has the address.
fn within_provider(
    registry: &Registry,
    target: &str,
    provider: Option<crate::providers::provider::Id>,
) -> Result<Option<AccountTarget>> {
    if registry.declared_alias(target).is_some() {
        return Ok(None);
    }
    let matches: Vec<_> = registry
        .accounts
        .iter()
        .filter(|account| {
            crate::name::same_name(account.email(), target)
                && provider.is_none_or(|selected| selected == account.provider())
        })
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [account] => Ok(Some(AccountTarget {
            email: account.key().into(),
            matched: format!(
                "`{target}` is an Account: {}.",
                registry.named_for_the_user(account.key())
            ),
        })),
        _ => Err(PerchError::Invalid(format!(
            "{target} names more than one Account. Name one by its Alias: {}.",
            matches
                .iter()
                .map(|account| registry.alias_of(account.key()).unwrap_or(account.key()))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Registry holding one Account, one Alias for it, and one Group: the
    /// three kinds of name a Target can be, so every branch of the order is
    /// there to be taken.
    fn holding() -> Registry {
        let mut registry = Registry::default();
        registry.upsert(crate::cycle::tests::account("someone@example.com", vec![]));
        registry
            .declare_group("work")
            .expect("`work` is a usable Group name");
        registry
            .aliases
            .insert("mine".to_string(), registry.accounts[0].key().to_string());
        registry
    }

    #[test]
    fn a_target_says_which_of_the_three_kinds_of_name_it_turned_out_to_be() {
        let registry = holding();

        assert_eq!(
            resolve(&registry, "mine")
                .expect("the Alias names it")
                .matched(),
            format!("`mine` is an Alias for {}.", registry.accounts[0].key())
        );
        assert_eq!(
            resolve(&registry, "someone@example.com")
                .expect("the address names it")
                .matched(),
            format!("`{}` is an Account.", registry.accounts[0].key())
        );
        assert_eq!(
            resolve(&registry, "work")
                .expect("the Group is declared")
                .matched(),
            "`work` is a Group."
        );
    }

    #[test]
    fn a_name_nothing_holds_is_refused_with_every_name_a_target_could_have_been() {
        let refused = resolve(&holding(), "wrok").expect_err("nothing is called that");

        let said = refused.to_string();
        assert!(said.contains("work"), "the Group is a candidate: {said}");
        assert!(
            said.contains("wrok"),
            "and the refusal quotes what was typed: {said}"
        );
    }

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

    /// The fixture is three one-edit names against one sixteen edits away: on
    /// raw distance they crowd out the long name the start rule exists for,
    /// because the list is cut at three.
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
