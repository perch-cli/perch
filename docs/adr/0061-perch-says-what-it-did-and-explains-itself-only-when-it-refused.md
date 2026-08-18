# Perch says what it did, and explains itself only when it refused

What `perch switch` prints today is seven lines and eighty words:

```
$ perch switch
Cycling within Group `work`.
overflow@example.com has the most room: 60% headroom, which is true of every one of its Quota Windows — 7-day is its fullest, as of 4m ago.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
That figure is what Perch last observed rather than what Anthropic says now. If overflow@example.com turns out fuller than it implied, the figure was stale — `perch status --refresh` reads a current one.
```

It is the command somebody types mid-task, at the moment quota just ran out
underneath them. Two of those lines answer what they asked. The rest explain
Perch to them.

## What was actually wrong

Not the count. Four of those seven lines are short and factual, and a command
that names its steps is not a command that talks too much.

What is wrong is the register. `has the most room: 60% headroom, which is true
of every one of its Quota Windows` is Perch defending its ranking to somebody
who did not question it. The closing paragraph is Perch pre-empting a
disappointment that has not happened, in a command whose whole reason to exist
is that the person running it is busy. Both are arguments, and nobody asked for
one. `perch switch` was asked to switch.

That register is everywhere, because it was never decided anywhere. Ninety-four
`say` call sites carry it, concentrated in `service.rs` (14), `switch.rs` (11)
and `watch.rs` (10), each written by somebody who was right that the fact was
true and never asked whether it was wanted.

**And the Watcher is where it actually costs something.** A Switch spends its
eighty words once. The Watcher spends about forty per round, every two and a
half minutes, for as long as it runs — `src/watch.rs:920` builds every one of
them from a single template that re-derives the threshold each time:

```
2026-08-04T12:02:30Z  switched  you@example.com 86% used, fullest 5-hour; threshold 80% — over it. Switched — overflow@example.com has the most room: 95% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of just now.
```

The threshold is in the header the Watcher already printed. It does not change
within a run. Eight hours of watching is roughly seven thousand seven hundred
words, and the part that varies between two consecutive lines is a percentage.

`perch list` has the third shape of it: `why_they_are_quarantined`
(`src/commands/list.rs:389`) returns one paragraph per Quarantined Account, each
ending in the identical remedy from `registry::how_to_repair`
(`src/registry.rs:180`). Three broken Accounts, three copies of the same
instruction, under a table that had already said `quarantined` three times.

## What was decided

**Perch says what it did, and what the person could not have predicted. It
explains itself only when it refused.**

A thing that happens on every single run is the definition of predictable, and
predictability is earned by the guide, not re-earned by every invocation. A
refusal is the opposite: nothing happened, the person cannot see why, and the
next step is not obvious. That is the one moment the prose is the product.

The rule binds everything Perch writes, at three budgets, because three
populations have three readers.

**Acting commands** report to somebody at a prompt, mid-task. `perch switch`
becomes:

```
$ perch switch
Switched to overflow@example.com, the most room in Group `work`.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
```

Three lines from seven, fifteen words from eighty. The ranking rationale goes.
The staleness paragraph goes. `Cycling within Group \`work\`.`
(`src/registry.rs:433`, said at `src/commands/switch.rs:126`) folds into the
landing line, which is where the guard rail is load-bearing anyway: the claim
worth making is that the Switch stayed inside the Group, and that claim is
better made beside the Account it landed on than in a sentence of its own before
anybody knows where they are going.

The figures keep their age. ADR 0015 is untouched: a figure without `(as of 4m
ago)` is a promise Perch cannot make, and shortening output is not license to
start making it.

**The Watcher** writes to a scrollback or a cron mailbox, read long after the
fact and often skimmed for the one line that mattered. Its five statuses split
exactly along the rule:

```
12:00:00  waiting   40% used, fullest 5-hour
12:02:30  switched  86% used, fullest 5-hour → overflow@example.com
12:05:00  cooling   86% used, fullest 5-hour — the last Switch was 2 minutes ago, so nothing moves for another 12 minutes.
```

`waiting` and `switched` are what the loop was asked to do, so they become data.
`held`, `nowhere` and `cooling` are refusals, so they keep their prose. This is
ADR 0036 preserved rather than disturbed: a hold promises nothing was changed,
and a hold that did not say what was holding it would read as a Watcher that had
given up. The threshold leaves the round line because the header declared it and
it does not change within a run — and if it ever did change mid-run, that would
deserve a line of its own rather than a repetition in every line.

