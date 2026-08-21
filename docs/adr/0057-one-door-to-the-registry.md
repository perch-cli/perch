# One door to the registry

## What was actually wrong

Not that three destructive commands re-derive a ritual. An architecture review
proposed `commands::destructively(host, out, did, guard, question, body)` on the
strength of `remove`, `purge` and `relogin` looking alike — each prints
frightening sentences, each checks something twice, each holds the registry lock
across a wait. They do not share a sequence, and the evidence is in the section
below.

What was wrong is that Perch takes the registry lock in three different shapes,
holds it for a different span in each, and had written none of that down. There
are 21 `registry::save` call sites. Which shape a command uses is forced rather
than chosen — by whether it has anything to write before its wait — but with the
rule unstated, a command's author infers it from whichever file they read first.

`remove.rs:89-95` is what that costs. The second liveness ask was added there
after the fact, and the comment on it names the other two commands as the
precedent it was measured against: "`perch holdings purge` and `perch relogin`
both ask twice; this is the only command that deletes a Credential, and it was
asking once." Three commands, two of them right, and the third found out by
being wrong. A rule nobody wrote down is a rule the next command discovers by
having a bug.

## What was decided

The lock is held for exactly as long as the command needs it, and three shapes
exhaust the ways. Every one of the 21 sites is in one of them.

**1 — No wait.** Take the lock, mutate, save, release. Nothing unbounded happens
in between, so there is nothing to guard against.
`enable`, `alias`, `group`, `config`.

**2 — A wait with nothing yet to write.** Defer the lock past it. The wait is a
browser round trip, and it happens before the command holds anything worth
guarding, so the exclusive lock is taken *afterwards*, against a registry read
fresh. `add` reads shared at `add.rs:38`, logs in at `65`, and takes the
exclusive lock at `75`; `relogin` does the same at `45`, `57` and `68`.

**3 — A wait about what is already held.** Hold the lock across it and guard it
with `still_ours`. The wait is a question put to a person *about* the registry
the command is already holding, so deferring is not available to it.
`remove.rs:104`, `purge.rs:128`, `import.rs:59`, `export.rs:99`.

Shapes 2 and 3 differ in exactly one thing: whether deferring was available. That
is why `relogin` correctly calls `still_ours` nowhere. There is no stale hold to
guard — the hold is minutes newer than the login, taken after it rather than
across it. Read as a missing guard, it would have been given one.

Shape 1 gets a door. `commands::only_the_registry` is `config.rs`'s `written`
promoted into `commands/mod.rs` and set beside `still_ours`, which is shape 3's
guard; each doc comment points at the other, so the pair teaches this ADR at the
point of use.

```rust
pub fn only_the_registry(
    host: &dyn Host,
    out: &mut dyn Write,
    change: impl FnOnce(&mut Registry) -> Result<Vec<String>>,
) -> Result<()>
```

**The closure taking no `&dyn Host` is the decision, not an accident of the
signature it was promoted from.** It is what turns membership from a reviewer's
observation into something the compiler holds: a command that goes through this
door cannot reach a Credential Store, cannot write a Profile, and cannot touch
the Default Profile, because it is handed nothing that could. A version widened
to accept a `Host` so more call sites fit would be a helper that saves three
lines, and three lines are not worth a function.

Eight sites go through it: `enable.rs:51`, `alias.rs:42` and `:49`,
`group.rs:81`, `:87`, `:92` and `:101`, and `config.rs:102`, which is where the
shape was already written.

The other thirteen stay where they are. Each either moves a Credential, saves
more than once in a run, or attaches a `with_note` narrating which half of an
irreversible act completed — and for those the save's *position* is the
load-bearing part. `remove.rs:296-316` writes `active` in the middle of the
destruction rather than with the rest of the registry at the end, precisely so a
failure between the two cannot strand it.

## What was rejected, and why it matters

**One ceremony for the destructive commands.** The three do not share a sequence.

| | `remove` | `purge` | `relogin` |
|---|---|---|---|
| lock | `ensure_adopted_exclusively` | `registry::lock` and a raw `load`; adoption deliberately refused (`purge.rs:62-67`) | `ensure_adopted`, then a second exclusive one after the browser |
| Target | one | **none**, by definition | one |
| Landing | `resolve_a_landing` | none | `resolve_a_landing` with a deliberate `Conflict` exemption (`relogin.rs:83-90`) |
| liveness | `switch::refuse_if_live_anywhere`, one Account | `purge::refuse_while_anything_is_running`, the whole registry | `switch::refuse_if_live_anywhere`, one Account |
| `still_ours` | yes | yes | never |
| ends with | `registry::save` | deletes the registry | `save`, then `make_live` |

