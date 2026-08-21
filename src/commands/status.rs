//! `perch status` — who is active, and where their quota stands.
//!
//! One Account in detail, and that is the whole of it
//! (ADR the-listing-owns-the-set): "where would I land" is a question about a
//! set, which [`crate::commands::list`] answers at whatever breadth.
//!
//! Rendered from cache unless asked to fetch (ADR a-figure-carries-its-age).
//! This is the command people put in a shell prompt, where it may run several
//! times a minute, and fetching by default would burn the budget switching
//! decisions need.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::adopt;
use crate::commands::say_json;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::listing;
use crate::observe::{self, Report};
use crate::registry::{self, Account, Registry};
use crate::utilization;

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusArgs {
    pub json: bool,
    /// Read current Utilization before showing it, rather than showing what
    /// was last observed.
    pub refresh: bool,
}

pub fn run(host: &dyn Host, args: StatusArgs, out: &mut dyn Write) -> Result<()> {
    // Exclusively only when there is something to write, which is `--refresh`
    // and nothing else: two shell prompts rendering at once is the ordinary
    // case, and a read that took the write lock would fail on one of them.
    let (mut perch, mut registry) = match args.refresh {
        true => {
            let (perch, registry) = adopt::ensure_adopted_exclusively(host)?;
            (Some(perch), registry)
        }
        false => (None, adopt::ensure_adopted(host)?),
    };
    // Perch on nobody *because* a Switch was in flight is the answer to why the
    // absence is there rather than an absence to report, so it exits 0
    // (ADR a-switch-is-written-down-first).
    let active = match (
        active_email(&registry),
        registry.active().a_switch_in_flight(),
    ) {
        (Ok(active), _) => active,
        (Err(_), Some(said)) => {
            return the_switch_alone(out, &registry, args.json, args.refresh, &said);
        }
        (Err(nobody), None) => return Err(nobody),
    };

    // This command shows one Account, so it reads one Account: a refresh reads
    // exactly what it is about to show, at whatever breadth.
    let report = match &mut perch {
        Some(perch) => observe::refresh(
            host,
            perch,
            &mut registry,
            std::slice::from_ref(&active),
            &crate::probe::Installed::probed(host),
        ),
        None => Report::default(),
    };

    let now = host.now();
    let account = registry.held(&active)?;
    if args.json {
        render_json(host, out, &registry, account, now, &report)
    } else {
        render_human(out, &registry, account, now, &report)
    }
}

/// The Account being reported on, or why there is not one.
///
/// The remedy depends on what Perch holds: with nothing held a login is the way
/// in, and with Accounts held it is not the answer at all — Perch has been left
/// on nobody, which is what `perch switch` is for.
fn active_email(registry: &Registry) -> Result<String> {
    if let Some(account) = registry.active_account() {
        return Ok(account.email().to_string());
    }
    Err(PerchError::NotFound(if registry.accounts.is_empty() {
        "Perch holds no Accounts. Run `claude` and log in, then run Perch again.".to_string()
    } else {
        format!(
            "Perch holds no active Account. `perch switch <target>` makes {} active.",
            if registry.accounts.len() == 1 {
                "the one it holds".to_string()
            } else {
                format!("one of the {} it holds", registry.accounts.len())
            }
        )
    }))
}

fn render_human(
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes_beside_the_accounts(out)?;

    // Above the Account line, because it qualifies it. A note rather than a
    // labeled row: the column is for facts about the Account, and this is one
    // about whether Perch can name one.
    if let Some(said) = registry.active().a_switch_in_flight() {
        crate::commands::say(out, &said)?;
    }

    utilization::write_labeled(out, "Account", account.email())?;
    if let Some(organization) = &account.identity.organization_name {
        utilization::write_labeled(out, "Organization", organization)?;
    }
    if let Some(plan) = &account.plan {
        utilization::write_labeled(out, "Plan", plan)?;
    }
    // Above the figures, because a Quarantined Account's figures describe quota
    // it cannot spend: the state is the news and the numbers are the detail.
    // Both halves on one line, since one Account means one copy of the repair.
    if let Some(why) = account.quarantine {
        utilization::write_labeled(
            out,
            "Quarantine",
            &format!(
                "{}. {}",
                why.because(),
                registry::how_to_repair(account.email())
            ),
        )?;
    }

    utilization::write_figures(out, account, now)
}

/// What a script reads about the Account you are on. `active` is the Account
/// object every Listing uses ([`listing::document`]) rather than a shape of its
/// own, and `active` against `sections` is what says which question was asked.
/// The Utilization sits under it and nowhere else: a copy at the top level would
/// insure against a shape this document cannot have.
fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let document = json!({
        "active": listing::document(host, registry, account, now),
        "landing": registry.active().document(),
        "refresh": report.document(),
    });

    say_json(out, &document)
}

/// The whole of the report where a Landing left nobody behind: the Switch that
/// was in flight, and no Account section to put under it. The document keeps
/// every key the ordinary one has, with what it cannot answer left `null` —
/// where a missing key is a script's `jq` failing for what reads like a
/// different reason.
fn the_switch_alone(
    out: &mut dyn Write,
    registry: &Registry,
    json: bool,
    refresh: bool,
    said: &str,
) -> Result<()> {
    if !json {
        return crate::commands::say(out, said);
    }
    say_json(
        out,
        &json!({
            "active": serde_json::Value::Null,
            "landing": registry.active().document(),
            // Whether one was asked for, not whether one happened: a Landing
            // that left nobody behind has no Account to read, and a caller who
            // passed `--refresh` is not one who asked for nothing.
            "refresh": match refresh {
                true => Report::asked_for().document(),
                false => Report::default().document(),
            },
        }),
    )
}