**Showing commands** are the case where the text is the deliverable, so the rule
lands only on the prose appended beneath. The table stays whole. Underneath it,
each varying fact is said once — the Quarantine reason genuinely differs per
Account — and each invariant fact is said once in total, so `how_to_repair` is
printed for the Listing rather than for each Account in it.

**The Capture line is cut, and this is the part worth arguing.** ADR 0006 makes
the Capture happen before every Switch without exception, which is exactly what
makes `Captured you@example.com's live Credential into its own Profile.`
predictable — it is the ordinary case announcing that it was ordinary. The
reassurance it carries is real, and it is the guide's to establish once rather
than the command's to repeat forever. What is not cut is any of the five other
outcomes: `NotTheirs`, `NothingLive`, `Unreadable`, `NoOutgoing` and
`NothingToSave` (`src/commands/switch.rs:205`–`258`) all still speak, because
each of them is a case where what happened is not what the guide describes. The
rule one level down is the same as the rule above it: silence on the path that
always runs, prose on the paths that do not.

## What was rejected, and why it matters

**A verbosity flag.** `--quiet`, `--verbose`, a Setting, or branching on whether
stdout is a terminal. This is the answer that looks free and is not: it keeps
every sentence alive, doubles what the suite has to cover, and adds an idea a
person has to learn before they can have the output they wanted. It is also how
a project avoids deciding what its output should be — the sentences stay,
unexamined, behind a flag nobody types. Perch has no verbosity mechanism today,
and the yardstick this decision is judged by is conceptual surface first. If a
sentence is worth printing, print it; if it is not, delete it.

**Moving the cut prose to another command.** The staleness caveat could have
become something `perch status` says. That is the flag answer wearing a
different hat: the sentence survives without anyone having to defend it, and a
second command inherits an explanation that was written for a first. The prose
is deleted. The guide is audited to carry anything that lived only at the
terminal — and this is not a formality, because `docs/guide/switching.md:88`
currently explains staleness and ends "which is why every Cycle says so", which
this decision makes false.

**A line-count cap enforced by a test.** This is ADR 0043's instrument pointed
at length instead of wording, and it fails the same way: it would bless whatever
sits under the cap and say nothing about whether a sentence earned its place. It
would also have passed the output above, which is seven lines. Prose stays
defended by somebody reading it, which is the only thing that has ever caught
one of these here.

**A sweep of the test suite.** The six affected suites hold 279 of the
repository's 1,241 `contains` calls, and ADR 0043 already refused to audit
assertions wholesale: most of those 279 assert that a datum reached the page,
which is what they should assert. Assertions are re-pointed as they break, each
under 0043's rule — is this test about what Perch *says*, or about a datum? The
ten `threshold 80%` assertions are the instructive ones. Under this decision the
threshold appears once per run rather than once per round, so those tests should
end up claiming *that*, which is a stronger claim than the one they make now.

**Nothing is superseded, and three ADRs look like collisions from a distance.**
ADR 0043 says the sentence is the designed artifact and is about how a sentence
is asserted, not how many there are; it also says wording is changed here
deliberately and often, which is the permission this decision uses. ADR 0015 is
why every figure carries its age, and every figure still does. ADR 0036 is why a
hold says what is holding it, and this decision is what keeps those lines long
while the ones around them shrink.

## The glossary

`CONTEXT.md` is unchanged, and the reason is in it already: `output` sits on the
_Avoid_ list under **Listing** (`CONTEXT.md:380`), beside `table`, `view` and
`report`. The domain has no object here to name. This is a rule about a
register, and coining a noun for it would produce a word whose first use would
be to describe a Listing — the exact collision that avoid-list exists to
prevent. ADR 0043 declined a glossary entry on the same grounds and was right
to.

## Consequences

The work lands as four tickets, one per population, each carrying this ADR as
its premise. `switch` and `cycle` go first, because that is the specimen and the
rest should have a worked example to follow. Then the Watcher, which is where
the volume is. Then the showing commands. Then the remaining acting commands —
`add`, `remove`, `purge`, `relogin`, `export`, `import`, `service` — which were
not audited before this decision and should be read rather than assumed to need
the same edit.

Guide edits ride inside each ticket rather than following behind. A guide that
documents output is wrong from the moment the output changes, and separating the
two ships a knowingly false guide for however long the gap lasts.

Every before-and-after in this ADR was read from source and from the guide
rather than from a running binary. The first ticket will find discrepancies;
that is expected, and is why the specimen goes first.

One thing gets cheaper. #153 — render at a hostile width and assert that nothing
load-bearing was lost — is easier to state and more meaningful against three
short lines than against a paragraph that was always going to wrap. That is a
note, not a dependency.
