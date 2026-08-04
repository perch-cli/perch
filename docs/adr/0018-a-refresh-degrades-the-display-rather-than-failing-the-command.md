# A refresh degrades the display rather than failing the command

`perch status --refresh` answers the same question `perch status` answers —
where does my quota stand — and asks Anthropic first. Everything about that
first step can fail on its own: the hourly allowance is spent and does not
refill early (ADR 0015), a client is running against the Profile so its
Credential is not Perch's to renew (ADR 0005), Anthropic will not accept a
Credential at all, the network is not there.

None of those turn the command into a failure. Each is reported by name — which
Account, and why — the Account keeps whatever figure it last had, and the
command renders and exits zero. `--json` carries the same under `refresh`, which
is `null` when no refresh was asked for, so a script can tell "nobody asked" from
"asked, and it went fine".

The reasoning is ADR 0015's, followed through. Utilization is already an
observation with an age rather than a live reading: every surface has room to
show a figure that is minutes old, and does. A refresh that failed the command
would throw away a perfectly good cached answer over the freshness of it, and
would do so at exactly the moment a person is trying to decide where to switch.
The figure's age already says how much to trust it.

The same rule makes each Account independent. One Account that cannot be read
leaves the others read and displayed, because a listing that lost every figure
to one broken Account answers worse than a listing with one gap in it — and the
gap is visible, since a figure nobody could read still says when it was last
observed.

## Consequences

`--refresh` has no failure exit code of its own. A script that needs to know
whether the figures it is looking at are current reads `refresh` from `--json`
rather than branching on the exit status; a person reads the age beside each
figure, which is the same information said the way people read it.

Errors that stop the command before any figure could be shown — no active
Account, a registry Perch cannot read — are unaffected, and still fail with the
codes they already had. The rule is about the fetch, not about the command.

The watcher (ADR 0013) inherits this. It will be reading Utilization on a loop,
where a single unreadable Account must not stop it looking at the rest, and
where a throttle is an ordinary event rather than an incident.
