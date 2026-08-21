# The port fits the machine

`Host` is 42 methods, and the width is the machine's rather than the interface's.
Anything that touches the machine touches several of its surfaces at once, so a
count of the methods is not a reason to cut them up.

It is declared as ten traits — nine named for kinds of effect, one intermediate
— with `Host` as the sum and nothing of its own, and the fake's copy of the
world is held in the same concerns. That is a reading of the port, not a set of
ports: `&dyn Host` still reaches every method, nothing new is substitutable, and
every effect crosses the same seam (ADR a-crate-must-not-cost-a-seam).

## No consumer narrows to one concern

This is the finding, and it is the answer to the next reading that counts the
methods and proposes eight traits to make somebody's signature smaller.

| Consumer | What it names of the port | Narrows? |
|---|---|---|
| `credentials.rs` | keychain 3 · filesystem 5 · `platform` · `note` | no — 10 methods, four concerns |
| `reconcile.rs` | links 3 · filesystem 4 · `platform` | no — three concerns |
| `probe.rs` | 13 methods across five concerns | no |
| `upgrade.rs` | `http` `env_var` `platform` `home_dir` `current_exe` `is_interactive` | 42 → 6, across **four** concerns |
| `anthropic.rs` | `http` `now` `note` | 42 → 3, across **three** concerns |

The two files that narrow at all cut across the concern lines rather than along
them. A cluster having one consumer is not a consumer needing one cluster.

Segmenting the interface also removes nothing from the fake. `fake.rs` states
the multiplier and what it is made of: a Host method arrives with the builders a
test needs to set it up, so `process_started_at` costs ten lines of trait,
thirty-six there, and four `with_*` methods for one concept. That cost is per
method, so nine traits holding the same 42 methods leave the 43rd costing
exactly what the 42nd did.

## What the language allows

Three constraints decide the shape, and together they leave one option.

There is no `&dyn (Files + Links)`. A trait object names one trait.

A `&dyn Host` can only be handed to something wanting a narrower trait if that
trait is a **supertrait** of `Host` — trait upcasting, stable since 1.86, and
the toolchain is pinned above that.

Supertraits must not overlap. Traits named for consumers would: both a
`Refreshing` and an `Upgrading` want `platform`, and two supertraits declaring
it make `host.platform()` ambiguous at every `&dyn Host` site in the tree.

## The nine, and the sum

```rust
pub trait Host: Clock + Environment + Filesystem + Keys + Processes + Waiting + Terminal + Network {}
pub trait Filesystem: Files + Links {}
```

| Trait | Methods | n |
|---|---|---|
| `Clock` | `now` | 1 |
| `Environment` | `home_dir` `current_dir` `env_var` `platform` `current_exe` `user_id` | 6 |
| `Files` | `read_file` … `remove_file`, the 16 filesystem methods | 16 |
| `Links` | `link` `link_target` `remove_link` | 3 |
| `Keys` | `keychain_get` `keychain_set` `keychain_delete` | 3 |
| `Processes` | `exec` `exec_interactive` `process_id` `process_alive` `process_started_at` | 5 |
| `Waiting` | `sleep` `listen_for_interrupts` `wait` | 3 |
| `Terminal` | `is_interactive` `read_line` `read_secret` `note` | 4 |
| `Network` | `http` | 1 |

42, accounted for. The nine are the section dividers `src/host/mod.rs` already
carried, which is why they need no defending: they have been the reading of that
file for as long as it has been long. Two placements are the ones a reader
trusting a divider could be misled by, so they are stated here.

- **`user_id` is the environment's.** It is read for the root refusal and for the
  `gui/<uid>` domain a LaunchAgent is bootstrapped into
  (ADR the-machine-runs-the-watcher). It is who this process is, not whether
  anybody has asked it to stop.
- **`sleep` is `Waiting`'s** rather than the processes'. It is `lock.rs`'s
  contention wait and has nothing to do with a process. It joins
  `listen_for_interrupts` and `wait`, which is also the honest coupling:
  `Waited::Interrupted` means nothing until `listen_for_interrupts` has been
  called, so the two belong to one trait or the ordering between them is
  unsayable.

`Keys` rather than `Keychain`, because `crate::keychain` already exists and
`KeychainError` is imported from it — a `Keychain` trait beside a `keychain`
module is one more thing to disambiguate for nothing. `Network` rather than
`Wire`, matching the divider the file already used.

## Narrowing is opt-in, and its cost falls on the tests

`Host` is the sum, so all 233 `&dyn Host` sites compile untouched and so do the
port's own helpers. A consumer whose need fits inside one concern narrows by
changing one signature; the table above is the record that today none does.

Code holding a **concrete** adapter is the other case. `host.now()` on a
`FakeHost` is `Clock::now`, and `Clock` has to be in scope to be found, so
`use perch::host::Host` does not bring 42 methods with it. The answer is
`host::prelude`, a glob of the nine traits plus `Host`, and one import line per
file. Nine imports written out by hand would not be a fact about any of those
files: a test that arranges a world and reads back what happened to it touches
most of the machine by definition, and spelling out which nine-tenths is noise a
reader has to check against the body. The asymmetry is worth knowing before the
next change of this shape — **the cost of splitting a trait is paid by whoever
holds the concrete type, which in this repository is only ever a test.**

