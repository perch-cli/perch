//! Behavior: what `perch list` answers when the question is "what do I have",
//! and what it shows when it is narrowed to a Scope — a Group by name, or the
//! Accounts in no Group (ADR 0053). Both render from cache, and neither touches
//! the network without `--refresh` (ADR 0015).
//!
//! And what it answers about which Account is *best*, which is the other half
//! of the listing (ADR 0049): the rows come out in the order a Cycle ranks
//! them, with the Headroom that order was made on beside them, except where
//! nothing has declared the Accounts interchangeable — there they are held
//! rather than ranked (ADR 0017).

mod common;

use chrono::{DateTime, TimeZone, Utc};
use common::*;
use perch::error::EXIT_NOT_FOUND;
use perch::host::FakeHost;
use perch::probe::Identity;
use perch::registry::{
    Account, CachedUtilization, Quarantine, Registry, Settings, WindowUtilization,
};

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 4, hour, minute, 0).unwrap()
}

fn account(email: &str, organization: &str) -> Account {
    Account {
        identity: Identity {
            email: email.to_string(),
            account_uuid: Some(format!("uuid-{email}")),
            organization_name: Some(organization.to_string()),
            organization_uuid: None,
        },
        plan: Some("pro".to_string()),
        disabled: false,
        quarantine: None,
        group: None,
        utilization: None,
    }
}

fn observed(observed_at: DateTime<Utc>, windows: &[(&str, f64)]) -> CachedUtilization {
    CachedUtilization {
        observed_at,
        windows: windows
            .iter()
            .map(|(window, used_percent)| WindowUtilization {
                window: window.to_string(),
                used_percent: *used_percent,
                resets_at: None,
            })
            .collect(),
    }
}

/// Three Accounts, deliberately unalike: the active one in a Group with a
/// second that is Quarantined, and a third that is in no Group and disabled.
fn machine_holding_three_accounts() -> FakeHost {
    let mut registry = Registry::default();

    let mut active = account(EMAIL, "Acme");
    active.group = Some("work".to_string());
    active.utilization = Some(observed(at(11, 57), &[("5-hour", 42.0), ("7-day", 18.0)]));
    registry.upsert(active);

    let mut overflow = account(SECOND_EMAIL, "Overflow Ltd");
    overflow.group = Some("work".to_string());
    overflow.quarantine = Some(Quarantine::RenewalRejected);
    registry.upsert(overflow);

    let mut spare = account(THIRD_EMAIL, "Spare Ltd");
    spare.disabled = true;
    spare.utilization = Some(observed(at(10, 0), &[("5-hour", 91.0)]));
    registry.upsert(spare);

    registry.settle(Some(EMAIL.to_string()));
    registry
        .groups
        .insert("work".to_string(), Settings::default());
    registry
        .name_account("overflow", SECOND_EMAIL)
        .expect("the name is free");

    machine_holding(&registry)
}

fn machine_holding(registry: &Registry) -> FakeHost {
    let host = logged_in_machine().with_now(at(12, 0));
    common::save_registry(&host, registry);
    host
}

/// The Accounts as the listing puts them, top row first.
///
/// Rows only. Every row starts with the marker column — `* ` on the active
/// Account and two spaces on the rest — and the sentences under the table start
/// at the margin, which matters because a Quarantine is written out down there
/// with the address in it and would otherwise be counted as a row.
fn accounts_in_order(printed: &str) -> Vec<&str> {
    printed
        .lines()
        .filter(|line| line.starts_with("* ") || line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().find(|word| word.contains('@')))
        .collect()
}

/// One Group of Accounts, each as full as it is said to be — or, with no Group
/// named, the same Accounts in none.
fn a_group_of(group: Option<&str>, active: &str, accounts: &[(&str, f64)]) -> Registry {
    let mut registry = Registry::default();
    if let Some(group) = group {
        registry
            .groups
            .insert(group.to_string(), Settings::default());
    }
    for (email, used_percent) in accounts {
        let mut held = account(email, "Acme");
        held.group = group.map(str::to_string);
        held.utilization = Some(observed(at(11, 57), &[("5-hour", *used_percent)]));
        registry.upsert(held);
    }
    registry.settle(Some(active.to_string()));
    registry
}

/// The whole point of showing the ranking: the top row is where a bare
/// `perch switch` would land, so the listing and the Switch cannot come to
/// disagree about which Account is better (ADR 0049).
#[test]
fn the_rows_come_out_in_the_order_a_cycle_ranks_them() {
    let host = machine_holding(&a_group_of(
        Some("work"),
        EMAIL,
        &[(EMAIL, 90.0), (SECOND_EMAIL, 50.0), (THIRD_EMAIL, 10.0)],
    ));

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert_eq!(
        accounts_in_order(&printed),
        [THIRD_EMAIL, SECOND_EMAIL, EMAIL],
        "registry order was {EMAIL}, {SECOND_EMAIL}, {THIRD_EMAIL}:\n{printed}"
    );
}

