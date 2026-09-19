# Security Policy

## What Perch is for

One person moving between logins they already hold — their own accounts, on
their own machine. Perch creates no accounts, authenticates nobody, and
carries no credential it was not handed by a login the person made themselves.
It is not a way to share one subscription between people, and nothing in it is
built for that.

## Verifying what you installed

Every release is built by a public workflow in this repository, and every
archive carries signed build provenance. If you have `gh`:

```sh
gh attestation verify perch-v<version>-<target>.tar.gz --repo perch-cli/perch
```

That says which workflow, in which repository, at which commit produced the
exact file you are holding. The `SHA256SUMS` on the release says which bytes,
which is a weaker claim — it is fetched from the same place as the archive, so
it proves only that the two agree. Both installers check the checksum always
and the provenance whenever `gh` is installed and logged in, and refuse to
install when that check fails.

Perch is unsigned on macOS and Windows: no Apple notarization, no Authenticode.
The build provenance above stands in for it, and it is the stronger claim about
where a file came from. A signature says only that somebody passed a certificate
authority's identity check.

What it does not do is satisfy Gatekeeper or SmartScreen. An archive downloaded
from the releases page in a browser will warn before it runs. One fetched by
either installer, by Homebrew or by npm will not, because none of those marks
the file as downloaded. Signing is not currently planned.

## Reporting a vulnerability

Report privately through GitHub's
[private vulnerability reporting](https://github.com/perch-cli/perch/security/advisories/new).
Please do not open a public issue for a security problem.

Expect an acknowledgment within a week. Perch is maintained by one person, so
a fix may take longer than that — the acknowledgment will say where it stands.

## What is in scope

Perch holds a provider's credentials wherever that provider's installed client
keeps one. For Claude Code that is the macOS keychain, or a file inside a
profile directory on Linux and Windows; for Codex it is a file inside the
profile directory on every platform. Both stores are in scope, and the file
store is the one with the least behind it:

- Reading, writing, or deleting keychain entries that are not Perch's own.
- Anything that writes a credential to disk, a log, the terminal, or `argv`.
- A credential file created or left at permissions others can read.
- Profile adoption or switching that hands one account's credentials to another.
- The `security` binary invocations, and how their arguments are constructed.
- The `curl` invocations that carry an `Authorization` header, and anything
  that could redirect or intercept one.
- The `codex app-server` exchanges Perch drives, the environment it hands that
  process, and anything that could point one at a credential it did not check.
- `perch holdings export` and `perch holdings import`: the passphrase prompt,
  the encryption, and anything that could leave plaintext behind on either side.

## What is not

- Anything requiring an attacker who already has your unlocked login keychain.
  At that point the credentials are readable without Perch.
- Anything requiring an attacker who can already run arbitrary code as your user.
- The plaintext credential file itself, on a platform or a provider whose
  client uses one. Perch narrows it to you alone and says so when it has to, but
  where a client puts a credential is that client's decision, not Perch's.
