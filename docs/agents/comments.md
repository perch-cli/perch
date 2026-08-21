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

`src/watch.rs`, `REFRESH_INTERVAL_MILLIS`. Thirteen lines to five, which is what
the tree holds.

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
/// Twenty-four reads an hour, inside the 28-30 Anthropic's usage endpoint allows per
/// Account (ADR a-figure-carries-its-age), leaving room for the `perch status
/// --refresh` somebody types while the loop is running.
```

What went, and why each was filling rather than fact: *"the same arithmetic read
the other way"* narrates the reasoning instead of stating it. *"Said here,
because this is the number it is the reason for"* justifies the comment's own
placement. *"rather than spending the whole allowance on the loop and having the
user's own question refused"* dramatizes a constraint the numbers already carry.

The Group-of-two arithmetic went with the second paragraph rather than being
compressed into the first, because `a-watcher-knob-is-arithmetic` is where a
knob's arithmetic is argued and the constant only has to say what it is. That is
the cap doing its work: five lines is one fact, and the second fact had a home.

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

**Strict register.** Decided against the tree rather than from it. The comments
that read best were the ones most in breach, and the module headers that argued
their design at length are the register being cut. So the tree supplied *which
facts* a comment carries; it did not supply how to say them.

**Caps, and what over the cap means.** A numeric cap is checkable and survives
ten sessions; a disposition does not. The cap's real work is not squeezing prose
— it is routing argument to `docs/adr/` and leaving the code holding facts.
Reflowing a long comment to fit is the one response the rule forbids.

The three tiers are roles rather than markers, because most of the tree is not
Rust. A `.toml`, `.yml`, `.sh` or `.css` file has one marker and the same three
jobs for it, and reading the caps as binding only where `///` exists exempts the
largest comment blocks in the repository. Which block is the header is the one
judgment that reading needs, and it is settled by moving the block to the top of
the file rather than by widening the rule.

A block costs its first line carrying text to its last, inclusive. A blank
separator between two paragraphs counts, because two facts in one block is the
commonest way over the cap and one of them belongs elsewhere. A `/*` or `*/`
alone on a line does not, because that is what the syntax spends rather than
what the author spends.

Over the cap has three answers rather than two. Usually an ADR, and sometimes a
ticket where the argument is load-bearing and the closed set has no home for it.
The third is a comment that had simply never been edited, and it compresses with
every fact intact: the rule that only `invalid_grant` in a refusal body may be
read as a retired refresh token ran to 23 lines over two blocks, has no home —
`a-broken-account-is-repaired` says how a Quarantine is repaired, never how it
is raised — and fits in two capped blocks with RFC 6749 §5.2 still in it. That
third answer looks like a false positive in a report and is not.

A `///` clap renders into `--help` is exempt from the cap and from nothing else.
That text is what a person reads at a terminal, and cutting a `--help` paragraph
to satisfy a comment cap breaks the surface to fix the tree. The same fact bars a
citation there outright, which is the one predicate under which *over the cap*
and *reaches a person* agree about a block.

**One citation per file.** The caps make a citation the cheapest thing a comment
can carry, so unbounded they would trade prose for slugs: a decision appearing
eleven times in one file marks nothing. The count is over comment lines only —
an assertion message is read on a failure rather than in the tree, so repeating
a citation across four asserts costs nothing and helps.

**Slugs, not numbers.** Identity on the number made a renumber a tree-wide
rewrite — 1,500 sites across 158 files, every one of them moving even where the
decision did not. On the slug, a merge moves only the citations whose decision
actually moved. The number survives as a sort key so the directory reads as a
table of contents; it is in no citation, because identity that shows up in a
citation has not moved.

Band `N` occupies `N01` through `N99`. The thirteen run in `CONTEXT.md`'s
section order, extended by the sections it has no entry for because they are
about the repository rather than the product. A band is sized for its section
rather than for the documents in it, so a new document appends at the end of its
own band and nothing after it moves — the renumber this bought is the last one
the tree pays for.

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

Never a path, so never clickable — that is the cost, paid because the slug is
the identity and a path names the number instead. Markdown gets no dialect of
its own for the same reason: one form, six file types.

**Who a citation is for.** A reader with the tree in front of them. That is the
line, rather than agent-against-person: `README.md` ships into the npm package
and `SECURITY.md` is read on github.com by somebody reporting a vulnerability,
and neither reader can glob `docs/adr/`. The guide is the same case, and so is
everything Perch says at a terminal — including a `///` clap renders, which is
`--help` output wearing a doc comment. `CHANGELOG.md` is the exemption in the
other direction: it records what happened on a date, so a number there is not a
citation and may name a document that is gone.

Checked by `tests/citation.rs`: a slug resolving to exactly one file, a slug
within cap and hyphenated, a filename's tail matching its document's title,
every band contiguous from its base, and nothing citing a number. Which band a
document belongs to is a judgment; that a band has no gap in it is not. The
three pages a reader arrives at are checked the other way round — that they
never say the word at all, since "the numbered ADRs" is not a citation and is
the same defect. One exemption stands: `docs/research/adr-inventory.md`, a dated
read of the 64 documents that indexes them by the number.

And by `tests/comment.rs`: every block within the cap of its tier, no decision
cited twice in one file, and nothing clap prints citing one at all. Those are
the two clauses of the standard a text comparison reaches. The other two — which
of the four things a comment says, and whether it says it in the present tense —
need a reader. Passing the check is not passing the standard, and it is worth
saying because a green check reads like one. What the check is for is the other
direction: in most of the eleven passes that rewrote this tree it found blocks
and duplicate citations that a reading judging every block by eye had passed.

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

**A measurement in a document.** ADR a-crate-must-not-cost-a-seam retires a
number in a comment that moves, and a document is held to it too — except that
an ADR sometimes needs a measurement to have an argument at all. So the test is
what the figure is doing. **Evidence** is restated as the method: *ambiguous at
every one of the 233 `&dyn Host` sites* became *at every `&dyn Host` site in the
tree*, because the argument survives a figure that has drifted and the figure
does not survive the next commit. A **datum** stays, because it is the thing
decided — ADR the-port-fits-the-machine's nine traits and 42 methods are
countable in `src/host/mod.rs` today, and a reader who cannot count them cannot
check the decision.

A figure measuring something outside the tree is evidence unless the document
names what it measured. ADR one-thing-renders-the-site's Astro 7.2.3 and its
three companions stay, because the argument is *this was written against these*
and the version is what makes the claim checkable at all. ADR
claude-code-chooses-the-store counted 39 sites in a Claude Code build it does
not name, which nobody could check on the day it was written; it now says
*wherever it writes one*.

Dating the measurement was refused. A date says when a number stopped being
true, which is git's, and it invites a stale number to be trusted for carrying
one.
