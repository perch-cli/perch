//! `perch status` — who is active, and where their quota stands.
//!
//! Rendered from cache and never from the network (ADR 0015). This is the
//! command people put in a shell prompt, where it may run several times a
//! minute; fetching here would burn the hourly usage budget needed for
//! switching decisions. Every figure is shown with its age, so a stale number
//! is visibly stale rather than quietly wrong.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::adopt;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::registry::{Account, CachedUtilization};

#[derive(Debug, Default, Clone, Copy)]
pub struct StatusArgs {
    pub json: bool,
}

const LABEL_WIDTH: usize = 14;

pub fn run(host: &dyn Host, args: StatusArgs, out: &mut dyn Write) -> Result<()> {
    let registry = adopt::ensure_adopted(host, out)?;
    let account = registry.active_account().ok_or_else(|| {
        PerchError::NotFound(
            "No active Account. Run `claude` and log in, then run Perch again.".to_string(),
        )
    })?;

    let now = host.now();
    if args.json {
        render_json(out, account, now)
    } else {
        render_human(out, account, now)
    }
}

fn render_human(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    let mut write_line = |label: &str, value: &str| -> Result<()> {
        writeln!(out, "{label:LABEL_WIDTH$}{value}").map_err(io)
    };

    write_line("Account", &account.email)?;
    if let Some(organization) = &account.organization {
        write_line("Organization", organization)?;
    }
    if let Some(plan) = &account.plan {
        write_line("Plan", plan)?;
    }

    match &account.utilization {
        None => write_line("Utilization", "never observed")?,
        Some(cached) if cached.windows.is_empty() => write_line("Utilization", "never observed")?,
        Some(cached) => {
            let age = age_phrase(cached.observed_at, now);
            for (index, window) in cached.windows.iter().enumerate() {
                let label = if index == 0 { "Utilization" } else { "" };
                let resets = match window.resets_at {
                    Some(at) => format!(", resets {}", relative_future(at, now)),
                    None => String::new(),
                };
                write_line(
                    label,
                    &format!(
                        "{:<8} {:>3.0}%  (as of {age}{resets})",
                        window.window, window.used_percent
                    ),
                )?;
            }
        }
    }

    Ok(())
}

fn render_json(out: &mut dyn Write, account: &Account, now: DateTime<Utc>) -> Result<()> {
    let utilization = match &account.utilization {
        Some(cached) if !cached.windows.is_empty() => json!({
            "observed_at": cached.observed_at.to_rfc3339(),
            "never_observed": false,
            "windows": windows_json(cached, now),
        }),
        _ => json!({
            "observed_at": serde_json::Value::Null,
            "never_observed": true,
            "windows": [],
        }),
    };

    let document = json!({
        "active": {
            "email": account.email,
            "account_uuid": account.account_uuid,
            "organization": account.organization,
            "plan": account.plan,
            "profile_dir": account.profile.dir,
        },
        "utilization": utilization,
    });

    writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&document)
            .map_err(|err| PerchError::Other(err.to_string()))?
    )
    .map_err(io)
}

/// Every figure carries its own observation time, so a script can decide for
/// itself whether the number is fresh enough (ADR 0015).
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

fn io(err: std::io::Error) -> PerchError {
    PerchError::Other(err.to_string())
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

/// "in 42m", "in 3h".
fn relative_future(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (at - now).num_seconds();
    if seconds <= 0 {
        return "now".to_string();
    }
    match seconds {
        0..=5399 => format!("in {}m", (seconds as f64 / 60.0).round().max(1.0) as i64),
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
}
