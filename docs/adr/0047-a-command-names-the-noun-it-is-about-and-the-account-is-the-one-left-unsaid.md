# A command names the noun it is about, and the Account is the one left unsaid

> **Carried out in #164.** Like ADR 0041, ADR 0042, ADR 0044, ADR 0045 and ADR
> 0046, this is the artifact of a planning effort rather than of a change, so it
> landed ahead of the work it describes instead of beside it. The surface now
> matches it: `perch --help` lists sixteen names, `holdings` and `watcher` are
> the two new nouns, `--once` is `perch watcher check`, and `Holdings` is a
> glossary entry.

The question arrived as a choice of shapes: eighteen top-level commands in a flat
verb list — should it stay flat, become noun-then-verb, or collapse to a small
core of verbs with the rest as flags?

Both halves of that premise are wrong, and correcting them is most of the answer.

There are **nineteen** commands, not eighteen; `watch` is missing from the list.
And the surface is **not a flat verb list**. Three of the nineteen are already
noun-then-verb trees:

| | |
| --- | --- |
| Flat verbs (16) | `add` `alias` `disable` `enable` `export` `import` `list` `purge` `relogin` `remove` `run` `status` `switch` `tui` `upgrade` `watch` |
| Noun trees (3) | `config` set/unset/get · `group` add/remove/rename/move/list · `service` install/uninstall/status |

Nineteen top-level names, **twenty-seven invocable forms**. So the real question
is not *flat or nested* — the surface is both. It is: **why did those three get a
noun, and the other sixteen not?**

## The rule was already there

Look at what `config`, `group` and `service` have in common. They are **the three
things that are not an Account**.

Every flat verb but four acts on an Account: `add` `alias` `disable` `enable`
`list` `relogin` `remove` `run` `status` `switch` `tui`. The noun is elided
because the Account is the subject of the entire product — `CONTEXT.md` opens
"Perch runs Claude Code as whichever Claude account you want". Saying it would be
saying it in every command.

That is a principled elision rather than an accident, and it means the surface has
been noun-then-verb all along, with one noun so pervasive it went unwritten. Only
**four** commands break it. `export`, `import` and `purge` act on the whole of
what Perch holds rather than on one Account. `watch` is a bare verb for the same
noun `service` names.

The guide has known this for longer than `main.rs` has. Its reference table has
**eighteen rows for nineteen commands**, because it already pairs `disable`/`enable`
and `export`/`import` as families and splits `switch` across two. The documentation
thinks in families; only the dispatch thinks in a list.

## The rule

> **A command is placed by the noun it is about. The Account is elided, because
> the Account is the subject of the product. Every other noun is written, at
> depth two.**

Depth is capped at two, hard. `perch account group move` is exactly what a
noun-first rule generates if nothing stops it, and depth three is where a CLI
stops being navigable. The cap costs nothing today and forces a real decision each
time instead of letting the tree grow a level — the kind of forward-looking guard
`CLAUDE.md` keeps.

## Why not the smaller surface

The obvious rival was to make the top level *what you type while working* and put
administration under its noun, including the Account's:

> `status` `switch` `run` `list` `tui` · `account` `config` `group` `watcher`
> `holdings`

Ten top-level names against this decision's sixteen. It was refused, and the
yardstick is why.

This rule is **one** idea, and its line takes no judgment: does an Account name
what the command is about, yes or no. The rival is **two** ideas, and the second
one has to be adjudicated per command — and adjudicated badly. `perch relogin` is
the way back from a Quarantine, which is a thing that bites you mid-task. `perch
disable` is what you type the moment you notice an Account misbehaving. Both are
"working" by any honest reading, and both would have been filed under
administration to keep the count down.

Under *conceptual surface first*, one sharp idea beats two ideas of which one
needs arguing about. Six saved names are not six ideas — they are six lookups.

## Why one capability gets one name

Aliasing was refused: no `perch switch` standing for `perch account switch`, the
shape `docker` reached with `ps` beside `container ls`.

It is the most expensive thing available under this yardstick. It doubles the
names *and* adds the idea that two spellings are one thing, which is itself an
idea a person has to carry. Docker is the cautionary tale rather than the model —
the short forms were never retired, so both live forever and every document has to
pick one.

Aliasing normally buys backwards compatibility. `CLAUDE.md` says there is nothing
to be compatible with. So: **one name, one place** — which is what makes the rule
load-bearing rather than decorative, since every command's placement is now a
decision that can be got wrong.

## Frequency, and the rule that fires nowhere

Where frequency and taxonomy conflict, frequency wins. ADR 0011 is built on
switching happening mid-task under mild frustration, with the shortest command
doing the whole job, and a taxonomy that spent keystrokes there would be bought at
the price of the product.

