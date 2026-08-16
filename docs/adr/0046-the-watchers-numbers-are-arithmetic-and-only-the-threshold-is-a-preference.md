# The Watcher's numbers are arithmetic, and only the Threshold is a preference

> **Carried out in #161.** Like ADR 0041, ADR 0042, ADR 0044 and ADR 0045, this
> is the artifact of a planning effort rather than of a change, so it landed
> ahead of the work it describes instead of beside it. The tree now matches it:
> `perch config` carries three Settings, the cooldown and the margin are
> constants in `src/watch.rs`, and the three glossary entries read as below.

The question arrived as a count: `perch config` exposes six Settings, five of
them Watcher tuning, and `CONTEXT.md` spends four pacing concepts explaining a
loop nobody has run in anger. Does the Watcher want that much surface?

The count is wrong in a way that changes the answer. **Back-off has no knob at
all** — ADR 0013 fixed it as arithmetic about Anthropic's allowance and made it
nobody's to configure. **`watcher-may-act` is not a pace**; it answers *may you*,
not *how often*. So the thing under indictment is not five knobs governing four
concepts. It is **four pacing knobs, one of which cannot act**, beside a
permission that was never in question.

Once each is judged on its own, the shape falls out cleanly: one dies because it
has no effect, two become constants because they are arithmetic rather than
taste, and one survives as the only genuine preference in the loop.

## The knob that cannot act

ADR 0013 conceded that `watcher-no-return` "changes no trace the watcher can be
shown today", and kept it anyway. The code is stronger than that concession, and
the difference matters.

`commands/watch.rs:659` short-circuits the round into `Outcome::Cooling` the
moment `Recently::resting` returns `Some` — *before* `act` runs.
`Recently::barred` is consulted only at `commands/watch.rs:804`, inside `act`.
Both are computed from the same `left_of_the_cooldown`. So `barred` is only ever
asked in the branch where that function has already returned `None`, which makes
`barred` return `None`. Always, and by construction rather than by coincidence.

This is not "the cooldown usually gets there first". There is no arrangement of
Settings, and no sequence of rounds, in which `watcher-no-return` changes what
Perch does.

What it costs is not one field. A `Setting` variant, a `Settings` field, an
`Overrides` field, a `Policy` field, `Recently::barred` under twenty lines of doc
arguing for its own existence, a `barred` parameter threaded through `act`,
`worth_reading` and `considered` and into the sentence explaining why an Account
was passed over, **four** branches of advice in `commands/config.rs:811-865` —
the `watcher-cooldown-minutes 0` branch exists only to warn about it — a row in
the guide's table, a paragraph of guide prose, unit tests, and the second
sentence of **Cooldown** in `CONTEXT.md`.

And one of those four sentences is false. `commands/config.rs:860` tells a person
who has turned it off that "Only the cooldown and the margin then stand between
two Accounts either side of the threshold and a ping-pong" — asserting a
behavioural difference that does not exist. A Setting that prints a lie about
itself is the clearest possible failure of the yardstick this decision is taken
by.

**So it goes entire: the knob, the field, `Recently::barred`, the threading and
all four sentences.**

ADR 0013's defence was forward-looking — "a rule nobody wrote down is one that
gets relaxed by accident the first time the other one is". `CLAUDE.md` keeps
guards of that kind where they cost nothing, and this one does not. The guard is
preserved here instead, at no surface at all:

> **If the cooldown ever stops gating Switches outright — becoming per-Account,
> or pacing rather than barring — a no-return has to come back.** It is absent
> because the cooldown subsumes it, not because returning immediately is
> acceptable.

## The margin is not hysteresis

`CONTEXT.md` says the Margin is "what stops two Accounts either side of the
Threshold from trading places every few minutes", and ADR 0013 says the same. It
is the wrong description, and the right one changes what the Margin is for.

