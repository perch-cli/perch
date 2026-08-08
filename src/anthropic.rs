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
#[derive(Clone, PartialEq, Eq)]
pub struct Fresh {
    pub access_token: String,
    /// The Rotated refresh token, when Anthropic Rotated one. Absent means the
    /// refresh token already stored is still the live one.
    pub refresh_token: Option<String>,
    /// When the new access token expires, in milliseconds — the unit a
    /// Credential records it in.
    pub expires_at: Option<i64>,
}

impl std::fmt::Debug for Fresh {
    /// A freshly Rotated refresh token is the most valuable secret in Perch —
    /// it is the only copy there is, the old one having just been retired — so
    /// it is not one to leave a derived `Debug` able to print.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Fresh")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .finish()
    }
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
    // than as an unauthorized one, and where the body agrees it means the same
    // thing here: this Account cannot be renewed and has to be logged into
    // again. Where the body does not agree, the status is only a 400 — see
    // [`REVOKED`].
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
        // Checked, because this is the one arithmetic in Perch on a number
        // Anthropic chose, at the one moment nothing can be retried: `renew`
        // has already caused the old refresh token to be retired, and the
        // Credential carrying its replacement has not been stored yet. A debug
        // build would panic there and lose the Account; a release build would
        // wrap to a negative `expires_at`, which reads as expired and renews
        // again on every single command. A lifetime that does not fit is a
        // lifetime the reply did not give: `None` already means that, and the
        // Credential is simply renewed when something else says it must be.
        //
        // A negative lifetime is the same non-answer reached by an easier
        // route: `expires_in: -1` yields an `expires_at` a second in the past,
        // which reads as already expired and renews on every command forever —
        // the outcome the overflow guard above exists to prevent. A token that
        // arrives already dead is not a lifetime this reply gave either.
        expires_at: document
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| seconds.checked_mul(1_000))
            .and_then(|millis| now.timestamp_millis().checked_add(millis)),
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

/// OAuth's own word for "this refresh token is no longer one", and the only
/// thing a 400 from the token endpoint may be read as a Quarantine.
///
/// A 400 is the status a request gets for being wrong in *any* way, and the
/// caller acts on this one for ever: [`observe`](crate::observe) turns
/// `Refused::Rejected` from a renewal into `Quarantine::RenewalRejected`, which
/// only a browser login clears. Reading a malformed request, a proxy's own
/// error page, or a `client_id` Anthropic has changed its mind about as "log in
/// again" would walk a whole Group and Quarantine every Account in it — which a
/// single `perch status --group --refresh` does in one pass.
const REVOKED: &str = "invalid_grant";

/// The document in a reply, or the reason there is not one.
///
/// `also_rejected` names the statuses this endpoint says "not you" with, over
/// and above the two every endpoint uses, and it is believed only when the body
/// agrees: see [`REVOKED`]. No reply body reaches a message either way — what an
/// endpoint says about a Credential it would not take is not something to print
/// or to log.
fn understand(response: HttpResponse, also_rejected: &[u16]) -> Result<Value, Refused> {
    if also_rejected.contains(&response.status) {
        return match says_revoked(&response.body) {
            true => Err(Refused::Rejected),
            // Not terminal, so it is retried rather than recorded: the next
            // Refresh asks again, and a Credential that was never the problem
            // goes on working.
            false => Err(Refused::Unrecognised(format!("HTTP {}", response.status))),
        };
    }
    match response.status {
        200..=299 => serde_json::from_str(&response.body)
            .map_err(|err| Refused::Unrecognised(format!("the reply is not JSON: {err}"))),
        401 | 403 => Err(Refused::Rejected),
        429 => Err(Refused::Throttled),
        status => Err(Refused::Unrecognised(format!("HTTP {status}"))),
    }
}

