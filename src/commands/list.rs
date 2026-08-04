//! `perch list` — the one place that answers "what do I have".
//!
//! Every Account with the four things that decide whether it is any use to you
//! — the name you reach it by, the Group that says what it is interchangeable
//! with, whether it is a Cycle candidate at all, and how full it is. Like every
//! surface that shows Utilization it renders from cache and never blocks on the
//! network (ADR 0015).
//!
//! `perch status --group` is the same view narrowed to one Group, so it lands
//! here rather than in `status`: showing a set of Accounts is one job whether
//! the set is everything or the Group you would Cycle within.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::adopt;
use crate::commands::{CYCLING_AMONG_UNGROUPED, IN_NO_GROUP, say, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{Account, Registry};
use crate::utilization;

#[derive(Debug, Default, Clone, Copy)]
pub struct ListArgs {
    pub json: bool,
}

/// Which Accounts a listing covers.
///
/// Being in no Group is not a Group (ADR 0017), so it is a scope of its own
/// rather than a Group with a reserved name: the two are shown differently
/// because Cycling treats them differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every Account Perch holds.
    Everything,
    /// The Accounts in one Group, named as the Group was declared.
    Group(String),
    /// The Accounts in no Group at all.
    Ungrouped,
}

impl Scope {
    fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Everything => registry.accounts.iter().collect(),
            Scope::Group(name) => registry.accounts_in(name),
            Scope::Ungrouped => registry.ungrouped_accounts(),
        }
    }

    /// What the listing is of, said before it. Nothing for the whole list: a
    /// heading over everything is a heading that says nothing.
    fn heading(&self) -> Option<String> {
        match self {
            Scope::Everything => None,
            Scope::Group(name) => Some(format!("Group `{name}`")),
            Scope::Ungrouped => Some(IN_NO_GROUP.to_string()),
        }
    }

    fn json(&self) -> serde_json::Value {
        match self {
            Scope::Everything => json!({"kind": "all", "name": serde_json::Value::Null}),
            Scope::Group(name) => json!({"kind": "group", "name": name}),
            Scope::Ungrouped => json!({"kind": "ungrouped", "name": serde_json::Value::Null}),
        }
    }
}

pub fn run(host: &dyn Host, args: ListArgs, out: &mut dyn Write) -> Result<()> {
    let registry = adopt::ensure_adopted(host, out)?;
    render(out, &registry, Scope::Everything, host.now(), args.json)
}

/// The listing itself, so `perch status --group` shows the same Accounts the
/// same way over a narrower set.
pub fn render(
    out: &mut dyn Write,
    registry: &Registry,
    scope: Scope,
    now: DateTime<Utc>,
    json: bool,
) -> Result<()> {
    let accounts = scope.accounts(registry);
    if json {
        render_json(out, registry, &scope, &accounts, now)
    } else {
        render_human(out, registry, &scope, &accounts, now)
    }
}

/// The fixed columns, in the order they are printed. The headers are the same
/// array the rows are measured against, so a renamed column cannot drift from
/// the width it was measured at.
const HEADERS: [&str; 4] = ["Account", "Alias", "Group", "State"];

/// Where the Group sits, for the listings narrow enough to leave it out.
const GROUP_COLUMN: usize = 2;

/// One row per Account, with the extra Quota Windows carried on rows of their
/// own so no figure is dropped for want of a column.
struct Row {
    active: bool,
    email: String,
    alias: String,
    group: String,
    state: String,
    figures: Vec<String>,
}

impl Row {
    fn columns(&self) -> [&str; HEADERS.len()] {
        [&self.email, &self.alias, &self.group, &self.state]
    }

    fn marker(&self) -> char {
        if self.active { '*' } else { ' ' }
    }
}

fn rows(registry: &Registry, accounts: &[&Account], now: DateTime<Utc>) -> Vec<Row> {
    accounts
        .iter()
        .map(|account| Row {
            active: registry.active.as_deref() == Some(account.email()),
            email: account.email().to_string(),
            alias: registry
                .alias_of(account.email())
                .unwrap_or("-")
                .to_string(),
            group: account.group.clone().unwrap_or_else(|| "none".to_string()),
            state: state_of(account),
            figures: utilization::lines(account, now),
        })
        .collect()
}

/// Whether the Account is a Cycle candidate, and whether its Credential still
/// works. Both are always said, because they are separate facts with separate
/// fixes: enabling a Quarantined Account would not repair it, and a Quarantined
/// Account that is listed like any other is never mistaken for one that
/// vanished.
fn state_of(account: &Account) -> String {
    let pool = if account.enabled {
        "enabled"
    } else {
        "disabled"
    };
    if account.quarantined {
        format!("{pool}, quarantined")
    } else {
        pool.to_string()
    }
}

