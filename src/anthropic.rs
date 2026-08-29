//! The three Anthropic endpoints Perch talks to, and the only place in Perch
//! that knows an address.
//!
//! None of this is a published contract, so it is held the way [`crate::probe`]
//! holds Claude Code's internals (ADR an-assumption-is-probed): one module
//! carries every assumption, and a reply Perch cannot make sense of is reported
//! as such rather than guessed at. What is assumed is that the usage endpoint
//! answers with an object whose values are Quota Windows, that the profile
//! endpoint says whose an access token is, and that the token endpoint renews an
//! access token, Rotating the refresh token whenever it pleases.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use zeroize::{Zeroize, Zeroizing};

use crate::host::{Host, HttpRequest, HttpResponse};
use crate::lock::{Lost, StillOurs};
use crate::registry::WindowUtilization;
use crate::secret::Secret;

/// Where an Account's Quota Windows are read from. Roughly 28-30 requests per
/// rolling hour per Account, and it does not refill early
/// (ADR a-figure-carries-its-age).
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Where Perch asks whose access token it is holding.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

/// Where a refresh token buys a fresh access token — and, when Anthropic
/// decides to Rotate, a fresh refresh token with it.
pub const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

/// The beta the OAuth endpoints are behind, sent beside the Bearer token on
/// every read.
pub const BETA: &str = "oauth-2025-04-20";

/// What an access token is presented as, and the whole of the rest of that
/// header's value.
const BEARER: &str = "Bearer ";

/// The OAuth client the Credentials Perch holds were issued to. A refresh token
/// can only be renewed by the client it was issued to, so this is Claude Code's
/// rather than one of Perch's own.
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Every Quota Window an Account has at one moment.
pub type QuotaWindows = Vec<WindowUtilization>;

/// Why an endpoint did not answer the question.
///
/// Five outcomes rather than one message, because a caller does something
/// different with each: a throttle falls back to cache, a rejection means the
/// Account has to be logged into again, and drift is not a network that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The hourly budget for this Account is spent, and does not refill early.
    Throttled,
    /// The Credential was not accepted.
    Rejected,
    /// The endpoint answered, and the answer is not the shape Perch believes in
    /// — a body that will not parse, a document missing a field. Told apart from
    /// [`Refused::Failed`], because drift in a reply Perch reads for reassurance
    /// is no reason to stop reading Utilization, and an outage folded in with it
    /// is permission to skip the ownership check.
    Unrecognized(String),
    /// The endpoint answered with a status Perch has no reading for — a 500, a
    /// 502, a 404 at a URL that used to work. Nothing about the Credential and
    /// nothing about the Account: a caller that carries on regardless is
    /// carrying on with no answer at all.
    Failed(u16),
    /// The request never got there.
    Unreachable(String),
    /// The request was never sent: whoever wanted it is no longer the one to
    /// act. Its own refusal rather than an [`Refused::Unreachable`], because a
    /// Watcher that was stopped and a network that was down are different pieces
    /// of news — one of them paces a Back-off and the other ends a round.
    Stopped(Lost),
}

const THROTTLED: &str = "Anthropic is rate-limiting reads of this Account's \
                         Utilization — about 28-30 an hour, and the window \
                         does not refill early";
const REJECTED: &str = "Anthropic did not accept the Credential";
const UNRECOGNIZED: &str = "Anthropic answered something Perch does not understand";
const FAILED: &str = "Anthropic answered with a failure";
const UNREACHABLE: &str = "Anthropic could not be reached";
const STOPPED: &str = "the request was not sent, because whoever wanted it is no \
                       longer the one to act";

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = match self {
            Refused::Throttled => THROTTLED.to_string(),
            Refused::Rejected => REJECTED.to_string(),
            Refused::Unrecognized(detail) => format!("{UNRECOGNIZED}: {detail}"),
            Refused::Failed(status) => format!("{FAILED} (HTTP {status})"),
            Refused::Unreachable(detail) => format!("{UNREACHABLE}: {detail}"),
            Refused::Stopped(_) => STOPPED.to_string(),
        };
        formatter.write_str(&said)
    }
}

/// What a renewal hands back.
#[derive(Clone, PartialEq, Eq)]
pub struct Fresh {
    pub access_token: Zeroizing<String>,
    /// The Rotated refresh token, when Anthropic Rotated one. Absent means the
    /// refresh token already stored is still the live one.
    ///
    /// `Zeroizing` because a freshly Rotated refresh token is the only copy
    /// there is, so this buffer is worth wiping as well as worth not printing.
    pub refresh_token: Option<Zeroizing<String>>,
    /// When the new access token expires, in milliseconds — the unit a
    /// Credential records it in.
    pub expires_at: Option<i64>,
}

