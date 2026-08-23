# An ordering is a type

Six orderings inside the Watcher's round each cost a shipped bug, and each was a
comment:

| | what it rules |
|---|---|
| (a) | the Landing resolves before anything is read (ADR a-switch-is-written-down-first) |
| (b) | the Cooldown short-circuits before candidates are read |
| (c) | liveness is asked before the candidate Refresh burst |
| (d) | every way `refuse_if_live` can fail is answered, not just `ProfileLive` |
| (e) | the Check is remembered before the Switch is recorded |
| (f) | a Switch that moved and then failed raises, whatever the failure |

A rule provable only by driving a fake and reading back a sequence of bearer
tokens is a rule a refactor can quietly break.

## Ordering becomes arity

Each ordering is a value that only the ask which earns it can construct:
`Settled` from resolving a Landing, `Idle` from asking whether the outgoing
Profile is Live, `Cooled` from a `Crossed` figure and a spent Cooldown. The
steps that must come after them take them as arguments.

```rust
fn permitted(registry: &Registry, _: &Settled) -> Result<Watching>;                        // (a)
fn considered(registry: &Registry, w: &Watching, _: &Cooled, _: &Idle) -> Vec<Considered>; // (b)(c)
```

`permitted` cannot be called without a resolved Landing. The one funnel that
produces candidate addresses cannot be reached without both `Cooled` and `Idle`,
so a candidate Refresh inside a Cooldown, or before the liveness ask, does not
compile. (d) is `watch::refused_or_raised`, an exhaustive match over three named
variants of `switch::NotIdle` — a fourth breaks the build until the round says
whether it is a refusal or a raise.

Two of the six are not witnesses and do not need to be, because the Switch has
one door (ADR a-switch-is-written-down-first): the Check is recorded before the
Switch inside `record_the_switch`, and whether anything moved is a field *both*
ways out carry, so the round asks one question of both rather than reading the
answer off which way out it got.

Error modes are untouched by any of this. `Turn::NotArranged` carries
`PerchError` whole so a Check keeps exit 18 and 14, `NotIdle::Live` is
`Outcome::Refused`, and the other two raise with the code the failure earned.

## What a witness is

> **A witness is a value that exists only as proof that an ask was made:
> nothing to substitute, and nothing in it that the ask did not establish.**

`Settled` and `Idle` are empty. `Cooled` borrows the `Crossed` it was earned
from, because it is proof about *that* crossing rather than about crossings in
general — which is the one thing a witness may carry, and the reason `Crossed`
itself is not one: it holds the figure, and a value that holds the figure is a
reading rather than a proof.

A witness is only as honest as the ask that mints one, so there are exactly two
minters. The second is `switch::nothing_in_flight`, for the opening line, which
holds no registry lock and so has a Landing to *check* rather than one to
settle; it can only answer `None` where one is in flight, which is why it cannot
weaken what `Settled` means. Any third has to be argued for here.

**witness** stays out of `CONTEXT.md`, for the reason **Round** does: it is a
word about how this code is built rather than about what Perch does
(ADR code-lives-where-it-reaches). It is defined once in the source, at
`switch::Settled`, and every other use points at that.

## What is rejected, and why it matters

Three interfaces were designed independently before this one was chosen. Two of
them are live alternatives; the third is
ADR code-lives-where-it-reaches's.

**A step protocol.** The round becomes two entry points in `watch.rs` yielding
`Step`s — `SettleTheLanding`, `Refresh(addresses)`, `RefuseIfLive`, `SwitchTo`,
`Done` — which `commands/watch.rs` answers with the real calls. It is the
deepest of the three: the two yeses, the exit-code table, the coalescing and all
six orderings sit behind two methods, and it buys one genuinely new assertion —
drive `a_loop()` and `a_check()` over the same registry and prove the step
sequences differ in exactly three places, which is the *three arrangements, one
behavior* rule made checkable. It is refused because the answers are not typed
per step: replying to `Refresh` with what `SwitchTo` expects is a panic rather
than a compile error. A design whose purpose is eliminating runtime ordering
failures should not introduce one, and the locality it buys is locality a single
command already has.

