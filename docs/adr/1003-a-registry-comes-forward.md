# A Registry comes forward

**A Registry claiming a version below this build's is brought into the shape it
reads — in memory on every read, and written back once by the run that finds it.
The steps chain from whichever version a document claims, and each is arithmetic
over the document rather than a conversation with the machine.**

ADR the-holdings-outlive-a-perch decided which answer the Registry gets: it
migrates, because a refused Registry is not survivable. This is that step, and
the four things carrying it out had to settle.

## What version 1 actually is

`CURRENT_VERSION` went 1 to 2 when the Settings moved onto Scopes, and the bump
never shipped. Every Registry v0.1.0, v0.1.1 and v0.2.0 wrote claims version 1,
and the guard in `load` fires only *above* the current version — so those files
fell through to `deny_unknown_fields` and came back as serde's words about an
unknown field, on a file Perch itself wrote three releases ago.

Seven things moved, and only the first is what the refusal named:

| version 1 | version 2 |
| --- | --- |
| `active`, an address | `Active`, which also holds a Landing |
| `enabled`, defaulting to true | `disabled`, defaulting to false |
| `global`, holding the fallback Settings | gone, merged into `ungrouped` |
| `ungrouped`, a partial `Overrides` | `UngroupedConfig`, a Scope's own record |
| `groups`, partial `Overrides` | `groups`, whole `Settings` |
| three watcher knobs among the Settings | gone, being arithmetic |
| `switched_off` on a scheduled Check | gone with the no-return that read it |

Serde stops at the first mismatch, so fixing what the message names leaves six
behind it. Two of the seven matter more than being on the list.

**`enabled` inverted.** An Account somebody took out of Cycling said
`"enabled": false`, and dropping the key rather than translating it puts that
Account back in — a Switch onto an Account its owner had deliberately parked.

**`groups` does not refuse.** A version 1 Group that named two of its six
Settings Inherited the other four from Global, and a partial `Overrides` object
deserializes cleanly into `Settings`, whose absent fields take the compiled-in
defaults. Nothing is refused and what comes back is wrong, which is the one
failure ADR the-holdings-outlive-a-perch names as beyond both mechanisms. So the
step reads what a Scope Inherited out of that document's own `global.settings`
rather than out of this build's defaults: the value somebody set is in the file,
and taking the default instead reverts it silently.

## Version 3 and version 4, and why a name rule moves the number

Neither changed a shape. Both changed which names `validate_name` accepts.
Version 3 took Unicode's whole `Default_Ignorable_Code_Point` set for
`crate::host::is_unshowable`, the hand-picked one having grown holes
(ADR nothing-drawn-is-obeyed). Version 4 replaced the deny-list around it with an
allow-list of identifier characters (ADR a-target-has-to-be-typeable).

That is still a version. The number tracks what this build can *read*, and a
Registry holding a name the rules now refuse is one `load` turns down — which is
every command, `perch group rename` among them. A rule with nothing to carry the
names already written down is the failure this document exists to prevent,
reached from the other direction.

So the rename pass is its own step rather than a line inside the version 1 one,
and `forward` runs it after whichever step brought the document to this shape. A
version 1 document meets it having already been renamed, and it does nothing.

What the pass carries is bounded by history: the rules the version on the
document actually shipped, one predicate per version. Each writes out the clauses
that have moved since and asks this build for the clauses that have not, a rule
moving one of the latter being a rule that owes a version of its own. A name no
build of that version ever wrote is a hand edit, named at `load` and left. The
bound is owed for version 1 as much as for the two after it: almost nothing was
refused, and *almost* is where a hand-edited `a@b` sits.

Both ways a Registry arrives say what was renamed. `bring_forward` says it about
this machine's own file, after the write; an Import says it about the Export's,
before one. `unseal` hands the renames back beside the document rather than
carrying them on it, `coming_forward` being a `Deserialize` with nowhere to put
them — and what a step renamed is a fact about reading that Export on this build
rather than a field of it.

## One version, two shapes

v0.1.0 and v0.1.1 wrote `groups` as a map of whole `GroupConfig` values, a
`global` holding one flag and no `ungrouped` key at all. v0.2.0 wrote `groups` as
a map of `Overrides`, added `ungrouped`, and nested a `Settings` inside `global`.
Both stamp `"version": 1`.

That is the mistake this document is about, already shipped: the shape moved and
the number did not. It is why the step reads the union of the two rather than one
of them, and why the suite holds a Registry per released version rather than a
test per field that moved — a per-field test over either document passes while
the other stays unreadable.

## The step is on the document

`Overrides`, `GlobalConfig` and `GroupConfig` are gone from the tree. Bringing
them back as deserialize-only mirror types would put a second definition of a
shape nothing writes any more into `registry.rs`, to be kept in step with a
history rather than with the code. What arrives is JSON and what leaves is JSON,
so the step is testable without a machine and the whole of it fits in one module
that reaches nothing.

Every Setting the build has is asked of the narrower Scope first and the wider
second, out of `Settings::default()` rather than out of a list — so a Setting
added later needs nothing here, and a Setting retired is dropped by not being
asked for.

