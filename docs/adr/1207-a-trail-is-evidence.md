# A trail is evidence

Perch has never written a log line. Not a debug flag, not a verbose mode, not a
file anywhere under `$PERCH_HOME` that says what happened — `grep` for `log::`
or `tracing::` across `src/` returns nothing, and that was deliberate for as long
as it lasted. A tool holding OAuth credentials that writes nothing down cannot
leak what it never recorded.

What it costs is every bug report. A report arrives as prose from somebody who
has already closed the terminal, and the four things that would identify the
failure — which Perch, which Claude Code, which Channel, what the Registry
looked like — are asked for by hand in a form and answered partly or not at all.
Meanwhile `probe.rs` knows more about the machine than any report has ever
carried: six named assumptions, each gating a dangerous operation
(ADR an-assumption-is-probed), and none of it reaches anybody unless a refusal
happens to quote one.

The Watcher is the single exception and it is the wrong shape to copy. Its
decisions go to `watch.log` on macOS and Windows, appended forever with no
rotation, and on Linux to the journal, which is why `perch watcher status`
declines to read its own log at all rather than shell out to `journalctl` three
ways (ADR a-crate-must-not-cost-a-seam). One question, two answers, one of them a
subprocess.

## The rule

> **Perch keeps a Trail: two lines per invocation, in a file that is evidence
> rather than one of the Holdings. `perch probe` renders it, redacted.**

## Why it is not a Holding

The test is the one `CLAUDE.md` states: the Holdings are what a changelog entry
cannot give back. A Profile, a Credential, the Registry naming them and an Export
carrying all three are gone for good when a build reads one wrong. A lost Trail
costs a person nothing they cannot have again by running the commands again.

So the Trail carries no `version`, gets no migration and earns no refusal, and it
is shaped so it can never need one: one JSON object per line, unknown fields
ignored on read, unparsable lines skipped rather than refused. A shape that
degrades cannot break. This is the whole of what keeps the Trail cheap — the
moment it needs a version, every future change to it costs a migration forever,
and it would have bought that with a file nobody would grieve.

It travels in no Export. A Trail from one machine means nothing on another, and
an Export is decrypted somewhere else by definition, so carrying one would move
unredacted history to a second machine in exchange for nothing. A Purge takes it
without a line of code: `purge::erase` already removes the whole of Perch's home.

## Two lines, not one

A single line written when a command finishes is silent about the invocation
most worth reporting. A `perch switch` that hangs on a keychain dialog, or takes
a Ctrl-C, or is killed outright, would leave nothing at all.

So a line goes down when the command starts and another when it ends, paired by
an id. An unpaired start line is a finding nothing else in Perch can produce:
*`perch switch <account 2>` was started 41 minutes ago and the process that ran
it is gone.* Today that arrives as "it hangs sometimes" with nothing to check it
against. Neither line is ever rewritten, which is what keeps the file
append-only and the reader trivial.

The start line carries its pid, because *unpaired* on its own means two things.
`perch watcher run` is a loop that runs until the session ends, so on every
machine with a Service one start line is unpaired at all times and nothing is
wrong. A pid that is still alive, and started no later than the line, is a
command still running; doubt answers the same way, so nothing is called dead on
a guess. That is the reasoning a session marker already uses
(ADR a-profile-is-live-by-evidence). What is left — dead, and unpaired — is read
only within the window, because a machine that went down leaves one of those per
reboot and a reboot explains it.

## Raw on disk, redacted on read

The Trail holds emails, Aliases, Group names and paths as they were typed. It
holds no Credential, ever, which `CONTEXT.md` already binds rather than this
document.

Storing them raw is not new exposure: the Registry sits in the same directory at
the same permissions and holds every email, organization and Alias in plaintext
already. Redacting at write time would destroy information permanently, and the
one person entitled to it is the machine's owner, who is also the only one who
can read the file. Every rendering is redacted instead, so what leaves the
machine is `<account 3>` and the raw form is a decision made once, by the person
who knows what is private, with `--raw`.

Placeholders number by the Account's position in the Registry, so `<account 3>`
means the same Account everywhere in one rendering and in every rendering
afterwards. Two probes pasted a week apart stay comparable, which is what makes
"it was fine on Tuesday" a checkable claim. The kind survives redaction where the
value does not: `switch <account 3> (alias)` keeps the two facts that debug a
Target, since which kind matched is already something Perch says when it acts.

## The one write in Perch that fails in silence

Every other write refuses and names the repair. This one never refuses, never
warns, and never moves an exit code — a full disk, a read-only home or a
`$PERCH_HOME` that has become somebody else's all cost the line and nothing more.

A diary that can refuse `perch switch` is a new way for every command in the
product to fail, bought for a feature nobody invoked. The failure surfaces where
it is useful instead, and the signal for it is one Perch already holds: a
Registry written *after* the Trail's last line means a command ran and wrote
nothing down, since every command writes both.

## What is rejected

**A log level, a `--verbose`, a general logging crate.** The Trail answers one
question — what was run, and how it went — and it answers it the same way every
time. A level is a knob whose wrong setting is discovered after the failure it
was needed for, which is the failure mode this document exists to remove.

**Reading the journal to fill out the picture.** The Watcher writes a Trail line
for the one round that moved something — it Switched — which puts the decision
that matters on every platform in one file. A round turned away or with nowhere
to go recurs every interval for as long as the condition lasts, some 576 a day
against a live client, so those stay in the Watcher's own log. What is left in the journal is the round-by-round
record, about 575 entries a day, which would evict everything a person typed
from any cap worth having. Spending a seam to reach it, and overturning a
decision already made against doing so, would buy the noise and lose the signal.
A Probe names the journal and the log file instead, and reads neither.

**The Trail as the Watcher's log.** The inverse of the same argument. A round
that looked and did nothing is not an event, and neither is one that could not
read a figure: what a Quarantine leaves is in the Registry, where a Probe reads
it as a finding of its own.

**`perch probe` writing its own lines.** It would push the lines somebody wanted
out of the window every time they re-ran it. For the same reason it is exempt
from bringing the Registry forward, joining `version` and `upgrade`: a Registry
at version 3 that no command has carried forward is a *finding*, and a probe that
quietly fixes it can never report it.

**A byte cap with the oldest lines dropped.** Trimming one file means rewriting
it under a lock on the hot path of every command. Two files taking turns is a
`rename`, which is atomic and which the port already has.

## Consequences

- The port gains one method, an append of a single private line. It cannot reuse
  `write_private_file`, which writes beside the target and renames over it: two
  concurrent commands would each rename a whole copy over the other, and one
  would lose everything the other wrote. A new port method is a failing
  conformance test until the table asks for it
  (ADR a-class-not-its-instances), and `FakeHost` gains its twin.
- Both writes live in `main`, around the one place every outcome already becomes
  an exit code. The exemption list reads like `migrates` beside it.
- `trail.log` rotates to `trail.log.1` at a cap, and `perch probe` reads the
  older file first.
- The Trail is written by every command including the reads. `perch status` in a
  shell prompt is a signal rather than noise: forty runs saying the same wrong
  figure is the shape of a real report.
- A Purge takes the Trail with everything else, and an Export leaves it behind.
  Neither needs a line of code to say so.
