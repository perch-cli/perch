# A Run makes a Profile Live, and a Switch reads through it

A Profile with a Run against it is a Live Profile. Perch already knows how to
recognise one — a session marker naming a process that is still the one that
wrote it (ADR 0022) — and already refuses to write into one. What it did not do
was produce that evidence for the sessions it launches itself, so a Run got the
protection only when Claude Code happened to write a marker of its own, and a
`perch run <target> -- npm test` got none at all.

## A Run writes the marker, and names its own process

Perch writes `sessions/<pid>.json` into the Profile before the launch and
removes it after, naming **its own** process and recording `startedAt` as the
moment the Run began. Perch waits for the program it launched, so that pid is
alive for exactly as long as the Run, and it is knowable before the launch,
which the child's is not. Naming the child instead was rejected on that alone:
the marker has to exist before the process it protects starts working, or the
window it leaves open is the first thing anybody does in a session.

The marker corroborates itself by construction. Perch's process began strictly
before Perch wrote the file, so the ADR 0022 check passes while the Run lives
and fails the moment that pid belongs to anybody else. That is what makes a Run
killed rather than exited safe: the marker outlives the process, and says
nothing without it. Closing the terminal is how a session usually ends, so this
is the ordinary path rather than the exception.

A Run that cannot write its marker is refused rather than launched unguarded.
Everything else a Run cannot do is a remark — a key that did not Carry costs a
dialog — but this one is the whole protection, and a session nothing is
protecting can be Captured or Renewed out from under its author mid-task. A
person told beforehand has lost a command; one told nothing loses their work.

## Writing into a Live Profile is refused, reading out of one is not

The consequences need stating rather than discovering, because they do not all
point the same way.

**Refused**, all with the existing exit code 16: a Capture, a Renewal, and a
`.claude.json` key write. Each of them writes *into* the Profile, under a client
that holds those files open and rewrites them on its way out.

**Allowed**: `perch switch` onto an Account with a Run against it. A Switch reads
that Profile's Credential and writes it to the Default Profile; the Live Profile
is untouched. Refusing it would make Run and Switch lock each other out for no
reason at all — the Account you are running in one terminal is exactly the one
you would want active in the others. So the refusal moved off the incoming
Account and onto the outgoing one, where the Capture actually lands.

**Allowed**: reading Utilization. An Account with a client running has a fresh
access token by ADR 0005's own reasoning, so its figures are readable without
renewing anything. The watcher is not blinded by a Run.

## The sessions directory stops crossing

This amends ADR 0026, which named `sessions` as an example of the Shared State
that must follow a person into a Run. It cannot.

`sessions` is not the person's and not the Account's: it is the configuration
directory's own answer to "is a client running *here*". Linked, one Profile's
answer is every Profile's — a single `claude` in another terminal would make
every Account on the machine Live and every Switch refuse, and a Run's own
marker would land in the Default Profile rather than in the Profile it is about.
So the denylist ADR 0026 wrote as two entries is three, and the third is a
different kind of thing from the other two.

Nothing is lost. `sessions` holds live-client markers; the transcripts and plans
a person would miss are `projects`, `history.jsonl` and `plans`, and all of
those still cross.

## Consequences

`Host` grows one primitive — this process's pid — which is the smallest thing a
Run needs to name itself and the only part of this that is not already in
`probe.rs`.

Perch now writes into a directory Claude Code owns. The marker carries the two
fields that make it evidence and one that says who wrote it; it does not invent
a `sessionId` or a version, because a file claiming to be a Claude Code session
when it is not is worse than one that is plainly Perch's. If a future Claude
Code reads these files for something beyond liveness, that is the assumption
that breaks, and it breaks in `probe.rs` where the rest of them are.
