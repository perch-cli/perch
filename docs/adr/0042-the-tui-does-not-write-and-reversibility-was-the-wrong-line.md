# The TUI does not write, and reversibility was the wrong line

> **Superseded by ADR 0049.** Both halves of the decision below are void:
> `perch tui` is removed entire, so the tab this document repealed goes with the
> view it preserved, and the Config tab's removal never had to happen on its own.
> The reasoning is not what was found wrong — the machinery really is what the
> panel cost, and reversibility really was the wrong axis. It simply did not go
> far enough: the frame loop underneath both tabs is the same kind of cost, and
> nothing was found that wanted what it bought. One sentence here outlives the
> panel and is carried forward by 0049 rather than buried with it — "a surface
> which writes at all pulls in locking, deferral, refusal and rollback, and a
> surface that only reads and acts does not". ADR 0034 stays superseded by this
> document; a chain is fine, and the route is worth as much as the destination.

ADR 0034 moved a line. ADR 0011 had said `perch tui` acts on exactly two things,
a Switch and a Run, because "a keystroke away from an irreversible act is the
wrong ergonomics for the one surface being navigated by arrow key" — and 0034
observed, correctly, that a Setting is not irreversible. Nothing one can be set
to destroys anything. So the line moved to where that reasoning pointed: the TUI
may write what it can unwrite.

The observation was right and the conclusion is repealed. The Config tab and
every mechanism ADR 0034 built to make its writes safe are to be removed, and
`perch tui` goes back to the two acts ADR 0011 gave it.

The reason is not that 0034's argument was refuted. It is that reversibility was
never the axis the cost sat on.

## One command, two products

`src/tui/model.rs` says it in its own doc comment, about the tabs it defines:
"'where am I' is a different question from 'what may I change', and a single
page answering both answers neither well."

That is two subsystems sharing a frame loop. The Status tab is ADR 0011's job —
accounts ranked by headroom, group by group, chosen between by eye. The Config
tab is ADR 0034's — settings per Scope, with `Inherit`, a debounce, a lock taken
and given back per edit, a refusal that rolls a row back, and the only place in
Perch that accepts typed input. They share no data, no verbs and no failure
modes.

Judged together they get a verdict neither earns: *the picker is worth having,
therefore the TUI stays* smuggles the panel past the yardstick without ever
weighing it. Judged apart, they separate cleanly, and the seam is visible in
every measure the repo keeps.

Of the 8,323 lines behind `perch tui` — 5,604 in `src/tui/` and 2,719 in
`tests/browsing.rs`, 11.5% of this repository — the Config tab is 3,292,
arriving in a single commit. It is 40% of the cost of the command and none of
the reason the command exists.

## Why the cost is the writing

The panel's weight is not the settings it shows. It is everything ADR 0034 had
to build so that showing them could also mean changing them.

There is no save button, so a change is written when it is made, so every edit
takes the exclusive lock and gives it straight back — because holding it would
make an open TUI a denial of service against `perch watch`. An edit can
therefore be refused, so a refusal has to be surfaced and the row rolled back to
what it was. Stepped values would otherwise be one write per keystroke, so they
are debounced, which means `Pending`, which means a deferred write that has to
be distinguished in the model from a save button. Names have no natural step, so
there is a text mode, which exists exactly once and had to be argued for in the
ADR so it would not be argued about again.

None of that is gold-plating. Each piece is the smallest correct answer to a
problem the one before it created, and the chain begins at *the panel writes*.

The defect record agrees. Fifteen of this repo's sixteen deep reviews found
something in the TUI, which on its own says only that it is 11.5% of the code —
its churn is 11.6% of all lines this repo has ever added, almost exactly
proportional. What is not proportional is the *kind*. "Two TUI keys acting from
a state nothing drew." "A debounced write that undid the next one." Those are
frame-loop and pending-write bugs, a class that exists nowhere else in Perch,
and both are on the Config tab's side of the seam. The Status tab's entire state
is which row the cursor is on.

So the line that holds is not about what a write can be taken back from. It is
that a surface which writes at all pulls in locking, deferral, refusal and
rollback, and a surface that only reads and acts does not. **The TUI does not
write.**

