//! What a Listing *is*: which Scopes it covers, in what order, and what each
//! Scope's Accounts are said to be.
//!
//! A Listing is every Account Perch holds, shown in the Scopes they sit in and
//! in the order each Scope's Strategy makes. It is one [`Section`] per Scope,
//! and the Section is where the weight is: it carries whether its order is a
//! ranking Perch would stand behind or Accounts merely held (ADR 0049), which
//! is the one thing every renderer has to agree about. Asked once here, it
//! travels with the Accounts rather than being worked out again by whoever is
//! drawing them.
//!
//! What `perch list` *does* — which breadth was asked for, how a table is laid
//! out, which sentences go under it — is [`crate::commands::list`]'s. What a
//! Listing is *made of* is here, where `perch status` can reach it too:
//! `status` answers about one Account and a listing about a set, but it is the
//! same Account either way (ADR 0053), and [`document`] is the one shape both
//! of them write it in.
//!
//! Here rather than on [`crate::registry::Account`], which is where an
//! Account's own document would otherwise go. That document carries the
//! Account's Headroom and its Utilization, so it is written in terms of
//! [`crate::cycle`] and [`crate::utilization`] — and each of those is written
//! in terms of [`crate::registry`] already. This module is the first place all
//! three can be reached at once (ADR 0060).

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::cycle;
use crate::host::Host;
use crate::registry::{self, Account, Quarantine, Registry};
use crate::reserve::Reserve;
use crate::utilization;

/// One Scope's Accounts as a Listing shows them: in order, and with what that
/// order *is*.
///
/// Ranked where a Cycle could happen in the Scope, and in the order they were
/// added where one could not. Being in no Group is the absence of a declaration
/// that Accounts are interchangeable rather than a weaker form of one (ADR
/// 0017), so until `interchangeable` says otherwise a bare `perch switch`
/// refuses there instead of choosing. Ordering those Accounts by Headroom would
/// show a ranking Perch would not make — the one thing a Listing exists not to
/// do — so they are held in the order they were added, with the Headroom still
/// beside each of them as the figure it is.
///
/// Which of the two it is travels *with* the Accounts rather than being asked
/// again by each renderer. A table shows the difference by not sorting; a
/// document has to say it in a word (ADR 0053), and two answers to "was this
/// ranked?" is how the two come to disagree about the distinction ADR 0049
/// called its weightiest.
///
/// Its fields are its own. Everything a renderer needs of a Section it gets by
/// asking — [`Section::document`], [`Section::reserve`], [`flattened`] — because
/// a renderer reaching past them for `accounts` could sort what it found, and
/// the order is the whole of what this type is carrying.
pub struct Section<'a> {
    scope: registry::Scope,
    ranked: bool,
    accounts: Vec<&'a Account>,
}

