//! How a cached Utilization figure is said — in prose and in JSON.
//!
//! Every surface that shows Utilization renders it from cache, and shows each
//! figure with its age so a stale number is visibly stale rather than quietly
//! wrong (ADR 0015). Only `perch status --refresh` fetches, and it fetches
//! before rendering rather than while: nothing here reaches the network.
//! `status`, `list` and eventually `tui` all have to say the same thing about
//! the same figure, so how a figure reads lives here rather than being spelled
//! out again by each of them.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::commands::write_failed;
use crate::error::Result;
use crate::registry::{Account, CachedUtilization};

/// How wide the label column is on the surfaces that answer about one Account —
/// `status`, and `switch` when it says where you landed. They are read one
/// after the other, so they line up.
pub const LABEL_WIDTH: usize = 14;

/// How wide the Quota Window name column has to be for these windows, and never
/// narrower than the two every Account has.
///
/// Measured rather than fixed, because a Quota Window's name is Anthropic's:
/// `window_name` turns `seven_day_sonnet` into `7-day-sonnet`, and there is a
/// window per model with more of them to come. A fixed eight pushed the
/// percentage right on the per-model rows alone, so they stepped out of line
/// with the `5-hour` and `7-day` above them — in the one place the eye is
/// running down a column. A fixed twelve would line them up by making every
/// block four columns wider whether or not anything needed it, and the
/// Utilization rows already run close to eighty.
///
/// The floor is the width of `5-hour`, which keeps a block that happens to hold
/// only short names in the column every other block puts them in.
pub fn window_width<'a>(windows: impl Iterator<Item = &'a str>) -> usize {
    windows
        .map(str::len)
        .chain(std::iter::once("5-hour".len()))
        .max()
        // The floor is chained on, so there is always one. `unwrap_or_default`
        // here answered nought — a width the doc above rules out — for a case
        // that cannot arise, which is worse than saying so.
        .expect("the floor is always among them")
}

/// The width to lay Quota Window names out in, measured across every Account
/// whose figures are going to be shown together.
///
/// Across the set rather than per Account, because the point of the width is that
/// the figures line up down a column — and a surface that stacks one Account's
/// rows under the next one's has one column, not one per Account. Measured per
/// Account, an Opus-eligible Account carrying `7-day-opus` beside a `pro` Account
/// that does not put the two percentages five columns apart under one
/// `Utilization` heading: the exact failure this width exists to prevent,
/// arriving between Accounts instead of between windows.
pub fn window_width_across<'a>(accounts: impl IntoIterator<Item = &'a Account>) -> usize {
    window_width(
        accounts
            .into_iter()
            .filter_map(Account::observed_utilization)
            .flat_map(|cached| cached.windows.iter().map(|window| window.window.as_str())),
    )
}

/// Writes a label and a value in that column, for the surfaces that render an
/// Account as labelled lines.
pub fn write_labelled(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    writeln!(out, "{label:LABEL_WIDTH$}{value}").map_err(write_failed)
}

/// Writes the cached Utilization under one `Utilization` label, however many
/// Quota Windows there turn out to be.
pub fn write_figures(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    // One Account's block, so the set to lay the names out across is that
    // Account alone.
    let width = window_width_across([account]);
    for (index, figure) in lines(account, now, width).iter().enumerate() {
        let label = if index == 0 { "Utilization" } else { "" };
        write_labelled(out, label, figure)?;
    }
    Ok(())
}

/// One line per Quota Window, each carrying its own age — or the single line
/// that says nothing has ever been observed.
///
/// An Account with no observation is never rendered as zero: "no figure" and
/// "plenty of room" are opposite pieces of advice.
pub fn lines(account: &Account, now: DateTime<Utc>, width: usize) -> Vec<String> {
    rows(account, now, width, |window, width| {
        format!(
            "{:<width$} {:>3}%",
            window.window,
            percentage(window.used_percent)
        )
    })
}

/// The same rows with when each Quota Window comes back, for the surfaces that
/// give an Account a block of its own rather than a row in a table.
///
/// The fill says how much is gone and the reset says how long that lasts, and
/// deciding where to work needs both: a five-hour window at 90% that comes back
/// in twenty minutes and one at 90% that comes back in four hours are the same
/// number and opposite advice. It is a second rendering rather than a longer
/// [`lines`] because `perch list` puts these in a column beside four others, and
/// a clock time there would push the table past the width of a terminal — the
/// two surfaces differ in the room they have, which is exactly what this splits
/// on.
pub fn lines_with_resets(account: &Account, now: DateTime<Utc>, width: usize) -> Vec<String> {
    rows(account, now, width, |window, width| {
        format!(
            // "used", because this row sits under a Headroom figure saying how
            // much is *left*: two percentages of the same window an inch apart,
            // and the reader is not asked to tell them apart by context.
            "{:<width$} {:>3}% used  {}",
            window.window,
            percentage(window.used_percent),
            // Said as its absence rather than left out, because a row with no
            // reset clause reads as a window that does not reset.
            match window.resets_at {
                Some(at) => format!("resets {}", reset_phrase(at, now)),
                None => "no reset time cached".to_string(),
            }
        )
    })
}

