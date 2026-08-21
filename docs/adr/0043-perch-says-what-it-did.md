# Perch says what it did

What Perch hands a person is a sentence. `Cycling within Group \`work\`.
overflow@example.com has the most room: 60% headroom, which is true of every one
of its Quota Windows — 7-day is its fullest, as of 4m ago.` The figures behind
that come from a cache and an endpoint, but the thing that was designed is the
wording.

Across `tests/` that wording is asserted 933 times by `contains`. The question
this ADR answers is whether it should be asserted by a baseline instead —
`insta` or an equivalent, holding accepted output whole, so a clause that goes
missing is caught rather than walked past.

It should not. A baseline asserts that output has not changed since somebody
accepted it, and the prose defects this repository has actually shipped were
never changes.

## What a baseline can see

Every prose defect Perch has found was found by a person reading the sentence,
and every one of them was wrong the first time it was printed.

The deep review at #116 found three in one pass. A reset that had already elapsed
rendered as `was due back at 11:00 UTC (any moment now), which has passed` —
a contradiction two words wide. The Watcher told somebody reading a cron mailbox
that `the last Switch was -55 minutes ago`, because a stamp written by one run
outlived the clock that made it. And a figure lost its age.

None of those is a regression. Each was the first thing that code ever printed. A
baseline taken at any point before that review would have recorded the
contradiction as accepted output and carried it forward silently, and every
review after would have had a wall of blessed prose to skim rather than a
sentence to read. The instrument would have been working exactly as designed
while the defect sat inside it.

That is the whole of the argument. A baseline is an excellent answer to *this
used to be right and something broke it*. It has nothing to say about *this was
never right*, which is the only failure mode with a track record here.

The usual objection to snapshots does not apply, and it is worth saying so
plainly rather than letting it look like the reason. Perch's clock is a `Host`
effect and `FakeHost` pins it, so snapshot output would be perfectly stable.
Determinism was free. Snapshots are declined despite that, not because of it.

## The mechanism a baseline would have replaced already exists

The premise that prose is only ever sampled does not survive counting.

Of the 933 `contains` calls, 116 take an interpolated identity — `contains(EMAIL)`,
`contains(&format!("Switched to {THIRD_EMAIL}"))` — and 720 take a string literal.
Of those literals, 303 are a single word, and the commonest are not fragments of
prose at all: `work` and `held` and `watcher-may-act` and `cycle-ungrouped` and
`4242`. Those are Group names, Setting keys, a PID. They assert that a datum
reached the page, which is exactly what they should assert, and a baseline would
not improve one of them.

Where the sentence genuinely is the claim, the suite already asserts it whole:

    contains("Reserve: 2 of 2 Accounts have Headroom, the best 93% left (as of 4m ago)")
    contains("5-hour emptiest 96% used across 2 Accounts (as of 4m ago)")

And there is purpose-built machinery for doing it. `said()` in
`tests/browsing.rs` reflows a frame into one run of words so a sentence can be
asserted across whatever line breaks the terminal put in, and its doc comment
says why in ADR a-figure-carries-its-age's terms.
`assert_eq!(said(frame).matches("as of 4m ago").count(), 2)` claims that every
figure carries its age — a stronger claim than any single substring, and one a
baseline would only make by accident.

So the proposed middle position — baselines for the load-bearing sentences,
`contains` for the rest — is already this repository's shape, minus the
baselines. What was missing was never a mechanism. It was a rule saying when to
reach for the one that exists.

## The rule

**When the sentence is the claim, assert the sentence.** Whole, and reflowed
through `said()` where a terminal wrapped it. A test about what Perch *says* —
that a Switch explains which Group it stayed inside, that a refusal names what it
declined to do, that a figure arrives with its age — asserts the sentence, not a
word from it.

**When the datum is the claim, assert the datum.** A test that an Account's
Organization is shown asserts the Organization. Widening that to a sentence would
couple it to wording it has no opinion about, and wording is changed here
deliberately and often.

The distinction is what the test is about, and the test's name almost always
already says which. #116 fixed its own instance of this by hand — *"the existing
tests asserted the ranking and walked past the bracket; they now assert the
bracket too"* — and the rule is that sentence, generalized.

It binds tests written from here, and tests being changed for another reason.
There is no sweep. Auditing 933 assertions against it would churn hundreds of
correct data-presence claims into worse ones, and the sentences most obviously
worth the rule — the Reserve line, the Headroom line — already pass it.

## What a baseline could see that this does not

One thing, and it is real. `said()` discards line breaks on purpose, so the suite
cannot see a clause lost to the right-hand edge — the exact defect its own doc
comment names. No fragment assertion can see it either.

The answer is not to assert a whole frame in order to catch one property of it.
It is to claim the property: render at a hostile width and assert that nothing
load-bearing was lost. That is a claim rather than a baseline, it survives
whatever #151 decides about the picker, and it does not pin a terminal width
nobody has an opinion about. Tracked at #153.

## What this does not decide

Where output is captured. #142 asks whether the suite should drive the real
binary rather than calling command functions with hand-built `Args`. This rule is
about what an assertion must claim, not about where the output came from, and it
travels to a binary's stdout unchanged. Deciding otherwise here would have been a
hidden constraint on a question that has not been answered yet.

Whether `perch tui` renders any frames at all, which is #151's, blocked on #147.

## Consequences

No dependency is added. No tool is introduced, no review step, no second place
where output lives.

`CONTEXT.md` is unchanged. The rule states in ordinary words, and buying it a
glossary entry would spend conceptual surface to say something `said()`'s doc
comment already says at the place it matters.

The 933 existing assertions stand as they are.

Prose correctness stays defended by somebody reading the prose. That is not a new
practice — it is what the deep reviews have been doing, and it is the only thing
that has ever caught one of these. It is written down here so that the next
person to propose a baseline finds the reasoning that declined it rather than 933
`contains` calls and the assumption that nobody thought about it.

#143 — whether the harness shape is right — loses one of its two blockers. What
this settles for it is that assertion style is not among the things it has to
decide; what remains open there is the level, which is #142's.
