//! What one frame looks like.
//!
//! Drawing and nothing else: everything here reads a [`Model`] and writes
//! cells, so the same function draws the real terminal and the buffer a test
//! reads back.
//!
//! Nothing on screen means anything by its colour *alone*. A Perch run over SSH
//! lands on whatever terminal is at the other end, and the one that cannot do
//! colour must still be able to tell the Account under the cursor from the
//! active one — so those are a `>` and a `*`, and the styling on top of them is
//! reverse video and bold, which every terminal that draws anything can do.
//!
//! Where a value came from is drawn two ways on purpose. On `Status` it is
//! written out, because that page is read once and has to survive a pipe and a
//! colour-blind palette. On `Config` it is a style, because that page is
//! navigated with a cursor and a dimmed value reads as "not set here" — and the
//! row it sits on is one keystroke from saying so in words.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Tabs};

use crate::commands::list::{self, COLUMNS};
use crate::cycle;
use crate::registry::{self, Account, Scope};
use crate::reserve::Reserve;
use crate::tui::model::{Column, Model, Refreshing, Row, ScopeRow, StatusRow, Tab};
use crate::utilization;

/// The share of the frame that what was said may take before it is counted
/// rather than shown.
///
/// A Switch says several sentences and a failed Refresh names an Account per
/// line, and both are worth reading — but the page is what the command is for,
/// and a report that pushed it off the screen would be the picker answering a
/// question nobody asked. A third, and never less than one line.
fn most_notes(height: usize) -> usize {
    (height / 3).max(1)
}

/// What the keys do, along the bottom of every frame.
///
/// On screen rather than under `?`, because a picker whose keys have to be
/// discovered is one people leave by killing the terminal. ASCII, because this
/// is the line that has to be legible on the terminal at the far end of an SSH
/// session.
///
/// Two lines' worth of keys on one row would not fit, so it says what the tab
/// in front of you does: `Status` is chosen from and `Config` is edited.
const STATUS_KEYS: &str = "q quit  Tab view  arrows move  Enter switch  x run  r refresh";
const CONFIG_KEYS: &str = "q quit  Tab view  arrows move/step  Spc flip  Esc inherit  n name";
const NAMING_KEYS: &str = "Enter confirm  Esc cancel";

/// Draws the whole frame.
pub fn render(frame: &mut Frame, model: &Model) {
    let notes = as_many_as_fit(
        notes(model),
        frame.area().width as usize,
        most_notes(frame.area().height as usize),
    );
    let asking = model.prompt.as_ref().map_or(0, |_| 2);
    let [bar, body, prompt, said, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(asking),
        Constraint::Length(notes.len() as u16),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_bar(frame, model, bar);
    match model.tab {
        Tab::Status => render_status(frame, model, body),
        Tab::Config => render_config(frame, model, body),
    }
    if let Some(asking) = &model.prompt {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("  {}", asking.what.asks()))
                    .style(Style::new().add_modifier(Modifier::BOLD)),
                Line::from(format!("  > {}_", asking.typed)),
            ]),
            prompt,
        );
    }
    frame.render_widget(
        Paragraph::new(notes.into_iter().map(Line::from).collect::<Vec<Line<'_>>>()),
        said,
    );
    frame.render_widget(
        Paragraph::new(
            Line::from(match (&model.prompt, model.tab) {
                (Some(_), _) => NAMING_KEYS,
                (None, Tab::Status) => STATUS_KEYS,
                (None, Tab::Config) => CONFIG_KEYS,
            })
            .style(Style::new().add_modifier(Modifier::DIM)),
        ),
        keys,
    );
}