Usage on the Account you are on climbs. Usage on the Accounts you are not on does
not. Two Accounts therefore do not trade places — **they walk upward together**.
At `watcher-margin-percent 0` with a threshold of 80: A is at 80 and B at 79, so
you are moved to B; you burn B to 80; one cooldown later you are moved back to A,
still at 80. That repeats every cooldown, and each move lands you on an Account
with almost nothing left. The cooldown sets how often the pointless move happens
and never stops it happening. Only the Margin does, by turning the round into
`Exhausted` — exit `17` — which is the true answer: there is nowhere better to go.

The low end of the range is worse than merely tight. The two predicates are
asymmetric — `Fullest::at_or_over` leaves on `used_percent >= threshold`
(`watch.rs:404`), while a candidate is set aside on `used_percent > ceiling`
(`watch.rs:626`). At margin 0 the ceiling *is* the threshold, so an Account at
exactly 80% is simultaneously full enough to leave and clear enough to arrive at.
The range permits a contradiction.

**The Margin becomes a constant of 10 points, expressed relative to the
Threshold.** Refusing a destination nearly as full as the Account being left is
not a preference — nobody wants the low end, and the high end is already reachable
by moving the Threshold. Relative rather than absolute, because "10 points under
the Threshold" tracks the Threshold as it changes, where a fixed 70 would quietly
stop meaning anything the first time somebody set the Threshold to 60.

Collapsing the Margin into the Threshold was considered and refused. Making the
rule symmetric — *you are moved off above the line and never moved onto anything
above it* — is one number with one meaning in both directions, and it repairs the
`>` / `>=` asymmetry by construction. It is also exactly margin 0 with the
comparison tightened, so it re-buys the walking-upward problem the Margin exists
to solve. The concept has to survive. Only the knob has to go.

## The cooldown fails ADR 0013's own test

ADR 0013 justified making the two-and-a-half-minute interval a constant on a
clean test: it "is derived from Anthropic's allowance rather than from anyone's
preference". It then justified the cooldown's fifteen minutes like this:

> A five-hour window moves slowly enough that fifteen minutes never misses a real
> crossing.

That is arithmetic about how fast a Quota Window moves. It is the same *kind* of
argument, and it produced the opposite disposition — a per-Scope Setting with a
range up to `10080`, where a cooldown of a week against a five-hour window is a
way of spelling "do not watch". One record, two decisions, one test, two answers.

**The cooldown becomes a constant of 15 minutes.** With the Margin gating the
quality of the destination, what is left for the cooldown to express is only how
promptly a person is moved after a real crossing, and that has a right answer
rather than a taste. `MAX_WATCHER_COOLDOWN_MINUTES` and `registry::a_cooldown`
retire with it.

**None of this touches where the cooldown is kept.** ADR 0013's argument that a
cooldown belongs to the Watcher rather than to the machine — the loop carrying it
in memory and forgetting it when it stops, a `--once` Check recording it against
its Group because there is no loop for it to live in — is untouched and still
governing. `Recently`, `Checked` and the memory-versus-registry split stay exactly
as they are. A constant still has to be paced across processes.

## The Threshold is the one that survives

`watcher-threshold-percent` is the only one of the four that passes ADR 0013's
preference-versus-arithmetic test cleanly. How full is too full cannot be derived
from Anthropic's allowance or from the length of a window. Someone who never
wants to hit a wall mid-task sets 60; someone squeezing every drop sets 95; both
are coherent, and nothing in the endpoint's behaviour prefers either. It is the
one place a person's appetite for risk enters the loop, and it is kept for that
reason rather than by default.

It keeps its `watcher-` prefix. The prefix no longer disambiguates it from four
siblings, but it still groups the two Settings the Watcher owns and still
separates a Watcher rule from `strategy`, which governs Cycles nobody is
watching.

## What this does not decide

**`watcher-may-act`.** A grant, not a pace, and load-bearing for ADR 0002's
"nothing changes underneath you unless you said it could", for the asymmetric
non-inheritance into the Ungrouped Scope, and for the `14` hold in ADR 0040. It
is deliberately re-affirmed and deliberately not judged by this ADR's yardstick:
measuring a permission by how much tuning it offers is a category error.

