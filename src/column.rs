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

use crate::commands::write_failed;
use crate::error::Result;
use crate::host::Shown;

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

/// How wide the label column is on the surfaces that answer about one Account —
/// `status`, and `switch` when it says where you landed. They are read one
/// after the other, so they line up.
pub const LABEL_WIDTH: usize = 14;

/// Writes a label and a value in that column, for the surfaces that render an
/// Account as labeled lines.
pub fn write_labeled(out: &mut dyn Write, label: &str, value: &Shown) -> Result<()> {
    writeln!(out, "{}{value}", padded(&Shown::of(label), LABEL_WIDTH)).map_err(write_failed)
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
        write_labeled(&mut written, "作業", &Shown::of("Overflow Ltd")).unwrap();
        let line = String::from_utf8(written).unwrap();
        assert_eq!(
            line.find("Overflow").unwrap(),
            LABEL_WIDTH - cells(&Shown::of("作業")) + "作業".len(),
            "the value starts in the column every other labeled row starts in"
        );
    }
}
