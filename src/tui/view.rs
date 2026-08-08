//! What one frame looks like.
//!
//! Drawing and nothing else: everything here reads a [`Model`] and writes
//! cells, so the same function draws the real terminal and the buffer a test
//! reads back.
//!
//! Nothing on screen means anything by its colour. A Perch run over SSH lands
//! on whatever terminal is at the other end, and the one that cannot do colour
//! must still be able to tell the Account under the cursor from the active one
//! — so those are a `>` and a `*`, and the styling on top of them is reverse
//! video and bold, which every terminal that draws anything can do.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Tabs};

use crate::commands::CYCLING_AMONG_UNGROUPED;
use crate::commands::list::{self, COLUMNS};
use crate::cycle;
use crate::registry::Account;
use crate::reserve::Reserve;
use crate::tui::model::{Model, Refreshing, Tab};
use crate::utilization;

/// The share of the frame that what was said may take before it is counted
/// rather than shown.
///
/// A Switch says several sentences and a failed Refresh names an Account per
/// line, and both are worth reading — but the listing is what the command is
/// for, and a report that pushed it off the screen would be the picker
/// answering a question nobody asked. A third, and never less than one line.
fn most_notes(height: usize) -> usize {
    (height / 3).max(1)
}

/// What the keys do, along the bottom of every frame.
///
/// On screen rather than under `?`, because a picker whose keys have to be
/// discovered is one people leave by killing the terminal. ASCII, because this
/// is the line that has to be legible on the terminal at the far end of an SSH
/// session.
const KEYS: &str = "q  quit   Tab  view   Up/Down  move   Enter  switch   x  run   r  refresh";

