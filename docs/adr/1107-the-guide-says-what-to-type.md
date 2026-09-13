# The Guide says what to type

**A sentence in the Guide tells the reader what to type, what they will see,
what to do next, or what Perch will not do. Why Perch behaves as it does is
not in the Guide.**

The Guide is the ten pages under `pages/src/content/docs/` that
ADR one-thing-renders-the-site publishes. That document decides where the Guide
lives and how it is rendered, and says nothing about what a page says. So each
page took on the register of the documents its author had just written, and by
v0.3.8 the Guide ran to nineteen thousand words. `watching.md` carried 220
lines of prose before its first heading, and Starlight rendered it with a
two-entry table of contents. The reader it faces has a terminal open and a
quota running out, and cannot find the line that says what to type.

## Two registers, three places

AGENTS.md splits the repository for code: `src/` states facts, `docs/adr/`
makes the case, `CONTEXT.md` defines terms. The Guide sat outside the split,
and the case got made a second time, in every page, for a reader who did not
ask for it. `running.md` explained Carry at the depth of
ADR carried-means-rehearsed; `watching.md` derived the Back-off from Anthropic's
read allowance the way ADR a-watcher-knob-is-arithmetic does. Every one of those
paragraphs is a copy, and a copy diverges.

So the Guide joins the split on the `src/` side. It states what is so. The
reader who wants to know why has the tree, and the citation rule already says
the Guide does not point them into it: a page faces someone without the
repository in front of them.

## The four things a sentence may do

A sentence on a task page stays if it does one of these:

1. **What to type.** The command, its flags, the Target.
2. **What they will see.** What Perch printed, in a transcript, and what the
   words on it mean.
3. **What to do next.** When Perch refuses or lands somewhere unexpected, the
   move that follows.
4. **What it will not do.** The boundary a reader would otherwise assume the
   other way: a Run does not change the active Account, the watcher never
   backgrounds itself.

Why is not on the list, and is deleted rather than shortened. Neither is a
sentence that restates one already on the page, nor one the reader could skip
without losing the ability to act. The fourth item earns its place least
obviously and is the one that stops a reader filing a bug about a decision.

A fact stated only in the Guide is not deleted with the sentence around it. It
moves first, to the ADR that owns the decision it belongs to, or to
`CONTEXT.md` if it is a term. Every rationale paragraph in the Guide was
checked against the tree before it went, and the tree already held what it
argued.

## Not a cap

The comment standard caps a comment at ten, five or three lines and gates the
cap in `tests/comment.rs`. The Guide has no cap, and the difference is
deliberate. A comment over its cap is a decision with no ADR, so the cap finds
a missing document. A page over any number of words is a page that may be
padded or may be exactly as long as operating Perch takes, and a count cannot
tell which. Three thousand words that each earn their place pass this
document; nine hundred of padding fail it. What is measured is whether a
sentence does one of the four things, and that needs a reader.

Length is a symptom. `watching.md` was not long because it had many words; it
was long because most of its sentences told the reader something they could
not act on.

## The transcript is the unit

Every `##` section of a task page opens with a transcript: the command as
typed, then what Perch printed, taken from the current build. The prose
beneath it explains what is on the screen and nothing that is not. A paragraph
with no transcript above it is a paragraph about something the reader cannot
see, and is the first place to look for a sentence doing none of the four
things.

Identities in a transcript are example-shaped, `you@example.com` and
`overflow@example.com`, and dates are the fixture's. Nothing else in one is
invented: a transcript that shows what Perch printed two releases ago is a
false Guide. Regenerating them is done by hand when a page is rewritten, which
is the one moment somebody is reading every one anyway. Checking them against
the binary on every pull request is ADR using-it-is-the-proof's question and is not
decided here.

## What is gated

`tests/publication.rs` reads two facts about a task page, and neither is a
number about length. Every `##` section holds a fenced block, so no section
explains a screen it does not show. A task page has more than one `##`, so no
page renders with a table of contents a reader cannot use. Both are structure,
and structure is a fact a test can read without argument.

`reference.md` is exempt from the first: it is tables, and a table is what a
reader looks a flag up in. `index.mdx` is exempt from both, because it is the
Splash.

Passing the test is not passing the standard. Which of the four things a
sentence does needs a reader, as it does for a comment.

## Two audiences

The Splash and `installing.md` face a reader deciding whether to install. The
other pages face one who has, and is mid-task. The first pair may say what
Perch is for; the nine may not. A sentence in `switching.md` that would serve
as the Splash's copy is one to cut, because the reader on that page already
chose.

## Refusals

ADR a-refusal-is-a-promise puts the remedy in the refusal itself, so a Guide page
that restates one duplicates the binary. A refusal appears in the Guide only
where the move that follows is not in Perch's message: it takes a second
command, or a decision the reader has to make. Where the Guide finds itself
explaining a refusal the message already explains, the message is what is
wrong, and the fix is to the binary. Exit codes stay in `reference.md`, once.

## Vocabulary

The Guide writes Account, Group, Headroom and Cycling with `CONTEXT.md`'s
capitals, because Perch's own output does and a Guide that matches the
terminal is worth more than one that reads as ordinary prose. A term carries
itself or gets one clause. The Guide defines nothing at length, because
`CONTEXT.md` is where a definition lives and a second one diverges.

## What is not decided here

**The page set.** Ten flat pages, each named by the Splash's card grid and the
README's table, and the site's document leaves a grouped sidebar open. A
page that is still too long once every sentence does one of the four things is
the evidence it said to wait for, and is looked at then.

**What Perch says at a terminal.** The citation rule groups the Guide with
everything Perch prints, as two surfaces facing a reader without the tree. This
document reaches the Guide alone. ADR perch-says-what-it-did and the refusal
document govern the other surface and are not reopened here.

**The README.** `npm/build.mjs` ships it as the npm package's landing page and
an offline clone reads it as its getting-started, a third audience neither the
Guide nor the Splash has. Its register is its own decision.
