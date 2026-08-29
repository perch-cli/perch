//! What a Listing *is*: one [`Section`] per Scope, each carrying whether its
//! order is a ranking Perch would stand behind or Accounts merely held
//! (ADR the-listing-owns-the-set). Asked once here, it travels with the
//! Accounts rather than being worked out again by whoever draws them.
//!
//! What `perch list` *does* — the breadth, the table, the sentences under it —
//! is [`crate::commands::list`]'s. What is here `perch status` reaches too, and
//! [`document`] is the one shape both write an Account in. Not on
//! [`crate::registry::Account`]: this is the first place [`crate::cycle`] and
//! [`crate::utilization`] can both be reached (ADR code-lives-where-it-reaches).

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::config;
use crate::cycle;
use crate::host::Host;
use crate::registry::{self, Account, Quarantine, Registry};
use crate::reserve::Reserve;
use crate::utilization;

/// One Scope's Accounts as a Listing shows them: ranked where a Cycle could
/// happen in the Scope, in the order they were added where one could not
/// (ADR a-group-is-a-declaration). Its fields are its own, because a renderer
/// reaching past [`Section::document`], [`Section::reserve`] and [`flattened`]
/// could sort them — and the order is the whole of what this carries.
pub struct Section<'a> {
    scope: config::Scope,
    ranked: bool,
    accounts: Vec<&'a Account>,
}

impl<'a> Section<'a> {
    pub fn of(registry: &'a Registry, scope: config::Scope, now: DateTime<Utc>) -> Section<'a> {
        let ranked = cycle::may_cycle_within(registry, &scope);
        let accounts = match ranked {
            true => cycle::ranked(registry, &scope, now),
            false => scope.accounts(registry),
        };
        Section {
            scope,
            ranked,
            accounts,
        }
    }

    /// What the order is, in the word the guide explains it by, so a script
    /// branches on the same term a person reads.
    fn order(&self) -> &'static str {
        match self.ranked {
            true => "ranked",
            false => "held",
        }
    }

    /// What this Scope has left to draw on, or nothing where nothing has
    /// declared its Accounts a set. Off the same field the order is: ranking a
    /// set and saying what it has left between them are the same claim, so they
    /// are declined together rather than by two answers that could differ.
    pub fn reserve<'r>(&self, registry: &'r Registry) -> Option<Reserve<'r>> {
        self.ranked.then(|| Reserve::of(registry, &self.scope))
    }

    pub fn document(
        &self,
        host: &dyn Host,
        registry: &Registry,
        alias_of: &registry::AliasOf<'_>,
        now: DateTime<Utc>,
    ) -> serde_json::Value {
        let listed: Vec<serde_json::Value> = self
            .accounts
            .iter()
            .map(|account| document(host, registry, alias_of, account, now))
            .collect();
        json!({
            // The same shape the document's own `scope` key carries, because it
            // is the same question asked of a narrower set: a script that reads
            // one should not have to learn a second spelling to read the other.
            "scope": scope_json(&self.scope),
            "order": self.order(),
            // At every breadth, unlike the table: the section has already
            // named the Scope. `null` for a Scope holding nobody, told from the
            // other `null` by the empty `accounts` beside it.
            "reserve": match self.accounts.is_empty() {
                true => serde_json::Value::Null,
                false => self
                    .reserve(registry)
                    .map_or(serde_json::Value::Null, |reserve| reserve.document()),
            },
            "accounts": listed,
        })
    }
}

/// Every Section's Accounts end to end, for the renderer that draws one table.
///
/// Here rather than at that renderer so a Section's Accounts stay its own. The
/// listing is the sections one after another, and this says so in the one place
/// that could say it differently.
pub fn flattened<'a>(sections: &[Section<'a>]) -> Vec<&'a Account> {
    sections
        .iter()
        .flat_map(|section| section.accounts.iter().copied())
        .collect()
}

