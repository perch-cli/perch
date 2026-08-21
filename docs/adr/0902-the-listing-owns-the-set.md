# The Listing owns the set

**`perch status` is the Account you are on. `perch list` is the Listing, at every
breadth.** Two shapes answering two questions — one Account in detail, and a set
as a table — and one shape per command.

`perch status` bare is not the Listing narrowed. Against a row it adds
`Organization` and `Plan`, and it has room to put the Quarantine sentence *above*
the figures, because the state is the news and the numbers are the detail — a
shape a table cannot hold. Its document is one object where the Listing's is an
array of sections. Two names each with one shape is fewer ideas than one name with
two renderings.

## The ranking belongs to the Listing

`perch switch` ranks the Accounts in a Scope by Headroom and lands on the best of
them. That ranking is visible rather than hidden, so the two surfaces cannot come
to disagree about which Account is better — and `perch list` is where it is
visible. Three pieces, always and not behind a flag:

- **The Cycle's order.** Each Scope ranks its own Accounts by its own Strategy,
  and the Scope the active Account is in comes first, because it is where you are
  and, wherever a Cycle happens at all, the one a bare `perch switch` looks in.
- **A Headroom column**, distinct from the Utilization printed beside it.
  Utilization is every Quota Window, one line each; Headroom is the *worst* of
  them (ADR headroom-is-the-worst-window), said as the single number the ranking
  sorted on. Without it the order is a claim the table gives no way of checking.
- **Held rather than ranked** for the Accounts in no Group until `interchangeable`
  is declared. This carries the most weight of the three: a ranking of Accounts
  Perch would refuse to choose between is the hidden claim this Listing exists to
  prevent, and it is just as false in a plain-text table as in a drawn one
  (ADR a-group-is-a-declaration).

The argument was never that the ranking should be *available*; it was that the two
surfaces must not disagree, and an optional agreement is not one.

## A breadth is an argument, not a flag

> **`perch list [<scope>]`** — a Group by name, or `ungrouped`.

The domain is Scopes rather than Groups, since an ungrouped Account narrows to the
Ungrouped, and Perch already addresses a Scope positionally: `perch config` names
one that way, and `validate_name` refuses `ungrouped` as a Group name *and* as an
Alias so the Scope can answer to it. A flag would be the only place in Perch a
Scope is named by flag, and a flag may mark an argument's absence but not carry
what an argument carries (ADR a-command-names-its-noun).

An argument-less flag is worse than merely inconsistent. **`perch list` must keep
working when Perch holds no active Account** — precisely the state `perch status`
refuses in and sends somebody to `perch switch` to leave — and a narrowing that
read the active Account to know what it meant would couple the Listing to a fact
it does not need.

An Alias or an email address is not a Scope and is not accepted as one. A Listing
of one row is what `perch status` answers better, so a name that is somebody's
Alias is answered as the Group it is not.

**`--refresh` follows the breadth**, because it always has: it reads exactly the
Accounts about to be shown and no others. Two commands reach a refresh and that is
not two capabilities — each refreshes what it shows, which is one rule applied to
two shows. Reading a set spends breadth-many independent budgets rather than
exhausting one (ADR a-figure-carries-its-age), so breadth is not what the network
has to be kept away from.

## What the Scope has left

**The Reserve is said under a narrowed Listing and nowhere else on the human
surface** — how many of a Scope's Accounts still have Headroom, and how much the
best of them has.

It attaches where a heading has already named the Scope the sentence is about. A
bare `perch list` has no heading: it is **one flat table** across every Scope with
the Group as a column, because a table that broke for a heading every few rows
would put a blank line between Accounts the eye is running down a column of. A
Reserve line there would have to name its own Scope, once per Scope, in a footer —
a heading smuggled into a sentence already as wide as a terminal, which is the
worse half of the shape the table declined. Narrowed, the heading is there and has
said it, and the footer already holds the Listing's other Scope-level sentence.

The Ungrouped stay silent until `interchangeable` is declared, off the same answer
that declines ranking them: a Reserve is what a set of Accounts has *between
them*, which is the claim nobody has made about them yet. Ranking them and saying
what they have left are declined together rather than separately.

A Scope holding nobody keeps the sentence it already has — "The Group `spare` holds
no Accounts yet." — which names the Group and says the one thing true of it. A
Reserve branch for that case would describe a Scope somebody has only just declared
as full of Accounts nothing may reach.

**Per-window rows are not part of it.** Saying the emptiest Account per Quota
Window answers "fine on the weekly window, empty on the five-hour one", and on this
surface each such row would summarize numbers already on screen a few lines above
it, because `perch list` prints every Account's per-window figures. That is an
argument about this surface rather than about the idea.

## A document says what its order is, or it does not have one

