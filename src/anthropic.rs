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
use zeroize::Zeroizing;

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
    /// The endpoint answered, and the answer is not the shape Perch believes
    /// in — a body that will not parse, a document missing a field.
    ///
    /// Drift, in other words: Anthropic changed something and this build has
    /// not caught up. Told apart from [`Refused::Failed`] because ADR 0019
    /// carves out exactly this and nothing wider — "drift in a reply Perch
    /// reads for reassurance is no reason to stop reading Utilization at all" —
    /// and folding an HTTP failure in with it turned an outage into permission
    /// to skip the ownership check.
    Unrecognized(String),
    /// The endpoint answered with a status Perch has no reading for — a 500, a
    /// 502, a 404 at a URL that used to work.
    ///
    /// Nothing about the Credential and nothing about the Account: an outage,
    /// or a request that never reached the thing it was for. A caller that
    /// carries on regardless is carrying on with no answer at all.
    Failed(u16),
    /// The request never got there.
    Unreachable(String),
}

const THROTTLED: &str = "Anthropic is rate-limiting reads of this Account's \
                         Utilization — about 28-30 an hour, and the window \
                         does not refill early";
const REJECTED: &str = "Anthropic did not accept the Credential";
const UNRECOGNIZED: &str = "Anthropic answered something Perch does not understand";
const FAILED: &str = "Anthropic answered with a failure";
const UNREACHABLE: &str = "Anthropic could not be reached";

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let said = match self {
            Refused::Throttled => THROTTLED.to_string(),
            Refused::Rejected => REJECTED.to_string(),
            Refused::Unrecognized(detail) => format!("{UNRECOGNIZED}: {detail}"),
            Refused::Failed(status) => format!("{FAILED} (HTTP {status})"),
            Refused::Unreachable(detail) => format!("{UNREACHABLE}: {detail}"),
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
    /// `Zeroizing` for the reason the `Debug` below gives, one step further: a
    /// freshly Rotated refresh token is the only copy there is, so this buffer
    /// is worth wiping as well as worth not printing.
    pub refresh_token: Option<Zeroizing<String>>,
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
    let mut said = Vec::new();
    let windows = windows_in(&document, &mut said).map_err(Refused::Unrecognized)?;
    if windows.is_empty() {
        return Err(Refused::Unrecognized(
            "the usage endpoint named no Quota Window".to_string(),
        ));
    }
    // After the refusals, so a reply that is not going to be used says the one
    // thing that is wrong with it rather than two. Said once, which is what
    // `Terminal::note` is for: this is a remark about the shape of Anthropic's
    // replies, and it is the same remark for every Account on the machine.
    for remark in said {
        host.note(&remark);
    }
    Ok(windows)
}

/// Which Account an access token belongs to.
///
/// A reply that parses and names nobody is `Unrecognized` rather than an
/// answer, which is how the other reader in this module treats a shape it does
/// not know: `windows_in` refuses a drifted window and `reset_time_in` remarks
/// on a `resets_at` it cannot parse. This one folded "the endpoint named nobody"
/// together with "the endpoint is not shaped the way Perch believes" and handed
/// both back as `None` — which [`crate::observe::confirm`] reads as permission
/// to cache.
///
/// So the day Anthropic renamed `email_address`, the ADR 0019 guard would have
/// become a no-op for every Account, for ever, with nothing printed anywhere.
/// That is strictly worse than the outage this endpoint's other failure mode
/// causes, because an outage ends and a rename does not.
///
/// It stays a *carve-out* rather than a refusal — `confirm` still goes on to
/// read Utilization, because drift in a reply is no evidence either way — but
/// now it says so out loud.
pub fn whose(host: &dyn Host, access_token: &str) -> Result<String, Refused> {
    let document = read(host, PROFILE_URL, access_token)?;
    email_in(&document).ok_or_else(|| {
        Refused::Unrecognized(
            "the profile endpoint named no email address, so whose an access \
             token is cannot be established — the check that keeps one \
             Account's figures from being filed under another's is passing \
             everything through until Perch is taught the new shape"
                .to_string(),
        )
    })
}

