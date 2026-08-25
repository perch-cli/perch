//! How a cached Utilization figure is said — in prose and in JSON.
//!
//! Every surface renders Utilization from cache and shows each figure with its
//! age (ADR a-figure-carries-its-age). Nothing here reaches the network: a
//! `--refresh` fetches before rendering rather than while. `status` and `list`
//! say the same thing about the same figure, so how a figure reads lives here
//! rather than being spelled out again by each of them.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;
use unicode_width::UnicodeWidthStr;

use crate::commands::write_failed;
use crate::error::Result;
use crate::host::Shown;
use crate::registry::{Account, CachedUtilization};

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
    format!("{text}{}", " ".repeat(width.saturating_sub(cells(text))))
}

/// How wide the label column is on the surfaces that answer about one Account —
/// `status`, and `switch` when it says where you landed. They are read one
/// after the other, so they line up.
pub const LABEL_WIDTH: usize = 14;

/// How wide the Quota Window name column has to be for these windows, and never
/// narrower than the two every Account has. Measured rather than fixed, because
/// a window's name is Anthropic's and there is one per model: a fixed width
/// either steps the per-model rows out of line or widens every block whether it
/// needs it or not. The floor keeps a block of short names in the usual column.
fn window_width(windows: impl Iterator<Item = Shown>) -> usize {
    windows
        .chain(std::iter::once(Shown::of("5-hour")))
        .map(|window| cells(&window))
        .max()
        // The floor is chained on, so there is always one.
        .expect("the floor is always among them")
}

/// The width to lay Quota Window names out in, measured across every Account
/// whose figures are shown together, because a surface that stacks one Account's
/// rows under the next one's has one column and not one per Account — measured
/// per Account, an Opus-eligible Account beside a `pro` one puts two percentages
/// five columns apart under one `Utilization` heading.
pub fn window_width_across<'a>(accounts: impl IntoIterator<Item = &'a Account>) -> usize {
    window_width(
        accounts
            .into_iter()
            .filter_map(Account::observed_utilization)
            .flat_map(|cached| {
                cached
                    .windows
                    .iter()
                    .map(|window| Shown::of(&window.window))
            }),
    )
}

/// Writes a label and a value in that column, for the surfaces that render an
/// Account as labeled lines.
pub fn write_labeled(out: &mut dyn Write, label: &str, value: &Shown) -> Result<()> {
    writeln!(out, "{}{value}", padded(&Shown::of(label), LABEL_WIDTH)).map_err(write_failed)
}

/// Writes the cached Utilization under one `Utilization` label, however many
/// Quota Windows there turn out to be.
pub fn write_figures(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    // One Account's block, so the set to lay the names out across is that
    // Account alone.
    let width = window_width_across([account]);
    for (index, figure) in lines(account, now, width).iter().enumerate() {
        let label = if index == 0 { "Utilization" } else { "" };
        write_labeled(out, label, &Shown::of(figure))?;
    }
    Ok(())
}

/// One line per Quota Window, each carrying its own age — or the single line
/// that says nothing has ever been observed.
///
/// An Account with no observation is never rendered as zero: "no figure" and
/// "plenty of room" are opposite pieces of advice.
pub fn lines(
    account: &Account,
    now: DateTime<Utc>,
    // The caller's, not this Account's: see [`window_width_across`].
    width: usize,
) -> Vec<String> {
    match account.observed_utilization() {
        None => vec!["never observed".to_string()],
        Some(cached) => {
            let age = age_phrase(cached.observed_at, now);
            cached
                .windows
                .iter()
                .map(|window| {
                    format!(
                        "{} {:>3}%  (as of {age})",
                        padded(&Shown::of(&window.window), width),
                        percentage(window.used_percent)
                    )
                })
                .collect()
        }
    }
}

/// The cached Utilization as a script reads it: every figure carries its own
/// observation time, so the script can judge freshness for itself.
pub fn document(account: &Account, now: DateTime<Utc>) -> serde_json::Value {
    match account.observed_utilization() {
        Some(cached) => json!({
            "observed_at": cached.observed_at.to_rfc3339(),
            "never_observed": false,
            "windows": windows_json(cached, now),
        }),
        None => json!({
            "observed_at": serde_json::Value::Null,
            "never_observed": true,
            "windows": [],
        }),
    }
}

