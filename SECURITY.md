# Security Policy

## What Perch is for

One person moving between Claude logins they already hold — their own accounts,
on their own machine. Perch creates no accounts, authenticates nobody, and
carries no credential it was not handed by a login the person made themselves.
It is not a way to share one subscription between people, and nothing in it is
built for that.

## Verifying what you installed

Every release is built by a public workflow in this repository, and every
archive carries signed build provenance. If you have `gh`:

```sh
gh attestation verify perch-v0.1.0-<target>.tar.gz --repo perch-cli/perch
```

That says which workflow, in which repository, at which commit produced the
exact file you are holding. The `SHA256SUMS` on the release says which bytes,
which is a weaker claim — it is fetched from the same place as the archive, so
it proves only that the two agree. Both installers check the checksum always
and the provenance whenever `gh` is installed and logged in, and refuse to
install when that check fails.

Perch is unsigned on macOS and Windows before 1.0: no Apple notarization, no
Authenticode. The provenance above is what stands in for it, and notarization
is on the list for 1.0.

## Reporting a vulnerability

Report privately through GitHub's
[private vulnerability reporting](https://github.com/perch-cli/perch/security/advisories/new).
Please do not open a public issue for a security problem.

Expect an acknowledgment within a week. Perch is maintained by one person, so
a fix may take longer than that — the acknowledgment will say where it stands.

## What is in scope

Perch holds Claude Code credentials wherever the installed Claude Code keeps
one — the macOS keychain, or a file inside a profile directory on Linux and
Windows. Both stores are in scope, and the file store is the one with the least
behind it:

- Reading, writing, or deleting keychain entries that are not Perch's own.
- Anything that writes a credential to disk, a log, the terminal, or `argv`.
- A credential file created or left at permissions others can read.
- Profile adoption or switching that hands one account's credentials to another.
- The `security` binary invocations, and how their arguments are constructed.
- The `curl` invocations that carry an `Authorization` header, and anything
  that could redirect or intercept one.
- `perch holdings export` and `perch holdings import`: the passphrase prompt,
  the encryption, and anything that could leave plaintext behind on either side.

## What is not

- Anything requiring an attacker who already has your unlocked login keychain.
  At that point the credentials are readable without Perch.
- Anything requiring an attacker who can already run arbitrary code as your user.
- The plaintext credential file itself, on a platform whose Claude Code uses
  one. Perch narrows it to you alone and says so when it has to, but where
  Claude Code puts a credential is Claude Code's decision, not Perch's.
