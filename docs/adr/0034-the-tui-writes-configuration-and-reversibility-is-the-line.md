# The TUI writes configuration, and reversibility is the line

> **Superseded by ADR 0042.** The Config tab is removed — and `perch tui` with
> it, since ADR 0049 took the whole view before the tab had to come out on its
> own. The observation below is not what was found wrong: a Setting really is
> not irreversible. Reversibility was simply the wrong axis. What the panel
> costs is not the recoverability of what it writes
> but the machinery that lets it write at all — the lock per edit, the refusal
> and rollback, the debounce and its `Pending`, the one text mode — and every
> link in that chain starts at *the panel writes*. One thing here is not
> repealed: `perch config unset` stays, on its own merits rather than the
> panel's, because a two-layer configuration needs a way back to Inherit
> whoever is doing the clearing.

`perch tui` acted on exactly two things, a Switch and a Run, and said why in its
own words: `add`, `remove`, `purge` and `config` stayed out because a keystroke
away from an irreversible act is the wrong ergonomics for the one surface being
navigated by arrow key.

The reasoning was right and the list was wrong. A Setting is not irreversible.
Nothing one can be set to destroys anything — the previous value is a keystroke
away, no Credential moves, and the worst a mistake costs is that a Cycle prefers
the wrong Account until somebody notices. `config` was grouped with the other
three because all four were things another command did, which is a different
criterion from the one the sentence claimed.

So the line moves to where that reasoning always pointed: **the TUI may write
what it can unwrite.** Config is in, and with it the reversible facts about an
Account — its Alias, and whether Cycling may choose it. Out stay `add`,
`remove`, `relogin`, `purge`, `export` and `import`, and so does deleting a
Group: the Accounts survive it and become Ungrouped, but that Group's Overrides
do not, and a value nobody can get back is exactly the loss this rule refuses.
`perch group` still does it.

ADR 0011 is untouched, because the TUI adds no capability. `perch config` sets
every Setting the panel sets, and gains an `unset` so the panel cannot reach a
state a script cannot — clearing an Override back to Inherit is a state, and one
the panel offers on every row.

## Two things the panel has to be honest about

**A write per edit, not per session.** There is no save button: a change is
written when it is made, which means taking the exclusive lock. The TUI takes it
per edit and gives it straight back, rather than holding it for as long as the
screen is up. Holding it would make an open TUI a denial of service against
`perch watch`, which takes the same lock every round, and against every other
`perch` on the machine. The cost of the other choice is that an edit can be
refused — so a refusal is shown where a failed Refresh is shown, and the row
goes back to what it was rather than showing a value that was never written.

Stepped values are debounced: `←`/`→` moves a percentage in steps of five and
the write follows the last keystroke rather than each one, because holding an
arrow from 0 to 80 is otherwise eighty writes and eighty lock acquisitions, some
of which will lose the race and leave a half-set value. This is a deferred write
and worth naming as one. It differs from a save button in the way that matters:
nothing has to be remembered, and walking away loses nothing.

**Text entry exists exactly once.** Naming a Group, renaming one, and setting an
Alias are the only places the panel accepts typed input, because a name is the
only value with no natural step — a bool has two states, a Strategy has two, and
a percentage has arrows. Admitting a text mode for numbers would buy a commit
key and a modal state, which is the save button under another name. The
exception is recorded here so it is not argued back and forth: it is about names
having no other input shape, not about text entry being fine in general.

## Consequences

The TUI is now the second thing that writes the registry, so it is the second
thing that can lose a lock mid-act. `registry::save` already refuses to write
against a lost hold, which is the behaviour the panel wants — the refusal is
surfaced rather than swallowed.

`perch config` grows `unset`, in both the two-word and three-word forms, so
Global's defaults and a Group's Overrides are each reachable the way they are
set.