impl<'a> Section<'a> {
    pub fn of(registry: &'a Registry, scope: registry::Scope, now: DateTime<Utc>) -> Section<'a> {
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

    /// What the order is, in the word the decision that drew the distinction
    /// uses (ADR 0049) — so a script branches on the same term the guide
    /// explains.
    fn order(&self) -> &'static str {
        match self.ranked {
            true => "ranked",
            false => "held",
        }
    }

    /// What this Scope has left to draw on, or nothing where nothing has
    /// declared its Accounts a set.
    ///
    /// The same gate the order goes through, and deliberately the same field: a
    /// Reserve is what these Accounts have *between them*, which is the claim
    /// ADR 0017 says nobody has made about the Ungrouped until `interchangeable`
    /// is declared. Ranking them and saying what they have left are the two
    /// things every surface declines together, so they are declined here off one
    /// answer rather than two.
    pub fn reserve<'r>(&self, registry: &'r Registry) -> Option<Reserve<'r>> {
        self.ranked.then(|| Reserve::of(registry, &self.scope))
    }

    pub fn document(
        &self,
        host: &dyn Host,
        registry: &Registry,
        now: DateTime<Utc>,
    ) -> serde_json::Value {
        let listed: Vec<serde_json::Value> = self
            .accounts
            .iter()
            .map(|account| document(host, registry, account, now))
            .collect();
        json!({
            // The same shape the document's own `scope` key carries, because it
            // is the same question asked of a narrower set: a script that reads
            // one should not have to learn a second spelling to read the other.
            "scope": scope_json(&self.scope),
            "order": self.order(),
            // At every breadth, unlike the table (ADR 0058): the section has
            // already named the Scope this is about, which is the whole of what
            // the table lacks.
            //
            // `null` for a Scope holding nobody, where the human listing says a
            // sentence of its own instead. Unambiguous against the other `null`
            // because `accounts` sits beside it: an empty array distinguishes
            // "nobody is here" from "nobody has declared these a set".
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

/// A Scope as a script reads it.
///
/// One spelling for every document that names one, whether it is the breadth
/// that was asked for or the Scope one Section holds: a script that reads one
/// should not have to learn a second shape to read the other. A listing's own
/// breadth has an arm no Cycle's Scope has — every Scope at once — and
/// [`crate::commands::list`] adds that one where it is the only place it can
/// mean anything.
pub fn scope_json(scope: &registry::Scope) -> serde_json::Value {
    match scope {
        registry::Scope::Group(name) => json!({"kind": "group", "name": name}),
        registry::Scope::Ungrouped => json!({"kind": "ungrouped", "name": serde_json::Value::Null}),
    }
}

/// Every Scope, with the one the active Account is in first.
///
/// Every Group and the Accounts in no Group, so an Account is in exactly one of
/// them and a Listing holds all of them — the invariant
/// [`crate::registry::Registry::group_names`] states and `load` enforces, since
/// an Account claiming a Group nothing declared would be in none of these and
/// so in no Listing at all.
///
/// The active Account's Scope leads, because it is where you are and, wherever
/// a Cycle happens at all, the one a bare `perch switch` looks in.
pub fn scopes(registry: &Registry) -> Vec<registry::Scope> {
    let mut every: Vec<registry::Scope> = registry
        .group_names()
        .map(|name| registry::Scope::Group(name.to_string()))
        .collect();
    every.push(registry::Scope::Ungrouped);

    let Some(active) = registry.active_account() else {
        return every;
    };
    let here = match &active.group {
        Some(name) => registry::Scope::Group(name.clone()),
        None => registry::Scope::Ungrouped,
    };
    let mut ordered = vec![here.clone()];
    ordered.extend(every.into_iter().filter(|scope| *scope != here));
    ordered
}

/// One Account as a script reads it, wherever it is being read.
///
/// One shape rather than two. `perch status --json` described its Account under
/// `active` and `perch list --json` described one under `accounts`, and the two
/// key sets did not even overlap: `status` carried `account_uuid` and neither
/// `alias`, `group` nor `disabled`, and the listing carried the reverse. So a
/// script that asked "which Group is the Account I am on in?" had to run a
/// second command to find out, and one written against either could not be
/// pointed at the other.
///
/// What each *document* answers still differs, and that is the part that should
/// — a Listing asks about a set and `perch status` about one Account. It is the
/// Account itself that has no business being two things.
pub fn document(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
) -> serde_json::Value {
    json!({
        "email": account.email(),
        "account_uuid": account.identity.account_uuid,
        "alias": registry.alias_of(account.email()),
        "group": account.group,
        // Present on every Account, unlike the cell above it. A machine reading
        // a shape is not a person reading a sentence (ADR 0043): a script that
        // had to test for a key's presence to learn a bool would have been
        // given a worse contract, not a truer one.
        "disabled": account.disabled,
        "quarantined": Quarantine::document(account.quarantine),
        "active": registry.is_active(account.email()),
        "organization": account.identity.organization_name,
        "plan": account.plan,
        // `ok()` rather than `?`, so an address no directory can be named after
        // is a null field on that Account rather than a `perch list --json`
        // that refuses to list anything at all. `registry::validate` does not
        // turn such an address away and `purge::forget_the_credential` guards it
        // as reachable by hand-editing, so the two surfaces disagreed about
        // whether the Account exists: every renderer a person reads shows it —
        // none of them ask for the directory — and the one a script reads failed
        // outright.
        "profile_dir": account.profile_dir(host).ok(),
        // The figure the section's order was made on, beside the windows it was
        // taken from. A section saying it is `ranked` and not carrying the
        // number it ranked on would be a claim with no way of checking it —
        // which is what the Headroom column exists to prevent on the surface a
        // person reads (ADR 0049).
        "headroom": cycle::headroom_document(account),
        "utilization": utilization::document(account, now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The Sections between them hold every Account the registry does, or
    /// `perch list` shows fewer Accounts than Perch holds.
    ///
    /// A Listing is built Scope by Scope — every declared Group, then the
    /// Accounts in none of them — so it lists everything only because those
    /// Scopes partition the registry. An Account that fell between them would
    /// simply not be printed, with no error anywhere to say so.
    ///
    /// What holds the partition up is `load` declaring any Group an Account
    /// claims, and nothing else. That is `registry`'s own invariant and is
    /// asserted there, including for a claim spelled in another case; this
    /// asserts only that a Listing depends on it.
    #[test]
    fn the_sections_hold_every_account_the_registry_holds() {
        let registry = holdings();
        assert_eq!(
            named(flattened(&every_section(&registry))),
            named(registry.accounts.iter().collect()),
        );
    }

    /// An address no directory can be named after is an Account the human
    /// renderers show and `--json` refused to say anything at all about.
    ///
    /// `registry::validate` does not turn such an address away, and
    /// `purge::forget_the_credential` guards it as reachable by hand-editing —
    /// so it is a state Perch has to be able to describe. Every renderer a
    /// person reads shows the Account, because none of them ask where its
    /// Profile is; this one asked, propagated, and failed the whole listing.
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

        let listed = document(&host, &registry, account, chrono::Utc::now());

        assert_eq!(listed["email"], "@");
        assert_eq!(
            listed["profile_dir"],
            serde_json::Value::Null,
            "said as the one thing that is not knowable rather than as a \
             refusal about the whole listing"
        );
    }

    /// Where you are is listed first, because it is the Scope a bare `perch
    /// switch` looks in — so the Accounts it would choose between are the ones
    /// at the top of the page rather than somewhere down it.
    #[test]
    fn the_scope_the_active_account_is_in_leads() {
        let registry = holdings();
        assert_eq!(scopes(&registry).first(), Some(&registry::Scope::Ungrouped));

        let mut in_a_group = holdings();
        in_a_group.settle(Some("b@example.com".to_string()));
        assert_eq!(
            scopes(&in_a_group).first(),
            Some(&registry::Scope::Group("play".to_string())),
        );
    }

    /// Every Scope is still listed, and each exactly once. Leading with one is
    /// a reordering rather than a promotion, and a Scope that dropped out of the
    /// list would take its Accounts with it.
    #[test]
    fn leading_with_a_scope_neither_drops_one_nor_repeats_it() {
        let mut every = scopes(&holdings());
        every.sort_by_key(|scope| scope.word().to_string());
        assert_eq!(
            every,
            vec![
                registry::Scope::Group("play".to_string()),
                registry::Scope::Ungrouped,
                registry::Scope::Group("work".to_string()),
            ],
        );
    }

    /// A Group is the declaration that its Accounts are interchangeable (ADR
    /// 0002), so a Group's Section is always ranked. The Accounts in no Group
    /// are ranked only once somebody says they may be (ADR 0017) — and the
    /// Reserve goes through the same answer, because saying what a set has left
    /// between them is the same claim as ranking them.
    #[test]
    fn a_section_is_ranked_only_where_something_declared_its_accounts_a_set() {
        let mut registry = holdings();
        let now = cycle::tests::now();
        let group = || registry::Scope::Group("work".to_string());

        assert!(
            !registry.ungrouped.interchangeable,
            "nobody has said so yet"
        );
        let held = Section::of(&registry, registry::Scope::Ungrouped, now);
        assert_eq!(held.order(), "held");
        assert!(held.reserve(&registry).is_none(), "and no Reserve with it");

        let declared = Section::of(&registry, group(), now);
        assert_eq!(declared.order(), "ranked", "a Group is that declaration");
        assert!(declared.reserve(&registry).is_some());

        registry.ungrouped.interchangeable = true;
        let now_a_set = Section::of(&registry, registry::Scope::Ungrouped, now);
        assert_eq!(now_a_set.order(), "ranked");
        assert!(now_a_set.reserve(&registry).is_some());
    }
}