## The one narrowing there is, and the sentence that spans two concerns

`tests/conformance.rs` holds both adapters to the port's sentences, and its
reach is what a scratch directory can drive: the filesystem and the links,
nineteen methods. The clock, the keychain, the processes, the terminal and the
network are either the machine's own state, which a test has no business owning,
or the very things a fake exists to invent. Its table takes a
`&dyn Filesystem`, so that claim is a signature rather than a paragraph and a
case reaching for `Clock::now` or `Network::http` stops compiling instead of
quietly asserting something the suite has disclaimed. `Filesystem` exists for
exactly this: two concerns are what that suite drives, and a supertrait is the
only way to say two.

One of its cases spans two concerns and cannot be made not to. *A lock carries
the time it was taken* reads `modified_at` on a freshly taken lock and asserts
it is within a minute of `now()`. That is a case about the two agreeing, and
`lock.rs` depends on it completely: every staleness rule there is
`now() - modified_at(artifact)`, so an adapter whose clock and whose mtimes
disagreed would answer both questions plausibly and break all of them.
`&dyn (Filesystem + Clock)` does not exist, and widening `Filesystem` to include
the clock would make the name a lie — so the driver, which holds the concrete
adapter and can therefore reach its clock, reads `now()` and passes it in, and
the cases that do not need it take `_now`. The cross-concern sentence is visible
in the table's type rather than reached for from inside a case.

## The fake's world is held by the same concerns

`FakeHost` holds nine fields rather than 40 flat ones. What was wrong with 40
was not the length of the file — a machine held in memory is a fair price — but
that all of it was in scope at once: a new method reached for the whole world to
find the two fields it needed, and a reader of terminal code had 39 others to
rule out.

The concerns are the port's, and they are entangled in the same places the
machine's surfaces are. Ten fields are read from outside their own concern, and
five private helpers straddle concerns — the last of them not over a field at
all:

```
record             → effects                                called from 7 of 9 concerns
mark_written       → modified, now                          called from Files, Links
lock_error         → keychain_lock, keychain_everywhere, platform
while_they_answer  → answering_takes_millis, now, while_waiting
intended           → Processes::process_id()                Files calling a second concern's method
```

`Files` and `Links` cross-read in *both* directions, which is why `Filesystem`
is a supertrait of both rather than a name for either.

**The clock is not one of the concerns.** `now` is written at five sites: one
builder, and then `while_they_answer` (Terminal), `keychain_set` (Keys), `sleep`
(Waiting) and `wait` (Waiting) — and at all four of those the very next
statement takes `while_waiting`. Time does not pass in this fake because a clock
ticks. It passes because an effect took time, and while that effect was in
flight somebody else touched the machine. So `now` and `somebody_else` are one
mechanism with two fields, held as `Stall` and reached through the private
`somebody_else_arrives`, and there is no `Clock` state struct:
`impl port::Clock for FakeHost` reads `self.stall.now`.

The implication runs one way only, and `wait` is where that shows: an
interrupted wait costs the clock nothing and still lets somebody else in,
because what the closure stands for is another `perch` arriving and that does
not stop happening because this Perch was interrupted. The rule is *whoever
moves the clock lets somebody else in*, not the converse.

That also supplies the test for what is cross-cutting: **mutated by more than
one concern**, not merely read by several. Only `effects`, `now` and
`somebody_else` pass it. `platform` — written by one builder, read by four
concerns — and `home` — written never, read by two — are settings, and they stay
in `Environment`.

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

Two inner fields are named so the paths read: `vars`, because
`self.environment.env` says nothing twice, and `somebody_else`, because
`self.stall.while_waiting` names the wait it is already inside. The `pub` alias
`WhileWaiting` keeps its name — it is the closure's type, not the field's.

**The structs carry no behavior.** Every method stays on `FakeHost`, so a
`port::Files` method writes `self.stall.now` and `self.fs.modified` in the same
breath and the crossings cost nothing. Three of the nine `impl` blocks reach a
struct that is not their own — `port::Keys` and `port::Waiting` both take
`stall.now` and `stall.somebody_else`, and `port::Processes` reads
`environment.home` — and the helpers carry the rest. Giving the sub-structs
methods would turn every one of those into real coupling: passing a clock into
the filesystem, which this document refuses at the trait level.

**The port is named `port::` there.** All nine trait names plus `Filesystem` are
also the names of the concerns whose state this is, so `use super as port;`
resolves the collision: `impl port::Files for FakeHost` says *this is the
surface*, and the bare `Filesystem` says *this is the world*. Inventing `Disk`,
`Wire` and `Person` for the same nine things would be a second vocabulary for
what is named once.