/// A Scope as a script reads it: one spelling whether it is the breadth asked
/// for or the Scope a Section holds, so a script reading one needs no second
/// shape for the other. The arm no Cycle's Scope has — every Scope at once — is
/// added by [`crate::commands::list`], where it is the only place it can mean
/// anything.
pub fn scope_json(scope: &config::Scope) -> serde_json::Value {
    match scope {
        config::Scope::Group(name) => json!({"kind": "group", "name": name}),
        config::Scope::Ungrouped => json!({"kind": "ungrouped", "name": serde_json::Value::Null}),
    }
}

/// Every Scope, the active Account's first, because it is where you are and the
/// one a bare `perch switch` looks in. These partition the registry, and what
/// keeps them partitioning it is `load` declaring any Group an Account claims:
/// one claiming a Group nothing declared would be in no Listing at all.
pub fn scopes(registry: &Registry) -> Vec<config::Scope> {
    let mut every: Vec<config::Scope> = registry
        .group_names()
        .map(|name| config::Scope::Group(name.to_string()))
        .collect();
    every.push(config::Scope::Ungrouped);

    let Some(active) = registry
        .active()
        .whose()
        .and_then(|email| registry.account(email))
    else {
        return every;
    };
    let here = match &active.group {
        Some(name) => config::Scope::Group(name.clone()),
        None => config::Scope::Ungrouped,
    };
    let mut ordered = vec![here.clone()];
    ordered.extend(every.into_iter().filter(|scope| *scope != here));
    ordered
}

/// One Account as a script reads it, wherever it is read — so a script written
/// against `perch list --json` can be pointed at `perch status --json`. What
/// each *document* answers still differs, and that is the part that should; it
/// is the Account itself that has no business being two things.
pub fn document(
    host: &dyn Host,
    registry: &Registry,
    alias_of: &registry::AliasOf<'_>,
    account: &Account,
    now: DateTime<Utc>,
) -> serde_json::Value {
    json!({
        "email": account.email(),
        "account_uuid": account.identity.account_uuid,
        "alias": alias_of.account(account.email()),
        "group": account.group,
        // Present on every Account, unlike the cell above it: a script made to
        // test for a key's presence to learn a bool has a worse contract rather
        // than a truer one (ADR perch-says-what-it-did).
        "disabled": account.disabled,
        "quarantined": Quarantine::document(account.quarantine),
        "active": registry.active().is_active(account.email()),
        "organization": account.identity.organization_name,
        "plan": account.plan,
        // `ok()` rather than `?`, because an address no directory can be named
        // after is a state Perch describes; lossy, because `json!` unwraps a
        // `Path` that is not UTF-8.
        "profile_dir": account
            .profile_dir(host)
            .ok()
            .map(|at| at.to_string_lossy().into_owned()),
        // The figure the section's order was made on. A section saying it is
        // `ranked` without it would be a claim with no way of checking it.
        "headroom": cycle::headroom_document(account),
        "utilization": utilization::document(account, now),
    })
}

/// What the Accounts in no Group are shown under. Being in no Group is not a
/// Group, so this never reads like a Group's name.
pub const IN_NO_GROUP: &str = "In no Group";

#[cfg(test)]
mod tests {
    use super::*;

