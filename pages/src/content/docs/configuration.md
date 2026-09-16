---
title: "Configuration"
sidebar:
  order: 8
---

`perch config` changes the rules Perch chooses Accounts by, and which provider
a bare command reaches. A policy Setting is said about a Scope: a Group by
name, or `ungrouped` for the Accounts in no Group. An Account carries no
Settings, and nothing here prompts.

## The Settings

```
$ perch config set work watcher-may-act true
`watcher-may-act` on Group `work` is now true.
`perch watcher run` may Switch within Group `work` on your behalf when the Account you are on reaches its threshold. Only while a Watcher is running: `perch watcher run`, a Service `perch watcher install` set up, or a `perch watcher check` on a schedule. Nothing here starts one.
```

`perch config set <scope> <key> <value>` sets one Setting and says what it now
means. It reaches the Scope it names and no other: a Group declared tomorrow
starts at the defaults. There is no `unset`. Set a value to what it should be.
`ungrouped`, `none` and `global` are refused as Group names and Aliases.

| Key | Said about | Values | Default |
| --- | ---------- | ------ | ------- |
| `strategy` | any Scope | `most-headroom`, `soonest-reset` | `most-headroom` |
| `preferred-workload` | any Scope | `true`, `false` | `false` |
| `watcher-may-act` | any Scope | `true`, `false` | `false` |
| `watcher-threshold-percent` | any Scope | 0-100 | `80` |
| `watcher-margin-percent` | any Scope | 1-100 | `10` |
| `interchangeable` | `ungrouped` only | `true`, `false` | `false` |

Every Scope carries all of them but `interchangeable`, which only `ungrouped`
carries. `perch config set --help` lists the same keys and values.

## The Watcher's numbers

```
$ perch config set work watcher-threshold-percent 70
`watcher-threshold-percent` on Group `work` is now 70.
`perch watcher run` Switches within Group `work` once that much of the fullest Quota Window of the Account you are on has been used. Only while a Watcher is running: `perch watcher run`, a Service `perch watcher install` set up, or a `perch watcher check` on a schedule. Nothing here starts one.

$ perch config set work watcher-margin-percent 20
`watcher-margin-percent` on Group `work` is now 20.
`perch watcher run` will only move within Group `work` to an Account at 50% or under. A round with nowhere that empty to go says so and moves nothing. Only while a Watcher is running: `perch watcher run`, a Service `perch watcher install` set up, or a `perch watcher check` on a schedule. Nothing here starts one.
```

The margin is in points under the threshold. A margin wider than the threshold
is allowed, and means the watcher moves only onto an Account with nothing used.

A Group holding both Claude and Codex Accounts refuses `watcher-may-act`
without `--provider`: `perch config set work --provider claude watcher-may-act
true`. Permission for one provider grants nothing to the other, and the
Watcher does not Cycle Codex Accounts yet.

## Strategy and workload

```
$ perch config set work strategy soonest-reset
`strategy` on Group `work` is now soonest-reset.
A Cycle within Group `work` prefers the Account whose fullest Quota Window resets soonest, so perishable quota is spent rather than wasted. Headroom is still measured by the worst window, so an exhausted Account is still never chosen however soon it comes back.

$ perch config set work preferred-workload true
`preferred-workload` on Group `work` is now true.
A Cycle within Group `work` first ranks Accounts that can serve the preferred workload, using the windows identified by their provider. Once that capacity is spent, it ranks the remaining capacity.
```

`most-headroom` prefers the Account with the most room left. `soonest-reset`
prefers the Account whose quota is about to be thrown away. Where a cached
figure carries no reset time, `soonest-reset` ranks it below one that does,
and a Cycle with no reset times to compare says it fell back to room.

`preferred-workload` turns on the provider's default workload: Fable, for
Claude. With it on and no Account reporting that window, the listing says so
and ranks on Headroom alone. Perch supplies capacity only. Which model a
session uses stays with the session. A Group holding both providers Cycles
each provider's Accounts on their own; Claude capacity never stands in for
Codex capacity.

The provider's own option is under `option.`:

```
$ perch config set work --provider claude option.preferred_workload fable
work claude option.preferred_workload: fable

$ perch config get work --provider claude option.preferred_workload
fable
```

Claude takes `preferred_workload=fable` and refuses any other option or value.

## Where a value comes from

