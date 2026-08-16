//! `perch list` — the one place that answers "what do I have", and the only
//! place that shows what a Cycle would make of it.
//!
//! Every Account with the things that decide whether it is any use to you — the
//! name you reach it by, the Group that says what it is interchangeable with,
//! whether it is a Cycle candidate at all, how much Headroom it has left and
//! how full each of its Quota Windows is. Like every surface that shows
//! Utilization it renders from cache and never blocks on the network (ADR
//! 0015).
//!
//! The rows come out in the order a Cycle ranks them (ADR 0012), always and not
//! behind a flag: the ranking `perch switch` makes should be visible rather
//! than hidden, so the two surfaces cannot come to disagree about which Account
//! is better (ADR 0049). Where nothing has declared a set of Accounts
//! interchangeable they are shown held rather than ranked (ADR 0017) — a
//! ranking of Accounts Perch would refuse to choose between is a claim nothing
//! backs.
//!
//! `perch status --group` is the same view narrowed to one Group, so it lands
//! here rather than in `status`: showing a set of Accounts is one job whether
//! the set is everything or the Group you would Cycle within.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;
use unicode_width::UnicodeWidthStr;

use crate::adopt;
use crate::commands::{IN_NO_GROUP, cycling_among_ungrouped, say, say_json, write_failed};
use crate::cycle;
use crate::error::Result;
use crate::host::Host;
use crate::observe::Report;
use crate::registry::{self, Account, Quarantine, Registry};
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
/// Deliberately not [`crate::registry::Scope`], which is the same idea for a
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
    ///
    /// Membership alone, and asked of the registry directly. What order they
    /// come out in is [`Scope::ranked`], which answers a question a Refresh does
    /// not have — it spends its budget on the Accounts about to be shown (ADR
    /// 0015) and has no opinion about which of them is best.
    pub fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Everything => registry.accounts.iter().collect(),
            Scope::Group(name) => registry.accounts_in(name),
            Scope::Ungrouped => registry.ungrouped_accounts(),
        }
    }

    /// The same Accounts, in the order the listing shows them, and in the
    /// sections that order was made within.
    ///
    /// The same Accounts is the load-bearing half. A Cycle never leaves the
    /// scope it started in (ADR 0002), so there is no one ranking over every
    /// Account Perch holds: each Group ranks its own by its own Strategy, and
    /// the Accounts in no Group rank among themselves when anything has said
    /// they may. The whole listing is those rankings one after another — which
    /// is why the Group is a column rather than a sort key nobody can see, and
    /// which is only a listing of everything because an Account is in exactly
    /// one of those scopes (see [`scopes`]).
    fn sections<'a>(&self, registry: &'a Registry, now: DateTime<Utc>) -> Vec<Section<'a>> {
        let of = |scope| Section::of(registry, scope, now);
        match self {
            Scope::Everything => scopes(registry).into_iter().map(of).collect(),
            Scope::Group(name) => vec![of(registry::Scope::Group(name.clone()))],
            Scope::Ungrouped => vec![of(registry::Scope::Ungrouped)],
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
/// Taken from [`registry::Scope::described`] rather than spelled again, because
/// that is what names the same Group in the middle of a sentence: a Group that
/// read one way over a listing and another in the sentence explaining a Cycle
/// would read as two Groups.
pub fn group_heading(name: &str) -> String {
    registry::Scope::Group(name.to_string()).described()
}

/// One scope's Accounts as the listing shows them: in order, and with what that
/// order *is*.
///
/// Ranked where a Cycle could happen in the scope, and in the order they were
/// added where one could not. Being in no Group is the absence of a declaration
/// that Accounts are interchangeable rather than a weaker form of one (ADR
/// 0017), so until `interchangeable` says otherwise a bare `perch switch`
/// refuses there instead of choosing. Ordering those Accounts by Headroom would
/// show a ranking Perch would not make — the one thing this listing exists not
/// to do — so they are held in the order they were added, with the Headroom
/// still beside each of them as the figure it is.
///
/// Which of the two it is travels *with* the Accounts rather than being asked
/// again by each renderer. A table shows the difference by not sorting; a
/// document has to say it in a word (ADR 0053), and two answers to "was this
/// ranked?" is how the two come to disagree about the distinction ADR 0049
/// called its weightiest.
struct Section<'a> {
    scope: registry::Scope,
    ranked: bool,
    accounts: Vec<&'a Account>,
}

