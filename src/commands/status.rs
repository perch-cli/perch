//! `perch status` — who is active, and where their quota stands.
//!
//! Rendered from cache and never from the network (ADR 0015). This is the
//! command people put in a shell prompt, where it may run several times a
//! minute; fetching here would burn the hourly usage budget needed for
//! switching decisions. Every figure is shown with its age, so a stale number
//! is visibly stale rather than quietly wrong.
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
use crate::registry::Account;
use crate::utilization;

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusArgs {
    pub json: bool,
    /// Show every Account the active one shares a Group with, rather than the
    /// active Account alone.
    pub group: bool,
}

pub fn run(host: &dyn Host, args: StatusArgs, out: &mut dyn Write) -> Result<()> {
    let registry = adopt::ensure_adopted(host, out)?;
    let account = registry.active_account().ok_or_else(|| {
        PerchError::NotFound(
            "No active Account. Run `claude` and log in, then run Perch again.".to_string(),
        )
    })?;

    let now = host.now();
    if args.group {
        // Being in no Group is not a Group (ADR 0017), so from an ungrouped
        // Account the answer to "where would I land" is every ungrouped
        // Account together with what Cycling will not do with them unasked.
        let scope = match &account.group {
            Some(group) => Scope::Group(group.clone()),
            None => Scope::Ungrouped,
        };
        return list::render(out, &registry, scope, now, args.json);
    }

    if args.json {
        render_json(out, account, now)
    } else {
        render_human(out, account, now)
    }
}

fn render_human(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    utilization::write_labelled(out, "Account", account.email())?;
    if let Some(organization) = &account.identity.organization_name {
        utilization::write_labelled(out, "Organization", organization)?;
    }
    if let Some(plan) = &account.plan {
        utilization::write_labelled(out, "Plan", plan)?;
    }

    utilization::write_figures(out, account, now)
}

fn render_json(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    let document = json!({
        "active": {
            "email": account.email(),
            "account_uuid": account.identity.account_uuid,
            "organization": account.identity.organization_name,
            "plan": account.plan,
            "profile_dir": account.profile.dir,
        },
        "utilization": utilization::document(account, now),
    });

    writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&document)
            .map_err(|err| PerchError::Other(err.to_string()))?
    )
    .map_err(write_failed)
}
