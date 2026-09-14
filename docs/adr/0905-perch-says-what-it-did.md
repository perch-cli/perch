# Perch says what it did

**Perch says what it did, and what the person could not have predicted. It explains
itself only where the reader has something to do about it.**

A thing that happens on every single run is the definition of predictable, and
predictability is earned by the guide rather than re-earned by every invocation. A
refusal looks like the opposite: nothing happened, and the person cannot see why. It is
not. What the person needs from a refusal is the same two things they need from a
success, what Perch decided and what to type next, and the mechanism behind the
decision is the guide's to explain once rather than the refusal's to explain at every
occurrence.

Under all of it sits one sentence, and everything below is it applied to a surface:

> **Perch states the verdict and the next command. It does not show its working.**

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

**The Capture line is cut, and so is every other Capture outcome that needs nothing
done.** A Capture happens before every Switch without exception
(ADR a-switch-is-written-down-first), which is exactly what makes *Captured
you@example.com's live Credential into its own Profile* the ordinary case announcing that
it was ordinary. The reassurance it carries is real, and it is the guide's to establish
once rather than the command's to repeat forever. The first version of this decision kept
the other outcomes, on the ground that each is a case the guide does not describe. That
was Perch narrating its bookkeeping: a Credential that could not be Captured because
Claude Code was logged out, or because the Profile already held a newer one, changes
nothing the person will do next, and a sentence about it is a sentence they have to read
to learn that.

> **Bookkeeping is silent unless the person has to act.** A Capture that could not
> happen and needs nothing done is not said. One that needs a command gets one line
> under the verdict, opening `Note:`, that names the command.

The one Capture outcome that survives is the login made outside Perch that a Switch is
about to replace: *Note: alice@x's login, made outside Perch, was replaced. `perch add` logs it
in again.* The person may want that login, and nothing else on the screen tells them it exists.

**No path on a success line.** *Installed the Watcher. Its decisions go to
/Users/you/Library/Logs/perch/watcher.log* tells the person where a file is on the one
occasion they have no reason to open it. A path appears in a refusal that needs the
person to open something, and in `perch probe` and `perch triage`, where locating things
is the command's job. Everywhere else the guide says where Perch keeps things, and says it
once.

The figures keep their age. A figure without `(as of 4m ago)` is a promise Perch cannot
make (ADR a-figure-carries-its-age), and shortening output is not license to start making
it.

## A refusal is the verdict and the next command

The landing line above cut its ranking rationale. One function away, the same clause
survived into the refusal that says you are already on the best Account:

```
$ perch switch
you@example.com is already the best Account in Group `work`, with 100% headroom, which
is true of every one of its Quota Windows — 5-hour is its fullest, as of 1m ago. Nothing
was changed — `perch list work --refresh` reads current figures.
```

Forty-four words, and the same argument the landing line was not allowed to make. The
first version of this decision let it through under an exemption for refusals, on the
claim that a refusal is where the person cannot see why and the next step is not obvious.
Measured on the tree when this was revised, refusals were 78% of the prose, and what filled them was
not next steps. It was mechanism: *This Installation came from Homebrew, which installs
whatever the formula names and cannot be pointed at 0.3.5. `brew upgrade perch` takes the
newest. To hold a particular Release, install it with the installer script instead, which
takes `PERCH_VERSION`.* Why Homebrew works that way is true on every run and is the
guide's to say.

> **A refusal is the verdict and the next command.** No mechanism, and no rationale for
> why the underlying system behaves as it does. Two lines at most: what Perch declined,
> then the command that gets the person past it, where one exists.

The two specimens become their verdicts:

```
you@example.com is already the best Account in Group `work`.
```

```
Homebrew cannot install a particular Release. `brew upgrade perch` takes the newest.
```

The figure goes with the argument it was serving. A figure Perch quotes carries its age
(ADR a-figure-carries-its-age), and quoting none breaks no promise: `perch status` is one
command away and exists to show them.

The exit code does not move. `EXIT_NOTHING_TO_DO` is what the Watcher's loop branches on
and what a shell prompt tests, and it is not output.

A refusal's `--json` document is not prose and is not touched. It is the shape a script
reads, and a script is owed the same shape after this decision as before it.

### What was changed is said only where something was

*Nothing was changed* had reached nineteen sentences, and in most of them the command
refused before it touched anything. There it reassures somebody about a danger that
never existed. After a `remove` that deleted a Credential and then lost its hold, it is
the most important sentence on the screen, and `Busy` rests on it
(ADR a-refusal-is-a-promise).

