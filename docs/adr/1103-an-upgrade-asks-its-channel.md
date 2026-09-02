# An Upgrade asks its Channel

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
depends on **by exact version**, which is the whole of how a Release stays
coherent across six packages. Overwriting it leaves a wrapper declaring one
version and spawning another, until the next `npm install` throws the change
away.

So `perch upgrade` works out which Channel left this Installation and hands the
work back to it: `brew upgrade perch`, or `npm update -g perch-cli`. The command
is echoed before it runs, and its exit status is Perch's — a failed `brew
upgrade` is a failed Upgrade, and translating it into a code of Perch's own
would lose which of `brew`'s many failures it was. Only the installer Channel —
the one where nothing else manages the binary — is replaced by Perch itself.

That is the same line ADR perch-takes-back-what-it-wrote draws and the glossary
already carries: an Installation is what a Channel left, and taking one back
belongs to the Channel that made it. Replacing one is the same kind of act as
removing one, and it was never going to be true that Perch could remove an
Installation only by asking the Channel but replace one behind its back.

**npm on Windows is the exception that proves it is the right shape.** There the
routed command cannot work at all: `npm update -g` would be replacing
`perch.exe` while it is the running process, and Windows holds the file open. So
that one case prints the command instead of running it, and reports nothing done
rather than success — a `0` there is a script's `perch upgrade &&
restart-my-thing` restarting the old binary on the strength of an Upgrade that
only printed a suggestion. A design that self-replaced everywhere would have had
no way to notice.

## How Perch knows which Channel

By the path of its own executable, and by nothing else, because there is nothing
else available. Every Channel points at the same Release and installs the same
bytes, so a Channel stamped into the binary at build time would mean five builds
per Target instead of one — paying for the answer in the place that is hardest
to keep honest. A marker file beside the binary is no better: it would be absent
for Homebrew and npm both, so its absence would mean two different things.

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
their defaults do not agree — `~/.local/bin` on Unix,
`%LOCALAPPDATA%\Perch\bin` on Windows. Hard-coding the Unix one reads every
Windows Installation as a binary Perch did not place, and refuses to upgrade the
only Channel it is able to upgrade itself.

**Anything else refuses.** A binary somebody unpacked from the Release page into
`/usr/local/bin` looks exactly like an installer Installation and is not one,
and overwriting a file at a path Perch never wrote is the one irreversible thing
this command could do. So it says what it found and points at the installer.
`--channel` is there for the person who symlinked or relocated it, and is the
whole of the escape hatch: what it names is taken as given rather than checked
back against the path, because the path being wrong is why it was typed.

## Perch does not download the Release; the installer does

`install.sh` and `install.ps1` already resolve a tag, download an Artifact,
verify it against `SHA256SUMS`, verify its provenance where `gh` is present,
and put the binary in place without a half-way state. They take `PERCH_VERSION`.
That is the entire job, written and tested.

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
passed into it is a tag. It is written closed to everyone but its owner and
without an execute bit, and cleared whichever way the run went: it is a program
about to be run, and one a second user on the machine could edit between the
write and the run is a program somebody else chose.

It also happens to be why an upgrade does not ask about `PATH` again:
`install.ps1` is idempotent there, on the reasoning that a re-install is how
Perch gets updated.

**Windows needs one thing the installer did not have.** A running `perch.exe`
cannot be replaced, and the installer's answer to that was to say so — "is perch
running? Close it and try again" — which is exactly the state a self-upgrade is
in. Windows does permit *renaming* a running executable, so the embedded script
renames it to `perch.exe.old`, moves the new one into place, and clears the
stray — which fails while that binary is still the running one, so in practice
the next Upgrade is what removes it. The residue sits beside the binary, on the
Installation side of the line ADR perch-takes-back-what-it-wrote draws, which is
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
quietly, because silence there reads exactly like a check that passed.

Requiring `gh` was the alternative. It makes a GitHub CLI a hard dependency of
staying current, which is more than this buys.

## Going backwards is allowed and is named

`--release` takes an older Release, because the alternative leaves somebody who
upgraded into a bad one with no route back but re-running the installer by hand.

It is not silent, though, because a Perch older than the one that last wrote the
Registry refuses to read it (ADR the-holdings-outlive-a-perch). So going back far
enough leaves a working machine with a binary that will not read its own
Registry. The confirmation names that consequence rather than asking a bare "are
you sure", which is a question nobody has ever answered with information.

Homebrew is the one Channel a Release cannot be named to at all: it installs
whatever the formula says. That is refused rather than quietly ignored, and
refused before anybody is asked to agree to anything — silently installing the
newest when somebody named an older one is the failure they find out about by
reading `perch version` afterwards, if they think to.

## What an Upgrade owes a running Service

The Channel has moved the binary, and neither `brew` nor `npm` has ever heard of
a unit file (ADR the-machine-runs-the-watcher). On Unix the Service is left
running the *old* binary out of an inode nothing can see any more, and the path
its unit names may not exist at all — so the unit is written again against the
binary that is there now, and restarted onto it.

Said rather than raised however that goes, and only where the Channel's own
command succeeded. The Upgrade itself is what this command is about: a Service
that could not be refreshed is a warning with a one-command repair, and a `brew
upgrade` that failed on a checksum must not bounce the Watcher onto the binary
it was already running and then report that it did.

## Perch does not look for its own updates on a schedule

`perch version` says when a newer Release exists, and that is the whole of
what Perch volunteers about itself. There is no schedule, no cache, no age to
reason about and no staleness to explain — the machinery
ADR a-figure-carries-its-age gets right for Utilization is not built a second
time for a notification. "Perch phones home on a schedule" is a sentence that
has to be true, explained and bounded, and it buys a line of text.

So the request happens when a human types the command, or not at all:

- suppressed when stdout is not a terminal, so scripts, CI, the Homebrew
  formula's test block and a `perch run` launching an inner client are all
  untouched;
- capped at two seconds and abandoned in silence on any failure, so a machine
  with no network, no `curl` or a slow one loses a line and nothing else;
- switched off entirely by `PERCH_NO_UPGRADE_CHECK`, checked before the request
  rather than after it, because the objection somebody has to this is the network
  call and not the line — and named in the install guide, because an opt-out
  nobody can find is not one.

And **`perch status` stays silent on the network.** A command cheap enough for a
shell prompt must not start reaching out, and nothing else in Perch mentions
upgrades either. `perch upgrade --check` exists for asking on purpose, and exits
nought whichever way the answer goes: every other non-zero code Perch has is a
refusal, and "there is news" is not one.

**`version` and `upgrade --check` both stay, and the overlap is the point.**
`perch version` answers what is installed and volunteers the rest, so it is
bounded like something nobody asked for: two seconds, off a pipe, silent on
failure. `perch upgrade --check` is the question asked deliberately, so it is
bounded like nothing — it waits as long as the answer takes, names the Channel
this Installation came from, and answers a script through `--json`. One command
carrying both would have to pick one set of bounds, and either choice is wrong
for half of who is typing it. `perch version` takes no `--json` for the same
reason: `--check` already has one, and a second document saying a subset of the
first is a second contract that can never quietly move.

`perch version` also comes before `migration::bring_forward` rather than after
it, so asking what is installed reads and writes nothing on the machine. It is
what somebody runs when the machine is already misbehaving, and a migration is a
write.

The check uses `curl` through the existing `Host::http`, and no HTTP client crate
was taken for it. That was considered and deferred: `Host::http` is exactly the
seam a crate may be taken behind, and a machine without `/usr/bin/curl` is a real
failure this repository has already seen once. But the traffic that would justify
a TLS stack in a credential tool is Anthropic's, not a version check, and the
decision should be argued there. What was refused outright is *two* HTTP
mechanisms, which is worse than either.

## What would reopen it

Somebody whose `perch version` got slower or noisier in a way that mattered, or
a machine where the tty guard turned out not to be the right test — a CI system
with a terminal attached, most likely. That would move the check to
`perch upgrade --check` alone and leave `version` saying only what is installed,
which is the smaller half of this and separable from the rest.