/// The tab bar, and the one fact that belongs to no tab: which Account is
/// active. It is on every frame because it is what the whole command is about.
fn render_bar(frame: &mut Frame, model: &Model, area: Rect) {
    let active = match &model.registry().active {
        Some(email) => format!("active: {email} "),
        None => "no active Account ".to_string(),
    };

    // The tabs come first and whole. A terminal too narrow for both drops the
    // label rather than sharing the row out: a bar reading `Conf` against an
    // email address is one nobody can tell the views apart from, and which view
    // this is is the thing the keys act on.
    let label = match room_for_both(area.width as usize, list::cells(&active)) {
        true => list::cells(&active) as u16,
        false => 0,
    };
    let [tabs, marked] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(label)]).areas(area);

    frame.render_widget(
        Tabs::new(Tab::ALL.map(Tab::title))
            .select(model.tab.index())
            .divider("|")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        tabs,
    );
    if label > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(active).style(Style::new().add_modifier(Modifier::DIM))),
            marked,
        );
    }
}

/// Whether the row can hold the tab bar at its full width and the label too.
fn room_for_both(width: usize, label: usize) -> bool {
    let tabs: usize = Tab::ALL
        .iter()
        .map(|tab| list::cells(tab.title()) + 2)
        .sum::<usize>()
        + Tab::ALL.len()
        - 1;
    width >= tabs + label
}

/// The width a sidebar of these rows needs, markers included.
fn sidebar_width(labels: &[String]) -> u16 {
    let widest = labels
        .iter()
        .map(|label| list::cells(label))
        .max()
        .unwrap_or(0);
    // The marker, a space, the label, and one column of gutter. No wider: every
    // column the sidebar takes is one the figures beside it have to wrap at.
    (widest + 3) as u16
}

/// One sidebar, with the row it is on marked as a character as well as a style.
fn render_sidebar(frame: &mut Frame, area: Rect, labels: &[String], at: usize, has_the_keys: bool) {
    let rows: Vec<Line<'_>> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let line = Line::from(format!("{} {label}", if index == at { '>' } else { ' ' }));
            match (index == at, has_the_keys) {
                (true, true) => line.style(Style::new().add_modifier(Modifier::REVERSED)),
                (true, false) => line.style(Style::new().add_modifier(Modifier::BOLD)),
                (false, _) => line,
            }
        })
        .collect();
    render_scrolled(frame, area, rows, at);
}

// ---- The Status tab ----

/// Where you are: a sidebar of the three questions, and the answer to whichever
/// one is showing.
fn render_status(frame: &mut Frame, model: &Model, area: Rect) {
    let labels: Vec<String> = StatusRow::ALL
        .iter()
        .map(|row| row.title().to_string())
        .collect();
    let [sidebar, body] = Layout::horizontal([
        Constraint::Length(sidebar_width(&labels)),
        Constraint::Min(1),
    ])
    .areas(area);
    // Which of the two the keys are on, said the way the `Config` tab says it.
    // Hardcoded `true` here and no column check on the table below, both
    // sidebar and table drew themselves as the one holding the keys on every
    // frame — so `←` and `→`, which decide whether `↓` moves the sidebar or the
    // listing and whether `Enter` Switches, changed nothing anybody could see.
    render_sidebar(
        frame,
        sidebar,
        &labels,
        model.status_row,
        model.column == Column::Scopes,
    );

    // Asked once rather than at the head of each row: holding no Accounts is a
    // fact about the machine, not about which page is showing.
    if model.is_empty() {
        return render_nothing_held(frame, body);
    }
    match model.status() {
        StatusRow::Overview => render_overview(frame, model, body),
        StatusRow::Accounts => render_accounts(frame, model, body),
        StatusRow::Config => render_governing(frame, model, body),
    }
}

