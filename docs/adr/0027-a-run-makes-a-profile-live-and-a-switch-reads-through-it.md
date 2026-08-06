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

**Refused**: a Capture, a Renewal, and a `.claude.json` key write. Each of them
writes *into* the Profile, under a client that holds those files open and
rewrites them on its way out.

They are refused in the three different registers those paths already have, and
deliberately not levelled to one. A Capture stops the Switch with exit code 16,
because a Switch that cannot Capture must not proceed. A Renewal degrades to the
cached figure and the command still exits 0, which is ADR 0018 — a refresh
reports what it could not read rather than failing. A `.claude.json` key simply
does not cross, silently, which is ADR 0003's amendment — nothing on the Carry
path may refuse a Run, because a person who answers one onboarding question has
lost less than one who was refused a client. Only the first of the three is a
failed command, and saying otherwise would contradict two ADRs.

A Renewal is refused for every directory the Account's Credential could be in
use from, not only the one being written. A Rotation retires the refresh token
for an *Account* rather than for a file, so every copy dies together: `perch run
<the active account>` puts a client on that Account's own Profile while the copy
a refresh would renew sits in the Default Profile, and renewing it would log
that client out just the same.

**Allowed**: `perch switch` onto an Account with a Run against it. A Switch reads
that Profile's Credential and writes it to the Default Profile; the Live Profile
is untouched. Refusing it would make Run and Switch lock each other out for no
reason at all — the Account you are running in one terminal is exactly the one
you would want active in the others. So the refusal moved off the incoming
Account and onto the outgoing one, where the Capture actually lands.

One case is both at once and stays refused: `perch switch X` where X is already
the active Account, which is the repair for an interrupted Switch. X is the
outgoing Account there as well as the incoming one, so the Capture would write
into the very Profile the Run is holding. That is the outgoing rule doing its
job rather than an exception to the incoming one.

The same reasoning puts a second liveness check into `perch relogin`, after the
login rather than only before it. The one before it is minutes stale by the time
the browser comes back, and what follows it writes a fresh Credential into the
Account's own Profile — so a Run started during the login would be written
under. The login itself runs against a directory of its own, so it can never be
what that check finds.

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
