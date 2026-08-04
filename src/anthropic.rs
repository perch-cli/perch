//! The three Anthropic endpoints Perch talks to, and the only place in Perch
//! that knows an address.
//!
//! None of this is a published contract, so it is held the way
//! [`crate::probe`] holds Claude Code's internals (ADR 0007): one module carries
//! every assumption, and a reply Perch cannot make sense of is reported as such
//! rather than guessed at. What is assumed here is
//!
//! - the usage endpoint answers with an object whose values are Quota Windows,
//!   each carrying how full it is and when it resets;
//! - the profile endpoint says which Account an access token belongs to;
//! - the token endpoint renews an access token in exchange for a refresh token,
//!   and Rotates the refresh token whenever it pleases.
//!
//! The reply is read as loosely as it can be: the Quota Windows are whatever
//! the reply holds rather than a fixed list, because an Account is limited by
//! whichever window fills first, and a window dropped for having a name Perch
//! had not been taught is a window nothing would ever rank on.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::host::{Host, HttpRequest, HttpResponse};
use crate::registry::WindowUtilization;

/// Where an Account's Quota Windows are read from. Roughly 28-30 requests per
/// rolling hour per Account, and it does not refill early (ADR 0015).
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Where Perch asks whose access token it is holding.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

/// Where a refresh token buys a fresh access token — and, when Anthropic
/// decides to Rotate, a fresh refresh token with it.
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

/// The beta the OAuth endpoints are behind, sent beside the Bearer token on
/// every read.
pub const BETA: &str = "oauth-2025-04-20";

/// The OAuth client the Credentials Perch holds were issued to. A refresh token
/// can only be renewed by the client it was issued to, so this is Claude Code's
/// rather than one of Perch's own.
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Every Quota Window an Account has at one moment.
pub type QuotaWindows = Vec<WindowUtilization>;

/// Why an endpoint did not answer the question.
///
/// Four outcomes rather than one message, because a caller does something
/// different with each: a throttle falls back to cache, a rejection means the
/// Account has to be logged into again, and drift in a reply is worth telling
/// apart from a network that was not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The hourly budget for this Account is spent, and does not refill early
    /// (ADR 0015).
    Throttled,
    /// The Credential was not accepted.
    Rejected,
    /// The endpoint answered something Perch does not understand.
    Unrecognised(String),
    /// The request never got there.
    Unreachable(String),
}

const THROTTLED: &str = "Anthropic is rate-limiting reads of this Account's \
                         Utilization — about 28-30 an hour, and the window \
                         does not refill early";
const REJECTED: &str = "Anthropic did not accept the Credential";
const UNRECOGNISED: &str = "Anthropic answered something Perch does not understand";
const UNREACHABLE: &str = "Anthropic could not be reached";

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = match self {
            Refused::Throttled => THROTTLED.to_string(),
            Refused::Rejected => REJECTED.to_string(),
            Refused::Unrecognised(detail) => format!("{UNRECOGNISED}: {detail}"),
            Refused::Unreachable(detail) => format!("{UNREACHABLE}: {detail}"),
        };
        formatter.write_str(&said)
    }
}

/// What a renewal hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    pub access_token: String,
    /// The Rotated refresh token, when Anthropic Rotated one. Absent means the
    /// refresh token already stored is still the live one.
    pub refresh_token: Option<String>,
    /// When the new access token expires, in milliseconds — the unit a
    /// Credential records it in.
    pub expires_at: Option<i64>,
}

/// Every Quota Window this access token's Account currently has.
pub fn utilization(host: &dyn Host, access_token: &str) -> Result<QuotaWindows, Refused> {
    let document = read(host, USAGE_URL, access_token)?;
    let windows = windows_in(&document);
    if windows.is_empty() {
        return Err(Refused::Unrecognised(
            "the usage endpoint named no Quota Window".to_string(),
        ));
    }
    Ok(windows)
}

/// Which Account an access token belongs to, when the reply says.
///
/// `None` is "the reply did not say" rather than "nobody": a caller weighs
/// evidence with this, and no evidence is not evidence against.
pub fn whose(host: &dyn Host, access_token: &str) -> Result<Option<String>, Refused> {
    let document = read(host, PROFILE_URL, access_token)?;
    Ok(email_in(&document))
}

/// Renews an access token, and reports the Rotation when there was one.
pub fn renew(host: &dyn Host, refresh_token: &str) -> Result<Fresh, Refused> {
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
    })
    .to_string();
    let headers = [("Content-Type", "application/json")];

    let response = send(host, &HttpRequest::post(TOKEN_URL, &headers, &body))?;
    // A refresh token Anthropic has retired comes back as a bad request rather
    // than as an unauthorized one, and it means the same thing here: this
    // Account cannot be renewed and has to be logged into again.
    let document = understand(response, &[400])?;
    let now = host.now();

    Ok(Fresh {
        access_token: document
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("access_token"))?
            .to_string(),
        refresh_token: document
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        expires_at: document
            .get("expires_in")
            .and_then(Value::as_i64)
            .map(|seconds| now.timestamp_millis() + seconds * 1_000),
    })
}

fn missing(field: &str) -> Refused {
    Refused::Unrecognised(format!("the token endpoint returned no {field}"))
}

/// A read as this Account: a Bearer token, and the beta the OAuth endpoints are
/// behind.
fn read(host: &dyn Host, url: &str, access_token: &str) -> Result<Value, Refused> {
    let authorization = format!("Bearer {access_token}");
    let headers = [
        ("Authorization", authorization.as_str()),
        ("anthropic-beta", BETA),
        ("Accept", "application/json"),
    ];

    let response = send(host, &HttpRequest::get(url, &headers))?;
    understand(response, &[])
}

