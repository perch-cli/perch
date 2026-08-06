# Reconcile shares by denylist, and never copies

A Run launches a client against a Profile that is a live configuration
directory rather than storage (ADR 0010), so every piece of Shared State has to
be reachable from it. Two questions fall out: which entries cross, and by what
mechanism.

## Which entries cross

Perch shares **everything in the Default Profile except `.credentials.json` and
`.claude.json`**, which are the two Account-scoped things: the Credential
itself, and the file holding `oauthAccount`.

The alternative is an allowlist naming what is shared. It is more precise and
it goes stale on Claude Code's release schedule rather than Perch's. When this
was written the glossary named four pieces of Shared State — memory, settings,
plugins, past work — and the installed Claude Code's configuration directory
held twenty-one entries, among them `sessions`, `tasks`, `plans`,
`file-history` and `session-env`, none of which existed when the sentence was
first written. Under an allowlist a person's plans and sessions silently fail
to follow them into a Run, which reads as Perch losing their work.

The denylist fails the other way: a genuinely new Account-scoped file would be
shared until somebody noticed. That is rarer — it needs Anthropic to invent
one — and it is the kind of assumption Perch already probes rather than
believes (ADR 0007).

## By what mechanism

By link, always. `CONTEXT.md` used to promise a fallback to copies "on
platforms that cannot link"; that fallback is withdrawn here. A copy is not a
degraded share, it is a different feature: edit `CLAUDE.md` inside a Run and
the real one silently does not have it, which inverts the one thing Shared
State promises.

Windows is the platform the fallback existed for, and it does not need one.
Directory **junctions** and file **hardlinks** both work without Developer Mode
and without elevation; only symlinks require either. Where even that fails, the
Run is refused with a message naming what could not be linked. A refusal is
recoverable. A silently diverged copy of somebody's memory is not.

## Consequences

Reconcile has to enumerate the Default Profile at Run time rather than consult
a list in Perch's source, so a Claude Code release that adds a directory needs
no change here.

`.claude.json` cannot be linked, because it holds identity that must stay
per-Profile. It is therefore the one file handled key by key, which ADR 0003
already began and its amendment continues.

Refusing rather than copying makes the Run path the only one that can fail for
a filesystem reason. That failure has to name the entry and the reason, because
the user's fix — enabling Developer Mode, or moving the Profile off a
filesystem that carries no links — is not one Perch can apply for them.
