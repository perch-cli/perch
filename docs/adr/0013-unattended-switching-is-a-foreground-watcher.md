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
