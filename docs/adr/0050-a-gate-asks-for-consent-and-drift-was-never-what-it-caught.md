# A gate asks for consent, and drift was never what it caught

> **Decided, not yet carried out.** The re-gating, the merges and the two
> documentation edits below are tracked separately. Like ADR 0041, ADR 0042,
> ADR 0044 and ADR 0045, this is the artifact of a planning effort rather than
> of a change, so it lands ahead of the work it describes.

The question arrived as a shape question: four suites, 895 lines, held back by
the `contract` feature because they need a real Claude Code and a real keychain
to assert against — is that the right way to hold beliefs about upstream?

The premise does not survive counting, in three separate places. Once it is
fixed there is no drift question left at all, because the flag was never what
caught drift. What is left is a gate whose criterion was never stated and which
sorts almost nothing correctly.

## The flag holds back six things, and three different reasons

Besides the four binaries, `contract` gates two unit tests:
`src/host/real.rs:1753` (`touch_moves_a_directorys_modification_time_forward`)
and `src/lock.rs:1229` (`mod exclusivity`). Neither needs Claude Code. Neither
needs a keychain. Both are gated for **wall clock**, and both say so in their
own doc comments — *"a second of wall clock in every `cargo test` is a second
nobody chose to spend"*, *"eight threads contending really do wait on each
other"*.

So one feature name carries three unrelated meanings: *needs Claude Code*,
*needs the real machine but nothing of Claude Code's*, and *is slow*. Nothing at
the `#[cfg]` site says which, and the name says the one that is least often
true.

## Most of the 895 lines need nothing the name claims

Sorted by what each test actually touches, rather than by which file it is in:

| suite                     | lines | what it touches                                                             |
| ------------------------- | ----- | --------------------------------------------------------------------------- |
| `contract.rs`             | 354   | the real login keychain (**writes** to it), the real `~/.claude`, `claude`   |
| `contract_credentials.rs` | 168   | `temp_dir`, except one test that runs `claude` against a temp config dir      |
| `contract_sessions.rs`    | 177   | `temp_dir` and a read of the real sessions dir; two tests are OS primitives   |
| `contract_links.rs`       | 196   | `temp_dir`. **Claude Code is not mentioned once in the file.**                |

Nine of the twenty-one tests genuinely need something the developer owns. The
rest are held back by association with the file they were written in.

Two of them are not contract tests under any reading.
`the_default_store_is_where_perch_believes_it_is` does **no I/O at all** — it
reads `HOME` and `USER` and asserts that `probe::default_store` derives
`~/.claude` and the bare service name. Under ADR 0045's discriminator, *what the
test names*, it names the probe.

And `taking_a_lock_is_a_directory_only_one_caller_can_make` works in `temp_dir`
and asserts a property of **every** filesystem — its own comment says *"the whole
lock protocol rests on this one property"* — while sitting under
`contract.rs`'s file-level `#![cfg(target_os = "macos")]`. A cross-platform
claim has been asserted on one platform for as long as it has existed, because
of the file it was put in.

## Much of what ungates is already asserted, ungated, on both adapters

`tests/conformance.rs` runs **27 cases** against `RealHost` and `FakeHost`
alike, on every platform CI has, on every pull request. Among them:
`"only the first exclusive create succeeds"`, `"removing a link leaves what it
points at"`, `"link_target answers for the link and not for the file"`,
`"nothing at all is a third answer"`, `"a hard link is a second name rather
than a link"`, `"a private write creates the file and its directory closed"`,
`"a file is created with exactly the mode asked for"`.

Against that, `contract_links.rs`'s five tests are **two pure duplicates**
(`removing_a_link_leaves_what_it_pointed_at`,
`a_path_that_is_not_a_link_and_a_path_that_is_nothing_are_different_answers`)
and three that each carry one claim conformance does not have:

- a write **after** the link is made is visible through it;
- **`a_directory_link_shows_what_the_directory_gains_afterwards`** — entirely
  new, the only case that picks a junction on Windows the way Reconcile itself
  does, and *"the part an allowlist would have got wrong"* (ADR 0026);
