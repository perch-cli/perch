# Security Policy

## Reporting a vulnerability

Report privately through GitHub's
[private vulnerability reporting](https://github.com/mschieller/perch/security/advisories/new).
Please do not open a public issue for a security problem.

Expect an acknowledgement within a week. Perch is maintained by one person, so
a fix may take longer than that — the acknowledgement will say where it stands.

## What is in scope

Perch handles Claude Code credentials in the macOS keychain, so the parts worth
looking hardest at are:

- Reading, writing, or deleting keychain entries that are not Perch's own.
- Anything that writes a credential to disk, a log, or the terminal.
- Profile adoption or switching that hands one account's credentials to another.
- The `security` binary invocations, and how their arguments are constructed.

## What is not

- Anything requiring an attacker who already has your unlocked login keychain.
  At that point the credentials are readable without Perch.
- Anything requiring an attacker who can already run arbitrary code as your user.