**`strategy`.** It governs every Cycle, including the one a bare `perch switch`
performs with nobody watching, so it is not Watcher tuning and the same category
error applies.

**Whether five nouns earn their keep for three Settings.** This ADR produces the
count and stops there. Global, Scope, Override, Ungrouped and Group are untouched,
and `watcher-threshold-percent` stays Overridable per Scope exactly as it is — a
work Group wanting a different threshold from a personal one is the strongest
remaining case for Overrides, which makes it that question's best evidence rather
than a side-effect to spend here.

## The glossary

Three concepts survive with no knob behind them, and all three keep their entry.
Being unsettable is not being invisible: a person still meets these words in a
`Cooling` line and in a held round, and a term you meet but cannot look up is
worse conceptual surface than one you can. **Back-off** has been the proof of that
all along.

**Margin** keeps its first sentence and loses its second. What replaces it is what
the Margin actually does: refuse a destination nearly as full as the Account being
left. The old sentence described a failure mode that does not occur.

**Cooldown** loses the no-return sentence, which is dead. Its third sentence — the
loop's memory against the Check's record — is untouched.

**Back-off** loses one clause. Its entry distinguishes itself with two contrasts:
*"a Cooldown paces Switches the Watcher may make **and is the Group's to set**,
and a Back-off paces questions nobody is answering and is arithmetic about
Anthropic's allowance."* Making the Cooldown a constant makes the second contrast
false — both are now arithmetic and neither is anyone's to set. The first survives
whole and is enough: **a Cooldown paces Switches the Watcher makes, a Back-off
paces questions nobody is answering.** Acting against asking.

No entry is added, and none is demoted into prose.

## Consequences

`perch config` goes from **six Settings to three**: `strategy`,
`watcher-may-act`, `watcher-threshold-percent`, beside Global-only
`cycle-ungrouped`. The Watcher's four pacing knobs become one.

**196 lines across nine files name the three departing Settings** — 44 in
`registry.rs`, 56 in `commands/config.rs`, 33 in `tests/configuring.rs`, 22 in
`watch.rs`, and the rest in `commands/watch.rs`, `tui/model.rs`,
`tests/browsing.rs`, `tests/watching.rs` and the guide. That is a floor rather
than the removal: it counts lines that mention a name, not the doc comments,
validation ranges and advisory branches that go with them.

The removal is sequenced **after ADR 0042's Config tab**. `src/tui/model.rs` and
`tests/browsing.rs` both reach these Settings, and both sit inside the 3,292 lines
that decision already condemned. Editing code slated for deletion is work done
twice.

The reduction is honest about where it comes from. **All four pacing concepts
survive.** Threshold, Margin, Cooldown and Back-off are each still an idea, and
no-return was never a term of its own — it lived inside **Cooldown**'s entry. What
changes is settability: three concepts move from *something a person decides* to
*something Perch does*, off the configuration table, out of `perch config get`,
and out of the interaction a person has to reason about when setting two of them
against each other. That is the reduction, and it is smaller than deleting an idea
and larger than deleting a line.

**This supersedes the whole of ADR 0013's "Amended: the numbers this asked for"
section, and nothing else in that record.** That section is the part that has
decayed, and it is cleanly severable. Everything above it stands and is still
governing: the rejection of a managed daemon, polling only the active Account,
the two-and-a-half-minute interval and why it is a constant, never acting on a
figure it did not just refresh, the Margin setting candidates aside before the
Strategy ranks them, the cooldown living in the loop and a `--once` Check
recording it, never acting on an ungrouped Account, both grants read every round,
and the whole exit-code table.

Superseding ADR 0013 whole was refused. It would restate a mostly-correct record
at length, and a third correction header on one file is the thing this map's
yardstick exists to refuse: a reader would need three passes to learn what the
Watcher does. Superseding the amendment alone puts the Watcher's numbers in one
place — the interval, the back-off, the cooldown, the margin, the threshold, and
which single one of them is anyone's to set.

No exit code changes, and no new one is added. `Cooling` still says why nothing
moved; it stops calling the wait "this Group's cooldown", because it is no longer
the Group's.