/// Draws the whole frame.
pub fn render(frame: &mut Frame, model: &Model) {
    let notes = as_many_as_fit(
        notes(model),
        frame.area().width as usize,
        most_notes(frame.area().height as usize),
    );
    let [bar, body, said, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(notes.len() as u16),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_bar(frame, model, bar);
    match model.tab {
        Tab::Accounts => render_accounts(frame, model, body),
        Tab::Utilization => render_utilization(frame, model, body),
    }
    frame.render_widget(
        Paragraph::new(notes.into_iter().map(Line::from).collect::<Vec<Line<'_>>>()),
        said,
    );
    frame.render_widget(
        Paragraph::new(Line::from(KEYS).style(Style::new().add_modifier(Modifier::DIM))),
        keys,
    );
}

/// The tab bar, and the one fact that belongs to no tab: which Account is
/// active. It is on every frame because it is what the whole command is about —
/// the Utilization tab would otherwise be a table of figures with no "you are
/// here" in it.
fn render_bar(frame: &mut Frame, model: &Model, area: Rect) {
    let active = match &model.registry().active {
        Some(email) => format!("active: {email} "),
        None => "no active Account ".to_string(),
    };

    // The tabs come first and whole. A terminal too narrow for both drops the
    // label rather than sharing the row out: a bar reading `Utiliz` against an
    // email address is one nobody can tell the views apart from, and which view
    // this is is the thing the keys act on.
    let label = match room_for_both(area.width as usize, active.chars().count()) {
        true => active.chars().count() as u16,
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
///
/// The tab bar's width is what ratatui will draw it in — every title with a
/// space either side of it, and a divider between each pair — measured from the
/// same array the bar is built from, so a third view cannot make this go quietly
/// wrong.
fn room_for_both(width: usize, label: usize) -> bool {
    let tabs: usize = Tab::ALL
        .iter()
        .map(|tab| tab.title().chars().count() + 2)
        .sum::<usize>()
        + Tab::ALL.len()
        - 1;
    width >= tabs + label
}

/// The columns of the Accounts view: `perch list`'s own, and the figure the
/// order was made on beside them.
///
/// Headroom is added here rather than shared with that listing, because the two
/// surfaces differ in exactly the way this column exists for. `perch list`
/// prints every Quota Window an Account has, which is the evidence. A picker
/// ranks the Accounts, so it owes the one number the ranking was made on —
/// otherwise the order is a claim with nothing on screen to check it against.
const ACCOUNT_COLUMNS: usize = COLUMNS + 1;
const HEADROOM: &str = "Headroom";

/// The Accounts, as things to choose between: what each one is called, what it
/// is interchangeable with, how much of it is left, and whether it is any use.
///
/// In the order a Cycle ranks them ([`crate::cycle::ranked`]), Group by Group,
/// so the ranking `perch switch` makes is visible rather than hidden.
fn render_accounts(frame: &mut Frame, model: &Model, area: Rect) {
    let accounts = model.accounts();
    if accounts.is_empty() {
        return render_nothing_held(frame, area);
    }

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
            match index == model.cursor {
                true => line.style(Style::new().add_modifier(Modifier::REVERSED)),
                false => line,
            }
        })
        .collect();

    // A row of its own, outside the scrolled area, because a column somebody
    // has to scroll up to read is a column that goes unread. Inside it the
    // header is line zero of the paragraph, and a paragraph scrolled by n
    // simply skips its first n lines — so the one line that has to stay is the
    // first one to go, as soon as the listing is taller than the frame.
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

/// The figures, at the two levels there are honest figures for: one Account, and
/// one Group (ADR 0015 for the age on every one of them).
///
/// A block per Account rather than a row, because an Account has several Quota
/// Windows at once and is limited by whichever fills first: one line per
/// Account would have to pick one of them, and the one it picked would be the
/// one hiding the other. Above the block, the Headroom those rows come to —
/// taken from the fullest of them (ADR 0012), and naming which, so the figure
/// can be checked against the rows underneath rather than taken on trust.
///
/// Above each Group, its Reserve and one row per Quota Window kind
/// ([`crate::reserve`]). There is deliberately **no total**: Accounts sit on
/// different plans and Perch only ever sees percentages, so nothing here sums or
/// averages anything, and every figure on a Group's rows is one an Account
/// actually reported.
fn render_utilization(frame: &mut Frame, model: &Model, area: Rect) {
    let accounts = model.accounts();
    if accounts.is_empty() {
        return render_nothing_held(frame, area);
    }

    // One column for the Headroom of every Account on screen, so the figures
    // line up down the view and the eye can run over them rather than hunting
    // for each one at the end of a different-length address.
    let widest = accounts
        .iter()
        .map(|account| account.email().chars().count())
        .max()
        .unwrap_or_default();

    let mut lines = Vec::new();
    let mut cursor_line = 0;
    for section in model.sections() {
        lines.push(
            Line::from(section.scope.heading()).style(Style::new().add_modifier(Modifier::BOLD)),
        );
        lines.extend(
            group_figures(model, &section.scope)
                .into_iter()
                .map(|figure| Line::from(format!("  {figure}"))),
        );
        lines.push(Line::from(""));

        for index in section.rows.clone() {
            let account = accounts[index];
            let heading = Line::from(format!(
                "{}{:widest$}   Headroom {}",
                markers(model, account, index),
                account.email(),
                cycle::headroom_in_full(account, model.now),
            ));
            lines.push(match index == model.cursor {
                true => heading.style(Style::new().add_modifier(Modifier::REVERSED)),
                false => heading.style(Style::new().add_modifier(Modifier::BOLD)),
            });
            // Nothing under an Account nobody has ever read a figure for: the
            // Headroom beside its name has already said so, and a row saying it
            // again reads as a Quota Window called "never observed".
            if account.observed_utilization().is_some() {
                lines.extend(
                    utilization::lines_with_resets(account, model.now)
                        .into_iter()
                        .map(|figure| Line::from(format!("      {figure}"))),
                );
            }

            // The *last* line of the selected Account's block rather than its
            // heading. `scrolled_to` pins the line it is given to the bottom of
            // the view, which is right on the Accounts tab where the row is the
            // whole of the content — but here the Quota Window rows come after
            // the heading, so pinning the heading scrolls every one of them off.
            // Selecting an Account below the fold showed its name and hid its
            // figures, on the one tab whose whole purpose is the figures.
            if index == model.cursor {
                cursor_line = lines.len().saturating_sub(1);
            }
            lines.push(Line::from(""));
        }
    }

    render_scrolled(frame, area, lines, cursor_line);
}

/// What is said above one scope's Accounts.
///
/// A Group is a declaration that its Accounts are interchangeable, which is what
/// makes "what is left across them" a question with an answer. Being in no Group
/// is the absence of that declaration (ADR 0017), so those Accounts get a
/// heading and nothing else until `cycle-ungrouped` says a Cycle may move
/// between them — a Reserve over Accounts nobody has said are interchangeable
/// would be a figure about a set that is not one.
fn group_figures(model: &Model, scope: &cycle::Scope) -> Vec<String> {
    if !cycle::may_cycle_within(model.registry(), scope) {
        return vec![format!("Cycling {CYCLING_AMONG_UNGROUPED}.")];
    }
    let reserve = Reserve::of(model.registry(), scope);
    let mut figures = reserve.lines(model.now);
    figures.extend(reserve.window_lines(model.now));
    figures
}

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
    let offset = scrolled_to(cursor_line, area.height as usize);
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), area);
}

