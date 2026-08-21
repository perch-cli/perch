# Dogfood runs on real machines and proves only what each one holds

> **Superseded by ADR using-it-is-the-proof.** The suite was removed entire
> (#148), and there is no reduced core: the cost was always the machinery rather
> than the phases, so keeping one phase would have kept the whole vocabulary.
> Nothing below was found to be wrong — what changed is that a person actually
> using Perch stopped being hypothetical, which closes two of the four gaps
> named here completely and a third halfway. Left here as written, because the
> four gaps are still the right list and the rejection of a stubbed `claude`
> still holds: ADR using-it-is-the-proof repeals the suite, not that argument.

Perch is tested three ways already, and every one of them buys its determinism
by replacing something. The behavior suites drive the real command code against
`FakeHost`, so there is no filesystem. `tests/conformance.rs` asks the two
adapters the same questions, so there is a filesystem and no Perch above it. The
`contract_*` suites ask whether Perch's beliefs about Claude Code are still true,
so there is a real client and no Account.

Four things are left over, and they are left over together. Nothing anywhere runs
`perch` as a process, so argv, the exit codes and every rendered line are
unasserted. Nothing runs `add`, then `switch`, then `run`, then `status` in
sequence over real bytes. Nothing has ever asked Anthropic to renew a real token.
Nothing has ever launched a real client against a Profile and looked at what
Reconcile linked.

## Why not a hermetic suite

The obvious answer is an end-to-end suite that replaces the awkward parts: a
scratch `PERCH_HOME`, a stub on `PERCH_CLAUDE_BIN`, a planted Credential, a
recorded transcript instead of Anthropic. It runs on every platform, it runs in
CI, it runs on a laptop with no login, and it is deterministic.

It is rejected, and the stub is the reason. Each of the four gaps is a gap
*because* something real was replaced, so a suite that replaces `claude` with a
script printing a version string has rebuilt `FakeHost` at a higher price and in
a worse language. It would find argv bugs. It would find nothing else on the
list, and the list is what it was for.

So Dogfood takes the opposite bargain. Nothing is replaced, and the price is paid
where it is cheapest to pay: the suite asks the machine in front of it what it
can prove, and proves that.

## The observer may be replaced; the subject may not

That rule needs one refinement, because read carelessly it forbids something it
should allow. `perch run <target> -- <program>` reaches the probe for Claude Code
only where nothing was named after `--`; name anything else and the Run is a Run
in full — Reconcile, then Carry, then a Live Profile claimed, then a launch with
`CLAUDE_CONFIG_DIR` pointing at the Profile. So a phase can run `perch run alias
-- <perch> --version`, wait for it to exit, and read off the real filesystem what
Reconcile actually linked. No client, no quota, no human.

That looks like the stub above wearing a different hat, and it is not. The stub
is rejected for replacing *the subject*: a script printing a version string
stands in for the very thing whose behavior is the question, so what survives is
a test of the harness. Here the subject is Reconcile, and Reconcile runs
untouched, over the real filesystem, against the real Profile. What has been
replaced is who is *watching* — and since what Reconcile does it does to disk,
the watcher can be anything at all, including nothing.

The question that separates the two: *what would still be proved if the
replacement were perfect?* A flawless fake client proves nothing whatever about a
Renewal, which is why replacing it there is the rejected bargain. A flawless fake
observer changes nothing about the links Reconcile made, because it never went
near them.

A real client is still launched once, attended, and it has to be: whether Claude
Code accepts what Reconcile built is a question only Claude Code can answer.

## A machine proves what it holds

A Preflight opens every run and reports before anything acts — which client is
installed, whether the network answers, how many Accounts are held, which of them
are Quarantined, and how many of the suite's phases that adds up to. A machine
with no login runs the phases needing no Account and says so as a number, not as
a skip line somebody scrolls past. The failure this exists to prevent is a green
run that quietly proved a third of what it was asked to.

It refuses outright on a machine the setup wizard has not marked. The wizard's
first act is an Export, and the Export is only a safety net if it is guaranteed
— *I am fairly sure I ran setup* is exactly the belief that is wrong on the
occasion it matters. A machine somebody only meant to connect to should not be
able to start Switching Accounts around because a command was recalled from
history.

## Repair is phase zero

There are more machines than Accounts, and every machine holding a login holds a
refresh token for the same Account. A Renewal may Rotate, a Rotation retires what
the others hold, and `Quarantine::RenewalRejected` already names the outcome:
*retired, revoked, or belonging to a login that has been ended elsewhere.* A
Dogfood run on one machine can therefore Quarantine its Accounts on the other
three, and nothing can be done about that short of one Account per machine.

So it is not treated as damage. Every run opens with a Repair, and a Quarantine
somebody else's run caused is the ordinary starting state rather than an ambush.
This buys something that is otherwise hard to arrange deliberately: `relogin`
against a genuinely retired token, on every platform, on every run after the
first.

## What it costs

Determinism, and the loss is real. A phase can fail because Anthropic is slow,
because a login expired, because the machine was busy. A suite that cannot
distinguish those from a defect is a suite whose red is ignored within a month,
so a Dogfood failure has to say which of the two it is — a fault in Perch, or
news about something upstream — and a phase that cannot tell the difference does
not belong here. The suites that can be deterministic already are, and none of
them are being given up for this.

It also costs real quota, which is why phases steer policy and never figures: a
Threshold set below where an Account already sits fires a Cycle immediately, and
burning an Account down to reach a branch is what the fake suites are for.

## Consequences

The suite runs in CI on all three platforms and skips nearly everything there,
deliberately. The point is not what CI proves — it is that the Preflight and the
degradation are exercised continuously, so the day somebody sits down to a full
run on a real machine, the part that decides what to run is not itself broken.

It is run one phase at a time by whoever is watching, and it never unwinds its
own changes. On a real machine an unwind can fail too, and a failed unwind after
a failed assertion leaves a state nobody can read. A phase that fails stops,
prints what is now true, and prints the commands that put it back.

There is no `perch state --json`. Phases read `list --json`, `status --json` and
exit codes, and where a phase genuinely cannot see, the answer is a `--json` a
person would want rather than a dump the tests need — a backdoor exposing the
registry's shape becomes a compatibility surface the moment it ships.
