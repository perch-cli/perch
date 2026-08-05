# A Credential lives wherever Claude Code puts it

Perch has treated the Credential Store as a keychain: `/usr/bin/security` under
a service name derived from the Profile's path (ADR 0008), recorded against each
Account in `registry.json`. That is only true on macOS. The Windows and Linux
2.1.222 builds keep a Credential as plaintext JSON at
`<config dir>/.credentials.json`, in the same `claudeAiOauth` shape, written
with mode 0600 — read out of the shipped binary, not inferred.

Perch stores a Credential exactly where the installed Claude Code would store
it, and nowhere else. The Credential Store therefore becomes a concept with two
implementations rather than a keychain with a name: the keychain, and a file.

Claude Code is a composite reader — a primary store with a fallback — so Perch
is too, and the two halves of that are decided by different things:

- **The platform chooses where Perch writes.** A Profile that `perch add` has
  just created holds no evidence about anything; the only question that can be
  answered about it is which store the Claude Code on this machine would use.
  On macOS that is the keychain, and everywhere else the file.
- **Evidence chooses what Perch believes.** An existing store can be looked at.
  A Credential found in the store that is not the primary is a Credential, and
  reading it is not a compromise: it is what the Claude Code beside it is doing.

That second half fixes a live macOS bug. A locked keychain today produces exit
11, or "no login", about a machine whose Claude Code is working perfectly off
its plaintext fallback. The composite reader consults the other store when the
primary yields nothing — whether because the item is absent or because the
keychain would not answer — and reports the primary's failure only when neither
store holds anything, which is when that failure is the whole story.

**A write removes the copy in the store it did not write to.** This is the part
with teeth. A Credential left behind is several Rotations past its retirement
within minutes, and it would arm the composite reader to hand a dead refresh
token to Claude Code the next time it consulted that store — ADR 0006's silent
poisoning, arriving by the back door. The copy is removed by the write that
superseded it, not swept up later.

The rule is symmetric, and the direction that is easy to overlook is the one
that matters more. A write that had to fall back leaves the *primary* holding
the superseded copy — and the primary is what a reader consults first, so that
copy would beat the Credential just stored. Removing it is safe at that point
and not before, because by then the new one has been written somewhere and read
back.

Because the store is derived from the Profile's path rather than recorded,
`registry.json` stops carrying `keychain_service`, `keychain_account` and `dir`.
All three were restatements of the Account's email address run through the
derivations in `probe`. A registry that records them can disagree with those
derivations, and a registry that cannot say where a Credential is cannot be
wrong about it. `Profile` collapses into values the registry already keys on,
and the file becomes version 2; a version 1 registry is read by dropping the
three fields.

## Consequences

Off macOS this multiplies plaintext secrets on disk from one to N: every Account
Perch holds has its refresh token in a file, where before only the active one
did. That is the cost of storing a Credential where Claude Code will find it,
and it is recorded here rather than mitigated. Mitigating it would mean putting
a Credential somewhere Claude Code does not read — which is the one thing Perch
cannot do and still be a switcher.

A file holding a Credential is created 0600 inside a directory created 0700,
with the mode set at creation. Not by a `chmod` afterwards: between the two
calls the secret is on disk and readable, and that window is the whole of what
the mode is for. That has a consequence worth stating, because it is easy to
write code that reads as though it did not: a directory that already exists
keeps the mode it has, so the *first* thing to make a Profile's directory has to
be the thing that makes it private. A file found looser than 0600 is tightened
and reported once rather than refused — Perch did not necessarily write it,
refusing would strand a machine whose Claude Code is working, and a tightened
file is a better outcome than an explained one.

The keychain remains the fallback off macOS and the primary on it, so there is
one code path rather than two. On Linux and Windows it simply never holds
anything, and a keychain that cannot be reached is a store that holds nothing
rather than a failure: `/usr/bin/security` not existing is not news on Linux.

The whole non-macOS design rests on one assumption — that the plaintext store
honours `CLAUDE_CONFIG_DIR`, which is what gives each Profile a Credential of
its own, exactly as the derived service name does on macOS (ADR 0001). It is one
assumption rather than several because the same reading of the config directory
produces both. A contract test asserts it (ADR 0007).

Two neighbouring env vars were found beside these paths and neither is acted on.
`CLAUDE_SECURESTORAGE_CONFIG_DIR` overrides the config directory for the
credential store alone, in both derivations, so a machine that sets it would put
every Profile's Credential in one place; nothing sets it, and Perch would rather
refuse to guess than encode a second config path. `CLAUDE_CODE_HOST_CREDS_FILE`
names a credentials file supplied by whatever is hosting Claude Code, ignored by
it when group- or other-readable — a path into Claude Code, not one out of it,
and not a store Perch writes.

If a build turns out to have a Secret Service primary — a distribution's own, or
a later Claude Code — that is a third backend to write. The composite reader
accommodates one without redesign: what changes is which store the platform
names as primary.
