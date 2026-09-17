---
title: "Switching, Cycling and Groups"
sidebar:
  order: 4
---

`perch switch <target>` moves the whole machine onto an Account you name. With
no Target it picks for you, within the Group you are in. A Group is how you
declare which Accounts are interchangeable.

## Switching

```
$ perch switch overflow
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour  12%  (as of 4m ago)
              7-day   40%  (as of 4m ago)
```

Every terminal, the editor extension and the desktop app are now on that
Account, with no login flow. The figures under it are the cached ones, with
their age. Your memory, settings, plugins and project history are Shared State
and follow you across the Switch untouched.

A Switch never renews a token a running Claude Code is holding, and is
refused while a client is running against the Profile of the Account you are
leaving. Switching to the Account already active does nothing. A Switch that
fails part way says which Account is active now, and running it again finishes
the job.

A login made outside Perch, sitting in the live store when you Switch, is
replaced, and a `Note:` line under the verdict says whose it was. `perch add`
logs it in again as an Account.

## Switching a Codex Account

```
$ perch switch work
Switched to work@example.com (as `work`).
Note: a Codex already open keeps its Account until it is restarted.
Utilization   never observed
```

Codex has its own active Account, so a Switch between Codex Accounts leaves
the Claude Code one where it is, and the other way round. The Switch writes
the login Codex reads at its next start; a Codex already running keeps the
Account it opened with, so restart it. Perch switches Codex's file store only:

```
$ perch switch work
Codex keeps its login in its `keyring` store, which Perch does not switch. Put `cli_auth_credentials_store = "file"` in /Users/you/.codex/config.toml first.   # exit 14
```

A Group holding Accounts of both providers needs the provider named for a
Cycle: `perch switch work --codex`.

## Cycling

```
$ perch switch
Switched to overflow@example.com, the most room in Group `work`.
Utilization   5-hour  12%  (as of just now)
              7-day   40%  (as of just now)
```

A bare `perch switch` asks nothing and lands on the Account in your Group
with the most Headroom. It reads current Utilization first for the Accounts it
cannot rank without, and stays in your Group: a work subscription running dry
never lands you on your personal Account. `perch switch <group>` Cycles within
a Group you name instead.

Exhausted, disabled and Quarantined Accounts are never chosen. An Account never
read ranks below every Account with a figure. `perch config set <group>
strategy soonest-reset` prefers the Account whose fullest window resets soonest
instead.

An Account that could not be read is ranked on its cached figure and named on
a line of its own, with the age of that figure. `perch switch --no-refresh`
Cycles on the cache alone, for when you are offline.

Two outcomes Switch nothing and say why:

```
$ perch switch
overflow@example.com is already the best Account in Group `work`.   # exit 15

$ perch switch
Every Account in Group `work` is exhausted.
you@example.com frees up soonest, at 2026-08-04 15:00 UTC (in 3h).   # exit 17


```

An Account in no Group has nothing to Cycle to until you put it in a Group or
run `perch config set ungrouped interchangeable true`.

## Managing Groups

```
$ perch group add work
Declared the Group `work`.

$ perch group move you@example.com work
Moved you@example.com into `work`.

$ perch group move overflow@example.com work
Moved overflow@example.com into `work`.

$ perch group list
work
  Accounts     you@example.com
               overflow@example.com
  Strategy     most-headroom
  Watcher      off (would act at 80%, onto 70% or better, at most every 15m)

In no Group
  Accounts     spare@example.com
  Cycling      off — `interchangeable` is false
  Strategy     most-headroom
  Watcher      off (would act at 80%, onto 70% or better, at most every 15m)
```

`perch group list` shows every Group, its Accounts and the Settings in force
for it. `perch group move <target> none` takes an Account out of every Group.
The Settings a Group carries are in [Configuration](configuration.md).

```
$ perch group rename work day-job
Renamed the Group `work` to `day-job`, which still holds 2 Accounts.
```

A rename keeps the Group's Accounts, its Settings and the cooldown the watcher
is pacing it by. A name an Alias or another Group already answers to is
refused. Changing only the capitalization is a rename. `perch group remove
<name>` forgets a Group, and is refused while the Group still holds Accounts.
