---
title: "Configuration"
sidebar:
  order: 8
---

`perch config` changes the rules Perch chooses Accounts by. Every Setting is
said about a Scope: a Group by name, or `ungrouped` for the Accounts in no
Group. There is nothing above a Scope, and an Account carries no Settings.

## The Settings

```
$ perch config set --help
Set one Setting on one Scope.

`<scope>` is a Group by name, or `ungrouped`. `<key>` and `<value>`:
  interchangeable            `true` or `false`
  strategy                   `most-headroom` or `soonest-reset`
  prefer-fable               `true` or `false`
  watcher-may-act            `true` or `false`
  watcher-threshold-percent  a whole number between 0 and 100
  watcher-margin-percent     a whole number between 1 and 100
```

Every Scope carries all of them but `interchangeable`, which only `ungrouped`
carries. `most-headroom` prefers the Account with the most room left.
`soonest-reset` prefers the Account whose quota is about to be thrown away, so
it is spent rather than wasted. `perch config get` reads every Setting back.

| Key | Said about | Values | Default |
| --- | ---------- | ------ | ------- |
| `strategy` | any Scope | `most-headroom`, `soonest-reset` | `most-headroom` |
| `prefer-fable` | any Scope | `true`, `false` | `false` |
| `watcher-may-act` | any Scope | `true`, `false` | `false` |
| `watcher-threshold-percent` | any Scope | 0–100 | `80` |
| `watcher-margin-percent` | any Scope | 1–100 | `10` |
| `interchangeable` | `ungrouped` only | `true`, `false` | `false` |

## Setting one

```
$ perch config set work watcher-may-act true
`watcher-may-act` on Group `work` is now true.
`perch watcher run` may Switch within Group `work` on your behalf when the Account you are on reaches its threshold. Only while a Watcher is running: `perch watcher run`, a Service `perch watcher install` set up, or a `perch watcher check` on a schedule. Nothing here starts one.
```

`perch config set <scope> <key> <value>` sets one Setting and says what it now
means. It reaches the Scope it names and no other: a Group declared tomorrow
starts at the defaults. There is no `unset`. Set a value to what it should be.

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

```
$ perch config set work strategy soonest-reset
`strategy` on Group `work` is now soonest-reset.
A Cycle within Group `work` prefers the Account whose fullest Quota Window resets soonest, so perishable quota is spent rather than wasted. Headroom is still measured by the worst window, so an exhausted Account is still never chosen however soon it comes back.

$ perch config set work prefer-fable true
`prefer-fable` on Group `work` is now true.
A Cycle within Group `work` now puts the Accounts that can serve Fable first, ranked by the room in their Fable weekly window, and falls through to the rest — ranked without that window — only when Fable is spent everywhere.
```

Where a cached figure carries no reset time, `soonest-reset` ranks it below
one that does, and a Cycle with no reset times to compare says it fell back to
room. With `prefer-fable` on and no Account reporting a Fable window, the
listing says so and ranks on Headroom alone. Perch supplies Fable capacity
only. Which model a session uses stays with the session.

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

## Reading it back

```
$ perch config get
ungrouped:
interchangeable            true
strategy                   most-headroom
prefer-fable               false
watcher-may-act            true
watcher-threshold-percent  80
watcher-margin-percent     10

personal:
strategy                   most-headroom
prefer-fable               false
watcher-may-act            false
watcher-threshold-percent  80
watcher-margin-percent     10

work:
strategy                   soonest-reset
prefer-fable               true
watcher-may-act            true
watcher-threshold-percent  70
watcher-margin-percent     20

$ perch config get work strategy
soonest-reset
```

`perch config get <scope>` prints one Scope's page without its heading. A
Scope and a key print the value alone, for `$(perch config get work strategy)`.
Each row under a Scope's name is the `perch config set` that would restore it.

A `set` that names no Scope, an unknown key or a value out of range is refused
and names what would have worked.
