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
incidental: they are most of the decisions this document makes below and most of
ADR a-linux-build-is-static, and each one would have been settled by whatever the
generator emitted rather than by anybody here.

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
is answered where it arises rather than accepted: every pull request
shellchecks the shell installer, parses the PowerShell one, runs the real
`install.sh` against a fabricated Release and asserts on the advice it prints,
and runs the real npm build against fabricated binaries and asserts on the six
packages it wrote. The formula generator reads every checksum before it writes
anything, so it cannot emit half a formula. Nothing else before a tag is pushed
runs any of them, and the way the npm build fails there is a package page that
publishes blank.

What owning the npm package buys, beyond the bytes, is its signal behavior, and
that is the part a generator has no opinion about. The wrapper is a process
between the terminal and Perch, so a `SIGINT` the terminal delivers reaches both
of them — and Node's default action would kill the wrapper while the client
`perch run` launched is still restoring the terminal. So the wrapper ignores it
and lets Perch act on it alone. A signal *directed* at the wrapper's own pid is
the other case, and reaches nothing else: `timeout 30 npx perch watcher run`, a
runner canceling a job, a terminal closing on a detached shell. Ignored there
too, the pair would run until Perch stopped on its own and `timeout` would be
defeated, so those are passed on.

The second cost is the one that is not paid back. A tool absorbs upstream change
— a new npm resolution rule, a Homebrew formula deprecation — and this does not.
When one of those lands it lands here, as a broken release.

## One owner per Artifact

release-plz decides the version and writes the changelog and stops at the tag;
`release.yml` owns every file. That is why the GitHub Release is created as a
draft and published only once the archives, the checksums and the attestations
are all on it — there is no moment when the page exists and the files do not,
which is not a property a two-owner arrangement can have.

The tag is release-plz's last act and the only thing it hands over, and it has to
be pushed with a token that is not `GITHUB_TOKEN`. GitHub refuses to start a
workflow from a push its own token made, so a tag pushed with it would sit on
main with nothing watching it, and a release pull request opened with it would
never run the CI that `release.yml` insists on having seen go green.

That insistence is the other half of one owner. A release builds rather than
tests, because CI has already tested the commit being tagged — but release-plz
tags the moment the release pull request merges, which is usually before CI on
main has finished, so a release waits for that run rather than assuming it. And
a release job takes no build cache: caching saves minutes on a job that runs once
per Release, and it does it by reusing artifacts from an earlier run in the binary
people are about to install.

Revisiting this is cheap and stays cheap: the thing that would replace these
workflows consumes the same tag and produces the same Artifacts.

## crates.io is not one of Perch's Channels

Perch is written in Rust and is not published to crates.io. `publish = false` in
`release-plz.toml` is what enforces it — not in `Cargo.toml`, and the difference
is not cosmetic. release-plz reads that field in a manifest as "this package is
not one of mine" and skips the package entirely: no version decision, no
changelog, no release pull request, and no error saying why. The workflow goes
green on every push to main and does nothing.

The name is taken — `perch` there is a Mastodon and Bluesky client, actively
published — but that is the smaller reason and it would be answered by
publishing as `perch-cli`.

The real one is that crates.io is a Channel for libraries. What it distributes is
source, and what installing from it means is `cargo install`: every user compiles
Perch, which requires a Rust toolchain they may not have and takes minutes they
should not spend. Every other Channel hands them a binary that already exists.
Nothing depends on Perch as a crate — the `[lib]` exists so the binary and the
tests can share code, not because anything downstream links against it — so a
crates.io entry would advertise an API nobody consumes and freeze it by
advertising it. `cargo install --git` still works for anyone who wants the source
path, which is the whole of what a publication would have bought.

Two consequences follow, and neither is optional.

The `[lib]` has no public API promise, so release-plz's semver check is switched
off: it would spend minutes every Release verifying a commitment nobody relies
on.

And release-plz still has to be told where the previous version is. Refusing the
upload does not stop it asking crates.io what the last released version of a
crate named `perch` was, because that is how it decides which commits are new —
and the `perch` on crates.io is somebody else's live crate. Left to ask, it
downloads that crate on every push to main and diffs Perch against it, which
concretely means proposing a version bump with a changelog entry about
dependencies that are not Perch's. `git_only` says the previous version is the
newest tag, which for a project that is never published is the only place it was
ever recorded; it costs a `cargo package` against a worktree at that tag and
touches no network. `dependencies_update = false` does not do this job —
it governs whether release-plz updates `Cargo.lock`, not whether it counts a
difference against somebody else's crate.

Nothing stops a person typing `cargo publish` by hand, where `Cargo.toml` used
to. That is accepted rather than worked around: the name is taken by a live
crate, so the command fails on its own, and no automation runs it.

## No Channel is distorted to say Perch is unfinished

Every Release is real and installable, and each Channel serves the newest one the
way that Channel serves anything.

**GitHub Releases**: published normally, and *not* marked as a prerelease. It
genuinely is the latest Release, and marking it otherwise would empty the
`releases/latest` endpoint that both installers and `perch upgrade` ask, which
would break the Channel in order to decorate it. The Release page is the source
of truth every other Channel points at, and a source of truth that lies about
which version is newest is worse than an unadorned one.

**npm**: published to `latest`, the default tag. This is not a state npm leaves
open. The registry attaches `latest` to a package's first version whatever
`--tag` said, and then refuses to remove it — the DELETE comes back `400` after
authenticating, and no public package on the registry is without one. So the
choice was never between "`npm install perch-cli` fails" and "it works". It was
between "it works" and "it silently installs whichever version `latest` happened
to be stuck on, forever". `release.yml` asserts that `latest` followed the
Release it just published, by reading it back from the registry rather than by
comparing the tag against the version being published, which is a check that
cannot fail.

That read is polled rather than taken once, because the write and the read do not
reach the same place: `npm publish` returns when the registry accepts it, and
`npm view` is served from a CDN that can be minutes behind. Asking immediately
asks the wrong replica, and the answer it gives is the previous Release — which
reads exactly like a publish that failed to move the tag.

**Homebrew**: a Tap. `brew tap perch-cli/perch` is a deliberate act, so no second
formula is needed and Homebrew has no channel concept worth fighting for this.
The Tap stays a Tap until there is a reason to consider homebrew-core.

What being 0.x means is carried by `CHANGELOG.md` and by what a break costs the
person it lands on (ADR the-holdings-outlive-a-perch), not by a Channel behaving
unlike every other package on it. Only one of the three Channels is opt-in in any
case, and the versionless one-liner on the site is not: it hands the newest
Release to anyone who pastes it, and always did.

## The floor for 1.0

The weekly contract suite green on all three platforms for four consecutive
weeks. Not an arbitrary interval: those tests assert Perch's beliefs against a
Claude Code that updates continuously (ADR an-assumption-is-probed), and drift
there is the failure that cannot be tested for in advance, only waited out.
Everything else is judgment, and Apple notarization is the thing worth
reconsidering at the same time.
