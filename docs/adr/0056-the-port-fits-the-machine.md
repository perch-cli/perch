# The port fits the machine

## What the question was

An architecture review counted 42 methods on `Host` and proposed cutting them
into eight traits — `Clock · Files · Keys · Links · Wire · Terminal · Processes ·
Pacing` — on three premises: that four of seven method clusters have exactly one
consumer, that `src/host/fake.rs` at 2029 lines outweighs `real.rs` at 1884
because every widening is paid three times, and that `anthropic.rs` would become
testable against a five-line `Wire` stub.

Two of the three do not survive being checked, and the one that does points
somewhere smaller than the review thought. This is the same shape
ADR a-suite-is-named-and-gated found when it went to check whether `tests/` was
too large: the arithmetic was wrong, and once fixed there was no size question
left — only a shape one.

## What the count actually shows

**Segmenting the interface removes nothing from the fake.** `fake.rs:266` states
the multiplier and what it is made of: *"a Host method arrives with the builders
a test needs to set it up, so `process_started_at` cost ten lines of trait,
thirty-six here, and four `with_*` methods for one concept."* That cost is per
method. Nine traits hold the same 42 methods, so the 43rd costs exactly what the
42nd did. The multiplier is real and it is not this interface's — it is the fake's
40 flat fields, which is #205 and not this ADR.

**A cluster having one consumer is not a consumer needing one cluster.** This is
the premise the whole cut rested on, and it is the one that fails. Every consumer
of a single-consumer cluster reaches across several:

| Consumer | What it names of the port | Narrows? |
|---|---|---|
| `credentials.rs` | keychain 3 · filesystem 5 · `platform` · `note` | no — 10 methods, four concerns |
| `reconcile.rs` | links 3 · filesystem 4 · `platform` | no — three concerns |
| `probe.rs` | 13 methods across five concerns | no |
| `upgrade.rs` | `http` `env_var` `platform` `home_dir` `current_exe` `is_interactive` | 42 → 6, across **four** concerns |
| `anthropic.rs` | `http` `now` `note` | 42 → 3, across **three** concerns |

Not one consumer's whole need fits inside one concern. The two files that narrow
at all cut across the proposed lines rather than along them. The port is wide
because the machine is wide, and anything that touches the machine touches
several of its surfaces at once.

**The testability win was already banked.** `anthropic.rs` holds 20 tests and
mentions `FakeHost` once: its translation of a reply into a figure needs no host
at all, and the one path that calls `http` is covered by `tests/refreshing.rs` —
713 lines driving real command code against the fake, which is the shape
ADR the-binary-proves-its-surface and ADR a-suite-is-named-and-gated chose. A
five-line stub would buy a second way to assert what is already asserted, at a
surface `tests/conformance.rs:20` says it cannot adjudicate.

## What the language allows

Three constraints decide the shape, and together they leave one option.

There is no `&dyn (Files + Links)`. A trait object names one trait.

A `&dyn Host` can only be handed to something wanting a narrower trait if that
trait is a **supertrait** of `Host` — trait upcasting, stable since 1.86, and the
toolchain is pinned at 1.97, so this is available rather than aspirational.

Supertraits must not overlap. Role traits named for consumers would: both a
`Refreshing` and an `Upgrading` want `platform`, and two supertraits declaring it
make `host.platform()` ambiguous at every one of the 233 `&dyn Host` sites.

So the traits are named for **kinds of effect**, they do not overlap, they are
`Host`'s supertraits, and `Host` itself declares nothing.

## What was decided

Ten traits. Nine leaves, one intermediate, and `Host` as the sum of eight:

```rust
pub trait Host: Clock + Environment + Filesystem + Keys + Processes + Waiting + Terminal + Network {}
pub trait Filesystem: Files + Links {}
```

| Trait | Methods | n |
|---|---|---|
| `Clock` | `now` | 1 |
| `Environment` | `home_dir` `current_dir` `env_var` `platform` `current_exe` **`user_id`** | 6 |
| `Files` | `read_file` … `remove_file`, the 16 filesystem methods | 16 |
| `Links` | `link` `link_target` `remove_link` | 3 |
| `Keys` | `keychain_get` `keychain_set` `keychain_delete` | 3 |
| `Processes` | `exec` `exec_interactive` `process_id` `process_alive` `process_started_at` | 5 |
| `Waiting` | **`sleep`** `listen_for_interrupts` `wait` | 3 |
| `Terminal` | `is_interactive` `read_line` `read_secret` `note` | 4 |
| `Network` | `http` | 1 |

