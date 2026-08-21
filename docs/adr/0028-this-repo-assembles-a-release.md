# This repo assembles a Release

There is a tool for this. `dist` builds a target matrix, uploads a GitHub
Release, and generates the shell installer, the PowerShell installer, the
Homebrew formula and the npm package from a single block of configuration —
which is very nearly the exact list Perch needs. It was not taken.

## What the tool would have decided

Everything downstream of the archives. The name and shape of every Artifact,
what an installer does when a checksum fails, whether the npm package downloads
its binary in a postinstall script or declares one optional dependency per
Target, whether the Homebrew formula builds from source. Those are not
incidental: they are most of the decisions in ADR a-linux-build-is-static and
ADR this-repo-assembles-a-release, and each one would have been settled by
whatever the generator emitted rather than by anybody here.

The concrete cost of that is visible in two places. A generated npm package
fetches its binary at install time, which stops working under `npm ci
--ignore-scripts`, behind a proxy, and offline — and for a program that holds
credentials, "npm has the bytes" is a materially different claim from "npm ran
a script that fetched something". And a generated installer verifies a checksum
but has no opinion about signed provenance, which is the check that actually
distinguishes an Artifact this repository built from one somebody else made.

## What owning it costs

Four packaging formats to write and keep working, and they are the parts of
Perch that are not Rust, so nothing else in CI would notice them breaking. That
is answered where it arises rather than accepted: the installers are
shellchecked and parsed on every pull request, the formula generator refuses to
emit half a formula, and the npm shim was verified against the real binary
including the signal path a TUI depends on.

The second cost is the one that is not paid back. A tool absorbs upstream change
— a new npm resolution rule, a Homebrew formula deprecation — and this does not.
When one of those lands it lands here, as a broken release.

## What is kept

The pipeline has one owner per Artifact. release-plz decides the version and
writes the changelog and stops at the tag; `release.yml` owns every file. That
is why the GitHub Release is created as a draft and published only once the
archives, the checksums and the attestations are all on it — there is no moment
when the page exists and the files do not, which is not a property a two-owner
arrangement can have.

Revisiting this is cheap and stays cheap: the thing that would replace these
workflows consumes the same tag and produces the same Artifacts.