    /// Why `document` spells a Profile path lossily. `json!` expands a
    /// non-literal to `to_value(&expr).unwrap()`, and a `Path` holding bytes
    /// that are not UTF-8 serializes to an error — so the `--json` surfaces
    /// would abort on a `HOME` the real Host takes as an `OsString` and passes
    /// through where `env_var` would have refused it.
    #[test]
    #[cfg(unix)]
    fn a_path_that_is_not_text_is_spelled_rather_than_serialized() {
        use std::os::unix::ffi::OsStrExt;

        let at = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            b"/Users/\xffsomeone/.config/perch",
        ));

        assert!(
            serde_json::to_value(&at).is_err(),
            "the raw path is what `json!` would have unwrapped"
        );
        assert!(
            serde_json::to_value(at.to_string_lossy()).is_ok(),
            "and the spelling is what it is handed instead"
        );
    }

    /// A registry holding four Accounts across two Groups and none, active on
    /// the ungrouped one.
    fn holdings() -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("the name is free");
        registry.declare_group("play").expect("the name is free");
        for (email, group) in [
            ("a@example.com", Some("work")),
            ("b@example.com", Some("play")),
            ("c@example.com", None),
            ("d@example.com", Some("work")),
        ] {
            let mut account = cycle::tests::account(email, vec![]);
            account.group = group.map(str::to_string);
            registry.upsert(account);
        }
        registry.settle(Some("c@example.com".to_string()));
        registry
    }

    fn every_section(registry: &Registry) -> Vec<Section<'_>> {
        scopes(registry)
            .into_iter()
            .map(|scope| Section::of(registry, scope, cycle::tests::now()))
            .collect()
    }

    fn named(mut accounts: Vec<&Account>) -> Vec<String> {
        accounts.sort_by_key(|account| account.email().to_string());
        accounts
            .into_iter()
            .map(|account| account.email().to_string())
            .collect()
    }

    /// The partition itself is `registry`'s invariant and is asserted there.
    /// What is claimed here is that a Listing depends on it: an Account falling
    /// between two Scopes is simply not printed, with nothing anywhere to say
    /// so.
    #[test]
    fn the_sections_hold_every_account_the_registry_holds() {
        let registry = holdings();
        assert_eq!(
            named(flattened(&every_section(&registry))),
            named(registry.accounts.iter().collect()),
        );
    }

    /// The fixture is hand-edited in, because `registry::validate` does not
    /// turn such an address away and `purge::forget_the_credential` guards it as
    /// reachable that way — so it is a state Perch has to describe.
    #[test]
    fn an_account_no_profile_can_be_named_for_is_listed_rather_than_failing_the_listing() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut registry = Registry::default();
        registry.upsert(crate::registry::Account {
            identity: crate::probe::Identity {
                email: "@".to_string(),
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
        let account = registry.account("@").expect("hand-edited in");

        let listed = document(
            &host,
            &registry,
            &registry.aliases_by_account(),
            account,
            chrono::Utc::now(),
        );

        assert_eq!(listed["email"], "@");
        assert_eq!(
            listed["profile_dir"],
            serde_json::Value::Null,
            "said as the one thing that is not knowable rather than as a \
             refusal about the whole listing"
        );
    }

    #[test]
    fn the_scope_the_active_account_is_in_leads() {
        let registry = holdings();
        assert_eq!(scopes(&registry).first(), Some(&config::Scope::Ungrouped));

        let mut in_a_group = holdings();
        in_a_group.settle(Some("b@example.com".to_string()));
        assert_eq!(
            scopes(&in_a_group).first(),
            Some(&config::Scope::Group("play".to_string())),
        );
    }

    /// Leading with a Scope is a reordering rather than a promotion, and a
    /// Scope that dropped out would take its Accounts with it.
    #[test]
    fn leading_with_a_scope_neither_drops_one_nor_repeats_it() {
        let mut every = scopes(&holdings());
        every.sort_by_key(|scope| scope.word().to_string());
        assert_eq!(
            every,
            vec![
                config::Scope::Group("play".to_string()),
                config::Scope::Ungrouped,
                config::Scope::Group("work".to_string()),
            ],
        );
    }

    /// A Group *is* that declaration, so its Section is always ranked; the
    /// Accounts in no Group are ranked once somebody says they may be. The
    /// Reserve goes through the same answer.
    #[test]
    fn a_section_is_ranked_only_where_something_declared_its_accounts_a_set() {
        let mut registry = holdings();
        let now = cycle::tests::now();
        let group = || config::Scope::Group("work".to_string());

        assert!(
            !registry.ungrouped.interchangeable,
            "nobody has said so yet"
        );
        let held = Section::of(&registry, config::Scope::Ungrouped, now);
        assert_eq!(held.order(), "held");
        assert!(held.reserve(&registry).is_none(), "and no Reserve with it");

        let declared = Section::of(&registry, group(), now);
        assert_eq!(declared.order(), "ranked", "a Group is that declaration");
        assert!(declared.reserve(&registry).is_some());

        registry.ungrouped.interchangeable = true;
        let now_a_set = Section::of(&registry, config::Scope::Ungrouped, now);
        assert_eq!(now_a_set.order(), "ranked");
        assert!(now_a_set.reserve(&registry).is_some());
    }
}
