# Using it is the proof

There is no suite standing in for somebody living with Perch. What such a suite
would have proved, use proves better and sooner, and what use does not reach is
smaller than it looks and named here rather than covered.

## What use closes, and what it does not

Four things no fake-driven suite reaches, taken one at a time against somebody
actually living with the tool:

- **`add`, then `switch`, then `run`, then `status` in sequence over real
  bytes** closes completely. That is not a simulation of daily use; it *is*
  daily use.
- **A real client accepting what Reconcile built** closes completely, and
  faster. Claude Code either starts against the Profile or it does not, and the
  person finds out inside a second.
- **argv, exit codes and rendered lines** do not close at all — nobody notices a
  broken `conflicts_with` or an unmapped exit code by using a tool correctly.
  They need a suite that drives the binary, which is
  ADR the-binary-proves-its-surface's.
- **A Renewal filed** closes halfway, and the half that is left is stated
  exactly below.

**This is paid for by the commitment to actually use Perch**, and that is the
honest dependency rather than a hedge on it. If nobody lives with the tool, the
first two are covered by neither use nor a suite, and this is a straight loss
rather than a trade.

## Why there is no reduced core

The tempting middle is a harness keeping the one or two phases that reach
furthest. It is refused, because the cost of such a subsystem is the machinery
rather than the phases: a Preflight, a Marker and the wizard that writes it,
`Needs`, `Halt`, `Setback`, `Attempt`, `Outcome`, `Run`, a report writer and an
attendance opt-in — most of the lines, and every one of the glossary entries.

Judged by conceptual surface, which is the yardstick, a reduced core is the
worst of the options rather than the moderate one. A subsystem's cost here is
what it makes a reader learn, and that cost is the same at one phase as at ten.
So the choice is binary, and it goes the whole way.

## What is actually lost

One thing, and it is worth being exact rather than reassuring: nothing in the
repository watches a real refresh token go to Anthropic and then reads what
Perch did with the answer. Real use provokes Renewals constantly and inspects
none of them.

Three things make the loss affordable.

Such an assertion is **opportunistic by construction**. A Renewal only reaches
Anthropic once the access token has actually run out, so it proves something
only on a machine whose token has already gone stale — one sample, taken when
somebody had an evening free.

**Real use takes far more samples.** Every stale token in a working week is a
real Renewal against real Anthropic, and there are more of them in a month of
use than in every suite run that would realistically ever be performed.

**Its failures are loud.** A Renewal filed wrongly is not a quiet wrong answer;
it is the next command failing. A renewed credential that never lands leaves a
stale token that simply renews again, which is harmless. A `RotationLost`
reaches the person as a Quarantine with `relogin` printed beside it. There is
no silent corruption here for an assertion to be the only witness to.

## Two problems a harness that asks a person will meet again

Both are answers to questions only a suite creates, so neither has a subject
now. They are written down because a future harness that asks a person to do
anything meets both, and neither should have to be rediscovered.

**How does a suite that can only run part of itself avoid a green run that
quietly proved a third of what it was asked to?** By counting what this machine
can prove and saying the number out loud before anything acts, so a skip is a
number rather than a line somebody scrolls past. The general form of that rule
survives without the suite, in ADR a-suite-is-named-and-gated.

**How do you ask a person to perform a step without letting the verdict become
a keystroke?** By letting them *do* things and never *decide* whether it
passed. A verdict that is a keystroke cannot be told apart from a `y` typed to
get out of the way, on the fourth browser round trip, at eleven at night — and
it is precisely the fourth round trip at eleven at night that such a suite gets
run on. Where the answer genuinely cannot be read off disk, it is recorded as
*attested* rather than asserted, so a line somebody vouched for and a line
Perch established never read alike a week later.

That second answer has a corollary about attendance which outlives it too:
**detection alone is not enough, because a terminal is not a person.**
`Host::is_interactive` asks whether this process's two ends are ttys, and that
has a yes on plenty of machines nobody is sitting at — a scheduler under a pty,
an SSH session somebody walked away from, a `tmux` pane left open upstairs.
Being there is a claim only a person can make, so it is opted into and then
checked: the flag is what says somebody is present, and the tty check is what
refuses an opt-in nothing could answer, turning a stall in the middle of a run
into a refusal at the top of it.

## Marker names the session marker

The word belongs to the session marker naming the process that wrote it — the
thing ADR a-profile-is-live-by-evidence corroborates a Live Profile with, and
what `probe.rs` calls `Marker` in code. A harness receipt holding the good noun
while the load-bearing idea goes around the glossary in lowercase is the wrong
way round, and this is a conceptual-surface gain independent of any line count.

## Consequences

`Host::is_interactive` stays. It is used by `add`, `remove`, `purge` and
`upgrade`, and the reasoning above about attendance is what a harness would
need rather than what those commands need.

`CONTEXT.md` holds no entry for a harness, a Preflight, a Phase, an Attendance
or an Attestation. **Repair** is a product feature — `perch relogin` — rather
than a step in a suite that borrowed it.