/// The active Account, summarised: who Claude Code is about to run as, what a
/// bare Switch would Cycle within, and how much is left.
fn render_overview(frame: &mut Frame, model: &Model, area: Rect) {
    let registry = model.registry();
    let Some(account) = registry.active_account() else {
        // One line rather than an empty frame, pointing at the page that has
        // something to do about it.
        return frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(
                    "  Perch holds no active Account. The Accounts page lists what it \
                     holds — Enter on one makes it active.",
                ),
            ]),
            area,
        );
    };

    // Everything on this page is wrapped at whatever the terminal is rather
    // than cut at it. Two of these are sentences rather than figures — why
    // Cycling behaves differently for somebody in no Group, and the command
    // that ends a Quarantine — and a sentence cut at the width loses its last
    // clause, which is the half naming what to do. The figures are wrapped for
    // the same reason a note is: a Reserve that lost "(as of 4m ago)" would be
    // a figure with no age on it, which is the one thing ADR 0015 will not have.
    let room = (area.width as usize).saturating_sub(LABEL + 2).max(20);
    let mut lines = vec![Line::from("")];
    let mut labelled = |label: &str, value: String| {
        for (at, part) in broken(&value, room).into_iter().enumerate() {
            lines.push(Line::from(format!(
                "  {:<LABEL$}{part}",
                if at == 0 { label } else { "" }
            )));
        }
    };
    labelled("Account", account.email().to_string());
    if let Some(alias) = registry.alias_of(account.email()) {
        labelled("Alias", alias.to_string());
    }
    if let Some(plan) = &account.plan {
        labelled("Plan", plan.clone());
    }
    labelled(
        "Group",
        match &account.group {
            Some(group) => format!("{group} — a bare Switch Cycles within it"),
            None => format!(
                "in no Group — Cycling {}",
                crate::commands::cycling_among_ungrouped(registry)
            ),
        },
    );
    // Above the figures where there is a Quarantine, because a Quarantined
    // Account's figures describe quota it cannot currently spend: the state is
    // the news and the numbers are the detail — and the repair is what somebody
    // reading it needs next.
    match account.quarantine {
        Some(why) => {
            labelled("Quarantine", why.because().to_string());
            labelled("Repair", registry::how_to_repair(account.email()));
        }
        None => {
            labelled("Headroom", cycle::headroom_in_full(account, model.now));
        }
    }

    if account.observed_utilization().is_some() {
        lines.push(Line::from(""));
        let widest = utilization::window_width_across(std::iter::once(account));
        for figure in utilization::lines_with_resets(account, model.now, widest) {
            lines.extend(wrapped(&figure, area.width as usize));
        }
    }

    // And what the Scope has left to draw on, which is the level above the
    // Account and the only other one there are honest figures for
    // ([`crate::reserve`]). A Group is a declaration that its Accounts are
    // interchangeable, which is what makes "what is left across them" a
    // question with an answer; being in no Group is the absence of that
    // declaration (ADR 0017), so there is nothing to total until
    // `cycle-ungrouped` says a Cycle may move between them.
    let scope = match &account.group {
        Some(group) => cycle::Scope::Group(group.clone()),
        None => cycle::Scope::Ungrouped,
    };
    if cycle::may_cycle_within(registry, &scope) {
        let reserve = Reserve::of(registry, &scope);
        let mut figures = reserve.lines(model.now);
        figures.extend(reserve.window_lines(model.now));
        if !figures.is_empty() {
            lines.push(Line::from(""));
            lines.push(
                Line::from(format!("  Across {}:", scope.described()))
                    .style(Style::new().add_modifier(Modifier::BOLD)),
            );
            for figure in figures {
                lines.extend(wrapped(&figure, area.width as usize));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// How wide the labels down the left of the Overview are.
const LABEL: usize = 14;

/// One line of the Overview, broken between words rather than cut at the width,
/// with what runs over indented under it.
///
/// Takes the width of the area it will be drawn in and subtracts its own
/// indents, because they are the one thing it knows and the caller does not.
/// Asked for the room instead, two of the four callers worked it out from the
/// first line's two-space indent and left the continuations' four unaccounted
/// for — so every line that wrapped was two cells too wide and ratatui cut the
/// tail off it. A Reserve that lost "(as of 4m ago)" and read "(as of ago)" is
/// a figure with no age on it, which is the one thing ADR 0015 will not have.
fn wrapped<'a>(text: &str, width: usize) -> Vec<Line<'a>> {
    broken(text, width.saturating_sub(CONTINUATION_INDENT).max(20))
        .into_iter()
        .enumerate()
        .map(|(at, part)| Line::from(format!("  {}{part}", if at == 0 { "" } else { "  " })))
        .collect()
}

/// How far a line that runs over is indented under the one before it — the
/// widest of the two indents [`wrapped`] writes, and so the one every line has
/// to leave room for.
const CONTINUATION_INDENT: usize = 4;

/// The columns of the Accounts page: `perch list`'s own, and the figure the
/// order was made on beside them.
const ACCOUNT_COLUMNS: usize = COLUMNS + 1;
const HEADROOM: &str = "Headroom";

/// The Accounts, as things to choose between: what each one is called, what it
/// is interchangeable with, how much of it is left, and whether it is any use.
///
/// In the order a Cycle ranks them ([`crate::cycle::ranked`]), Group by Group,
/// so the ranking `perch switch` makes is visible rather than hidden.
fn render_accounts(frame: &mut Frame, model: &Model, area: Rect) {
    let accounts = model.accounts();
    let cells: Vec<[String; ACCOUNT_COLUMNS]> = accounts
        .iter()
        .map(|account| {
            with_headroom(
                list::columns(model.registry(), account),
                cycle::headroom_phrase(account),
            )
        })
        .collect();
    let headers: [&str; ACCOUNT_COLUMNS] = with_headroom(list::HEADERS, HEADROOM);
    let widths = list::widths(&headers, &cells);

    let rows: Vec<Line<'_>> = cells
        .iter()
        .enumerate()
        .map(|(index, cells)| {
            let line = Line::from(format!(
                "{}{}",
                markers(model, accounts[index], index),
                row(cells, &widths)
            ));
            // The same pair `render_sidebar` draws, and for the same reason:
            // reversed while the keys are here, bold while they are in the
            // sidebar. The `>` in `markers` says which row either way.
            match (index == model.cursor, model.column == Column::Content) {
                (true, true) => line.style(Style::new().add_modifier(Modifier::REVERSED)),
                (true, false) => line.style(Style::new().add_modifier(Modifier::BOLD)),
                (false, _) => line,
            }
        })
        .collect();

    // A row of its own, outside the scrolled area, because a column somebody
    // has to scroll up to read is a column that goes unread.
    let [heading, listing] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(
            Line::from(format!(
                "{MARKERS}{}",
                row(&headers.map(str::to_string), &widths)
            ))
            .style(Style::new().add_modifier(Modifier::BOLD)),
        ),
        heading,
    );
    render_scrolled(frame, listing, rows, model.cursor);
}

/// What governs the active Account: every Setting in force for the Scope it is
/// in, and — written out rather than styled — where each one came from.
///
/// Only what governs *them*. A Setting belonging to a Group somebody is not in
/// is not a rule about them, and a page that listed it would be answering a
/// question they did not ask.
fn render_governing(frame: &mut Frame, model: &Model, area: Rect) {
    let scope = model.governing_scope();
    let mut lines = vec![
        Line::from(""),
        Line::from(format!("  In force for {}:", scope.described()))
            .style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(""),
    ];

    let mut rows: Vec<(String, String, String)> = Vec::new();
    if scope == Scope::Ungrouped {
        // Through the model rather than off the registry, for the reason every
        // row below it goes that way: a write the arrow keys have made and the
        // debounce has not landed yet is what the row is showing, and this line
        // used to be the one that said what was still on disk.
        rows.push((
            "cycle-ungrouped".to_string(),
            model.shown(&scope, &Row::CycleUngrouped).0,
            sourced(&Scope::Global),
        ));
    }
    for setting in crate::commands::config::SETTINGS {
        let (value, source) = model.value_of(&scope, setting);
        rows.push((setting.as_str().to_string(), value, sourced(&source)));
    }

    let key_width = rows
        .iter()
        .map(|(key, ..)| list::cells(key))
        .max()
        .unwrap_or(0);
    let value_width = rows
        .iter()
        .map(|(_, value, _)| list::cells(value))
        .max()
        .unwrap_or(0);
    lines.extend(rows.iter().map(|(key, value, source)| {
        Line::from(format!(
            "  {}  {}  {source}",
            list::padded(key, key_width),
            list::padded(value, value_width),
        ))
    }));

    // And the one place the layering is deliberately not uniform, said here as
    // well as on the panel. Without it this page reads `watcher-may-act false
    // from Global` and a Global `true` would read as in force — which is
    // exactly what ADR 0017 says it is not.
    let caveat = not_in_force(model, &scope);
    if !caveat.is_empty() {
        lines.push(Line::from(""));
        lines.extend(wrapped(caveat.trim(), area.width as usize));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Where a value came from, as words. Read once, over a pipe, on whatever
/// palette — so never as a style alone.
fn sourced(scope: &Scope) -> String {
    match scope {
        Scope::Global => "from Global".to_string(),
        Scope::Ungrouped => "set here".to_string(),
        Scope::Group(name) => format!("set on `{name}`"),
    }
}

// ---- The Config tab ----

/// What each Scope declares, and the keys that change it.
fn render_config(frame: &mut Frame, model: &Model, area: Rect) {
    let rows = model.scope_rows();
    let labels: Vec<String> = rows
        .iter()
        .map(|row| match row {
            ScopeRow::Scope(scope) => scope.title().to_string(),
            ScopeRow::NewGroup => "+ new Group".to_string(),
        })
        .collect();

    // The middle column names each Account the way its owner does — by the
    // Alias where it has one — rather than by the full `named_for_the_user`
    // form, which carries both and is a column and a half wide on its own.
    let accounts = model.scope_accounts();
    let names: Vec<String> = accounts
        .iter()
        .map(|account| {
            model
                .registry()
                .alias_of(account.email())
                .unwrap_or_else(|| account.email())
                .to_string()
        })
        .collect();
    let middle = match names.is_empty() {
        true => 0,
        false => sidebar_width(&names),
    };
    let [sidebar, column, content] = Layout::horizontal([
        Constraint::Length(sidebar_width(&labels)),
        Constraint::Length(middle),
        Constraint::Min(1),
    ])
    .areas(area);

    render_sidebar(
        frame,
        sidebar,
        &labels,
        model.scope_row,
        model.column == Column::Scopes,
    );

    if middle > 0 {
        render_sidebar(
            frame,
            column,
            &names,
            model.account_row.min(names.len().saturating_sub(1)),
            model.column == Column::Accounts,
        );
    }

    render_content(frame, model, content);
}

/// The Scope's Settings, and the facts of the Account beside them.
///
/// An Inherited value is shown dimmed rather than blank: what is in force is
/// what somebody needs to know, and a blank would send them to Global to find
/// out. The dimming is what says this Scope declares nothing of its own — on a
/// page navigated with a cursor, where one keystroke clears an Override and the
/// row that has nothing to clear is the one that is already dim.
fn render_content(frame: &mut Frame, model: &Model, area: Rect) {
    let Some(scope) = model.scope() else {
        return frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from("  `n` names a new Group. It starts empty, Inheriting every Setting."),
                Line::from(
                    "  Deleting one is `perch group remove` — the Accounts survive it, that \
                     Group's Overrides do not.",
                ),
            ]),
            area,
        );
    };

    let rows = model.content_rows();
    let width = rows
        .iter()
        .map(|row| list::cells(row.label()))
        .max()
        .unwrap_or(0);

    let lines: Vec<Line<'_>> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let (value, inherited) = model.shown(&scope, row);
            let line = Line::from(format!("  {}  {value}", list::padded(row.label(), width)));
            let line = match inherited {
                true => line.style(Style::new().add_modifier(Modifier::DIM)),
                false => line,
            };
            match index == model.content_row && model.column == Column::Content {
                true => line.style(Style::new().add_modifier(Modifier::REVERSED)),
                false => line,
            }
        })
        .collect();

    // Wrapped rather than cut, and the room it takes measured from the wrap:
    // the sentence above the rows is what the dimming below is read against,
    // and half of it says nothing.
    let mut said = vec![
        Line::from(format!("  {}", scope.described()))
            .style(Style::new().add_modifier(Modifier::BOLD)),
    ];
    said.extend(wrapped(&declares(model, &scope), area.width as usize));
    let [heading, body] =
        Layout::vertical([Constraint::Length(said.len() as u16), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(said), heading);
    render_scrolled(frame, body, lines, model.content_row);
}

