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

use crate::column;
use crate::error::Result;
use crate::host::{Host, Shown};
use crate::listing;
use crate::observe::Report;
use crate::registry::{self, Account, Registry};
use crate::say;
use crate::utilization;

#[derive(Debug, Default, Clone, Copy, clap::Args)]
pub struct StatusArgs {
    /// Read current Utilization from Anthropic first.
    ///
    /// The one Account this command is about, and no others. Asking for a
    /// refresh is the only thing in Perch that touches the network, here or
    /// on a listing. Roughly 28-30 reads an hour are allowed per Account
    /// and the allowance does not refill early, so a figure that cannot be
    /// read falls back to the cached one rather than failing.
    #[arg(long)]
    pub refresh: bool,

    /// Emit machine-readable output, with an observation time on every
    /// Utilization figure.
    #[arg(long)]
    pub json: bool,
}

pub fn run(host: &dyn Host, args: StatusArgs, out: &mut dyn Write) -> Result<()> {
    let mut viewing = crate::commands::Viewing::opened(host, args.refresh)?;
    // Perch on nobody *because* a Switch was in flight is the answer to why the
    // absence is there rather than an absence to report, so it exits 0
    // (ADR a-switch-is-written-down-first).
    let active = match (
        active_email(viewing.registry()),
        viewing.registry().active().a_switch_in_flight(),
    ) {
        (Ok(active), _) => active,
        (Err(_), Some(said)) => {
            return the_switch_alone(out, viewing.registry(), args.json, args.refresh, &said);
        }
        // The other absence, and a script reads it off the same document: no
        // Account, and the refusal on stderr with its own exit code as ever.
        (Err(nobody), None) => {
            if args.json {
                nobody_at_all(out, viewing.registry(), args.refresh)?;
            }
            return Err(nobody);
        }
    };

    // This command shows one Account, so it reads one Account.
    let report = viewing.figures_for(std::slice::from_ref(&active));

    let now = host.now();
    let registry = viewing.registry();
    let account = registry.held(&active)?;
    if args.json {
        render_json(host, out, registry, account, now, &report)
    } else {
        render_human(out, registry, account, now, &report)
    }
}

/// The Account being reported on, or why there is not one — which is
/// [`crate::commands::no_active_account`]'s to say, `perch switch` meeting the
/// same state.
fn active_email(registry: &Registry) -> Result<String> {
    match registry
        .active()
        .whose()
        .and_then(|email| registry.account(email))
    {
        Some(account) => Ok(account.email().to_string()),
        None => Err(registry::no_active_account(registry, "")),
    }
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
        say::line(out, &said)?;
    }

    column::write_labeled(out, "Account", &Shown::of(account.email()))?;
    if let Some(organization) = &account.identity.organization_name {
        column::write_labeled(out, "Organization", &Shown::of(organization))?;
    }
    if let Some(plan) = &account.plan {
        column::write_labeled(out, "Plan", &Shown::of(plan))?;
    }
    // Above the figures, because a Quarantined Account's figures describe quota
    // it cannot spend: the state is the news and the numbers are the detail.
    // Both halves on one line, since one Account means one copy of the repair.
    if let Some(why) = account.quarantine {
        column::write_labeled(
            out,
            "Quarantine",
            &Shown::of(&format!(
                "{}. {}",
                why.because(),
                registry::how_to_repair(account.email())
            )),
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
        "active": listing::document(host, registry, &registry.aliases_by_account(), account, now),
        "landing": registry.active().document(),
        "refresh": report.document(),
    });

    say::json(out, &document)
}

/// The document for either way Perch can be on no Account: a Landing that left
/// nobody behind, and a registry settled on nobody. One function, so the two
/// cannot differ in shape — and every key the ordinary one has, with what it
/// cannot answer left `null`, where a missing key is a script's `jq` failing
/// for what reads like a different reason.
fn nobody_at_all(out: &mut dyn Write, registry: &Registry, refresh: bool) -> Result<()> {
    say::json(
        out,
        &json!({
            "active": serde_json::Value::Null,
            "landing": registry.active().document(),
            // Whether one was asked for, not whether one happened: with no
            // Account there is nothing to read, and a caller who passed
            // `--refresh` is not one who asked for nothing.
            "refresh": match refresh {
                true => Report::asked_for().document(),
                false => Report::default().document(),
            },
        }),
    )
}

/// The whole of the report where a Landing left nobody behind: the Switch that
/// was in flight, and no Account section to put under it. Exits 0, so this is
/// the document on its own rather than one beside a refusal.
fn the_switch_alone(
    out: &mut dyn Write,
    registry: &Registry,
    json: bool,
    refresh: bool,
    said: &str,
) -> Result<()> {
    match json {
        true => nobody_at_all(out, registry, refresh),
        false => say::line(out, said),
    }
}
