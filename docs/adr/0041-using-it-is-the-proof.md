# Using it is the proof

> **Carried out in #148.** This ADR is the artifact of a planning effort rather
> than of a change, so it landed ahead of the work it describes instead of
> beside it — a departure from this repo's habit of shipping an ADR with its
> implementation. The tree now matches it: the subsystem repealed below is gone.

ADR 0037 built a suite that runs on real machines and proves only what each one
holds. ADR 0038 changed what a Phase is, so a person could be asked to walk a
login while Perch kept the judging to itself. Both are repealed here, and the
subsystem they describe — `src/dogfood.rs`, `src/dogfood/phases.rs`,
`src/bin/dogfood-setup.rs`, `tests/dogfood.rs`, the `dogfood` feature and the CI
job that exercises it — is to be removed entire.

Nothing about the reasoning in either ADR turned out to be wrong. What changed is
that the thing the suite was a model of became available.

## What Dogfood was for, and what arrived instead

ADR 0037 named four things nothing proved: argv and the exit codes and every
rendered line; `add` then `switch` then `run` then `status` in sequence over real
bytes; a real token renewed by Anthropic; a real client launched against a
Profile. The suite existed because a person using Perch was hypothetical, and the
only way to get at those four was to write down what such a person would do.

A person using Perch is no longer hypothetical. Taken one at a time against
somebody actually living with the tool:

- **The sequence over real bytes** closes completely. That is not a simulation of
  daily use; it *is* daily use.
- **A real client accepting what Reconcile built** closes completely, and faster.
  Claude Code either starts against the Profile or it does not, and the person
  finds out inside a second.
- **argv, exit codes and rendered lines** do not close at all — nobody notices a
  broken `conflicts_with` or an unmapped exit code by using a tool correctly. But
  these were never Dogfood's to hold: they need a suite that drives the binary,
  and they are decided elsewhere.
- **A Renewal filed** closes halfway, and the half that is left is small enough
  to state exactly. It is stated below.

So the suite is not being traded for nothing. It is being traded for the thing it
was built to stand in for, and a stand-in loses to what it stands in for.

## Why there is no reduced core

The tempting middle is to keep the one or two phases that reach furthest and drop
the rest. It is refused, because it buys almost nothing and costs almost
everything.

Of roughly 4,290 lines of production code, the ten phases are 1,466. The other
2,800 are the Preflight, the Marker and the wizard that writes it, `Needs`,
`Halt`, `Setback`, `Attempt`, `Outcome`, `Run`, the report writer and the
attendance opt-in. Every one of the seven `CONTEXT.md` entries and both of the
superseded ADRs describe *that* machinery, not the phases. A Renewal-only core
would shed nine phases, save under a third of the lines, and keep the entire
vocabulary — a person would still have to hold Preflight, Phase, Phase zero,
Attended, Attestation and Marker in their head to read the repo.

Judged by conceptual surface, which is the yardstick, the reduced core is the
worst of the three options rather than the moderate one. A subsystem's cost here
is what it makes a reader learn, and that cost is the same at one phase as at
ten. So the choice is binary, and it goes the whole way.

## What is actually lost

One thing, and it is worth being exact rather than reassuring.

The phase `a Renewal is asked of Anthropic and what comes back is filed` is the
only assertion in the repo that watches a real refresh token go to Anthropic and
then reads what Perch did with the answer. Real use provokes Renewals constantly
but inspects none of them.

Three things make the loss affordable:

It was always opportunistic. The phase returns `Halt::not_here` wherever
`credential.usable_at(now)` — a Renewal only reaches Anthropic once the access
token has actually run out, so the phase proved something only on a machine whose
token had already gone stale. ADR 0038 accepted this and turned the Preflight's
figure into a ceiling because of it. A suite run is one sample, taken when
somebody had an evening free.

Real use takes far more samples. Every stale token in a working week is a real
Renewal against real Anthropic, and there are more of them in a month of use than
in every Dogfood run that would realistically ever be performed.

And its failures are loud. A Renewal filed wrongly is not a quiet wrong answer —
it is the next command failing. A renewed credential that never lands leaves a
stale token that simply renews again, which is harmless. A `RotationLost` reaches
the person as a Quarantine with `relogin` printed beside it. There is no silent
corruption here for a phase to be the only witness to.

## Two ideas released on purpose

The Preflight's figure and the Attested/asserted distinction are the best things
in the two superseded ADRs, and they are released rather than migrated. They are
written down here because releasing an idea and forgetting one should not leave
the same trace.

**The figure** answers: *how does a suite that can only run part of itself avoid a
green run that quietly proved a third of what it was asked to?* By counting what
this machine can prove and saying the number out loud before anything acts, so a
skip is a number rather than a line somebody scrolls past.

**Attested versus asserted** answers: *how do you ask a person to perform a step
without letting the verdict become a keystroke?* By letting a Phase ask a human to
*do* things and never to *decide* whether it passed — and where Perch structurally
cannot see, by recording the answer as attested, so a line somebody vouched for
and a line Perch established never read alike a week later.

Both questions are created by the suite. With no suite there is no partial run to
be honest about and no human-performed step to keep honest, so neither answer has
a subject any more. Nor can a binary-driving suite inherit the figure: it has no
machine-dependent capability, so there would be nothing for the Preflight to
count. If a future harness recreates either problem, the answer is here already
and should not be rediscovered.

## Marker is freed

The glossary currently gives **Marker** to the receipt `dogfood-setup` writes.
Meanwhile the concept that matters to the product — the session marker naming the
process that wrote it, which ADR 0022 corroborates a Live Profile with, and which
`probe.rs` calls `Marker` in code — has to go around the glossary in lowercase.
The good noun is held by the throwaway and the load-bearing idea is unnamed.

Removing the suite frees the word, and **Marker** becomes the session marker. This
is a conceptual-surface gain independent of the line count, and it is the reason
the removal makes the vocabulary better rather than merely smaller.

## Consequences

Seven `CONTEXT.md` entries go — Dogfood, Preflight, Phase zero, Marker, Phase,
Attended, Attestation — and **Marker** is rewritten for the session marker.
The **Repair** entry loses its Phase-zero clause; Repair itself is untouched,
because `perch relogin` was always the product feature and phase zero only ever
borrowed it.

`Host::is_interactive` stays. ADR 0038's reasoning about attendance was
Dogfood-specific, but the primitive is used by `add`, `remove`, `purge`, `tui`
and `upgrade`, and none of that changes.

The repo is left with no test that drives `perch` as a process. That was already
true in every way that counts — the suite was feature-gated, attended and never
run unattended — but it is now true without qualification, and it makes the
argv-and-exit-codes question sharper rather than softer for whoever answers it
next. This ADR deliberately does not answer it.

**This decision is paid for by the commitment to actually use Perch.** That is the
honest dependency. If nobody lives with the tool, the four gaps of ADR 0037 are
not covered by real use *or* by a suite, and this ADR is a straight loss rather
than a trade. The removal should not be read as a claim that the four gaps stopped
mattering — only that a better instrument for three of them became available.
