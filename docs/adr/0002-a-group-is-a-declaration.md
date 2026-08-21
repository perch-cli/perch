# A Group is a declaration

Groups were first justified by shared state — a company account and a personal
one wanting isolation, several identical subscriptions wanting everything
shared. Switching in place made that moot: there is one config directory, so
memory, settings, plugins, and project history are shared unconditionally and
isolation is not on offer.

Groups survive for a narrower reason. When quota runs out, cycling should move
you to another account you would actually accept — another work subscription,
never your personal one — and a group is the declaration of which accounts are
interchangeable. `perch switch` with no target chooses within the current
account's group, and never leaves it.

A group also carries configuration: whether the watcher (ADR 0013) may switch
accounts within it unattended, at what utilization threshold, and which strategy
it prefers. Unattended switching is off by default, so a group only ever changes
underneath you because you said it could.

## Considered Options

Groups could be inferred from the account's organization UUID. Rejected: three
subscriptions bought personally each carry their own organization UUID, so
inference would split them and fail precisely the case it exists to serve. Perch
may offer the organization as a default when a profile is created, but the user
confirms it.

Dropping groups entirely and using a per-account disable flag was seriously
considered and is close in value. Groups win only when there
are two or more groups with several accounts each, where cycling should follow
whichever account you are currently on without toggling flags. That, plus having
somewhere to hang per-group configuration, is what keeps them.

The term "flock" was considered and rejected. Perch has already spent its whimsy
budget on its own name, and `--group` reads better on a command line.

## Amended: Global carries the defaults and a Group Overrides them

> **Superseded in full by [ADR 0051](0051-a-setting-is-said-about-the-scope-it-governs-and-the-case-for-overrides-never-defended-the-fallback.md).**
> Every sentence below is about Global, Override, Inherit, the two layers or the
> word-count idiom, and none of the five exists any more: a Setting is said about
> the Scope it governs, each Scope holds its own full Settings, and the defaults
> are compiled-in constants.
>
> **The body above is untouched, and better than untouched.** "A group also
> carries configuration: whether the watcher may switch accounts within it
> unattended, at what utilization threshold, and which strategy it prefers" names
> precisely the three Settings a Group now holds outright — a sentence this
> amendment made approximate and 0051 restores word for word.

A Group still carries the rules governing Cycling within it. What changes is
that it no longer has to state all of them.

Every Setting exists at Global as well, where it is the value that applies until
something narrower is said. A Group holds an Override for a Setting it wants
different and Inherits the rest, so somebody running four Groups sets a
threshold once rather than four times, and changing their mind changes it once.
Exactly two layers: an Override beats Global, nothing beats an Override, and an
Account carries nothing — every Setting there is describes how Perch chooses
*between* Accounts, and a rule for choosing has nothing to say to a set of one.

Inherit is a state and not an absence. A Group that Inherits tracks Global as
Global changes; a Group holding an Override that happens to equal Global's does
not. The two look identical on screen unless the display says which, so it says
which, everywhere a value is shown.

Before this, Global held one Setting and Groups held six, disjoint, so nothing
could override anything and "which layer set this" had no answer to give. It is
worth being explicit that widening Global is not a step back towards the
organization-UUID inference rejected above: that would have decided *which
Accounts are interchangeable*, which is still and only the user's declaration.
This decides what happens to a Group nobody has configured, which previously was
a constant compiled into Perch.

`perch config`'s existing shape already expresses it and needs no new syntax:
three words name a Group and set an Override, two words set Global's default.
The vocabulary that reads it back is the same one, so the layer a value came
from is the number of words that would set it again.

