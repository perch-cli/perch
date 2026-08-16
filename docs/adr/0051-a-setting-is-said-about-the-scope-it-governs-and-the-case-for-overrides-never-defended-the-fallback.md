# A Setting is said about the Scope it governs, and the case for Overrides never defended the fallback

> **Carried out in #173.** Like ADR 0041 and ADR 0046 through 0050, this is the
> artifact of a planning effort rather than of a change, so it landed ahead of
> the work it describes instead of beside it. The tree now matches it: `Global`,
> `Override` and `Inherit` are gone, `registry::Scope` and `cycle::Scope` are one
> type, `perch config unset` is deleted, `cycle-ungrouped` is `interchangeable`
> and carried by the Ungrouped Scope alone, and the registry's `version` is `2`.

ADR 0046 left this behind as a count: five nouns — Group, Scope, Override,
Ungrouped, Global — serving three Settings, one of which uses none of them. The
count is wrong in both directions, and correcting it is most of the decision.

`CONTEXT.md`'s Configuration section holds **seven** entries: Setting, Config,
Scope, Global, Ungrouped, Override, **Inherit**. The list of five names **Group**,
which is not in that section and is not configuration's to delete — a Group is
the Cycling boundary (ADR 0002), and it exists whether or not it carries a value.
It names **Ungrouped**, which `cycle::Scope` and `cycle::may_cycle_within` need
whatever happens here. And it omits **Inherit**, which is machinery and nothing
else.

So three nouns are actually on trial: **Global, Override and Inherit**. All three
are the same idea — a value said in one place that a narrower place falls back
to. The per-Scope *value* was never in question. The **fallback** was.

## The strongest surviving case defends the wrong thing

ADR 0046 put the best remaining argument for Overrides on the record and handed
it here as evidence: *a work Group wanting a different threshold from a personal
one.*

That is a case for a per-Scope **value**. An Override is a per-Scope value **that
falls back to Global**, and the case is silent on the falling back. Both survive
it: a design where each Scope simply holds its own threshold serves the work
Group and the personal one exactly as well, and serves them without a layer.

The evidence pointing the other way is thin and worth stating with its limits.
The only registry that exists — the author's, on this machine — holds two
Accounts, two Groups (one of them a dogfood artifact), `ungrouped: null`, and
Global at every compiled default. **Not one Override has ever been written.**
Twelve days and one user is not a track record, and this is decided a priori as
#151 and ADR 0050 were. But nothing is being taken away from anybody.

## The asymmetry the count leaned on does not exist

ADR 0017's amendment, `commands/watch.rs:523`, `commands/config.rs:869` and
`docs/guide/configuration.md:86` all state the same rule: `watcher-may-act` does
not reach the Accounts in no Group by Inheritance, so the watcher acts there only
on two independent yeses.

The code does not do this. `permitted` gates on `cycle_ungrouped` at
`commands/watch.rs:531`, then reads `registry.in_force(&scope.config())` at
`:548` — and `Overrides::over` inherits `watcher_may_act` like every other field
(`registry.rs:390`). Four cases, and the rule holds in three:

| `cycle-ungrouped` | Ungrouped Override | Global `watcher-may-act` | What happens |
| --- | --- | --- | --- |
| off | — | — | refused, on the declaration |
| on | `true` | — | acts — correctly, two yeses said |
| on | none | `false` | refused, on the permission |
| on | none | `true` | **acts, on one yes** |

The two tests guarding this each turn off one of the two keys
(`tests/scheduling.rs:228` and `:261`), so neither reaches the fourth row —
including the one whose doc comment calls itself "ADR 0017, amended, in
executable form" and "the case most likely to be 'fixed' by somebody tidying the
layering into uniformity, which is why it names the ADR". It named the ADR and
still did not assert it.

**The rule is kept, and the finding is that it could not survive being
hand-maintained.** An exception written into a uniform layer is an exception
somebody has to keep re-deciding, and the record shows what happens: it was
described in four places, tested in two, and enforced in none.

## Every Setting belongs to one Scope

**A Scope — each Group, and the Ungrouped Accounts — holds its own full
Settings. There is nothing above it. Defaults are compiled-in constants.**

Global, Override and Inherit go with the layer. So does `perch config unset`,
which exists only to return a Scope to Inherit, and so does the idiom where the
number of words says which layer a value came from — every `set` is now three
words, because every Setting has a subject.

Two things follow that the counting does not capture.

**`registry::Scope` merges with `cycle::Scope`.** `registry.rs:461` explains at
length why they are deliberately different types: "sharing the type would put
Global within reach of the ranking, which is the one thing ADR 0002 is about."
That hazard cannot occur once there is no `Global` variant, so the two become one
type and the warning becomes a sentence about a mistake nobody can make.