/// The figure the order was made on, said so the order can be checked against
/// it rather than taken on trust.
///
/// Distinct from the Utilization beside it, which is every Quota Window: the
/// Headroom is what is left in the *worst* of them (ADR 0012), and that is the
/// one number a Cycle sorts on.
#[test]
fn the_headroom_the_order_was_made_on_is_a_column_of_its_own() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(
        printed.contains("Headroom"),
        "the column is named:\n{printed}"
    );
    let active_line = printed
        .lines()
        .find(|line| line.contains(EMAIL))
        .expect("the active Account is listed");
    assert!(
        active_line.contains("58%"),
        "the fullest of its two windows is 42% used, so 58% is left in every \
         one of them:\n{printed}"
    );
    assert!(
        active_line.contains("42%"),
        "and the window that decided it is still shown as the Utilization it \
         is:\n{printed}"
    );
    assert!(
        printed
            .lines()
            .find(|line| line.contains(SECOND_EMAIL))
            .is_some_and(|line| line.contains("never observed")),
        "an Account nothing was read for has no Headroom either — no figure \
         and plenty of room are opposite pieces of advice:\n{printed}"
    );
}

/// Being in no Group is the absence of a declaration that Accounts are
/// interchangeable rather than a weaker form of one (ADR 0017), so a bare
/// `perch switch` refuses there. Ordering them by Headroom would show a ranking
/// Perch would not make, which is the one thing this listing exists not to do.
#[test]
fn ungrouped_accounts_are_held_rather_than_ranked() {
    let host = machine_holding(&a_group_of(
        None,
        EMAIL,
        &[(EMAIL, 90.0), (SECOND_EMAIL, 10.0)],
    ));

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert_eq!(
        accounts_in_order(&printed),
        [EMAIL, SECOND_EMAIL],
        "the emptier Account is not promoted over one Perch would refuse to \
         choose between:\n{printed}"
    );
    assert!(
        printed.contains("10%") && printed.contains("90%"),
        "the Headroom is still beside each of them as the figure it is:\n{printed}"
    );
}

/// The other half of it: once somebody has said those Accounts are
/// interchangeable, a Cycle does choose between them, so the listing ranks them.
#[test]
fn ungrouped_accounts_are_ranked_once_cycling_may_move_between_them() {
    let mut registry = a_group_of(None, EMAIL, &[(EMAIL, 90.0), (SECOND_EMAIL, 10.0)]);
    registry.ungrouped.interchangeable = true;
    let host = machine_holding(&registry);

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert_eq!(
        accounts_in_order(&printed),
        [SECOND_EMAIL, EMAIL],
        "{printed}"
    );
}

/// A document says what its order is, or it does not have one (ADR 0053).
///
/// `accounts[0]` of the first section is the Account a bare `perch switch`
/// would land on, and a flat array states that nowhere — a script reading it
/// would be relying on a ranking the document never claimed to be making. The
/// held-versus-ranked distinction is the half with teeth: a `--json` showing a
/// ranking of Accounts Perch would refuse to choose between is the
/// two-surfaces-disagreeing failure ADR 0049 exists to prevent, reached through
/// a different renderer.
#[test]
fn the_json_says_which_of_its_sections_is_ranked_and_which_is_held() {
    let mut registry = a_group_of(Some("work"), EMAIL, &[(EMAIL, 90.0), (SECOND_EMAIL, 10.0)]);
    let mut loose = account(THIRD_EMAIL, "Spare Ltd");
    loose.utilization = Some(observed(at(11, 57), &[("5-hour", 30.0)]));
    registry.upsert(loose);
    let host = machine_holding(&registry);

    let (result, printed) = run_list(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert!(
        document["accounts"].is_null(),
        "there is no shape that makes no claim about its order: {document}"
    );

    let sections = document["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2, "{document}");

    assert_eq!(sections[0]["scope"]["kind"], "group");
    assert_eq!(sections[0]["scope"]["name"], "work");
    assert_eq!(sections[0]["order"], "ranked");
    assert_eq!(
        sections[0]["accounts"][0]["email"], SECOND_EMAIL,
        "the top of the ranked section is where a bare `perch switch` lands: \
         {document}"
    );

    assert_eq!(sections[1]["scope"]["kind"], "ungrouped");
    assert_eq!(
        sections[1]["order"], "held",
        "nothing has declared these interchangeable, so their order is not a \
         ranking and does not claim to be: {document}"
    );
}

/// The figure the ranking was made on travels with it, so a section saying it
/// is `ranked` carries the number that ranked it — and the three answers stay
/// three, rather than an absent figure arriving as nought.
#[test]
fn the_json_carries_the_headroom_the_ranking_was_made_on() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    let of = |email: &str| -> serde_json::Value {
        document["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|section| section["accounts"].as_array().unwrap())
            .find(|account| account["email"] == email)
            .unwrap_or_else(|| panic!("{email} is listed: {document}"))["headroom"]
            .clone()
    };

    assert_eq!(
        of(EMAIL),
        serde_json::json!({"state": "room", "percent": 58.0})
    );
    assert_eq!(
        of(SECOND_EMAIL),
        serde_json::json!({"state": "never-observed", "percent": null}),
        "no figure and plenty of room are opposite pieces of advice, and a \
         script reads them apart rather than both as nought"
    );
}

/// A Cycle never leaves the scope it started in (ADR 0002), so a listing
/// spanning several is those rankings one after another — and the one you are
/// standing in leads, because it is where a bare `perch switch` would look.
#[test]
fn the_scope_the_active_account_is_in_comes_first() {
    let mut registry = Registry::default();
    for name in ["alpha", "zulu"] {
        registry
            .groups
            .insert(name.to_string(), Settings::default());
    }
    let mut first = account(EMAIL, "Acme");
    first.group = Some("alpha".to_string());
    registry.upsert(first);
    let mut second = account(SECOND_EMAIL, "Overflow Ltd");
    second.group = Some("zulu".to_string());
    registry.upsert(second);
    registry.upsert(account(THIRD_EMAIL, "Spare Ltd"));
    registry.settle(Some(SECOND_EMAIL.to_string()));
    let host = machine_holding(&registry);

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert_eq!(
        accounts_in_order(&printed),
        [SECOND_EMAIL, EMAIL, THIRD_EMAIL],
        "`zulu` leads because that is where the active Account is, then the \
         other Group, then the Accounts in none:\n{printed}"
    );
}

#[test]
fn list_shows_every_account_with_its_alias_group_and_state() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert!(printed.contains(email), "{email} is missing:\n{printed}");
    }
    assert!(
        printed.contains("overflow"),
        "the Alias is shown:\n{printed}"
    );
    assert!(printed.contains("work"), "the Group is shown:\n{printed}");
    assert!(
        printed.contains("quarantined"),
        "a broken Account stays listed and says so:\n{printed}"
    );
    assert!(
        printed.contains("disabled"),
        "an Account out of the pool says so:\n{printed}"
    );
}

