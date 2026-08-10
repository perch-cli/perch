# Being pre-1.0 is carried by each Channel's own idea of default

Perch is published before it is finished. Every Release is real and installable;
none of them should be what somebody arrives at without meaning to. There is no
single switch for that, because each Channel means something different by "the
default", so it is carried in each Channel's own terms rather than by one
mechanism bolted across all of them — and one of those Channels turns out to
have no such term at all.

**npm**: published to `latest`, the default tag, during 0.x as well.

This was `--tag dev` with `latest` pointing at nothing, and that is not a state
npm allows a package to be in. The registry attaches `latest` to a package's
first version whatever `--tag` said, and then refuses to remove it — the DELETE
comes back `400` after authenticating, and no public package on the registry is
without one. So the choice was never between "`npm install perch-cli` fails" and
"it works". It was between "it works" and "it silently installs whichever
version `latest` happened to be stuck on, forever" — which is the same install,
with a worse version, and a check in `release.yml` reporting success the whole
time because it only compared `latest` against the version being published.

What is left is honest: `latest` follows the newest Release, and `release.yml`
asserts that it does. Being pre-1.0 is said where people read — the README, the
version number itself — rather than carried by making one Channel behave
unlike every other package on it.

**Homebrew**: a Tap. `brew tap perch-cli/perch` is already a deliberate act
nobody performs by accident, so the Channel is the opt-in and no second formula
is needed. Homebrew has no channel concept worth fighting for this.

**GitHub Releases**: published normally, and *not* marked as a prerelease. This
is the one that reads as inconsistent and is not. It genuinely is the latest
Release; marking it otherwise would empty the `releases/latest` endpoint that
both installers ask, which would break the Channel in order to decorate it. The
Release page is the source of truth every other Channel points at, and a source
of truth that lies about which version is newest is worse than an unadorned one.

## What ends it

Tagging `1.0.0` is the whole of it. The Homebrew Tap stays a Tap until there is
a reason to consider homebrew-core; nothing else changes shape, because nothing
else is holding a version back.

The floor for taking it is the weekly contract suite green on all three
platforms for four consecutive weeks. Not an arbitrary interval: those tests
assert Perch's beliefs against a Claude Code that updates continuously (ADR
0007), and drift there is the failure that cannot be tested for in advance, only
waited out. Everything else is judgement.
