# A command names its noun

**A command is placed by the noun it is about. The Account is elided, because the
Account is the subject of the product. Every other noun is written, at depth
two.**

`CONTEXT.md` opens "Perch runs Claude Code as whichever Claude account you want",
so an Account command that named its noun would be naming it in every command.
That elision is the rule rather than an accident of it: the surface is
noun-then-verb throughout, with one noun so pervasive it goes unwritten.

| | |
| --- | --- |
| Top level, Account elided (10) | `add` `alias` `disable` `enable` `list` `relogin` `remove` `run` `status` `switch` |
| Top level, Perch itself (1) | `upgrade` |
| `config` (2) | `set` `get` |
| `group` (5) | `add` `remove` `rename` `move` `list` |
| `watcher` (5) | `run` `check` `install` `uninstall` `status` |
| `holdings` (3) | `export` `import` `purge` |

**Fifteen names, twenty-six invocable forms.** The count is not the finding, and
it is written down because a wrong number left standing is read as a right one.

Depth is capped at two, hard. `perch account group move` is what a noun-first
rule generates if nothing stops it, and depth three is where a CLI stops being
navigable. The cap forces a decision each time instead of letting the tree grow a
level.

## The rival is the smaller surface, and it is refused

The obvious alternative makes the top level *what you type while working* and puts
administration under its noun, the Account's included:

> `status` `switch` `run` `list` · `account` `config` `group` `watcher`
> `holdings`

Nine top-level names against fifteen. This rule is **one** idea whose line takes
no judgment — does an Account name what the command is about, yes or no. The rival
is **two** ideas, and the second has to be adjudicated per command, badly.
`perch relogin` is the way back from a Quarantine, which bites mid-task.
`perch disable` is what you type the moment you notice an Account misbehaving.
Both are "working" by any honest reading, and both would have been filed under
administration to keep the count down.

Under *conceptual surface first*, one sharp idea beats two ideas of which one
needs arguing about. Six saved names are not six ideas — they are six lookups.

## One capability, one name, one place

There is no aliasing: no `perch switch` standing for `perch account switch`, the
shape `docker` reached with `ps` beside `container ls`. It doubles the names *and*
adds the idea that two spellings are one thing, which is itself an idea a person
has to carry — and once both spellings exist neither is ever retired, so every
document has to pick one.

What aliasing normally buys is a way to move a name without breaking the people
typing the old one. Perch does not need buying off: the CLI surface is the side
that moves freely, renamed or cut with a `[**breaking**]` entry in
`CHANGELOG.md`, while the Holdings are what a changelog entry cannot give back
(ADR the-holdings-outlive-a-perch). A command's placement is therefore a decision
that can be got wrong and then corrected in the open, which is what makes this
rule load-bearing rather than decorative.

## Frequency, and the tiebreak that fires nowhere

Where frequency and taxonomy conflict, frequency wins: switching happens mid-task
under mild frustration, and a taxonomy that spent keystrokes there would be bought
at the price of the product.

**On this surface they never conflict.** Every hot command is an Account command,
and the Account is the elided noun, so the taxonomy already puts them where
frequency wants them. The tiebreak is recorded because it is what would decide the
case if a hot non-Account command ever appeared, and saying so plainly is better
than leaving a future reader to infer a rule from an outcome it did not cause.

## The three nouns that are written

### Holdings

`export`, `import` and `purge` share a noun that had no name. `CONTEXT.md` defines
Export as "*Everything Perch holds*, written to one file", Purge as "Giving the
machine back *every piece of state Perch holds*", and Import as the inverse of a
Purge — the same phrase doing the work three times and never a term. The family
was not missing a prefix; it was missing a noun, and the prefix is what a noun
gives it for free.

**The noun is Holdings**, and `CONTEXT.md` chose it rather than this document: it
is the word the glossary reaches for every time it needs the concept, including in
the Installation entry — "the counterpart to what Perch **holds**". An
Installation is what a Channel *left*; Holdings are what Perch *holds*; a Purge
takes the second without touching the first.

`Registry` understates the thing by exactly the part that matters, since an Export
writes the registry "alongside every Credential". `State` is the kind of word the
glossary's *Avoid* lines exist to catch. `Backup` is refused by `CONTEXT.md`
itself, which rules that "a backup is what one is *for*; the thing itself is an
Export".

The cost is three top-level names traded for one glossary term, on three commands
typed once each in a machine's life. It is worth it because the diagnosis is
right, and an exception here would be the rule conceding on the very case that
exposed it.

### The Watcher

