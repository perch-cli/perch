# A Reserve attaches where a heading names its Scope, and the table has none

The **Reserve** — how many of a Scope's Accounts still have Headroom and how
much the best of them has — was written for the Utilization tab and outlived it.
ADR perch-does-not-draw removed the tab and `perch list` took the tab's Headroom
column; e5e6c6f deleted `src/reserve.rs` because nothing called it any more, and
left
#197 open on whether the listing should take the saying up too.

It should, and it attaches in one place: **where a heading has already named the
Scope the sentence is about.**

## The listing does not group, and its reason is the constraint

The issue's own body says the listing "already groups: writes a heading per
Group". It does not. `Scope::heading` returns nothing for a bare `perch list`,
which renders as **one flat table** across every Scope with the Group as a
column — and the reason is written into `render_human`: a table that broke for a
heading every few rows would put a blank line between Accounts the eye is
running down a column of.

So a Reserve line under a bare listing would have to name its own Scope, once per
Scope, in the footer. That is a heading smuggled into a sentence that is already
as wide as a terminal is — the worse half of the shape the table declined, with
none of what makes a heading readable.

Narrowed — `perch list <group>`, or `perch list ungrouped` — the heading is
there and has said it. The footer already holds the listing's other Scope-level
sentence, the `Cycling …` clause, which is precisely a sentence about the whole
narrowed set.

> **The Reserve is said under a narrowed listing and nowhere else on the human
> surface.**

Two other facts, found while looking, back this rather than being needed for it.

**The Reserve never faced a listing of every Scope at once.** In the tab it was a
detail panel under *one* Account, drawn for that Account's own Scope under the
caption `Across <scope>:` and gated on `cycle::may_cycle_within`. The narrowing
it always had was the reader's cursor.

**The Ungrouped stay silent until `interchangeable` is declared.** Not a special
case: `cycle::may_cycle_within` is the same gate the section's `ranked` already
goes through, and ADR a-group-is-a-declaration already says what it means. A
Reserve is what a set of Accounts has *between them*, which is the claim nobody
has made about the Ungrouped until they make it. Ranking them and saying what
they have left are the two things every surface declines together, so they are
declined off one answer rather than two.

## `--json` carries it at every breadth, and that is not a disagreement

The section document already names its own Scope in a key. So a bare
`perch list --json` carries a `reserve` per section while the bare table carries
none.

This is a divergence and it is deliberate. **The table's silence is a rendering
constraint, not a domain one.** The same document already spells out a fact the
table leaves implicit: `order` is an explicit key, and the table expresses it
only by not sorting. ADR the-listing-owns-the-set set the rule that put it there
— a document says what its order is, or it does not have one — and
ADR perch-says-what-it-did's line is the same one from the other side: a machine
reading a shape is not a person reading a sentence.

What ADR the-listing-owns-the-set forbids is the two surfaces **disagreeing
about a judgment**: a ranking on one that the other would not make. Neither
surface here claims anything the other denies. One of them declines to say a
true thing for want of somewhere to put it.

Three shapes follow from the same reasoning:

- **Fields, not the sentence.** The listing's document is structured throughout,
  and a prose sentence in a document is a thing scripts end up regexing.
- **`null` where `cycle::may_cycle_within` is false**, saying in the document the
  same "there is no answer here" the human surface says by silence.
- **`null` for a Scope holding nobody.** Not ambiguous against the first, because
  `accounts` sits beside it: an empty array distinguishes "nobody is here" from
  "nobody has declared these a set."

## An empty Scope keeps the sentence it already has

`render_human` returns early with "The Group `spare` holds no Accounts yet."
before it reaches the footer, and that is the better sentence — it names the
Group and says the one thing true of it.

The recovered module carried a branch for this case, `Reserve: none — no Account
is in this Group yet`, and it is **deleted rather than left unreachable**. The
branch beneath it — no Account here may be Cycled to — is what an empty Scope
would otherwise fall into, describing a Scope somebody has only just declared as
full of Accounts nothing may reach.

## The per-window rows are dropped

The recovered module said a Scope two ways. The second was one row per Quota
Window kind: the emptiest Account in that window, and how many were read for it —
what answers "fine on the weekly window, empty on the five-hour one".

In the tab, those rows summarized Accounts you could not see; the panel showed
one Account and the rows spoke for the rest. **`perch list` already prints every
Account's per-window Utilization figures**, so on this surface each row would
summarize numbers already on screen a few lines above where the row would sit.

That is the whole argument, and it is about this surface rather than about the
idea. `window_lines`, `window_kinds` and their four tests are recoverable from
e5e6c6f if the case is ever made on its own terms.

## `out_of_the_running` is exported again

e5e6c6f made `cycle::out_of_the_running` private, arguing that a seam with zero
callers outside its module is one more thing a reader has to rule out. That
argument was against *zero*; one caller is the ordinary case, and it is the same
one it had before. The refusal that nobody can be Cycled to and the Reserve that
says what is out of the running are the same count of the same Accounts, which is
why it was shared in the first place.

## Considered Options

**A Reserve line per Scope under the bare listing.** Refused above: it names its
own Scope in the sentence, which is the heading the table declined, badly. It is
also the option that cannot be walked back once scripts and screenshots have seen
it — narrowed-only is the subset of it, and can be widened later without
overturning the reason written into `render_human`.

**Breaking the bare listing into per-section tables with headings.** The honest
way to have Reserve lines everywhere, and a much larger change than the one #197
asks for: it overturns `render_human`'s own reason, and the column of Accounts
the eye runs down is what it would spend.

**A Reserve on `perch status`.** Structurally the panel the Reserve came from,
and refused: a Reserve is a fact about a set, and ADR the-listing-owns-the-set
removed `status --group` for being the listing wearing a flag. An
`Across <scope>:` block there is that flag returning without its name.

**Leaving it unimplemented** — the `won't do` the issue offers. The per-Account
**State** and **Headroom** columns do say enough for one person on one machine.
It is refused because the count is the one thing they cannot say: "two of these
three are worth switching to" is a fact about the set, and reading it off the
column is arithmetic the reader does rather than a sentence Perch says.

## Consequences

**No new surface.** No argv, no flag, no exit code changes
(ADR the-binary-proves-its-surface), so nothing new in the binary-driving suite.
`perch list` gains a footer line at a breadth it already had.

**The listing's footer is collected before it is written**, so the blank line
that separates it from the table is decided by whether there is anything down
there at all. It was decided by a condition naming three of the four sentences,
and the `Cycling …` clause was not among them — so `perch list ungrouped` on a
machine whose active Account is elsewhere printed that clause hard against the
last row. A fourth sentence joining that condition would have been the third
occasion to remember to.

**`CONTEXT.md`'s Reserve says Scope rather than Group**, matching how
`Reserve::of` is actually typed — it takes a `registry::Scope`, so it already
serves the Ungrouped — and matching the case the gate admits.

**`src/reserve.rs` returns recovered rather than rewritten**, six of its eleven
tests with it: four go with the per-window rows and one with the empty-Scope
branch. Its own rules stay unit tests there; the attachment rules go to
`tests/listing.rs`, where a sentence appearing under a table is already asserted.

**ADR the-listing-owns-the-set gains a reader rather than an amendment.** Its
ban is on the two surfaces disagreeing about a judgment, and neither of these
contradicts the other.

**The switching page is untouched.** Its exit-15 message already points at
`perch list <group> --refresh`; that pointer simply answers more than it did.
