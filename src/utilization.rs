//! How a cached Utilization figure is said — in prose and in JSON.
//!
//! Every surface that shows Utilization renders it from cache and never from
//! the network, and shows each figure with its age so a stale number is visibly
//! stale rather than quietly wrong (ADR 0015). `status`, `list` and eventually
//! `tui` all have to say the same thing about the same figure, so how a figure
//! reads lives here rather than being spelled out again by each of them.

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::registry::{Account, CachedUtilization};

/// One line per Quota Window, each carrying its own age — or the single line
/// that says nothing has ever been observed.
///
/// An Account with no observation is never rendered as zero: "no figure" and
/// "plenty of room" are opposite pieces of advice.
pub fn lines(account: &Account, now: DateTime<Utc>) -> Vec<String> {
    match account.observed_utilization() {
        None => vec!["never observed".to_string()],
        Some(cached) => {
            let age = age_phrase(cached.observed_at, now);
            cached
                .windows
                .iter()
                .map(|window| {
                    format!(
                        "{:<8} {:>3.0}%  (as of {age})",
                        window.window, window.used_percent
                    )
                })
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
}
