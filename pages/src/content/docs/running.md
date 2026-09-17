---
title: "Running one Account in one terminal"
sidebar:
  order: 6
---

`perch run <target>` launches one Account in one terminal and leaves the
active Account alone. A Claude Account launches Claude Code; a Codex Account
launches Codex.

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

## Running a Codex Account

```
$ perch run personal
Running Codex as person@example.com (as `personal`), in this terminal alone.
```

A Codex Account runs with its own `CODEX_HOME`, so its login, configuration and
history stay apart from every other Account's. Codex support is experimental.

```
$ perch run --claude personal
personal is a Codex Account. `--codex` selects it.   # exit 14
```

`--claude`, `--codex` and `--provider <name>` name the provider outright. An
Account of the other provider is refused, never launched with the wrong
client.

## Choosing the provider for a bare `perch run`

```
$ perch config set --global run-provider codex
run-provider: codex
```

Without a flag, `perch run` uses the `run-provider` CLI, and the other one
only if that CLI is not installed. An explicit flag never falls back, and
neither does a failed login, an exhausted quota or a client that exits with an
error: the exit code is the client's.

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
you@example.com: its access token has expired and a client is running against it (pid 4242 in /Users/you/.config/perch/providers/claude/profiles/claude-4206ebf7d1d5bb1f78b812321decc32809cac5786ade33037181bbdd08da2e94), so it was not Renewed.
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