The Accounts arrive in `sections` rather than in one flat `accounts` array. The
order is load-bearing — `accounts[0]` of the first section is the Account a bare
`perch switch` would land on — and a flat array states that nowhere, so a script
reading it would rely on a ranking the document never claimed to be making. Worse,
the held-versus-ranked distinction would be invisible entirely, and a `--json`
showing a ranking of Accounts Perch would refuse to choose between is the
two-surfaces-disagreeing failure reached through a different renderer.

One `accounts` array beside the sections is the same mistake twice: a shape that
makes no claim, kept for scripts, next to the shape that makes it.

**`--json` carries the Reserve at every breadth**, and that is not a disagreement
with the table's silence. A section names its own Scope in a key, which is the
whole of what the table lacks. The table's silence is a rendering constraint rather
than a domain one — the same document already spells out `order`, which the table
expresses only by not sorting. What must not happen is the two surfaces disagreeing
about a *judgment*: a ranking on one that the other would not make. Here neither
claims anything the other denies; one of them declines to say a true thing for want
of somewhere to put it.

Three shapes follow from the same reasoning:

- **Fields, not the sentence.** The Listing's document is structured throughout,
  and a prose sentence in a document is a thing scripts end up regexing.
- **`null` where no Cycle could happen in the Scope**, saying in the document the
  same "there is no answer here" the human surface says by silence.
- **`null` for a Scope holding nobody.** Not ambiguous against the first, because
  `accounts` sits beside it: an empty array distinguishes "nobody is here" from
  "nobody has declared these a set".

**One Account has one shape wherever it is read.** `perch status --json` describes
its Account with the same object a section's `accounts` array uses, so a script
asking which Group the active Account is in does not need a second command, and one
written against either can be pointed at the other. What each *document* answers
still differs, and that is the part that should — `active` against `sections` is
what says which was asked.

The Utilization sits under `active` and nowhere else. A copy at the top level would
be insurance against reaching into the wrong shape, and this document answers about
exactly one Account and cannot be anything else, so there is nothing left to insure
against.

## The names stay

`status` is the only noun among the top-level Account commands, and that is not a
defect: it names the thing asked for, and it is asked for at two levels.
ADR a-command-names-its-noun defends `perch status` against `perch watcher status`
as the rule working — same verb, different noun, and the elided noun is what tells
them apart — and that defense needs the word to be the same at both levels.
Renaming the Account's one would orphan the Watcher's as a lone noun. `status` is
also the most conventional name any CLI has, and a convention every user already
holds is conceptual surface bought for nothing.

## The glossary

**Listing** and **Section** are entries, and **Section** is the one carrying the
weight: it is where ranked-versus-held lives, and `--json` states it as
`"order": "ranked" | "held"` in a contract scripts branch on
(ADR code-lives-where-it-reaches). An idea a script branches on is an idea a
person holds.

Neither entry names a *rendering*, which is the distinction that makes them
sayable at all. **Listing**'s `_Avoid_` line rules out `table`, `view`, `report`
and `output` for exactly that reason: the Listing is every Account Perch holds in
the Scopes they sit in and in each Scope's own order — a thing, described — and a
table is one way of drawing it. On the same ground the register this document
writes in gets no noun of its own (ADR perch-says-what-it-did).

**Reserve** says Scope rather than Group, matching how `Reserve::of` is typed and
the case the gate admits.

## Considered options

**One command, with a flag deciding the breadth.** The strongest rival, and it
turns entirely on whether the labeled block survives. If `perch status` were
`perch list` filtered to one row, the two shapes would be identical and the count
would follow. It is refused because a table of one row is a worse answer to "which
Account am I on" than a block is, and because the block is the only shape with room
to put the Quarantine above the figures. **If the block goes, this decision is
wrong.**

**A breadth flag on `status`.** Refused: it renders the other command's output, and
after sectioning it renders that command's *first section*.

**A Reserve line per Scope under the bare Listing.** Refused above. It is also the
option that cannot be walked back once scripts and screenshots have seen it —
narrowed-only is the subset of it, and can be widened later.

**Breaking the bare Listing into per-section tables with headings.** The honest way
to have Reserve lines everywhere, and a much larger change: it spends the column of
Accounts the eye runs down.

**Leaving the Reserve unsaid.** The per-Account State and Headroom columns do say
enough for one person on one machine. Refused because the count is the one thing
they cannot say: "two of these three are worth switching to" is a fact about the
set, and reading it off the column is arithmetic the reader does rather than a
sentence Perch says.

## Consequences

`perch list` is the whole of looking and `perch switch` the whole of choosing.
Every Account Perch holds appears in exactly one Section, because the Scopes
partition the registry — every declared Group, then the Accounts in none — and what
holds that partition up is `load` declaring any Group an Account claims. An Account
that fell between them would simply not be printed, with nothing anywhere to say
so.

The Listing's footer is collected before it is written, so the blank line that
separates it from the table is decided by whether there is anything down there at
all rather than by a condition each new sentence has to remember to join.

`out_of_the_running` is shared rather than private: the refusal that nobody can be
Cycled to and the Reserve that says what is out of the running are the same count
of the same Accounts.

No exit code changes and no new one is added.