/// What a Scope declares of its own, said above its rows so the dimming below
/// has a sentence to be read against.
fn declares(model: &Model, scope: &Scope) -> String {
    match scope {
        Scope::Global => "What applies where nothing narrower is said.".to_string(),
        _ => {
            let overridden = crate::commands::config::SETTINGS
                .iter()
                .filter(|setting| model.value_of(scope, **setting).1 == *scope)
                .count();
            let declared = match overridden {
                0 => "Inherits every Setting from Global (dimmed).".to_string(),
                1 => "Overrides 1 Setting; the dimmed rows are Global's.".to_string(),
                many => format!("Overrides {many} Settings; the dimmed rows are Global's."),
            };
            format!("{declared}{}", not_in_force(model, scope))
        }
    }
}

/// The one place the layering is deliberately not uniform, said where somebody
/// would otherwise read the watcher's Settings and believe something is running
/// that is not.
///
/// `watcher-may-act` does not reach the Ungrouped Scope by Inheritance: it is
/// gated behind `cycle-ungrouped`, so the watcher acts on those Accounts only
/// where both are on (ADR 0017). With the gate shut, the four numbers under it
/// are a policy nothing is applying.
fn not_in_force(model: &Model, scope: &Scope) -> String {
    match scope {
        // `false` rather than `off`, for the reason
        // `commands::cycling_among_ungrouped` was written down: `on`/`off` is
        // not a value this Setting takes, and a reader who typed back what the
        // screen showed them was refused.
        Scope::Ungrouped if !model.registry().global.cycle_ungrouped => {
            " The watcher Settings are not in force here: `cycle-ungrouped` is \
             false, so nothing acts on these Accounts unasked (ADR 0017)."
                .to_string()
        }
        _ => String::new(),
    }
}

