# `status` is about one Account, and every set was already the listing

The question arrived as two names over one dataset. `perch status` shows the
active Account and its cached Utilization, `--group` widens it to that Account's
Group, and `perch list` shows every Account — all three from cache, all three
with `--json`, and `status --group` already most of the way to `list`. Keep both,
collapse to one with a flag deciding the breadth, or keep two and move the
breadth onto one of them.

The two constraints said to make this hard are both weaker than stated, and a
third fact nobody counted decides it.

## The constraints, measured

**Prompt-cheapness distinguishes nothing.** `status` without `--refresh` calls
`adopt::ensure_adopted` (`status.rs:51`); `list` calls the same function
(`list.rs:92`). The same non-exclusive read, the same registry, the same cache,
neither touching the network. "Cheap enough for a shell prompt" is a property of
*not passing `--refresh`*, not a property of `status`, and `perch list` has it in
exactly the same measure.

**A network flag is already within reach of a listing.** `perch status --group
--refresh` reads every Account in the Group — `to_refresh` at `status.rs:121`
says so, and `switching.md:103` advertises it in an exit-15 message.
ADR a-figure-carries-its-age's allowance is roughly 28–30 reads an hour *per
Account*, so breadth spends breadth-many independent budgets rather than
exhausting one, and ADR a-figure-carries-its-age already makes each Account's
failure its neighbors' business. Reading every Account on a loop is what
`perch watch` is for.

So the rule was never "keep the network away from listings". It is **a refresh
reads the Accounts it is about to show and no others**, which already holds at
every breadth Perch has.

## `status --group` has no unique output, and after #169 it has no output at all

It is `list::render` with `Scope::Group` — the call is at `status.rs:73`, and
the scope is chosen four lines earlier, falling to `Scope::Ungrouped` from an
Account in no Group (ADR a-group-is-a-declaration).

ADR a-suite-is-named-and-gated found this before this question was asked:
`perch status` is asserted across three files, and **`listing.rs` holds both
`list` and `--group`**. The harness has treated the listing as one behavior
regardless of which command reaches it since before anybody wondered whether it
was.

And ADR the-listing-owns-the-set closes it. The ordering it moves out of the
picker is `model.rs:1645`'s `ranked`, whose own doc comment describes what it
does: each scope ranks its own Accounts, the Ungrouped are held rather than
ranked, and **"the scope the active Account is in comes first, because it is
where you are"**. Once #169 lands, the first section of `perch list` *is*
`perch status --group`. The flag's only surviving job is bounding a refresh.

ADR the-listing-owns-the-set also already wrote the sentence. Its Consequences
say **"`perch list` is the whole of looking"** — true of drawing, which is what
it was about, and now true of listings too.

## Two commands, and the distinctness is not breadth

`perch status` bare is not the listing narrowed. Against a `list` row it adds
`Organization` and `Plan`, and it has room to put the Quarantine sentence *above*
the figures (`status.rs:150`) because the state is the news and the numbers are
the detail — a shape a table cannot hold. Its JSON is one object where the
listing's is an array.

So there are two shapes, and they answer two questions: **one Account in detail**
and **a set as a table**. That is the real seam, and it is worth two names.

What was wrong is that `status` owned a flag that made it render the other
command's output. The fix is not fewer commands; it is **one shape per command**.

> **`perch status` is the active Account. `perch list` is the listing, at every
> breadth.** `--group` leaves `status`.

Two names each with one shape is fewer ideas than one name with two renderings.
The collapse was refused on exactly that arithmetic: it would have kept both
shapes and put them under one word, moving the polymorphism rather than paying it
off.

## Narrowing is an argument, not a flag

`--group` takes no argument on `status` because the Account is implied. On `list`
nothing is implied — and `perch list` must keep working when Perch holds no
active Account, since that is precisely the state `status`'s own error tells you
to leave with `perch switch` (`status.rs:99`). An argument-less flag would
reintroduce the coupling this decision just cut.

