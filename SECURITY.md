# Security Policy

## Reporting a vulnerability

Report privately through GitHub's
[private vulnerability reporting](https://github.com/mschieller/perch/security/advisories/new).
Please do not open a public issue for a security problem.

Expect an acknowledgement within a week. Perch is maintained by one person, so
a fix may take longer than that — the acknowledgement will say where it stands.

## What is in scope

Perch holds Claude Code credentials wherever the installed Claude Code keeps
one — the macOS keychain, or a file inside a profile directory on Linux and
Windows (ADR 0020). Both stores are in scope, and the file store is the one
with the least behind it:

- Reading, writing, or deleting keychain entries that are not Perch's own.
- Anything that writes a credential to disk, a log, the terminal, or `argv`.
- A credential file created or left at permissions others can read.
- Profile adoption or switching that hands one account's credentials to another.
- The `security` binary invocations, and how their arguments are constructed.
- The `curl` invocations that carry an `Authorization` header, and anything
  that could redirect or intercept one.
- `perch export` and `perch import`: the passphrase prompt, the encryption, and
  anything that could leave plaintext behind on either side.

## What is not

- Anything requiring an attacker who already has your unlocked login keychain.
  At that point the credentials are readable without Perch.
- Anything requiring an attacker who can already run arbitrary code as your user.
- The plaintext credential file itself, on a platform whose Claude Code uses
  one. Perch narrows it to you alone and says so when it has to, but where
  Claude Code puts a credential is Claude Code's decision, not Perch's.