/// The positive state has no name (ADR 0052), so the column says nothing about
/// the Account nobody has done anything to — the placeholder the Alias column
/// already uses for having nothing to say. `disabled`, `quarantined` and
/// `disabled, quarantined` are the only things it prints.
#[test]
fn the_state_column_is_empty_for_an_account_in_neither_state() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(
        !printed.contains("enabled"),
        "nothing prints the word `enabled` — an Account that is not disabled \
         is not in a state with a name:\n{printed}"
    );
    assert_eq!(
        state_cell(&printed, EMAIL),
        "-",
        "its state cell holds the nothing-to-say placeholder:\n{printed}"
    );
    assert_eq!(state_cell(&printed, SECOND_EMAIL), "quarantined");
    assert_eq!(state_cell(&printed, THIRD_EMAIL), "disabled");
}

/// One Account's State cell, found by where the header puts the column rather
/// than by counting words — the Alias cell holds the same placeholder, so a
/// positional read would pass on the wrong one.
fn state_cell<'a>(printed: &'a str, email: &str) -> &'a str {
    let header = printed
        .lines()
        .find(|line| line.contains("State"))
        .expect("the table is headed");
    let at = header.find("State").expect("the State column is headed");
    printed
        .lines()
        .find(|line| line.contains(email))
        .expect("the Account is listed")[at..]
        .split_whitespace()
        .next()
        .expect("its State cell holds something")
}

#[test]
fn list_says_which_account_is_active() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    let active_line = printed
        .lines()
        .find(|line| line.contains(EMAIL))
        .expect("the active Account is listed");
    assert!(
        active_line.starts_with('*'),
        "the active Account is marked:\n{printed}"
    );
    assert!(
        printed.contains("* is the active Account"),
        "and the mark is explained:\n{printed}"
    );
}

/// A listing with nothing to say under it ends at the table, and does not space
/// for the sentences it has not got.
///
/// The blank line above the footer is the footer's own — a bare listing on a
/// machine with no active Account, nothing Quarantined and no Switch in flight
/// has no legend to explain, no Reserve to say (ADR 0058) and no reason to
/// print, so there is nothing for the line to separate.
#[test]
fn a_listing_with_nothing_under_the_table_ends_at_the_table() {
    let mut registry = a_group_of(None, EMAIL, &[(EMAIL, 42.0), (SECOND_EMAIL, 10.0)]);
    registry.settle(None);
    let host = machine_holding(&registry);

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(
        printed.trim_end().ends_with("(as of 3m ago)"),
        "the last row is the last thing said:\n{printed:?}"
    );
    assert!(
        !printed.contains("\n\n"),
        "and nothing is spaced off from it:\n{printed:?}"
    );
}

#[test]
fn list_shows_each_utilization_figure_with_its_age() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(printed.contains("5-hour"), "{printed}");
    assert!(printed.contains("42%"), "{printed}");
    assert!(printed.contains("7-day"), "{printed}");
    assert!(printed.contains("18%"), "{printed}");
    assert_eq!(
        printed.matches("as of 3m ago").count(),
        2,
        "both of the active Account's figures carry their age:\n{printed}"
    );
    assert!(
        printed.contains("91%") && printed.contains("as of 2h ago"),
        "an older figure reads as older:\n{printed}"
    );
}

#[test]
fn an_account_with_no_observed_utilization_is_not_shown_as_zero() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    let line = printed
        .lines()
        .find(|line| line.contains(SECOND_EMAIL))
        .expect("the Account with no figure is still listed");
    assert!(line.contains("never observed"), "{printed}");
    assert!(!line.contains('%'), "no number is invented:\n{printed}");
}

#[test]
fn list_without_refresh_never_touches_the_network() {
    let host = machine_holding_three_accounts().with_now(at(23, 0));

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(printed.contains("11h ago"), "{printed}");
    assert!(
        host.http_calls().is_empty(),
        "cheapness is a property of not passing `--refresh` (ADR 0053)"
    );
}

