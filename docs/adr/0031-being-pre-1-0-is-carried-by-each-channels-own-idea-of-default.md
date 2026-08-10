# Being pre-1.0 is carried by each Channel's own idea of default

Perch is published before it is finished. Every Release is real and installable;
none of them should be what somebody arrives at without meaning to. There is no
single switch for that, because each Channel means something different by "the
default", so it is carried in each Channel's own terms rather than by one
mechanism bolted across all of them.

**npm**: published with `--tag dev`. `latest` points at nothing, so
`npm install perch-cli` fails loudly rather than quietly handing somebody a CLI
that is not ready. `release.yml` asserts this after every publish, because a
`latest` that has quietly moved looks exactly like one that has not until
somebody installs it.

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

Graduating to the default Channel and tagging `1.0.0` are the same act. Two
decisions would allow a state where a stable-looking version sits on a dev tag,
or a `1.0.0` nobody can install without opting in; one decision cannot.

The floor for taking it is the weekly contract suite green on all three
platforms for four consecutive weeks. Not an arbitrary interval: those tests
assert Perch's beliefs against a Claude Code that updates continuously (ADR
0007), and drift there is the failure that cannot be tested for in advance, only
waited out. Everything else is judgement.
