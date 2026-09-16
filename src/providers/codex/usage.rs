//! Codex usage implementation.

use super::auth::identity;
use super::{layout, process::environment, refused};
use crate::domain::WindowUtilization;
use crate::providers::provider::ProfileRef as Account;
use crate::providers::provider::{ConfiguredProvider, DefaultRelation, Observation};
use crate::{Host, PerchError, Result};
use serde_json::{Value, json};

const ARGS: &[&str] = &[
    "app-server",
    "-c",
    "cli_auth_credentials_store=\"file\"",
    "-c",
    "forced_login_method=\"chatgpt\"",
];

pub(super) fn read_limits(
    host: &dyn Host,
    held: &mut crate::lock::Held<'_>,
    request: Observation<'_>,
    still_ours: crate::lock::StillOurs<'_>,
) -> std::result::Result<Vec<WindowUtilization>, crate::observe::Outcome> {
    let mut stopped = None;
    let mut checkpoint = || {
        if let Err(lost) = still_ours() {
            stopped = Some(lost);
            return Err(PerchError::Busy("Observation was interrupted".into()));
        }
        held.renew();
        if !held.still_held() {
            return Err(PerchError::Busy(
                "The configuration lock was lost during observation".into(),
            ));
        }
        Ok(())
    };
    let mut spent = false;
    // The active Account is read where its login is live: the Default home,
    // whose copy is the one Codex Renews. Its Profile copy is refreshed by the
    // Capture of the next Switch.
    let home = match request.context.default {
        DefaultRelation::Active => layout::default_home(host),
        _ => request.profile.profile_dir(host),
    };
    let result = home.and_then(|home| {
        read(
            host,
            request.configured,
            request.profile,
            &home,
            &mut checkpoint,
            &mut spent,
        )
    });
    match stopped {
        Some(lost) => Err(crate::observe::Outcome::Stopped(lost)),
        None => result.map_err(|error| crate::observe::Outcome::Failed {
            why: error.to_string(),
            spent,
        }),
    }
}

fn read(
    host: &dyn Host,
    configured: &ConfiguredProvider,
    account: &Account,
    home: &std::path::Path,
    checkpoint: &mut dyn FnMut() -> Result<()>,
    spent: &mut bool,
) -> Result<Vec<WindowUtilization>> {
    checkpoint()?;
    let home = home.to_path_buf();
    if matches!(
        crate::live::ask(
            host,
            &[crate::live::Place::new(
                crate::providers::provider::Id::Codex,
                "Codex Profile",
                &home
            )]
        ),
        crate::live::Answer::NotIdle(_)
    ) {
        return Err(PerchError::Busy(
            "Codex is running against this Profile; cached Utilization is retained until it exits"
                .into(),
        ));
    }
    credential_at(host, account, &home)?;
    let installation = configured.installation(host)?;
    let executable = installation.executable();
    let _claim = crate::providers::sessions::claim(host, &home)?;
    let env = environment(host, &home);
    let env: Vec<_> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let requests = [
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"perch","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":false}}}).to_string(),
        json!({"method":"initialized","params":{}}).to_string(),
        json!({"id":2,"method":"account/read","params":{"refreshToken":false}}).to_string(),
        json!({"id":3,"method":"account/rateLimits/read","params":{}}).to_string(),
    ];
    checkpoint()?;
    *spent = true;
    let mut interrupted = None;
    let responses = host.rpc(
        &executable.to_string_lossy(),
        ARGS,
        &env,
        &requests,
        crate::host::RpcControl {
            timeout: std::time::Duration::from_secs(30),
            checkpoint: &mut || {
                checkpoint().map_err(|error| {
                    interrupted = Some(error);
                    crate::host::HostError::Other("Observation interrupted".into())
                })
            },
        },
    );
    let responses = responses.map_err(|_| {
        interrupted.unwrap_or_else(|| {
            PerchError::Other(
                "Codex observation failed or timed out; cached Utilization is retained".into(),
            )
        })
    })?;
    checkpoint()?;
    credential_at(host, account, &home)?;
    let mut initialized = false;
    let mut identified = false;
    let mut limits = None;
    for response in responses {
        let value: Value =
            serde_json::from_str(&response).map_err(|_| refused("returned an invalid response"))?;
        if value.get("error").is_some() {
            return Err(PerchError::Other(
                "Codex could not read this Account; cached Utilization is retained".into(),
            ));
        }
        match value.get("id").and_then(Value::as_u64) {
            Some(1) => initialized = value.get("result").is_some_and(Value::is_object),
            Some(2) => {
                identified = value
                    .pointer("/result/account/type")
                    .and_then(Value::as_str)
                    == Some("chatgpt")
            }
            _ => {}
        }
        if value.get("id").and_then(Value::as_u64) == Some(3) {
            limits = value.get("result").cloned();
        }
    }
    if !initialized || !identified {
        return Err(refused("did not identify a subscription-backed Account"));
    }
    parse_limits(&limits.ok_or_else(|| refused("did not return Utilization"))?)
}