/// Whether a refusal body is the endpoint saying the refresh token is retired.
///
/// The field OAuth 2.0 gives this is `error`, and Anthropic writes it there.
/// Anything else — a body that is not JSON, a JSON body with no `error`, or an
/// `error` naming something the request got wrong — is not this.
fn says_revoked(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|document| document.get("error"))
        .and_then(Value::as_str)
        == Some(REVOKED)
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
        // How full a window is, and a window cannot be less than empty or more
        // than full. Anything outside that is a reply Perch does not understand
        // rather than a figure, and clamping it here is what stops it becoming
        // "105% headroom" in a sentence somebody is asked to act on.
        used_percent: used_percent.clamp(0.0, 100.0),
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
    /// A window cannot be less than empty or more than full. A reply saying
    /// otherwise is one Perch does not understand, and left alone it becomes
    /// "105% headroom" in a sentence somebody is asked to act on.
    #[test]
    fn a_utilization_outside_nought_to_a_hundred_is_brought_back_into_it() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": -5}, "seven_day": {"utilization": 130}}"#,
        )
        .unwrap();

        let windows = super::windows_in(&document);

        assert_eq!(windows[0].used_percent, 0.0);
        assert_eq!(windows[1].used_percent, 100.0);
    }

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
        let windows = windows();
        let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
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
        let said = |body: &str| HttpResponse {
            status: 400,
            body: body.to_string(),
        };

        assert_eq!(understand(reply(429), &[]), Err(Refused::Throttled));
        assert_eq!(understand(reply(401), &[]), Err(Refused::Rejected));
        assert_eq!(
            understand(said(r#"{"error":"invalid_grant"}"#), &[400]),
            Err(Refused::Rejected),
            "a retired refresh token is a rejection, and this is how it says so"
        );
        assert!(matches!(
            understand(reply(400), &[]),
            Err(Refused::Unrecognised(_))
        ));
        assert_eq!(understand(reply(200), &[]), Ok(json!({})));
    }

    /// A 400 is what a request gets for being wrong in any way, and the caller
    /// acts on a rejection for ever: it Quarantines the Account, which only a
    /// browser login clears. So the status alone is not enough — a proxy's
    /// error page, or a request Anthropic changed its mind about the shape of,
    /// would otherwise Quarantine every Account in a Group in one pass of
    /// `perch status --group --refresh`.
    #[test]
    fn a_bad_request_that_does_not_say_the_token_is_retired_is_not_terminal() {
        let refused = |body: &str| {
            understand(
                HttpResponse {
                    status: 400,
                    body: body.to_string(),
                },
                &[400],
            )
        };

        for body in [
            "{}",
            r#"{"error":"invalid_request"}"#,
            r#"{"error":{"type":"invalid_request_error"}}"#,
            "<html>Bad Request</html>",
            "",
        ] {
            assert!(
                matches!(refused(body), Err(Refused::Unrecognised(_))),
                "a Quarantine is for ever, and this is not the endpoint asking \
                 for one: {body}"
            );
        }
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

    /// The one arithmetic in Perch on a number Anthropic chose, at the one
    /// moment nothing can be retried: by the time this runs the old refresh
    /// token has already been retired, and the Credential carrying its
    /// replacement is not stored yet. A debug build panicked on the overflow
    /// and lost the Account there; a release build wrapped to a negative
    /// `expires_at`, which reads as already expired and renews again on every
    /// command for ever.
    #[test]
    fn a_lifetime_that_does_not_fit_is_a_lifetime_the_reply_did_not_give() {
        let host = crate::host::FakeHost::new();
        let renewed = |expires_in: &str| {
            let reply =
                format!(r#"{{"access_token":"sk-ant-oat01-new","expires_in":{expires_in}}}"#);
            host.reply(TOKEN_URL, None, 200, &reply);
            renew(&host, "sk-ant-ort01-old").expect("the endpoint answered")
        };

        assert_eq!(
            renewed("3600").expires_at,
            Some(host.now().timestamp_millis() + 3_600_000),
            "an ordinary lifetime is still read"
        );
        assert_eq!(
            renewed(&i64::MAX.to_string()).expires_at,
            None,
            "and one that cannot be added to now is no answer rather than a panic"
        );
        assert_eq!(renewed("9223372036854775").expires_at, None);

        // The same non-answer by an easier route. An `expires_at` a second in
        // the past reads as already expired, so the Credential would be renewed
        // again on every single command — which is the outcome the overflow
        // guard above is written to prevent, reached without overflowing
        // anything.
        assert_eq!(
            renewed("-1").expires_at,
            None,
            "a token that arrives already dead is not a lifetime this reply gave"
        );
        assert_eq!(renewed("0").expires_at, None);
    }
}