/// Which line the listing starts at, so the one the cursor is on is on screen.
///
/// It scrolls exactly far enough and no further, which is what makes the frame
/// a function of the model alone: nothing here is remembered between frames, so
/// the same model drawn twice is the same picture both times.
fn scrolled_to(cursor_line: usize, height: usize) -> usize {
    (cursor_line + 1).saturating_sub(height.max(1))
}

/// The width of the two markers every row starts with, and the blank the
/// header sits behind instead.
const MARKERS: &str = "   ";

/// Where the cursor is, and which Account is active — as characters rather than
/// as styling, because the styling is what a terminal at the far end of an SSH
/// session may not have. `>` is the row the keys act on and `*` is the Account
/// every client is currently using; they are separate facts and are shown as
/// two.
fn markers(model: &Model, account: &Account, index: usize) -> String {
    format!(
        "{}{} ",
        if index == model.cursor { '>' } else { ' ' },
        if model.registry().active.as_deref() == Some(account.email()) {
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
/// What goes *in* those columns is that listing's to say ([`list::columns`]);
/// this only lays them out, which is the half a frame does differently from a
/// line.
fn row<const N: usize>(cells: &[String; N], widths: &[usize; N]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:width$}"))
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
/// quietly stopped doing that: an Account nobody was told about is an Account
/// whose figure is silently old. So what is dropped is a whole note and a
/// count, never the end of a sentence — a note cut at the width loses its last
/// clause, which is the half naming the command that puts it right.
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
        } else if line.chars().count() + 1 + word.chars().count() <= width {
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
/// about the machine that the listing cannot show — and a second region kept
/// empty most of the time is rows taken away from the Accounts.
///
/// What was just done comes first, and what a Refresh said follows it. Only one
/// of them is answering a key that was pressed a moment ago, and what is left
/// out when there is not room for both is the older news — a Refresh that named
/// five unreadable Accounts stands until something is done, and would otherwise
/// take the whole region for as long as the view is open.
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
        assert_eq!(scrolled_to(0, 10), 0);
        assert_eq!(scrolled_to(9, 10), 0);
    }

    /// Past the bottom, it scrolls exactly far enough to bring the cursor into
    /// view and no further.
    #[test]
    fn a_cursor_below_the_fold_scrolls_the_listing_to_it() {
        assert_eq!(scrolled_to(10, 10), 1);
        assert_eq!(scrolled_to(25, 10), 16);
    }

    /// A terminal squeezed to nothing is one row rather than a division by
    /// zero.
    #[test]
    fn a_screen_with_no_room_still_shows_the_row_the_cursor_is_on() {
        assert_eq!(scrolled_to(4, 0), 4);
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
    /// There is not always room for all of them, and the ones there is no room
    /// for are counted rather than dropped: an Account nobody was told about is
    /// an Account whose figure is silently old.
    #[test]
    fn the_notes_there_is_no_room_for_are_counted_rather_than_dropped() {
        let said = as_many_as_fit((0..5).map(note).collect(), WIDE, MOST);

        assert_eq!(said.len(), MOST);
        assert_eq!(said[0], note(0));
        assert_eq!(said[MOST - 1], "...and 3 more.");
    }

    /// A Switch says several sentences and they are all worth reading, so the
    /// allowance grows with the terminal — but never past the share of it the
    /// listing needs to stay the thing on screen.
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
        assert!(
            said.iter().all(|line| line.chars().count() <= 40),
            "{said:?}"
        );
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

    /// A bar reading `Utiliz` against an email address is one nobody can tell
    /// the views apart from.
    #[test]
    fn a_row_with_no_room_for_both_keeps_the_tabs() {
        let label = "active: someone@example.com ".chars().count();

        assert!(room_for_both(80, label));
        assert!(!room_for_both(46, label));
        assert!(
            room_for_both(24, 0),
            "the tab bar's own width is what it needs"
        );
    }
}