fn render_human(
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    accounts: &[&Account],
    now: DateTime<Utc>,
) -> Result<()> {
    if let Some(heading) = scope.heading() {
        say(out, &heading)?;
    }

    let rows = rows(registry, accounts, now);
    if rows.is_empty() {
        return say(out, &nothing_here(scope));
    }

    // The Group is a column only when the listing spans Groups. Narrowed to
    // one, every row would carry the same answer to a question the heading has
    // already answered.
    let show_group = matches!(scope, Scope::Everything);
    let widths = widths(&rows);

    write_row(out, ' ', HEADERS, "Utilization", &widths, show_group)?;
    for row in &rows {
        for (index, figure) in row.figures.iter().enumerate() {
            match index {
                0 => write_row(
                    out,
                    row.marker(),
                    row.columns(),
                    figure,
                    &widths,
                    show_group,
                )?,
                // A second Quota Window belongs to the Account above it, so it
                // is shown under that Account's first figure and nothing else
                // is repeated.
                _ => write_row(out, ' ', [""; HEADERS.len()], figure, &widths, show_group)?,
            }
        }
    }

    if rows.iter().any(|row| row.active) {
        say(out, "")?;
        say(out, "* is the active Account.")?;
    }

    if matches!(scope, Scope::Ungrouped) {
        say(out, &format!("Cycling {CYCLING_AMONG_UNGROUPED}."))?;
    }

    Ok(())
}

fn write_row(
    out: &mut dyn Write,
    marker: char,
    columns: [&str; HEADERS.len()],
    figure: &str,
    widths: &[usize; HEADERS.len()],
    show_group: bool,
) -> Result<()> {
    let cells: Vec<String> = columns
        .iter()
        .zip(widths)
        .enumerate()
        .filter(|(column, _)| show_group || *column != GROUP_COLUMN)
        .map(|(_, (value, width))| format!("{value:width$}"))
        .collect();
    writeln!(
        out,
        "{}",
        format!("{marker} {}  {figure}", cells.join("  ")).trim_end()
    )
    .map_err(write_failed)
}

/// Each column is as wide as the widest thing in it, header included. Measured
/// in characters rather than bytes, because that is what the padding counts.
fn widths(rows: &[Row]) -> [usize; HEADERS.len()] {
    std::array::from_fn(|column| {
        rows.iter()
            .map(|row| row.columns()[column].chars().count())
            .chain(std::iter::once(HEADERS[column].chars().count()))
            .max()
            .unwrap_or_default()
    })
}

/// A listing with nothing in it, said as the state it is rather than as an
/// empty table.
fn nothing_here(scope: &Scope) -> String {
    match scope {
        Scope::Everything => {
            "No Accounts yet. `perch add` logs into one in a Profile of its own.".to_string()
        }
        Scope::Group(name) => format!("The Group `{name}` holds no Accounts yet."),
        Scope::Ungrouped => "Every Account is in a Group.".to_string(),
    }
}

fn render_json(
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    accounts: &[&Account],
    now: DateTime<Utc>,
) -> Result<()> {
    let listed: Vec<serde_json::Value> = accounts
        .iter()
        .map(|account| {
            json!({
                "email": account.email(),
                "alias": registry.alias_of(account.email()),
                "group": account.group,
                "enabled": account.enabled,
                "quarantined": account.quarantined,
                "active": registry.active.as_deref() == Some(account.email()),
                "organization": account.identity.organization_name,
                "plan": account.plan,
                "profile_dir": account.profile.dir,
                "utilization": utilization::document(account, now),
            })
        })
        .collect();

    let document = json!({
        "scope": scope.json(),
        // Named apart from `status --json`'s `active`, which is an object: one
        // command answers two questions under `--group`, and a script that
        // reaches for the wrong one should not find a plausible value there.
        "active_account": registry.active,
        "accounts": listed,
    });

    writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&document)
            .map_err(|err| PerchError::Other(err.to_string()))?
    )
    .map_err(write_failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(email: &str, alias: &str) -> Row {
        Row {
            active: false,
            email: email.to_string(),
            alias: alias.to_string(),
            group: "none".to_string(),
            state: "enabled".to_string(),
            figures: vec!["never observed".to_string()],
        }
    }

    #[test]
    fn a_column_is_as_wide_as_its_widest_value_or_its_header() {
        let widths = widths(&[row("a@b.com", "overflow"), row("someone@b.com", "-")]);
        assert_eq!(widths[0], "someone@b.com".len());
        assert_eq!(widths[1], "overflow".len());
        assert_eq!(widths[2], "Group".len(), "the header is the floor");
    }

    #[test]
    fn a_column_is_measured_in_characters_rather_than_bytes() {
        // A name a terminal draws in eight columns pads to eight, not to the
        // eleven bytes it happens to occupy.
        let widths = widths(&[row("a@b.com", "øverfløw")]);
        assert_eq!(widths[1], 8);
    }
}
