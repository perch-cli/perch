# Perch does not draw

**Perch has no interactive surface. Every command reads its arguments, does its
work and exits.**

Switching accounts happens when quota runs out, which is mid-task and under mild
frustration. So the shortest command does the whole job: `perch switch` picks an
Account within the current Account's Scope, by that Scope's Strategy and on the
Headroom ADR headroom-is-the-worst-window defines, and switches to it without
asking anything. `perch switch <target>` names an Account, an Alias or a Group
explicitly. Those two are the whole of choosing, and `perch list` is the whole of
looking (ADR the-listing-owns-the-set).

## Every capability exists non-interactively

This is the constraint the decision is worth stating for, and it holds whether or
not anything is ever drawn again: Perch has to be complete over SSH, in scripts,
and in CI. Listing state, setting configuration, saying what a Cycle would decide
— each needs a plain command form, and none may be reachable only by eye.

The rule generalizes past any particular surface: **a panel cannot reach a state a
script cannot.** Clearing a Setting, choosing between Accounts, seeing the ranking
a Cycle makes — if a drawn surface can arrive somewhere, a typed command line can
arrive there too, or the drawn surface is the only route to a state and Perch is
not complete without a terminal.

The constraint has already paid for itself in the one instance where it bit, which
is a point in its favor rather than a footnote to it.

## Why a picker is not the exception

The tempting shape is a full-screen view of Accounts, their Groups and their
Utilization, "for when the choice wants making by eye rather than by rule". It is
refused, and the reason is that no moment wants it.

Making bare `perch switch` open a picker shows the evidence before spending quota,
and costs an interaction at exactly the moment the user wants none — anyone
rotating between subscriptions would type the unattended form every time. That is
the switching moment ruled out. Giving the picker a command of its own instead
means naming some *other* moment, and there is none to name: every moment worth
describing here is the one just ruled out.

A picker's one unique output is not unique either. Showing the Accounts in the
order a Cycle ranks them, with the Headroom that order was made on beside them and
the Ungrouped held rather than ranked, is a real job — and it survives its surface,
in `perch list`, for the price of a sort and a column
(ADR the-listing-owns-the-set).

**A read-only view is refused for a different reason: the reading it offers already
exists.** What a configuration panel would show read-only, `perch config` prints in
one line, from a script, over SSH. What an overview panel would show, `perch status`
and `perch list` already print. A read-only panel keeps a navigation model — the
sidebar, the columns, the cursor — to duplicate views that are already on screen.

**A picker that is only a picker is refused because the rows were never the cost.**
Delete every panel but the table and what survives is a frame loop, a thread, a
second seam, a process-global raw-mode flag and two crates. A view is roughly a
hundred lines against nearly two thousand lines of machinery. If the machinery is
what costs, the machinery is what the decision is about.

## Why the cost is the writing

The general rule, which is what outlives every particular panel:

> **A surface which writes at all pulls in locking, deferral, refusal and rollback.
> A surface that only reads and acts does not.**

Trace it once and the chain is visible. There is no save button on a live panel, so
a change is written when it is made, so every edit takes the exclusive lock and
gives it straight back — because holding it would make an open panel a denial of
service against `perch watcher run`, which takes the same lock every round. An edit
can therefore be refused, so a refusal has to be surfaced and the row rolled back
to what it was. Stepped values would otherwise be one write per keystroke, so they
are debounced, which means a pending state, which is a deferred write that has to be
distinguished from the save button it resembles. Names have no natural step, so a
text mode exists, and it exists exactly once and has to be argued for so it is not
argued about again.

None of that is gold-plating. Each piece is the smallest correct answer to a problem
the one before it created, and the chain begins at *the surface writes*. The kinds of
defect it produces — a key acting from a state nothing drew, a debounced write that
undid the next one — are frame-loop and pending-write bugs, a class that exists
nowhere else in Perch.

**Reversibility is not the axis.** A Setting is not irreversible: nothing one can be
set to destroys anything, the previous value is a keystroke away, no Credential
moves, and the worst a mistake costs is that a Cycle prefers the wrong Account until
somebody notices. The sound-looking rule *a surface may write what it can unwrite*
still reaches the wrong place, because it measures recoverability when the cost is in
the machinery. That word is retired here and everywhere; where a cohort needs
describing, the axis is reach (ADR one-door-to-the-registry).

## What follows

`perch config` is the only way to write a Setting, and `perch alias`,
`perch disable`, `perch enable` and `perch group` the only ways to write what else a
panel would reach. Nothing loses a form.

The `Host` port stays narrow. It is the only way out of the process, and a `Host`
that knew about frames would be one every test carried and every fake had to answer
for; nothing on the far side of a drawing call is a primitive like `mkdir`. Keeping
remarks back so they do not land in the middle of a frame is the shape of widening
the port has to be refused: the deduplication `Host::note` performs is not a
picker's and stays, and nothing beyond it is owed to one.

Nothing in Perch touches raw mode, and no terminal-drawing crate is in the dependency
set. Raw mode is process-global state, so two crates that each thought they owned it
would be a genuine hazard rather than a tidiness complaint — which is the reason a
drawing dependency would have to be named directly rather than left to a wrapper, and
the reason not having one is cheaper than getting it right.

`CONTEXT.md` has no entry for a drawn surface and gains none. A surface whose absence
changes not one word of the vocabulary is the clearest evidence available that it
carried no idea a person has to hold.

## Consequences

Every command reads its arguments, does its work and exits — a property worth stating
once rather than rediscovering, and the premise `perch run` and `perch watcher run`
both rest on when they hand the terminal to something else.

Perch's own writers of the Registry are the commands, and each of them holds the lock
for exactly as long as it needs it (ADR one-door-to-the-registry). There is no
surface holding it across a person's attention span.

`perch list --json` and the table it stands beside are two renderings that both have
to exist, because a person and a script are two readers and neither is served by the
other's shape (ADR the-listing-owns-the-set).

A future proposal for a drawn surface is not foreclosed, and this is what it has to
answer: name the moment, show that the moment is not one an argument here has already
ruled out, and account for the machinery rather than the view.