```
$ perch config set --defaults watcher-threshold-percent 75
Scope default watcher-threshold-percent: 75

$ perch config set work --provider claude watcher-threshold-percent 70
work claude watcher-threshold-percent: 70

$ perch config set work strategy inherit
work strategy: inherited
```

A policy value is read from the first of these that sets it: the Scope's
override for that provider, the Scope's own value, `--defaults`, then the
compiled default. `inherit` removes an override for `strategy`,
`watcher-threshold-percent` and `watcher-margin-percent`; `watcher-may-act`
is never inherited and is set per Scope and provider. `perch config get
--effective work --provider claude` prints each value with where it came from.

## Letting the ungrouped Accounts Cycle

```
$ perch config set ungrouped interchangeable true
`interchangeable` on the Ungrouped Scope is now true.
A bare `perch switch` from an Account in no Group now Cycles among the other ungrouped Accounts. That declares every ungrouped Account interchangeable at once, present and future, including the next one `perch add` creates.

$ perch config set ungrouped watcher-may-act true
`watcher-may-act` on the Ungrouped Scope is now true.
`perch watcher run` may Switch among the Accounts in no Group on your behalf when the Account you are on reaches its threshold. Those Accounts have also been declared interchangeable, which is the other half of it: the watcher acts here only where `interchangeable` is on too. Only while a Watcher is running: `perch watcher run`, a Service `perch watcher install` set up, or a `perch watcher check` on a schedule. Nothing here starts one.
```

The Accounts in no Group need both. A Group needs only `watcher-may-act`, and
does not carry `interchangeable`.

## Choosing a provider

```
$ perch config set --global run-provider codex
run-provider: codex

$ perch config set --global run-fallback disabled
run-fallback: disabled

$ perch config set --global watcher-paused true
watcher-paused: true
```

| Key | Values | Default |
| --- | --- | --- |
| `run-provider` | `claude`, `codex` | `claude` |
| `run-fallback` | `installed`, `disabled` | `installed` |
| `watcher-paused` | `true`, `false` | `false` |

`run-provider` is the CLI a `perch run` or `perch triage` with no `--claude` or
`--codex` reaches. `perch add` without a flag adds a Claude Account. With `run-fallback` at `installed`, a bare
command whose preferred CLI is missing runs the other enabled one; an explicit
flag never falls back. `watcher-paused` holds every provider's unattended
Switching, and clearing it leaves every grant as it was. `perch config get
--global` reads the three back.

## Provider installation

```
$ perch config set --provider codex cli-path /opt/bin/codex
codex cli-path: /opt/bin/codex

$ perch config get --provider codex
enabled true
cli-path /opt/bin/codex
```

Both providers start enabled. `cli-path auto` clears the path, and Perch then
takes `PERCH_CLAUDE_BIN` or `PERCH_CODEX_BIN`, then the first on your PATH.
`enabled false` keeps a provider's Accounts and stops Perch reaching for its
CLI. Providers are built into Perch; there is no third to configure.

## Reading it back

```
$ perch config get
--global:
  run-provider claude
  run-fallback installed
  watcher-paused false

ungrouped:
interchangeable            true
strategy                   most-headroom
preferred-workload         false
watcher-may-act            true
watcher-threshold-percent  80
watcher-margin-percent     10

personal:
strategy                   most-headroom
preferred-workload         false
watcher-may-act            false
watcher-threshold-percent  80
watcher-margin-percent     10

work:
strategy                   soonest-reset
preferred-workload         true
watcher-may-act            true
watcher-threshold-percent  70
watcher-margin-percent     20

$ perch config get work strategy
soonest-reset
```

`perch config get <scope>` prints one Scope's page without its heading. A
Scope and a key print the value alone, for `$(perch config get work strategy)`.
Each row under a Scope's name is the `perch config set` that would restore it.
`perch config get work --provider claude` prints the page as Claude sees it.

A `set` that names no Scope, an unknown key or a value out of range is refused
and names what would have worked.

## Files

```
~/.config/perch/
  config.json
  providers/claude/
    state.json
    profiles/
    pending/
  providers/codex/
    state.json
    profiles/
    pending/
```

`config.json` holds `global`, `providers`, `scope_defaults`, `groups`,
`ungrouped` and `accounts`. Each provider's `state.json` holds its active
Account, observations, Quarantines and Watcher pacing. Credentials stay in the
provider's own store, never in these files.

This layout is new, and Perch refuses an older one with instructions rather
than migrating it. Keep the old directory aside until the new installation is
set up. An Export written by an older Perch opens with the Perch that wrote it.
