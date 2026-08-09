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
use crate::commands::{CYCLING_AMONG_UNGROUPED, IN_NO_GROUP, say, say_json, write_failed};
use crate::error::Result;
use crate::host::Host;
use crate::observe::Report;
use crate::registry::{Account, Quarantine, Registry};
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
///
/// Deliberately not [`crate::cycle::Scope`], which is the same idea for a
/// Cycle and has no `Everything`. Showing every Account is ordinary; Cycling
/// across every Account is the thing ADR 0002 exists to prevent, and the
/// difference is worth keeping in the types rather than in a check.
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
    /// The Accounts a listing covers, which is also the set `--refresh` reads.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
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
            Scope::Group(name) => Some(group_heading(name)),
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

/// One Group, said as the line above the Accounts in it.
///
/// Shared with the TUI ([`crate::cycle::Scope::heading`]), which heads the same
/// set of Accounts and would otherwise name it in a second place: the two
/// surfaces are the same listing drawn twice (ADR 0011), and a Group that read
/// one way in a line and another in a frame would read as two Groups.
pub fn group_heading(name: &str) -> String {
    format!("Group `{name}`")
}

pub fn run(host: &dyn Host, args: ListArgs, out: &mut dyn Write) -> Result<()> {
    let registry = adopt::ensure_adopted(host)?;
    let now = host.now();
    // `perch list` never fetches (ADR 0015), so there is nothing to report
    // about a refresh: the empty report renders as "nobody asked".
    let unasked = Report::default();
    render(
        host,
        out,
        &registry,
        Scope::Everything,
        now,
        args.json,
        &unasked,
    )
}

/// The listing itself, so `perch status --group` shows the same Accounts the
/// same way over a narrower set — and, when it was asked to fetch, says what
/// came of that in the same breath.
pub fn render(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    scope: Scope,
    now: DateTime<Utc>,
    json: bool,
    report: &Report,
) -> Result<()> {
    let accounts = scope.accounts(registry);
    if json {
        render_json(host, out, registry, &scope, &accounts, now, report)
    } else {
        render_human(out, registry, &scope, &accounts, now, report)
    }
}

/// The fixed columns, in the order they are printed. The headers are the same
/// array the rows are measured against, so a renamed column cannot drift from
/// the width it was measured at.
///
/// Read by the TUI's Accounts view as well as by `perch list`, because they are
/// the same listing drawn twice — once in a line and once in a frame (ADR
/// 0011). Two copies of these columns is how the two surfaces come to disagree
/// about what an Account is called or what state it is in.
pub const HEADERS: [&str; 4] = ["Account", "Alias", "Group", "State"];

/// How many there are, for the callers that have to name the array's length in
/// a type.
pub const COLUMNS: usize = HEADERS.len();

/// Where the Group sits, for the listings narrow enough to leave it out.
const GROUP_COLUMN: usize = 2;

/// What those columns hold for one Account: the name you reach it by, what it
/// is interchangeable with, and whether it is any use.
pub fn columns(registry: &Registry, account: &Account) -> [String; COLUMNS] {
    [
        account.email().to_string(),
        registry
            .alias_of(account.email())
            .unwrap_or("-")
            .to_string(),
        account.group.clone().unwrap_or_else(|| "none".to_string()),
        state_of(account),
    ]
}

/// Each column as wide as the widest thing in it, header included. Measured in
/// characters rather than bytes, because that is what the padding counts — a
/// name a terminal draws in eight columns pads to eight, not to the eleven
/// bytes it happens to occupy.
///
/// Written over however many columns there are rather than over these four,
/// because the TUI's Accounts view shows the same listing with the figure its
/// order was made on beside it. Measuring a column is the same arithmetic
/// whatever the columns are, and two copies of it is how two surfaces come to
/// pad differently.
pub fn widths<'a, const N: usize>(
    headers: &[&str; N],
    rows: impl IntoIterator<Item = &'a [String; N]> + Clone,
) -> [usize; N] {
    std::array::from_fn(|column| {
        rows.clone()
            .into_iter()
            .map(|row| row[column].chars().count())
            .chain(std::iter::once(headers[column].chars().count()))
            .max()
            .unwrap_or_default()
    })
}

/// One row per Account, with the extra Quota Windows carried on rows of their
/// own so no figure is dropped for want of a column.
struct Row {
    active: bool,
    cells: [String; COLUMNS],
    figures: Vec<String>,
}

