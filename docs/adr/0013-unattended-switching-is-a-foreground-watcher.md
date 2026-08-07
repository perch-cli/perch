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

**It polls every two and a half minutes.** Twenty-four reads an hour, which
sits inside the ~28-30 the endpoint allows with room left for the `perch status
--refresh` somebody types while the watcher is running — a loop that spent the
whole allowance would answer the user's own question with a throttle. It is a
constant rather than a setting: it is derived from Anthropic's allowance rather
than from anyone's preference, and a group configured to poll every ten seconds
would be a group configured to be refused.

**It never acts on a figure it did not just refresh.** A failed refresh holds
the decision, backs off and retries. Acting on a cached figure would be a
switch made on evidence the user already had; a held decision costs nothing,
and a wrong switch costs a capture, a credential write, and possibly an account
more exhausted than the one it left.

This is the one place the watcher diverges from every other surface. ADR 0018
has a refresh degrade the display rather than fail the command, and ADR 0015
has everything served from cache, because those surfaces show a person a number
they will judge for themselves. The watcher shows nobody anything; it acts. A
reply that arrived but carries no quota window perch can read is a failed
refresh too, and not a reading of zero — an account with nothing used is the
one reading that can never be over any threshold.

**Back-off doubles from the interval and stops at twenty minutes.** It starts
at the ordinary two and a half rather than under it, so no retry ever asks
faster than a working loop does and the arithmetic above covers a failing
endpoint unchanged. At twenty minutes a persistent failure asks three times an
hour instead of twenty-four. It is bounded there because the endpoint coming
back does not announce itself and the only way the watcher finds out is by
asking — a loop that had doubled its way to an hour would come back long after
the crossing it was left running for. Twenty minutes is the order of thing a
five-hour window forgives, which is the same reasoning the cooldown's fifteen
was set on. The first refresh that works drops the whole of it rather than
winding it down a step, because pacing the loop on a failure that has stopped
happening is pacing it on nothing. It lives in the running loop beside the
cooldown and is nobody's to configure — a group that could set it could set it
to nothing, and nothing is what the endpoint is already refusing.

Every held round says which failure held it and when the watcher will ask
again. A hold whose line said neither reads as a watcher that has given up.

**Cooldown 15 minutes, margin 10 points, no return for one cooldown.** A
five-hour window moves slowly enough that fifteen minutes never misses a real
crossing. The margin is what kills the ping-pong: at an 80% threshold nothing
is switched to unless it is at 70% or better. All three are per-group
configuration beside `watcher-threshold-percent`, not constants —
`watcher-cooldown-minutes`, `watcher-margin-percent` and `watcher-no-return`.

The margin sets candidates aside before the strategy ranks them rather than
vetoing the winner afterwards. A `soonest-reset` group would otherwise be told
there is nowhere to go while a perfectly empty account sat behind the fullest
one, and the strategy is entitled to prefer among whatever clears the ceiling.
An account perch has never read a figure for is set aside too. Ranking puts an
unknown above a window that is certainly full, which is right for a switch
somebody asked for; unasked it is a move onto an account the watcher knows
nothing about.

The cooldown lives in the running loop and nowhere else. A cooldown is about
the loop somebody is running, not about the machine — recording it in the
registry would have one person's watcher pacing another's decisions, and would
give `perch watch` a file to write when the whole of the record above is that
it leaves nothing behind. Stopping the loop and starting it again is a person
saying "go on then".

The cooldown and no-return hold the same window by construction: nothing is
switched during the cooldown, so nothing can be switched back either. The
cooldown always gets there first, and `watcher-no-return` changes no trace the
watcher can be shown today — including at `watcher-cooldown-minutes 0`, where a
no-return of no minutes bars nothing. This is stated plainly rather than dressed
up, and `perch config` says it back when either is set.

It is still written as two rules, because they answer different questions —
*whether* to move and *where* not to — and the second is what a later change to
the first would otherwise silently repeal. A per-account cooldown, or one that
paces switches rather than gating them, would leave no-return doing the work
alone. Unit tests pin it directly; the loop cannot, because the loop never gets
far enough to ask.

A margin at or above the threshold is not out of range. It is a group that will
only move onto an account with nothing used at all, which is a coherent thing to
ask for; refusing it would make the order two `perch config set`s are typed in
matter.

**It never acts on an ungrouped account.** `cycle-ungrouped` (ADR 0017) lets a
bare `perch switch` cycle among accounts in no group; it grants the watcher
nothing. Permission to switch when asked and permission to switch while nobody
is looking are different grants, and the second has no owner when there is no
group to carry it. `perch watch` started on an ungrouped account says so and
exits rather than idling forever having decided nothing.

**A Switch that changed nothing is a decision; one that changed something and
then failed stops the loop.** A Capture refused because a client is running
against the outgoing Profile (ADR 0027) leaves the machine exactly as it was
and clears itself when that client exits, so it is printed and the loop goes on
watching. A Switch that made the incoming Credential live and then failed has
left the machine part way through, and a watcher that carried on polling would
be deciding what to do next about a machine nobody has looked at.

**Its decision log goes to standard output.** This is a loop in a terminal the
user can see; a rotated logfile is what a daemon needs because nobody is
watching, and the whole of this record is that Perch is not one. Redirection is
the user's call, and `--once` under cron has its output captured already.

`--once` reports in its exit code, reusing the table rather than growing it:
`0` switched, `15` below threshold so nothing to do, `17` a switch was wanted
and every candidate was exhausted, `18` ungrouped. One code is new — `20`, held
because the figures were stale — because a scheduler retrying shortly needs to
tell that apart from `17`, and only one of the two resolves itself.