/// The rule `perch status` states for itself, which the listing follows at every
/// breadth: two of these drawing at once is the ordinary case rather than a
/// race, and a read that took the write lock would wait out whatever holds it
/// and then fail.
#[test]
fn list_reads_alongside_another_perch_rather_than_waiting_on_it() {
    let host = machine_holding_three_accounts();
    let held = perch::registry::lock(&host).expect("the other `perch` has it");

    let (everything, listed) = run_list(&host, false);
    let (narrowed, in_the_group) = run_list_in(&host, "work", false);

    everything.expect("a read does not wait on a writer");
    narrowed.expect("and narrowing it is still a read");
    assert!(listed.contains(THIRD_EMAIL), "{listed}");
    assert!(in_the_group.contains(SECOND_EMAIL), "{in_the_group}");
    drop(held);
}

/// The other half of the same rule: `--refresh` writes what it fetched, so it
/// takes the lock exclusively, at whatever breadth it was asked for.
#[test]
fn a_listing_that_refreshes_waits_for_the_other_perch_because_it_writes() {
    let host = machine_holding_three_accounts();
    let _held = perch::registry::lock(&host).expect("the other `perch` has it");

    for (what, result) in [
        ("list --refresh", run_list_refresh(&host, false).0),
        (
            "list work --refresh",
            run_list_in_refresh(&host, "work", false).0,
        ),
    ] {
        let refused = result.expect_err("a writer waits on a writer");
        assert!(
            refused.to_string().contains("the Perch registry lock"),
            "{what}: {refused}"
        );
    }
}

#[test]
fn list_json_carries_an_observation_time_on_every_figure() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "all");
    assert_eq!(document["active_account"], EMAIL);

    // Two sections — the Group the active Account is in, then the Accounts in
    // none — and the Accounts are inside them rather than in a flat array.
    let sections = document["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2, "{document}");
    let accounts: Vec<&serde_json::Value> = sections
        .iter()
        .flat_map(|section| section["accounts"].as_array().unwrap())
        .collect();
    assert_eq!(accounts.len(), 3);

    let active = accounts[0];
    assert_eq!(active["email"], EMAIL);
    assert_eq!(active["active"], true);
    assert_eq!(active["group"], "work");
    assert_eq!(
        active["disabled"], false,
        "the key follows the field's name and stays present on every Account: \
         a script testing for a key's presence to learn a bool has a worse \
         contract, not a truer one (ADR 0052)"
    );
    assert!(
        active["quarantined"].is_null(),
        "an Account that works says nothing about a Quarantine, which is what a \
         script reading it as false reads"
    );
    assert!(active["alias"].is_null());

    let windows = active["utilization"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    for window in windows {
        assert_eq!(
            window["observed_at"]
                .as_str()
                .map(|at| at.starts_with("2026-08-04T11:57:00")),
            Some(true),
            "every figure carries its observation time: {window}"
        );
        assert_eq!(window["observed_seconds_ago"], 180);
    }

    let overflow = accounts[1];
    assert_eq!(overflow["alias"], "overflow");
    assert_eq!(
        overflow["quarantined"]["reason"], "renewal-rejected",
        "and a broken one says what broke it, so a script can tell a Credential \
         Anthropic turned down from one Perch never had: {overflow}"
    );
    assert!(
        overflow["quarantined"]["said"].is_string(),
        "the reason rendered as prose, named for what it is: `detail` is what \
         `said_of` calls the failure underneath, and this is not that"
    );
    assert!(
        overflow["quarantined"]["detail"].is_null(),
        "and the word that means the other thing is not here: {overflow}"
    );
    assert_eq!(overflow["active"], false);
    assert_eq!(
        overflow["utilization"]["never_observed"], true,
        "a figure nobody has observed is said to be missing, not reported as zero"
    );
    assert!(overflow["utilization"]["observed_at"].is_null());

    let spare = accounts[2];
    assert_eq!(spare["disabled"], true);
    assert!(spare["group"].is_null());
}

/// A Scope narrows the listing to the Accounts you could Cycle between, which
/// is where you would land before you switch (ADR 0053).
#[test]
fn list_in_a_group_shows_every_account_in_it() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert!(printed.contains("work"), "the Group is named:\n{printed}");
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(printed.contains(SECOND_EMAIL), "{printed}");
    assert!(
        !printed.contains(THIRD_EMAIL),
        "an Account in another Group is not somewhere you would land:\n{printed}"
    );
    assert!(printed.contains("as of 3m ago"), "{printed}");
    assert!(
        host.http_calls().is_empty(),
        "a listing without `--refresh` must not fetch"
    );
}

/// The Scope is named rather than implied, so the listing answers about the one
/// asked for rather than about wherever the active Account happens to be
/// standing. The flag this replaced could only ever mean the latter.
#[test]
fn list_narrows_to_the_scope_named_rather_than_to_the_active_accounts_own() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    assert!(
        printed.contains(THIRD_EMAIL),
        "the Scope asked for is the Scope shown:\n{printed}"
    );
    assert!(
        !printed.contains(SECOND_EMAIL),
        "and the Group the active Account is in is not:\n{printed}"
    );
}