// ---- Shared ----

/// Perch holding nothing at all, said as the state it is rather than as an
/// empty table — in the sentence `perch list` says it in, because it is the
/// same news and names the same way out of it.
fn render_nothing_held(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(format!(
                "  {}",
                list::nothing_here(&list::Scope::Everything)
            )),
        ]),
        area,
    );
}

/// Draws what fits, keeping the line the cursor is on in view.
fn render_scrolled(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>, cursor_line: usize) {
    let offset = scrolled_to(cursor_line, area.height as usize, lines.len());
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), area);
}

/// Which line the listing starts at, so the one the cursor is on is on screen
/// with as much of what is around it as will fit.
///
/// Still a function of the model alone: nothing is remembered between frames, so
/// the same model drawn twice is the same picture both times. Centred, and
/// clamped to the end of the listing so the last screenful is a full one rather
/// than the tail padded with blanks.
fn scrolled_to(cursor_line: usize, height: usize, lines: usize) -> usize {
    let height = height.max(1);
    let centred = cursor_line.saturating_sub(height / 2);
    centred.min(lines.saturating_sub(height))
}

/// The width of the two markers every row starts with, and the blank the
/// header sits behind instead.
const MARKERS: &str = "   ";

/// Where the cursor is, and which Account is active — as characters rather than
/// as styling, because the styling is what a terminal at the far end of an SSH
/// session may not have.
fn markers(model: &Model, account: &Account, index: usize) -> String {
    format!(
        "{}{} ",
        if index == model.cursor { '>' } else { ' ' },
        if model.registry().is_active(account.email()) {
            '*'
        } else {
            ' '
        },
    )
}