impl std::fmt::Debug for Fresh {
    /// A freshly Rotated refresh token is the only copy there is, the old one
    /// having just been retired, so it is not one to leave a derived `Debug`
    /// able to print.
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
pub fn utilization(
    host: &dyn Host,
    access_token: &str,
    still_ours: StillOurs<'_>,
) -> Result<QuotaWindows, Refused> {
    let document = read(host, USAGE_URL, access_token, still_ours)?;
    let mut said = Vec::new();
    let windows = windows_in(&document, &mut said).map_err(Refused::Unrecognized)?;
    if windows.is_empty() {
        return Err(Refused::Unrecognized(
            "the usage endpoint named no Quota Window".to_string(),
        ));
    }
    // After the refusals, so a reply that is not going to be used says the one
    // thing wrong with it rather than two. `note` because this is a remark about
    // the shape of Anthropic's replies, the same for every Account.
    for remark in said {
        host.note(&remark);
    }
    Ok(windows)
}

/// Which Account an access token belongs to. A reply that parses and names
/// nobody is `Unrecognized` rather than an answer, and says so out loud: handed
/// back as an absence, the check that keeps one Account's figures out of
/// another's goes silent on a rename (ADR a-figure-names-its-account).
pub fn whose(
    host: &dyn Host,
    access_token: &str,
    still_ours: StillOurs<'_>,
) -> Result<String, Refused> {
    let document = read(host, PROFILE_URL, access_token, still_ours)?;
    email_in(&document).ok_or_else(|| {
        Refused::Unrecognized(
            "the profile endpoint named no email address, so whose an access \
             token is cannot be established from Anthropic — the check that \
             keeps one Account's figures from being filed under another's is \
             falling back to what this machine holds until Perch is taught the \
             new shape"
                .to_string(),
        )
    })
}

/// Renews an access token, and reports the Rotation when there was one.
pub fn renew(
    host: &dyn Host,
    refresh_token: &str,
    still_ours: StillOurs<'_>,
) -> Result<Fresh, Refused> {
    // Through `sealed`, because this body carries the only copy there is of the
    // refresh token: `to_string` would grow into it and abandon prefixes.
    let mut asking = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
    });
    let body = crate::json::sealed(&asking);
    // The tree holds a second copy and frees its strings untouched.
    wipe_the_tokens_in(&mut asking);
    let headers = [("Content-Type", "application/json")];

    // Before the request: `expires_in` is measured from when the server issued
    // the token, so a clock read on receipt makes the lifetime longer than it is
    // by the whole round trip — up to `MAX_TIME_SECONDS`.
    let now = host.now();

    let response = send(
        host,
        &HttpRequest::post(TOKEN_URL, &headers, &body),
        still_ours,
    )?;
    // A refresh token Anthropic has retired comes back as a bad request rather
    // than an unauthorized one, and it means the same thing here only where the
    // body agrees — see [`REVOKED`] and [`REFUSALS`].
    let mut document = understand(response, REFUSALS)?;

    // Read out before the tree is wiped, because dropping a `serde_json::Value`
    // frees its strings untouched and this reply holds the rotated refresh
    // token — the only copy of it there is.
    let access_token = document
        .get("access_token")
        .and_then(Value::as_str)
        .map(|token| Zeroizing::new(token.to_string()));
    let fresh = access_token.map(|access_token| Fresh {
        access_token,
        refresh_token: document
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|token| Zeroizing::new(token.to_string())),
        // Checked, at the one moment nothing can be retried: the old refresh
        // token is already retired. A lifetime that overflows or is negative is
        // one the reply did not give; one spelled as a float is not.
        expires_at: document
            .get("expires_in")
            .and_then(|value| {
                value.as_i64().or_else(|| {
                    value
                        .as_f64()
                        .filter(|seconds| seconds.is_finite())
                        .map(|seconds| seconds as i64)
                })
            })
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| seconds.checked_mul(1_000))
            .and_then(|millis| now.timestamp_millis().checked_add(millis)),
    });
    // Before the refusal as well as before the return: a reply carrying a
    // rotated refresh token and no access token is still a reply carrying the
    // only copy of that token.
    wipe_the_tokens_in(&mut document);
    fresh.ok_or_else(|| missing("access_token"))
}

/// Wipes the two token strings out of a token-endpoint reply.
///
/// `Fresh` has copied what it needs into `Zeroizing` by the time this runs, and
/// what is left is the tree serde built — whose strings a plain drop frees
/// untouched. `probe::credential_after_rotation` takes the same care.
fn wipe_the_tokens_in(document: &mut Value) {
    for field in ["access_token", "refresh_token"] {
        if let Some(Value::String(held)) = document.get_mut(field) {
            held.zeroize();
        }
    }
}

fn missing(field: &str) -> Refused {
    Refused::Unrecognized(format!("the token endpoint returned no {field}"))
}

/// A read as this Account: a Bearer token, and the beta the OAuth endpoints are
/// behind.
fn read(
    host: &dyn Host,
    url: &str,
    access_token: &str,
    still_ours: StillOurs<'_>,
) -> Result<Value, Refused> {
    // A `Secret` for the reason `renew`'s body is one: `format!` would build
    // the header in a plain `String` and free it holding the token.
    let mut authorization = Secret::with_room_for(BEARER.len() + access_token.len());
    authorization.push_str(BEARER);
    authorization.push_str(access_token);
    let headers = [
        ("Authorization", authorization.as_str()),
        ("anthropic-beta", BETA),
        ("Accept", "application/json"),
    ];

    let response = send(host, &HttpRequest::get(url, &headers), still_ours)?;
    understand(response, &[])
}

/// The only path from this module to the network, and the door the Watcher's
/// still-ours question is asked at (ADR an-invariant-gets-a-door).
///
/// `still_ours` is a parameter rather than a call each sender makes, so a sender
/// nobody has written yet cannot reach the network without obtaining one.
fn send(
    host: &dyn Host,
    request: &HttpRequest<'_>,
    still_ours: StillOurs<'_>,
) -> Result<HttpResponse, Refused> {
    // Before the request and never after one: a Renewal's reply has already
    // retired the refresh token Perch holds, so there is nothing left to stop.
    still_ours().map_err(Refused::Stopped)?;
    #[allow(
        clippy::disallowed_methods,
        reason = "this is the way through that `clippy.toml` names"
    )]
    host.http(request)
        .map_err(|err| Refused::Unreachable(err.to_string()))
}

/// OAuth's word for a refresh token that is no longer one, and the only thing a
/// refusal from the token endpoint may be read as a Quarantine.
///
/// A refusal there is the status a request gets for being wrong in any way, and
/// a Quarantine is cleared only by a login (ADR a-broken-account-is-repaired).
const REVOKED: &str = "invalid_grant";

/// The statuses the token endpoint refuses with, every one held to [`REVOKED`]
/// before it is believed. RFC 6749 §5.2 gives `invalid_client` a 401, and
/// `invalid_client` is about [`CLIENT_ID`] — the same in every renewal Perch
/// sends, so a 401 read as this Account's token being retired Quarantines a
/// whole Group in one pass.
const REFUSALS: &[u16] = &[400, 401, 403];