## Where it runs, and why not in one place

`load` brings the document forward in memory. It cannot write it back: the
Registry lock is taken *before* the read on every path that writes
(ADR one-door-to-the-registry), so a `load` that took the lock itself would wait
on the command that called it and give up after five seconds.

So `main` brings it forward ahead of the command — take the lock, load, save,
release, which is shape 1's sequence without shape 1's door, because the door
adopts an existing login where there is no Registry and a migration has nothing
to adopt. Its outcome is deliberately not the command's: an older Registry is
read correctly either way, so a lock somebody else is holding costs the
write-back and nothing else, and the next run takes it.

Twice, then — once in memory and once on the way to disk — which is why the step
is idempotent and asserted to be.

## It says so, once

The migration prints one line, on the run that writes: which version was on disk
and which is there now. The stance in `AGENTS.md` is that a Holding never lands
as a file this Perch reads wrong; it does not say quiet. A file somebody did not
ask to have rewritten was rewritten, and that is worth a sentence.

Through `host.note`, so it goes to stderr and is said once however many times it
is provoked. A `--json` document on stdout is what a script is parsing, and a
sentence in the middle of one is a worse failure than the one this fixes.

## A version below the earliest is refused

Version 1 is the oldest any Perch stamped. A file claiming `0`, or claiming
nothing, was not written by a Perch — and today a `0` is read as though it were
the current shape and silently restamped, which after this step lands would mean
skipping it.

So both are refused, naming the versions this build reads. The refusal is
`Malformed` and carries the sentence that says where the file is, because a
version no Perch wrote is a thing only a hand edit could have done. What it does
not do is guess: the version says which shape the rest of the file is in, and
reading a document whose shape is unstated is the half-parse this whole document
exists to prevent.

The floor is held by both readers of a Registry, not only by the file on disk. An
Import writes what it read back out under the current version, so a Registry
inside an Export claiming a version no Perch stamped would be relabeled rather
than refused — the same corruption by a longer route.

A `version` that is not a number at all is left to serde, which names the value.
Only saying nothing — no `version`, or a null one — is Perch's own sentence to
write, because only then is there nothing to describe.

## A number that belies the shape is refused, not carried

The shape has already moved once under an unchanged number, and this diff had to
re-stamp a fixture in the suite for it. So the step refuses a document whose
`active` is version 2's `Active` under a version that says 1, rather than reading
past it: dropping the field would lose which Account the machine is on and say
nothing, which is the failure mode the version exists to prevent, reached from
inside the thing that exists to prevent it.

## The Export's Registry comes forward; the Export does not

An Export written by any published Perch carries envelope version 1 — still the
current one — around a Registry claiming 1. Both guards fire only above the
current version, so neither trips, and the envelope's derive walks into the same
seven mismatches. The refusal ADR the-holdings-outlive-a-perch gives an Export
has no number to name here: nothing is ahead of anything.

The Registry inside is the same Registry, so it takes the same step, on the same
grounds — it holds Groups, Aliases and Settings that exist nowhere else, and the
Export it arrived in is how somebody moves a machine. The *envelope* keeps its
refusal for a version above this build's, unchanged.

It runs on the `registry` field alone, as that field's own `deserialize_with`,
rather than over the unsealed document. Reading the whole document as a `Value`
first would put every Credential in the Export into a `String` that nothing
wipes; the Registry half holds no secret.

## What was not chosen

**A migration chain, or a framework for one.** There is one step, from one
version. A chain earns its shape from the second step, which will know things
this one cannot guess — whether the steps compose in memory or through the disk,
and whether a version can be skipped. The number the step lands on is its own
constant rather than `CURRENT_VERSION` for that reason: read from the current
version, a two-step chain is wrong the day it exists.

What the second step must not inherit is a range. Whether a file is rewritten is
asked of the step itself — has it a move for this document — rather than of the
versions between the earliest and the current, because `save` stamps the current
version on whatever it is handed. A version bumped without a step for the one
before it would otherwise relabel a shape that never changed, which is this
document's own subject arriving through the mechanism meant to end it.

**Writing what the step produced, rather than saving what it loaded.** It would
skip a serialize. It would also skip `validate`, the version stamp and the
one-step replacement, which are the three things making a half-finished
migration impossible.

**Refusing a version 1 Registry with instructions to install v0.2.0 and export.**
It is survivable, unlike a bare refusal, and it asks somebody to find a release
by hand to keep the Groups they typed. The refusal costs more than the step does.

**Taking the compiled-in defaults for what a Scope Inherited.** Two lines
shorter, and it quietly changes a threshold somebody set. The values are in the
document; not reading them is the corruption, not a simplification.

**Making the write-back the command's business, so a failure to persist is
loud.** Every command would have to report a failure about a file it read
correctly, and the loudest case — another Perch holding the lock — is the one
where nothing is wrong at all.

## The glossary

No new term. **Holdings**, **Registry** and **Settings** already say all of this,
and "the Registry comes forward" is a sentence about them rather than a coinage.
The word `AGENTS.md` uses for the mechanism is *migration*, and this document is
the one it points at.
