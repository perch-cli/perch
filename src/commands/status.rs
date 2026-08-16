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
use crate::commands::say_json;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::observe::{self, Report};
use crate::registry::{self, Account, Active, Registry};
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
        a_switch_in_flight(&registry.active),
    ) {
        (Ok(active), _) => active,
        (Err(_), Some(said)) => return the_switch_alone(out, &registry, args.json, &said),
        (Err(nobody), None) => return Err(nobody),
    };

    // Being in no Group is not a Group (ADR 0017), so from an ungrouped Account
    // the answer to "where would I land" is every ungrouped Account together
    // with what Cycling will not do with them unasked.
    let scope = args.group.then(|| match group_of(&registry, &active) {
        Some(group) => Scope::Group(group),
        None => Scope::Ungrouped,
    });

    let report = match &mut perch {
        Some(perch) => {
            let asking_about = to_refresh(&registry, &scope, &active);
            observe::refresh(host, perch, &mut registry, &asking_about)
        }
        None => Report::default(),
    };

    let now = host.now();
    match scope {
        Some(scope) => {
            // Said before the listing rather than inside it. `--group` widens
            // the question to "where would I land", which is the listing's to
            // draw (ADR 0053) — but which Account you are standing on is this
            // command's to qualify, whichever question it is answering.
            if let Some(said) = a_switch_in_flight(&registry.active)
                && !args.json
            {
                utilization::write_labelled(out, "Switch", &said)?;
            }
            list::render(host, out, &registry, scope, now, args.json, &report)
        }
        None => {
            let account = registry.held(&active)?;
            if args.json {
                render_json(host, out, &registry, account, now, &report)
            } else {
                render_human(out, &registry, account, now, &report)
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
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes_beside_the_accounts(out)?;

    // Above the Account line, because it is what qualifies it: with a Switch in
    // flight, the Account named below is the one Perch was on rather than the
    // one it can establish is live.
    if let Some(said) = a_switch_in_flight(&registry.active) {
        utilization::write_labelled(out, "Switch", &said)?;
    }

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

/// What a script reads about the Account you are on.
///
/// `active` is the Account object every listing uses ([`list::document`]) rather
/// than a shape of its own: the two used to carry key sets that did not overlap,
/// so a script asking which Group the active Account is in had to run a second
/// command, and one written against `perch list --json` could not be pointed at
/// this. The Account is the same Account; only the question the document answers
/// differs, and `active` against `accounts` is what says which was asked.
///
/// `utilization` stays at the top level as well as inside `active`, because it
/// is what this command is *for*: `perch status --json | jq .utilization` is the
/// line in somebody's shell prompt.
fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let document = json!({
        "active": list::document(host, registry, account, now)?,
        "landing": the_landing(&registry.active),
        "utilization": utilization::document(account, now),
        "refresh": report.document(),
    });

    say_json(out, &document)
}

/// The whole of the report where a Landing left nobody behind: the Switch that
/// was in flight, and no Account section, because there is no Account
/// established to put in one.
///
/// The `--json` document keeps every key the ordinary one has, with the ones it
/// cannot answer left empty. A script reaching for `.utilization` on this
/// machine is asking about an Account Perch cannot name, and `null` is that
/// answer — where a document missing the key is a script's `jq` failing for
/// what reads like a different reason.
fn the_switch_alone(
    out: &mut dyn Write,
    registry: &Registry,
    json: bool,
    said: &str,
) -> Result<()> {
    if !json {
        return utilization::write_labelled(out, "Switch", said);
    }
    say_json(
        out,
        &json!({
            "active": serde_json::Value::Null,
            "landing": the_landing(&registry.active),
            "utilization": serde_json::Value::Null,
            "refresh": Report::default().document(),
        }),
    )
}

/// The Switch that was in flight, as a script reads it.
fn the_landing(active: &Active) -> serde_json::Value {
    match active {
        Active::Landing { leaving, arriving } => json!({
            "leaving": leaving,
            "arriving": arriving,
        }),
        _ => serde_json::Value::Null,
    }
}

/// What there is to say about a Switch that was in flight and never recorded,
/// or `None` on the machines where there was not one.
///
/// Said at all because half of why this hazard survived is that a machine
/// mid-Landing is indistinguishable from a healthy one, so nobody looks (ADR
/// 0048). It does not change the exit code: status reports what it found rather
/// than judging it (ADR 0018), and a state the next Switch resolves by itself
/// should not fail somebody's shell prompt.
fn a_switch_in_flight(active: &Active) -> Option<String> {
    let Active::Landing { leaving, arriving } = active else {
        return None;
    };
    let was_on = match leaving {
        Some(leaving) => format!("Perch was on {leaving}"),
        None => "Perch was on no Account".to_string(),
    };
    Some(format!(
        "in flight and not recorded — {was_on} and was switching to {arriving}, \
         so which Credential is live is not settled. The next Switch resolves \
         it, and says so if it cannot."
    ))
}