## Why there is no read-only Config tab

The tempting middle is to keep the tab and take the keys away: still show what
each Scope resolves to and where an Override sits, just never change anything.
It is refused, and this time the reason is not the one ADR 0041 gave.

There, the middle option kept most of the vocabulary for a fraction of the
coverage. Here it would not even do that much, because the reading it offers
already exists twice over. `StatusRow::Config` on the *other* tab renders the
governing configuration read-only today. The Config tab's own `Plan` and
`Quarantine` rows show facts `render_accounts` already draws. And `perch config`
with no Scope prints the same thing in one line, from a script, over SSH.

A read-only Config tab would keep the sidebar, the three-column navigation and
the `Scope`-and-`Row` model — most of the navigation cost — to duplicate a view
that is already on screen. If the writing is what justifies the tab and the
writing is what costs, the tab goes with the writing.

## What ADR 0034 keeps

One thing, and it is kept on merits rather than by inheritance.

`perch config unset` was grown by ADR 0034 so that "the panel cannot reach a
state a script cannot" — clearing an Override back to Inherit. With no panel,
that justification is gone. The verb stays anyway, in both the two-word and
three-word forms.

`Inherit` is a real state in the glossary, distinct from holding an Override
that happens to equal Global's — one tracks and the other does not. A two-layer
configuration with no way back to the upper layer's absence is incomplete no
matter who is doing the clearing. It is recorded here so that a future reader
tidying up after the panel does not remove it as debris.

This is worth naming for its own sake: the constraint ADR 0011 imposed —
that every interactive capability exist non-interactively too — improved the
CLI in the one instance where it bit. That is a point in the constraint's
favor, and it survives the panel it was applied to.

## ADR 0011 is re-affirmed, not rewritten

Its text stands unamended. `perch tui` acts on a Switch and a Run; `add`,
`remove`, `purge` and `config` stay out; every capability has a plain command
form.

Its *reason* is corrected here rather than there, because the episode is worth
keeping. ADR 0034 made a sound argument — a Setting really is not irreversible
— and still reached the wrong place, because it was measuring recoverability
when the cost was in the machinery. Editing that out of 0011 would erase the
fact that the line was tested. The route is worth as much as the destination.

## What this does not decide

Whether `perch tui` should exist at all.

That question is left open deliberately and tracked at #151, blocked by #147.
It cannot be answered the way this one was. Whether the Config tab earned its
lines is legible in the source — it duplicated four commands' write surface for
a once-ever setup convenience, and the other tab already drew everything
readable. Whether a person wants to choose an account by eye is not legible
anywhere in this repository. It is a fact about using Perch, and nobody has yet.

Recording the boundary honestly: this ADR removes 3,292 lines from a command
whose remaining 5,000 rest on an untested premise. That is not a reason to wait.
The panel's verdict does not depend on the picker's, and an excision that is
later subsumed by a deletion has cost nothing.

## Consequences

`perch config` becomes the only way to write a Setting, and `perch alias`,
`perch enable`, `perch disable` and `perch group` the only ways to write what
else the panel reached. Nothing loses a form; every one of these was already the
command the panel stood in front of.

The registry goes back to having one writer among Perch's interactive surfaces.
ADR 0034 noted that the TUI was the second thing that could lose a lock mid-act;
it stops being that.

`perch tui` is one view, so the tab bar goes with the tab, and `Tab`, `ScopeRow`,
`Row`, `Edit`, `Pending`, `Prompt` and `Column`'s third value go with it.
`registry::save`'s refusal against a lost hold is untouched — it was correct
before the panel and stays correct after.

`CONTEXT.md` is unchanged. The TUI has never had a glossary entry and does not
gain or lose one here; `Override`, `Inherit` and `Scope` are configuration's
vocabulary and were never the panel's.

Releases #145 — what shape the command surface should be — from the TUI's hold
on it, though it stays blocked on #144 until the Watcher's knobs are settled.
What the TUI imposed on that question was the write surface: the obligation that
every settable thing be settable by arrow key. A picker that only Switches and
Runs adds no capability to reach, so #145 is free of this whether or not #151
keeps it, and #151 does not block it.