/// The shared columns with the Headroom one after them, for a row of cells and
/// for the headers alike.
fn with_headroom<T>(shared: [T; COLUMNS], headroom: T) -> [T; ACCOUNT_COLUMNS] {
    let mut taking = shared.into_iter().chain(std::iter::once(headroom));
    std::array::from_fn(|_| taking.next().expect("one more than the shared columns"))
}

/// One row of the listing, padded to the columns it shares with `perch list`.
fn row<const N: usize>(cells: &[String; N], widths: &[usize; N]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| list::padded(cell, *width))
        .collect::<Vec<String>>()
        .join("  ")
        .trim_end()
        .to_string()
}

/// As much of that as there is room for, broken between words and with what is
/// being left out counted rather than dropped.
///
/// ADR 0018 has a failed Refresh report every Account it could not read by
/// name, and a display that clipped the last two of five would be one that had
/// quietly stopped doing that.
fn as_many_as_fit(notes: Vec<String>, width: usize, most: usize) -> Vec<String> {
    let wrapped: Vec<Vec<String>> = notes.iter().map(|note| broken(note, width)).collect();
    if wrapped.iter().map(Vec::len).sum::<usize>() <= most {
        return wrapped.concat();
    }

    // One line is kept back for the count, which is why a note is only taken
    // whole while there is still room for it and that line.
    let mut lines: Vec<String> = Vec::new();
    let mut said = 0;
    for note in &wrapped {
        if lines.len() + note.len() + 1 > most {
            break;
        }
        lines.extend(note.iter().cloned());
        said += 1;
    }
    lines.push(format!("...and {} more.", notes.len() - said));
    lines
}