fn send(host: &dyn Host, request: &HttpRequest<'_>) -> Result<HttpResponse, Refused> {
    host.http(request)
        .map_err(|err| Refused::Unreachable(err.to_string()))
}

/// The document in a reply, or the reason there is not one.
///
/// `also_rejected` names the statuses this endpoint says "not you" with, over
/// and above the two every endpoint uses. No reply body reaches a message: what
/// an endpoint says about a Credential it would not take is not something to
/// print or to log.
fn understand(response: HttpResponse, also_rejected: &[u16]) -> Result<Value, Refused> {
    if also_rejected.contains(&response.status) {
        return Err(Refused::Rejected);
    }
    match response.status {
        200..=299 => serde_json::from_str(&response.body)
            .map_err(|err| Refused::Unrecognised(format!("the reply is not JSON: {err}"))),
        401 | 403 => Err(Refused::Rejected),
        429 => Err(Refused::Throttled),
        status => Err(Refused::Unrecognised(format!("HTTP {status}"))),
    }
}

/// The Quota Windows a usage reply describes.
///
/// Every value in the reply that carries a utilization is one, whatever it is
/// called: a five-hour window, a seven-day window and one per model are what
/// there are today, and a per-model window added tomorrow is recorded without
/// Perch having to learn its name first.
fn windows_in(document: &Value) -> QuotaWindows {
    let Some(fields) = document.as_object() else {
        return QuotaWindows::new();
    };

    let mut windows: QuotaWindows = fields
        .iter()
        .filter_map(|(name, value)| window_from(name, value))
        .collect();
    windows.sort_by(|one, other| {
        rank(&one.window)
            .cmp(&rank(&other.window))
            .then_with(|| one.window.cmp(&other.window))
    });
    windows
}

fn window_from(name: &str, value: &Value) -> Option<WindowUtilization> {
    let used_percent = value.get("utilization").and_then(Value::as_f64)?;
    Some(WindowUtilization {
        window: window_name(name),
        used_percent,
        resets_at: value
            .get("resets_at")
            .and_then(Value::as_str)
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc)),
    })
}

/// A window's name as the rest of Perch says it: `five_hour` is the `5-hour`
/// window, `seven_day_opus` the `7-day-opus` one.
fn window_name(key: &str) -> String {
    key.split('_')
        .map(|part| match part {
            "five" => "5",
            "seven" => "7",
            other => other,
        })
        .collect::<Vec<&str>>()
        .join("-")
}

/// Where a window sits when the figures are shown together: the two every
/// Account has, in the order they run out, and then the per-model ones by name.
fn rank(window: &str) -> u8 {
    match window {
        "5-hour" => 0,
        "7-day" => 1,
        _ => 2,
    }
}

/// The email address a profile reply names, if it names one.
fn email_in(document: &Value) -> Option<String> {
    document
        .get("account")
        .and_then(|account| account.get("email_address"))
        .or_else(|| document.get("email_address"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE: &str = r#"{
      "five_hour": {"utilization": 42, "resets_at": "2026-08-04T14:30:00Z"},
      "seven_day": {"utilization": 18.5, "resets_at": "2026-08-09T00:00:00Z"},
      "seven_day_opus": {"utilization": 3, "resets_at": "2026-08-09T00:00:00Z"},
      "seven_day_sonnet": {"utilization": 61},
      "session": "not a window"
    }"#;

    fn windows() -> QuotaWindows {
        windows_in(&serde_json::from_str(USAGE).expect("valid JSON"))
    }

    #[test]
    fn every_quota_window_in_the_reply_is_recorded() {
        let named: Vec<&str> = windows().iter().map(|w| w.window.as_str()).collect();
        assert_eq!(
            named,
            vec!["5-hour", "7-day", "7-day-opus", "7-day-sonnet"],
            "the two windows every Account has come first, then the per-model ones"
        );
    }

    #[test]
    fn a_window_carries_how_full_it_is_and_when_it_resets() {
        let windows = windows();
        assert_eq!(windows[0].used_percent, 42.0);
        assert_eq!(
            windows[0].resets_at.map(|at| at.to_rfc3339()),
            Some("2026-08-04T14:30:00+00:00".to_string())
        );
        assert_eq!(windows[1].used_percent, 18.5);
        assert_eq!(
            windows[3].resets_at, None,
            "a window that said nothing about resetting does not claim a time"
        );
    }

    #[test]
    fn anything_in_the_reply_that_is_not_a_window_is_not_read_as_one() {
        assert!(!windows().iter().any(|w| w.window == "session"));
        assert!(windows_in(&json!("not an object")).is_empty());
    }

    #[test]
    fn the_statuses_that_matter_are_told_apart() {
        let reply = |status: u16| HttpResponse {
            status,
            body: "{}".to_string(),
        };

        assert_eq!(understand(reply(429), &[]), Err(Refused::Throttled));
        assert_eq!(understand(reply(401), &[]), Err(Refused::Rejected));
        assert_eq!(
            understand(reply(400), &[400]),
            Err(Refused::Rejected),
            "a retired refresh token is a rejection however it is spelled"
        );
        assert!(matches!(
            understand(reply(400), &[]),
            Err(Refused::Unrecognised(_))
        ));
        assert_eq!(understand(reply(200), &[]), Ok(json!({})));
    }

    #[test]
    fn a_profile_reply_names_the_account_or_says_nothing() {
        let named = json!({"account": {"email_address": "someone@example.com"}});
        assert_eq!(
            email_in(&named),
            Some("someone@example.com".to_string()),
            "which Account a token belongs to is what this endpoint is for"
        );
        assert_eq!(email_in(&json!({"organization": {"name": "Acme"}})), None);
    }
}