/// The document in a reply, or the reason there is not one. `also_rejected`
/// names the statuses this endpoint says "not you" with, and it overrides the
/// generic reading below rather than adding to it ([`REVOKED`], [`REFUSALS`]).
/// No reply body reaches a message: what an endpoint says about a Credential it
/// would not take is not something to print or log.
fn understand(response: HttpResponse, also_rejected: &[u16]) -> Result<Value, Refused> {
    if also_rejected.contains(&response.status) {
        return match says_revoked(&response.body) {
            true => Err(Refused::Rejected),
            // Not terminal, so retried rather than recorded: a Credential that
            // was never the problem goes on working.
            false => Err(Refused::Failed(response.status)),
        };
    }
    match response.status {
        200..=299 => serde_json::from_str(&response.body)
            .map_err(|err| Refused::Unrecognized(format!("the reply is not JSON: {err}"))),
        401 | 403 => Err(Refused::Rejected),
        429 => Err(Refused::Throttled),
        status => Err(Refused::Failed(status)),
    }
}

/// Whether a refusal body is the endpoint saying the refresh token is retired.
/// The field OAuth 2.0 gives this is `error`, and Anthropic writes it there:
/// a body that is not JSON, one with no `error`, or an `error` naming something
/// the request got wrong is not this.
fn says_revoked(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|document| document.get("error"))
        .and_then(Value::as_str)
        == Some(REVOKED)
}

/// The Quota Windows a usage reply describes: `limits` where the reply carries
/// one, and the top-level map where it does not (ADR a-window-comes-from-limits).
/// What makes an entry a window, and what makes an unreadable one drift, is
/// ADR headroom-is-the-worst-window's. `said` collects what is worth remarking
/// on about a reply that is still usable ([`reset_time_in`]).
fn windows_in(
    document: &Value,
    said: &mut Vec<String>,
) -> std::result::Result<QuotaWindows, String> {
    // An empty array is an answer rather than an absence, so it is not a
    // fallback: a reply naming no window is `utilization`'s to refuse.
    match document.get("limits").and_then(Value::as_array) {
        Some(limits) => ranked(limits_in(limits, said)?),
        None => map_in(document, said),
    }
}

/// The Quota Windows a `limits` array describes.
///
/// An entry meters a period its `group` names, and that is what admits it: a
/// `kind` is the narrower field and grows with every scope Anthropic adds, so a
/// rule keyed on it refuses what it has not been taught yet.
fn limits_in(
    limits: &[Value],
    said: &mut Vec<String>,
) -> std::result::Result<QuotaWindows, String> {
    let mut windows = QuotaWindows::new();
    for entry in limits {
        // A group Perch does not know is a limit beside the windows, which it is
        // not entitled to an opinion about.
        let Some(period) = entry.get("group").and_then(Value::as_str).and_then(period) else {
            continue;
        };
        let named = match qualifier(entry) {
            Some(qualifier) => format!("{period}-{qualifier}"),
            None => period.to_string(),
        };
        let Some(percent) = entry.get("percent").and_then(Value::as_f64) else {
            return Err(drifted(&named, "percent"));
        };
        windows.push(WindowUtilization {
            resets_at: reset_time_in(&named, entry, said),
            window: named,
            // A window cannot be less than empty or more than full, and clamping
            // here is what stops a figure outside that becoming "105% headroom"
            // in a sentence somebody is asked to act on.
            used_percent: percent.clamp(0.0, 100.0),
        });
    }
    the_two_are_there(&windows, EVERY_ACCOUNT_HAS_AS_LIMITS)?;
    Ok(windows)
}

/// What follows the period in a window's name.
///
/// A scope names the window before its `kind` does, because a `kind` says only
/// that a window is scoped and the scope says to what. `session` and
/// `weekly_all` meter the whole of their period and take no qualifier.
fn qualifier(entry: &Value) -> Option<String> {
    let scope = entry.get("scope");
    if let Some(named) = named_by(scope.and_then(|scope| scope.get("model")))
        .or_else(|| named_by(scope.and_then(|scope| scope.get("surface"))))
    {
        return Some(slug(&named));
    }
    let kind = entry.get("kind").and_then(Value::as_str)?;
    let group = entry
        .get("group")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind.strip_prefix(group).unwrap_or(kind).trim_matches('_') {
        "" | "all" => None,
        rest => Some(slug(rest)),
    }
}

