//! `perch status` — who is active, and where their quota stands.
//!
//! Rendered from cache unless it is asked to fetch (ADR 0015). This is the
//! command people put in a shell prompt, where it may run several times a
//! minute; fetching by default would burn the hourly usage budget needed for
//! switching decisions. Every figure is shown with its age, so a stale number
//! is visibly stale rather than quietly wrong, and `--refresh` is how one stops
//! being stale.
//!
//! `--group` widens the question from "where am I" to "where would I land",
//! which is the listing [`crate::commands::list`] already draws — so it is
//! answered there, over the Accounts the active one may be Cycled to.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::adopt;
use crate::commands::list::{self, Scope};
use crate::commands::write_failed;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::observe::{self, Report};
use crate::registry::{self, Account, Quarantine, Registry};
use crate::utilization;

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusArgs {
    pub json: bool,
    /// Show every Account the active one shares a Group with, rather than the
    /// active Account alone.
    pub group: bool,
    /// Read current Utilization before showing it, rather than showing what
    /// was last observed.
    pub refresh: bool,
}

pub fn run(host: &dyn Host, args: StatusArgs, out: &mut dyn Write) -> Result<()> {
    let (_perch, mut registry) = adopt::ensure_adopted_exclusively(host, out)?;
    let active = active_email(&registry)?;

    // Being in no Group is not a Group (ADR 0017), so from an ungrouped Account
    // the answer to "where would I land" is every ungrouped Account together
    // with what Cycling will not do with them unasked.
    let scope = args.group.then(|| match group_of(&registry, &active) {
        Some(group) => Scope::Group(group),
        None => Scope::Ungrouped,
    });

    let report = if args.refresh {
        let asking_about = to_refresh(&registry, &scope, &active);
        observe::refresh(host, &mut registry, &asking_about)
    } else {
        Report::default()
    };

    let now = host.now();
    match scope {
        Some(scope) => list::render(host, out, &registry, scope, now, args.json, &report),
        None => {
            let account = registry
                .account(&active)
                .expect("the active Account is one Perch holds");
            if args.json {
                render_json(host, out, account, now, &report)
            } else {
                render_human(out, account, now, &report)
            }
        }
    }
}

/// The Account being reported on, or why there is not one.
///
/// The remedy depends on what Perch holds. With nothing held there is nobody to
/// switch to and a login is the way in; with Accounts held, a login is not the
/// answer at all — Perch has Credentials and has simply been left on nobody,
/// which is what `perch switch` is for and what `perch remove` itself
/// recommends when it leaves the machine in this state.
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

fn group_of(registry: &Registry, email: &str) -> Option<String> {
    registry
        .account(email)
        .and_then(|account| account.group.clone())
}

/// Which Accounts a refresh covers: exactly the ones about to be shown.
///
/// Every read spends from a budget that does not refill early (ADR 0015), so
/// `perch status --refresh` reads the Account you are on, and `--group` reads
/// the ones being offered as landing places — never the whole registry.
fn to_refresh(registry: &Registry, scope: &Option<Scope>, active: &str) -> Vec<String> {
    match scope {
        Some(scope) => scope
            .accounts(registry)
            .iter()
            .map(|account| account.email().to_string())
            .collect(),
        None => vec![active.to_string()],
    }
}

fn render_human(
    out: &mut dyn Write,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes(out)?;

    utilization::write_labelled(out, "Account", account.email())?;
    if let Some(organization) = &account.identity.organization_name {
        utilization::write_labelled(out, "Organization", organization)?;
    }
    if let Some(plan) = &account.plan {
        utilization::write_labelled(out, "Plan", plan)?;
    }
    // Above the figures, because a Quarantined Account's figures describe quota
    // it cannot currently spend: the state is the news, and the numbers are the
    // detail.
    if let Some(why) = account.quarantine {
        utilization::write_labelled(
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

fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let document = json!({
        "active": {
            "email": account.email(),
            "account_uuid": account.identity.account_uuid,
            "organization": account.identity.organization_name,
            "plan": account.plan,
            "profile_dir": account.profile_dir(host)?,
            "quarantined": Quarantine::document(account.quarantine),
        },
        "utilization": utilization::document(account, now),
        "refresh": report.document(),
    });

    writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&document)
            .map_err(|err| PerchError::Other(err.to_string()))?
    )
    .map_err(write_failed)
}