**On this surface they never conflict.** Every hot command is an Account command,
and the Account is the elided noun, so the taxonomy already puts them where
frequency wants them. The tiebreak is recorded because it is true and because it
is what would decide the case if a hot non-Account command ever appeared — not
because it does any work today. Saying so plainly is better than leaving a future
reader to infer a rule from an outcome it did not cause.

## The four that move

### Holdings

`export`, `import` and `purge` have no shared prefix because **the noun they share
has no name**. `CONTEXT.md` defines Export as "*Everything Perch holds*, written
to one file", Purge as "Giving the machine back *every piece of state Perch
holds*", and Import as the inverse of a Purge. The same phrase does the work three
times and is never a term. `registry` appears in the prose, lowercase and
ungoverned, standing in for it.

So the family is not missing a prefix. It is missing a noun, and the prefix is
what a noun would have given it for free.

**The noun is Holdings**, and the document chose it rather than this decision: it
is the word `CONTEXT.md` reaches for every time it needs the concept, including in
the Installation entry — "the counterpart to what Perch **holds**". That contrast
is the entry's whole point and now lands cleanly: an Installation is what a Channel
*left*, Holdings are what Perch *holds*, and a Purge takes the second without
touching the first.

`Registry` was considered and refused: Export writes "the whole registry
**alongside every Credential**", so Credentials are not in it and the word
understates the thing by exactly the part that matters. `State` was refused as the
kind of word the glossary's *Avoid* lines exist to catch. `Backup` was refused by
`CONTEXT.md` itself, which already rules that "a backup is what one is *for*; the
thing itself is an Export".

The cost is honest and worth stating: this trades three top-level names for one
new glossary term, on three commands typed once each in a machine's life. It is
worth it only because the diagnosis above is right — an exception here would be the
rule conceding on the very case that exposed it.

### The Watcher

`CONTEXT.md` already says these are one thing: the Watcher is "**Three
arrangements and one behavior**: a loop you can see and kill, a Service the
machine runs for you, or a sequence of Checks something else schedules. One of them
at a time."

The surface splits that one noun three ways — a bare verb, a flag on it, and an
unrelated tree. This is the sharpest instance of the diagnosis anywhere on the
surface: the domain model is more coherent than the dispatch, and has been all
along.

| Now | Then |
| --- | --- |
| `perch watch` | `perch watcher run` |
| `perch watch --once` | `perch watcher check` |
| `perch service install\|uninstall\|status` | `perch watcher install\|uninstall\|status` |

**Service keeps its glossary entry and loses its tree.** It names an *arrangement*
of the Watcher, which is why `install` and `uninstall` are the right verbs for it
and why it was never a rival noun. A tree per arrangement would have been three
trees for one behavior.

