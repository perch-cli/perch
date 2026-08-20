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
/// (ADR 0015). Two and a half minutes is twenty-four of them, which leaves room
/// for the `perch status --refresh` somebody types while the watcher is running
/// rather than spending the whole allowance on the loop and having the user's
/// own question refused.
///
/// It is the Refresh of **one** Account, and the case for that is the same
/// arithmetic read the other way: at twenty-four an hour each, a Group of two
/// would already be at the limit and a Group of four past it. Said here,
/// because this is the number it is the reason for (ADR 0013).
```

After:

```rust
/// How long the watcher waits between Refreshing the Account it is on.
///
/// 150s is 24 reads an hour. The endpoint allows 28-30 per Account (ADR 0015),
/// so a concurrent `perch status --refresh` still fits. Refreshes one Account:
/// at 24/hour each, a Group of two is already at the limit.
```

What went, and why each was filling rather than fact: *"the same arithmetic read
the other way"* narrates the reasoning instead of stating it. *"Said here,
because this is the number it is the reason for"* justifies the comment's own
placement. *"somebody types"* and *"the user's own question refused"* dramatize a
constraint the numbers already carry. Every figure survives.

ADR 0013 goes because the module header already cites it — one citation per file.
ADR 0015 stays because this constant is what that decision is about, and the
header does not mention it.

### Deletes

`src/watch.rs`, above `still_holding_line`. Nine lines, gone whole:

```rust
/// ADR 0013 had every held round say which failure held it, and a hold whose
/// line said neither that nor when it would ask again "reads as a watcher that
/// has given up". That was written about a person watching a terminal, where the
/// repeated line *is* the proof of life. A Service writes to a log nobody reads
/// until something is wrong, and a permission hold repeats until somebody
/// changes a setting — possibly for weeks. At one line every two and a half
/// minutes, what five hundred and seventy-six identical lines a day bury is the
/// one line that matters.
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
ADR 0013 eleven times today and `src/commands/watch.rs` cites ADR 0040 eighteen;
a decision appearing eleven times in one file marks nothing.

**Present tense.** The same rule an ADR follows: state what stands, not the route
to it. A rejected alternative is timeless and stays. Perch's own former behavior
is a commit, not an alternative.

**Test names.** A failure prints the name, not the comment, so the name is the
one that has to work. `a_refresh_that_fails_across_a_threshold_crossing_never_switches`
needs nothing above it.

**Comments, not documents.** An argument compressed to bullets stops being
answerable by the next reader, which is what ADR 0043 exists to prevent. The
ADRs keep their prose on purpose. Written down so no session "fixes" them to
match this rule.

## The scale this was measured against

| | |
| --- | --- |
| `src` | 42,213 lines — `///` 10,414, `//!` 949, `//` 4,152 |
| `tests` | 26,844 lines — `///` 3,666, `//!` 325, `//` 725 |
| Comment lines narrating Perch's past | 268 (`used to`, `no longer`, `was never`, `the old`) |
| Comments echoing the line below them | 3, in all of `src` |
| Citation sites in `src` + `tests` | 953 |

The third row is the surprise, and it redirects the work. *What the code does* is
nearly absent from this tree, so the deletion pressure is the history lines and
the argument over the cap — not prose narrating syntax.

## What is enforced

The caps and one-citation-per-file are mechanical. The four kinds and the present
tense rule are judgment and cannot be checked. Tracked at
[#260](https://github.com/perch-cli/perch/issues/260).

**Passing a check is not passing the standard.**