**`RefCell` stays per field.** The fake is deliberately re-entered mid-call by a
`somebody_else` closure — the closure is taken out before it runs, so it can
reach back in without meeting a borrow the call it interrupts is still holding —
and coarsening 39 fine-grained borrows into eight struct-wide ones converts that
into `already borrowed` panics at runtime. `home` stays a bare `PathBuf`: a
`RefCell` on it for uniformity's sake would be a promise the fake does not make,
since nothing can change it.

**`Filesystem` keeps all twelve.** The obvious sub-cut is `{ files, links }`,
and the crossing data forbids it: `files`, `modes`, `dirs`, `modified`,
`unwritable` and `undeletable` are all read from the `port::Links` block, and
`links` is read from the `port::Files` one. Twelve in scope for filesystem code
is a fair price; the reader of terminal code has five.

**The two durations stay with their surfaces.**
`keychain_set_takes_millis` and `answering_takes_millis` say how long one
surface stalls, and each is written and read by exactly one concern. They do not
join `Stall`. Applying the cross-cutting test consistently is worth more than
gathering everything thematically related.

**The builders are grouped, not renamed.** All 33 `with_*` methods and the
arranging methods beside them keep their names and signatures, and the 695 call
sites across `tests/` say what they always said. What a builder's position says
is which struct it arranges.

## What is rejected, and why it matters

**Eight traits named for the review's clusters.** They do not cover 42 — the
environment cluster is missing entirely, and `platform` alone has eight
consumers. A cut that leaves five methods unhoused is not a cut.

**Traits named for consumers.** More meaningful names — *what a Refresh needs of
the machine* beats `Network` — and inexpressible, because the roles overlap and
overlapping supertraits are ambiguous at the call site. Generic bounds
(`H: Network + Clock + Terminal + ?Sized`) would express it, at the price of
turning `&dyn Host` into a type parameter at the two call sites that would use
it and monomorphizing the rest.

**A narrow stub for `anthropic.rs`.** Its translation of a reply into a figure
needs no host at all, and the one path that calls `http` is covered by
`tests/refreshing.rs` driving real command code against the fake, which is the
shape ADR the-binary-proves-its-surface chose. The rule taken from it is worth
keeping: a stub at this port is admissible only where `conformance.rs` has
already declared it cannot adjudicate *and* the trait is thin enough that the
stub is the whole contract. `Network` passes both and still has no use for one.
`Keys` fails the first — the keychain's sentences are the ones that must match
`/usr/bin/security` (ADR claude-code-chooses-the-store).

**Splitting either adapter into files.** Of the last twenty-five commits that
added or removed a function in `fake.rs`, eleven touched both halves and eight
touched one; a per-concern split faces the same arithmetic, and it applies to
`real.rs` unchanged. Both adapters keep one file, with nine grouped `impl`
blocks in the order the trait declares them. `real.rs` also holds no world to
partition — it has the machine.

**Namespacing the builders.** `host.fs().with_file(..)` would make the API say
which struct each arrangement lands in, at the cost of every call site in the
tree and of a reader having to know the answer before writing the fixture. The
grouping is for whoever edits the fake; the flat surface is for everybody else.

**Narrowing the port's own helpers.** `write_atomically` reaches `Files`,
`Links` (`link_target`, to write through a dotfile manager's symlink) and
`Processes` (`process_id`, via `temp_beside`). They keep `&dyn Host`, and
`temp_beside` is this document's finding at the smallest scale there is: naming
a temp file needs the process id, that id cannot come from `std::process::id`,
and so the filesystem concern cannot be separated from the process concern even
inside the port that owns both.

**"A new Credential Store backend becomes an adapter at `Keys` alone."** False,
and worth recording because it is the sort of thing this reading appears to buy.
An adapter must also answer `platform`, `read_file` and `note` —
`credentials.rs` needs all of them to decide where a Credential lives. A second
backend is composition inside `RealHost`.

## Consequences

- A new Host method has to choose a concern, which is a question about what it
  is rather than where it goes in a list. It is added to one `impl` block per
  adapter rather than to one — the same two edits, in a named place.
- A new arrangement in the fake has to choose a struct, answered once in the
  field list rather than by scanning 40 names for a neighbor.
- Anything holding a concrete `FakeHost` or `RealHost` needs `host::prelude` in
  scope. That is tests, and it is a compile error rather than a subtlety.
- `conformance.rs`'s claim about its own reach is in its signature. If that
  suite ever does need the clock, widening `Filesystem` is not the answer.
- If a future field looks like the clock's, the test is whether the site that
  writes it also takes `somebody_else` — if it does, it belongs to the stall.
- The cost the fake's structure pays down is per *arrangement*, not per method.
  That is the measurement to re-run if this is ever revisited: count the fields
  that arrive without a `Host` method, not the methods.
- Nothing here reaches `CONTEXT.md`. `Clock`, `Files`, `Network`, `Stall` are
  names for the machine's surfaces and for how the fake holds a copy of them —
  words about how Perch is built rather than about what Perch does
  (ADR code-lives-where-it-reaches).
