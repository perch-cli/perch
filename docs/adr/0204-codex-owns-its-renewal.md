# Codex owns its renewal

Perch does not hold a second OAuth implementation for Codex. Account and
Utilization reads go through the `codex` binary's own `app-server` interface,
and Codex renews and persists the Credential as it would for a person at a
terminal. What Perch owns is which Account and which Profile the question is
asked about, exclusive access while it is asked, and the timestamped figures it
displays afterwards.

That qualifies ADR a-window-comes-from-limits' rule for Codex alone. A
structured account protocol is not a terminal panel being scraped, but a request
for figures can still renew a Credential, and calling it a read does not make it
safe beside a running client.

Codex support is experimental. The adapter is covered by offline protocol tests
and synthetic Holdings; a pinned live client is what the remaining assumptions
still want, and they are named at the end.

## Ownership before freshness

An observation opens a parked Profile only with exclusive access, and takes it
rather than assuming it. Perch asks whether anything is running against that
directory (ADR a-profile-is-live-by-evidence) and declines where something is,
keeping the cached figures with their age and saying why. It then writes its own
Marker into the Profile for the length of the exchange, so a Run or a Switch
starting mid-read meets the same refusal from the other side. Without a prior
reading there is nothing to keep, and Utilization is unknown rather than zero.

The exchange is four messages — `initialize`, `initialized`, `account/read` with
`refreshToken` false, `account/rateLimits/read` — under a thirty-second
deadline, with cancellation checked while a child read or write is pending and
the child cleaned up on every way out. The store and the login method are forced
on the command line rather than read from the home being observed, so a
`config.toml` cannot redirect the exchange to a Credential Perch did not check.

An isolated Run keeps the Account it was launched with. Perch does not put a
second credential-writing client on a Live Profile to get a fresher number.

## A figure is attributed or it is not recorded

The Credential at the home about to be read is checked to be this Account's
before the exchange and again after it, by the identity its own document
carries. The active Account is read at the Default home, because that is the
copy Codex renews; a parked Account is read in its Profile. An answer that does
not name a subscription-backed Account is refused, and so is a quota state that
is not a percentage — credits, a spend control reached — because unknown
capacity ranked as unused capacity is the one error this band exists to prevent
(ADR headroom-is-the-worst-window). A refusal keeps the last complete reading;
it never becomes Headroom.

Each bucket the reply names keeps its own identity, as `<limit>/<period>/<span>`
rather than being mapped onto Claude's window names. Two buckets that meter
differently are two windows, and a name that claimed otherwise would rank them
as one.

## What is not chosen

**Direct HTTP calls.** They would give Perch control over exactly when it
writes, at the price of owning an unpublished endpoint shape and a second
renewal implementation — the thing this decision exists to avoid. They remain
the alternative if the documented interface cannot meet the behavior required
here, rather than a fallback reached silently when an exchange fails.

## What a pinned client is still wanted for

The protocol half is checked already: the gated suite sends the installed
`codex` the four documents an observation sends, and asks whether the store
setting a Switch pins still names the `file` variant
(ADR a-suite-is-named-and-gated). What that cannot reach is everything behind a
login.

- Which files a managed account or quota request changes, across an expired
  token, a rejected one, an interrupted process and an unavailable store.
- That exclusive observation, a running client and concurrent requests do not
  produce competing Renewals or a lost Credential write.
- What the cost of observation is over time, since Anthropic's polling allowance
  is not an OpenAI allowance and the Watcher currently paces both from the
  former (ADR a-watcher-knob-is-arithmetic).

The evidence gathered so far is in
[Codex account lifecycle feasibility](https://github.com/perch-cli/perch/blob/ebc4dc0/docs/research/codex-account-lifecycle.md)
and [Codex utilization and quota boundaries](https://github.com/perch-cli/perch/blob/fde2ba4/docs/research/codex-utilization.md).