fn windows_json(cached: &CachedUtilization, now: DateTime<Utc>) -> Vec<serde_json::Value> {
    cached
        .windows
        .iter()
        .map(|window| {
            json!({
                "window": window.window,
                "used_percent": window.used_percent,
                "resets_at": window.resets_at.map(|at| at.to_rfc3339()),
                "observed_at": cached.observed_at.to_rfc3339(),
                // Not clamped at nought: clamping reports a figure stamped in
                // the future as maximally fresh, which is what `age_phrase`
                // refuses to claim in prose about the same cache.
                "observed_seconds_ago": (now - cached.observed_at).num_seconds(),
            })
        })
        .collect()
}

/// A percentage as every surface prints one, without the two roundings that say
/// the opposite of what is true. `{:.0}` renders 0.4 as `0` and 99.6 as `100`,
/// and both ends are read as a state rather than a number, so the two edges say
/// which side of the boundary they are on. Only the open interval is bent: a real
/// nought and a real hundred still print as themselves.
pub fn percentage(value: f64) -> String {
    if value > 0.0 && value < 1.0 {
        return "<1".to_string();
    }
    if value > 99.0 && value < 100.0 {
        return ">99".to_string();
    }
    format!("{value:.0}")
}

/// A percentage printed beside the figure it is being judged against, at a
/// precision that cannot contradict the judgment. [`percentage`] rounds to whole
/// numbers and the comparison is made on what Anthropic sent, so a rounded 79.6
/// reads "80% used … threshold 80% — under it": a figure is never rounded into a
/// claim about a boundary it has not reached.
pub fn percentage_against(value: f64, boundary: u8) -> String {
    let rounded = percentage(value);
    if rounded != boundary.to_string() || value == f64::from(boundary) {
        return rounded;
    }

    // Widened until the figure differs from the boundary. One decimal place is
    // not a guarantee, only a second chance to round onto the same number.
    for places in 1..=3 {
        let said = format!("{value:.places$}");
        if said.parse::<f64>() != Ok(f64::from(boundary)) {
            return said;
        }
    }

    // No figure will say which side of the boundary this is. The words do.
    format!(
        "just {} {boundary}",
        if value < f64::from(boundary) {
            "under"
        } else {
            "over"
        }
    )
}

/// "just now", "3m ago", "2h ago", "4d ago".
pub fn age_phrase(observed_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - observed_at).num_seconds();
    match seconds {
        ..0 => "in the future".to_string(),
        0..=44 => "just now".to_string(),
        _ => format!("{} ago", spans(seconds)),
    }
}

/// Which unit a span is said in, and what one of them is worth. One table for
/// [`age_phrase`] and [`how_long`] both, or the handover moves in one and not
/// the other and an age that grew by a second falls by half an hour. Minutes as
/// far as two hours, where the two forms first agree.
fn unit_of(seconds: i64) -> (&'static str, i64) {
    match seconds {
        ..7200 => ("m", 60),
        7200..86_400 => ("h", 3600),
        _ => ("d", 86_400),
    }
}

/// How long ago something was, in the largest unit that does not round it away:
/// `3m`, `2h`, `4d`. To nearest, which is an age's rounding — a wait's is
/// [`how_long`]'s, and says more.
fn spans(seconds: i64) -> String {
    let (name, unit) = unit_of(seconds);
    format!("{}{name}", (seconds as f64 / unit as f64).round() as i64)
}

/// How long a wait is: the largest unit, and what is left in the one below it,
/// both rounded up. Up, because a wait that reads shorter than it is is the one
/// direction that costs somebody something. The remainder, because a whole unit
/// rounded up is printed beside the clock time it counts to — `14:29 UTC (in 3h)`
/// disagrees with itself, and 1d0h1m read `in 2d`.
fn how_long(seconds: i64) -> String {
    // `i64::div_ceil` is unstable, and both operands here are positive: a wait
    // of nought or less is answered before this is reached.
    let up = |what: i64, per: i64| (what + per - 1) / per;

    let (name, unit) = unit_of(seconds);
    if unit == 60 {
        return format!("{}m", up(seconds, 60));
    }

    let (below, worth) = unit_of(unit - 1);
    let mut whole = seconds / unit;
    let mut rest = up(seconds % unit, worth);
    // The remainder's own round-up can fill the unit above it: 2h59m30s is `3h`
    // rather than `2h60m`.
    if rest * worth >= unit {
        whole += 1;
        rest = 0;
    }
    match rest {
        0 => format!("{whole}{name}"),
        _ => format!("{whole}{name}{rest}{below}"),
    }
}

/// When a Quota Window comes back, said both ways: the clock time to plan around
/// and how long that is from now, so it can be judged without arithmetic —
/// "2026-08-04 15:00 UTC (in 3h)". A time already past reads as "any moment now"
/// rather than as a negative wait, because a cached figure can outlive the window
/// it describes and a reset that has already happened is good news.
pub fn reset_phrase(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format!(
        "{} ({})",
        clock_time(resets_at),
        wait_phrase(resets_at, now)
    )
}