/// One row per Quota Window, however each surface says the window itself, with
/// the age of the observation on every one of them (ADR 0015) — or the single
/// line that says nothing has ever been observed.
fn rows(
    account: &Account,
    now: DateTime<Utc>,
    // The caller's, not this Account's: see [`window_width_across`].
    width: usize,
    said: impl Fn(&crate::registry::WindowUtilization, usize) -> String,
) -> Vec<String> {
    match account.observed_utilization() {
        None => vec!["never observed".to_string()],
        Some(cached) => {
            let age = age_phrase(cached.observed_at, now);
            cached
                .windows
                .iter()
                .map(|window| format!("{}  (as of {age})", said(window, width)))
                .collect()
        }
    }
}

/// The cached Utilization as a script reads it: every figure carries its own
/// observation time, so the script can decide for itself whether the number is
/// fresh enough (ADR 0015).
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
                // Not clamped at nought. A figure stamped in the future is a
                // clock that ran backwards, and clamping reports it to a script
                // as maximally fresh — the one reading that gets a stale figure
                // trusted rather than doubted. `age_phrase` refuses to make
                // that claim in prose and says "in the future"; the two
                // surfaces answer for the same cache and must not disagree
                // about whether it can be believed.
                "observed_seconds_ago": (now - cached.observed_at).num_seconds(),
            })
        })
        .collect()
}

/// A percentage as every surface prints one, without the two roundings that
/// say the opposite of what is true.
///
/// `{:.0}` rounds to nearest, so 0.4 renders as `0` and 99.6 as `100`. Both
/// ends of that are read as a state rather than as a number: an Account with
/// 0.4% headroom is not exhausted and is perfectly choosable, but "0% headroom"
/// in the sentence explaining why Perch just switched to it reads as a mistake.
/// The mirror is worse — 99.6% *used* printed as "100% used" reads as an
/// Account that is finished when it is one the watcher is still deciding about.
///
/// So the two edges say which side of the boundary they are on and leave the
/// exact figure, which nobody is acting on at that precision, unsaid. Only the
/// open interval is bent: a real 0 and a real 100 are states, and they still
/// print as themselves.
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
/// precision that cannot contradict the judgement.
///
/// [`percentage`] rounds to whole numbers and the comparison is made on what
/// Anthropic sent, so the two disagreed either side of a threshold: 79.6 is
/// under 80 and printed as `80`, and the watcher's line read "80% used …
/// threshold 80% — under it, so nothing was wanted". That line is the whole of
/// the evidence the policy works, read out of a cron mailbox by somebody
/// deciding whether the watcher is broken, and a flat self-contradiction on it
/// is worse than a decimal place.
///
/// The same shape as the `<1` and `>99` forms above: a figure is never rounded
/// into a claim about a boundary it has not reached.
pub fn percentage_against(value: f64, boundary: u8) -> String {
    let rounded = percentage(value);
    if rounded == boundary.to_string() && value != f64::from(boundary) {
        return format!("{value:.1}");
    }
    rounded
}

/// "just now", "3m ago", "2h ago", "4d ago".
pub fn age_phrase(observed_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - observed_at).num_seconds();
    if seconds < 0 {
        return "in the future".to_string();
    }
    // Minutes as far as two hours, for the reason `wait_phrase` hands over
    // there: the two forms first agree at the boundary, and anywhere else they
    // disagree across it. Handing over at ninety minutes had a figure a second
    // older reading half an hour fresher — 5399 seconds rounded to "90m ago"
    // and 5400 to "2h ago" — and under ADR 0015 the age is the whole of what a
    // reader has to decide whether to trust the number beside it. The hour form
    // hands over to the day form where those two agree already.
    match seconds {
        0..=44 => "just now".to_string(),
        45..=7199 => format!("{}m ago", (seconds as f64 / 60.0).round() as i64),
        7200..=86_399 => format!("{}h ago", (seconds as f64 / 3600.0).round() as i64),
        _ => format!("{}d ago", (seconds as f64 / 86_400.0).round() as i64),
    }
}