- a hard link **stops** sharing when the file behind it is replaced by the
  write-beside-then-rename-over an editor does.

`taking_a_lock_…` is likewise a duplicate plus one half — the age its directory
carries. `contract_credentials.rs`'s two private-write tests are substantially
covered by conformance's mode and private-write cases.

So `contract_links.rs` is not 196 lines of coverage behind a flag. It is
largely 196 lines of *duplicate* behind a flag, asserting on one adapter what is
already asserted on two.

## And it asserts the half that cannot be wrong

`src/host/fake.rs:179` records that a hard link is held in the link table
**and** in `files`, *"because that is what a hard link is: another name for the
same bytes — the whole reason a Run reaches for junctions and hard links (ADR
0026)"*. The fake was built to model exactly the properties `contract_links.rs`
checks, and `contract_links.rs` drives only `RealHost`, so **nothing asserts the
fake models them correctly**.

That is the half that can actually be wrong. The platform is not going to stop
sharing bytes across a hard link; the hand-maintained fake might stop modelling
it, and `conformance.rs`'s own header is a list of five times the two silently
drifted apart. Moving these cases into conformance's table does not relocate a
claim, it strengthens one — from *"the platform behaves as ADR 0026 assumes"* to
*"…and the fake agrees"*, which is what every behaviour test leaning on
Reconcile actually depends on.

The cost is named rather than hidden: two of the carried-over properties are
ones the fake can only agree about by construction rather than by observation.
That is still strictly more than today, where the fake is asked nothing.

## The criterion is consent

The gate is kept, and its meaning is replaced. It stops meaning *asserts about
Claude Code* and starts meaning:

> **This test touches state the developer owns and did not offer.**

The reason to hold a test back is not that it might damage something. It is
that **its outcome is not the repository's to determine**. A suite that reports
green on Tuesday and skipped on Wednesday, on the same machine at the same
commit, according to whether a client happened to be open, is the one thing a
default-on suite must never be.

So a **read** of the developer's real state counts as much as a write.
`a_running_clients_marker_is_the_shape_perch_believes_in` reads the live
`~/.claude/sessions` and asserts, or skips, or finds nothing, according to the
developer's unrelated afternoon. Writing into their login keychain is worse in
consequence and identical in kind. One line covers both.

The feature is renamed **`your-machine`**, because the name is the criterion made
visible and leaving it as `contract` would let every reader keep the meaning the
code no longer has. `real-machine` was considered and fails on its own terms:
every ungated test uses the real machine too, `temp_dir` and all. The
discriminator is *ownership*, not reality.

Under it, the twenty-one tests sort:

- **Nine stay gated** — seven of `contract.rs`'s nine, one from
  `contract_credentials.rs`, one from `contract_sessions.rs`. The keychain
  writes, the two runs of the installed `claude`, and the reads of the real
  keychain and the real sessions directory.
- **Eleven ungate.** Three link cases and the two private-write cases and
  `taking_a_lock_…` fold into `conformance.rs`'s table; two are dropped as
  duplicates. The three process and marker tests need a suite of their own,
  because `conformance.rs` excludes processes by charter — *"the very things a
  fake exists to invent"*.
- **One moves out of `tests/` entirely** — `the_default_store_…` into
  `probe.rs`'s own `mod tests`, driven by `FakeHost`.

**Four binaries become two**, and the gated one keeps its macOS cases behind
per-test `#[cfg]` rather than a file-level one. A file-level `cfg` is what
narrowed a cross-platform claim to one platform without anybody choosing it, and
one binary means one place to look for *what needs my machine*.

## The two slow tests ungate

They are not consent cases — both work in `temp_dir` — so under one criterion
they cannot stay. A second feature named for a *cost* was refused: `slow` names
a price rather than a claim, so nothing ever tells you when a test has stopped
qualifying, and under this map's yardstick a second flag is a second idea held.

The price is measured rather than asserted. The two together take **7.01s run
alone**; the whole 463-test lib suite takes **6.13s**, and Cargo parallelises
within a binary, so the marginal cost of ungating both is **one to two seconds**
on a six-second suite. Their own comment says *"a second of wall clock"*, which
understates what it is guarding by about seven times.