/// What a scope names, however it names it: Anthropic writes a display name
/// under `model`, and `surface` is null in every reply seen so far, so a bare
/// string is read as the name as well.
fn named_by(scope: Option<&Value>) -> Option<String> {
    let scope = scope?;
    match scope.as_str() {
        Some(named) => Some(named.to_string()),
        None => scope
            .get("display_name")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// A name Anthropic wrote for a person, as the rest of a window's name is
/// spelled. `scope.model.id` is null, so a display name is the only handle
/// there is, and it is prose.
fn slug(named: &str) -> String {
    let mut out = String::with_capacity(named.len());
    for character in named.chars() {
        match character.is_ascii_alphanumeric() {
            true => out.push(character.to_ascii_lowercase()),
            false if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            false => {}
        }
    }
    out.truncate(out.trim_end_matches('-').len());
    out
}

/// The Quota Windows the top-level map describes, read where a reply carries no
/// `limits` array. Every value carrying a `utilization` is a window, whatever it
/// is called: what makes something a window is that it says how full it is.
fn map_in(document: &Value, said: &mut Vec<String>) -> std::result::Result<QuotaWindows, String> {
    let Some(fields) = document.as_object() else {
        return Ok(QuotaWindows::new());
    };

    let mut windows = QuotaWindows::new();
    for (name, value) in fields {
        // A block that answers to `utilization` without metering a period is not
        // a window whatever it says — see [`never_a_window`].
        if never_a_window(name) {
            continue;
        }

        // What makes something a window is that it says how full it is, which
        // is what lets a window Anthropic adds be recorded without Perch having
        // to learn its name. Everything else goes to [`is_drift`].
        let Some(used_percent) = how_full_it_says_it_is(value) else {
            if is_drift(name, value) {
                return Err(drifted(name, "utilization"));
            }
            continue;
        };
        windows.push(window_from(name, used_percent, value, said));
    }

    the_two_are_there(&windows, EVERY_ACCOUNT_HAS)?;
    ranked(windows)
}

/// The two every Account has, held to the same standard absent as unreadable.
/// `spelled` is whichever half of the reply this came from, so a refusal names
/// what Anthropic did not send rather than what Perch would have called it.
/// Only where something was read: a reply naming no window at all is
/// [`utilization`]'s to say.
fn the_two_are_there(
    windows: &QuotaWindows,
    spelled: [&str; 2],
) -> std::result::Result<(), String> {
    if windows.is_empty() {
        return Ok(());
    }
    match EVERY_ACCOUNT_HAS_SHOWN
        .iter()
        .enumerate()
        .find(|(_, shown)| !windows.iter().any(|held| held.window == **shown))
    {
        Some((at, _)) => Err(went_missing(spelled[at])),
        None => Ok(()),
    }
}

/// The windows in the order every surface shows them.
fn ranked(windows: QuotaWindows) -> std::result::Result<QuotaWindows, String> {
    // Ranked once each rather than inside a comparator that runs O(n log n)
    // times, which is what `cycle::ranked` does for the same reason.
    let mut ranked: Vec<(u8, WindowUtilization)> = windows
        .into_iter()
        .map(|window| (rank(&window.window), window))
        .collect();
    ranked.sort_by(|(theirs, one), (ours, other)| {
        theirs.cmp(ours).then_with(|| one.window.cmp(&other.window))
    });
    Ok(ranked.into_iter().map(|(_, window)| window).collect())
}

/// Whether a key names a Quota Window by the period it meters, as every one
/// Anthropic has named so far is: `five_hour`, `seven_day_opus`. Keyed on the
/// *unit* rather than the count, or the rule holds for the two counts in use and
/// for nothing else. Asked only of something Perch could not read as a window,
/// to tell drift from a field beside the windows.
fn named_by_a_period(key: &str) -> bool {
    let mut parts = key.split('_');
    let counted = parts.next().is_some_and(|count| !count.is_empty());
    counted
        && matches!(
            parts.next(),
            Some("hour" | "hours" | "day" | "days" | "week" | "weeks" | "month" | "months")
        )
}

/// How full a key says it is, when it reads as a Quota Window at all.
///
/// An object carrying a numeric `utilization`, and nothing else. Every other
/// shape is not a reading, and what to do about that is [`is_drift`]'s.
fn how_full_it_says_it_is(value: &Value) -> Option<f64> {
    value.as_object()?.get("utilization")?.as_f64()
}

/// Whether something Perch could not read as a Quota Window is drift to refuse,
/// or a field beside the windows to pass over. Naming a period is what makes
/// silence suspicious. A `null` under a per-model name is the endpoint saying
/// the Account has no such window, which is most models for most Accounts.
fn is_drift(name: &str, value: &Value) -> bool {
    named_by_a_period(name) && !says_the_account_has_no_such_window(name, value)
}

/// Whether a reply is saying the Account has no such Quota Window, rather than
/// declining to say how full one it does have is.
fn says_the_account_has_no_such_window(name: &str, value: &Value) -> bool {
    value.is_null() && !every_account_has(name)
}

/// A block the reply carries that answers to `utilization` without being a Quota
/// Window, whatever it answers. `extra_usage` is the paid-credit allowance — an
/// allowance beyond the plan rather than a constraint on it — so it is never a
/// window, even once credits are turned on and it carries a figure.
fn never_a_window(name: &str) -> bool {
    name == EXTRA_USAGE
}

/// The paid-credit allowance, which is [`never_a_window`].
const EXTRA_USAGE: &str = "extra_usage";

/// The Quota Windows every Account has, in the order they run out. Named here
/// once because [`rank`] shows them first, [`is_drift`] holds them back from
/// being read as absent, and [`windows_in`] refuses a reply that leaves one out —
/// all three because they are the ones always there to be read.
const EVERY_ACCOUNT_HAS: [&str; 2] = ["five_hour", "seven_day"];

/// The same two as Perch shows them. Written out rather than derived, because
/// [`rank`] is asked inside a comparator and `window_name` allocates; that the
/// two cannot drift apart is a test rather than a call.
const EVERY_ACCOUNT_HAS_SHOWN: [&str; 2] = ["5-hour", "7-day"];

/// The same two as a `limits` reply spells them, in the order
/// [`EVERY_ACCOUNT_HAS_SHOWN`] names them.
const EVERY_ACCOUNT_HAS_AS_LIMITS: [&str; 2] = ["session", "weekly_all"];

/// The period each `group` meters, as Perch names it. Perch's own names rather
/// than anything the reply says, since a `limits` entry carries no period at
/// all. One table, because a window is named from it on the way in and says
/// which group it came from on the way out, and a second copy is where those two
/// stop agreeing.
const PERIODS: [(&str, &str); 2] = [("session", "5-hour"), ("weekly", "7-day")];

/// What Perch calls the period a `group` meters, where it is one Perch knows.
fn period(group: &str) -> Option<&'static str> {
    PERIODS
        .iter()
        .find(|(named, _)| *named == group)
        .map(|(_, period)| *period)
}

/// Which `group` a window's name came from, for the script that needs a handle
/// steadier than a name derived from Anthropic's prose.
pub fn group_of(window: &str) -> Option<&'static str> {
    PERIODS
        .iter()
        .find(|(_, period)| {
            window == *period
                || window
                    .strip_prefix(period)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
        .map(|(group, _)| *group)
}

/// Whether a key names one of [`EVERY_ACCOUNT_HAS`], as the endpoint spells it
/// rather than as Perch shows it — `five_hour`, never `5-hour`.
fn every_account_has(name: &str) -> bool {
    EVERY_ACCOUNT_HAS.contains(&name)
}

/// `field` because the two halves of a reply say how full a window is under
/// different names, and a refusal a person is meant to check against the body
/// names the one that was actually looked for.
fn drifted(name: &str, field: &str) -> String {
    format!(
        "the usage endpoint named the Quota Window `{name}` without a numeric \
         `{field}`, so how full it is could not be read"
    )
}

fn went_missing(name: &str) -> String {
    format!(
        "the usage endpoint did not name the Quota Window `{name}` at all, and \
         every Account has one, so the fullest window could not be read"
    )
}

fn window_from(
    name: &str,
    used_percent: f64,
    value: &Value,
    said: &mut Vec<String>,
) -> WindowUtilization {
    let named = window_name(name);
    WindowUtilization {
        // A window cannot be less than empty or more than full, and clamping
        // here is what stops a figure outside that becoming "105% headroom" in a
        // sentence somebody is asked to act on.
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at: reset_time_in(&named, value, said),
        window: named,
    }
}

/// When a window says it resets, and a remark where it said something Perch could
/// not read. A window carrying no `resets_at` is ordinary; one carrying an
/// unparsable `resets_at` loses a Strategy rather than a figure, since every
/// `soonest-reset` Scope quietly becomes a `most-headroom` one with the Setting
/// unchanged.
fn reset_time_in(named: &str, value: &Value, said: &mut Vec<String>) -> Option<DateTime<Utc>> {
    // Absent and `null` are the same answer: this window does not say when it
    // resets, and Anthropic writes both.
    let carried = value.get("resets_at").filter(|at| !at.is_null())?;

    let parsed = carried
        .as_str()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc));

    if parsed.is_none() {
        said.push(format!(
            "the usage endpoint said when the `{named}` window resets in a form \
             Perch could not read, so that window is ranked as one with no reset \
             time. A Group set to `soonest-reset` chooses on Headroom alone while \
             this lasts."
        ));
    }
    parsed
}