/// Renews an access token, and reports the Rotation when there was one.
pub fn renew(host: &dyn Host, refresh_token: &str) -> Result<Fresh, Refused> {
    // `Zeroizing`, because this body carries the refresh token — the one this
    // module calls "the most valuable secret in Perch… the only copy there is"
    // two screens down, where the *reply* is wrapped for exactly that reason.
    // The request holding it was a plain `String` dropped untouched.
    let body = Zeroizing::new(
        json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        })
        .to_string(),
    );
    let headers = [("Content-Type", "application/json")];

    let response = send(host, &HttpRequest::post(TOKEN_URL, &headers, &body))?;
    // A refresh token Anthropic has retired comes back as a bad request rather
    // than as an unauthorized one, and where the body agrees it means the same
    // thing here: this Account cannot be renewed and has to be logged into
    // again. Where the body does not agree, the status is only a status — see
    // [`REVOKED`] and [`REFUSALS`].
    let document = understand(response, REFUSALS)?;
    let now = host.now();

    Ok(Fresh {
        access_token: Zeroizing::new(
            document
                .get("access_token")
                .and_then(Value::as_str)
                .ok_or_else(|| missing("access_token"))?
                .to_string(),
        ),
        refresh_token: document
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|token| Zeroizing::new(token.to_string())),
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
        //
        // A number JSON holds as a float is the same lifetime spelled another
        // way, and `as_i64` answers `None` for every one of them — `3600.0` and
        // `3.6e3` were indistinguishable here from a reply that gave no
        // lifetime at all. What that costs is not nothing: a Credential stored
        // without an `expiresAt` is one `usable_at` takes at its word for ever,
        // so every later reading of that Account goes the long way round, being
        // refused by Anthropic before anything renews it.
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
    })
}

