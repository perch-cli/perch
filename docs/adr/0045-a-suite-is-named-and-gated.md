# A suite is named and gated

`tests/` slightly outweighs what it tests, and reads as far worse than that
because the count everybody reaches for is the wrong one. Once that is fixed
there is no size question left — only a shape one, and three rules answer it:
what a suite asserts, what its file is called, and whether it may run without
being asked.

## The count that indicts is the wrong count

Counting `tests/` against `src` puts the tests at half the repository. It is
not the honest split: a quarter or so of every test line in this tree lives in
`src`'s own `#[cfg(test)] mod tests` — `registry.rs`, `probe.rs`, `watch.rs`,
`lock.rs` and thirty more — so the file a line sits in is not what makes it a
test. Read by what a line *is*, the tests outweigh production rather than
taking half the repository.

That is a different fact, and not by itself a defect. It is the arithmetic of a
tool with fifteen command names in twenty-six forms, a `Host` port with two
adapters that must agree, and a stated policy of breaking anything at will — a
repository that breaks freely is one that leans on its suite to notice.

**So no suite is removed for its size.** What indicts a suite is a conceptual
argument — a vocabulary nobody needs to hold, a path that duplicates another —
and neither of those is found by counting. What is here is thin files, not one
of which introduces an idea.

## Three kinds of suite

Sorting the binaries by *what they assert* rather than by how they are named
produces three kinds, not one rule with a ragged edge.

**Behavior.** What a command does, driving real command code against
`FakeHost`. Most of them. Named for the behavior, which is not the same as
named for the command: `exporting` happens to be one command, but `carrying`
(ADR everything-but-the-account), `storing`, `reconciling` and `naming` are
mechanics no single command owns, and no per-command rule could have produced
them. The clearest evidence the rule is behavior and not command is `perch
status`, asserted across three files — `reporting.rs` for what it shows,
`listing.rs` for `list` and `--group`, `refreshing.rs` for `--refresh`. Under a
per-command rule that is a defect. Under this one it is three behaviors that
share a verb, which is what they are.

**Correspondence.** That two artifacts which must agree, do. `conformance` asks
the two `Host` adapters the same questions; `corroboration` asks whether the
operating system still answers about a process the way the Marker's evidence
needs; `publication` asks whether the site still matches the repository that
publishes it (ADR one-thing-renders-the-site); `citation` asks whether every
citation in the tree still names a document in `docs/adr/`; `your_machine` asks
whether the probe's beliefs still match the installed Claude Code
(ADR an-assumption-is-probed). These assert a relationship, not a behavior, and
none of them drives a command against a fake.

The gating axis cuts across this and does not define it. `your_machine` is held
back by a feature and its failures are news about upstream; `conformance` is
ungated and its failures are ordinary faults in the change that caused them.
Two suites can assert the same *kind* of thing and mean different things by
failing.

**Surface.** What the binary accepts and returns. `tests/invoking.rs`, which is
on the record as explicitly not a behavior suite: *the surface, never the
behavior behind it* (ADR the-binary-proves-its-surface).

The naming signal is **a gerund for what Perch does, a noun for a
correspondence**. Behavior and Surface are both Perch doing something, so
`invoking.rs` keeps its name; a correspondence wearing a gerund is renamed
rather than excused, which is why the site's suite is `publication.rs` and the
citation suite is `citation.rs`. A test binary's name is its filename, so a
rename is a `git mv` and nothing else moves.

**Correspondence** is a coinage and the one term here at risk. The industry
word is *contract test*, and this repository has spent it on a subset — the
suite asking about the installed Claude Code is one correspondence, not the
category. Anyone reaching for the obvious word will name the part after the
whole. *Avoid*: contract test, compatibility test, integration test.

## Where a test lives

The line between a `mod tests` in `src` and a binary in `tests/` is **what the
test names**. A `mod tests` asserts a module's own vocabulary through the
module's own API. A binary in `tests/` asserts what a *command* does.

The obvious guess is wrong, and anybody who assumed it would draw the line in
the wrong place on their first try: it is not that unit tests are pure and
behavior tests use the fake. `src/lock.rs` and `src/registry.rs` both drive
`FakeHost` from inside their own `mod tests`. The fake is not the
discriminator.

`watch` is the demonstration, because it has both and they do not overlap.
`src/watch.rs` holds its tests on the pacing arithmetic and the decision log —
`the_wait_doubles_with_every_failure_and_stops_at_the_longest` — which reach
`round()` and `after_failing()` directly, and no command has an opinion about
either. `tests/watching.rs` drives the real loop —
`a_refresh_that_fails_across_a_threshold_crossing_never_switches`. Neither
could be moved to the other side without loss: pushing `lock.rs`'s renewal
cases into a behavior binary would mean reaching them through whichever command
happens to take a lock, and pulling command behavior into a `mod tests` would
abandon the what-does-a-person-get framing every file in `tests/` opens with.

