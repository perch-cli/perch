# The Holdings outlive a Perch

Perch is at v0.2.0 with three published releases, a Homebrew tap and an
installer pasted from a URL with no version in it. There are people on the other
end of it, and the premise the tree was written on is that there are none:
breaking changes free and preferred, no migration code, no format upgraders, no
fallback for a registry an older Perch wrote.

What that premise costs is already on disk rather than hypothetical.
`registry.rs` bumped `CURRENT_VERSION` from 1 to 2 when the Settings moved onto
Scopes, and that bump is unreleased. Every registry any published Perch has
written claims version 1 and carries the shape that went with it. The version
guard in `load` fires only above `CURRENT_VERSION`, so a version 1 registry is
not diagnosed — it falls through to `deny_unknown_fields` and comes back as
serde's words about an unknown field, on a file Perch itself wrote three releases
ago. It would ship in the release that announces Perch.

## The line is whether being told is a remedy

The tempting line is "on disk", and it is the wrong one. What separates the two
sides is whether a changelog entry hands somebody something they can act on.

A renamed command is loud, and the entry says what to type instead. The cost is
seconds, and the person is whole. So the CLI surface moves freely: commands,
flags, output prose, exit codes and the shape of `--json`, each marked
`[**breaking**]` in `CHANGELOG.md`, which is what `feat!:` already produces.

A registry whose shape moved is a file that person cannot convert. The entry
tells them what happened and leaves them with it. So the Holdings — a Profile, a
Credential, the registry naming them, an Export carrying all three off the
machine — change shape only behind a migration or a refusal-with-instructions.

Two of the free ones are named in the rule rather than left to the list, because
they are the ones read by a script rather than by a person: exit codes and
`--json`. Prose warns the person reading it. Nothing warns a script, so the
changelog entry is all there is, and it has to be there.

One boundary inside the Holdings is worth stating, because it does not look like
a format change. A Credential Store is derived from its Profile's path, so moving
`profiles/` orphans the Credentials keyed to the old one, silently. A Profile's
*contents* are Claude Code's shape rather than Perch's, and are not Perch's to
break or to keep.

## An Export refuses and a registry migrates

Which of the two answers applies is not a preference. It follows from what the
refusal costs the person it lands on.

An Export is a backup, and the Perch that wrote it can still open it. Refusing
one takes nothing away that is not still there, and the refusal can say which
version wrote the file, which is a better answer than a partial read. This is
already the shape `export.rs` argues for its own two version fields, and the
reason an Export is meant to outlive the machine it was written on
(ADR export-is-encrypted-with-a-required-passphrase).

A registry holds Groups, Aliases and Settings that were typed by hand and exist
nowhere else. Refusing one leaves starting over as the only route, and starting
over means logging in to every Account again. Nothing reconstructs it. So the
registry migrates forward from every version Perch has written.

That generalizes rather than enumerating: refusal is the default, and a migration
is the exception a single artifact earns by being irreplaceable. Today exactly
one artifact earns it.

## The version moves when the shape moves

Both answers read the same field, and neither can do anything without it. A
refusal that cannot tell an unreadable file from a newer one says the wrong thing
about it; a migration that cannot tell which shape it is holding cannot pick a
step.

So a change to the shape of the registry or of an Export moves its `version`. A
shape that changes under a version that does not is the one failure neither
mechanism catches: the file parses, or half-parses, and what comes back is wrong
rather than refused. That is the corruption both of these exist to prevent, and
the only way to reach it is to forget the number.

## What 1.0 leaves open

This says nothing about 1.0 and neither does `CLAUDE.md`. The Holdings rule is
already what it would be at any version — irreplaceable is not a property that
changes on a release — so what a 1.0 would have to decide is the other half: how
freely the CLI surface may still move once a version number promises it will not.
That is a decision for the release that claims it.

## What was not chosen

**Freezing the formats until 1.0.** It buys the same safety by forbidding the
change instead of handling it, and the price is paid by the design rather than by
the release: a registry that cannot change shape is a registry that accumulates
fields it does not want. The bump from 1 to 2 was right; only its landing was
wrong.

**Migrating the Export too.** Symmetry, and it costs a migration path for a file
written perhaps twice a year, kept forever, tested against shapes nobody holds.
The Perch that wrote the file still opens it, so the symmetry buys nothing the
person did not already have.

**Refusing the registry too.** Cheapest to write, and symmetric with the Export.
It fails on the one thing the symmetry hides: a refused registry is not
survivable. There is no earlier Perch still installed to fall back to, and what
is being refused is the only copy of what a person told Perch about their own
Accounts.

**Leaving the whole stance in this document, with nothing in `CLAUDE.md`.** An
agent about to change the registry's shape reads `CLAUDE.md` and may not glob
`docs/adr/`. The rule goes where it is always read; the case goes where a case
belongs.