/// Being in no Group is not a Group (ADR 0017), so it is addressed by the one
/// word reserved for it rather than by a Group name.
#[test]
fn list_ungrouped_shows_every_account_in_no_group() {
    let mut registry = Registry::default();
    let mut first = account(EMAIL, "Acme");
    first.utilization = Some(observed(at(11, 57), &[("5-hour", 42.0)]));
    registry.upsert(first);
    let mut second = account(SECOND_EMAIL, "Overflow Ltd");
    second.group = Some("work".to_string());
    registry.upsert(second);
    registry.upsert(account(THIRD_EMAIL, "Spare Ltd"));
    registry
        .groups
        .insert("work".to_string(), Settings::default());
    registry.settle(Some(EMAIL.to_string()));
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    assert!(printed.contains("In no Group"), "{printed}");
    assert!(
        printed.contains(EMAIL) && printed.contains(THIRD_EMAIL),
        "{printed}"
    );
    assert!(
        !printed.contains(SECOND_EMAIL),
        "an Account in a Group is not one of the ungrouped:\n{printed}"
    );
    assert!(
        printed.contains("only moves between these when you say it may"),
        "being ungrouped is not a Group, and Cycling says so (ADR 0017):\n{printed}"
    );
    assert!(
        printed.contains("`interchangeable` is false"),
        "and says whether it has been said yet, rather than only stating the \
         rule: the bare rule reads as \"you have yet to say it\" to somebody who \
         already has:\n{printed}"
    );
}

/// The other half of it, which nothing was asking: the same clause on a machine
/// where the declaration *has* been made.
///
/// `perch group list` and the TUI both say which way the Setting is set;
/// `perch list` printed the rule alone, so somebody who had run
/// `perch config set ungrouped interchangeable true` was still told Cycling moves between
/// these "when you say it may".
#[test]
fn the_ungrouped_cycling_clause_says_so_once_cycling_has_been_allowed() {
    let mut registry = Registry::default();
    registry.upsert(account(EMAIL, "Acme"));
    registry.upsert(account(THIRD_EMAIL, "Spare Ltd"));
    registry.ungrouped.interchangeable = true;
    registry.settle(Some(EMAIL.to_string()));
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    assert!(
        printed.contains("`interchangeable` is true"),
        "the one Setting gating the whole Scope reads off the listing that \
         shows it:\n{printed}"
    );
}

/// What the Scope has left to draw on, said under the table a heading has
/// already named the Scope of (ADR 0058).
///
/// A count over the Accounts a Cycle may choose, the best one's own figure, and
/// the age of the reading that figure came from — never one pooled figure, since
/// Accounts sit on different plans and Perch only ever sees percentages.
#[test]
fn a_narrowed_listing_says_what_the_scope_has_left() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert!(
        printed.contains("Reserve: 1 of 1 Account has Headroom, the best 58% left (as of 3m ago)"),
        "the count, the best Account's own figure and the age of the reading it \
         came from:\n{printed}"
    );
    assert!(
        printed.contains("1 Quarantined, so nothing Cycles to it."),
        "and the Account a Cycle may not choose is said as what is out of the \
         running rather than counted as something the Group has:\n{printed}"
    );
}

/// An Account a Cycle may not choose is not part of what the Scope has, so the
/// counts under the table add up to the Accounts in it.
#[test]
fn the_reserve_counts_only_the_accounts_a_cycle_may_choose() {
    let mut registry = a_group_of(
        Some("work"),
        EMAIL,
        &[(EMAIL, 90.0), (SECOND_EMAIL, 10.0), (THIRD_EMAIL, 40.0)],
    );
    registry
        .held_mut(THIRD_EMAIL)
        .expect("it was just added")
        .disabled = true;
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert!(
        printed.contains("Reserve: 2 of 2 Accounts have Headroom, the best 90% left"),
        "three Accounts are listed and one of them is Disabled, so the Reserve \
         is over the other two:\n{printed}"
    );
    assert!(
        printed.contains("1 disabled, so nothing Cycles to it."),
        "and the third is said rather than dropped:\n{printed}"
    );
}

/// No figure is invented for an Account nothing was ever read for, so a Scope
/// nothing has been read for says what is in the way rather than `0%`.
#[test]
fn a_scope_with_nothing_left_says_what_is_in_the_way_rather_than_a_figure() {
    let mut registry = a_group_of(Some("work"), EMAIL, &[(EMAIL, 100.0)]);
    let mut unread = account(SECOND_EMAIL, "Overflow Ltd");
    unread.group = Some("work".to_string());
    registry.upsert(unread);
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert!(
        printed
            .contains("Reserve: none of 2 Accounts have Headroom (1 exhausted, 1 never observed)"),
        "\"none\" without a reason is a Group somebody stares at wondering \
         which:\n{printed}"
    );
    assert!(
        !printed.contains("Reserve: 0%") && !printed.contains("the best 0%"),
        "and nothing reads as a figure Perch never had:\n{printed}"
    );
}

/// A bare `perch list` is one table across every Scope at once, with the Group
/// as a column and no heading to name which Scope a sentence would be about
/// (ADR 0058). A Reserve line there would have to name its own Scope, which is a
/// heading smuggled into a sentence already as wide as a terminal.
#[test]
fn a_bare_listing_says_no_reserve() {
    for host in [
        machine_holding_three_accounts(),
        machine_holding(&a_group_of(Some("work"), EMAIL, &[(EMAIL, 42.0)])),
        machine_holding(&a_group_of(None, EMAIL, &[(EMAIL, 42.0)])),
    ] {
        let (result, printed) = run_list(&host, false);

        result.unwrap();
        assert!(
            !printed.contains("Reserve"),
            "on no registry does the listing of everything say one:\n{printed}"
        );
    }
}

