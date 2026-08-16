# A suite is named for what it asserts, and the ratio was never the question

> **Carried out in #158.** Like ADR 0041, ADR 0042 and ADR 0044, this is the
> artifact of a planning effort rather than of a change, so it landed ahead of
> the work it describes instead of beside it. The tree now matches it: the three
> files carry the names below, and the rule about where a test lives is in
> `tests/common/mod.rs`'s header.

The question arrived as a size question: `tests/` is 24,565 lines against 47,980
of `src`, a ratio of one to two, and is that too much harness for a tool one
person runs on one machine.

The ratio is not one to two, and it does not point the way the question assumed.
Once that is fixed there is no size question left — only a shape one, which turns
out to have a good answer that has never been written down anywhere a person
could find it.

## What the count actually is

`47,980` is `src` including `src`'s own tests. Thirty-eight files there carry a
`mod tests`, and those modules are **14,227 lines** — `registry.rs` 1,469,
`tui/model.rs` 1,197, `probe.rs` 812, `watch.rs` 788, `lock.rs` 703, and on down.

So the honest split is **38,792 lines of test against 33,753 of production**, a
ratio of **1.15 to 1**. Tests do not take up half the repository. They slightly
outweigh everything they test.

That is a different fact, and it is not by itself a defect. It is the arithmetic
of a tool with twenty-four commands, a `Host` port with two adapters that must
agree, and a stated policy of breaking anything at will — a repository that
breaks freely is one that leans on its suite to notice.

Nor is the figure stable enough to indict. Three decisions already taken remove
**9,896 lines** before this one touches anything: ADR 0041 deletes 6,604 with
Dogfood, roughly 2,400 of it unit tests, and ADR 0042 deletes 3,292 with the
Config tab. ADR 0044 adds `tests/invoking.rs` back. The ratio moves by a point.

**So size is not what is wrong here, and no suite is removed by this ADR.** The
two subsystems this sweep has deleted were indicted by conceptual arguments —
seven glossary entries for a harness nobody ran, a write path duplicating four
commands — and neither was found by counting. There is no third such subsystem in
`tests/`: it is thin files averaging seven hundred lines, not one of which
introduces an idea.

## Three kinds of suite

The premise also said thirty gerund binaries. There are thirty-three binaries,
twenty-seven of them gerunds, and sorting them by *what they assert* rather than
by how they are named produces three kinds — not one rule with a ragged edge.

**Behaviour.** What a command does, driving real command code against
`FakeHost`. Twenty-six of them. Named for the behaviour, which is not the same as
named for the command: `exporting` happens to be one command, but `carrying`
(ADR 0003), `storing` (ADR 0020), `reconciling` (ADR 0026) and `naming` are
mechanics no single command owns, and no per-command rule could have produced
them. The clearest evidence the rule is behaviour and not command is `perch
status`, which is asserted across three files — `status.rs` for what it shows,
`listing.rs` for `list` and `--group`, `refreshing.rs` for `--refresh`. Under a
per-command rule that is a defect. Under this one it is three behaviours that
share a verb, which is what they are.

**Correspondence.** That two artifacts which must agree, do. Six of them:
`conformance` asks the two `Host` adapters the same questions; the four
`contract_*` suites ask whether the probe's beliefs still match the installed
Claude Code (ADR 0007); `publishing` asks whether the site still matches the
repository that publishes it (ADR 0035). These assert a relationship, not a
behaviour, and none of them drives a command against a fake.

The gating axis cuts across this and does not define it. `contract_*` is held
back by a feature and its failures are news about upstream; `conformance` is
ungated and its failures are ordinary faults in the change that caused them —
`conformance.rs`'s own header draws that line and draws it correctly. Two suites
can assert the same *kind* of thing and mean different things by failing.

**Surface.** What the binary accepts and returns. One of them, `tests/invoking.rs`
from ADR 0044, which is on the record as explicitly not a behaviour suite: *the
surface, never the behaviour behind it.*

The naming signal is **a gerund for what Perch does, a noun for a
correspondence**. Behaviour and Surface are both Perch doing something, so
`invoking.rs` keeps the name ADR 0044 gave it. That leaves three files misnamed
against a rule the other thirty already follow, and they are renamed rather than
excused:

| now              | becomes           | why                                    |
| ---------------- | ----------------- | -------------------------------------- |
| `adoption.rs`    | `adopting.rs`     | a behaviour wearing a noun             |
| `status.rs`      | `reporting.rs`    | a behaviour wearing a command's name   |
| `publishing.rs`  | `publication.rs`  | a correspondence wearing a gerund      |

Three `git mv`s. A test binary's name is its filename, so nothing else moves.

**Correspondence** is a coinage and the one term here at risk. The industry word
is *contract test*, which this repository has already spent on a subset — the
four `contract_*` suites are three-quarters of the category but not the category.
Anyone reaching for the obvious word will name the part after the whole. The term
is written down here for that reason. *Avoid*: contract test, compatibility test,
integration test.