**The struct was already the right shape, under the wrong name.**
`GlobalConfig { cycle_ungrouped, settings }` pairs "may these Accounts be Cycled
among" with "the Settings governing how" — which is a record of the Ungrouped
Scope, and has never been a record of anything else. `cycle_ungrouped` has no
meaning at Global, which is why `commands/config.rs:285` and `:344` both carry a
special case printing it against the Ungrouped Scope "because that is where it
takes effect". The struct survives with its Scope corrected; `ungrouped:
Overrides` merges into it, `groups` becomes a map of `Settings`, and both special
cases go.

### A grant is said about the Scope it grants

`strategy` and `watcher-threshold-percent` are taste. `watcher-may-act` is
consent, and layering consent is what produced the hole above. It also produces a
quieter one that has never been examined: a Global `watcher-may-act true`
authorises **every Group, including ones not yet declared**, so
`perch group declare X` yields a watcher-enabled Group nobody said anything
about. ADR 0017 worried about exactly that shape for ungrouped Accounts and
accepted it there deliberately; for Groups it arrived unexamined, by inheritance.

Saying a grant only about the Scope it grants makes ADR 0017's two yeses
structural. The fourth row of the table above is not fixed — it cannot be
written down.

**The cost is real and is accepted: there is no longer one command that withdraws
the watcher everywhere.** ADR 0040's held Service is paused by withdrawing the
grant, and that becomes one command per Scope. The grant was never the only
brake — the loop is a foreground process you kill and a Service is one you stop —
and a brake that works by blanket inheritance is the wrong brake for consent.

### The shallow end goes

Today `perch config set watcher-threshold-percent 70` works knowing nothing about
Scopes; the layer is progressive disclosure, and Override is met only by somebody
who needs it. That is a genuine property and it is given up. It is shallow only
until the second Group exists, at which point the layer must be learned anyway
and the two-word form becomes ambiguous in the reader's head — *did that change
`work`?* — and the refusal that replaces it teaches the one idea it is refusing
over.

**Reading is not writing.** Bare `perch config get` survives and prints every
Scope's Config in full. A read has no subject to be wrong about; a write does.
That is the rule rather than an exception to it.

**An implicit Scope was considered and refused.** A missing Scope could mean the
Scope of the Account you are on — the shallow end restored, without Global. ADR
0047's elision works because the unsaid noun is *always the same one*; here the
unsaid subject changes with whichever Account happens to be active, so the
identical command means different things on different days. Tolerable for a
Switch, which says what it did. Not tolerable for a rule that persists after the
sentence has scrolled away.

## `cycle-ungrouped` becomes `interchangeable`

With no Global there is no "Global's alone", and the key moves onto the Scope it
has always governed. **It is renamed `interchangeable` and is carried by the
Ungrouped Scope alone**, absent from a Group's page:
`perch config set ungrouped interchangeable true`.

The name repeating its own Scope is the defect ADR 0047 found in commands, and
the same answer applies. `may-cycle` was the runner-up — it pairs with
`watcher-may-act` and matches `cycle::may_cycle_within`, the predicate it already
is — and was refused because it recasts a declaration as a grant. ADR 0017 is
careful that this is *a person declaring their Accounts substitutable*, not *a
permission handed to Perch*, and that distinction is the whole reason the two
yeses are two rather than one thing said twice. Two grants side by side would
read as redundant. A declaration and a grant do not.

A Group carries no such line, because a Group **is** that declaration. Printing
`work interchangeable true` and refusing to set it would break the invariant the
guide leans on — every line `perch config get` prints is the tail of the
`perch config set` that would restore it — so the honest form is silence. One
Scope carrying a key the others do not is the asymmetry that exists today,
relocated from a layer where it meant nothing to the Scope where it takes effect,
at two special cases less.

## `unset` goes, and nothing replaces it

`perch config unset` returns a Scope to Inherit. With no Inherit there is nothing
to clear, and a value is simply set to what it should be. Keeping it as "back to
the compiled default" was considered and refused, because the moment the default
is a thing a command returns you to, `perch config get` wants to annotate
`(default)` beside values nobody set — and **a value that knows whether it was
set is Inherit under another name**, tracking restored, the entry deleted from the
glossary and the idea still in the code. The annotation is refused with the
command.

The word survives elsewhere: `perch alias <name> --unset` is untouched and is a
different act, freeing a name rather than clearing a layer.

## `global` stays a reserved word

`registry.rs:718` refuses `global` as a Group name and as an Alias, on a hazard
that dies here: taking `perch group add global` would make every later
`perch config set global …` write a Group Override while Global stayed as it was.

The word does not die with it. Somebody typing
`perch config set global watcher-may-act true` means **everywhere**, and there is
no everywhere — so that has to produce a refusal saying so, rather than a Group
quietly named `global` taking the value. **The reservation is kept and its reason
rewritten**: `global` is how people say "every Scope at once", and the refusal is
where they learn Perch has no such layer, which is a better place to learn it
than from a Setting that appeared to take. `CLAUDE.md` keeps forward-looking
guards that cost nothing, and this one is a predicate that already exists.
`ungrouped`'s reservation and its reason are untouched.

## The glossary

**Three entries are deleted: Global, Override, Inherit.** The Configuration
section goes from seven to four.

**Scope** survives, cut to "a Group, or the Accounts in no Group taken together".
Dissolving it was considered — with two members that both have names, the word
looks like a wrapper — and refused on the grounds `Scope::within` already states
in the code: "three spellings of 'among the Accounts in no Group' is how two of
them come to name the same set differently". Its sentence about being *the only
levels at which a Setting means anything* is kept, and is now the whole of the
layering rather than the bottom of it.

**Ungrouped** gains the declaration it now carries. **Rename** loses Global from
"Global and the Ungrouped Accounts are not Groups", and its Overrides become
Settings — a Renamed Group still brings everything with it, and declaring one and
forgetting another still loses the lot. **Setting**, **Config** and **Group** are
untouched; ADR 0002's "a Group carries the settings that govern when it may
Cycle unasked" is more literally true after this than before it.

**Nothing is added.** A word for the compiled-in defaults was the one candidate
and is Inherit's ghost. Five ADRs in this sweep have declined to add an entry;
this is the first to subtract, and the reduction is measured in ideas rather than
in lines.

## What this supersedes

**ADR 0002's amendment, "Global carries the defaults and a Group Overrides them"
— superseded in full.** Every sentence in it is about Global, Override, Inherit,
the two layers or the word-count idiom. **ADR 0002's body is untouched**, and
better than untouched: "A group also carries configuration: whether the watcher
may switch accounts within it unattended, at what utilization threshold, and
which strategy it prefers" names precisely the three Settings a Group now holds
outright, which is a sentence the amendment made approximate and this restores
word for word. The second leg Groups stand on is strengthened rather than cut.

**ADR 0017's amendment, "the Ungrouped Accounts are a Scope that reads Global" —
superseded in full.** Its core claim survives and is strengthened: they are a
Scope, and now hold Settings outright rather than Overrides over something else.
What goes is "reads Global" and the whole non-uniform-layering paragraph, which
describes an exception that can no longer be expressed.

**ADR 0017's body loses one sentence.** "The setting is global rather than
per-Group" is now false. The two sentences of reasoning under it survive intact —
an ungrouped Account has no Group to carry the declaration, and modelling the
ungrouped pool as a Group with a reserved name would make a Group mean two
contradictory things — and both are still why the Ungrouped Accounts are a Scope
and never a Group. The Consequences section's "the shape of that form is for the
configuration spec to settle, along with the key's name" is settled here.

Superseding ADR 0017 whole was considered and refused, on ADR 0046's precedent:
supersede the section that decayed and leave a mostly-correct record standing
rather than restating it at length. 0017's record is longer and cleaner than
0013's was, and superseding it entire to correct one sentence whose *reasoning*
survives would bury three Considered Options that are the only written record of
why ungrouped Accounts do not Cycle freely.

**ADR 0046 is answered, not superseded.** This is the question it produced and
stopped at. One clause of its "What this does not decide" decays —
`watcher-threshold-percent` is still per-Scope, but no longer "Overridable per
Scope exactly as it is" — and its decision, which knobs survive, is untouched.

**ADR 0047 and ADR 0040 are unamended.** `perch config unset` leaving is a
consequence for the carry-out rather than a revision of 0047, which this obeys;
and 0040's held Service still holds on a grant it still reads, at a Scope it
already asks about.

## Consequences

**`perch config` keeps three Settings and loses a form.** `get` and `set`
survive; `unset` goes. Every `set` is `<scope> <key> <value>`; every line `get`
prints is still the tail of the `set` that would restore it, and no longer
carries a second reading in its word count.

The surface is named widely rather than deeply. **Inherit is named in 71 places
across eleven live files** — 30 in `commands/config.rs`, 11 in
`tests/configuring.rs`, 10 in `registry.rs`, 8 in the guide — and **Global as a
Scope in 51 across three**. `Overrides` the type takes `registry.rs:332-443`
whole, and is reached from `commands/config.rs`, `commands/group.rs` and
`import.rs`. That is a floor: it counts places that name the thing, not the
`InForce` display, the `everything`/`scope_lines` split, `overridden_at`,
`From<Settings> for Overrides` or the two validate paths that collapse into one.

**The registry's on-disk shape changes** — `groups` becomes a map of `Settings`,
`ungrouped` merges into the record that was `global`, and the key is renamed.
Nothing is migrated, per `CLAUDE.md`, including the author's own registry. The
carry-out **bumps `version`**: the forward-looking guard that refuses a registry
written by a newer Perch is only worth having if the number moves when the shape
does.

**Sequenced after #161**, which removes ADR 0046's three departing Settings and
is itself blocked on #169 removing the TUI. Converting six Settings' worth of
Override machinery that three of are about to be deleted is work done twice, and
`tui/model.rs` and `tests/browsing.rs` both reach it.

No exit code changes and no new one is added. The refusals that gain sentences —
`global`, a `set` with no Scope, and `interchangeable` asked of a Group — all
land on the register they already use.