The domain is Scopes rather than Groups, since an ungrouped Account narrows to
the Ungrouped. Perch already addresses a Scope, and already reserved the word:
`perch config` names one positionally — a Group by name or `ungrouped`
(`configuration.md:71`) — `registry.rs:525` holds the constant, and
`validate_name` refuses `ungrouped` as a Group name *and* as an Alias so that
the Scope can answer to it. **Scope** is a glossary entry (`CONTEXT.md:340`)
that ADR a-setting-names-its-scope keeps.

> **`perch list [<scope>]`** — a Group by name, or `ungrouped`. `--group` is
> deleted rather than moved.

A flag would be the only place in Perch a Scope is named by flag, against
`perch config` and `perch switch [<group>]` both naming one positionally.
ADR a-command-names-its-noun supplies the rule from the other side: a flag may
mark an argument's absence, and may not carry what an argument carries. A Scope
name is an argument.

`--refresh` follows the breadth, because it always has: `perch list --refresh`
reads every Account it is about to show, `perch list <scope> --refresh` reads that
Scope's. Two commands reach a refresh and that is not two capabilities — each
refreshes what it shows, which is one rule applied to two shows.

## The names stay, and ADR a-command-names-its-noun's own defense is why

ADR a-command-names-its-noun deferred naming deliberately: "ruling on their
names before ruling on their number would decide in the wrong order." With the
number settled at two, and with `tui` gone, `status` is the only noun among ten
top-level Account commands.

It stays, and not by default. ADR a-command-names-its-noun defended
`perch status` against `perch watcher status` as **"the rule working — same
verb, different noun, and the elided noun is precisely what tells them apart."**
That defense requires the word to be the same at both levels; renaming the
Account's one orphans the Watcher's as a lone noun and spends the collision
ADR a-command-names-its-noun built. `status` also happens to be the most
conventional name any CLI has — `git`, `systemctl`, `docker` — and a convention
every user already holds is conceptual surface bought for nothing.

The noun-among-verbs observation was worth making and the answer is that it is
not a defect. `status` names the thing asked for, and it is asked for at two
levels.

## What a document says about its order

Two changes, one of them a rule.

**`status --json` loses its duplicated `utilization`.** It sits at the top level
beside `active.utilization` because "`perch status --json | jq .utilization` is
the line in somebody's shell prompt" (`status.rs:174`). That earned its keep
against a document which, under `--group`, also answered about a set — the
duplicate was insurance against reaching into the wrong shape. `status --json`
now answers about exactly one Account and cannot be anything else, so the
insurance has nothing to cover and `jq .active.utilization` is one word longer.

**The rule: a document says what its order is, or it does not have one.** After
#169 the listing is scope-sectioned and ranked, so `accounts[0]` is the
top-ranked Account of the Scope you are in — a load-bearing fact that a flat
array states nowhere, and the held-versus-ranked distinction
ADR the-listing-owns-the-set called its weightiest piece is invisible in JSON
entirely. A `--json` that shows the ranking without naming it as a ranking is
the two-surfaces-disagreeing failure ADR the-listing-owns-the-set exists to
prevent, arrived at through a different renderer.

This is ADR perch-says-what-it-did's line where ADR a-command-names-its-noun
last drew it — a machine reading a shape is not a person reading a sentence —
with the corollary that when the *shape* is the claim, the shape has to make it.
**The key names and the section shape are
#169's to carry out under this rule**, not this decision's to invent.

## The glossary is a person's vocabulary

**Nothing is added, and the reason is new.**

The case for adding **Listing** was the ADR a-command-names-its-noun pattern
exactly: the word does real work 49 times across `list.rs`, `cycle.rs` and
`model.rs` and is a term nowhere, which is the diagnosis that named
**Holdings**. And after ADR the-listing-owns-the-set the listing does carry an
idea — it is the surface that must not disagree with the Cycle's judgment.