## Where a test lives

The line between a `mod tests` in `src` and a binary in `tests/` is followed
consistently and stated nowhere.

The obvious guess is wrong. It is not that unit tests are pure and behaviour
tests use the fake: `src/lock.rs` and `src/registry.rs` both drive `FakeHost`
from inside their own `mod tests`. The fake is not the discriminator, and anybody
who assumed it was would draw the line in the wrong place on their first try.

The line actually being drawn is **what the test names**. A `mod tests` asserts a
module's own vocabulary through the module's own API. A binary in `tests/`
asserts what a *command* does.

`watch` is the demonstration, because it has both and they do not overlap.
`src/watch.rs` holds thirty-eight tests on the pacing arithmetic and the decision
log — `the_wait_doubles_with_every_failure_and_stops_at_the_longest`,
`a_hold_that_has_not_changed_is_said_once_rather_than_every_round`. Those reach
`round()` and `after_failing()` directly, and no command has an opinion about
either. `tests/watching.rs` holds fifty driving the real loop —
`a_refresh_that_fails_across_a_threshold_crossing_never_switches`. Neither could
be moved to the other side without loss: pushing `lock.rs`'s renewal cases into a
behaviour binary would mean reaching them through whichever command happens to
take a lock, and pulling command behaviour into a `mod tests` would abandon the
what-does-a-person-get framing every file in `tests/` opens with.

That rule is added to `tests/common/mod.rs`'s header, which is the file every
behaviour binary already declares and already carries the harness's other
standing rule. A `tests/README.md` was declined: a second venue is a second thing
to disagree with this ADR.

## The fixtures stay shared, and the allow stays

`tests/common/mod.rs` is 763 lines of fixtures, command drivers and readers,
recompiled into twenty-five of the thirty-three binaries under a blanket
`#![allow(dead_code)]`.

The allow is the correct cost of Cargo's one-module-per-binary model, and the
file's own header already says so. The two ways out are both worse. Extracting a
`perch-test-support` dev-dependency crate would make `dead_code` honest by
putting everything behind a crate boundary — buying a suppressed lint with an
extra crate in the tree, which is conceptual surface paid for with maintenance
surface, backwards. Splitting into submodules changes nothing: Rust compiles the
module per binary either way, and per-binary imports would move the noise rather
than remove it.

What the allow genuinely costs is that it would cover real rot as willingly as it
covers the normal case. That is a habit to keep rather than a decision to take.

## What this does not decide

**Whether gating the `contract_*` suites is the right way to hold beliefs about
upstream.** This ADR places them — a correspondence, four binaries, correctly
distinguished from `conformance` — and says nothing about the gate. That the gate
makes 895 lines invisible to the coverage job, and that it fires when Perch
changes rather than when Claude Code does, are live and are #156's. Placing a
suite in a taxonomy is not the same as approving how it runs.

**Whether `browsing.rs` survives.** It is a behaviour suite under this ADR and
will be a smaller one once ADR 0042's Config tab goes, but whether the picker
exists at all is #151's, blocked on #147. If the picker goes, so does the file,
and nothing here objects.

**What the commands are.** #145 may redraw the command surface entirely. That is
the strongest argument for naming binaries after behaviours rather than commands:
a rename of `perch status` does not invalidate `reporting.rs`, because the file
was never named for the command.

## Consequences

Three renames, and no change to any assertion.

`tests/common/mod.rs`'s header gains the rule about where a test lives.

`CONTEXT.md` loses its **Proving it works** heading. ADR 0041 empties it of all
seven entries, and this ADR declines to refill it — the same call ADR 0042, ADR
0043 and ADR 0044 each made in turn, on the same grounds. Behaviour,
Correspondence and Surface are terms a contributor holds, not terms somebody
using Perch holds, and the glossary is for the second kind. They live in this ADR
and in the header, which are the places a person writing a test is already
looking. An empty heading would be worse than a deleted one.

The `Host` port seam survives, and the map's watch on it can close. The seam
exists so command code can be driven with no machine, and the map named exactly
two things that could threaten it: #142, which passed by refusing to move
behaviour off the fakes, and this ticket. Behaviour stays where it is, `host::fake`
remains the only way it is driven, and there is nothing left watching.

This ADR supersedes nothing. ADR 0043 governs what an assertion must claim, ADR
0044 governs the level a claim is made at, and this governs where a test lives
and what its file is called. Three axes, no overlap.

The shape is re-affirmed rather than left alone, which is the point. A reader who
counts 24,565 lines and thirty-three binaries and finds no reasoning is entitled
to assume nobody chose it. The reasoning is here now, along with the corrected
number, so the next person to propose collapsing the harness finds an argument to
answer.