fn missing(field: &str) -> Refused {
    Refused::Unrecognized(format!("the token endpoint returned no {field}"))
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
/// thing a refusal from the token endpoint may be read as a Quarantine.
///
/// A refusal there is the status a request gets for being wrong in *any* way,
/// and the caller acts on this one for ever: [`observe`](crate::observe) turns
/// `Refused::Rejected` from a renewal into `Quarantine::RenewalRejected`, which
/// only a browser login clears. Reading a malformed request, a proxy's own
/// error page, or a `client_id` Anthropic has changed its mind about as "log in
/// again" would walk a whole Group and Quarantine every Account in it — which a
/// single `perch list <group> --refresh` does in one pass.
const REVOKED: &str = "invalid_grant";

/// The statuses the token endpoint refuses with, every one of them held to
/// [`REVOKED`] before it is believed.
///
/// All three, rather than the 400 Anthropic sends today, because which one a
/// refusal arrives under is not something Perch gets to decide and the cost of
/// guessing wrong is terminal. RFC 6749 §5.2 gives `invalid_client` a 401, and
/// `invalid_client` is a statement about the `client_id` in the request — which
/// is Perch's, hard-coded at [`CLIENT_ID`], and the same in every renewal it
/// ever sends. A 401 read as "this Account's refresh token is retired" would
/// Quarantine every Account in a Group the day Anthropic retires that client
/// id, and nothing short of a login for each would clear it.
///
/// A 401 or 403 whose body *does* say `invalid_grant` still Quarantines, which
/// is the whole point of asking the body rather than the status.
const REFUSALS: &[u16] = &[400, 401, 403];

/// The document in a reply, or the reason there is not one.
///
/// `also_rejected` names the statuses this endpoint says "not you" with, and it
/// is believed only when the body agrees: see [`REVOKED`]. It *overrides* the
/// generic reading below rather than adding to it, which is what lets the token
/// endpoint hold its 401 and 403 to the same evidence as its 400 — see
/// [`REFUSALS`]. No reply body reaches a message either way: what an endpoint
/// says about a Credential it would not take is not something to print or log.
fn understand(response: HttpResponse, also_rejected: &[u16]) -> Result<Value, Refused> {
    if also_rejected.contains(&response.status) {
        return match says_revoked(&response.body) {
            true => Err(Refused::Rejected),
            // Not terminal, so it is retried rather than recorded: the next
            // Refresh asks again, and a Credential that was never the problem
            // goes on working.
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
/// Every value in the reply that carries a `utilization` is one, whatever it is
/// called: a five-hour window, a seven-day window and one per model are what
/// there are today, and a per-model window added tomorrow is recorded without
/// Perch having to learn its name first. What makes something a window is the
/// key rather than the type — a field beside them that happens to be an object
/// is not one, and Perch is not entitled to an opinion about it.
/// `said` collects what is worth remarking on about a reply that is still
/// usable — see [`reset_time_in`]. Refusals travel in the return value; these
/// are the things that degrade rather than fail.
fn windows_in(
    document: &Value,
    said: &mut Vec<String>,
) -> std::result::Result<QuotaWindows, String> {
    let Some(fields) = document.as_object() else {
        return Ok(QuotaWindows::new());
    };

    let mut windows = QuotaWindows::new();
    for (name, value) in fields {
        // A block that answers to `utilization` without metering a period is
        // not a window whatever it says, so it is settled before the reading is
        // even attempted — see [`never_a_window`].
        if never_a_window(name) {
            continue;
        }

        // How full the key says it is, when it reads as a window at all. What
        // makes something a window is that it says how full it is: the reply
        // carries fields beside the windows and Perch is not entitled to an
        // opinion about those, which is what lets a window Anthropic adds be
        // recorded without Perch having to learn its name first.
        //
        // Everything that could not be read that way goes to [`is_drift`],
        // which is the single place that decides whether it is a field to pass
        // over or a window that has stopped answering. Three arms used to
        // decide it separately and one of them did not ask, which is what took
        // every Account on the machine to "never observed" (#105).
        let Some(used_percent) = how_full_it_says_it_is(value) else {
            if is_drift(name, value) {
                return Err(drifted(name));
            }
            continue;
        };
        windows.push(window_from(name, used_percent, value, said));
    }

    // The two every Account has, held to the same standard when they are missing
    // as when they are unreadable. [`is_drift`] can only refuse a window that is
    // *there* and has stopped answering, because it is asked once per key the
    // reply carries — so a `"five_hour": null` was the loud failure and a
    // `five_hour` the reply simply left out was the quiet one, which is the same
    // ADR 0012 loss arriving by omission instead of by type.
    //
    // Only where something was read. A reply carrying no window at all is not
    // this — it is an answer that says nothing about the Account rather than one
    // missing a window, and [`utilization`] already has the sentence for it.
    if !windows.is_empty()
        && let Some(missing) = EVERY_ACCOUNT_HAS
            .iter()
            .find(|name| !windows.iter().any(|held| held.window == window_name(name)))
    {
        return Err(went_missing(missing));
    }

    windows.sort_by(|one, other| {
        rank(&one.window)
            .cmp(&rank(&other.window))
            .then_with(|| one.window.cmp(&other.window))
    });
    Ok(windows)
}

/// Whether a key names a Quota Window by the period it meters, which is how
/// every one Anthropic has named so far is named: `five_hour`, `seven_day`,
/// `seven_day_opus`.
///
/// Only ever asked of something Perch could not read as a window, and only to
/// decide whether that is drift or a field beside the windows. A window
/// Anthropic adds under a period Perch has never seen is still recorded — it
/// says how full it is, and that is what makes it a window.
///
/// Keyed on the *unit* rather than on the count, which is what makes that last
/// sentence true. `matches!(.., "five" | "seven")` was an allowlist of the two
/// counts in use, so the argument this whole module makes — that a window
/// dropped in silence "becomes one nothing would ever rank on" — held for
/// exactly those two and for nothing else. A `one_hour` or `thirty_day` window
/// that arrived flattened to a bare number would have been passed over without
/// a word, and the ADR 0012 failure would be back under a new name.
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
/// shape — a bare number, a `null`, an object with no `utilization`, an
/// `organization` block, the `limits` array — is not a reading, and what to do
/// about that is [`is_drift`]'s to decide rather than this one's.
fn how_full_it_says_it_is(value: &Value) -> Option<f64> {
    value.as_object()?.get("utilization")?.as_f64()
}

/// Whether something Perch could not read as a Quota Window is drift to refuse,
/// or a field beside the windows to pass over.
///
/// The single place that draws the line, because it used to be drawn in three
/// and one of them drew it differently (#105).
///
/// Drift is refused rather than dropped. Dropped silently, a window becomes one
/// nothing would ever rank on: a reply where the five-hour window stops being a
/// number — flattened to `"five_hour": 98`, emptied to `"five_hour": {}`, or
/// quoted as `"98"` — leaves the weekly window at 10% as the fullest Perch can
/// see, so an Account reporting 90% Headroom is one whose five-hour window is
/// 98% full, ranked top of its Group, switched onto and dead on arrival. That
/// is precisely what ADR 0012 exists to refuse.
///
/// The name is what tells the two apart. A name that says a period is a window;
/// one that does not is a field Perch is not entitled to an opinion about, and
/// reading those as windows failed the whole Refresh for every Account in one
/// pass, in a message calling a thing that is not a window a Quota Window.
///
/// Except where the reply is saying the Account has no such window, which is
/// `null` under a name beyond the two every Account has. The endpoint nulls out
/// every per-model window the Account has no window for, which is most models
/// for most Accounts, and reading that as drift refused the whole reply (#105).
/// Nothing is lost by passing it over: a window that is not there meters
/// nothing, so it can never be the fullest one.
///
/// The two every Account has are held back from that generosity, because they
/// are the ones always there to be read — so a `null` in their place is the
/// field going missing rather than the Account not having it, and passing that
/// over is the ADR 0012 failure above arriving as a `null` instead of as a `98`.
/// Refusing the reply is the loud failure and dropping the window is the quiet
/// one, and this is the one place in Perch that prefers the loud one.
fn is_drift(name: &str, value: &Value) -> bool {
    named_by_a_period(name) && !says_the_account_has_no_such_window(name, value)
}

/// Whether a reply is saying the Account has no such Quota Window, rather than
/// declining to say how full one it does have is.
fn says_the_account_has_no_such_window(name: &str, value: &Value) -> bool {
    value.is_null() && !every_account_has(name)
}

/// A block the reply carries that answers to `utilization` without being a
/// Quota Window, whatever it answers.
///
/// `extra_usage` is the paid-credit allowance: `null` until credits are turned
/// on, and a figure once they are. Passing it over only while it was `null` —
/// which is all #105 needed — left it becoming a window the moment somebody
/// bought credits, and a window is what Headroom takes its most constrained of.
/// That reads an allowance *beyond* the plan as a constraint *on* it, so an
/// Account with nine tenths of its credits spent would rank as one with no room
/// left, which is backwards: extra usage is what an Account draws on after its
/// Quota Windows are full, never a thing that fills.
fn never_a_window(name: &str) -> bool {
    name == EXTRA_USAGE
}

/// The paid-credit allowance, which is [`never_a_window`].
const EXTRA_USAGE: &str = "extra_usage";

/// The Quota Windows every Account has, in the order they run out. The only
/// two, and named here once because [`rank`] shows them first, [`is_drift`]
/// holds them back from being read as absent, and [`windows_in`] refuses a
/// reply that leaves one out — all three for the same reason: they are the ones
/// always there to be read.
const EVERY_ACCOUNT_HAS: [&str; 2] = ["five_hour", "seven_day"];

/// Whether a key names one of [`EVERY_ACCOUNT_HAS`], as the endpoint spells it
/// rather than as Perch shows it — `five_hour`, never `5-hour`.
fn every_account_has(name: &str) -> bool {
    EVERY_ACCOUNT_HAS.contains(&name)
}

fn drifted(name: &str) -> String {
    format!(
        "the usage endpoint named the Quota Window `{name}` without a numeric \
         `utilization`, so how full it is could not be read"
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
    WindowUtilization {
        window: window_name(name),
        // How full a window is, and a window cannot be less than empty or more
        // than full. Anything outside that is a reply Perch does not understand
        // rather than a figure, and clamping it here is what stops it becoming
        // "105% headroom" in a sentence somebody is asked to act on.
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at: reset_time_in(name, value, said),
    }
}

/// When a window says it resets, and a remark when it said something and Perch
/// could not read it.
///
/// A window that does not carry `resets_at` at all is ordinary — the per-model
/// ones routinely do not — and `None` is the honest answer for it. One that
/// carries a `resets_at` in a form Perch cannot parse is drift, and it used to
/// become the same `None` in silence.
///
/// That is not a figure lost, it is a strategy lost. `Headroom::Room` takes its
/// reset from the window it was measured by and `Strategy::SoonestReset` ranks
/// on exactly that, so a date format Anthropic changes turns every
/// `soonest-reset` Group on the machine into a `most-headroom` one — with the
/// setting still reading `soonest-reset`, and nothing anywhere saying otherwise.
/// It is not a refusal: how full the window is came through, which is what the
/// reading was for and what every other decision rests on.
fn reset_time_in(name: &str, value: &Value, said: &mut Vec<String>) -> Option<DateTime<Utc>> {
    // Absent and `null` are the same answer: this window does not say when it
    // resets, and Anthropic writes both.
    let carried = value.get("resets_at").filter(|at| !at.is_null())?;

    let parsed = carried
        .as_str()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc));

    if parsed.is_none() {
        said.push(format!(
            "the usage endpoint said when the `{}` window resets in a form Perch \
             could not read, so that window is ranked as one with no reset time. \
             A Group set to `soonest-reset` chooses on Headroom alone while this \
             lasts.",
            window_name(name),
        ));
    }
    parsed
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
///
/// Asked of a window as Perch shows it, and answered from
/// [`EVERY_ACCOUNT_HAS`], which is spelled as the endpoint spells it — so the
/// two orders cannot drift apart as they did when each held its own copy.
fn rank(window: &str) -> u8 {
    EVERY_ACCOUNT_HAS
        .iter()
        .position(|key| window_name(key) == window)
        .map_or(EVERY_ACCOUNT_HAS.len() as u8, |at| at as u8)
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

    /// A window that *says* when it resets in a form Perch cannot read is drift,
    /// and it used to become the same `None` a window that says nothing gets.
    ///
    /// What that loses is a strategy rather than a figure. `Headroom::Room`
    /// takes its reset from the window it was measured by and
    /// `Strategy::SoonestReset` ranks on exactly that, so one change to the date
    /// format turns every `soonest-reset` Group on the machine into a
    /// `most-headroom` one — with the setting still reading `soonest-reset` and
    /// nothing anywhere saying otherwise.
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

    /// A window that is shaped like one and will not say how full it is is
    /// drift, and drift is refused rather than dropped.
    ///
    /// Dropping it silently leaves the windows that did parse looking like the
    /// whole picture. Here the five-hour window is 98% full and unreadable, so
    /// the weekly one at 10% becomes the fullest Perch can see — an Account
    /// reporting 90% headroom that a Cycle would land on and that dies
    /// immediately. That is the ADR 0012 failure mode arriving through a type
    /// change rather than through a name Perch had not been taught.
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

    /// The same rule one level up. A window that stops being an object at all —
    /// the reply flattened to `"five_hour": 98`, or the field nulled out during
    /// an outage — is the identical loss with the identical consequence, and
    /// passing over it silently was how the ADR 0012 failure got back in: the
    /// weekly window at 10% becomes the fullest Perch can see, and the Account
    /// whose five-hour window is 98% full ranks top of its Group.
    ///
    /// Said of the two windows every Account has, which is what makes a `null`
    /// there drift rather than the absence it is under a per-model name — see
    /// [`a_per_model_window_the_account_does_not_have_is_absent_rather_than_drift`].
    #[test]
    fn a_window_that_stops_being_an_object_is_drift_too() {
        for flattened in [
            r#"{"five_hour": 98, "seven_day": {"utilization": 10}}"#,
            r#"{"five_hour": null, "seven_day": {"utilization": 10}}"#,
        ] {
            let document: Value = serde_json::from_str(flattened).unwrap();

            let refused =
                windows_of(&document).expect_err("half a picture is not a picture: {flattened}");

            assert!(
                refused.contains("five_hour"),
                "it names the window: {refused}"
            );
        }
    }

    /// And a window under a period Anthropic has not used yet is drift on the
    /// same terms, rather than a field beside the windows.
    ///
    /// `named_by_a_period` was an allowlist of the two counts in use — `five`
    /// and `seven` — so the whole argument this module makes about a window
    /// dropped in silence held for those two and for nothing else. A `one_hour`
    /// window flattened to a number would have been passed over without a word,
    /// and the ADR 0012 failure would be back under a new name. It keys on the
    /// unit now, which is what its own doc always claimed.
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

    /// The other side of the same line: a field beside the windows is still
    /// passed over, so keying on the unit did not turn every unread key into a
    /// refusal.
    #[test]
    fn a_field_beside_the_windows_is_still_not_a_window() {
        let document: Value = serde_json::from_str(
            r#"{"five_hour": {"utilization": 1}, "seven_day": {"utilization": 10},
                "organization": {"uuid": "x"}, "limits": [], "extra_usage": 3}"#,
        )
        .unwrap();

        let windows = windows_of(&document).expect("those are not windows");

        assert_eq!(windows.len(), 2, "{windows:?}");
    }

    /// A window every Account has, left out of the reply altogether, is the same
    /// loss as one that has stopped answering — and it used to be the quiet one.
    ///
    /// [`is_drift`] is asked once per key the reply *carries*, so it can only
    /// ever catch a window that is there and unreadable. A `five_hour` the reply
    /// simply omits was never asked about, and `utilization` refuses only a reply
    /// naming no window at all — so a partial outage answering with the weekly
    /// window alone left an Account whose five-hour window was 98% full reading
    /// as 90% Headroom, ranked top of its Group and switched onto. That is
    /// exactly the ADR 0012 failure [`is_drift`]'s doc refuses, arriving by
    /// omission instead of by type.
    #[test]
    fn a_window_every_account_has_is_drift_when_the_reply_leaves_it_out() {
        for (left_out, document) in [
            ("five_hour", r#"{"seven_day": {"utilization": 10}}"#),
            ("seven_day", r#"{"five_hour": {"utilization": 10}}"#),
        ] {
            let document: Value = serde_json::from_str(document).unwrap();

            let refused = windows_of(&document)
                .expect_err("half a picture is not a picture: {left_out} was left out");

            assert!(
                refused.contains(left_out),
                "it names the window that went missing: {refused}"
            );
        }
    }

    /// A per-model window is `null` for every model the Account has no window
    /// for, which is most of them, and that is the endpoint saying there is no
    /// such window rather than a window declining to say how full it is.
    ///
    /// Read as drift it refused the whole reply, and every Account on the
    /// machine read "never observed" — no ranking, no Headroom, no Reserve, no
    /// Watcher and an empty Utilization tab (#105). Nothing is lost by passing
    /// it over: a window the Account does not have is metering nothing, so it
    /// cannot be the one that is 98% full.
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

    /// The `extra_usage` block Anthropic sends is not a window and never was,
    /// but it carries a `utilization` of its own — `null` until paid credits are
    /// turned on — and that reached the one drift branch that did not first ask
    /// whether the name said a period at all (#105).
    ///
    /// It sorts before every `seven_day_*`, so this is the refusal that actually
    /// fired, and the one behind it stayed hidden until it was fixed.
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

    /// And still not a window once paid credits are turned on and it carries a
    /// figure. Passing it over only while it was `null` was enough to stop the
    /// refusal, and left it becoming a Quota Window the moment somebody bought
    /// credits — feeding Headroom, Reserve and every ranking.
    ///
    /// Backwards in the dangerous direction: extra usage is what an Account
    /// draws on *after* its Quota Windows are full, so an allowance nine tenths
    /// spent would read as an Account with a tenth of its room left and drop
    /// down the Group it should be leading.
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

    /// A window that is there and will not say how full it is is still drift,
    /// per-model name or not. `null` under a per-model name is the endpoint
    /// saying there is no such window; a window that is present and carries a
    /// `utilization` of `null` is one that has stopped answering, which is the
    /// shape Perch has never seen and must not read as room.
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

    /// What makes something a Quota Window is the `utilization` key, not the
    /// fact that it is an object. A reply that grows an object-valued field
    /// beside the windows — an `organization` block, a `limits` block — is
    /// Anthropic adding something Perch is not entitled to an opinion about,
    /// and reading it as a window failed the whole Refresh for every Account in
    /// one pass, in a message calling it a Quota Window, with nothing the user
    /// could do about it.
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

    /// The line between the two, which is the name. Something carrying no
    /// `utilization` is a field to pass over only where its name does not say a
    /// period — one that does is a window that has stopped saying how full it
    /// is, and passing over *that* would drop the fullest window Perch has out
    /// of the picture silently.
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
            Err(Refused::Failed(400))
        ));
        assert_eq!(understand(reply(200), &[]), Ok(json!({})));
    }

    /// A 400 is what a request gets for being wrong in any way, and the caller
    /// acts on a rejection for ever: it Quarantines the Account, which only a
    /// browser login clears. So the status alone is not enough — a proxy's
    /// error page, or a request Anthropic changed its mind about the shape of,
    /// would otherwise Quarantine every Account in a Group in one pass of
    /// `perch list <group> --refresh`.
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
                matches!(refused(body), Err(Refused::Failed(400))),
                "a Quarantine is for ever, and this is not the endpoint asking \
                 for one: {body}"
            );
        }
    }

    /// The same rule, over every status the token endpoint refuses with rather
    /// than only the 400 Anthropic sends today.
    ///
    /// Which status a refusal arrives under is not Perch's to decide, and the
    /// cost of guessing wrong is terminal: RFC 6749 §5.2 gives `invalid_client`
    /// a 401, and `invalid_client` is about the `client_id` in the request —
    /// Perch's own, the same in every renewal it sends. Read as "this Account's
    /// refresh token is retired", one such 401 Quarantines every Account in a
    /// Group in a single `perch list <group> --refresh`, and only a login for
    /// each clears it.
    #[test]
    fn a_renewal_refused_without_saying_the_token_is_retired_is_never_terminal() {
        let refused = |status: u16, body: &str| {
            understand(
                HttpResponse {
                    status,
                    body: body.to_string(),
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

    /// The read endpoints are the mirror, and must stay that way: there the
    /// bearer token really is the Account's Credential, so a 401 needs no body
    /// to corroborate it.
    #[test]
    fn a_read_endpoint_still_rejects_on_the_status_alone() {
        let reply = |status: u16| HttpResponse {
            status,
            body: String::new(),
        };

        assert_eq!(understand(reply(401), &[]), Err(Refused::Rejected));
        assert_eq!(understand(reply(403), &[]), Err(Refused::Rejected));
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

        // A number JSON holds as a float is the same lifetime spelled another
        // way, and `as_i64` answers `None` for every one of them. Read as no
        // lifetime at all, what is stored carries no `expiresAt` — which
        // `usable_at` takes at its word for ever, so every later reading of that
        // Account goes the long way round and is refused by Anthropic before
        // anything renews it.
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
}