42, accounted for. The nine are the section dividers `src/host/mod.rs` already
carried, which is why they need no defending here: they have been the reading of
this file for as long as it has been long. Two placements are corrected on the
way, both cases of a method filed where a reader trusting the divider would be
misled:

- **`user_id`** sat under *being asked to stop*. It is read for the root refusal
  and for the `gui/<uid>` domain a LaunchAgent is bootstrapped into
  (ADR the-machine-runs-the-watcher). It is who this process is, not whether
  anybody has asked it to stop.
- **`sleep`** sat under *processes*. It is `lock.rs`'s contention wait and has
  nothing to do with a process. It joins `listen_for_interrupts` and `wait`,
  which is also the honest coupling: `Waited::Interrupted` means nothing until
  `listen_for_interrupts` has been called, so the two belong to one trait or the
  ordering between them is unsayable.

`Keys` rather than `Keychain`, because `crate::keychain` already exists and
`KeychainError` is imported from it — a `Keychain` trait beside a `keychain`
module is one more thing to disambiguate for nothing. `Network` rather than the
review's `Wire`, matching the divider the file already used.

**No production consumer changes.** `Host` remains the sum, so all 233 `&dyn Host`
sites compile untouched, and so do the port's own helpers. Narrowing is opt-in,
per consumer, whenever one appears whose need fits — and the table above is the
record that today none does.

**Tests were a different matter, and this was not foreseen.** The plan said
"zero consumer churn", and that held for exactly the code that holds a `&dyn
Host`: a trait object finds its supertraits' methods without them being
imported. Code holding a *concrete* adapter is the other case — `host.now()` on a
`FakeHost` is `Clock::now`, and `Clock` has to be in scope to be found — so
`use perch::host::Host` stopped bringing 42 methods with it and 27 files stopped
compiling: 21 suites in `tests/` and 6 `mod tests` in `src/`.

The answer is `host::prelude`, a glob of the nine traits plus `Host`, and one
import line per affected file. Nine imports written out by hand would not have
been a fact about any of those files: a test that arranges a world and reads back
what happened to it touches most of the machine by definition, and spelling out
which nine-tenths is noise a reader has to check against the body. The asymmetry
is worth knowing before the next change of this shape — the cost of splitting a
trait is paid by whoever holds the concrete type, which in this repository is
only ever a test.

## The narrowing's first act was to find a sentence that spans two concerns

`tests/conformance.rs` did not narrow cleanly, and what stopped it is this ADR's
own finding arriving in the one place the ADR claimed a clean cut.

One of its cases — *"a lock carries the time it was taken"* — reads
`modified_at` on a freshly taken lock and asserts it is within a minute of
`now()`. That is not a case about the clock, which the suite disclaims; it is a
case about the two agreeing, and `lock.rs` depends on it completely: every
staleness rule there is `now() - modified_at(artifact)`, so an adapter whose
clock and whose mtimes disagreed would answer both questions plausibly and break
every one of those rules. It is a good conformance sentence and it spans `Files`
and `Clock`.

`&dyn (Filesystem + Clock)` does not exist, and widening `Filesystem` to include
the clock would make the name a lie. So the driver — which holds the concrete
adapter, and can therefore reach its clock — reads `now()` and passes it in:
`asserts: fn(&dyn Filesystem, &Path, &str, DateTime<Utc>)`, with the 32 cases
that do not need it taking `_now`. The cross-concern sentence is now visible in
the table's type rather than reached for from inside a case, which is a better
record of it than the assertion was.

The rest of the narrowing is the one place a type can replace a claim. `tests/conformance.rs`
holds the two adapters to the port's sentences, and its own header says what it
can and cannot reach: *"What a scratch directory can drive is the filesystem and
the links — nineteen methods. The clock, the keychain, the processes, the terminal
and the network are either the machine's own state, which a test has no business
owning, or the very things a fake exists to invent."* Its table was
`fn(&dyn Host, &Path, &str)`. It is now `fn(&dyn Filesystem, &Path, &str)`, and a
case that reaches for `now()` or `http()` in that table stops compiling instead
of quietly asserting something the suite has disclaimed. `Filesystem` exists for
exactly this: two concerns are what that suite drives, and a supertrait is the
only way to say two.

## What was rejected, and why it matters

**The review's eight.** They do not cover 42 — the environment cluster is missing
entirely, and `platform` alone has eight consumers. A cut that leaves five methods
unhoused is not a cut.

