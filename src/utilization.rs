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

/// Writes a label and a value in that column, for the surfaces that render an
/// Account as labelled lines.
pub fn write_labelled(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    writeln!(out, "{label:LABEL_WIDTH$}{value}").map_err(write_failed)
}

/// Writes the cached Utilization under one `Utilization` label, however many
/// Quota Windows there turn out to be.
pub fn write_figures(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    for (index, figure) in lines(account, now).iter().enumerate() {
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
pub fn lines(account: &Account, now: DateTime<Utc>) -> Vec<String> {
    rows(account, now, |window| {
        format!("{:<8} {:>3.0}%", window.window, window.used_percent)
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
pub fn lines_with_resets(account: &Account, now: DateTime<Utc>) -> Vec<String> {
    rows(account, now, |window| {
        format!(
            // "used", because this row sits under a Headroom figure saying how
            // much is *left*: two percentages of the same window an inch apart,
            // and the reader is not asked to tell them apart by context.
            "{:<8} {:>3.0}% used  {}",
            window.window,
            window.used_percent,
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
    said: impl Fn(&crate::registry::WindowUtilization) -> String,
) -> Vec<String> {
    match account.observed_utilization() {
        None => vec!["never observed".to_string()],
        Some(cached) => {
            let age = age_phrase(cached.observed_at, now);
            cached
                .windows
                .iter()
                .map(|window| format!("{}  (as of {age})", said(window)))
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
                "observed_seconds_ago": (now - cached.observed_at).num_seconds().max(0),
            })
        })
        .collect()
}

/// "just now", "3m ago", "2h ago", "4d ago".
pub fn age_phrase(observed_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - observed_at).num_seconds();
    if seconds < 0 {
        return "in the future".to_string();
    }
    match seconds {
        0..=44 => "just now".to_string(),
        45..=5399 => format!("{}m ago", (seconds as f64 / 60.0).round() as i64),
        5400..=86_399 => format!("{}h ago", (seconds as f64 / 3600.0).round() as i64),
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
    match seconds {
        1..=5399 => format!("in {}m", (seconds as f64 / 60.0).ceil() as i64),
        5400..=86_399 => format!("in {}h", (seconds as f64 / 3600.0).round() as i64),
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
    fn a_reset_the_cache_outlived_is_good_news_rather_than_a_negative_wait() {
        assert_eq!(
            reset_phrase(at(11, 0), at(12, 0)),
            "2026-08-04 11:00 UTC (any moment now)"
        );
    }
}
