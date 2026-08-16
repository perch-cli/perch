# Perch does not draw, and the ranking belongs to the listing

ADR 0042 cut the Config tab and left the picker standing, saying in as many
words that whether `perch tui` should exist at all was not decidable from the
source: "whether a person wants to choose an account by eye is not legible
anywhere in this repository."

That was true of the *want*. It was not true of the *reason*, and the reason is
what this document finds missing. `perch tui` is removed entire — all 8,418
lines of it — and the one thing it uniquely rendered moves to `perch list`.

## ADR 0011 argued the picker's moment away

ADR 0011 considered making bare `perch switch` open the picker and refused it:

> It shows the evidence before spending quota, but it **costs an interaction at
> exactly the moment the user wants none**, and anyone rotating between
> subscriptions would type the unattended form every time.

Three lines earlier, the same document gives the picker a command of its own,
"for when the choice wants making by eye rather than by rule."

That moment is never named. Every moment ADR 0011 *does* name — quota running
out, mid-task, under mild frustration — is the one it has just ruled the picker
out of. The Considered Options establish that the switching moment wants no
interaction; the decision then provides an interactive surface for it anyway, on
the strength of a use the document declines to describe.

This is not the argument being re-litigated with new information. It is the
argument reaching two conclusions that cannot both hold, and only one of them
was carried into the code.

## The picker is the only subsystem that widened the `Host` port

```
src/host/mod.rs:548   fn print_remarks(&self, aloud: bool);
src/host/mod.rs:552   fn remarks(&self) -> Vec<String>;
```

Both exist for `perch tui`. Nothing else calls either, and `mod.rs` says so
itself: "`perch tui` turns it off while it owns the screen and back on when it
gives it back, and nothing else calls it." They sit on the port rather than on
`RealHost` for a stated reason — "it is the picker that has to say it, and the
picker holds a `&dyn Host`" — and they cost a `Cell<bool>` on `RealHost`, a
second constructor in `keeping_its_remarks`, a branch inside `note`, and two
stub implementations on `FakeHost` that every non-drawing test carries.

ADR 0016 was careful about exactly this. It put the terminal behind `tui::Screen`
rather than on the port, because "a `Host` that knew about frames would be one
every non-TUI test carried and every fake had to answer for." The reasoning was
right and the line still moved: `Host::print_remarks` is described in that same
ADR as "the one thing about frames the port does know."

The port is what this repository has been least willing to disturb, and the
picker is the only subsystem that has made it wider. That is not a bug — the
remarks really would have landed in the middle of a frame — but it is a cost,
and it is charged to the one seam nothing else was allowed to charge.

## Why removal is clean rather than merely large

Three measurements, taken because a subsystem this size usually leaves a hole.

**Nothing has to migrate.** `perch tui` takes no arguments at all: a Target,
`--json`, `--refresh` and `--group` are each refused, and `main.rs` explains why
— "a `--json` here would be `perch list --json`, which is the command ADR 0011
requires to exist anyway." The constraint ADR 0011 imposed, that every
interactive capability exist non-interactively too, means the picker was never
allowed to become the only route to anything. It was held to that, and the
receipt is that removing it removes a name and no surface.

**`CONTEXT.md` does not move.** The picker has never had a glossary entry, and
searching 497 lines for *tui*, *interactive*, *picker* or *arrow key* finds one
match: the word "interactive" in the **Attended** entry's `_Avoid_` list, which
is Dogfood vocabulary ADR 0041 is deleting anyway. An 8,418-line subsystem whose
removal changes not one word of the vocabulary is the clearest evidence
available that it carried no idea a person had to hold.

**The figures.** `src/tui/` is 5,604 lines, `src/commands/tui.rs` 95, and
`tests/browsing.rs` 2,719 — **8,418 in all, 11.6% of this repository**. ADR
0042's figure of 8,323 missed the command module. Of that, 3,292 is the Config
tab #150 was already going to take, so the increment this decision adds is
**5,126 lines**, and #150 stops being work anybody has to do.

## Why there is no middle

The Status tab has three sidebar rows and only one of them is the picker.
`render_overview` is roughly 116 lines of view and `render_governing` 70,
against `render_accounts`' 57 — and by ADR 0042's own test, those two are
`perch status` and `perch config` drawn with box-drawing characters. ADR 0042
refused a read-only Config tab because "the reading it offers already exists
twice over," and cited `StatusRow::Config` on the Status tab as one of the two
places it already existed. That test, turned around, indicts the row doing the
citing.

So a smaller picker is available: delete both rows, keep the table, and
`perch tui` becomes one listing with a cursor and two keys.

It is refused, because the rows were never the cost. `terminal.rs` (466),
`refresh.rs` (332), `mod.rs` (530), `act.rs` (213) and `fake.rs` (227) survive a
row deletion untouched, and so do both crates and the pinning note that keeps
one `crossterm` in the tree. The middle saves about 190 lines of view and keeps
roughly 1,800 lines of frame loop, a thread, a second seam and a process-global
raw-mode flag. That is ADR 0042's refused read-only tab in different clothes,
and it is refused for the same reason: if the machinery is what costs, the
machinery is what the decision is about.

What the seam does establish is that the picker was already less than it looked.
Two of its three rows were duplicating commands before anything was decided
here.

## The ranking belongs to the listing