Three things follow from it. The **cohorts do not match the proposal**:
`still_ours` has four callers — `remove`, `purge`, `import`, `export` — so
`relogin` is in the candidate and not in the set, while `import` and `export` are
in the set and not in the candidate. The **ordering disagrees between the two
commands that share both rules**: `remove` runs ask, liveness, `still_ours`
(`85`, `95`, `104`) and `purge` runs ask, `still_ours`, liveness (`111`, `128`,
`136`), so the second liveness check sits on opposite sides of the guard and each
file argues for its own order. A ceremony would have to pick one and silently
overrule the other. And **`relogin` is not destructive**: it deletes nothing, and
its write is deliberately non-replacing so that a login which produced something
unstorable "leaves the Account exactly as broken as it was rather than more so"
(`relogin.rs:246-248`).

The proposal's headline was that the asks-once bug becomes unrepresentable, and
that bug is already fixed. So it was prophylaxis, and there is no fourth
destructive command for it to protect: **Remove** is "always exactly one Account"
and **Purge** "takes no Target, because it is never about one Account". The
glossary forecloses the space. Leverage over a set of two does not pay for a
six-parameter higher-order function whose three callbacks' ordering-between is
the whole of its interface — an interface more complex than the code it hides is
the shallow module the review exists to find, arrived at from the other side.

**`add` as a ninth site.** `add.rs:105-114` already hand-rolls this exact shape,
an immediately-invoked closure around `upsert`, `name_account` and `save`, which
is fair evidence the pattern wants a home. It is still not a member. The `if let
Err` beneath it runs `profile::discard(host, &placed)`, because a Profile nothing
records holds a live refresh token that nothing will ever look at again — a
compensating action over a Credential already on disk. Admitting it means putting
`&dyn Host` in the closure, which is the one thing the door is for. `add` is not
a near-miss; it is the proof that the boundary is somewhere real, and
`only_the_registry` names it in prose for that reason.

**Reversibility as the axis.** It is the obvious way to describe the cohort —
every one of the eight is undoable — and it is wrong for the reason
ADR one-door-to-the-registry already found it wrong: "reversibility really was
the wrong axis." What separates the eight is not that they can be undone but
that they touch nothing outside the registry, which is a statement about reach
rather than about consequence, and is the one a signature can hold.

**A test that walks the command modules.** Nothing stops a future command taking
the lock by hand and never finding the door. A test asserting otherwise would be
a lint wearing a suite's clothes, checking by reflection what the type system
half-holds already. The realistic ceiling is that the right way is shorter than
the wrong way and this document says why; it is accepted rather than papered
over.

## The glossary

No new terms, and the absence is the finding. "The registry" is already the
project's word for the written-down part of what Perch holds — **Holdings** is
defined as "every Profile, every Credential Perch holds, the registry naming them
and what each Group carries", so the `_Avoid_: registry` note there forbids only
using it as a synonym for the whole. The rule was sayable in the domain's own
vocabulary before it was written down, which is why `only_the_registry` is named
in it rather than in a coinage.

The three shapes stay out of `CONTEXT.md` for the reason **Round** and
**witness** stayed out (ADR code-lives-where-it-reaches): they are words about
how this code is built, not about what Perch does, and the glossary is the
domain's. They are defined here and at `only_the_registry`, and nowhere else.

## Consequences

- A command that changes only the registry is three lines shorter and cannot be
  written to touch a Credential. One that needs to touch one cannot use the door,
  which is the refusal working rather than the door being too narrow.
- The shape a new command belongs in is answerable before it is written: does it
  wait, and does it hold anything worth guarding when it does.
- `still_ours` and `only_the_registry` sit adjacent in `commands/mod.rs` and
  reference each other. Splitting them later would separate shape 3's guard from
  shape 1's door and leave neither legible.
- Behavior is unchanged. `enabling.rs`, `naming.rs`, `grouping.rs` and
  `configuring.rs` pass untouched, and that they were not edited is the check.
- Nothing enforces that a *new* command finds the door. If a future one takes
  the lock by hand and grows the fourth shape, this is the ADR that failed to
  prevent it — and the fourth shape is then the thing to argue with, not the
  door.
- The rejected ceremony is recorded at length because it is re-suggestible: three
  commands that print frightening sentences will look alike to the next review as
  they did to this one, and the table above is the answer to it.