/// Being in no Group is the absence of a declaration that those Accounts are
/// interchangeable (ADR 0017), and what they have left *between them* is exactly
/// the claim nobody has made. So the listing declines it in the same breath it
/// declines ranking them, and says it once the declaration is made.
#[test]
fn the_ungrouped_say_no_reserve_until_they_are_declared_interchangeable() {
    let mut registry = a_group_of(None, EMAIL, &[(EMAIL, 90.0), (SECOND_EMAIL, 10.0)]);
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    assert!(
        !printed.contains("Reserve"),
        "nothing has said these are a set:\n{printed}"
    );

    registry.ungrouped.interchangeable = true;
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    assert!(
        printed.contains("Reserve: 2 of 2 Accounts have Headroom, the best 90% left"),
        "and once it has, what they have between them is a fact:\n{printed}"
    );
}

/// The slot under the table reads top to bottom in the order each sentence
/// qualifies the one above it: the legend, what a Switch in flight did to it
/// (ADR 0048), what the Scope has left, whether Cycling may move within it, and
/// last what is broken — with the repair closing that block, since it is about
/// every Account in it (ADR 0061).
#[test]
fn the_reserve_sits_between_the_legend_and_the_quarantine_reasons() {
    let host = machine_holding_three_accounts();
    common::a_switch_died_mid_flight(&host, Some(EMAIL), SECOND_EMAIL);

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    let at = |said: &str| {
        printed
            .find(said)
            .unwrap_or_else(|| panic!("{said} is under the table:\n{printed}"))
    };
    assert!(at("* is the active Account.") < at("A Switch was in flight"));
    assert!(
        at("A Switch was in flight") < at("Reserve:"),
        "the Reserve comes after the Switch note:\n{printed}"
    );
    assert!(
        at("Reserve:") < at("Anthropic would not renew"),
        "and before the reasons anything is broken:\n{printed}"
    );
    assert!(
        at("Anthropic would not renew") < at("perch relogin"),
        "which the repair closes rather than interleaves:\n{printed}"
    );
}

/// The Quarantine reason varies between Accounts and the repair does not, so
/// the reason is said once per Account and the repair once for the Listing
/// (ADR 0061).
///
/// Three broken Accounts used to mean three copies of the identical
/// `perch relogin` sentence, under a table that had already said `quarantined`
/// three times in its State column.
#[test]
fn the_repair_is_said_once_however_many_accounts_are_quarantined() {
    let mut registry = a_group_of(
        Some("work"),
        EMAIL,
        &[(EMAIL, 90.0), (SECOND_EMAIL, 50.0), (THIRD_EMAIL, 10.0)],
    );
    for (email, why) in [
        (EMAIL, Quarantine::RenewalRejected),
        (SECOND_EMAIL, Quarantine::RotationLost),
        (THIRD_EMAIL, Quarantine::NoCredential),
    ] {
        registry
            .held_mut(email)
            .expect("it was just added")
            .quarantine = Some(why);
    }
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert_eq!(
        printed.matches("perch relogin").count(),
        1,
        "the repair does not differ between them, so it is said once:\n{printed}"
    );
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        assert!(
            printed.contains(email),
            "and every broken Account is still named:\n{printed}"
        );
    }
    for reason in [
        "Anthropic would not renew its Credential",
        "Perch holds no Credential for it",
    ] {
        assert_eq!(
            printed.matches(reason).count(),
            1,
            "each with the reason that is its own:\n{printed}"
        );
    }
}

/// And where there is exactly one to name, it is named — a person with one
/// broken Account learns both why it broke and the command that repairs it,
/// with nothing left to substitute.
#[test]
fn one_quarantined_account_is_told_the_repair_for_itself() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "work", false);

    result.unwrap();
    assert!(
        printed.contains(
            "overflow@example.com (as `overflow`): Anthropic would not renew its \
             Credential."
        ),
        "the Account, as it is named, and what happened to it:\n{printed}"
    );
    assert!(
        printed.contains("`perch relogin overflow@example.com` logs it in again in place"),
        "and the repair pointed at the one Account it can be about:\n{printed}"
    );
}

/// And the `Cycling …` clause follows it directly, qualifying it: the count is
/// over the Accounts a Cycle may move between, and that clause is the sentence
/// saying whether it may.
#[test]
fn the_cycling_clause_follows_the_reserve_it_qualifies() {
    let mut registry = a_group_of(None, EMAIL, &[(EMAIL, 90.0)]);
    registry.ungrouped.interchangeable = true;
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", false);

    result.unwrap();
    let said: Vec<&str> = printed
        .lines()
        .skip_while(|line| !line.starts_with("Reserve:"))
        .collect();
    assert!(
        said.get(1).is_some_and(|line| line.starts_with("Cycling ")),
        "the clause sits directly under the line it qualifies:\n{printed}"
    );
}

/// A Scope holding nobody says so in the one sentence that fits it, rather than
/// counting to nought about Accounts it has not got.
#[test]
fn a_narrowed_scope_holding_no_accounts_says_only_that() {
    let mut registry = Registry::default();
    registry.upsert(account(EMAIL, "Acme"));
    registry.settle(Some(EMAIL.to_string()));
    registry
        .groups
        .insert("spare".to_string(), Settings::default());
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "spare", false);

    result.unwrap();
    assert_eq!(
        printed.trim(),
        "Group `spare`\nThe Group `spare` holds no Accounts yet.",
        "the heading and the one sentence that fits an empty Scope, and nothing \
         else at all:\n{printed}"
    );
}

