# One door to the Registry

**Perch holds the Registry lock in three shapes, and which shape a command uses is
forced by whether it waits and whether it holds anything worth guarding when it
does.**

There are twenty-one `registry::save` call sites. The shape each one is in was
always determined; what was missing is the rule saying so, and a rule nobody writes
down is a rule the next command discovers by having a bug.

**1 — No wait.** Take the lock, mutate, save, release. Nothing unbounded happens in
between, so there is nothing to guard against. `enable`, `alias`, `group`, `config`.

**2 — A wait with nothing yet to write.** Defer the lock past it. The wait is a
browser round trip and it happens before the command holds anything worth guarding,
so the exclusive lock is taken *afterwards*, against a Registry read fresh. `add`
reads shared, logs in, then takes the exclusive lock; `relogin` does the same.

**3 — A wait about what is already held.** Hold the lock across it and guard it with
`still_ours`. The wait is a question put to a person *about* the Registry the command
is already holding, so deferring is not available to it. `remove`, `purge`, `import`,
`export`.

Shapes 2 and 3 differ in exactly one thing: whether deferring was available. That is
why `relogin` calls `still_ours` nowhere — there is no stale hold to guard, because
the hold is minutes newer than the login, taken after it rather than across it. Read
as a missing guard it would be given one.

## The door, and what its signature holds

Shape 1 gets a door. `commands::only_the_registry` sits beside `still_ours`, which is
shape 3's guard, and each doc comment points at the other so the pair teaches this
decision at the point of use.

```rust
pub fn only_the_registry(
    host: &dyn Host,
    out: &mut dyn Write,
    change: impl FnOnce(&mut Registry) -> Result<Vec<String>>,
) -> Result<()>
```

**The closure taking no `&dyn Host` is the decision rather than an accident of the
signature.** It is what turns membership from a reviewer's observation into something
the compiler holds: a command that goes through this door cannot reach a Credential
Store, cannot write a Profile, and cannot touch the Default Profile, because it is
handed nothing that could. A version widened to accept a `Host` so more call sites fit
would be a helper that saves three lines, and three lines are not worth a function.

What `change` returns is said after the save rather than before it, because a line
describing a change that then failed to be written is a line that was not true.

Eight sites go through it. The other thirteen each either move a Credential, save more
than once in a run, or attach a note narrating which half of an irreversible act
completed — and for those the save's *position* is the load-bearing part.
`remove` writes which Account is active in the middle of the destruction rather than
with the rest of the Registry at the end, precisely so a failure between the two cannot
strand it.

## `add` is the proof that the boundary is somewhere real

`add` hand-rolls this exact shape — a closure around `upsert`, naming the Account, and
the save — which is fair evidence the pattern wants a home. It is still not a member.
The arm beneath it discards the Profile the login had already made, because a Profile
nothing records holds a live refresh token nothing will ever look at again: a
compensating action over a Credential already on disk. Admitting it means putting
`&dyn Host` in the closure, which is the one thing the door is for.

## What is rejected, and why it matters

**One ceremony for the destructive commands.** `remove`, `purge` and `relogin` look
alike — each prints frightening sentences, each checks something twice, each waits with
the Registry in hand — and a single higher-order `destructively(host, out, did, guard,
question, body)` is the obvious generalization. They do not share a sequence.

| | `remove` | `purge` | `relogin` |
|---|---|---|---|
| lock | adopts exclusively | locks and loads raw; adoption deliberately refused | adopts, then a second exclusive lock after the browser |
| Target | one | **none**, by definition | one |
| Landing | resolved | none | resolved, with a deliberate `Conflict` exemption |
| liveness | one Account | the whole Registry | one Account |
| `still_ours` | yes | yes | never |
| ends with | a save | deleting the Registry | a save, then making the Credential live |

Three things follow. The **cohorts do not match the proposal**: `still_ours` has four
callers — `remove`, `purge`, `import`, `export` — so `relogin` is in the candidate and
not in the set, while `import` and `export` are in the set and not in the candidate. The
**ordering disagrees between the two commands that share both rules**: `remove` runs
ask, liveness, `still_ours` and `purge` runs ask, `still_ours`, liveness, so the second
liveness check sits on opposite sides of the guard and each file argues for its own
order. A ceremony would have to pick one and silently overrule the other. And
**`relogin` is not destructive**: it deletes nothing, and its write is deliberately
non-replacing so that a login which produced something unstorable leaves the Account
exactly as broken as it was rather than more so.

The proposal's headline is that asking once where twice is needed becomes
unrepresentable, and there is no fourth destructive command for it to protect:
**Remove** is always exactly one Account and **Purge** never about one, so the glossary
forecloses the space. Leverage over a set of two does not pay for a six-parameter
higher-order function whose three callbacks' ordering-between is the whole of its
interface — an interface more complex than the code it hides is a shallow module arrived
at from the other side.

This is recorded at length because it is re-suggestible: three commands that print
frightening sentences will look alike to the next review as they did to the last, and
the table above is the answer to it.

**Reversibility as the axis.** It is the obvious way to describe the cohort — every one
of the eight is undoable — and it is the wrong axis, here and everywhere in Perch. What
separates the eight is not that they can be undone but that they touch nothing outside
the Registry, which is a statement about **reach** rather than about consequence, and is
the one a signature can hold.

**A test that walks the command modules.** Nothing stops a future command taking the
lock by hand and never finding the door. A test asserting otherwise would be a lint
wearing a suite's clothes, checking by reflection what the type system half-holds
already. The realistic ceiling is that the right way is shorter than the wrong way and
this document says why; it is accepted rather than papered over. If a future command
grows a fourth shape, this is the decision that failed to prevent it — and the fourth
shape is then the thing to argue with, not the door.

## The glossary

No new terms, and the absence is the finding. "The Registry" is already the project's
word for the written-down part of what Perch holds — **Holdings** is "every Profile,
every Credential Perch holds, the Registry naming them and what each Group carries", so
that entry's `_Avoid_` note forbids only using the word as a synonym for the whole. The
rule was sayable in the domain's own vocabulary before it was written down, which is why
`only_the_registry` is named in it rather than in a coinage.

The three shapes stay out of `CONTEXT.md` for the reason **Round** and **witness** stay
out (ADR code-lives-where-it-reaches): they are words about how this code is built
rather than about what Perch does, and the glossary is the domain's. They are defined
here and at `only_the_registry`, and nowhere else.

## Consequences

A command that changes only the Registry is three lines shorter and cannot be written to
touch a Credential. One that needs to touch one cannot use the door, which is the
refusal working rather than the door being too narrow.

The shape a new command belongs in is answerable before it is written: does it wait, and
does it hold anything worth guarding when it does.

`still_ours` and `only_the_registry` sit adjacent and reference each other. Splitting
them later would separate shape 3's guard from shape 1's door and leave neither legible.

Behavior is unchanged, and that the behavior suites were not edited is the check.
