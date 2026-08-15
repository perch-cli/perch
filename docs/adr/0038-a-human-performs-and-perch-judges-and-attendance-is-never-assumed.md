# A human performs, Perch judges, and attendance is never assumed

> **Superseded by ADR 0041.** Once the removal tracked at #148 lands there is no
> Phase left to attend, so the rules below have no subject. Two of them are
> worth more than the suite was and are
> named in ADR 0041 as ideas deliberately released rather than overlooked: the
> Preflight's figure, and the refusal to let a verdict be a keystroke. One thing
> here is *not* repealed — `Host::is_interactive` survives untouched, because
> `add`, `remove`, `purge`, `tui` and `upgrade` all use it. Left here as
> written, because a future harness that asks a person to do anything will meet
> both of these problems again.

ADR 0037 left four things unproved and closed one of them. What is left needs a
person: a browser round trip to log an Account in, a real Claude Code launched
and then quit. Phase zero can already hand the terminal over — `repair` takes an
`out` and calls `Perch::interactive` for each Quarantined Account — but a Phase
cannot. Its signature is `fn(&Perch<'_>) -> Proof`: no writer, no reader, nothing
to ask a question with.

So the phases that are left are not a phase-writing exercise. They are a change
to what a Phase *is*, and that change has two halves worth writing down, because
both are easy to get wrong in the direction that feels helpful.

## The human performs; Perch judges

The tempting shape is the one that writes itself. Launch a real Claude Code
against a Profile, wait, and ask: *does it say `alice@example.com`? [y/N]*.

It is refused. A verdict that is a keystroke cannot be told apart from a `y`
typed to get out of the way, on the fourth browser round trip, at eleven at
night — and it is precisely the fourth round trip at eleven at night that a
suite like this is run on. A suite whose red can be typed away is the suite ADR
0037 describes as having its red ignored within a month, arrived at from the
other direction.

It is also unnecessary, which is what makes refusing it cheap. Nearly everything
a person would lean in and check is on disk by the time they would check it: the
session marker naming a process, `.claude.json`, and the links Reconcile made.
Those are things Perch can read, and reading them is a better assertion than
watching somebody read them.

So a Phase may ask a human to *do* things — walk a login, quit a client — and may
not ask one to *decide* whether it passed. An Attestation stays available for the
case where Perch structurally cannot see: it is recorded in the report as
attested rather than asserted, so that a line somebody vouched for and a line
Perch established never read alike a week later.

## Attendance is opted into, and then checked

A phase that hands over the terminal must not run where there is nobody. CI is
the obvious case and the easy one — the runner holds no Accounts and has no tty
— but the failure this guards against is worse than a skip.

Detection alone is not enough, because a terminal is not a person.
`Host::is_interactive` asks whether this process's two ends are ttys, and that
question has a yes on plenty of machines nobody is sitting at: a scheduler
running under a pty, an SSH session somebody walked away from, a `tmux` pane left
open on a laptop upstairs. A run that inferred attendance would take on the
phases that open a browser and then wait, all evening, for a person who is not
coming. Being there is a claim only a person can make.

A flag alone is not enough either, for the mirror image of that reason. An opt-in
carried in from an exported variable, a CI job's environment, or a shell profile
reaches contexts where nothing can be typed at all, and there the first question
blocks for ever — with, at best, the question itself on a screen nobody is
reading.

So both, pointing opposite ways. `PERCH_DOGFOOD_ATTENDED=1` is what opts in — a
deliberate act, in keeping with the Marker's rule that anything costly and
state-moving is asked for rather than inferred. `Host::is_interactive` is what
refuses: an opt-in nothing could answer becomes a refusal at the top of the run
rather than a stall in the middle of it. CI needs no change to either the
workflow or the phases.

One thing that is *not* the reason, though it reads like it should be: `cargo
test` swallowing the question. It does not. libtest captures the `print!` macro
sink, and this suite writes through a `Stdout` handle, so the Preflight and every
phase print to the terminal with or without `--nocapture`. `--nocapture` is still
what a Dogfood run wants — libtest holds a passing test's captured output back
entirely — but it is not what stands between an attended phase and a silent
hang.

## What it costs: the figure becomes a ceiling

The Preflight's figure is the sentence ADR 0037 built the Preflight around, and
this weakens it. It is worth being exact about how.

A Renewal only reaches Anthropic when the access token has actually run out —
`renew_under_the_lock` hands back the stored token untouched where
`credential.usable_at(now)`. So a phase about a Renewal is a phase that cannot
decide when it runs, and its gate is "is this Account's token stale?", which
`Needs` cannot express: the registry holds identity, plan, quarantine, group and
cached Utilization, and expiry is not among them. It lives on the Credential, in
the Credential Store, which on macOS is a `security` call per Account (ADR 0008).

Computing the gate in the Preflight would make the figure exact and open every
run on a Mac with a row of keychain prompts, before anything has been proved.
That is a suite people stop running, which costs more than the figure is worth.

So a Phase may discover at run time that it cannot prove anything here, and say
so — `Halt::Skipped` alongside `Halt::Stopped`, mapped onto the `Outcome::Skipped`
the report already knows how to print. The figure becomes an upper bound: *this
machine can prove up to seven*.

The distinction that keeps this honest is the one 0037 actually draws. What it
forbids is a run that **quietly** proved a third of what it was asked to. A
run-time skip is named, counted and in the report, with a reason somebody can act
on — *nothing was stale here, come back in the morning*. That is the loud kind.
The alternative on offer was a phase that runs against a live token, renews
nothing, and reports green, which is the quiet kind wearing the figure's colours.

## Consequences

`Needs` gains `attended`, and two fields the Marker rather than the registry
answers: `grouped`, counted over the Group the wizard arranged, and
`spare_login`, from how many logins the wizard was told this person holds. Gating
on a Group somebody made for their own reasons, or on a login count guessed at,
would both be the suite acting on something nobody deliberately told it.

A Phase takes a writer, and `Perch` grows an `ask`. Nothing else in the suite
gains a way to talk to a person: phase zero had one already, and a third would be
a third place for the rules above to be forgotten in.

Deliberately not part of this: a Phase may still not unwind itself, and an
attended one that stops says what is now true in prose — including *quit the
client you have open* — rather than in a new field on `Setback`. A field that
exists for two phases and is `None` everywhere else is one the third phase fills
in wrong.