/// The document says it at every breadth, unlike the table (ADR 0058): each
/// section names its own Scope in a key, which is the whole of what the table
/// lacks.
///
/// As fields rather than the rendered sentence — the listing's document is
/// structured throughout, and a prose sentence in a document is a thing scripts
/// end up regexing.
#[test]
fn every_section_of_the_json_carries_the_scopes_reserve() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list(&host, true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    let sections = document["sections"].as_array().unwrap();

    let work = sections
        .iter()
        .find(|section| section["scope"]["name"] == "work")
        .unwrap_or_else(|| panic!("`work` is a section: {document}"));
    assert_eq!(work["reserve"]["candidates"], 1, "{document}");
    assert_eq!(work["reserve"]["with_headroom"], 1, "{document}");
    assert_eq!(work["reserve"]["out_of_the_running"], 1, "{document}");
    assert_eq!(work["reserve"]["best"]["email"], EMAIL, "{document}");
    assert_eq!(
        work["reserve"]["best"]["percent"], 58.0,
        "unrounded, like every other percentage in a document: {document}"
    );
    assert!(
        work["reserve"]["best"]["observed_at"]
            .as_str()
            .is_some_and(|at| at.starts_with("2026-08-04T11:57:00")),
        "the figure carries the age of the reading it came from: {document}"
    );

    let ungrouped = sections
        .iter()
        .find(|section| section["scope"]["kind"] == "ungrouped")
        .unwrap_or_else(|| panic!("the Ungrouped are a section: {document}"));
    assert!(
        ungrouped["reserve"].is_null(),
        "nothing has declared these a set, so there is no answer here — the \
         same thing the table says by silence: {document}"
    );
}

/// `null` for a Scope holding nobody too, which is not ambiguous against the
/// other `null` because `accounts` sits beside it and tells "nobody is here"
/// from "nobody declared these a set".
#[test]
fn a_section_holding_no_accounts_carries_no_reserve() {
    let mut registry = Registry::default();
    registry.upsert(account(EMAIL, "Acme"));
    registry.settle(Some(EMAIL.to_string()));
    registry
        .groups
        .insert("spare".to_string(), Settings::default());
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "spare", true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    let section = &document["sections"][0];
    assert!(section["reserve"].is_null(), "{document}");
    assert_eq!(
        section["accounts"].as_array().map(Vec::len),
        Some(0),
        "and the empty array beside it is what says which `null` this is: \
         {document}"
    );
}

#[test]
fn list_in_a_group_json_says_which_group_it_narrowed_to() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "work", true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "group");
    assert_eq!(document["scope"]["name"], "work");
    let sections = document["sections"].as_array().unwrap();
    assert_eq!(
        sections.len(),
        1,
        "narrowed to one Scope, the listing is one section: {document}"
    );
    assert_eq!(
        sections[0]["scope"], document["scope"],
        "and the section says the same Scope the document was asked for, in the \
         same shape: {document}"
    );
    let accounts = sections[0]["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["email"], EMAIL);
    assert_eq!(accounts[1]["email"], SECOND_EMAIL);
}

#[test]
fn list_ungrouped_json_says_it_narrowed_to_no_group() {
    let mut registry = Registry::default();
    registry.upsert(account(EMAIL, "Acme"));
    registry.settle(Some(EMAIL.to_string()));
    let host = machine_holding(&registry);

    let (result, printed) = run_list_in(&host, "ungrouped", true);

    result.unwrap();
    let document: serde_json::Value = serde_json::from_str(&printed).expect("valid JSON");
    assert_eq!(document["scope"]["kind"], "ungrouped");
    assert!(document["scope"]["name"].is_null());
}

/// A Scope nothing declared is a mistyped name, and the answer is what *was*
/// declared — the same answer `perch group move` gives, because it is the same
/// mistake. An empty table would read as a Group that exists and holds nothing.
#[test]
fn a_scope_nothing_declared_is_refused_rather_than_listed_as_empty() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_list_in(&host, "wrok", false);

    let refused = result.expect_err("there is no Group called `wrok`");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND);
    let said = refused.to_string();
    assert!(said.contains("`wrok`"), "which name: {said}");
    assert!(said.contains("work"), "and which one was meant: {said}");
    assert!(printed.is_empty(), "and nothing was listed: {printed}");
}

/// `global` is how people say every Scope at once, and it is refused as a Group
/// name so that a Group can never take that word (`validate_name`). Answered as
/// an ordinary mistyped Group it would be met with "Declare it with `perch group
/// add global`" — an offer the registry would then refuse. Here the answer is
/// the happy one: every Scope at once is what a bare `perch list` already shows.
#[test]
fn global_is_answered_with_the_listing_it_means_rather_than_offered_as_a_group() {
    let host = machine_holding_three_accounts();

    let (result, _) = run_list_in(&host, "global", false);

    let refused = result.expect_err("there is no Scope called `global`");
    assert_eq!(refused.exit_code(), EXIT_NOT_FOUND);
    let said = refused.to_string();
    assert!(
        said.contains("bare `perch list`"),
        "the listing of everything is what was meant: {said}"
    );
    assert!(
        !said.contains("perch group add"),
        "and no offer is made that the registry would refuse: {said}"
    );
}

