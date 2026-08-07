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

use crate::commands::list::{self, COLUMNS};
use crate::tui::model::{Model, Refreshing, Tab};
use crate::utilization;

/// The most lines given over to saying what a Refresh did. Beyond that the
/// figures themselves are the more useful thing to be looking at, and every
/// note is about an Account whose figure says how old it is anyway.
const MOST_NOTES: usize = 3;

/// What the keys do, along the bottom of every frame.
///
/// On screen rather than under `?`, because a picker whose keys have to be
/// discovered is one people leave by killing the terminal. ASCII, because this
/// is the line that has to be legible on the terminal at the far end of an SSH
/// session.
const KEYS: &str = "q  quit    Tab  view    Up/Down  move    r  refresh";

/// Draws the whole frame.
pub fn render(frame: &mut Frame, model: &Model) {
    let notes = as_many_as_fit(notes(model), frame.area().width as usize);
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
    let active = match &model.registry.active {
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

/// The Accounts, as things to choose between: what each one is called, what it
/// is interchangeable with, and whether it is any use.
fn render_accounts(frame: &mut Frame, model: &Model, area: Rect) {
    if model.accounts().is_empty() {
        return render_nothing_held(frame, area);
    }

    let cells: Vec<[String; COLUMNS]> = model
        .accounts()
        .iter()
        .map(|account| list::columns(&model.registry, account))
        .collect();
    let widths = list::widths(&cells);

    let mut lines = vec![
        Line::from(format!(
            "{MARKERS}{}",
            row(&list::HEADERS.map(str::to_string), &widths)
        ))
        .style(Style::new().add_modifier(Modifier::BOLD)),
    ];
    lines.extend(cells.iter().enumerate().map(|(index, cells)| {
        let line = Line::from(format!("{}{}", markers(model, index), row(cells, &widths)));
        match index == model.cursor {
            true => line.style(Style::new().add_modifier(Modifier::REVERSED)),
            false => line,
        }
    }));

    // One for the header, which does not scroll away: a column somebody has to
    // scroll up to read is a column that goes unread.
    render_scrolled(frame, area, lines, model.cursor + 1);
}

/// The figures, one Account at a time, each with the age of the observation it
/// came from (ADR 0015).
///
/// A block per Account rather than a row, because an Account has several Quota
/// Windows at once and is limited by whichever fills first: one line per
/// Account would have to pick one of them, and the one it picked would be the
/// one hiding the other.
fn render_utilization(frame: &mut Frame, model: &Model, area: Rect) {
    if model.accounts().is_empty() {
        return render_nothing_held(frame, area);
    }

    let mut lines = Vec::new();
    let mut cursor_line = 0;
    for (index, account) in model.accounts().iter().enumerate() {
        if index == model.cursor {
            cursor_line = lines.len();
        }
        let heading = Line::from(format!("{}{}", markers(model, index), account.email()));
        lines.push(match index == model.cursor {
            true => heading.style(Style::new().add_modifier(Modifier::REVERSED)),
            false => heading.style(Style::new().add_modifier(Modifier::BOLD)),
        });
        lines.extend(
            utilization::lines(account, model.now)
                .into_iter()
                .map(|figure| Line::from(format!("    {figure}"))),
        );
        lines.push(Line::from(""));
    }

    render_scrolled(frame, area, lines, cursor_line);
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
fn markers(model: &Model, index: usize) -> String {
    let account = &model.accounts()[index];
    format!(
        "{}{} ",
        if index == model.cursor { '>' } else { ' ' },
        if model.registry.active.as_deref() == Some(account.email()) {
            '*'
        } else {
            ' '
        },
    )
}

/// One row of the listing, padded to the columns it shares with `perch list`.
/// What goes *in* those columns is that listing's to say ([`list::columns`]);
/// this only lays them out, which is the half a frame does differently from a
/// line.
fn row(cells: &[String; COLUMNS], widths: &[usize; COLUMNS]) -> String {
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
fn as_many_as_fit(notes: Vec<String>, width: usize) -> Vec<String> {
    let wrapped: Vec<Vec<String>> = notes.iter().map(|note| broken(note, width)).collect();
    if wrapped.iter().map(Vec::len).sum::<usize>() <= MOST_NOTES {
        return wrapped.concat();
    }

    // One line is kept back for the count, which is why a note is only taken
    // whole while there is still room for it and that line.
    let mut lines: Vec<String> = Vec::new();
    let mut said = 0;
    for note in &wrapped {
        if lines.len() + note.len() + 1 > MOST_NOTES {
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

/// What is said above the keys: what a Refresh is doing, or what it could not
/// do. Nothing at all in the ordinary case, where the figures speak for
/// themselves by carrying their age.
fn notes(model: &Model) -> Vec<String> {
    match &model.refreshing {
        Refreshing::Unasked => Vec::new(),
        Refreshing::Waiting => {
            vec!["Refreshing. What is on screen is what was known before it.".to_string()]
        }
        Refreshing::Back(notes) => notes.clone(),
    }
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

    fn note(which: usize) -> String {
        format!("account-{which}@example.com: no answer from Anthropic")
    }

    #[test]
    fn every_note_there_is_room_for_is_said_whole() {
        let said = as_many_as_fit((0..MOST_NOTES).map(note).collect(), WIDE);

        assert_eq!(said, (0..MOST_NOTES).map(note).collect::<Vec<String>>());
    }

    /// ADR 0018 has a failed Refresh name every Account it could not read.
    /// There is not always room for all of them, and the ones there is no room
    /// for are counted rather than dropped: an Account nobody was told about is
    /// an Account whose figure is silently old.
    #[test]
    fn the_notes_there_is_no_room_for_are_counted_rather_than_dropped() {
        let said = as_many_as_fit((0..5).map(note).collect(), WIDE);

        assert_eq!(said.len(), MOST_NOTES);
        assert_eq!(said[0], note(0));
        assert_eq!(said[MOST_NOTES - 1], "...and 3 more.");
    }

    /// The last clause of one of these names the command that repairs the
    /// Account, so a note is broken between words rather than cut at the width.
    #[test]
    fn a_note_too_long_for_the_line_is_broken_between_words() {
        let repair = "spare@example.com: Anthropic would not renew its Credential. \
                      `perch relogin spare@example.com` repairs it.";

        let said = as_many_as_fit(vec![repair.to_string()], 40);

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
