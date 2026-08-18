//! `perch list` — the one place that answers "what do I have", and the only
//! place that shows what a Cycle would make of it.
//!
//! Every Account with the things that decide whether it is any use to you — the
//! name you reach it by, the Group that says what it is interchangeable with,
//! whether it is a Cycle candidate at all, how much Headroom it has left and
//! how full each of its Quota Windows is. Like every surface that shows
//! Utilization it renders from cache unless it is asked to fetch (ADR 0015).
//!
//! The rows come out in the order a Cycle ranks them (ADR 0012), always and not
//! behind a flag: the ranking `perch switch` makes should be visible rather
//! than hidden, so the two surfaces cannot come to disagree about which Account
//! is better (ADR 0049). Where nothing has declared a set of Accounts
//! interchangeable they are shown held rather than ranked (ADR 0017) — a
//! ranking of Accounts Perch would refuse to choose between is a claim nothing
//! backs.
//!
//! This is the listing at every breadth (ADR 0053). A Scope narrows it — a
//! Group by name, or `ungrouped` — because showing a set of Accounts is one job
//! whether the set is everything or the Group you would Cycle within, and
//! "where would I land before I switch" is that job asked of one Scope.
//!
//! Narrowed, it also says what that Scope has left to draw on — its Reserve
//! (ADR 0058). Only narrowed: the table spans every Scope at once with the Group
//! as a column, so there is no heading for a sentence about one of them to sit
//! under. A `--json` section names its own Scope in a key, so every section
//! carries the Reserve at every breadth.
//!
//! `--refresh` follows the breadth: it reads the Accounts about to be shown and
//! no others, which is one rule rather than a capability each breadth has of
//! its own.
//!
//! What a Listing *is* — which Scopes it covers, in what order, and whether a
//! Scope's order is a ranking — is [`crate::listing`]'s. What this command does
//! is choose the breadth and draw it: the word somebody typed, the table, and
//! the sentences under it. The [`Scope`] here is the breadth alone, which is why
//! it has an arm a Cycle's Scope must never have.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::adopt;
use crate::commands::{IN_NO_GROUP, cycling_among_ungrouped, group, say, say_json, write_failed};
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::listing::{self, Section};
use crate::observe::{self, Report};
use crate::registry::{self, Account, Registry, UNGROUPED};
use crate::utilization;

#[derive(Debug, Default, Clone)]
pub struct ListArgs {
    /// Which Accounts to show: a Group by name, or `ungrouped` for the Accounts
    /// in no Group. `None` is every Account Perch holds.
    ///
    /// The word as it was typed rather than a resolved [`Scope`], because a
    /// name nothing was declared under is answered with what *was* declared,
    /// and a parser that had already thrown the word away could not.
    pub scope: Option<String>,
    /// Read current Utilization before showing it, rather than showing what was
    /// last observed.
    pub refresh: bool,
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
enum Scope {
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
    fn accounts<'a>(&self, registry: &'a Registry) -> Vec<&'a Account> {
        match self {
            Scope::Everything => registry.accounts.iter().collect(),
            Scope::Group(name) => registry.accounts_in(name),
            Scope::Ungrouped => registry.ungrouped_accounts(),
        }
    }

    /// The same Accounts, as the emails a Refresh is asked for.
    ///
    /// Here rather than at the caller because it is the same set [`accounts`]
    /// is: a refresh reads exactly what is about to be shown (ADR 0053), and a
    /// second walk of the registry to work out what that is would be a second
    /// answer to a question this type already answers.
    ///
    /// [`accounts`]: Scope::accounts
    fn emails(&self, registry: &Registry) -> Vec<String> {
        self.accounts(registry)
            .iter()
            .map(|account| account.email().to_string())
            .collect()
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
            Scope::Everything => listing::scopes(registry).into_iter().map(of).collect(),
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

    /// The breadth as a script reads it.
    ///
    /// Only the arm a Cycle's Scope has no answer for is spelled here. The other
    /// two are [`listing::scope_json`]'s, which is also what a Section names
    /// itself with — a listing narrowed to a Group would otherwise be at liberty
    /// to describe that Group one way at the top of the document and another in
    /// the section below it.
    fn json(&self) -> serde_json::Value {
        match self {
            Scope::Everything => json!({"kind": "all", "name": serde_json::Value::Null}),
            Scope::Group(name) => listing::scope_json(&registry::Scope::Group(name.clone())),
            Scope::Ungrouped => listing::scope_json(&registry::Scope::Ungrouped),
        }
    }
}