/// An Alias names one Account and a Scope is a set, so the two are not
/// interchangeable here even though they share a namespace: the Account you want
/// a row for is `perch status`'s question, or a row of the listing it is in.
#[test]
fn an_alias_is_not_a_scope() {
    let host = machine_holding_three_accounts();

    let (result, _) = run_list_in(&host, "overflow", false);

    let refused = result.expect_err("`overflow` is an Alias rather than a Group");
    assert!(
        refused.to_string().contains("No Group called `overflow`"),
        "{refused}"
    );
}

/// `perch list` is the surface that keeps working when Perch holds no active
/// Account — precisely the state `perch status` refuses in and sends somebody to
/// `perch switch` to leave. A narrowing that read the active Account to know
/// what it meant would have brought that coupling back.
#[test]
fn list_narrows_on_a_machine_with_no_active_account() {
    let host = machine_holding_three_accounts();
    let mut registry = common::registry_of(&host);
    registry.settle(None);
    common::save_registry(&host, &registry);

    let (result, printed) = run_list_in(&host, "work", false);

    result.expect("a Scope is named rather than implied, so nothing has to be active");
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(printed.contains(SECOND_EMAIL), "{printed}");

    let (result, printed) = run_status(&host, false);

    result.expect_err("while `perch status` has no Account to be about");
    assert!(printed.is_empty(), "{printed}");
}

#[test]
fn status_shows_only_the_active_account() {
    let host = machine_holding_three_accounts();

    let (result, printed) = run_status(&host, false);

    result.unwrap();
    assert!(printed.contains(EMAIL), "{printed}");
    assert!(
        !printed.contains(SECOND_EMAIL),
        "plain status is about the Account you are on:\n{printed}"
    );
}

#[test]
fn list_on_a_machine_perch_has_never_run_on_adopts_first() {
    let host = logged_in_machine();

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    assert!(
        host.notes()
            .iter()
            .any(|note| note.contains("Adopted the Claude Code login")),
        "{:?}",
        host.notes()
    );
    assert!(printed.contains(EMAIL), "{printed}");
}

/// The first run is where a script meets adoption, and adoption used to say its
/// piece on the stream the document is written to. A `--json` that begins with
/// three lines of prose is not a document, and `jq` is what finds out.
#[test]
fn the_json_of_a_first_run_is_a_document_and_nothing_else() {
    for json_of in [
        (|host: &FakeHost| run_list(host, true)) as fn(&FakeHost) -> (perch::Result<()>, String),
        |host: &FakeHost| run_status(host, true),
    ] {
        let host = logged_in_machine();

        let (result, printed) = json_of(&host);

        result.unwrap();
        assert!(
            host.notes()
                .iter()
                .any(|note| note.contains("Adopted the Claude Code login")),
            "adoption still says what it did: {:?}",
            host.notes()
        );
        serde_json::from_str::<serde_json::Value>(&printed)
            .unwrap_or_else(|err| panic!("stdout has to parse whole: {err}\n{printed}"));
    }
}

/// Every Account's Quota Windows go into one `Utilization` column, so the
/// percentages line up down it however unalike the Accounts are.
///
/// The width of the window-name column was measured per Account, and an
/// Opus-eligible Account carries `7-day-opus` where a `pro` Account does not — so
/// the same `5-hour` percentage sat five columns further right on one Account
/// than on the one above it, in the one place the eye is running down a column.
#[test]
fn the_utilization_figures_line_up_down_the_column_across_unalike_accounts() {
    let mut registry = Registry::default();

    let mut plain = account(EMAIL, "Acme");
    plain.utilization = Some(observed(at(11, 57), &[("5-hour", 42.0), ("7-day", 18.0)]));
    registry.upsert(plain);

    let mut per_model = account(SECOND_EMAIL, "Overflow Ltd");
    per_model.utilization = Some(observed(
        at(11, 57),
        &[("5-hour", 7.0), ("7-day", 3.0), ("7-day-opus", 1.0)],
    ));
    registry.upsert(per_model);

    registry.settle(Some(EMAIL.to_string()));
    let host = machine_holding(&registry);

    let (result, printed) = run_list(&host, false);

    result.unwrap();
    let columns: Vec<usize> = printed
        .lines()
        .filter_map(|line| {
            let at = line.find("5-hour")?;
            Some(at + line[at..].find('%')?)
        })
        .collect();
    assert_eq!(
        columns.len(),
        2,
        "both Accounts show a 5-hour row: {printed}"
    );
    assert_eq!(
        columns[0], columns[1],
        "the same window's percentage is in the same column on both: {printed}"
    );
}

/// A machine holding nothing is diagnosed the same way whichever Scope was
/// named.
///
/// `perch list` on an empty machine says "No Accounts yet"; `perch list
/// ungrouped` said "Every Account is in a Group", which is a true-sounding
/// sentence about a machine with no Account to be in one. Narrowing a listing
/// should change what it shows, not what Perch says is the matter.
#[test]
fn narrowing_a_listing_on_an_empty_machine_does_not_change_the_diagnosis() {
    let host = machine_holding(&Registry::default());

    let (bare, everything) = run_list(&host, false);
    bare.expect("an empty machine is not a failed listing");
    let (narrowed, ungrouped) = run_list_in(&host, "ungrouped", false);
    narrowed.expect("nor is a narrowed one");

    assert!(everything.contains("No Accounts yet"), "{everything}");
    assert!(
        ungrouped.contains("No Accounts yet"),
        "there is no Account to be in a Group: {ungrouped}"
    );
}