It is refused on a discriminator the sweep has not used before. **Holdings** is a
word a *person* reaches for and had no term. "Listing" is a word the *codebase*
reaches for: of those 49 uses, none is user-facing, and surviving guide prose
uses it three times where a person says `perch list`. The glossary is the
vocabulary of somebody using Perch, not of somebody maintaining it.

The confirming count: `CONTEXT.md` holds sixty-two entries today, and not one of
them names a rendering. Every entry is a thing, an act, a state or a measure.
That is a property worth stating once rather than rediscovering — and the ideas
the listing carries are already entered, under **Cycle** for the ranking,
**Ungrouped** for held-rather-than-ranked, and **Headroom** for the column ADR
the-listing-owns-the-set adds.

This is the sixth ADR in this sweep to decline a `CONTEXT.md` entry and the
first to decline one for this reason.

## Considered Options

**Collapse to one command.** The strongest rival, and it turns entirely on
whether the labeled block survives. If `perch status` were `perch list` filtered
to one row, the two shapes would be identical and the count would follow — one
name. It was refused because a table of one row is a worse answer to "which
Account am I on" than a block is, and because the block is the only shape with
room to put the Quarantine above the figures. Naming the load-bearing premise is
half of recording the decision: **if the block goes, this decision is wrong.**

**Keep both exactly as they are.** The map holds that a re-affirmed choice is
worth more than a churned one, and both names *are* re-affirmed here. What is not
is the flag: `status --group` renders another command's output, and after #169 it
renders another command's first section. That is not a shape anybody chose.

**`--group` moves to `list` as a flag.** The smaller edit, refused above — an
argument-less flag would make `perch list` depend on there being an active
Account, and a flag naming a Scope would be the only one in Perch.

## Consequences

**The surface does not move: fifteen names, twenty-seven forms.** `--group`
leaves one command and arrives at another as an argument; no name is added,
removed or renamed, and no form appears.

**This corrects one sentence of ADR a-command-names-its-noun's body.** It closes
with "sixteen names, twenty-eight forms" — ADR a-command-names-its-noun's
figures from before ADR perch-does-not-draw removed `perch tui`, which took a
name and a form with it. The correct count then and now is fifteen and
twenty-seven. ADR a-command-names-its-noun's decision is untouched; the
arithmetic was never its finding, and a wrong number left standing is read as a
right one.

**This supersedes nothing, and ADR a-command-names-its-noun is answered rather
than amended.** Its "What this does not decide" section posed both halves of
this question — the count and the naming — and deferred them on purpose. A
decision that supplies an answer a prior decision asked for is not correcting
it.

**ADR a-figure-carries-its-age is untouched.** It cites `perch status --refresh`
and lists the surfaces that show Utilization as "`status`, `list`, `tui`". Those
citations go stale and the decisions do not decay —
ADR a-command-names-its-noun's rule, which this is the third occasion to apply.

**ADR the-listing-owns-the-set gains a reader rather than an amendment.**
"`perch list` is the whole of looking" was written about drawing; it is now also
true of listings, and the sentence needed no help to say so.

**`docs/guide/status.md` is restructured rather than edited.** It opens "Two
commands answer it" and then splits by breadth; it now splits by question — the
Account you are on, and everything Perch holds — with `--refresh` said once
against whichever is being shown. `reference.md`'s two rows change to
`perch status [--refresh] [--json]` and `perch list [<scope>] [--refresh]
[--json]`.

**`CONTEXT.md` is unchanged.**

**`tests/status.rs` still becomes `reporting.rs`**
(ADR a-suite-is-named-and-gated, tracked at #158), and that rename is
unaffected: ADR a-suite-is-named-and-gated chose the name so that "a rename of
`perch status` does not invalidate `reporting.rs`", and here `perch status` is
not even renamed. `listing.rs` keeps `list` and inherits the narrowing;
`refreshing.rs` keeps `--refresh` at both breadths.
