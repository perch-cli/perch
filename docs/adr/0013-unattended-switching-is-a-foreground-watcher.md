# Unattended switching is a foreground watcher, not a daemon

Someone rotating between subscriptions wants to stop noticing that they ran out.
That needs something watching utilization and switching when a threshold is
crossed, without being asked.

`perch watch` is that, as a loop in a terminal the user can see and kill.
`perch watch --once` performs a single check-and-switch and reports the outcome
in its exit code, so anyone wanting it truly unattended can schedule it with
cron or a systemd timer. A group's configuration says whether the watcher may
act on it, and that is off by default.

A managed background daemon was rejected. It would mutate credentials while
nobody is watching — every hazard in ADR 0006 firing unattended — and would need
service lifecycle management on three platforms, in exchange for convenience the
user's own scheduler already provides. Scheduling is the operating system's job.

## Consequences

The watcher polls `/api/oauth/usage`, which allows roughly 28-30 requests per
hour per account. Polling must be adaptive — watching busy accounts more closely
than exhausted ones — or a handful of accounts will saturate the budget and the
numbers it decides on will be stale exactly when they matter.

It must also not flip-flop. A cooldown between switches and a margin around the
threshold are required, so two accounts hovering near the line do not ping-pong.

Everything the watcher does is a Switch, so it captures the outgoing credential
first (ADR 0006), takes Claude Code's locks, and never refreshes a live
profile's token (ADR 0005). Running while Claude Code is working is the normal
case, not the exception.

## Amended: the numbers this asked for

This record required adaptive polling, a cooldown and a margin, and named none
of them. They are named here so the watcher is a policy that can be argued
with rather than a set of constants discovered in the source.

**It polls only the active account.** One account watched continuously fits
inside the ~28-30 requests per hour comfortably. Candidates are ranked at the
moment a decision is taken, not kept warm — they are idle by definition then,
so ADR 0005 permits renewing them. Polling every account in the group instead
would spend every account's budget to keep figures fresh that are read only at
a threshold crossing, and would make the size of a group a scaling limit.

**It never acts on a figure it did not just refresh.** A failed refresh holds
the decision, backs off and retries. Acting on a cached figure would be a
switch made on evidence the user already had; a held decision costs nothing,
and a wrong switch costs a capture, a credential write, and possibly an account
more exhausted than the one it left.

**Cooldown 15 minutes, margin 10 points, no return for one cooldown.** A
five-hour window moves slowly enough that fifteen minutes never misses a real
crossing. The margin is what kills the ping-pong: at an 80% threshold nothing
is switched to unless it is at 70% or better. All three are per-group
configuration beside `watcher-threshold-percent`, not constants.

**It never acts on an ungrouped account.** `cycle-ungrouped` (ADR 0017) lets a
bare `perch switch` cycle among accounts in no group; it grants the watcher
nothing. Permission to switch when asked and permission to switch while nobody
is looking are different grants, and the second has no owner when there is no
group to carry it. `perch watch` started on an ungrouped account says so and
exits rather than idling forever having decided nothing.

**Its decision log goes to standard output.** This is a loop in a terminal the
user can see; a rotated logfile is what a daemon needs because nobody is
watching, and the whole of this record is that Perch is not one. Redirection is
the user's call, and `--once` under cron has its output captured already.

`--once` reports in its exit code, reusing the table rather than growing it:
`0` switched, `15` below threshold so nothing to do, `17` a switch was wanted
and every candidate was exhausted, `18` ungrouped. One code is new — `20`, held
because the figures were stale — because a scheduler retrying shortly needs to
tell that apart from `17`, and only one of the two resolves itself.