/// A window's name as the rest of Perch says it: `five_hour` is the `5-hour`
/// window, `seven_day_opus` the `7-day-opus` one.
fn window_name(key: &str) -> String {
    let mut name = String::with_capacity(key.len());
    for part in key.split('_') {
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(match part {
            "five" => "5",
            "seven" => "7",
            other => other,
        });
    }
    name
}

/// Where a window sits when the figures are shown together: the two every
/// Account has, in the order they run out, then the per-model ones by name.
///
/// Asked of a window as Perch shows it and answered from [`EVERY_ACCOUNT_HAS`],
/// which is spelled as the endpoint spells it, so the two cannot drift apart.
fn rank(window: &str) -> u8 {
    EVERY_ACCOUNT_HAS_SHOWN
        .iter()
        .position(|shown| *shown == window)
        .map_or(EVERY_ACCOUNT_HAS.len() as u8, |at| at as u8)
}

/// The email address a profile reply names, if it names one. Two spellings,
/// because the endpoint has sent both and a machine may be talking to either.
/// Read rather than chosen between: preferring one turns the other into drift
/// on the day it comes back.
fn email_in(document: &Value) -> Option<String> {
    ["email", "email_address"]
        .into_iter()
        .flat_map(|spelled| {
            [
                document.get("account").and_then(|it| it.get(spelled)),
                document.get(spelled),
            ]
        })
        .flatten()
        .find_map(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {

    /// What `EVERY_ACCOUNT_HAS_SHOWN` is written out instead of derived for, so
    /// the two cannot drift apart without this saying so.
    #[test]
    fn the_two_windows_every_account_has_are_spelled_the_same_both_ways() {
        for (key, shown) in EVERY_ACCOUNT_HAS.iter().zip(EVERY_ACCOUNT_HAS_SHOWN) {
            assert_eq!(window_name(key), shown);
        }
    }
    #[test]
    fn a_utilization_outside_nought_to_a_hundred_is_brought_back_into_it() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": -5}, "seven_day": {"utilization": 130}}"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("both say how full they are");

        assert_eq!(windows[0].used_percent, 0.0);
        assert_eq!(windows[1].used_percent, 100.0);
    }

    use super::*;
    use crate::host::prelude::*;

    /// The windows a reply describes, with what it remarked on set aside —
    /// which is what every test here but the one about the remarks is asking.
    fn windows_of(document: &Value) -> std::result::Result<QuotaWindows, String> {
        windows_in(document, &mut Vec::new())
    }

    const USAGE: &str = r#"{
      "five_hour": {"utilization": 42, "resets_at": "2026-08-04T14:30:00Z"},
      "seven_day": {"utilization": 18.5, "resets_at": "2026-08-09T00:00:00Z"},
      "seven_day_opus": {"utilization": 3, "resets_at": "2026-08-09T00:00:00Z"},
      "seven_day_sonnet": {"utilization": 61},
      "session": "not a window"
    }"#;

    fn windows() -> QuotaWindows {
        windows_of(&serde_json::from_str(USAGE).expect("valid JSON"))
            .expect("every window says how full it is")
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
    fn a_reset_time_perch_cannot_read_is_said_rather_than_read_as_no_reset_time() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 42, "resets_at": "in about two hours"},
                "seven_day": {"utilization": 18, "resets_at": null}}"#,
        )
        .unwrap();
        let mut said = Vec::new();

        let windows = windows_in(&document, &mut said).expect("both say how full they are");

        assert_eq!(
            windows[0].used_percent, 42.0,
            "the figure still came through"
        );
        assert_eq!(windows[0].resets_at, None);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("5-hour"), "{said:?}");
        assert!(said[0].contains("soonest-reset"), "{said:?}");
        assert!(
            windows[1].resets_at.is_none(),
            "a window that says nothing about resetting is ordinary, not drift"
        );
    }

    #[test]
    fn anything_in_the_reply_that_is_not_a_window_is_not_read_as_one() {
        assert!(!windows().iter().any(|w| w.window == "session"));
        assert!(
            windows_of(&json!("not an object"))
                .expect("nothing shaped like a window is nothing to refuse")
                .is_empty()
        );
    }

    #[test]
    fn a_window_that_will_not_say_how_full_it_is_is_drift_rather_than_one_fewer_window() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": "98"}, "seven_day": {"utilization": 10}}"#,
        )
        .unwrap();

        let refused = windows_of(&document).expect_err("half a picture is not a picture");

        assert!(
            refused.contains("five_hour"),
            "it names the window: {refused}"
        );
        assert!(refused.contains("utilization"), "{refused}");
    }

    /// Said of the two windows every Account has, which is what makes a `null`
    /// there drift rather than the absence it is under a per-model name — see
    /// `a_per_model_window_the_account_does_not_have_is_absent_rather_than_drift`.
    #[test]
    fn a_window_that_stops_being_an_object_is_drift_too() {
        for flattened in [
            r#"{"five_hour": 98, "seven_day": {"utilization": 10}}"#,
            r#"{"five_hour": null, "seven_day": {"utilization": 10}}"#,
        ] {
            let document: Value = serde_json::from_str(flattened).unwrap();

            let refused = windows_of(&document)
                .err()
                .unwrap_or_else(|| panic!("half a picture is not a picture: {flattened}"));

            assert!(
                refused.contains("five_hour"),
                "it names the window: {refused}"
            );
        }
    }

    #[test]
    fn a_window_under_a_period_anthropic_has_not_used_yet_is_drift_when_it_stops_answering() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 1}, "seven_day": {"utilization": 10},
                "one_hour": 98}"#,
        )
        .unwrap();

        let refused = windows_of(&document).expect_err("a window is a window");

        assert!(refused.contains("one_hour"), "it names it: {refused}");
    }

    #[test]
    fn a_field_beside_the_windows_is_still_not_a_window() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 1}, "seven_day": {"utilization": 10},
                "organization": {"uuid": "x"}, "spend": {"percent": 3}, "extra_usage": 3}"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("those are not windows");

        assert_eq!(windows.len(), 2, "{windows:?}");
    }

    /// `is_drift` is asked once per key the reply *carries*, so a window left
    /// out altogether is never asked about — and `utilization` refuses only a
    /// reply naming no window at all.
    #[test]
    fn a_window_every_account_has_is_drift_when_the_reply_leaves_it_out() {
        for (left_out, document) in [
            ("five_hour", r#"{"seven_day": {"utilization": 10}}"#),
            ("seven_day", r#"{"five_hour": {"utilization": 10}}"#),
        ] {
            let document: Value = serde_json::from_str(document).unwrap();

            let refused = windows_of(&document).err().unwrap_or_else(|| {
                panic!("half a picture is not a picture: {left_out} was left out")
            });

            assert!(
                refused.contains(left_out),
                "it names the window that went missing: {refused}"
            );
        }
    }

    /// A per-model window is `null` for every model the Account has no window
    /// for, which is most of them.
    #[test]
    fn a_per_model_window_the_account_does_not_have_is_absent_rather_than_drift() {
        let document: Value = serde_json::from_str(
            r#"{
              "five_hour": {"utilization": 74},
              "seven_day": {"utilization": 77},
              "seven_day_opus": null,
              "seven_day_sonnet": null
            }"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("the two windows the Account has are readable");

        let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
        assert_eq!(named, vec!["5-hour", "7-day"]);
        assert_eq!(windows[0].used_percent, 74.0);
    }

    /// `extra_usage` carries a `utilization` of its own — `null` until paid
    /// credits are turned on — and sorts before every `seven_day_*`.
    #[test]
    fn a_field_beside_the_windows_is_not_drift_for_carrying_a_null_utilization() {
        let document: Value = serde_json::from_str(
            r#"{
              "five_hour": {"utilization": 74},
              "seven_day": {"utilization": 77},
              "extra_usage": {"is_enabled": false, "monthly_limit": null, "utilization": null}
            }"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("neither window is `extra_usage`'s business");

        let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
        assert_eq!(named, vec!["5-hour", "7-day"]);
    }

    #[test]
    fn the_paid_credit_allowance_is_not_a_window_once_it_carries_a_figure() {
        let document: Value = serde_json::from_str(
            r#"{
              "five_hour": {"utilization": 12},
              "seven_day": {"utilization": 5},
              "extra_usage": {"is_enabled": true, "utilization": 90.0, "monthly_limit": 200}
            }"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("both windows say how full they are");

        let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
        assert_eq!(
            named,
            vec!["5-hour", "7-day"],
            "an allowance beyond the plan is not a constraint on it"
        );
    }

    #[test]
    fn a_per_model_window_that_is_there_and_says_nothing_is_still_drift() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 74}, "seven_day_opus": {"utilization": null}}"#,
        )
        .unwrap();

        let refused = windows_of(&document).expect_err("a window that is there said nothing");

        assert!(
            refused.contains("seven_day_opus"),
            "it names the window: {refused}"
        );
    }

    #[test]
    fn a_field_beside_the_windows_is_not_one_however_object_shaped_it_is() {
        let document: Value = serde_json::from_str(
            r#"{
              "five_hour": {"utilization": 42},
              "seven_day": {"utilization": 8},
              "organization": {"uuid": "org-1", "name": "Overflow Ltd"},
              "session": "not a window"
            }"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("both windows say how full they are");

        let named: Vec<&str> = windows.iter().map(|w| w.window.as_str()).collect();
        assert_eq!(named, vec!["5-hour", "7-day"]);
    }

    #[test]
    fn a_window_that_stopped_saying_anything_at_all_is_still_drift() {
        let document: Value =
            serde_json::from_str(r#"{"five_hour": {}, "seven_day": {"utilization": 10}}"#).unwrap();

        let refused = windows_of(&document).expect_err("a window said nothing");

        assert!(
            refused.contains("five_hour"),
            "it names the window: {refused}"
        );
    }

    #[test]
    fn the_statuses_that_matter_are_told_apart() {
        let reply = |status: u16| HttpResponse {
            status,
            body: Zeroizing::new("{}".to_string()),
        };
        let said = |body: &str| HttpResponse {
            status: 400,
            body: Zeroizing::new(body.to_string()),
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
            Err(Refused::Failed(400))
        ));
        assert_eq!(understand(reply(200), &[]), Ok(json!({})));
    }

    #[test]
    fn a_bad_request_that_does_not_say_the_token_is_retired_is_not_terminal() {
        let refused = |body: &str| {
            understand(
                HttpResponse {
                    status: 400,
                    body: Zeroizing::new(body.to_string()),
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
                matches!(refused(body), Err(Refused::Failed(400))),
                "a Quarantine is for ever, and this is not the endpoint asking \
                 for one: {body}"
            );
        }
    }

    #[test]
    fn a_renewal_refused_without_saying_the_token_is_retired_is_never_terminal() {
        let refused = |status: u16, body: &str| {
            understand(
                HttpResponse {
                    status,
                    body: Zeroizing::new(body.to_string()),
                },
                REFUSALS,
            )
        };

        for status in REFUSALS.iter().copied() {
            assert!(
                matches!(refused(status, "{}"), Err(Refused::Failed(_))),
                "HTTP {status} alone is not the endpoint asking for a Quarantine"
            );
            assert_eq!(
                refused(status, r#"{"error":"invalid_grant"}"#),
                Err(Refused::Rejected),
                "and a body that does say so still is, whatever it arrives under"
            );
        }
    }

    /// The read endpoints are the mirror: there the bearer token really is the
    /// Account's Credential, so a 401 needs no body to corroborate it.
    #[test]
    fn a_read_endpoint_still_rejects_on_the_status_alone() {
        let reply = |status: u16| HttpResponse {
            status,
            body: Zeroizing::new(String::new()),
        };

        assert_eq!(understand(reply(401), &[]), Err(Refused::Rejected));
        assert_eq!(understand(reply(403), &[]), Err(Refused::Rejected));
    }

    #[test]
    fn a_profile_reply_names_the_account_or_says_nothing() {
        for named in [
            json!({"account": {"email": "someone@example.com"}}),
            json!({"account": {"email_address": "someone@example.com"}}),
            json!({"email_address": "someone@example.com"}),
        ] {
            assert_eq!(
                email_in(&named),
                Some("someone@example.com".to_string()),
                "which Account a token belongs to is what this endpoint is for: {named}"
            );
        }
        assert_eq!(email_in(&json!({"organization": {"name": "Acme"}})), None);
    }

    #[test]
    fn a_spelling_that_says_nothing_does_not_shut_out_the_one_that_does() {
        let named = json!({
            "account": {"email": null, "email_address": "someone@example.com"}
        });

        assert_eq!(email_in(&named), Some("someone@example.com".to_string()));
    }

    /// The ask is in front of the request rather than beside it: a Renewal is the
    /// one request that cannot be taken back, because Anthropic retires the old
    /// refresh token as it answers.
    #[test]
    fn a_stopped_ask_sends_nothing_and_travels_as_itself() {
        let host = crate::host::FakeHost::new();
        host.reply(
            TOKEN_URL,
            None,
            200,
            r#"{"access_token":"sk-ant-oat01-new"}"#,
        );

        let refused = renew(&host, "sk-ant-ort01-old", &mut || Err(Lost::Stopped));

        assert_eq!(refused, Err(Refused::Stopped(Lost::Stopped)));
        assert!(
            host.http_calls().is_empty(),
            "and the request the endpoint would have answered never went out"
        );
    }

    /// A reply with no `access_token` is still a reply, and where Anthropic
    /// Rotated it carries the only copy of the new refresh token. The refusal is
    /// raised after the tree is wiped rather than through it, which is the one
    /// path the `?` used to leave by.
    #[test]
    fn a_reply_missing_the_access_token_is_wiped_before_it_is_refused() {
        let host = crate::host::FakeHost::new();
        host.reply(
            TOKEN_URL,
            None,
            200,
            r#"{"refresh_token":"sk-ant-ort01-rotated","expires_in":3600}"#,
        );

        assert_eq!(
            renew(&host, "sk-ant-ort01-old", &mut || Ok(())),
            Err(missing("access_token")),
            "a reply Perch cannot use is refused, and named"
        );
    }

    /// By the time this runs the old refresh token is retired and the Credential
    /// carrying its replacement is not stored yet, so nothing can be retried.
    #[test]
    fn a_lifetime_that_does_not_fit_is_a_lifetime_the_reply_did_not_give() {
        let host = crate::host::FakeHost::new();
        let renewed = |expires_in: &str| {
            let reply =
                format!(r#"{{"access_token":"sk-ant-oat01-new","expires_in":{expires_in}}}"#);
            host.reply(TOKEN_URL, None, 200, &reply);
            renew(&host, "sk-ant-ort01-old", &mut || Ok(())).expect("the endpoint answered")
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

        // An `expires_at` in the past reads as already expired, so the
        // Credential would be renewed on every single command.
        assert_eq!(
            renewed("-1").expires_at,
            None,
            "a token that arrives already dead is not a lifetime this reply gave"
        );
        assert_eq!(renewed("0").expires_at, None);

        // `as_i64` answers `None` for a lifetime JSON holds as a float, and a
        // Credential stored with no `expiresAt` is one `usable_at` takes at its
        // word for ever.
        assert_eq!(
            renewed("3600.0").expires_at,
            Some(host.now().timestamp_millis() + 3_600_000),
            "a lifetime spelled as a float is a lifetime the reply gave"
        );
        assert_eq!(
            renewed("3.6e3").expires_at,
            Some(host.now().timestamp_millis() + 3_600_000)
        );
        assert_eq!(
            renewed("-1.0").expires_at,
            None,
            "and the refusals hold whichever way the number is spelled"
        );
    }

    /// A real reply, trimmed of the blocks no rule here reads. Kept whole where
    /// it is evidence: the codename keys, the nulls beside them, and the
    /// `limits` array describing the same three windows the map describes two of.
    const BOTH_HALVES: &str = r#"{
      "five_hour": {"utilization": 2.0, "resets_at": "2026-08-29T13:20:00.1+00:00"},
      "seven_day": {"utilization": 10.0, "resets_at": "2026-09-03T20:00:00.1+00:00"},
      "seven_day_opus": null,
      "seven_day_sonnet": null,
      "tangelo": null,
      "iguana_necktie": null,
      "nimbus_quill": {"utilization": 0.0, "resets_at": null},
      "juniper_tide": null,
      "extra_usage": {"is_enabled": false, "utilization": null},
      "limits": [
        {"kind": "session", "group": "session", "percent": 2,
         "resets_at": "2026-08-29T13:20:00.1+00:00", "scope": null, "is_active": false},
        {"kind": "weekly_all", "group": "weekly", "percent": 10,
         "resets_at": "2026-09-03T20:00:00.1+00:00", "scope": null, "is_active": true},
        {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "resets_at": null,
         "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null},
         "is_active": false}
      ],
      "spend": {"percent": 0, "enabled": false},
      "member_dashboard_available": false
    }"#;

    fn both_halves() -> QuotaWindows {
        windows_of(&serde_json::from_str(BOTH_HALVES).expect("valid JSON"))
            .expect("every limit says how full it is")
    }

    fn named(windows: QuotaWindows) -> Vec<String> {
        windows.into_iter().map(|window| window.window).collect()
    }

    #[test]
    fn a_reply_carrying_both_halves_is_read_from_limits() {
        assert_eq!(named(both_halves()), ["5-hour", "7-day", "7-day-fable"]);
    }

    #[test]
    fn a_window_the_map_names_only_by_a_codename_is_named_by_its_model() {
        let named = named(both_halves());

        assert!(
            !named.iter().any(|window| window.contains("nimbus")),
            "the codename is the map's name for the window `limits` calls Fable: {named:?}"
        );
    }

    #[test]
    fn a_scoped_window_carries_the_period_its_group_meters() {
        let fable = &both_halves()[2];

        assert_eq!(fable.window, "7-day-fable");
        assert_eq!(fable.used_percent, 0.0);
        assert_eq!(fable.resets_at, None, "the reply gives it none");
    }

    #[test]
    fn every_window_a_limits_reply_names_says_which_group_it_came_from() {
        for window in both_halves() {
            assert!(
                group_of(&window.window).is_some(),
                "{} came from a group and can name it",
                window.window
            );
        }
        assert_eq!(group_of("5-hour"), Some("session"));
        assert_eq!(group_of("7-day-fable"), Some("weekly"));
        assert_eq!(
            group_of("nimbus-quill"),
            None,
            "the map's names say nothing"
        );
    }

    #[test]
    fn an_empty_limits_array_is_an_answer_rather_than_a_reply_without_one() {
        let document: Value = serde_json::from_str(
            r#"{"limits": [], "five_hour": {"utilization": 1},
                "seven_day": {"utilization": 10}}"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("nothing here is drift");

        assert!(
            windows.is_empty(),
            "the map is read where there is no array, not where the array is empty: {windows:?}"
        );
    }

    #[test]
    fn a_reply_with_no_limits_array_is_read_from_the_map() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 1}, "seven_day": {"utilization": 10},
                "nimbus_quill": {"utilization": 4}}"#,
        )
        .unwrap();

        let named = named(windows_of(&document).expect("every key that answers is a window"));

        assert_eq!(
            named,
            vec!["5-hour", "7-day", "nimbus-quill"],
            "a key nobody can read is still a window on the fallback"
        );
    }

    #[test]
    fn a_limits_array_that_is_not_an_array_falls_back_to_the_map() {
        let document: Value = serde_json::from_str(
            r#"{"limits": {"session": 2}, "five_hour": {"utilization": 1},
                "seven_day": {"utilization": 10}}"#,
        )
        .unwrap();

        assert_eq!(windows_of(&document).expect("the map answers").len(), 2);
    }

    #[test]
    fn a_limit_in_a_group_perch_does_not_know_is_not_a_window() {
        let document: Value = serde_json::from_str(
            r#"{"limits": [
                {"kind": "session", "group": "session", "percent": 2},
                {"kind": "weekly_all", "group": "weekly", "percent": 10},
                {"kind": "monthly_all", "group": "monthly", "percent": 90},
                {"kind": "spend", "group": "spend"}
            ]}"#,
        )
        .unwrap();

        let named = named(
            windows_of(&document).expect("an unknown group is passed over rather than refused"),
        );

        assert_eq!(named, vec!["5-hour", "7-day"]);
    }

    #[test]
    fn a_limit_in_a_group_perch_knows_that_will_not_say_how_full_it_is_is_drift() {
        let document: Value = serde_json::from_str(
            r#"{"limits": [
                {"kind": "session", "group": "session", "percent": 2},
                {"kind": "weekly_all", "group": "weekly", "percent": 10},
                {"kind": "weekly_scoped", "group": "weekly", "percent": "lots",
                 "scope": {"model": {"display_name": "Fable"}}}
            ]}"#,
        )
        .unwrap();

        let refused = windows_of(&document).expect_err("a window is a window");

        assert!(refused.contains("7-day-fable"), "it names it: {refused}");
        assert!(refused.contains("percent"), "and the field: {refused}");
    }

    #[test]
    fn a_limits_reply_missing_one_of_the_two_names_it_as_limits_spells_it() {
        for (left_out, kept) in [("session", "weekly_all"), ("weekly_all", "session")] {
            let group = match kept {
                "session" => "session",
                _ => "weekly",
            };
            let document: Value = serde_json::from_str(&format!(
                r#"{{"limits": [{{"kind": "{kept}", "group": "{group}", "percent": 10}}]}}"#
            ))
            .unwrap();

            let refused = windows_of(&document).expect_err("half a picture is not a picture");

            assert!(refused.contains(left_out), "{refused}");
        }
    }

    #[test]
    fn a_limit_scoped_to_something_that_is_not_a_model_is_named_from_that() {
        let document: Value = serde_json::from_str(
            r#"{"limits": [
                {"kind": "session", "group": "session", "percent": 2},
                {"kind": "weekly_all", "group": "weekly", "percent": 10},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 5,
                 "scope": {"model": null, "surface": {"display_name": "Claude Code"}}},
                {"kind": "weekly_oauth_apps", "group": "weekly", "percent": 7}
            ]}"#,
        )
        .unwrap();

        let named = named(windows_of(&document).expect("every limit answers"));

        assert_eq!(
            named,
            vec!["5-hour", "7-day", "7-day-claude-code", "7-day-oauth-apps"],
            "a scope names the window, and a kind names it where no scope does"
        );
    }
}
