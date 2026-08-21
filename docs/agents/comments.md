# Comments: the worked examples and the reasoning

The rule is in `CLAUDE.md` under **A comment earns its place**. This file holds
the examples and the case for each clause. The rule binds without reading this;
read it when a call is close.

## Worked examples

### Keeps as written

`src/lock.rs`, `WAIT_MILLIS`. Three lines, a rejected alternative stated as a
live alternative, no history:

```rust
/// How long to wait between attempts. Claude Code jitters the same wait; Perch
/// takes these locks once per command rather than in a loop, so a fixed wait
/// has nothing to spread out.
```

Why it passes: a reader reaching for jitter is answered, and the answer is a
present-tense fact about how Perch takes locks. Nothing here is about what Perch
used to do.

### Rephrases

`src/watch.rs`, `REFRESH_INTERVAL_MILLIS`. Eleven lines to four.

Before:

```rust
/// How long the watcher waits between Refreshing the Account it is on.
///
/// Anthropic's usage endpoint allows roughly 28-30 reads an hour per Account
/// (ADR a-figure-carries-its-age). Two and a half minutes is twenty-four of
/// them, which leaves room for the `perch status --refresh` somebody types
/// while the watcher is running rather than spending the whole allowance on the
/// loop and having the user's own question refused.
///
/// It is the Refresh of **one** Account, and the case for that is the same
/// arithmetic read the other way: at twenty-four an hour each, a Group of two
/// would already be at the limit and a Group of four past it. Said here,
/// because this is the number it is the reason for
/// (ADR a-watcher-knob-is-arithmetic).
```

After:

```rust
/// How long the watcher waits between Refreshing the Account it is on.
///
/// 150s is 24 reads an hour. The endpoint allows 28-30 per Account
/// (ADR a-figure-carries-its-age), so a concurrent `perch status --refresh`
/// still fits. Refreshes one Account: at 24/hour each, a Group of two is
/// already at the limit.
```

What went, and why each was filling rather than fact: *"the same arithmetic read
the other way"* narrates the reasoning instead of stating it. *"Said here,
because this is the number it is the reason for"* justifies the comment's own
placement. *"somebody types"* and *"the user's own question refused"* dramatize a
constraint the numbers already carry. Every figure survives.

ADR a-watcher-knob-is-arithmetic goes because the module header already cites it
— one citation per file. ADR a-figure-carries-its-age stays because this
constant is what that decision is about, and the header does not mention it.

### Deletes

`src/watch.rs`, above `still_holding_line`. Nine lines, gone whole:

```rust
/// ADR a-watcher-knob-is-arithmetic had every held round say which failure held
/// it, and a hold whose line said neither that nor when it would ask again
/// "reads as a watcher that has given up". That was written about a person
/// watching a terminal, where the repeated line *is* the proof of life. A
/// Service writes to a log nobody reads until something is wrong, and a
/// permission hold repeats until somebody changes a setting — possibly for
/// weeks. At one line every two and a half minutes, what five hundred and
/// seventy-six identical lines a day bury is the one line that matters.
```

It fails twice over. It narrates what a decision *used to* require and then
argues with it, which is git history's job. And it is an argument over the cap,
which means it belongs in an ADR — if the reasoning is still load-bearing, the
ADR states it and the comment cites it.

What survives is the fact: a hold is said once, with its citation.

## Why each clause

**Four kinds, closed.** A disposition — "say what a careful reader needs" —
produces one standard per session, which is the failure this rule exists to
prevent. The four are what load-bearing comments in the tree actually carry.

The thin edge is the gloss against the banned kind. A gloss translates into
Perch's vocabulary; a restatement reads the syntax back. `EXIT_OK: i32 = 0` earns
one, because `0` is a convention and not a meaning.

**Strict register.** Decided against the tree rather than from it. The tree's
most admired comments are the ones most in breach — `src/watch.rs`'s 26-line
header is the register being cut. So the tree supplied *which facts* a comment
carries; it did not supply how to say them.

**Caps, and what over the cap means.** A numeric cap is checkable and survives
ten sessions; a disposition does not. The cap's real work is not squeezing prose
— it is routing argument to `docs/adr/` and leaving the code holding facts.
Reflowing a long comment to fit is the one response the rule forbids.

**One citation per file.** The caps make a citation the cheapest thing a comment
can carry, so unbounded they would trade prose for slugs. `src/watch.rs` cites
ADR a-watcher-knob-is-arithmetic eleven times today and `src/commands/watch.rs`
cites ADR the-machine-runs-the-watcher eighteen; a decision appearing eleven
times in one file marks nothing.

**Slugs, not numbers.** Identity on the number made a renumber a tree-wide
rewrite — 1,500 sites across 158 files, every one of them moving even where the
decision did not. On the slug, a merge moves only the citations whose decision
actually moved. The number survives as a sort key so the directory reads as a
table of contents; it is in no citation, because identity that shows up in a
citation has not moved.

There is no `docs/adr/README.md`. The prefixes put the section order in the
listing, so a directory listing already reads as the table of contents an index
would be — and an index is a second place every title has to be right, kept in
step by hand, which is what one of those titles says not to do.

One name rather than two. A short slug beside a long title is a second register,
and the next session has to know which of them a reader wants — so the titles
are short and the slug is the title. The 30-character cap is hard rather than a
target: a target drifts across eleven passes, and unlike a comment cap this one
has no "write the ADR" escape hatch. Hyphenation is what lets a check tell
`ADR the-port-fits-the-machine` from the `ADR` in `## Flag ADR conflicts`.

Never a path, so never clickable — that is the cost, paid because the set is
contiguous from 0001 and adding a document renumbers every one after it. Markdown
gets no dialect of its own for the same reason: one form, six file types.

**Who a citation is for.** An agent reading the tree. So the guide never says
`ADR`, and neither does anything Perch says to a person: a user reading a refusal
cannot follow the citation and did not ask for it. `CHANGELOG.md` is the other
exemption, in the other direction — it records what happened on a date, so a
number there is not a citation and may name a document that is gone.

Checked by `tests/citation.rs`: a slug resolving to exactly one file, a slug
within cap and hyphenated, a filename's tail matching its document's title, and
nothing citing a number. The guide is checked the other way round — that it
never says the word at all, since "the numbered ADRs" is not a citation and is
the same defect. One exemption stands: `docs/research/adr-inventory.md`, a dated
read of the 64 documents that indexes them by the number.

**Present tense.** The same rule an ADR follows: state what stands, not the
route to it. A rejected alternative is timeless and stays. Perch's own former
behavior is a commit, not an alternative.

This is where the deletions are. A comment that merely echoes the line below it
is nearly absent from this tree — a sweep looking for one finds almost nothing
and walks past the real defect, which is a comment narrating what Perch used to
do, or an argument sitting over its cap. Aim there.

**Test names.** A failure prints the name, not the comment, so the name is the
one that has to work. `a_refresh_that_fails_across_a_threshold_crossing_never_switches`
needs nothing above it.

**Comments, not documents.** An argument compressed to bullets stops being
answerable by the next reader, which is what ADR perch-says-what-it-did exists
to prevent. The ADRs keep their prose on purpose. Written down so no session
"fixes" them to match this rule.
