//! What a column is measured in, and padded to.
//!
//! A terminal cell rather than a character or a byte: a CJK Group name is drawn
//! two columns per character and a combining mark in none, and a width is
//! shared down a column, so one such name puts a whole block out of line.
//!
//! Named for the column rather than for what stands in one. `list` lays out
//! Accounts, `status` labels an Account's lines and `group` labels a Group's,
//! and none of the three is about a Quota Window
//! (ADR code-lives-where-it-reaches).

use std::io::Write;

use unicode_width::UnicodeWidthStr;

use crate::error::Result;
use crate::host::Shown;
use crate::say;

/// How many terminal cells a string is drawn in. Not its bytes and not its
/// characters: a CJK Group name is drawn two columns per character and a
/// combining mark in none, and because a width is shared down a column, one such
/// name puts a whole block out of line. `unicode-width` carries the East Asian
/// width table nobody should keep by hand (ADR a-crate-must-not-cost-a-seam).
pub fn cells(text: &Shown) -> usize {
    UnicodeWidthStr::width(text.as_str())
}

/// `text` with spaces after it until it fills `width` cells.
///
/// `format!("{text:width$}")` pads to a count of characters, which is
/// [`cells`]'s mistake from the other side: it would take the right width and
/// then fill it wrongly.
pub fn padded(text: &Shown, width: usize) -> String {
    let mut out = String::with_capacity(width.max(text.as_str().len()));
    pad_into(&mut out, text, cells(text), width);
    out
}

/// The same into a buffer the caller keeps, and against a width already
/// measured: a table measures every cell to decide its column, so the padding
/// is the second reading of a number the column already knows.
pub fn pad_into(out: &mut String, text: &Shown, measured: usize, width: usize) {
    out.push_str(text.as_str());
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(measured)));
}

/// One aligned label column: every label padded to the same number of cells,
/// behind the same indent, the value after it. A value rather than a helper
/// per surface, because the width is the agreement between the rows — spelled
/// at one site, it cannot drift into a second constant or a `format!` that
/// counts characters.
#[derive(Clone, Copy)]
pub struct Labeled {
    indent: usize,
    width: usize,
}

impl Labeled {
    /// The label column of the surfaces that answer about one Account —
    /// `status`, and the figures under it. They are read one after the other,
    /// so they line up.
    pub fn the_account_column() -> Labeled {
        Labeled {
            indent: 0,
            width: 14,
        }
    }

    /// A column `width` cells wide, `indent` spaces in.
    pub fn of(indent: usize, width: usize) -> Labeled {
        Labeled { indent, width }
    }

    /// One row of the column.
    pub fn row(&self, label: &str, value: &Shown) -> String {
        let label = Shown::of(label);
        let mut out = String::with_capacity(self.indent + self.width + value.as_str().len());
        out.extend(std::iter::repeat_n(' ', self.indent));
        pad_into(&mut out, &label, cells(&label), self.width);
        out.push_str(value.as_str());
        out
    }

    /// The same row, written.
    pub fn write(&self, out: &mut dyn Write, label: &str, value: &Shown) -> Result<()> {
        writeln!(out, "{}", self.row(label, value)).map_err(say::failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_is_measured_in_the_cells_a_terminal_draws_it_in() {
        assert_eq!(cells(&Shown::of("作業")), 4, "two characters, four columns");
        assert_eq!(
            cells(&Shown::of("øverfløw")),
            8,
            "and a narrow one is still one each, not the ten bytes it occupies"
        );
        assert_eq!(
            cells(&Shown::of("5-hour\u{1b}[2K")),
            "5-hour[2K".len(),
            "and a column is measured on what will be drawn: the character a \
             terminal would act on is gone by the time this is asked"
        );
        assert_eq!(
            cells(&Shown::of("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}")),
            2,
            "while what a terminal draws as one glyph is still one glyph here: \
             the joiners survive the strip, so this is not three emoji wide"
        );
    }

    #[test]
    fn padding_fills_a_width_in_cells_rather_than_in_characters() {
        assert_eq!(
            padded(&Shown::of("作業"), 6),
            "作業  ",
            "four cells, two to fill"
        );
        assert_eq!(padded(&Shown::of("ab"), 4), "ab  ");
        assert_eq!(
            padded(&Shown::of("5-hour\u{1b}[2K"), 10),
            "5-hour[2K ",
            "and what a terminal would act on is not written at all, while what \
             it would draw is"
        );
        assert_eq!(
            padded(&Shown::of("作業作業"), 4),
            "作業作業",
            "and nothing is trimmed to fit: a cell count is a floor here"
        );
    }

    /// `status` is where an organization name lands, and an organization name is
    /// whatever Anthropic holds.
    #[test]
    fn a_labeled_row_is_padded_in_cells_like_every_other_column() {
        let mut written = Vec::new();
        let labeled = Labeled::the_account_column();
        labeled
            .write(&mut written, "作業", &Shown::of("Overflow Ltd"))
            .unwrap();
        let line = String::from_utf8(written).unwrap();
        assert_eq!(
            line.find("Overflow").unwrap(),
            14 - cells(&Shown::of("作業")) + "作業".len(),
            "the value starts in the column every other labeled row starts in"
        );
    }

    #[test]
    fn an_indented_column_pads_its_labels_in_cells_too() {
        assert_eq!(
            Labeled::of(2, 9).row("作業", &Shown::of("held")),
            "  作業     held",
            "two spaces in, then five cells of padding after four of label"
        );
    }
}