**Traits named for consumers.** More meaningful names — *what a Refresh needs of
the machine* beats *`Network`* — and inexpressible, because the roles overlap and
overlapping supertraits are ambiguous at the call site. Generic bounds
(`H: Network + Clock + Terminal + ?Sized`) would express it, at the price of
turning `&dyn Host` into a type parameter at the two call sites that would use it
and monomorphizing the rest. Not worth it for two consumers, neither of which is
short of tests.

**A narrow stub for `anthropic.rs`.** Covered above. The rule taken from it is
worth keeping: a stub at this port is admissible only where `conformance.rs` has
already declared it cannot adjudicate *and* the trait is thin enough that the
stub is the whole contract. `Network` passes both and still has no use for one.
`Keys` fails the first — the keychain's sentences are the ones
ADR claude-code-chooses-the-store says must match `/usr/bin/security`, and
`your_machine.rs` exists to check them against the real thing.

**Splitting the adapters into files.** `fake.rs:266` carries the measurement that
settled this once: of the last twenty-five commits that added or removed a
function there, eleven touched both halves and eight touched one. A per-concern
split faces the same arithmetic. Both adapters keep one file, with nine grouped
`impl` blocks in the order the trait declares them.

**Narrowing the port's own helpers.** `write_atomically` reaches Files, Links
(`link_target`, to write through a dotfile manager's symlink) and Processes
(`process_id`, via `temp_beside`). They keep `&dyn Host`, and `temp_beside` is the
proof of this ADR's finding at the smallest scale there is: naming a temp file
needs the process id, `mod.rs:550` explains why that cannot come from
`std::process::id`, and so the filesystem concern cannot be separated from the
process concern even inside the port that owns both.

**"A new Credential Store backend becomes an adapter at `Keys` alone."** False,
and worth recording because it is the sort of thing this cut appears to buy. An
adapter at this port must also answer `platform`, `read_file` and `note` —
`credentials.rs` needs all of them to decide where a Credential lives. A second
backend is composition inside `RealHost`, which is #205's neighborhood and not
this one.

## The glossary

Nothing here reaches `CONTEXT.md`, for the reason
ADR code-lives-where-it-reaches kept **Round** and **witness** out of it.
`Clock`, `Files`, `Network` are names for the machine's surfaces — words about
how Perch is built, not about what Perch does. The glossary is the domain's, and
it holds **Account**, **Profile**, **Credential**, **Landing**. Teaching it nine
words only `src/host/` uses is the drift the vocabulary exists to prevent.

## Consequences

- A new Host method has to choose a concern, which is a question about what it
  is rather than where it goes in a list. It also has to be added to one `impl`
  block per adapter rather than to one — the same two edits, in a named place.
- Anything holding a concrete `FakeHost` or `RealHost` needs `host::prelude` in
  scope. That is tests, and it is a compile error rather than a subtlety, so the
  cost is paid once when a file is written and never again.
- `conformance.rs`'s thirty-line claim about its own reach is now in its
  signature. If that suite ever does need the clock, widening `Filesystem` is
  not the answer — the answer is in its header.
- ADR a-crate-must-not-cost-a-seam stands unamended, and so does
  ADR code-lives-where-it-reaches's restatement of it: `&dyn Host` remains the
  only port Perch has. This is nine names for one port's surfaces, not nine
  ports. Nothing new is substitutable, no adapter was added, and every effect
  still crosses the same seam.
- The fake's cost is untouched, deliberately. #205 is where that is answered,
  and it is worth noting that the nine concerns partition the fake's 40 fields
  38 ways — so this ADR makes that work more mechanical without doing any of it.

  > **Amended by ADR the-port-fits-the-machine.** They do not partition it 38
  > ways. Ten of the 40 fields were read from outside their own concern, and
  > `now` and `while_waiting` turned out to be one mechanism rather than two
  > concerns' state. The finding above is strengthened rather than damaged: the
  > fake's copy of the world is entangled in exactly the places this ADR found
  > the machine's surfaces entangled, which is why `Filesystem` was minted here
  > and why the fake's state holds all twelve filesystem fields as one struct.
  > The work was less mechanical than this sentence promised, and it landed.
- `fake.rs:266`'s comment is amended to say the interface is nine traits while
  the file is deliberately one, so the next reader who finds the split obvious
  finds the measurement first.
- If a consumer ever narrows, the traits are already there and the change is one
  signature. The table in this ADR is the answer to the next review that counts
  42 methods and proposes this again — the finding is not *42 is fine*, it is
  *no consumer of this port narrows to one concern, and the width is the
  machine's rather than the interface's.*