/// One Group, said as the line above the Accounts in it.
///
/// Taken from [`registry::Scope::described`] rather than spelled again, because
/// that is what names the same Group in the middle of a sentence: a Group that
/// read one way over a listing and another in the sentence explaining a Cycle
/// would read as two Groups.
fn group_heading(name: &str) -> String {
    registry::Scope::Group(name.to_string()).described()
}

pub fn run(host: &dyn Host, args: ListArgs, out: &mut dyn Write) -> Result<()> {
    // Exclusively only when there is something to write, which is `--refresh`
    // and nothing else — the rule `perch status` states for itself and the
    // reason it gives holds here too: a read that took the write lock would
    // wait out whatever holds it and then fail, and two listings drawn at the
    // same moment are the ordinary case rather than a race.
    let (mut perch, mut registry) = match args.refresh {
        true => {
            let (perch, registry) = adopt::ensure_adopted_exclusively(host)?;
            (Some(perch), registry)
        }
        false => (None, adopt::ensure_adopted(host)?),
    };

    let scope = match &args.scope {
        Some(name) => narrowed(&registry, name)?,
        None => Scope::Everything,
    };

    // Exactly the Accounts about to be shown. Every read spends from a budget
    // that does not refill early (ADR 0015), so narrowing the listing narrows
    // the reads with it and nothing is spent on an Account nobody asked about.
    let report = match &mut perch {
        Some(perch) => {
            let asking_about = scope.emails(&registry);
            observe::refresh(
                host,
                perch,
                &mut registry,
                &asking_about,
                &crate::probe::Installed::probed(host),
            )
        }
        // Nothing to report about a refresh nobody asked for: the empty report
        // renders as "nobody asked".
        None => Report::default(),
    };

    let now = host.now();
    render(host, out, &registry, scope, now, args.json, &report)
}

/// The Scope a name addresses, which is a Group by name or the Accounts in no
/// Group.
///
/// An Alias or an email address is not one of them and is not accepted as one.
/// A Target names one Account and this narrows to a set — a listing of one row
/// is what `perch status` answers better — so a name that is somebody's Alias
/// is answered as the Group it is not, the same way `perch config` answers it.
fn narrowed(registry: &Registry, name: &str) -> Result<Scope> {
    if registry::means_ungrouped(name) {
        return Ok(Scope::Ungrouped);
    }
    // The one word that has to be answered here rather than left to fall
    // through, and for the reason `validate_name` refuses it as a name: fallen
    // through it is answered with "Declare it with `perch group add global`",
    // which is an offer the registry refuses to honor. `perch config` answers
    // it with there being no Scope every other one falls back to; a listing has
    // the happier answer, because every Scope at once is precisely what it
    // shows when it is asked for no Scope at all.
    if registry::means_global(name) {
        return Err(PerchError::NotFound(format!(
            "There is no Scope called `{name}` — it is how people say every \
             Scope at once, and every Scope at once is what a bare `perch list` \
             shows. Narrowing takes a Group by name, or `{UNGROUPED}` for the \
             Accounts in no Group."
        )));
    }
    match registry.declared_group(name) {
        Some(declared) => Ok(Scope::Group(declared.to_string())),
        None => Err(group::no_such_group(registry, name)),
    }
}