The rule lives in `tests/common/mod.rs`'s header, which every behavior binary
already declares. A `tests/README.md` is declined: a second venue is a second
thing to disagree with this document.

## The fixtures stay shared, and the allow stays

`tests/common/mod.rs` is fixtures, command drivers and readers, recompiled into
most of the binaries under a blanket `#![allow(dead_code)]`. The allow is the
correct cost of Cargo's one-module-per-binary model. The two ways out are both
worse: extracting a `perch-test-support` dev-dependency crate would make
`dead_code` honest by putting everything behind a crate boundary, buying a
suppressed lint with an extra crate in the tree — conceptual surface paid for
with maintenance surface, backwards. Splitting into submodules changes nothing,
because Rust compiles the module per binary either way.

What the allow genuinely costs is that it would cover real rot as willingly as
it covers the normal case. That is a habit to keep rather than a decision to
take.

## The gate is consent

A feature holds a suite back, and the criterion is:

> **This test touches state the developer owns and did not offer.**

The reason to hold a test back is not that it might damage something. It is
that **its outcome is not the repository's to determine**. A suite that reports
green on Tuesday and skipped on Wednesday, on the same machine at the same
commit, according to whether a client happened to be open, is the one thing a
default-on suite must never be.

So a **read** of the developer's real state counts as much as a write. A test
that reads the live `~/.claude/sessions` asserts, or skips, or finds nothing,
according to the developer's unrelated afternoon. Writing into their login
keychain is worse in consequence and identical in kind. One line covers both.

The feature is **`your-machine`**, because the name is the criterion made
visible. Naming it for what a test asserts *about* — `contract` — is the
tempting alternative and the one that rots: a binary gathers tests by file and
a file gathers them by subject, so such a name comes to describe a minority of
what it holds and gates the rest by association. `real-machine` fails on its
own terms, since every ungated test uses the real machine too, `temp_dir` and
all. The discriminator is *ownership*, not reality.

**One binary carries everything gated**, so there is one place to look for
*what needs my machine*, and it keeps its platform-specific cases behind
per-test `#[cfg]` rather than a file-level one. A file-level `cfg` is what
narrows a cross-platform claim to one platform without anybody choosing it.

**Slowness is not a criterion.** A second feature named for a *cost* is
refused: `slow` names a price rather than a claim, so nothing ever tells you
when a test has stopped qualifying, and a second flag is a second idea held.
Where a test is genuinely slow the price is measured rather than asserted, and
weighed against what is under there — a flag over a claim nothing else executes
buys seconds at the cost of the claim.

## What ungating buys, and what it costs

A case that moves out of the gated binary and into `conformance.rs`'s table
does not relocate a claim, it strengthens one: from *the platform behaves as
Perch assumes* to *…and the fake agrees*, which is what every behavior test
leaning on that platform property actually depends on. The platform is not
going to stop sharing bytes across a hard link; the hand-maintained fake might
stop modeling it, and asserting against `RealHost` alone asks nothing of the
half that can be wrong.

The cost is named rather than hidden: some carried-over properties are ones the
fake can only agree about by construction rather than by observation. That is
still strictly more than a table the fake is asked nothing by.

## A skip is loud or it is a lie

The failure every gate and every capability check here exists to prevent is a
green run that quietly proved a third of what it was asked to. So a suite that
skipped itself and a suite that asserted must never look identical: a skip says
which case it was and why, out loud, in a reason somebody can act on —
*nothing was stale here, come back in the morning*. CI runs the gated binary
with `--nocapture` for exactly this, and `conformance.rs` refuses outright a
run in which every link case skipped itself.

The quiet kind wears the same colors: a test that runs, finds the state it
needs absent, asserts nothing and reports green. Counting what a machine can
prove and saying the number before anything acts is one way to keep that
honest; naming and counting each skip as it happens is the other, and it is the
one a suite of independent tests can do.

## Consequences

`CONTEXT.md`'s **Proving it works** section holds **Behavior**, with
Correspondence and Surface named inside that entry rather than given ones of
their own. The glossary is for what somebody using Perch has to hold, and these
are terms a contributor holds — but a term defined in a decision record and
nowhere else is a term the next agent writes an issue without.

This governs where a test lives, what its file is called, and whether it may
run without being asked. ADR perch-says-what-it-did governs what an assertion
must claim and ADR the-binary-proves-its-surface the level a claim is made at.
Three axes, no overlap.

A reader who counts the lines and the binaries and finds no reasoning is
entitled to assume nobody chose it. The reasoning is here, along with the
correct way to take the count, so the next person to propose collapsing the
harness finds an argument to answer.