impl<'a> Section<'a> {
    fn of(registry: &'a Registry, scope: registry::Scope, now: DateTime<Utc>) -> Section<'a> {
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

    fn document(
        &self,
        host: &dyn Host,
        registry: &Registry,
        now: DateTime<Utc>,
    ) -> Result<serde_json::Value> {
        let listed: Vec<serde_json::Value> = self
            .accounts
            .iter()
            .map(|account| document(host, registry, account, now))
            .collect::<Result<_>>()?;
        Ok(json!({
            // The same shape the document's own `scope` key carries, because it
            // is the same question asked of a narrower set: a script that reads
            // one should not have to learn a second spelling to read the other.
            "scope": Scope::from(&self.scope).json(),
            "order": self.order(),
            "accounts": listed,
        }))
    }
}

/// The Accounts of a Cycle's scope, said as a listing's.
///
/// Only this direction. [`Scope`]'s own doc gives the reason the two types are
/// separate — a Cycle must not be handed "every Account" — and widening a
/// Cycle's scope into a listing's cannot express that, since nothing here can
/// produce [`Scope::Everything`] and no Cycle is ever handed one of these.
impl From<&registry::Scope> for Scope {
    fn from(scope: &registry::Scope) -> Scope {
        match scope {
            registry::Scope::Group(name) => Scope::Group(name.clone()),
            registry::Scope::Ungrouped => Scope::Ungrouped,
        }
    }
}

/// Every scope, with the one the active Account is in first.
///
/// Every Group and the Accounts in no Group, so an Account is in exactly one of
/// them and the listing holds all of them — the invariant
/// [`crate::registry::Registry::group_names`] states and `load` enforces, since
/// an Account claiming a Group nothing declared would be in none of these and
/// so in no listing at all.
///
/// The active Account's scope leads, because it is where you are and, wherever
/// a Cycle happens at all, the one a bare `perch switch` looks in.
fn scopes(registry: &Registry) -> Vec<registry::Scope> {
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
    let sections = scope.sections(registry, now);
    if json {
        render_json(host, out, registry, &scope, &sections, now, report)
    } else {
        render_human(out, registry, &scope, &sections, now, report)
    }
}

/// The fixed columns, in the order they are printed. The headers are the same
/// array the rows are measured against, so a renamed column cannot drift from
/// the width it was measured at.
///
/// **Headroom** is the figure the order was made on: an Account's *worst* Quota
/// Window (ADR 0012), which is a different question from the Utilization
/// printed beside it. Utilization is every window, one line each; Headroom is
/// the one of them that decides whether a Cycle would come here, said as the
/// single number the ranking sorted on. Without it the order is a claim the
/// table gives no way of checking.
const HEADERS: [&str; 5] = ["Account", "Alias", "Group", "State", "Headroom"];

/// How many there are, for the arrays measured against them.
const COLUMNS: usize = HEADERS.len();

/// Where the Group sits, for the listings narrow enough to leave it out.
const GROUP_COLUMN: usize = 2;

/// What those columns hold for one Account: the name you reach it by, what it
/// is interchangeable with, whether it is any use, and how much of it is left.
fn columns(registry: &Registry, account: &Account) -> [String; COLUMNS] {
    [
        account.email().to_string(),
        registry
            .alias_of(account.email())
            .unwrap_or("-")
            .to_string(),
        account.group.clone().unwrap_or_else(|| "none".to_string()),
        state_of(account),
        cycle::headroom_phrase(account),
    ]
}

/// How many terminal cells a string is drawn in.
///
/// Not its bytes, and not its characters either. A CJK Group name is drawn two
/// columns per character and a combining mark is drawn in none, so a count of
/// characters pads to the wrong width — and because the widths are shared by
/// every row, one such name puts every column after it out of line for the whole
/// table. What a column is measured in has to be what a terminal draws in.
///
/// `unicode-width` sits on no seam — the width of a string is a pure function
/// of it — which is the test ADR 0025 actually sets, and a hand-rolled table of
/// East Asian widths would be the same data kept by hand, going wrong quietly.
fn cells(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// `text` with spaces after it until it fills `width` cells.
///
/// `format!("{text:width$}")` pads to a count of characters, which is
/// [`cells`]'s mistake from the other side: it would take the right width and
/// then fill it wrongly.
fn padded(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(cells(text))))
}

/// Each column as wide as the widest thing in it, [`HEADERS`] included,
/// measured in the cells a terminal draws them in — see [`cells`].
///
/// The headers are not a parameter, because there is one set of them and a
/// column measured against anything else is a column padded to a width its own
/// heading does not fit in.
fn widths<'a>(rows: impl IntoIterator<Item = &'a [String; COLUMNS]> + Clone) -> [usize; COLUMNS] {
    std::array::from_fn(|column| {
        rows.clone()
            .into_iter()
            .map(|row| cells(&row[column]))
            .chain(std::iter::once(cells(HEADERS[column])))
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
    // Measured once, across every Account in the table. Every Account's rows go
    // into one `Utilization` column here, so a width measured per Account put
    // the same window's percentage in a different place on each of them.
    let width = utilization::window_width_across(accounts.iter().copied());
    accounts
        .iter()
        .map(|account| Row {
            active: registry.is_active(account.email()),
            cells: columns(registry, account),
            figures: utilization::lines(account, now, width),
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
    sections: &[Section<'_>],
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    report.write_notes_beside_the_accounts(out)?;

    if let Some(heading) = scope.heading() {
        say(out, &heading)?;
    }

    // One table rather than one per section. What a section is shows in the
    // Group column and in the order, and a table that broke for a heading every
    // few rows would put a blank line between Accounts the eye is running down
    // a column of.
    let accounts = &flattened(sections);
    let rows = rows(registry, accounts, now);
    if rows.is_empty() {
        return say(out, &nothing_here(scope));
    }

    // The Group is a column only when the listing spans Groups. Narrowed to
    // one, every row would carry the same answer to a question the heading has
    // already answered.
    let show_group = matches!(scope, Scope::Everything);
    let widths = widths(rows.iter().map(|row| &row.cells));

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
    // Said whether or not the `*` is in this listing: with a Switch in flight
    // the marker is on the Account Perch was on rather than one it can
    // establish is live, and a listing narrowed to a Group that Switch was
    // leaving may carry no marker at all (ADR 0048).
    let in_flight = registry.active.a_switch_in_flight();
    if rows.iter().any(|row| row.active) || !broken.is_empty() || in_flight.is_some() {
        say(out, "")?;
    }
    if rows.iter().any(|row| row.active) {
        say(out, "* is the active Account.")?;
    }
    // Straight after the line it qualifies.
    if let Some(said) = in_flight {
        say(out, &said)?;
    }

    if matches!(scope, Scope::Ungrouped) {
        say(
            out,
            &format!("Cycling {}.", cycling_among_ungrouped(registry)),
        )?;
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
        .map(|(_, (value, width))| padded(value, *width))
        .collect();
    writeln!(
        out,
        "{}",
        format!("{marker} {}  {figure}", cells.join("  ")).trim_end()
    )
    .map_err(write_failed)
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

/// One Account as a script reads it, wherever it is being read.
///
/// One shape rather than two. `perch status --json` described its Account under
/// `active` and `perch list --json` described one under `accounts`, and the two
/// key sets did not even overlap: `status` carried `account_uuid` and neither
/// `alias`, `group` nor `enabled`, and the listing carried the reverse. So a
/// script that asked "which Group is the Account I am on in?" had to run a
/// second command to find out, and one written against either could not be
/// pointed at the other.
///
/// What each *document* answers still differs, and that is the part that should
/// — `--group` asks about a set and `perch status` about one Account (see
/// [`render_json`]). It is the Account itself that has no business being two
/// things.
pub fn document(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
    now: DateTime<Utc>,
) -> Result<serde_json::Value> {
    Ok(json!({
        "email": account.email(),
        "account_uuid": account.identity.account_uuid,
        "alias": registry.alias_of(account.email()),
        "group": account.group,
        "enabled": account.enabled,
        "quarantined": Quarantine::document(account.quarantine),
        "active": registry.is_active(account.email()),
        "organization": account.identity.organization_name,
        "plan": account.plan,
        "profile_dir": account.profile_dir(host)?,
        // The figure the section's order was made on, beside the windows it was
        // taken from. A section saying it is `ranked` and not carrying the
        // number it ranked on would be a claim with no way of checking it —
        // which is what the Headroom column exists to prevent on the surface a
        // person reads (ADR 0049).
        "headroom": cycle::headroom_document(account),
        "utilization": utilization::document(account, now),
    }))
}

/// A document says what its order is, or it does not have one (ADR 0053).
///
/// So the Accounts arrive in `sections` rather than in one `accounts` array.
/// The order is load-bearing — `accounts[0]` of the first section is the
/// Account a bare `perch switch` would land on — and a flat array states that
/// nowhere: a script reading it would be relying on a ranking the document
/// never claimed to be making. Worse, the held-versus-ranked distinction ADR
/// 0049 called its weightiest piece would be invisible, and a `--json` showing
/// a ranking of Accounts Perch would refuse to choose between is the
/// two-surfaces-disagreeing failure reached through a different renderer.
///
/// One `accounts` array beside the sections was the other option and is the
/// same mistake twice: a shape that makes no claim, kept for scripts, next to
/// the shape that makes it.
fn render_json(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    scope: &Scope,
    sections: &[Section<'_>],
    now: DateTime<Utc>,
    report: &Report,
) -> Result<()> {
    let sectioned: Vec<serde_json::Value> = sections
        .iter()
        .map(|section| section.document(host, registry, now))
        .collect::<Result<_>>()?;

    let document = json!({
        // What was asked for. Each section says what it holds and how it is
        // ordered, which is what came back.
        "scope": scope.json(),
        // Named apart from `status --json`'s `active`, which is an object: one
        // command answers two questions under `--group`, and a script that
        // reaches for the wrong one should not find a plausible value there.
        "active_account": registry.active.whose(),
        // What qualifies the key above, under the same name and the same shape
        // it has in `status --json` (ADR 0048). Here as well as there because
        // `perch status --group` is answered by this document, and which
        // Account you are standing on is not a fact that stops being worth
        // qualifying because the question widened to where you could land.
        "landing": registry.active.document(),
        "sections": sectioned,
        "refresh": report.document(),
    });

    say_json(out, &document)
}

/// Every section's Accounts end to end, for the renderer that draws one table.
fn flattened<'a>(sections: &[Section<'a>]) -> Vec<&'a Account> {
    sections
        .iter()
        .flat_map(|section| section.accounts.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(email: &str, alias: &str) -> [String; COLUMNS] {
        [email, alias, "none", "enabled", "40%"].map(str::to_string)
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

    /// And not in characters either, which is the same mistake one step later.
    ///
    /// A CJK name is drawn two columns per character. Measured by character it
    /// takes half the room it needs, and because one set of widths lays out
    /// every row, the columns after it step out of line for the whole table —
    /// on every row, not just the one with the name in it.
    #[test]
    fn a_column_is_measured_in_the_cells_a_terminal_draws_it_in() {
        assert_eq!(cells("作業"), 4, "two characters, four columns");
        assert_eq!(cells("øverfløw"), 8, "and a narrow one is still one each");

        let in_group = |name: &str| -> [[String; COLUMNS]; 1] {
            let mut row = row("a@b.com", "-");
            row[2] = name.to_string();
            [row]
        };
        assert_eq!(
            widths(&in_group("作業"))[2],
            "Group".len(),
            "the header is still the floor"
        );
        assert_eq!(widths(&in_group("作業作業"))[2], 8);
    }

    /// The sections between them hold the same Accounts the scope does, or
    /// `perch list` shows fewer Accounts than Perch holds.
    ///
    /// The listing is built scope by scope — every declared Group, then the
    /// Accounts in none of them — so it lists everything only because those
    /// scopes partition the registry. An Account that fell between them would
    /// simply not be printed, with no error anywhere to say so.
    ///
    /// What holds the partition up is `load` declaring any Group an Account
    /// claims, and nothing else. That is `registry`'s own invariant and is
    /// asserted there, including for a claim spelled in another case; this
    /// asserts only that the listing depends on it.
    #[test]
    fn the_sections_hold_every_account_the_scope_covers() {
        use crate::registry::Active;

        let mut registry = Registry::default();
        registry.declare_group("work").expect("the name is free");
        registry.declare_group("play").expect("the name is free");
        for (email, group) in [
            ("a@example.com", Some("work")),
            ("b@example.com", Some("play")),
            ("c@example.com", None),
            ("d@example.com", Some("work")),
        ] {
            let mut account = crate::cycle::tests::account(email, vec![]);
            account.group = group.map(str::to_string);
            registry.upsert(account);
        }
        registry.active = Active::settled_on(Some("c@example.com".to_string()));

        let named = |mut accounts: Vec<&Account>| -> Vec<String> {
            accounts.sort_by_key(|account| account.email().to_string());
            accounts
                .into_iter()
                .map(|account| account.email().to_string())
                .collect()
        };
        let sections = Scope::Everything.sections(&registry, crate::cycle::tests::now());
        assert_eq!(
            named(flattened(&sections)),
            named(Scope::Everything.accounts(&registry)),
        );
    }

    /// Padding fills the width the same way it was measured, or the right
    /// answer is arrived at and then spent wrongly.
    #[test]
    fn padding_fills_a_width_in_cells_rather_than_in_characters() {
        assert_eq!(padded("作業", 6), "作業  ", "four cells, two to fill");
        assert_eq!(padded("ab", 4), "ab  ");
        assert_eq!(
            padded("作業作業", 4),
            "作業作業",
            "and nothing is trimmed to fit: a cell count is a floor here"
        );
    }
}
