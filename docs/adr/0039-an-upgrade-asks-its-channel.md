# An Upgrade asks its Channel

Supersedes ADR an-upgrade-asks-its-channel, which said Perch does not look for
its own updates. It now does, in two places and on purpose, and the title of
that ADR is no longer true. What survives of its argument is set out at the end,
because most of it does.

`perch upgrade` replaces this machine's Installation with a newer Release. It
takes `--release` to name one, and without it takes the newest.

## It routes rather than overwrites

The obvious implementation is the wrong one. Perch would find its own binary,
download a Release, and write over itself — and on two of the four Channels
that is actively broken rather than merely impolite.

Under Homebrew the binary lives in a Cellar that `brew` owns and keeps a receipt
for. Overwriting it makes the receipt a lie, and the next `brew upgrade` reverts
the machine to whatever the formula says without mentioning that it did.

Under npm the binary lives in a platform package that the `perch-cli` wrapper
depends on **by exact version**, which is the whole of how a release stays
coherent across six packages (`npm/build.mjs`). Overwriting it leaves a wrapper
declaring `0.2.0` and spawning a `0.3.0` binary, until the next `npm install`
throws the change away.

So `perch upgrade` works out which Channel left this Installation and hands the
work back to it: `brew upgrade perch`, or `npm update -g perch-cli`. The command
is echoed before it runs, and its exit status is Perch's. Only the installer
Channel — the one where nothing else manages the binary — is replaced by Perch
itself.

That is the same line ADR perch-takes-back-what-it-wrote drew and the glossary
already carried: an Installation is what a Channel left, and taking one back
belongs to the Channel that made it. Replacing one is the same kind of act as
removing one, and it was never going to be true that Perch could remove an
Installation only by asking the Channel but replace one behind its back.

**npm on Windows is the exception that proves it is the right shape.** There the
routed command cannot work at all: `npm update -g` would be replacing
`perch.exe` while it is the running process, and Windows holds the file open. So
that one case prints the command instead of running it. A design that
self-replaced everywhere would have had no way to notice.

## How Perch knows which Channel

By the path of its own executable, and by nothing else, because there is nothing
else available. Every Channel points at the same Release and installs the same
bytes, so a Channel stamped into the binary at build time would mean five builds
per Target instead of one — paying for the answer in the place that is hardest
to keep honest.

A `node_modules` segment is npm — checked first, because `brew install node`
puts an npm prefix *inside* a Homebrew one, and a path holding both is an npm
Installation of Perch under a Homebrew installation of Node. Reading that as
Homebrew's would run `brew upgrade perch` against a formula that is not
installed and report success having changed nothing.

A `Cellar` segment is Homebrew, and the `brew` to run is derived from the prefix
above it rather than found on `PATH`, for the reason
ADR a-crate-must-not-cost-a-seam gives: a program Perch shells out to is named
absolutely so nothing earlier on the path answers instead. A machine with two
Homebrew prefixes has two `brew`s, and only the one that owns this Installation
can replace it.

The installer's own directory is the installer, and that is asked for rather
than assumed: both installers take `PERCH_INSTALL_DIR` above everything, and
their defaults do not agree — `~/.local/bin` on Unix, `%LOCALAPPDATA%\Perch\bin`
on Windows. Hard-coding the Unix one read every Windows Installation as a binary
Perch had not placed, and refused to upgrade the only Channel it is able to
upgrade itself.

**Anything else refuses.** A binary somebody unpacked from the Release page into
`/usr/local/bin` looks exactly like an installer Installation and is not one,
and overwriting a file at a path Perch never wrote is the one irreversible thing
this command could do. So it says what it found and points at the installer.
`--channel` is there for the person who symlinked or relocated it, and is the
whole of the escape hatch.

## Perch does not download the Release; the installer does

`install.sh` and `install.ps1` already resolve a tag, download an Artifact,
verify it against `SHA256SUMS`, verify its provenance where `gh` is present,
and put the binary in place without a half-way state. They take `PERCH_VERSION`.
That is the entire job, written and tested (`packaging/pages/install-test.sh`).

Doing it again in Rust would cost more than it looks. `Host::http` answers with
a `String`, which cannot carry a tarball, so the seam and every fake behind it
would have to widen to bytes; there is no `tar`, gzip or zip crate in the build;
and the checksum and provenance checks would exist twice, in two languages, to
drift apart at the first fix applied to one of them.

So the installers are **embedded** with `include_str!`, written to a private
file under Perch's own directory, and run with `PERCH_VERSION` set. Not fetched
at run time: "Perch downloads a script from the internet and executes it" is a
sentence a program that holds Credentials should not have to defend, even though
it is the same script the install guide already tells people to pipe to `sh`.
The embedded copy is pinned to the build, which costs nothing — the only thing
passed into it is a tag.

