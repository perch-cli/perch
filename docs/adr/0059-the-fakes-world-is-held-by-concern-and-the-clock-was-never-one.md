# The fake's world is held by concern, and the clock was never one

## What was actually wrong

Not that `src/host/fake.rs` is long. The 2057 lines it stood at against
`real.rs`'s 1904 are a fair price for a machine held in memory, and length is
not what a reader pays.

What was wrong is that all of it was in scope at once. `FakeHost` declared **40
flat fields** — 39 `RefCell`s and `home` — with no interior structure, and 55
arranging methods sitting on them in the order they happened to be written. A
new method reached for the whole world to find the two fields it needed. A
reader of terminal code had 39 other fields in scope to rule out.

### The reason the issue gave does not survive measurement

#205 rested the payoff on what the 43rd `Host` method would cost. That is the
wrong customer. Since the port reached 41 methods on 2026-08-07:

| | 08-07 | 08-17 | net |
|---|---|---|---|
| `Host` methods | 41 | 42 | **+1** |
| `FakeHost` fields | 35 | 40 | **+5** |
| `fake.rs` lines | 1469 | 2057 | **+588** |

Three of those five fields — `not_text`, `filling`, `keychain_set_takes_millis`
— arrived with **no new `Host` method at all**. They are new ways for an
existing method to fail or to stall. The fake grows from the arrangements tests
need, and those arrive whether or not the port widens. That is the reason to
record, and unlike the other one it does not expire: the multiplier
`fake.rs`'s own header measures is per method, and this cost is per
*arrangement*.

### And neither does the partition it proposed

ADR 0056's Consequences says the nine concerns partition these 40 fields 38
ways, with `effects` and `while_waiting` left over. They do not. Ten fields were
read outside their own concern, and five private helpers straddled concerns —
the last of them not over a field at all, which is worse:

```
record             → effects                                called from 7 of 9 concerns
mark_written       → modified, now                          called from Files, Links
lock_error         → keychain_lock, keychain_everywhere, platform
while_they_answer  → answering_takes_millis, now, while_waiting
intended           → Processes::process_id()                Files calling a second concern's method
```

This is ADR 0056's own thesis arriving in the state. The machine's surfaces are
entangled, and a copy of the world is entangled in the same places — `Files` and
`Links` cross-read in *both* directions, which is exactly why that ADR minted
`Filesystem: Files + Links`.

## The finding the design turns on

`now` is not the clock's. It was written at five sites: one builder, and then
`while_they_answer` (Terminal), `keychain_set` (Keys), `sleep` (Waiting) and
`wait` (Waiting). **At all four of those, the very next statement takes
`while_waiting`.** Four out of four: there is no site that moves the clock and
does not then let somebody else in.

Time does not pass in this fake because a clock ticks. It passes because an
effect took time, and while that effect was in flight somebody else touched the
machine. `now` and `while_waiting` are one mechanism with two fields, and the
take-and-run they shared — four verbatim copies of it — is now the private
`somebody_else_arrives` those four sites call.

The implication runs one way only, and `wait` is where that shows: an
interrupted wait costs the clock nothing and still lets somebody else in,
because what the closure stands for is another `perch` arriving and that does
not stop happening because this Perch was interrupted. The rule is *whoever
moves the clock lets somebody else in*, not the converse.

That is what `Stall` is, and it is why there is **no `Clock` state struct**:
`impl port::Clock for FakeHost` reads `self.stall.now`. The port has nine
concerns; the state has eight structs, because one of the nine never had state
of its own.

This also supplies the test for what is cross-cutting: **mutated by more than
one concern**, not merely read by several. Only `effects`, `now` and
`while_waiting` pass it. `platform` — written by one builder, read by four
concerns — and `home` — written never, read by two — are settings, and they stay
in `Environment`.

## What was decided

`FakeHost` holds nine fields:

| Field | Type | n | Holds |
|---|---|---|---|
| `environment` | `Environment` | 6 | `home` `current_dir` `platform` `vars` `current_exe` `user_id` |
| `fs` | `Filesystem` | 12 | `files` `modes` `dirs` `modified` `unreadable` `not_text` `unwritable` `undeletable` `corrupting` `filling` `links` `developer_mode` |
| `keys` | `Keys` | 5 | `keychain` `keychain_lock` `keychain_everywhere` `keychain_keeps` `keychain_set_takes_millis` |
| `processes` | `Processes` | 3 | `executions` `login` `live_processes` |
| `waiting` | `Waiting` | 3 | `listening` `interrupt_after` `waits` |
| `terminal` | `Terminal` | 5 | `notes` `interactive` `answers` `secrets` `answering_takes_millis` |
| `network` | `Network` | 3 | `replies` `traces` `sent` |
| `stall` | `Stall` | 2 | `now` `somebody_else` |
| `effects` | — | 1 | the recorder, bare |

40 accounted for. Two inner fields are renamed so the paths read: `env` became
`vars`, because `self.environment.env` says nothing twice, and `while_waiting`
became `somebody_else`, because `self.stall.while_waiting` names the wait it is
already inside. The `pub` alias `WhileWaiting` keeps its name — it is the
closure's type, not the field's.

