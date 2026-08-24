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

Error modes are untouched by any of this. `Verdict::NotArranged` carries
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
six orderings sit behind two methods. It buys no assertion the code does not
already carry, for the reason the section below gives. It is refused because the
answers are not typed per step: replying to `Refresh` with what `SwitchTo`
expects is a panic rather than a compile error. A design whose purpose is
eliminating runtime ordering failures should not introduce one, and the locality
it buys is locality a single command already has.

**A typestate chain.** Seven states, each consuming the last, from `Round::opens`
through `Moving`. It gets the same compile-time ordering as the witnesses and
dissolves the `Watcher` enum. It is refused on its own usage: the Check is the
same twelve lines with two differences, so the sequence gets written out once per
arrangement — reintroducing at the Round exactly the
protocol-assembled-differently-by-two-callers that one door removed from the
Switch.

## Where the three arrangements differ

*The three arrangements differ in exactly three places* is structurally true
rather than unproved. `Watcher` is a two-variant enum, every difference between
its variants is one of three methods — `asking_again`, `pacing`, `reason` — each
a match on the variant, and there is one round function both variants are handed
to. A fourth difference is a fourth method or a fourth match, and neither hides:
the round function takes the enum, so anything that varies varies there.

The count runs two things together. The domain has three arrangements — typed at
a terminal, run by the machine's own service manager, one round for a scheduler
— and the code has two, because the Service is the loop under a supervisor.
Three places is a fact about the two.

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
- A fourth way the arrangements differ is a fourth method on `Watcher` or a
  fourth match inside one, both of which are read off the enum. No test is owed
  for it, which is what withdraws the step protocol's second argument.