The test is whether anything moved, not whether this is a refusal — with one
case where nothing moved and the sentence is still owed. A refusal that comes
*after* somebody agreed to a deletion reaches a reader who has just authorized
one and cannot see whether it began. `perch holdings purge` refusing the export
path it was handed is the specimen. What the reader can believe is the test, and
agreeing to a destructive act is what changes it.

### An ambiguity is a fact where the branches differ

The longest sentences Perch had were case analyses: the live Credential *may be this
one's, made after a Switch that could not finish, or that one's, Rotated since*. Under
the rule that is working, and under the rule it is also sometimes the only warning that
running the suggested command picks one branch and discards the other.

> **An unresolved ambiguity survives where the reader would choose differently knowing
> it, and is cut where they would not.**

That disposes of the class. Where the branches end the same way, the enumeration is
Perch narrating a search it already finished. Where they end differently, the reader may
want to look at the Profile before running anything, and nothing else on the screen
tells them so.

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

The word column is the verdict, so nothing after the dash repeats it. A hold opened with
*nothing current to decide on, so nothing was decided:* and a round with nowhere to go
with *nowhere to go:* — eleven words and three, in front of a column already reading
`held` and `nowhere`, on every such line for as long as the loop runs. What the line is
for is the reason and the retry time.

## Showing commands

Here the text is the deliverable, so the rule lands only on the prose appended beneath.
The table stays whole. Underneath it, each varying fact is said once and each invariant
fact is said once in total: a Quarantine reason genuinely differs per Account, so it is
written out for each; the repair is the same command whatever broke and however many
Accounts it broke, so it closes the block once rather than ending every line in it.

## The dash, and the one place it is not prose

The mark that joins a verdict to its working is the em dash, and Perch had 142 of them in
what it says. Cutting the working takes most; a rule finishes it.

> **No em dash in anything Perch says, except between a value and its qualifier in a
> labeled row.**

The exception is a shape rather than a list of places. *Reserve: none — no Account here
may be Cycled to (1 Quarantined)*, *Watcher  off — `watcher-may-act` is false*, and the
Watcher's own mark where a decision line stops being data. None of them is a sentence:
there is no verb, and the dash is doing a table's work somewhere a table will not fit.

Inside a sentence there is no exception, and the substitutes are not one. A parenthesis
or a colon in the dash's old position is the same clause wearing a different mark, which
is what gave the dash away rather than the dash itself.

**Gated, and this is the one place a mechanical check is right.** `tests/comment.rs`
already gates comment shape, and AGENTS.md already says that passing it is not passing
the standard; this sits beside it under the same caveat. It is not the line-count cap
rejected below: a character's presence is a fact about the text rather than a proxy for
whether a sentence earned its place, so the check can be wrong about a labeled row and
about nothing else.

It stops at what Perch says. Comments, `docs/adr/`, the guide and `tests/` keep theirs —
the reader this is for is the one at a terminal, and a test asserting what Perch says has
to be free to quote it. A `///` on a clap-derived item is not a comment by that test: clap
renders it into `--help`, so the reader is the one at a terminal, and the gate reads it.

## `--help`

`--help` is the largest thing Perch says, and the first version of this decision did not
name it. Unswept, `perch upgrade --help` opened with ninety words on who owns a Homebrew
binary and who owns an npm one, and a subcommand's one-line description ran to 28 words
with a dash in it. That is a second guide, unindexed, rendered at 80 columns, read by
somebody who typed `--help` to learn what to type next.

> **`--help` is bound by this decision.** A command's description is one line, under
> twelve words, no em dash. The paragraph beneath it is deleted, or moves to the guide if
> it says something the guide does not. An argument's description is a phrase.

What `--help` is for is the shape of the command line: which words, which flags, what
each takes. The guide says what happens when you run it (ADR
the-guide-says-what-to-type), and a `--help` that says it too is wrong from the moment
one of them changes.


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

Finding the sentence-shaped ones ahead of a change that moves every sentence at once is
not that sweep. Of 1,610 `contains` calls, 44 assert something sentence-shaped, and they
are re-pointed deliberately rather than whichever way greens them: a red assertion invites
the smallest edit that passes, which is how a sentence claim decays into a fragment claim
and stops seeing the defect it was written for.

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
