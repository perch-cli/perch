# Codex owns its renewal

Implemented as an experimental app-server adapter. Offline protocol and
synthetic Holdings tests cover the adapter; live Renewal, Workspace selection,
and quota attribution still require validation.

Perch uses Codex's documented app-server interface for account and Utilization
reads, with Codex managing Credential Renewal and persistence. Perch owns the
choice of Account and Profile, coordination of access, and the timestamped
figures it displays. It does not add a competing direct OAuth renewal path.

This qualifies the client-reading rule in ADR a-window-comes-from-limits for
Codex. A structured account protocol avoids parsing a terminal panel, but a
request for figures can still renew a Credential. Calling it a read does not
make it safe beside a running client.

## Ownership before freshness

A parked Profile may be opened for managed observation only with exclusive
access against other Perch operations. Perch must establish how it detects an
external client using that Profile before claiming the access is exclusive.

An isolated Run keeps its chosen Account. Perch must not launch a second managed
credential writer against its Live Profile without demonstrated coordination
(ADR a-profile-is-live-by-evidence). Until that coordination is established,
Perch declines the fresh read, retains any cached figures with their age, and
explains the restriction. Without a prior reading, Utilization is unknown.

Perch does not turn failure into a fresh reading. Account or workspace mismatch,
an incomplete applicable quota reading, and an unrecognized limit state cannot
silently become available Headroom. Preserve the last complete attributed
reading; do not rank unknown capacity as unused capacity.

## Evidence required before support is claimed

Use a pinned supported Codex release to establish:

- Which files or stores a managed account or quota request changes, including
  token expiry, rejection, process interruption, and an unavailable store.
- How exclusive observation, a running client, and concurrent requests avoid
  competing Renewal and lost Credential writes.
- How each reading is attributed to the intended Account and workspace, and
  how all applicable quota buckets, absent fields, and credit limits are handled.
- How cached reads, request coalescing, throttling, and back-off bound the cost
  of observation. Anthropic's polling allowance is not an OpenAI allowance.

These are validation requirements, not permission to exercise live Credentials.
Failed coordination narrows fresh-read availability; it does not authorize
falling back silently to direct service calls. Unattended Codex Switching keeps
its separate live-client evidence requirement.

## What is not chosen

Direct HTTP calls offer control over when Perch writes, but also make Perch
responsible for unpublished endpoint shapes and a second renewal implementation.
They remain an alternative to reconsider if the documented interface cannot
meet the required behavior, not an automatic fallback.

The evidence is captured in
[Codex account lifecycle feasibility](https://github.com/perch-cli/perch/blob/ebc4dc0/docs/research/codex-account-lifecycle.md)
and [Codex utilization and quota boundaries](https://github.com/perch-cli/perch/blob/fde2ba4/docs/research/codex-utilization.md).
