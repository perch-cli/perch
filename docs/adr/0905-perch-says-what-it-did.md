# Perch says what it did

**Perch says what it did, and what the person could not have predicted. It explains
itself only when it refused.**

A thing that happens on every single run is the definition of predictable, and
predictability is earned by the guide rather than re-earned by every invocation. A
refusal is the opposite: nothing happened, the person cannot see why, and the next step
is not obvious. That is the one moment the prose is the product.

The rule binds everything Perch writes, at three budgets, because three populations have
three readers.

## Acting commands

They report to somebody at a prompt, mid-task. `perch switch` is the specimen:

```
$ perch switch
Switched to overflow@example.com, the most room in Group `work`.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
```

Three lines. What is not there is the ranking rationale — *has the most room: 60%
headroom, which is true of every one of its Quota Windows* is Perch defending its
ranking to somebody who did not question it — and a closing paragraph pre-empting a
disappointment that has not happened, in a command whose whole reason to exist is that
the person running it is busy. Both are arguments, and `perch switch` was asked to
switch.

The Scope a Cycle stayed inside folds into the landing line rather than announcing
itself beforehand, because that is where the guard rail is load-bearing: the claim worth
making is that the Switch stayed inside the Group, and it is better made beside the
Account it landed on than in a sentence of its own before anybody knows where they are
going.

**The Capture line is cut, and this is the part worth arguing.** A Capture happens
before every Switch without exception (ADR a-switch-is-written-down-first), which is
exactly what makes *Captured you@example.com's live Credential into its own Profile* the
ordinary case announcing that it was ordinary. The reassurance it carries is real, and it
is the guide's to establish once rather than the command's to repeat forever. What is
*not* cut is any of the other Capture outcomes — each of them is a case where what
happened is not what the guide describes. The rule one level down is the rule above it:
silence on the path that always runs, prose on the paths that do not.

The figures keep their age. A figure without `(as of 4m ago)` is a promise Perch cannot
make (ADR a-figure-carries-its-age), and shortening output is not license to start making
it.

## The Watcher

It writes to a scrollback or a cron mailbox, read long after the fact and often skimmed
for the one line that mattered. Volume is where the cost is: roughly forty words per
round, every two and a half minutes, for as long as the loop runs.

```
12:00:00  waiting   40% used, fullest 5-hour
12:02:30  switched  86% used, fullest 5-hour → overflow@example.com
12:05:00  cooling   86% used, fullest 5-hour — the last Switch was 2 minutes ago, so nothing moves for another 12 minutes.
```

The statuses split exactly along the rule. `waiting` and `switched` are what the loop was
asked to do, so they become data. The refusals keep their prose: a hold promises nothing
was changed (ADR a-refusal-is-a-promise), and a hold that did not say what was holding it
would read as a Watcher that had given up.

The threshold is not in the round line. The header declared it and it does not change
within a run, so re-deriving it in every line spends words on the one thing that never
varies — and if it ever did change mid-run, that deserves a line of its own rather than a
repetition in every line.

## Showing commands

Here the text is the deliverable, so the rule lands only on the prose appended beneath.
The table stays whole. Underneath it, each varying fact is said once and each invariant
fact is said once in total: a Quarantine reason genuinely differs per Account, so it is
written out for each; the repair is the same command whatever broke and however many
Accounts it broke, so it closes the block once rather than ending every line in it.

## The sentence, and how it is asserted

The prose is the designed artifact, so the suite that defends it is part of this
decision.

> **When the sentence is the claim, assert the sentence.** Whole, and reflowed where a
> terminal wrapped it. A test about what Perch *says* — that a Switch explains which
> Group it stayed inside, that a refusal names what it declined to do, that a figure
> arrives with its age — asserts the sentence, not a word from it.

> **When the datum is the claim, assert the datum.** A test that an Account's
> Organization is shown asserts the Organization. Widening that to a sentence would
> couple it to wording it has no opinion about, and wording is changed here deliberately
> and often.

The distinction is what the test is about, and a test's name almost always already says
which.

**No baseline.** Holding accepted output whole — `insta` or an equivalent — is an
excellent answer to *this used to be right and something broke it*, and it has nothing to
say about *this was never right*, which is the only failure mode with a track record
here. Every prose defect Perch has found was found by a person reading the sentence, and
every one of them was wrong the first time it was printed: a reset that had already
elapsed rendering as *(any moment now), which has passed*; a cron mailbox told *the last
Switch was -55 minutes ago*; a figure that lost its age. A baseline taken before any of
those would have recorded the contradiction as accepted output and carried it forward
silently, and every review after would have had a wall of blessed prose to skim rather
than a sentence to read. The instrument would have been working exactly as designed while
the defect sat inside it.

The usual objection to snapshots does not apply, and saying so keeps it from looking like
the reason: the clock is a `Host` effect and the fake pins it, so snapshot output would be
perfectly stable. Determinism is free. Snapshots are declined despite that rather than
because of it.

**The one thing a baseline could see** is a clause lost to the right-hand edge, since
reflowing discards line breaks on purpose and no fragment assertion sees it either. The
answer is not to assert a whole frame in order to catch one property of it — it is to
claim the property: render at a hostile width and assert that nothing load-bearing was
lost. That is a claim rather than a baseline, and it pins no terminal width nobody has an
opinion about.

**A machine reading a shape is not a person reading a sentence.** The two renderings are
governed by the same rule from opposite ends: a person is owed the sentence Perch chose,
and a script is owed a shape that states what it claims. So where the *shape* is the
claim, the shape has to make it — which is why a document says what its order is, or does
not have one (ADR the-listing-owns-the-set).

## What is rejected, and why it matters

**A verbosity flag.** `--quiet`, `--verbose`, a Setting, or branching on whether stdout is
a terminal. This is the answer that looks free and is not: it keeps every sentence alive,
doubles what the suite has to cover, and adds an idea a person has to learn before they
can have the output they wanted. It is also how a project avoids deciding what its output
should be — the sentences stay, unexamined, behind a flag nobody types. If a sentence is
worth printing, print it; if it is not, delete it.

**Moving cut prose to another command.** A staleness caveat could become something
`perch status` says. That is the flag answer wearing a different hat: the sentence survives
without anyone having to defend it, and a second command inherits an explanation written
for a first. The prose is deleted, and the guide carries anything that lived only at the
terminal.

**A line-count cap enforced by a test.** This is the baseline's instrument pointed at
length instead of wording, and it fails the same way: it would bless whatever sits under
the cap and say nothing about whether a sentence earned its place. Prose stays defended by
somebody reading it, which is the only thing that has ever caught one of these here.

**A sweep of the assertions.** Most of the `contains` calls in `tests/` assert that a
datum reached the page, which is what they should assert, and auditing them wholesale
would churn hundreds of correct data-presence claims into worse ones. Assertions are
re-pointed as they break, each under the rule above.

## The glossary

`CONTEXT.md` is unchanged, and the reason is in it already: `output` sits on the _Avoid_
list under **Listing**, beside `table`, `view` and `report`. The domain has no object here
to name. This is a rule about a register, and coining a noun for it would produce a word
whose first use would be to describe a Listing — the exact collision that avoid-list
exists to prevent.

## Consequences

No dependency is added, no tool, no review step, no second place where output lives.

Guide edits ride inside the change that moves the output rather than following behind. A
guide that documents output is wrong from the moment the output changes, and separating
the two ships a knowingly false guide for however long the gap lasts.

Prose correctness stays defended by somebody reading the prose. That is written down here
so the next person to propose a baseline finds the reasoning that declined it rather than a
thousand `contains` calls and the assumption that nobody thought about it.
