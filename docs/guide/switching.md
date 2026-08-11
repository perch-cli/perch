# Switching, Cycling and Groups

`perch switch` with a target moves the whole machine onto an Account you name.
With no target it picks for you, within the Group the Account you are on belongs
to. A Group is how you declare which Accounts are interchangeable.

- [Switching](#switching)
- [Cycling](#cycling)
- [Managing Groups](#managing-groups)

## Switching

`perch switch <target>` makes an Account active everywhere — every terminal, the
editor extension, the desktop app — with no login flow.

```
$ perch switch overflow
`overflow` is an Alias for overflow@example.com.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour    12%  (as of 4m ago)
```

It is three steps in one order and never another (ADR 0006). The Credential you
are leaving is **Captured** back into its own Profile first, because Anthropic
retires a refresh token whenever it issues a new one — so the copy in an
Account's Profile is several Rotations behind by the time you switch away, and
skipping the Capture would quietly poison the Account you are leaving. Then the
incoming Credential is written to the Default Profile. Then the `oauthAccount`
block of `.claude.json` is patched to match, and only that block: your project
history, MCP configuration and settings live in the same file and belong to you
rather than to the Account (ADR 0001).

All three run inside Claude Code's own OAuth refresh locks, taken in Claude
Code's order — the refresh lock, the legacy config-home lock, then the config
file lock — so a refresh cannot land between the Capture and the write. A lock
somebody is holding is waited on and then given up on; one whose holder died is
taken over.

Nothing else moves. Your memory, settings, plugins and project history are
Shared State: a Switch leaves them untouched, which is what makes them follow
you across Accounts for free.

A Switch onto a Profile a client is already running against is refused with exit
code 16 rather than writing a Credential something else is holding, and
switching to the Account that is already active does nothing and exits 15. If a
Switch fails part way, it says which Account is active now and what is where —
running it again finishes the job.

## Cycling

`perch switch` with no target picks the Account for you. It is the command you
type mid-task when quota just ran out, so it asks nothing, under any
circumstances (ADR 0011).

```
$ perch switch
Cycling within Group `work`.
overflow@example.com has the most room: 60% headroom, which is true of every one of its Quota Windows — 7-day is its fullest, as of 4m ago.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
That figure is what Perch last observed rather than what Anthropic says now. If overflow@example.com turns out fuller than it implied, the figure was stale — `perch status --refresh` reads a current one.
```

It Cycles **within the current Account's Group** and never leaves it, so a work
subscription running dry does not land you on your personal Account. `perch
switch <group>` Cycles within a Group you name instead.

Each Account is ranked by its **worst** Quota Window, and the Account whose
worst is best wins (ADR 0012). Being blocked by any window blocks you
completely, so that is the only ranking that measures what actually stops you
working: the headroom Perch reports is true of every one of that Account's
windows. Exhausted, disabled and Quarantined Accounts are never chosen, and an
Account nobody has ever read a figure for is ranked below every Account that
has one — no figure and plenty of room are opposite pieces of advice.

How headroom is measured is fixed; which Account to prefer is the Group's to
say. `perch config set <group> strategy soonest-reset` makes a Cycle take the
Account whose fullest window comes back soonest instead of the one with the
most room, so perishable quota is spent rather than wasted — see
[Configuration](configuration.md). It reorders the Accounts that have room and
nothing else: an exhausted Account is never chosen however soon it resets.

Ranking reads the cache and never the network, so the figures can be minutes
old. Landing on an Account that turns out fuller than they implied is the cache
being stale rather than the Switch going wrong, which is why every Cycle says so
before you find out.

Three outcomes are honest non-outcomes. They perform no Switch, explain
themselves, and exit with a code of their own rather than pretending to have
worked.

```
$ perch switch
Cycling within Group `work`.
Every Account in Group `work` is exhausted, so there is nowhere useful to Switch. Nothing was changed.
you@example.com frees up soonest, at 2026-08-04 15:00 UTC (in 3h).   # exit 17

$ perch switch
Cycling within Group `work`.
you@example.com is already the best Account in Group `work`, with 90% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of 4m ago. Nothing was changed — `perch status --group --refresh` reads current figures.   # exit 15

$ perch switch
you@example.com is in no Group, so nothing has declared which Accounts it is interchangeable with. Nothing was changed.
Either put it in a Group with `perch group move you@example.com <group>`, or declare that every ungrouped Account is interchangeable with `perch config set cycle-ungrouped true`.   # exit 18
```

That last one is ADR 0017. An Account need not be in a Group — adoption leaves
the first one ungrouped — but being ungrouped is the *absence* of a declaration
that Accounts are interchangeable, not a weaker form of one. So bare `perch
switch` Cycles among ungrouped Accounts only when a global setting says it may,
and that setting is off until you turn it on.

## Managing Groups

`perch group add <name>` declares one, `perch group move <target> <group>` puts
an Account in it — `none` as the Group takes it out of every one — and `perch
group list` shows every Group with its Accounts and the rules in force for it.
`perch group remove <name>` forgets one, and is refused while it still holds
Accounts rather than quietly orphaning them.

`perch group rename <old> <new>` changes what a Group is called and **keeps
everything it carries**: its Overrides, the Accounts in it, and the cooldown the
watcher is pacing it by. Doing it by hand would be an add, a move per Account
and a remove — and a freshly declared Group declares nothing, so every rule you
had set on the old one would have to be typed again.

```
$ perch group rename work day-job
Renamed the Group `work` to `day-job`, which still holds 3 Accounts.
  Strategy     soonest-reset
  Watcher      off (would act at 55%, onto 45% or better, at most every 15m)
  Overrides    strategy, watcher-threshold-percent
```

The `Overrides` line is the point: those two Settings were said about this Group,
and they are still said about it afterwards. A rename by hand would have left
them behind on a Group that no longer exists.

A name that an Alias or another Group already answers to is refused before
anything is written, because Aliases and Group names share one namespace.
Changing only how a Group is capitalised is a rename rather than a collision
with itself, the same way naming an Account by the name it already answers to
is.
