---
title: "Accounts"
sidebar:
  order: 2
---

An Account is one Claude login Perch holds. Gain one, name one, keep one out
of Cycling, repair one, give one up.

## Adopting the login you already have

```
$ perch status
Adopted the Claude Code login as you@example.com (Acme, pro).
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

The first command you run takes the Claude Code login already on the machine as
your first Account, and a Codex login the same way: `Adopted the Codex login as
you@example.com (plus).` Nothing is logged into again. That Account is in no
Group, so a bare `perch switch` has nothing to Cycle to until you put it in
one, or declare the ungrouped Accounts interchangeable. A Codex login kept in
the keyring, or made with an API key, is left where it is and not adopted.

## Adding an Account

```
$ perch add --group work --alias overflow
Logging in to a new Profile.
Quit Claude Code when the login is done.

Added overflow@example.com (Overflow Ltd, max).
Alias:  overflow
Group:  work
Group `work` now holds 2 Accounts, and nothing Cycles between them unasked: `perch config set work watcher-may-act true` says it may.
```

A browser opens for the login. Log in as the new Account, then quit Claude Code
to come back. The Account you were on stays active in every terminal.

`--group <name>` puts the new Account in a Group, and `--no-group` puts it in
none. Without either, Perch offers the Account's organization as the Group and
asks you to confirm. `--alias <name>` names the Account at the same time.

```
$ perch add --no-group
Logging in to a new Profile.
Quit Claude Code when the login is done.

Added spare@example.com (Spare Ltd, pro).
Group:  none
```

In a script, pass one of the two flags, or the Add is refused.

## Adding a Codex Account

```
$ perch add --codex --alias personal --no-group
Logging in to a new Profile.

Added person@example.com (workspace-1, plus).
Alias:  personal
Group:  none
```

`--codex` logs in with the Codex CLI instead, and the login returns on its own.
Each Codex Workspace is its own Account, so one email can be held twice, once
per Workspace: give each an Alias, since the shared email then names neither.
Codex support is experimental. A Switch to a Codex Account changes the login
the next `codex` starts with; one already open keeps its Account until it is
restarted.

## Naming an Account

```
$ perch alias overflow@example.com overflow
`overflow` now names overflow@example.com.

$ perch alias overflow --unset
`overflow` no longer names overflow@example.com.
```

Every command that takes a Target takes the Alias or the email address. To
free a name you have forgotten, name the Account it is on.

Aliases and Group names share one namespace. A name is letters, digits, `_` and
`-` in any alphabet, opening with a letter, a digit or `_`. `work` and `Work`
are the same name. `ungrouped`, `none` and `global` are refused as names.

## Keeping an Account out of Cycling

```
$ perch disable spare
Disabled spare@example.com (as `spare`).

$ perch enable spare
Enabled spare@example.com (as `spare`).
```

A disabled Account keeps its Alias, its Group and its Credential. `perch list`
shows it as `disabled`, and `perch switch spare` still switches to it. Only
Cycling passes it over. Disabling every Account in a Group is allowed, and a
bare `perch switch` there then finds nowhere to land.

Enabling does not repair a Quarantined Account:

```
$ perch enable spare
spare@example.com (as `spare`) was already enabled. It is Quarantined: the provider would not renew its Credential. `perch relogin spare@example.com` repairs it.
```

## When an Account breaks

```
$ perch status
Account       you@example.com
Organization  Acme
Plan          pro
Quarantine    the provider would not renew its Credential. `perch relogin you@example.com` repairs it.
Utilization   never observed
```

An Account whose Credential stopped working is Quarantined. It stays listed
and named, with the reason and the repair beside it. Cycling never chooses it,
and `perch switch` onto it is refused.

```
$ perch relogin overflow
Logging in again to repair overflow@example.com.
Quit Claude Code when the login is done.

Repaired overflow@example.com (as `overflow`). It is no longer Quarantined.
```

Log in as the same Account in the browser that opens. The Account keeps its
Alias, its Group, its place in the listing and whether Cycling may choose it.
Only the Credential is replaced. A login as a different Account is refused.
Abandoning the login changes nothing.

Relogging in the Account you are on also makes the fresh Credential the live
one. A healthy Account may be relogged in too.

## Giving up an Account

```
$ perch remove spare
Removed spare@example.com (as `spare`).
The Alias `spare` is free to use again.
```

The Account is forgotten and the Credential Perch holds for it is deleted. The
Group it was in stays declared.

Removing the Account you are on asks first, and lands you somewhere else before
anything is deleted:

```
$ perch remove work-main
you@example.com (as `work-main`) is the active Account. overflow@example.com (as `overflow`) will be made active first; `perch switch <target>` before this lands somewhere else. Its Credential is deleted with it.
Remove you@example.com (as `work-main`)? [y/N]: y
overflow@example.com (as `overflow`) is the active Account now.
Removed you@example.com (as `work-main`).
The Alias `work-main` is free to use again.
```

It lands on an Account in the same Group where there is one, never on a
disabled or Quarantined Account, and never ranked by Headroom. `perch switch
<target>` first if you want a different landing. Removing the last Account is
allowed and confirmed the same way, and does not log Claude Code out.

`--yes` agrees in advance. Without a terminal and without the flag, a removal
that would have asked is refused instead, and end of input is a no.