**The structs carry no behavior.** Every method stays on `FakeHost`, so a
`port::Files` method writes `self.stall.now` and `self.fs.modified` in the same
breath and all the crossings cost nothing. `record`, `mark_written`,
`lock_error`, `while_they_answer`, `somebody_else_arrives`, `resolved`,
`through_links`, `make_dirs`, `as_stored`, `intended` and `note_directories_of`
stay private helpers on `FakeHost`. Measured after the change, three of the nine
`impl` blocks still reach a struct that is not their own — `port::Keys` and
`port::Waiting` both take `stall.now` and `stall.somebody_else`, and
`port::Processes` reads `environment.home` — and the helpers carry the rest.
Giving the sub-structs methods would turn every one of those into real coupling:
passing a clock into the filesystem, which ADR 0056 refused at the trait level.

**The port is qualified, and the state gets the nouns.** All nine trait names
plus `Filesystem` are the names of the concerns whose state this is, so
`use super as port;` resolves the collision. The impl headers read `impl
port::Files for FakeHost`, and the bare `Filesystem` is the world. This is
deliberately not a second vocabulary — ADR 0056 spent real effort making those
nine names canonical, and inventing `Disk`, `Wire` and `Person` for the same
nine things is the drift its glossary section refuses. `port::Files` at the
header is informative on its own: it says *this is the surface*, where the bare
noun says *this is the world*.

**`RefCell` stays per field.** All 39 keep their own; `home` stays a bare
`PathBuf`. `somebody_else_arrives` carries this comment for a reason — *"Taken
out before it runs, so it can reach back into the fake without meeting a borrow
the call it interrupts is still holding."* The fake is deliberately re-entered
mid-call by a `somebody_else` closure, and coarsening 39 fine-grained borrows
into eight struct-wide ones converts that documented safety into `already
borrowed` panics at runtime. A `RefCell` on `home` for uniformity's sake would
be a promise the fake does not make: nothing can change it.

**`Filesystem` keeps all twelve.** The obvious sub-cut is `Filesystem { files,
links }`, and the crossing data forbids it: `files`, `modes`, `dirs`,
`modified`, `unwritable` and `undeletable` are all read from the `port::Links`
block, and `links` is read from the `port::Files` one. The traffic runs both
ways, which is the whole reason `Filesystem` exists as a supertrait. Twelve in
scope for filesystem code is a fair price; the reader of terminal code went from
39 in scope to 5, and that is where the complaint came from.

**The two durations stay with their surfaces.** `keychain_set_takes_millis` and
`answering_takes_millis` say how long one surface stalls, and each is written
and read by exactly one concern. They do not join `Stall`. Applying the
cross-cutting test consistently is worth more than gathering everything
thematically related.

**No builder changed.** All 33 `with_*` methods keep their names and signatures,
and so do the twenty-one `set_*`, `forget_*` and other arranging methods beside
them. There are 695 call sites across `tests/` and not one moved: the diff for
this work is `src/host/fake.rs` and this record. What moved is where a builder
sits — each is now grouped against the struct it arranges, in the order the
structs are declared.

## What was rejected, and why

**Splitting the file.** `fake.rs`'s own header carries the measurement that
settled it: of the last twenty-five commits that added or removed a function
there, eleven touched both halves and eight touched one. That argument was made
about the builders-versus-`impl` split and it applies unchanged to a per-concern
split. The state gets structure; the file stays one.

**Touching `real.rs`.** It holds no world to partition — it has the machine. If
a second Credential Store ever arrives, that is composition inside `RealHost`,
which is its own question.

**Namespacing the builders.** `host.fs().with_file(...)` would make the API say
which struct each arrangement lands in, at the cost of every call site in the
tree and of a reader having to know the answer before writing the fixture. The
grouping is for whoever edits `fake.rs`; the flat surface is for everybody else.

**`CONTEXT.md`.** `Stall`, `Filesystem` and *concern struct* are words about how
Perch is built rather than about what Perch does, which is the line ADR 0056's
glossary section already drew.

## Consequences

- A new arrangement has to choose a struct, which is the same question a new
  `Host` method already answers under ADR 0056 — and it is answered once, in
  the field list, rather than by scanning 40 names for a neighbor.
- The cost this ADR pays down is per *arrangement*, not per method. That is the
  measurement to re-run if this is ever revisited: count the fields that arrive
  without a `Host` method, not the methods.
- ADR 0056's Consequences is amended in place rather than superseded. Its core
  finding is strengthened by what the fake's state turned out to look like, and
  ADR 0046 and ADR 0051 both set the precedent for refusing to supersede a
  mostly-correct record.
- There is no `Clock` struct and there should not be one. If a future field
  looks like the clock's, the test is whether the site that writes it also takes
  `somebody_else` — if it does, it belongs to the stall.
- `fake.rs` now names the port `port::`. A file that holds a concrete adapter
  *and* names its state after the port's concerns is the only place that needs
  it; `host::prelude` remains the answer everywhere else.
