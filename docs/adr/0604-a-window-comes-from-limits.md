# A window comes from limits

The usage endpoint describes the same quota twice. A map at the top level, keyed
by window name, and a `limits` array beside it. Perch reads `limits`.

The map's per-model keys have stopped saying anything. They arrive as
`nimbus_quill`, `iguana_necktie`, `juniper_tide`, names that give neither the
model nor the period, and an Account holding one has a row under `Utilization`
that nobody on the machine can identify. The same window in `limits` is
`{"kind": "weekly_scoped", "group": "weekly", "percent": 0, "scope": {"model":
{"display_name": "Fable"}}}`. Only one of the two halves can be shown to a
person.

None of this is a published contract, so it is held the way every other
assumption about somebody else's shape is held (ADR an-assumption-is-probed):
one module, and a reply Perch cannot make sense of reported rather than guessed
at.

## What makes an entry a window

The rule is keyed on `group`, the period an entry meters over. `kind` is the
narrower field and grows with every scope Anthropic adds, so a rule keyed on it
learns a new name each time and refuses what it has not learned yet. `group` is
what "names a period" was reaching for.

- An entry in a `group` Perch knows, carrying a numeric `percent`, is a window.
- An entry in a `group` Perch knows, carrying no numeric `percent`, is drift, and
  the reading is refused (ADR headroom-is-the-worst-window).
- An entry in a `group` Perch does not know is a limit beside the windows, and is
  passed over.

Every Account has a `session` entry and a `weekly_all` entry, and a reply missing
either is refused. That is the same loss the map's version of this rule catches,
arriving by omission rather than by drift: a window nobody can read and a window
nobody sent are equally absent from a ranking.

A `weekly_scoped` entry that is not there is an Account that has no such window,
which is most models for most Accounts, and meters nothing.

## What a window is called

`5-hour` and `7-day` are Perch's names for the two every Account has, and do not
move.

A scoped window is named from what its entry says rather than from what Perch
guesses: `group` gives the period, `scope.model.display_name` gives the rest,
downcased. The Fable weekly window is `7-day-fable`, which is how `7-day-opus`
already reads.

Naming it `Fable` on its own is refused. A Quota Window is a period an Account's
usage is metered over, and a row under `Utilization` that does not say which
period is not one. The display name alone also reads as a model rather than as a
window, in a column where every other row is a span of time.

`scope.model.id` is null, so the display name is the only handle Anthropic offers
and it is prose. **So `window` in `--json` is a label rather than a key.** Each
window carries its `group` beside it, which is the stable thing a script matches
on, and a model renamed upstream renames a row here and nothing else.

## The map is a fallback, for one release

Read when `limits` is missing or is not an array. Not when it is empty: an empty
array is an answer, and a reply naming no window is already refused.

On that path a key carrying a number is a window whatever it is called, so
`nimbus_quill` shows as `nimbus-quill` and nobody can say what it is. That is the
honest reading rather than a defect. Perch does not know, and a fallback that
dropped the row would be claiming an Account has room it may not have.

One release, because two readers of one endpoint are two shapes to keep in step.
`limits` has been seen on one Account on one plan, and the fallback covers
Accounts where it turns out not to be sent. After a release there either are such
Accounts or there are not, and the answer is worth more than the second reader
costs.

## Utilization is never read through a client

`perch run <target> -- "/usage"` reads this endpoint. It is not a second source:
the drift that breaks one breaks the other, with a panel drawn for a person in
the way.

Nothing is saved by going that way and four things are spent. The hourly
allowance is the same allowance. A Run writes a Marker and makes the Profile Live
(ADR a-run-is-one-shot), and a Renewal is refused on a Live Profile
(ADR a-profile-is-live-by-evidence), so a read needing a fresh access token
cannot get one, and a client that renews for itself Rotates the refresh token
Perch holds. Reading becomes writing. The Watcher reads twenty-four times an hour
per Account (ADR a-watcher-knob-is-arithmetic), which is twenty-four client
launches an hour with nobody watching. And `perch status` is built to cost
nothing in a shell prompt (ADR a-figure-carries-its-age).

## Consequences

**`is_active` and `severity` are read from nothing.** Anthropic now says which
limit binds and how severe it is, and Perch works the first out for itself by
taking the worst window. Two predicates that have to stay in agreement are a
tally that stops adding up, and `severity` is a display band with no vocabulary
Perch can hold Anthropic to. The live alternative is to read `is_active` as a
drift signal, remarking when the limit Anthropic calls binding is not the one
Perch ranked. That is a second opinion nobody asked for on a command people run
several times a minute, so it stays an alternative.

**A partly readable reply is not stored.** Where one window drifts, the Account
keeps the figures it had, shown with their age, and the attempt says what broke.
Storing what did read would replace a complete older reading with a fresh one
nothing can rank on, so the day a field moves, every Account has figures and no
Scope has an answer. Keeping the old reading degrades instead: Perch goes on
routing on figures whose age is printed beside them, which is what a cached
figure is for.

**No version moves.** A window's name is a value in a `String` a Refresh replaces
whole, so nothing about the registry's shape changes and the codenames a cache
holds today die at the next read (ADR the-holdings-outlive-a-perch). `--json`
gains `group` on each window, which is additive.