That refusal is reopened below, against evidence it did not have.

**A typestate chain.** Seven states, each consuming the last, from `Round::opens`
through `Moving`. It gets the same compile-time ordering as the witnesses and
dissolves the `Watcher` enum. It is refused on its own usage: the Check is the
same twelve lines with two differences, so the sequence gets written out once per
arrangement — reintroducing at the Round exactly the
protocol-assembled-differently-by-two-callers that one door removed from the
Switch.

## The step protocol is reopened

One shape has outlived every answer given to it: a Watcher that acts after it
has lost the watch or been told to stop. ADR an-invariant-gets-a-door counts it
in five of the six reviews it sorted, and answers it with a door,
`commands::watch::Watch::goes_on`, which renews the hold and asks both questions
in one call. The eighth review and the ninth found it again, on the far side of
that door:

| what acted after the watch was gone | how it got past `goes_on` |
|---|---|
| the round's own Refresh | `observe::refresh` asks once per address and threw the first answer away; the round hands it one address, so the ask was never made |
| the refresh token a turn was holding | asked once at the top of a turn, and a turn is up to six requests bounded at thirty seconds |

That ADR states the limit itself: **`Watch` cannot make a step exist that
nobody wrote.** `goes_on` is called by hand at five sites, two of them
as a closure `observe::refresh` takes as
`&mut dyn FnMut() -> Result<(), Lost>`, so a step can hold the door, call it and
discard what it says. A door removes the site that forgot the rule. It does not
decide where the rule is asked, and both findings above are about where.

Putting every step through one driver is the design refused here, and the ask
becomes the driver's job rather than each step's — a step nobody has written yet
cannot skip an ask it never makes.

**The objection, and what answers it.** The refusal above is that the answers
are not typed per step: replying to `Refresh` with what `SwitchTo` expects is a
panic rather than a compile error, and a design whose purpose is ending runtime
ordering failures should not open one. That holds against a `Step` enum answered
by a shared reply type. It does not hold against a driver stated as a trait with
one method per step, where each answer's type is that method's return type and
no reply can be mismatched. That shape was not among the three designed before
this one was chosen, and it is what a reopening weighs.

**What it would also settle.** The consequences below record that *the three
arrangements differ in exactly three places* is a rule no test checks, and that
refusing the step protocol is what left it unproved. A driver makes it
checkable. That was the refused design's best argument and nothing has answered
it since.

**What the reopening has to decide.** Not whether the driver asks, but what it
drives. A driver that asks once per step answers the eighth review's finding and
not the ninth's, where the step is one Account's turn and the requests inside it
are six. Either the unit is the request, and the driver sits below
`observe::refresh` rather than above it, or the driver buys the ordering without
buying the granularity and the shape has somewhere left to live. The witnesses
are untouched either way: they rule what must come after what, and a driver
rules what every step passes through.

## Consequences

- Adding a step to the round means deciding what it must come after, because it
  will not compile otherwise.
- A new way for `refuse_if_live` to fail breaks the build until the round says
  whether it is a refusal or a raise.
- The `Fullest` that `Crossed` consumes has to be reached again for the decision
  line — the one piece of ceremony the witnesses cost. The arm that crossed
  nothing is handed the figure back; the two that crossed read it off the
  crossing, through `Crossed::fullest` either way, so the figure on the line and
  the figure the decision was taken on cannot come to differ.
- `perch watcher run` says nothing about which Account it is watching until the
  first round has settled the Landing. A registry read before that may hold a
  Landing, and during one `Active::whose` answers with the Account being *left*
  — the last thing Perch established rather than the thing that is true.
  Covered by a test, because an unasserted claim about what a command prints is
  one the next refactor is free to undo.
- *The three arrangements differ in exactly three places* is a rule no test
  checks. It was the step protocol's best argument, and refusing that design
  leaves it unproved. If a fourth difference ever appears, this is the document
  that failed to prevent it.
- The shape the witnesses were built against is still shipping defects, now
  through the door built beside them. Reopening the step protocol is the
  outstanding item this document owns.
