# The Watcher's round stays where it is, and the ordering is a type it cannot skip

## What was actually wrong

Not the placement. `src/watch.rs` holds what a round *means* and
`src/commands/watch.rs` holds what a round *does*, and `watch.rs:16-18` says so
in as many words. That line has held up: everything in `watch.rs` can still be
argued with in a unit test because nothing in it reaches the network or the
filesystem.

What was wrong is that six orderings — each of which cost a shipped bug — lived
as comments in the doing half, provable only by driving a fake and reading back a
sequence of bearer tokens:

| | what it rules |
|---|---|
| (a) | the Landing resolves before anything is read (ADR 0048) |
| (b) | the Cooldown short-circuits before candidates are read |
| (c) | liveness is asked before the candidate Refresh burst |
| (d) | every way `refuse_if_live` can fail is answered, not just `ProfileLive` |
| (e) | the Check is remembered before the Switch is recorded |
| (f) | a Switch that moved and then failed raises, whatever the failure |

A rule that can only be checked by observing traffic is a rule a refactor can
quietly break.

## What was decided

Each ordering becomes a value that only the ask which earns it can construct:
`Settled` from resolving a Landing, `Idle` from asking whether the outgoing
Profile is Live, `Cooled` from a `Crossed` figure and a spent Cooldown. The steps
that must come after them take them as arguments.

```rust
fn permitted(registry: &Registry, _: &Settled) -> Result<Watching>;                        // (a)
fn considered(registry: &Registry, w: &Watching, _: &Cooled, _: &Idle) -> Vec<Considered>; // (b)(c)
```

Ordering becomes arity. `permitted` cannot be called without a resolved Landing.
The one funnel that produces candidate addresses cannot be reached without both
`Cooled` and `Idle`, so a candidate Refresh inside a Cooldown, or before the
liveness ask, does not compile. (d) becomes `watch::refused_or_raised`, an
exhaustive match over three named variants of `switch::NotIdle` — a fourth breaks
the build until the round says whether it is a refusal or a raise.

Four of the six stop being comments. The other two are behind `switch::switch_to`
since the Switch got one door (ADR 0048, #200): the Check is recorded before the
Switch inside `record_the_switch`, and whether anything moved is a field *both*
ways out carry, so the round asks one question of both rather than reading the
answer off which way out it got.

Error modes are unchanged. `Turn::NotArranged` still carries `PerchError` whole
so a Check keeps exit 18 and 14, `NotIdle::Live` is still `Outcome::Refused`, and
the other two still raise with the code the failure earned.

The round itself does not move.

## What was rejected, and why it matters

Three interfaces were designed independently before this one was chosen.

**A step protocol.** The round becomes two entry points in `watch.rs` yielding
`Step`s — `SettleTheLanding`, `Refresh(addresses)`, `RefuseIfLive`, `SwitchTo`,
`Done` — which `commands/watch.rs` answers with the real calls. It is the deepest
of the three: the two yeses, the exit-code table, the coalescing and all six
orderings sit behind two methods, and it buys one genuinely new assertion — drive
`a_loop()` and `a_check()` over the same registry and prove the step sequences
differ in exactly three places, which is the "three arrangements, one behaviour"
rule finally made checkable. It was rejected because the answers are not typed
per step: replying to `Refresh` with what `SwitchTo` expects is a panic, not a
compile error. A design whose purpose is eliminating runtime ordering failures
should not introduce one, and the locality it buys is locality a single command
already has.

**A typestate chain.** Seven states, each consuming the last, from `Round::opens`
through `Moving`. It gets the same compile-time ordering as the witnesses and
dissolves the `Watcher` enum. It was rejected on its own usage: the Check is "the
same twelve lines with two differences", so the sequence gets written out once
per arrangement — reintroducing at the Round exactly the
protocol-assembled-differently-by-two-callers that `switch_to` had just removed
from the Switch.

**A port.** `WatchPorts`, covering refresh / refuse-if-live / choose / perform, so
the round could be unit-tested against a recording adapter. This was the shape
the architecture review proposed, and the designer briefed to build it refused
it: one real adapter of four one-line pass-throughs, one test-only recorder that
re-records what `FakeHost::effects()` and `sent_to(..)` already record, and
`cycle::choose` — a function that performs no IO — sitting behind a port. One
adapter is a hypothetical seam. `&dyn Host` remains the only port Perch has, as
ADR 0025 decided, and `host::fake` remains the only way behaviour is driven, as
ADR 0044 decided.

## The glossary

**Round** stays out of `CONTEXT.md`. Design C would have added it, and would have
been right to — a load-bearing noun in an interface has to be a word the project
knows. But the round did not become a module here, so the glossary would be
learning a word only the source uses, which is the drift the vocabulary exists to
prevent. If the round is ever given its own interface, the term comes with it.

The witnesses need no entries: `Settled`, `Idle` and `Cooled` are the negatives
of **Landing**, **Live Profile** and **Cooldown**, all already defined. A
**witness** is a value that exists only as proof that an ask was made: nothing to
substitute, and nothing in it that the ask did not establish. `Settled` and
`Idle` are empty. `Cooled` borrows the `Crossed` it was earned from, because it
is proof about *that* crossing rather than about crossings in general — which is
the one thing a witness may carry, and the reason `Crossed` itself is not one:
it holds the figure, and a value that holds the figure is a reading rather than a
proof.

The word **witness** stays out of `CONTEXT.md` for the reason **Round** does. It
is a word about how this code is built rather than about what Perch does, and the
glossary is the domain's. It is defined once in the source — at
`switch::Settled` — and every other use points at that and at this ADR.

## Consequences

- Adding a step to the round means deciding what it must come after, because it
  will not compile otherwise.
- A new way for `refuse_if_live` to fail breaks the build until the round says
  whether it is a refusal or a raise. `NotIdle::Unnameable` gained its first
  coverage on the way in; it had none.
- `watch.rs:16-18` stands unamended.
- The `Fullest` that `Crossed` consumes has to be reached again for the decision
  line — the one piece of ceremony the witnesses cost. The arm that crossed
  nothing is handed the figure back; the two that crossed read it off the
  crossing, through `Crossed::fullest` either way, so the figure on the line and
  the figure the decision was taken on cannot come to differ.
- A witness is only as honest as the ask that mints one, so there is exactly one
  other minter: `switch::nothing_in_flight`, for the opening line, which holds no
  registry lock and so has a Landing to *check* rather than one to settle. It can
  only answer `None` where one is in flight, which is why it cannot weaken what
  `Settled` means. Any third one would have to be argued for here.
- **That minter changes one line of output**, and it is the change the witness
  was for. `perch watcher run` used to open by naming the Account it was about to
  watch, taken from a registry that might hold a Landing — and during one,
  `Active::whose` answers with the Account being *left*, which is the last thing
  Perch established rather than the thing that is true. It now says nothing until
  the first round has settled the Landing. Covered by a test, because an
  unasserted change to what a command prints is one the next refactor is free to
  undo.
- "The three arrangements differ in exactly three places" remains a rule no test
  checks. It was the step protocol's best argument, and rejecting that design
  leaves it unproved. If a fourth difference ever appears, this is the ADR that
  failed to prevent it.
