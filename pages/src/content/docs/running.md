---
title: "Running one Account in one terminal"
sidebar:
  order: 6
---

`perch run <target>` launches Claude Code or Codex against one Account's
Profile. Select the provider explicitly with either flag:

```sh
perch run --claude work
perch run personal --codex
perch config set --global run-provider codex
```

Without a flag, Perch tries the configured provider (Claude by default), then
the other CLI if the preferred CLI is absent. An explicit flag never falls back.
An Account from the other provider is refused with the matching flag suggested.
A failed login, exhausted quota, or child failure does not trigger fallback.

Codex support is experimental. Add a subscription-backed Account with
`perch add --codex --alias personal --no-group`. Each user and Workspace pair
has its own `CODEX_HOME`, file Credential, configuration, and history. Use
Aliases when the same email names more than one Workspace. An unaliased Account
can also be reached by its `id` from `perch list --json`. `perch list --refresh`
asks the Codex app-server for percentage quotas when the Profile is idle;
otherwise the cached figure keeps its original age. Credit and spend-control
states that Perch cannot represent remain unknown or retain the prior cache.

Runs stay on their selected Account. Codex live Switching and unattended Codex
Cycling are not available yet; Claude's Watcher only chooses Claude Accounts.
Groups may contain both providers, and each Account remains visible.

After `--`, a leading flag goes to the selected coding tool. A program name runs
that program with the named Account's Profile, without requiring either coding
CLI. The client's exit code becomes Perch's exit code.

## Running as an Account

```
$ perch run overflow
Running Claude Code as overflow@example.com (as `overflow`), in this terminal alone.
```

Every other terminal, the editor extension and the desktop app go on as the
Account they were on. Two terminals running two Accounts is what the command is
for. Your memory, settings, plugins, past work and plans are linked into the
Account's Profile before the launch, so Claude Code opens with your Shared
State. Where a link cannot be made, the Run is refused and names the entry.

Trust and tool approvals for the repository you are standing in are carried
over from the most recently used Profile in the same Group, so the first Run of
a new Account does not ask for trust again mid-task.

The client's exit code is Perch's, so `perch run` stands in a script wherever
`claude` would.

```
$ perch run work
`work` is a Group. Name one Account: its Alias, or its email address.   # exit 14
```

A Quarantined Account is refused rather than launched into a login prompt.

## What a Run protects while it lasts

```
$ perch switch overflow
A client is running against you@example.com's Profile (pid 4242).
Quit it, or `perch switch` to another Account.   # exit 16
```

While a Run is going, its Profile is Live, and Perch writes nothing into it.
A Switch away from the Account you are running is refused as above. A
`--refresh` on that Account shows the cached figure and says why:

```
$ perch status --refresh
you@example.com: its access token has expired and a client is running against it (pid 4242 in /Users/you/.config/perch/profiles/you-example-com), so it was not Renewed.
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

Reading is untouched. `perch switch` onto the Account you are running lands
normally, and a Run and a Switch do not lock each other out. A Run that was
killed rather than closed leaves nothing behind that matters.

## Running with arguments, and running something else

```
$ perch run overflow -- --resume --model opus
Running Claude Code as overflow@example.com (as `overflow`), in this terminal alone.

$ perch run overflow -- npm test
Running `npm` as overflow@example.com (as `overflow`), in this terminal alone.
```

Everything after `--` reaches the program exactly as typed. A first word that
is a flag goes to Claude Code. Any other first word is the program to launch,
with `CLAUDE_CONFIG_DIR` pointed at the Account's Profile.

```
$ perch run dev --resume
`--resume` could be Perch's flag or the program's, and Perch will not guess which. Everything meant for the program you are running goes after `--`:

        perch run dev -- --resume
```

A program typed without the separator is refused the same way.
