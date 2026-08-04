# Keychain access shells out to /usr/bin/security rather than using a Rust crate

macOS anchors a keychain item's access control to the binary that created it.
The idiomatic Rust approach — the `keyring` or `security-framework` crate,
in-process and dependency-free — would make each Perch build a different
creator, so a Homebrew upgrade, a `cargo install` rebuild, or a fresh binary
from the install script could turn a silent read into a modal prompt or a
denial. This is a known failure mode for any tool whose installer replaces the
binary that owns its keychain items, and it surfaces only after release.

Perch therefore drives `/usr/bin/security`, as Claude Code does. That binary
never changes, so creator == reader across upgrades of both tools — which is
also what lets Perch read the item Claude Code wrote in the first place.

## Consequences

Part of the case for Rust was avoiding interpreter startup, and the credential
path spawns a subprocess anyway. The hot path is `perch status` reading cached
utilization (ADR 0015), which touches no keychain at all and still benefits;
the credential path will not be where Rust pays off.

Four constraints come with the `security` CLI. Writes hex-encode with `-X` and
pipe through `-i` so secrets never reach `argv`. Its stdin buffer is 4096 bytes
and overflow truncates mid-argument, silently corrupting the entry — Claude Code
issue #30337 — so writes near the limit must fall back to argv. Exit code 44
means "not found"; every other non-zero exit means locked, denied, or
unavailable and must be reported differently, since conflating them reads as an
account having vanished. And `-w` returns hex for non-printable data, so the
wrapper is safe for ASCII JSON credentials only.
