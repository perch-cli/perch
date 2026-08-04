# Groups scope cycling and carry per-group configuration

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