/// One note as the lines it takes at this width, broken between words.
///
/// A word of its own longer than the line is left long rather than split: the
/// long words here are email addresses and commands to type, and half of either
/// is worse than one the terminal cuts where the reader can see it was cut.
fn broken(note: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in note.split_whitespace() {
        if line.is_empty() {
            line = word.to_string();
        } else if list::cells(&line) + 1 + list::cells(word) <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// What is said above the keys: what a Refresh is doing or could not do, and
/// what the last act said. Nothing at all in the ordinary case, where the
/// figures speak for themselves by carrying their age.
///
/// The two share one region because they are the same kind of thing — news
/// about the machine that the page cannot show — and a second region kept empty
/// most of the time is rows taken away from what is on it.
///
/// What was just done comes first, and what a Refresh said follows it. Only one
/// of them is answering a key that was pressed a moment ago, and what is left
/// out when there is not room for both is the older news.
fn notes(model: &Model) -> Vec<String> {
    let mut notes = model.said.clone();
    notes.extend(match &model.refreshing {
        Refreshing::Unasked => Vec::new(),
        Refreshing::Waiting => {
            vec!["Refreshing. What is on screen is what was known before it.".to_string()]
        }
        Refreshing::Back(notes) => notes.clone(),
    });
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole listing on a screen with room for it does not scroll at all.
    #[test]
    fn a_listing_that_fits_is_not_scrolled() {
        assert_eq!(scrolled_to(0, 10, 10), 0);
        assert_eq!(scrolled_to(9, 10, 10), 0);
    }

    /// Past the fold, the cursor is brought into view with what is around it
    /// rather than pinned to the bottom row — and the last screenful is a full
    /// one rather than the tail padded with blanks.
    #[test]
    fn a_cursor_below_the_fold_is_shown_with_what_is_around_it() {
        assert_eq!(scrolled_to(10, 10, 40), 5, "centred");
        assert_eq!(scrolled_to(25, 10, 40), 20);
        assert_eq!(
            scrolled_to(39, 10, 40),
            30,
            "at the end it stops rather than scrolling into blank space"
        );
    }

    /// The property the fix is about: something below the cursor is on screen
    /// wherever the cursor is, so the next candidate and how many are left can
    /// both be seen.
    #[test]
    fn there_is_always_something_below_the_cursor_to_see() {
        let (height, lines) = (10, 40);
        for cursor in 0..lines - 1 {
            let offset = scrolled_to(cursor, height, lines);
            assert!(
                offset <= cursor && cursor < offset + height,
                "the cursor is on screen at {cursor}"
            );
            assert!(
                cursor + 1 < offset + height,
                "and so is the line under it, at {cursor}"
            );
        }
    }

    /// A terminal squeezed to nothing is one row rather than a division by
    /// zero.
    #[test]
    fn a_screen_with_no_room_still_shows_the_row_the_cursor_is_on() {
        assert_eq!(scrolled_to(4, 0, 10), 4);
    }

    const WIDE: usize = 80;
    /// The allowance on an ordinary terminal.
    const MOST: usize = 3;

    fn note(which: usize) -> String {
        format!("account-{which}@example.com: no answer from Anthropic")
    }

    #[test]
    fn every_note_there_is_room_for_is_said_whole() {
        let said = as_many_as_fit((0..MOST).map(note).collect(), WIDE, MOST);

        assert_eq!(said, (0..MOST).map(note).collect::<Vec<String>>());
    }

    /// ADR 0018 has a failed Refresh name every Account it could not read.
    #[test]
    fn the_notes_there_is_no_room_for_are_counted_rather_than_dropped() {
        let said = as_many_as_fit((0..5).map(note).collect(), WIDE, MOST);

        assert_eq!(said.len(), MOST);
        assert_eq!(said[0], note(0));
        assert_eq!(said[MOST - 1], "...and 3 more.");
    }

    /// A Switch says several sentences and they are all worth reading, so the
    /// allowance grows with the terminal — but never past the share of it the
    /// page needs to stay the thing on screen.
    #[test]
    fn what_is_said_may_take_a_third_of_the_frame_and_no_more() {
        assert_eq!(most_notes(24), 8);
        assert_eq!(most_notes(9), 3);
        assert_eq!(
            most_notes(2),
            1,
            "a terminal with no room for a third still says one thing"
        );
    }

    /// The last clause of one of these names the command that repairs the
    /// Account, so a note is broken between words rather than cut at the width.
    #[test]
    fn a_note_too_long_for_the_line_is_broken_between_words() {
        let repair = "spare@example.com: Anthropic would not renew its Credential. \
                      `perch relogin spare@example.com` repairs it.";

        let said = as_many_as_fit(vec![repair.to_string()], 40, MOST);

        assert!(said.len() > 1, "{said:?}");
        assert!(said.iter().all(|line| list::cells(line) <= 40), "{said:?}");
        assert_eq!(
            said.join(" ").split_whitespace().collect::<Vec<&str>>(),
            repair.split_whitespace().collect::<Vec<&str>>(),
            "every word survives the break"
        );
    }

    /// Half an email address is worse than one the reader can see was cut.
    #[test]
    fn a_word_longer_than_the_line_is_left_whole() {
        assert_eq!(
            broken("a-very-long-account-name@example.com: no", 12),
            ["a-very-long-account-name@example.com:", "no"]
        );
    }

    /// A bar reading `Conf` against an email address is one nobody can tell the
    /// views apart from.
    #[test]
    fn a_row_with_no_room_for_both_keeps_the_tabs() {
        let label = list::cells("active: someone@example.com ");

        assert!(room_for_both(80, label));
        assert!(!room_for_both(40, label));
        assert!(
            room_for_both(20, 0),
            "the tab bar's own width is what it needs"
        );
    }

    /// Provenance on `Status` is words rather than a style, because that page
    /// is read once and has to survive a pipe and a colour-blind palette.
    #[test]
    fn where_a_value_came_from_is_written_out_rather_than_only_styled() {
        assert_eq!(sourced(&Scope::Global), "from Global");
        assert!(sourced(&Scope::Group("work".to_string())).contains("work"));
        assert!(sourced(&Scope::Ungrouped).contains("here"));
    }
}