Two apparent collisions are not. `perch status` is an Account's and `perch watcher
status` is the Watcher's; `perch run` launches a client and `perch watcher run`
starts the loop. That is the rule working rather than failing — same verb,
different noun, and the elided noun is precisely what tells them apart. A surface
where the same verb means the same act against different subjects is the surface
this rule is for.

### `upgrade` stays where it is

Its noun is the Installation, which is not an Account, so the rule appears to
demand `perch installation upgrade`.

It does not, and no exception is needed. **The noun is Perch, and you have already
typed it.** `perch upgrade` reads as the whole sentence: perch, upgrade yourself.
`CONTEXT.md`'s Upgrade entry anticipates this — it opens by distinguishing the act
from the command, "Names the act rather than the command". `perch self upgrade`
was refused for importing a concept from `rustup` that Perch has nowhere else.

## Flag or verb

Needed, because promoting `--once` to `check` spends it.

The discriminator cannot be *the glossary names it*: **Refresh** and **Check** both
have full entries and behave nothing alike. The one that works:

> **If it changes the meaning of the exit code or the lifetime of the command, it
> is a verb. Otherwise it is a flag.**

`--once` changes both — a Check "says what it decided in its exit code" against
ADR 0013's table, where the loop runs until you kill it. So it is a verb, and
becomes one. `--refresh` changes neither; it is the same answer differently
sourced, at a different cost. `--json`, `--yes`, `--group` and `--check` change
neither. **No flag other than `--once` is affected.**

## Admitting a command later

1. **Placement.** Does an Account name what it is about? Top level. Otherwise it
   goes under its noun, at depth two.
2. **The noun must already be in `CONTEXT.md`.** A command may not invent one.
3. **Flag or verb**, by the test above.
4. **A reversal takes its own name.** Where a command's whole effect is to undo
   another's, it is its own command rather than a flag on the one it undoes. The
   flag-or-verb test does not reach the question.

Clause 4 was added by ADR 0052, which amends this decision in that one clause
and nothing else. Its decision, its table and its counts are untouched.

Clause 2 is the one that will do the work. It makes growing the surface cost a
glossary entry, which is the only price that reliably deters it — and it is
exactly what would have caught `export`, `import` and `purge`, which invented an
unnamed noun and then had nowhere to live.

## The borderline cases, named

**`perch group move <target> <group>` takes an Account as its first argument.** It
stays under `group` because a Group is "a set of Accounts you have declared
interchangeable" — membership is the Group's property, and moving an Account
between Groups changes two Groups and nothing about the Account.

**`config` names an aggregate while `set` acts on a member.** `CONTEXT.md` defines
a Config as "Every Setting in force, taken together", so a Setting is always *in* a
Config and naming the aggregate names the member's home. `perch setting set` was
refused because its sibling `perch setting get` would print what is definitionally
a Config. A tree's verbs may act on the tree or on its members; depth two forbids
the nesting that would separate them, and that is the cap earning its keep.

## What this does not decide

**Naming.** This rule governs *where* a capability lives and says nothing about
what it is called. Two things are therefore untouched: `tui` is a jargon acronym
where every other top-level name is a word, and `status` and `list` are two names
over one dataset. Both exclusions are principled. The picker's own survival is
still open, so naming it now is work with a live chance of not surviving; and
`status` against `list` is a question of whether they are *one command*, so ruling
on their names before ruling on their number would decide in the wrong order.

**The collapses.** `disable`/`enable` as two verbs for one boolean, `status`
against `list`, and whether the backup family is three commands or fewer. All are
questions of *how many* commands there are. This decision settles where each one
lives, which is what those questions were waiting on, and none of them is
foreclosed by it.

**ADR 0011 is re-affirmed without touching its text.** Its constraint — every
capability the interactive view offers must exist non-interactively, because Perch
has to be complete over SSH and in scripts — is untouched and still governing.
Nothing here moves a capability into or out of the picker.

## The glossary

**Holdings** is added: everything Perch holds on this machine — every Profile,
every Credential Perch holds, the registry naming them and what each Group carries.
The counterpart to an Installation, which is what a Channel left. What an Export
writes, an Import puts back and a Purge gives up, and the reason none of the three
takes a Target.

**Export**, **Import** and **Purge** keep their entries and tighten by the new
word: three definitions currently spell out "everything Perch holds" because they
had to, and can now say Holdings.

**Check** loses one citation. Its entry names the flag — "One round of the Watcher
taken on its own — `perch watch --once`" — and that becomes `perch watcher check`.
It is the only path `CONTEXT.md` cites that this decision moves; `perch status
--refresh`, `perch relogin` and `perch upgrade` are all unaffected.

**Watcher** and **Service** are untouched. The Watcher entry's "three arrangements
and one behavior" is what this decision implements rather than something it
changes, and Service cites no path.

## Consequences

**Top-level names go from nineteen to sixteen. Invocable forms go from
twenty-seven to twenty-eight.**

The second number is the honest one, and it goes the wrong way: promoting `--once`
to a verb adds a form. **This shape does not shrink the surface, and must not be
sold as though it does.** The count was never the defect — nineteen looked-up names
cost one idea if a rule places them and nineteen facts if nothing does. What is
bought is that a person now holds one rule and a lookup, and that a reader who
learns `perch group add` can predict where `perch holdings export` lives.

| | |
| --- | --- |
| Top level, Account elided (11) | `add` `alias` `disable` `enable` `list` `relogin` `remove` `run` `status` `switch` `tui` |
| Top level, Perch itself (1) | `upgrade` |
| `config` (3) | `set` `unset` `get` |
| `group` (5) | `add` `remove` `rename` `move` `list` |
| `watcher` (5) | `run` `check` `install` `uninstall` `status` |
| `holdings` (3) | `export` `import` `purge` |

**Ten older ADRs cite the moving paths** — 0013, 0014, 0015, 0021, 0025, 0034,
0036, 0040, 0042 and 0044. **None is amended.** A citation going stale is not a
decision decaying, and this repository supersedes records rather than editing them.
ADR 0040 in particular is untouched and still governing: that the Watcher may be
run for you by the machine's service manager is unaffected by which words invoke
it.

**This supersedes nothing.** It is the first record of a rule the surface has been
following imperfectly since ADR 0011, and there is no prior decision to correct —
which is itself the finding. The shape was never chosen; it accreted, and three
commands happened to get a noun because their nouns happened to have names.

**Nothing about behavior changes.** No exit code moves, no flag other than
`--once` is affected, `perch watcher check` reports on ADR 0013's table exactly as
`perch watch --once` did, and no capability is added or removed. This is a decision
about where twenty-eight capabilities are reached from, and about the one sentence
that says why.
