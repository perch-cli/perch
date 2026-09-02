---
title: "Switching, Cycling and Groups"
sidebar:
  order: 4
---

`perch switch` with a target moves the whole machine onto an Account you name.
With no target it picks for you, within the Group the Account you are on belongs
to. A Group is how you declare which Accounts are interchangeable.

## Switching

`perch switch <target>` makes an Account active everywhere — every terminal, the
editor extension, the desktop app — with no login flow.

```
$ perch switch overflow
`overflow` is an Alias for overflow@example.com.
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour    12%  (as of 4m ago)
```

It is three steps in one order and never another. The Credential you are leaving
is **Captured** back into its own Profile first, because Anthropic retires a
refresh token whenever it issues a new one — so the copy in an Account's Profile
is several Rotations behind by the time you switch away, and skipping the
Capture would quietly poison the Account you are leaving. Then the incoming
Credential is written to the Default Profile. Then the `oauthAccount` block of
`.claude.json` is patched to match, and only that block: your project history,
MCP configuration and settings live in the same file and belong to you rather
than to the Account.

The Capture happens on every Switch, without exception, which is why no Switch
says that it did. What a Switch says is what you could not have predicted:
where you landed, and what the cache says you have there. The Capture *not*
happening the ordinary way is said — Claude Code logged out, a live Credential
belonging to somebody else, bytes nothing can read — because that is the part
this paragraph does not already tell you.

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
circumstances.

```
$ perch switch
Switched to overflow@example.com, the most room in Group `work`.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
```

It Cycles **within the current Account's Group** and never leaves it, so a work
subscription running dry does not land you on your personal Account. `perch
switch <group>` Cycles within a Group you name instead.

Each Account is ranked by its **worst** Quota Window, and the Account whose
worst is best wins. Being blocked by any window blocks you completely, so that
is the only ranking that measures what actually stops you working: the headroom
Perch reports is true of every one of that Account's windows. Exhausted,
disabled and Quarantined Accounts are never chosen, and an Account nobody has
ever read a figure for is ranked below every Account that has one — no figure
and plenty of room are opposite pieces of advice.

How headroom is measured is fixed; which Account to prefer is the Group's to
say. `perch config set <group> strategy soonest-reset` makes a Cycle take the
Account whose fullest window comes back soonest instead of the one with the
most room, so perishable quota is spent rather than wasted — see
[Configuration](configuration.md). It reorders the Accounts that have room and
nothing else: an exhausted Account is never chosen however soon it resets.

Before it ranks, a Cycle reads current Utilization — but only for the Accounts
it cannot rank without. Quota climbs within a window and refills when the window
comes back, so an Account's cached figure is the most room it could still have
until its reset passes. A candidate that would lose even if its quota had
refilled entirely is not worth a round trip, and is not read. Often that is
every one of them, and a Cycle spends nothing.

An Account that could not be read — a spent hourly allowance, an endpoint that
is not answering — keeps whatever figure it had, is ranked on it, and is named
on its own line. A Switch you asked for is one you get; the line says how old
the figure it was ranked on is, so what the choice rested on is still in front
of you.
`perch switch --no-refresh` skips all of it and Cycles on the cache, for when
you are offline or in a hurry. Nothing is read when you name an Account,
because naming one decides nothing.

The landing line says which Account was chosen and what it was chosen on; the
argument for why it beat the others is not something a Switch owes you.

Three outcomes are honest non-outcomes. They perform no Switch, explain
themselves, and exit with a code of their own rather than pretending to have
worked. How much they say turns on whether there is anything for you to do: a
Cycle with nowhere to land explains itself and names the next command, and one
that is already where it wants to be says only that.

```
$ perch switch
Every Account in Group `work` is exhausted.
you@example.com frees up soonest, at 2026-08-04 15:00 UTC (in 3h).   # exit 17

$ perch switch
you@example.com is already the best Account in Group `work`.   # exit 15

$ perch switch
you@example.com is in no Group, so nothing has declared which Accounts it is interchangeable with.
Either put it in a Group with `perch group move you@example.com <group>`, or declare that every ungrouped Account is interchangeable with `perch config set ungrouped interchangeable true`.   # exit 18
```

That last one is not a gap. An Account need not be in a Group — adoption leaves
the first one ungrouped — but being ungrouped is the *absence* of a declaration
that Accounts are interchangeable, not a weaker form of one. So bare `perch
switch` Cycles among ungrouped Accounts only once that Scope has been declared
`interchangeable`, and it is off until you say so.

## Managing Groups

`perch group add <name>` declares one, `perch group move <target> <group>` puts
an Account in it — `none` as the Group takes it out of every one — and `perch
group list` shows every Group with its Accounts and the rules in force for it.
`perch group remove <name>` forgets one, and is refused while it still holds
Accounts rather than quietly orphaning them.

`perch group rename <old> <new>` changes what a Group is called and **keeps
everything it carries**: its Settings, the Accounts in it, and the cooldown the
watcher is pacing it by. Doing it by hand would be an add, a move per Account
and a remove — and a freshly declared Group starts at the compiled-in defaults,
so every rule you had set on the old one would have to be typed again.

```
$ perch group rename work day-job
Renamed the Group `work` to `day-job`, which still holds 3 Accounts.
```

What it says is what a rename by hand would have lost: the Accounts came with
the name. The rules did too — a rename never touches one, so it does not report
them, and `perch group list` is where they are read.

A name that an Alias or another Group already answers to is refused before
anything is written, because Aliases and Group names share one namespace.
Changing only how a Group is capitalized is a rename rather than a collision
with itself, the same way naming an Account by the name it already answers to
is.