`CONTEXT.md` already says these are one thing: the Watcher is "**Three
arrangements and one behavior**: a loop you can see and kill, a Service the
machine runs for you, or a sequence of Checks something else schedules. One of
them at a time." So one noun carries all three — `watcher run`, `watcher check`,
and `watcher install`/`uninstall`/`status`.

**Service keeps its glossary entry and has no tree of its own.** It names an
*arrangement* of the Watcher, which is why `install` and `uninstall` are the right
verbs for it and why it was never a rival noun. A tree per arrangement would have
been three trees for one behavior.

Two apparent collisions are not. `perch status` is an Account's and
`perch watcher status` is the Watcher's; `perch run` launches a client and
`perch watcher run` starts the loop. Same verb, different noun, and the elided
noun is precisely what tells them apart.

### `upgrade` stays at the top level

Its noun is the Installation, which is not an Account, so the rule appears to
demand `perch installation upgrade`. It does not, and no exception is needed:
**the noun is Perch, and you have already typed it.** `perch upgrade` reads as the
whole sentence. `CONTEXT.md`'s Upgrade entry anticipates it by opening "Names the
act rather than the command". `perch self upgrade` imports a concept from `rustup`
that Perch has nowhere else.

## Flag or verb

> **If it changes the meaning of the exit code or the lifetime of the command, it
> is a verb. Otherwise it is a flag.**

The discriminator cannot be *the glossary names it*: **Refresh** and **Check** both
have full entries and behave nothing alike. `perch watcher check` is a verb
because a Check "says what it decided in its exit code"
(ADR a-watcher-knob-is-arithmetic) against a loop that runs until you kill it —
both halves move. `--refresh` changes neither; it is the same answer differently
sourced, at a different cost. Neither do `--json`, `--yes` or `--check`.

**The test adjudicates a capability within a command's scope, and never how many
commands one capability is reached by.** Read past that boundary it condemns
`perch group add` and `perch group remove` — neither changes an exit code's
register or a command's lifetime, so both would be flags, and nobody has ever
proposed `perch group add <name> --undo`. A flag needs a verb to hang on, so
applying the test to a pair with no host means first choosing which of the two
becomes the host, which is the question rather than an input to it.

## A reversal takes its own name

`perch disable <target>` takes an Account out of Cycling and `perch enable
<target>` puts it back. They share an implementation — one `EnableCommand`, one
`run` — and they are two commands anyway.

Two verbs cost **one** idea, not two: a person holds *Disabled*, and knowing
`perch disable` tells them `perch enable` without a lookup.
`perch disable <target> --undo` inverts that, discoverable only by reading the
help, which is a lookup the pair does not charge. The maintenance surface is
shared either way, so a flag would not shrink it.

> **A flag may mark an argument's absence. It may not carry a verb's polarity.**

`perch alias <name> --unset` is not a counter-example, and the difference is the
argument list: its two forms take *different* arguments, and `--unset` is what
makes the Target's absence deliberate rather than a missing operand — which is
also the safety property, since a bare `perch alias work` would otherwise silently
free a name. `disable` and `enable` take the same argument in both directions, so
a flag there marks nothing absent and carries only the polarity.

**The positive state has no name.** An Account that has never been disabled is not
*enabled*; it simply is not disabled. `CONTEXT.md` has **Disabled** and no
**Enabled**, because the positive is the absence of the negative. A glossary names
states and a command surface names acts, and **undoing is an act you perform** —
it stays an act whether or not the state it restores has a name. So `perch enable`
on a fresh Account says it was already so and exits 0; neither verb ever reaches
`EXIT_NOTHING_TO_DO`, because a script that runs twice has not done anything
wrong.

Three spellings follow. `Account.disabled` is present-only in the registry, the
shape `quarantine` has and for the reason `quarantine` gives — a healthy Account
reads more clearly for saying nothing about its health. The listing's State cell
empties to the placeholder the Alias column already uses, so `disabled`,
`quarantined` and `disabled, quarantined` are the only things it says. And
`"disabled"` is present on every Account in `--json` unconditionally, because a
machine reading a shape is not a person reading a sentence
(ADR perch-says-what-it-did) and a script made to test for a key's presence to
learn a bool has a worse contract rather than a truer one.

## A command is named for what it does in every case

> **A command takes the glossary's word for its act only where the command and the
> act are the same size. Where the command is wider, it is named for what it does
> in every case rather than for the case that matters most.**