/// The clock time alone, for the sentences that have already said which side of
/// now it falls on. [`reset_phrase`] is the one to reach for wherever the wait is
/// the point: it renders a time already gone as "any moment now", which
/// contradicts a clause going on to say "which has passed".
pub fn clock_time(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// A wait until a reset, or "any moment now" for one already gone. How the
/// number is arrived at is [`how_long`]'s.
fn wait_phrase(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (resets_at - now).num_seconds();
    if seconds <= 0 {
        return "any moment now".to_string();
    }
    format!("in {}", how_long(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 4, hour, minute, 0).unwrap()
    }

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

    #[test]
    fn ages_read_as_prose() {
        assert_eq!(age_phrase(at(12, 0), at(12, 0)), "just now");
        assert_eq!(age_phrase(at(11, 57), at(12, 0)), "3m ago");
        assert_eq!(age_phrase(at(9, 0), at(12, 0)), "3h ago");
    }

    #[test]
    fn a_clock_that_ran_backwards_does_not_claim_freshness() {
        assert_eq!(age_phrase(at(13, 0), at(12, 0)), "in the future");
    }

    #[test]
    fn the_document_does_not_claim_freshness_the_prose_refuses_to() {
        let mut account = observed_at_the_edges();
        account.utilization.as_mut().expect("observed").observed_at = at(13, 0);

        let document = document(&account, at(12, 0));
        let ago = document["windows"][0]["observed_seconds_ago"]
            .as_i64()
            .expect("a number of seconds");

        assert_eq!(
            ago, -3600,
            "an hour in the future is an hour in the future, not this instant"
        );
    }

    #[test]
    fn the_window_column_is_as_wide_as_the_widest_window_that_is_in_it() {
        let mut account = observed_at_the_edges();
        account.utilization.as_mut().expect("observed").windows = ["5-hour", "7-day-sonnet"]
            .into_iter()
            .map(|window| crate::registry::WindowUtilization {
                window: window.to_string(),
                used_percent: 7.0,
                resets_at: None,
            })
            .collect();

        let rows = lines(&account, at(12, 0), window_width_across([&account]));
        let percentage_at = |row: &str| row.find('%').expect("every row prints one");
        assert_eq!(
            percentage_at(&rows[0]),
            percentage_at(&rows[1]),
            "the figures line up down the block: {rows:?}"
        );

        // And a block with nothing long in it keeps the column every other
        // block puts a short name in, rather than closing up around itself.
        account
            .utilization
            .as_mut()
            .expect("observed")
            .windows
            .pop();
        assert!(
            lines(&account, at(12, 0), window_width_across([&account]))[0]
                .starts_with("5-hour   7%"),
            "{:?}",
            lines(&account, at(12, 0), window_width_across([&account]))
        );
    }

    #[test]
    fn a_reset_is_said_as_a_clock_time_and_as_a_wait() {
        assert_eq!(
            reset_phrase(at(15, 0), at(12, 0)),
            "2026-08-04 15:00 UTC (in 3h)"
        );
        assert_eq!(
            reset_phrase(at(12, 20), at(12, 0)),
            "2026-08-04 12:20 UTC (in 20m)"
        );
    }

    #[test]
    fn the_wait_never_grows_as_the_reset_gets_closer() {
        let now = at(12, 0);
        let said = |minutes: i64| wait_phrase(now + chrono::Duration::minutes(minutes), now);

        assert_eq!(said(89), "in 89m");
        assert_eq!(said(90), "in 90m", "rather than jumping to `in 2h`");
        assert_eq!(said(119), "in 119m");
        assert_eq!(
            said(120),
            "in 2h",
            "and it hands over where the two forms agree"
        );
    }

    /// The one direction a wait must never read: shorter than it is. Rounding to
    /// nearest above the two-hour handover put a 2h29m reset at `in 2h`, which
    /// is somebody coming back half an hour early to a window still full.
    #[test]
    fn a_wait_rounds_up_in_every_unit_rather_than_only_in_minutes() {
        let now = at(12, 0);
        let said = |minutes: i64| wait_phrase(now + chrono::Duration::minutes(minutes), now);

        assert_eq!(said(149), "in 2h29m", "2h29m is not two hours away");
        assert_eq!(said(180), "in 3h", "and an exact one is itself");
        assert_eq!(said(60 * 24 + 1), "in 1d1h", "the same at the day handover");
        assert_eq!(said(60 * 48), "in 2d");
    }

    /// A wait is printed beside the clock time it counts to, so a whole unit
    /// rounded up is a phrase that disagrees with itself: `14:29 UTC (in 3h)`
    /// by half an hour, and a reset one minute past a day by almost a day.
    #[test]
    fn a_wait_says_what_is_left_over_rather_than_rounding_a_whole_unit_up() {
        let now = at(12, 0);
        let said = |seconds: i64| wait_phrase(now + chrono::Duration::seconds(seconds), now);

        assert_eq!(said(7201), "in 2h1m", "one second past the handover");
        assert_eq!(
            said(10_770),
            "in 3h",
            "and the remainder's own round-up carries rather than reading `2h60m`"
        );
        assert_eq!(said(86_401), "in 1d1h");
        assert_eq!(said(172_770), "in 2d", "the same carry a day up");
    }

    #[test]
    fn a_percentage_is_never_rounded_into_a_boundary_it_has_not_reached() {
        assert_eq!(percentage_against(79.6, 80), "79.6");
        assert_eq!(percentage_against(80.4, 80), "80.4");
        assert_eq!(
            percentage_against(80.0, 80),
            "80",
            "a figure exactly at the boundary contradicts nothing"
        );
        assert_eq!(
            percentage_against(74.0, 80),
            "74",
            "and one nowhere near it is said the ordinary way"
        );
        assert_eq!(
            percentage_against(70.4, 70),
            "70.4",
            "the ceiling a candidate is set aside by is the same rule"
        );

        // Two decimal places, where one is not enough: both of these read as
        // "80.0" at one place.
        assert_eq!(percentage_against(79.96, 80), "79.96");
        assert_eq!(percentage_against(80.04, 80), "80.04");
        assert_eq!(
            percentage_against(79.999_999, 80),
            "just under 80",
            "and where no figure will say which side of it this is, the words do"
        );
        assert_eq!(percentage_against(80.000_001, 80), "just over 80");
    }

    #[test]
    fn an_age_never_falls_as_the_figure_gets_older() {
        let now = at(12, 0);
        let said = |minutes: i64| age_phrase(now - chrono::Duration::minutes(minutes), now);

        assert_eq!(said(89), "89m ago");
        assert_eq!(said(90), "90m ago", "rather than jumping back to `2h ago`");
        assert_eq!(said(119), "119m ago");
        assert_eq!(
            said(120),
            "2h ago",
            "and it hands over where the two forms agree"
        );
        assert_eq!(said(24 * 60), "1d ago", "as the hour form does to the day");
    }

    #[test]
    fn a_reset_the_cache_outlived_is_good_news_rather_than_a_negative_wait() {
        assert_eq!(
            reset_phrase(at(11, 0), at(12, 0)),
            "2026-08-04 11:00 UTC (any moment now)"
        );
    }

    #[test]
    fn a_percentage_never_rounds_into_a_state_the_account_is_not_in() {
        assert_eq!(percentage(0.4), "<1", "0.4% headroom is not none");
        assert_eq!(percentage(0.999), "<1");
        assert_eq!(percentage(99.6), ">99", "99.6% used is not full");
        assert_eq!(percentage(99.0001), ">99");

        // The two that really are states still say so.
        assert_eq!(percentage(0.0), "0");
        assert_eq!(percentage(100.0), "100");

        // And everything between rounds as it always did.
        assert_eq!(percentage(1.0), "1");
        assert_eq!(percentage(42.4), "42");
        // Half-to-even, which is what `{:.0}` has always done here.
        assert_eq!(percentage(42.5), "42");
        assert_eq!(percentage(43.5), "44");
        assert_eq!(percentage(99.0), "99");
    }

    #[test]
    fn the_rows_that_print_a_percentage_print_it_the_way_every_surface_does() {
        let account = observed_at_the_edges();

        let rows = lines(&account, at(12, 0), window_width_across([&account]));
        assert!(rows[0].starts_with("5-hour >99%"), "{rows:?}");
        assert!(rows[1].starts_with("7-day   <1%"), "{rows:?}");
    }

    fn observed_at_the_edges() -> Account {
        let mut account = Account {
            identity: crate::probe::Identity {
                email: "someone@example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        };
        account.utilization = Some(crate::registry::CachedUtilization {
            observed_at: at(12, 0),
            windows: vec![
                crate::registry::WindowUtilization {
                    window: "5-hour".to_string(),
                    // Not exhausted, and the watcher is still deciding about it.
                    used_percent: 99.6,
                    resets_at: None,
                },
                crate::registry::WindowUtilization {
                    window: "7-day".to_string(),
                    used_percent: 0.4,
                    resets_at: None,
                },
            ],
        });
        account
    }
}