ADR 0011's stated justification — "a read-only picker is `perch list` with
box-drawing characters, so the whole justification for it is making a choice" —
is false today, and in the picker's favour. `perch list` prints four columns in
registry order. The picker's Accounts view prints the same listing **ordered as
a Cycle ranks it, with the Headroom that order was made on beside it**, and
renders Ungrouped Accounts as *held* rather than ranked (ADR 0017). `list.rs`
knows this: `widths` is generalised over N columns rather than written over its
own four, "because the TUI's Accounts view shows the same listing with the
figure its order was made on beside it."

So there is one thing in Perch that only the picker does, and ADR 0011 named the
job correctly: the ranking `perch switch` makes should be visible rather than
hidden, "so the two surfaces cannot come to disagree about which account is
better."

That job survives its surface. `perch list` gains all three pieces —
the Cycle ordering, a **Headroom** column distinct from the Utilization it
already prints, and the held-rather-than-ranked rendering for the Ungrouped.

**Always, and not behind a flag.** The argument was never that the ranking
should be *available*; it was that the two surfaces must not disagree, and an
optional agreement is not one. The held-versus-ranked distinction carries the
most weight of the three: a ranking of Accounts Perch would refuse to choose
between is the hidden claim ADR 0011 built this listing to prevent, and it is
just as false in a plain-text table as in a drawn one.

**It lands with the removal, in one change.** Separated, there is a window in
which nothing in Perch shows the Cycle's judgment — which is the disagreement
this whole thread exists to prevent, arrived at by scheduling.

## What ADR 0016 keeps, and what ADR 0042 leaves behind

ADR 0016 is three decisions in one file and only two of them are the picker's.
Its ratatui-over-crossterm choice and its second Amended section — `tui::Screen`,
the Refresh thread's own Host, `Host::print_remarks` — are superseded here. **Its
colour-eyre repeal and the two-error-idiom rule stand**: expected failures are
`PerchError` carrying an exit code a script reads, unexpected ones are panics
through the twelve-line hook in `report.rs`, and anything that starts as a panic
and turns out to be an outcome moves across. None of that was ever about drawing,
and superseding the file whole would have taken `report.rs`'s charter with it.

ADR 0042 is superseded in full — both halves of its decision are void, since the
tab it removed goes with the view it preserved — but one sentence is carried
forward rather than buried, because it generalised past the thing it was written
about:

> a surface which writes at all pulls in locking, deferral, refusal and
> rollback, and a surface that only reads and acts does not.

That rule outlives the panel. ADR 0034 stays superseded by 0042; a chain is
fine, and the route is worth as much as the destination.

**ADR 0025 is not amended.** Its "Reopened by `perch tui`" section asked whether
crossterm's arrival should take over the export passphrase read or terminal
detection, and answered no to both. Crossterm leaving closes the reopening
without moving a line, because the answer was already that both stay where they
are. The section becomes history rather than a live tension.

**`unicode-width` stays declared**, with its justification rewritten. It is
there today on the strength of being free — "already in the build transitively —
ratatui measures every frame with it — so declaring it adds no crate" — and that
sentence stops being true. The decision survives anyway on ADR 0025's actual
test, which is whether a crate costs a seam: the width of a string is a pure
function and sits on nothing. The comment's next sentence was always the real
argument and is untouched — "a hand-rolled table of East Asian widths would be
the same data, kept by hand, going wrong quietly" — and `perch list` is about to
grow a column, so it needs the measuring more rather than less.

## Considered Options

**Keeping it, deliberately.** The map this decision belongs to holds that "keep
exactly as is" is a valid resolution and that a re-affirmed choice is worth more
than a churned one. It is refused here not because the picker is large but
because nothing was found that wanted it: the ADR that built it disqualified its
own moment, the glossary never learned its name, and its only unique output
moves for the price of a sort and a column.

**Waiting for real use.** This was the plan of record — the question was tracked
as the one purely empirical item on the map, blocked on a ticket that has not
been worked. It is decided ahead of that deliberately, on grounds that do not
need a machine. See the limit below.

**A picker that is only a picker.** Refused above: the frame loop is the cost.

## Consequences

`perch switch` and `perch switch <target>` are the whole of choosing, and
`perch list` is the whole of looking. **Perch has no interactive surface left**
— every command reads its arguments, does its work and exits, which is a
property worth stating once rather than rediscovering.

The `Host` port narrows. `print_remarks`, `remarks`, `RealHost::keeping_its_remarks`,
the `aloud` cell and the branch in `note` all go; the deduplication `note`
performs is not the picker's and stays. This is the port getting *smaller*,
which is the only direction the map allowed it to move.

`ratatui` and `crossterm` leave the dependency set, and with them the note
explaining why crossterm is named directly — raw mode is process-global state
and two of them would each think they owned it. Nothing in Perch touches raw
mode afterwards.

The command surface loses a name. ADR 0047 is **not** amended: one command
leaving says nothing about how the remaining ones are placed, and its finding
stands unchanged. Its arithmetic does move — the sweep it describes becomes
**18 names to 15**, and its forms 26 to 27.

`docs/guide/tui.md` goes, with its row in the guide's table of contents and its
entry in the reference. ADR 0045 counted eighteen guide rows against nineteen
commands; that pairing is unaffected, since the page removed here is the one
command that had a page to itself.

`CONTEXT.md` is unchanged.