It also happens to be why an upgrade does not ask about `PATH` again:
`install.ps1` was already written to be idempotent there, on the reasoning that
a re-install is how Perch gets updated.

**Windows needs one thing the installer did not have.** A running `perch.exe`
cannot be replaced, and `install.ps1` had a branch saying so — "is perch
running? Close it and try again" — which is exactly the state a self-upgrade is
in. Windows does permit *renaming* a running executable, so the embedded script
renames it to `perch.exe.old`, moves the new one into place, and clears the
stray — which fails while that binary is still the running one, so in practice
the next Upgrade is what removes it. The residue sits beside the binary, on the
Installation side of the line ADR perch-takes-back-what-it-wrote drew, which is
where a Channel's litter belongs. A failed move puts the working binary back,
because an upgrade that did not happen is a better outcome than a machine with
no Perch on it.

The alternative was a detached PowerShell that waits for Perch to exit and then
performs the move. It was refused: it does the one dangerous step at the moment
Perch has lost any means of reporting that it failed.

## What it says about what it checked

The installers verify checksums always and provenance only where `gh` is
installed, so on most machines the strongest check is skipped. `perch upgrade`
inherits that exactly rather than being stricter — one behavior for install and
upgrade, which is one behavior to keep right — but it *says* which checks it
made, including the one it did not. A silently skipped provenance check is the
single thing a tool built around being careful with Credentials should not do
quietly.

Requiring `gh` was the alternative. It makes a GitHub CLI a hard dependency of
staying current, which is more than this buys.

## Going backwards is allowed and is named

`--release` takes an older Release, because the alternative leaves somebody who
upgraded into a bad one with no route back but re-running the installer by hand.

It is not silent, though, because Perch has a guard that makes a downgrade
sharper than it looks: a registry written by a newer Perch is refused
(`registry.rs`, and CLAUDE.md's one forward-looking exception). So going back far
enough leaves a working machine with a binary that will not read its own
registry. The confirmation names that consequence rather than asking a bare
"are you sure", which is a question nobody has ever answered with information.

## What this gives up, which ADR an-upgrade-asks-its-channel was protecting

`perch --version` now makes a network request, and it did not before. That is
the cost, and it is a real one: 0032 named `perch --version` as the thing that
simply says what is installed.

What is *not* given up is the shape 0032 actually refused. There is no schedule,
no cache, no age to reason about and no staleness to explain — the machinery it
took ADR a-figure-carries-its-age to get right for Utilization is not built a
second time for this. The request happens when a human types the command, or not
at all:

- suppressed when stdout is not a terminal, so scripts, CI, the Homebrew
  formula's test block and the Dogfood phase that launches `perch --version` as
  an inner client are all untouched;
- capped at two seconds and abandoned in silence on any failure, so a machine
  with no network, no `curl` or a slow one loses a line and nothing else;
- switched off entirely by `PERCH_NO_UPGRADE_CHECK`, named in the install guide,
  because an opt-out nobody can find is not one.

And **`perch status` stays silent on the network**, which was the specific harm
0032 identified: a command cheap enough for a shell prompt must not start
reaching out. Nothing else in Perch mentions upgrades either. `perch upgrade
--check` exists for asking on purpose.

The check uses `curl` through the existing `Host::http`, and no HTTP client
crate was taken for it. That was considered and deferred: `Host::http` is
exactly the seam ADR a-crate-must-not-cost-a-seam says a crate may be taken
behind, and a machine without `/usr/bin/curl` is a real failure this repository
has already seen once. But the traffic that would justify a TLS stack in a
credential tool is Anthropic's, not a version check, and the decision should be
argued there. What was refused outright is *two* HTTP mechanisms, which is worse
than either.

## Honesty about why this exists

ADR an-upgrade-asks-its-channel named its reopener precisely: "somebody stranded
on an old version who installed through the shell installer. That is a real
report, not a hypothetical." No such report arrived. The author asked for the
command because Perch now has Releases and packages and it felt missing, which
is exactly the reasoning 0032 was written to resist.

It is recorded rather than dressed up. The bet being made is that routing to the
Channel makes this a thin command rather than the machinery 0032 feared, and that
the `--version` line is worth the silence it costs. Both halves are checkable
later against a real reading.

## What would reopen it

Somebody whose `perch --version` got slower or noisier in a way that mattered,
or a machine where the tty guard turned out not to be the right test — a CI
system with a terminal attached, most likely. That would move the check to
`perch upgrade --check` alone and leave `--version` as it was, which is the
smaller half of this and separable from the rest.