Every command whose act the glossary names takes the glossary's word: `remove` →
Remove, `purge` → Purge, `export` → Export, `import` → Import, `switch` → Switch,
`run` → Run, `upgrade` → Upgrade, `group rename` → Rename, `watcher check` →
Check. The ones that take no glossary word — `add`, `list`, `status`,
`disable`/`enable` — are exactly the ones where the glossary names no act.

`relogin` is the single case where a word for the act exists and the command
declines it, and the reason is that **the command is wider than the act**.
**Repair** is "Logging a *Quarantined* Account in again in place", while the
command is allowed on a healthy Account and behaves identically there, because a
Credential somebody suspects is going wrong should not have to break first before
it can be replaced (ADR a-broken-account-is-repaired). So `perch repair work` on a
healthy Account is a false sentence, and the only ways to make it true are
widening **Repair** — the vocabulary in `CONTEXT.md` is fixed — or narrowing the
command to Quarantined Accounts alone, which deletes a capability to make a name
fit.

The `re-` is load-bearing in its own right: `perch add` is also a login, so
`perch login` would collide with it conceptually while `relogin` says precisely
what distinguishes them — again, and in place.

**`alias` survives as a name** for two reasons that hold independently. `status`
is a noun-shaped name on this surface already, so noun-shaped is not
disqualifying; and `alias` is a verb in English and the oldest idiom in
command-line naming, so a person meets it already knowing what it does.
`perch rename` is refused by the vocabulary: **Rename** is reserved for Groups,
and the two are different acts — a Group's name is its identity and carries its
Overrides, its Accounts and its Cooldown, while an Alias is optional and
detachable.

## The thing acted on comes first

> **The thing a command acts on is its first argument. What it is being given
> comes after.**

`group rename <from> <to>`, `group move <target> <group>`, `perch list [<scope>]`
and every single-argument command take the subject first and the new value second.
`perch alias <target> <name>` is written the same way, and `--unset` takes the
Target rather than the name — which is a strict superset, since a Target resolves
an Alias before an email (`CONTEXT.md`, **Target**), so somebody who knows only
the address can free a name too.

The shell idiom does not argue for the inversion. `alias ll='ls -l'` is one
`name=value` token rather than two positional arguments, so it supplies the word
and is silent on the order.

## Admitting a command later

1. **Placement.** Does an Account name what it is about? Top level. Otherwise it
   goes under its noun, at depth two.
2. **The noun must already be in `CONTEXT.md`.** A command may not invent one.
3. **Flag or verb**, by the test above.
4. **A reversal takes its own name.** Where a command's whole effect is to undo
   another's, it is its own command rather than a flag on the one it undoes. The
   flag-or-verb test does not reach the question.
5. **A command is named for what it does in every case.** Where `CONTEXT.md` names
   the act and the command does no more than that act, the command takes that
   word. Where the command is wider, it is named for the whole of what it does.

Clause 2 is the one that will do the work. It makes growing the surface cost a
glossary entry, which is the only price that reliably deters it — and it is
exactly what would have caught `export`, `import` and `purge`, which shared an
unnamed noun and so had nowhere to live.

## The borderline cases, named

**`perch group move <target> <group>` takes an Account as its first argument.** It
stays under `group` because a Group is "a set of Accounts you have declared
interchangeable" — membership is the Group's property, and moving an Account
between Groups changes two Groups and nothing about the Account.

**`config` names an aggregate while `set` acts on a member.** `CONTEXT.md` defines
a Config as "Every Setting in force, taken together", so a Setting is always *in* a
Config and naming the aggregate names the member's home. `perch setting set` is
refused because its sibling `perch setting get` would print what is definitionally
a Config. A tree's verbs may act on the tree or on its members; depth two forbids
the nesting that would separate them.

## What this does not decide

**Naming**, beyond the two rules above. This governs *where* a capability lives
and what it may be called; it does not rename anything that already fits.

**The glossary gains nothing here.** Every act these names perform is either
already an entry or deliberately not one, and a glossary that named every act a
command performs would be a second copy of the surface.

## Consequences

Fifteen names and twenty-six forms, and a reader who learns `perch group add` can
predict where `perch holdings export` lives. What is bought is not a smaller
surface — nineteen looked-up names cost one idea if a rule places them and
nineteen facts if nothing does.

`perch watcher check` reports on ADR a-watcher-knob-is-arithmetic's table, and
exits with what it decided rather than with "it worked". `perch run` and
`perch upgrade` also exit with something other than Perch's own code, for reasons
their own commands hold.

A command that reaches nothing but the registry goes through the one door
(ADR one-door-to-the-registry), which is a statement about reach rather than about
placement and does not follow from anything here.