/// The listing itself, at whatever breadth it was asked for — and, when it was
/// asked to fetch, saying what came of that in the same breath.
fn render(
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

/// What a cell holds when there is nothing to say in it — no Alias, or no state
/// worth naming. One spelling, because two columns saying nothing should not say
/// it two ways.
const NOTHING_TO_SAY: &str = "-";

/// What those columns hold for one Account: the name you reach it by, what it
/// is interchangeable with, whether it is any use, and how much of it is left.
fn columns(registry: &Registry, account: &Account) -> [String; COLUMNS] {
    [
        account.email().to_string(),
        registry
            .alias_of(account.email())
            .unwrap_or(NOTHING_TO_SAY)
            .to_string(),
        account.group.clone().unwrap_or_else(|| "none".to_string()),
        state_of(account),
        cycle::headroom_phrase(account),
    ]
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
            .map(|row| utilization::cells(&row[column]))
            .chain(std::iter::once(utilization::cells(HEADERS[column])))
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

/// Whether the Account has been taken out of Cycling, and whether its Credential
/// still works. Each is said whenever it is true, and an Account both things
/// are true of says both — they are separate facts with separate fixes:
/// enabling a Quarantined Account would not repair it, and a Quarantined
/// Account that is listed like any other is never mistaken for one that
/// vanished.
///
/// Neither being true is not a third thing to say. The positive state has no
/// name (ADR 0052), so the cell empties to the placeholder the Alias column
/// already uses for having nothing to say, and `disabled`, `quarantined` and
/// `disabled, quarantined` are the only things it prints.
fn state_of(account: &Account) -> String {
    let said: Vec<&str> = [
        account.disabled.then_some("disabled"),
        account.quarantined().then_some("quarantined"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if said.is_empty() {
        NOTHING_TO_SAY.to_string()
    } else {
        said.join(", ")
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
    let accounts = &listing::flattened(sections);
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

    // The sentences under the table, in the order they qualify one another: the
    // legend, what a Switch in flight has done to it, what this Scope has left,
    // what Cycling will do with it, and what is broken. Collected rather than
    // written one at a time so the blank line that separates them from the table
    // is decided by whether there is anything down here at all, rather than by a
    // condition each new sentence has to remember to join.
    let mut footer = Vec::new();
    if rows.iter().any(|row| row.active) {
        footer.push("* is the active Account.".to_string());
    }
    // Said whether or not the `*` is in this listing: with a Switch in flight
    // the marker is on the Account Perch was on rather than one it can
    // establish is live, and a listing narrowed to a Group that Switch was
    // leaving may carry no marker at all (ADR 0048).
    footer.extend(registry.active().a_switch_in_flight());
    footer.extend(reserve_lines(registry, scope, sections, now));
    if matches!(scope, Scope::Ungrouped) {
        // After the Reserve, because it qualifies it: the count above is over
        // Accounts a Cycle may move between, and this is the sentence saying
        // whether it may.
        footer.push(format!("Cycling {}.", cycling_among_ungrouped(registry)));
    }
    footer.extend(why_they_are_quarantined(registry, accounts));

    if !footer.is_empty() {
        say(out, "")?;
    }
    for line in footer {
        say(out, &line)?;
    }

    Ok(())
}

/// What the Scope has left to draw on, said where a heading has already named
/// which Scope that is (ADR 0058).
///
/// Off [`Scope::heading`] rather than off the breadth, because the heading is
/// the condition rather than a proxy for it. A bare `perch list` has none: it is
/// one table across every Scope at once — the reason is in [`render_human`] — so
/// a Reserve line there would have to name its own Scope, which is a heading
/// smuggled into a sentence already as wide as a terminal. Anything that gave
/// the bare listing headings would be giving these sentences somewhere to sit by
/// the same stroke.
fn reserve_lines(
    registry: &Registry,
    scope: &Scope,
    sections: &[Section<'_>],
    now: DateTime<Utc>,
) -> Vec<String> {
    if scope.heading().is_none() {
        return Vec::new();
    }
    sections
        .iter()
        .filter_map(|section| section.reserve(registry))
        .flat_map(|reserve| reserve.lines(now))
        .collect()
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
        .map(|(_, (value, width))| utilization::padded(value, *width))
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
        // Named apart from `status --json`'s `active`, which is an object: the
        // two documents answer two questions, and a script that reaches for the
        // wrong one should not find a plausible value there.
        "active_account": registry.active().whose(),
        // What qualifies the key above, under the same name and the same shape
        // it has in `status --json` (ADR 0048). Here as well as there because
        // which Account you are standing on is not a fact that stops being
        // worth qualifying because the question widened from it to the set it
        // sits in.
        "landing": registry.active().document(),
        "sections": sectioned,
        "refresh": report.document(),
    });

    say_json(out, &document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Quarantine;

    fn row(email: &str, alias: &str) -> [String; COLUMNS] {
        [email, alias, "none", NOTHING_TO_SAY, "40%"].map(str::to_string)
    }

    fn account_in(disabled: bool, quarantine: Option<Quarantine>) -> Account {
        Account {
            identity: crate::probe::Identity {
                email: "someone@example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled,
            quarantine,
            group: None,
            utilization: None,
        }
    }

    /// Every state the cell has a word for, and the one it has none for.
    ///
    /// The positive state has no name (ADR 0052), so nothing is printed where
    /// nothing has been done — and the two facts that do have names are still
    /// said separately, because they have separate fixes.
    #[test]
    fn the_state_cell_says_only_what_has_been_done_to_the_account() {
        let broken = Some(Quarantine::RenewalRejected);
        assert_eq!(state_of(&account_in(false, None)), NOTHING_TO_SAY);
        assert_eq!(state_of(&account_in(true, None)), "disabled");
        assert_eq!(state_of(&account_in(false, broken)), "quarantined");
        assert_eq!(
            state_of(&account_in(true, broken)),
            "disabled, quarantined",
            "an Account both are true of says both"
        );
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
}