/// When a Quota Window comes back, said both ways: the clock time to plan
/// around, and how long that is from now so it can be judged without arithmetic.
///
/// "2026-08-04 15:00 UTC (in 3h)". A time already past reads as "any moment
/// now" rather than as a negative wait — a cached figure can outlive the window
/// it describes, and a reset that has already happened is good news.
pub fn reset_phrase(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format!(
        "{} ({})",
        resets_at.format("%Y-%m-%d %H:%M UTC"),
        wait_phrase(resets_at, now)
    )
}

fn wait_phrase(resets_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (resets_at - now).num_seconds();
    if seconds <= 0 {
        return "any moment now".to_string();
    }
    // Minutes as far as two hours, which is where the hour form and the minute
    // form first agree. Handing over at ninety minutes had the two disagreeing
    // across the boundary — 5399 seconds read "in 90m" and 5400 read "in 2h",
    // so a wait grew by half an hour as it shortened by a second. Whether to
    // wait for perishable quota is the decision this line is read for, and half
    // an hour is the difference between making a cup of tea and doing something
    // else.
    match seconds {
        1..=7199 => format!("in {}m", (seconds as f64 / 60.0).ceil() as i64),
        7200..=86_399 => format!("in {}h", (seconds as f64 / 3600.0).round() as i64),
        _ => format!("in {}d", (seconds as f64 / 86_400.0).round() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 4, hour, minute, 0).unwrap()
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

    /// And the script reading the same cache is told the same thing. Clamping
    /// the age at nought reported a figure stamped in the future as maximally
    /// fresh — the one reading that gets a stale figure trusted rather than
    /// doubted, and the opposite of what the prose beside it says.
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

    /// A Quota Window's name is Anthropic's, and there is one per model:
    /// `seven_day_sonnet` becomes `7-day-sonnet`, which is twelve characters
    /// against `5-hour`'s six. A fixed column narrower than that does not
    /// truncate — it shoves the percentage right on the long rows alone, so
    /// they step out of line with the two above them.
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

    /// A wait that never grows as it shortens. The minute form and the hour
    /// form disagreed across the boundary they were handed over at: ninety
    /// minutes less a second read "in 90m" and ninety minutes read "in 2h", so
    /// a window a second closer to coming back was advertised as half an hour
    /// further away. Whether to wait for perishable quota is what this line is
    /// read for.
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

    /// A figure and the verdict beside it must not disagree. The comparison is
    /// made on what Anthropic sent and the rendering rounded, so 79.6 against a
    /// threshold of 80 printed "80% used … threshold 80% — under it".
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
    }

    /// The same rule for the other direction: an age that never falls as the
    /// figure gets older. `wait_phrase` was moved off the ninety-minute
    /// handover and this was left on it, so a figure read at 89m59s said "90m
    /// ago" and the same figure a second later said "2h ago" — half an hour
    /// fresher, on the line ADR 0015 has a reader judging the number by.
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

    /// `{:.0}` rounds to nearest, and both ends of that round into a word
    /// rather than a number: "0% headroom" reads as exhausted and "100% used"
    /// reads as finished, about Accounts that are neither.
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

    /// And the surfaces that print one go through it.
    ///
    /// These two rows are the only place in Perch that prints the words
    /// "% used" about a raw window figure, which is the rounding the doc above
    /// calls the worse of the two: they formatted `used_percent` themselves, so
    /// an Account at 99.6% used rendered as `100% used` directly beneath a
    /// Headroom figure saying it had room, and `perch switch` would land on it
    /// happily. A helper nothing calls is a rule nothing follows.
    #[test]
    fn the_rows_that_print_a_percentage_print_it_the_way_every_surface_does() {
        let account = observed_at_the_edges();

        let rows = lines(&account, at(12, 0), window_width_across([&account]));
        assert!(rows[0].starts_with("5-hour >99%"), "{rows:?}");
        assert!(rows[1].starts_with("7-day   <1%"), "{rows:?}");

        let rows = lines_with_resets(&account, at(12, 0), window_width_across([&account]));
        assert!(rows[0].starts_with("5-hour >99% used"), "{rows:?}");
        assert!(rows[1].starts_with("7-day   <1% used"), "{rows:?}");
    }

    /// An ordinary figure keeps the column it always had: the two edges are the
    /// only rows whose width this could have moved.
    #[test]
    fn an_ordinary_figure_lands_in_the_same_column_it_always_did() {
        let mut account = observed_at_the_edges();
        account.utilization.as_mut().expect("observed").windows =
            vec![crate::registry::WindowUtilization {
                window: "5-hour".to_string(),
                used_percent: 7.0,
                resets_at: None,
            }];

        assert!(
            lines_with_resets(&account, at(12, 0), window_width_across([&account]))[0]
                .starts_with("5-hour   7% used"),
            "{:?}",
            lines_with_resets(&account, at(12, 0), window_width_across([&account]))
        );
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
            enabled: true,
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