impl Row {
    fn columns(&self) -> [&str; COLUMNS] {
        self.cells.each_ref().map(String::as_str)
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
            cells: columns(registry, account),
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
    if account.quarantined() {
        format!("{pool}, quarantined")
    } else {
        pool.to_string()
    }
}

/// What each Quarantined Account is Quarantined for, under the table rather
/// than in it.
///
/// A reason is a sentence and a column is not, and the reason is the half of
/// the state that says what to do about it — so it is written out in full for
/// every broken Account, with the one command that repairs it. Nothing is said
/// at all when nothing is broken, which is the ordinary case.
fn why_they_are_quarantined(registry: &Registry, accounts: &[&Account]) -> Vec<String> {
    accounts
        .iter()
        .filter_map(|account| {
            let why = account.quarantine?;
            Some(why.said_of(
                &registry.named_for_the_user(account.email()),
                account.email(),
                None,
            ))
        })
        .collect()
}

fn render_human(
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    accounts: &[&Account],
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes(out)?;

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
    let widths = widths(&HEADERS, rows.iter().map(|row| &row.cells));

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
                _ => write_row(out, ' ', [""; COLUMNS], figure, &widths, show_group)?,
            }
        }
    }

    let broken = why_they_are_quarantined(registry, accounts);
    if rows.iter().any(|row| row.active) || !broken.is_empty() {
        say(out, "")?;
    }
    if rows.iter().any(|row| row.active) {
        say(out, "* is the active Account.")?;
    }

    if matches!(scope, Scope::Ungrouped) {
        say(out, &format!("Cycling {CYCLING_AMONG_UNGROUPED}."))?;
    }

    for why in broken {
        say(out, &why)?;
    }

    Ok(())
}

fn write_row(
    out: &mut dyn Write,
    marker: char,
    columns: [&str; COLUMNS],
    figure: &str,
    widths: &[usize; COLUMNS],
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

/// A listing with nothing in it, said as the state it is rather than as an
/// empty table. The TUI draws the same sentence, for the same reason it draws
/// the same columns.
pub fn nothing_here(scope: &Scope) -> String {
    match scope {
        Scope::Everything => {
            "No Accounts yet. `perch add` logs into one in a Profile of its own.".to_string()
        }
        Scope::Group(name) => format!("The Group `{name}` holds no Accounts yet."),
        Scope::Ungrouped => "Every Account is in a Group.".to_string(),
    }
}

fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    accounts: &[&Account],
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let listed: Vec<serde_json::Value> = accounts
        .iter()
        .map(|account| {
            Ok(json!({
                "email": account.email(),
                "alias": registry.alias_of(account.email()),
                "group": account.group,
                "enabled": account.enabled,
                "quarantined": Quarantine::document(account.quarantine),
                "active": registry.active.as_deref() == Some(account.email()),
                "organization": account.identity.organization_name,
                "plan": account.plan,
                "profile_dir": account.profile_dir(host)?,
                "utilization": utilization::document(account, now),
            }))
        })
        .collect::<Result<_>>()?;

    let document = json!({
        "scope": scope.json(),
        // Named apart from `status --json`'s `active`, which is an object: one
        // command answers two questions under `--group`, and a script that
        // reaches for the wrong one should not find a plausible value there.
        "active_account": registry.active,
        "accounts": listed,
        "refresh": report.document(),
    });

    say_json(out, &document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(email: &str, alias: &str) -> [String; COLUMNS] {
        [email, alias, "none", "enabled"].map(str::to_string)
    }

    #[test]
    fn a_column_is_as_wide_as_its_widest_value_or_its_header() {
        let widths = widths(
            &HEADERS,
            &[row("a@b.com", "overflow"), row("someone@b.com", "-")],
        );
        assert_eq!(widths[0], "someone@b.com".len());
        assert_eq!(widths[1], "overflow".len());
        assert_eq!(widths[2], "Group".len(), "the header is the floor");
    }

    #[test]
    fn a_column_is_measured_in_characters_rather_than_bytes() {
        // A name a terminal draws in eight columns pads to eight, not to the
        // eleven bytes it happens to occupy.
        let widths = widths(&HEADERS, &[row("a@b.com", "øverfløw")]);
        assert_eq!(widths[1], 8);
    }
}