/// The Credential at `home` is this Account's, or the reading is not made:
/// a figure is recorded against the Account whose login produced it.
fn credential_at(host: &dyn Host, account: &Account, home: &std::path::Path) -> Result<()> {
    let path = home.join(super::AUTH_FILE);
    let document = match host.read_file(&path) {
        Ok(document) => zeroize::Zeroizing::new(document),
        Err(crate::host::HostError::NotFound { .. }) => {
            return Err(PerchError::NotFound(format!(
                "No Codex Credential is held for {}. `perch relogin {}` logs it in again.",
                account.key(),
                account.key()
            )));
        }
        Err(error) => return Err(PerchError::file_read(path, error)),
    };
    let (found, _, _) = identity(&document)?;
    if account.provider_identity.as_ref() != Some(&found) {
        return Err(refused(
            "Credential belongs to another Account or Workspace",
        ));
    }
    Ok(())
}

pub fn parse_limits(result: &Value) -> Result<Vec<WindowUtilization>> {
    let buckets: Vec<&Value> =
        if let Some(map) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
            if map.is_empty() {
                return Err(refused("returned no quota buckets"));
            }
            map.values().collect()
        } else {
            vec![
                result
                    .get("rateLimits")
                    .ok_or_else(|| refused("returned no quota buckets"))?,
            ]
        };
    let mut windows = Vec::new();
    for bucket in buckets {
        if bucket
            .get("individualLimit")
            .is_some_and(|value| !value.is_null())
            || bucket
                .get("rateLimitReachedType")
                .is_some_and(|value| !value.is_null())
            || bucket
                .get("spendControlReached")
                .is_some_and(|value| !value.is_null() && value != false)
            || bucket
                .pointer("/credits/hasCredits")
                .is_some_and(|value| value != false)
            || bucket
                .pointer("/credits/unlimited")
                .is_some_and(|value| value != false)
        {
            return Err(refused(
                "credit or spend-control state cannot be represented as percentage Utilization yet",
            ));
        }
        let id = bucket
            .get("limitId")
            .and_then(Value::as_str)
            .ok_or_else(|| refused("quota bucket has no identity"))?;
        if id.is_empty() {
            return Err(refused("quota bucket has no identity"));
        }
        for period in ["primary", "secondary"] {
            let Some(window) = bucket.get(period).filter(|v| !v.is_null()) else {
                continue;
            };
            let percent = window
                .get("usedPercent")
                .and_then(Value::as_f64)
                .filter(|v| (0.0..=100.0).contains(v))
                .ok_or_else(|| refused("quota percentage is unreadable"))?;
            let duration = window
                .get("windowDurationMins")
                .and_then(Value::as_u64)
                .filter(|v| *v > 0)
                .ok_or_else(|| refused("quota duration is unreadable"))?;
            let reset = window
                .get("resetsAt")
                .and_then(Value::as_i64)
                .and_then(|value| chrono::DateTime::from_timestamp(value, 0))
                .ok_or_else(|| refused("quota reset is unreadable"))?;
            windows.push(WindowUtilization {
                group: None,
                window: format!("{id}/{period}/{duration}m"),
                used_percent: percent,
                resets_at: Some(reset),
            });
        }
    }
    if windows.is_empty() {
        return Err(refused(
            "has no percentage quota; credit-based Utilization is not supported yet",
        ));
    }
    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_windows_keep_their_bucket_identity_and_invalid_figures_are_unknown() {
        let window = json!({"usedPercent":32,"windowDurationMins":300,"resetsAt":1800000000});
        let limits = json!({"rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":window},"review":{"limitId":"review","primary":window}}});
        let parsed = parse_limits(&limits).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_ne!(parsed[0].window, parsed[1].window);
        for value in [
            json!({}),
            json!({"rateLimitsByLimitId":{}}),
            json!({"rateLimits":{"limitId":"codex"}}),
            json!({"rateLimits":{"limitId":"codex","primary":{"usedPercent":120,"windowDurationMins":300,"resetsAt":1800000000}}}),
        ] {
            assert!(parse_limits(&value).is_err());
        }
    }
}
