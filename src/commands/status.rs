//! `perch status` — who is active, and where their quota stands.
//!
//! Rendered from cache unless it is asked to fetch (ADR 0015). This is the
//! command people put in a shell prompt, where it may run several times a
//! minute; fetching by default would burn the hourly usage budget needed for
//! switching decisions. Every figure is shown with its age, so a stale number
//! is visibly stale rather than quietly wrong, and `--refresh` is how one stops
//! being stale.
//!
//! One Account in detail, and that is the whole of it (ADR 0053). "Where would
//! I land" is a question about a set, which is the listing
//! [`crate::commands::list`] draws at whatever breadth it is asked for — so it
//! is asked there, and this command answers about the Account you are on and
//! cannot be anything else.

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
    // and nothing else. This is the command advertised for shell prompts: two
    // of them rendering at once is the ordinary case, not a race, and taking
    // the registry lock to read would have one of them wait out the other and
    // then fail — a prompt showing an error because a second prompt drew at the
    // same moment.
    let (mut perch, mut registry) = match args.refresh {
        true => {
            let (perch, registry) = adopt::ensure_adopted_exclusively(host)?;
            (Some(perch), registry)
        }
        false => (None, adopt::ensure_adopted(host)?),
    };
    // Perch on nobody *because* a Switch was in flight and never recorded is
    // not an absence to report — it is the answer to why the absence is there,
    // and it exits 0 like every other way of saying one (ADR 0048). A Landing
    // that left nobody behind is the one shape with no Account under it to
    // describe, so what it gets is the line and the field alone.
    let active = match (
        active_email(&registry),
        registry.active().a_switch_in_flight(),
    ) {
        (Ok(active), _) => active,
        (Err(_), Some(said)) => return the_switch_alone(out, &registry, args.json, &said),
        (Err(nobody), None) => return Err(nobody),
    };

    // Every read spends from a budget that does not refill early (ADR 0015),
    // and this command shows one Account, so it reads one Account. A refresh
    // reads exactly what it is about to show, which is the rule `perch list`
    // follows at its own breadths.
    let report = match &mut perch {
        Some(perch) => observe::refresh(host, perch, &mut registry, std::slice::from_ref(&active)),
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

fn render_human(
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes_beside_the_accounts(out)?;

    // Above the Account line, because it is what qualifies it: with a Switch in
    // flight, the Account named below is the one Perch was on rather than the
    // one it can establish is live. A note rather than a labeled row, as the
    // Refresh's own notes above it are — the column is for facts about the
    // Account, and this is a fact about whether Perch can name one.
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
    // it cannot currently spend: the state is the news, and the numbers are the
    // detail.
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

/// What a script reads about the Account you are on.
///
/// `active` is the Account object every Listing uses ([`listing::document`])
/// rather than a shape of its own: the two used to carry key sets that did not
/// overlap, so a script asking which Group the active Account is in had to run a
/// second command, and one written against `perch list --json` could not be
/// pointed at this. The Account is the same Account; only the question the
/// document answers differs, and `active` against `sections` is what says which
/// was asked.
///
/// The Utilization is under `active` and nowhere else. It sat at the top level
/// too — `perch status --json | jq .utilization` being the line in somebody's
/// shell prompt — and that earned its keep against a document which, under the
/// flag that once widened this command to a Group, also answered about a set:
/// the duplicate was insurance against reaching into the wrong shape. This
/// document answers about exactly one Account and cannot be anything else (ADR
/// 0053), so the insurance has nothing left to cover and `jq
/// .active.utilization` is one word longer.
fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let document = json!({
        "active": listing::document(host, registry, account, now)?,
        "landing": registry.active().document(),
        "refresh": report.document(),
    });

    say_json(out, &document)
}

/// The whole of the report where a Landing left nobody behind: the Switch that
/// was in flight, and no Account section, because there is no Account
/// established to put in one.
///
/// The `--json` document keeps every key the ordinary one has, with the ones it
/// cannot answer left empty. A script reaching for `.active` on this machine is
/// asking about an Account Perch cannot name, and `null` is that answer — where
/// a document missing the key is a script's `jq` failing for what reads like a
/// different reason.
fn the_switch_alone(
    out: &mut dyn Write,
    registry: &Registry,
    json: bool,
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
            "refresh": Report::default().document(),
        }),
    )
}