What decided it is what is under there. `lock::exclusivity` is the **only
execution of the lock's exclusivity claim anywhere in the repository**, and CI's
own comment records that it *"had never executed"* until somebody noticed that
naming `--test` targets suppresses the default set. A claim that has already
been silently unexecuted once does not go back behind a flag.

## Drift is caught by three things, and none of them was the flag

The flag decided what compiled. It never had a trigger, never had a reader, and
never protected a user.

**The trigger already exists and the ticket's premise about it was false.**
`ci.yml:13` carries a weekly `schedule`, with the argument on it: *"Claude Code
updates continuously while this repository may not change for a week. A weekly
run is what turns 'we would have noticed' into 'we did'."* It is re-affirmed
here, unchanged, rather than left unexamined.

**The reading is the separate CI step**, kept apart *"so a failure here reads as
upstream drift rather than as a fault in the pull request"*.

**The protection is `probe.rs`** — one module that refuses the dangerous
operation and says which assumption failed (ADR 0007). That is the only part of
this a user could ever feel.

Nothing new is named for drift, and that is the finding rather than an omission.
Naming a feature after a purpose it does not serve is exactly the confusion this
decision found; inventing a second name for the purpose would repeat it.

Neither is this settled by track record. The repository is **twelve days old**,
the scheduled run has fired **once**, and `INSTALL_SHA` has never been bumped.
There is no ledger here in either direction, and the decision is made a priori
because that is the only way it can be made.

## What this does not decide

**The cadence.** Weekly against `latest` is what `ci.yml` chose deliberately.
Re-opening it without a single drift event to reason from would be churn under a
neutral-bias rule, so it is re-affirmed and closed.

**Coverage.** That the gate makes lines invisible to the coverage job is not an
argument and was refused as one. A coverage figure is not a claim about
correctness, and letting it choose a gate would be the figure choosing the test
shape. Most of these lines become visible as a consequence of the re-gating; the
consequence is not priced.

**The names.** The two surviving binaries are correspondences, so ADR 0045's
rule already fixes their shape — a noun — and the carry-out chooses the words,
on #145's precedent of excluding naming from the decision. Not #158: that pass
renames `adoption.rs`, `status.rs` and `publishing.rs` and edits
`tests/common/mod.rs` and `CONTEXT.md`, none of which this touches, so the two
are independent rather than sequenced.

**ADR 0007's decision**, which is untouched. Probing rather than assuming, and
refusing at runtime, is exactly as sound after this as before.

## Consequences

`ADR 0007` gains **one amendment and no supersession**. Its closing sentence —
*"Contract tests assert the same shapes against the installed bundle in CI, to
find drift before users do"* — names an artifact that will not exist by that
name, so it is rewritten to name the scheduled run, which is the thing that
actually does it. A first correction header on a file that has none; #144's
objection was to a third.

The CI step loses its `--lib`, which existed only to reach the two unit tests
that now run by default, and its `--test` list shrinks to the one gated binary.
`--features contract` becomes `--features your-machine`.

`CONTEXT.md` gains nothing — the fifth ADR in this sweep to decline it, on the
same grounds. `your-machine` is a build flag, drift is a CI cadence, and the
probe's refusal already speaks for itself in prose the user reads at the moment
it fires. Nothing here is an idea a person must hold in order to use Perch, and
the glossary is for that kind and no other. The 497 lines of `CONTEXT.md`
contain no entry for *contract*, *drift*, *probe*, *conformance* or *assumption*
today, and that was correct.

This ADR supersedes nothing. ADR 0043 governs what an assertion claims, ADR 0044
the level it is made at, ADR 0045 where a test lives and what its file is
called, and this one **whether a test may run without being asked**. Four axes,
no overlap.

ADR 0045 placed the four `contract_*` suites as Correspondence and explicitly
left the gate to this ticket. The placement survives: the two survivors are
correspondences still, and the cases that move into `conformance.rs` move within
the same kind.
