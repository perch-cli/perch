---
title: "Reference"
sidebar:
  order: 9
---

## Commands

| Command | What it does |
| ------- | ------------ |
| `perch status [--refresh] [--json]` | the active Account and how full it is |
| `perch list [<scope>] [--refresh] [--json]` | every Account, its Alias, Group, state, Headroom and Utilization, in the order a Cycle ranks them — or one Group's, or the ungrouped ones', with what that Scope has left to draw on |
| `perch add [--group <name>\|--no-group] [--alias <name>]` | gain an Account by logging in, without disturbing the active one |
| `perch alias <target> <name>` / `perch alias <target> --unset` | name an Account, or free the name |
| `perch switch <target>` | make an Account active everywhere |
| `perch switch [<group>]` | Cycle to the best Account in a Group |
| `perch watcher run` | Cycle automatically when the Account you are on runs low |
| `perch watcher check` | take one round for cron or a systemd timer, saying what it decided in the exit code |
| `perch watcher install\|uninstall` | have the machine run the watcher for you, starting at login, or take that unit back |
| `perch watcher status [--json]` | whether a Service is installed, whether it is running, and whether a Watcher holds the lock right now |
| `perch run <target> [-- <command>]` | launch Claude Code as an Account, in this terminal alone |
| `perch group add\|move\|rename\|remove\|list` | declare Groups and move Accounts between them |
| `perch config set\|get` | the rules Perch chooses Accounts by, one Scope at a time |
| `perch disable <target>` / `perch enable <target>` | keep an Account out of Cycling, or put it back |
| `perch relogin <target>` | repair an Account whose Credential stopped working |
| `perch remove <target> [--yes]` | give up an Account |
| `perch holdings export <path>` / `perch holdings import <path>` | write everything Perch holds to one encrypted file, and put it back |
| `perch holdings purge [--yes]` | give the machine back the state it had before Perch |
| `perch upgrade [--release <tag>] [--check] [--json] [--channel <name>] [--yes]` | replace this Perch with a newer Release, through the Channel that installed it |
| `perch version` | which Perch is installed, and a line more when a newer Release exists |
| `perch probe [--json] [--raw]` | everything Perch can see of this machine, for pasting into a bug report |

## Reporting something broken

`perch probe` gathers what a report needs and would otherwise be typed by hand:
which Perch and which Claude Code, how Perch was installed, where its files are,
what the Holdings hold, which of its assumptions about Claude Code still hold,
and what has been run here lately.

```
$ perch probe
Findings
  <account 3> is Quarantined, so Cycling will not choose it and a Switch to it
  refuses. `perch relogin <account 3>` is the way back. (exit 19)

Perch         0.2.0 (linux x86_64), installed by npm
Claude Code   2.1.221, at <home>/.local/share/claude/bin/claude
Home          <home>/.config/perch, registry version 5
Active        <account 1>
Holdings      4 Accounts in 2 Groups, 1 Quarantined, 0 Disabled
Watcher       installed, running, may act somewhere
Its log       <home>/.config/perch/watch.log
Trail         412 lines, last written 2026-08-28 09:16:41Z
...
```

The judgment comes first and the facts sit under it, so the facts still stand
where the judgment is wrong. A finding only ever restates something Perch
already works out to decide a refusal, and carries the code that refusal exits
with.

Email addresses, Alias and Group names and your home directory come out as
placeholders, because the point of this is pasting it somewhere else. The
numbers are the Account's place in the registry, so `<account 3>` is the same
Account every time you run it — two reports a week apart are comparable.
`--raw` prints the names as they are, and is worth checking before you paste.

It reads and judges and repairs nothing. No network, no registry brought
forward, no line added to the Trail, and it exits `0` whatever it finds.

Every command writes two lines to the **Trail** as it goes, one when it starts
and one when it ends: what was typed, and what it exited with. Words after `--`
go to Claude Code and are counted rather than recorded. A start with no end
whose process is gone is a command that died without a word, and `perch probe`
says so — one whose process is still running is not, which is why the Watcher
sitting in its loop is never reported as a failure. The Watcher adds a line of
its own when it Switches. The Trail is not one of the Holdings — it is never
exported, and a Purge takes it with everything else.

`perch probe` names the Watcher's own log and does not read it. On macOS and
Windows that is `watch.log` beside the registry; on Linux systemd keeps it, and
what the Probe prints is the `journalctl` line to run.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account, no such Group |
| 13 | it collides with something that is already there — an Account added twice, a name already spoken for, a path an Export would have written over, an Import onto a Perch that already holds an Account |
| 14 | Perch understood it and will not accept it — an ambiguous name, a value out of range, a Group that has not said the watcher may act on it, a command that needs a terminal run where there is none |
| 15 | there was nothing to do — you are already on that Account, a check found nothing to do now, or Perch is holding nothing on this machine to purge |
| 16 | refused: a client is running against that Profile, so what is in it is not Perch's to write or to delete |
| 17 | a Cycle found nowhere to land — every Account in the Group is exhausted, or none is a candidate |
| 18 | a bare Cycle, or a watcher, on an Account nobody has declared interchangeable with anything |
| 19 | that Account is Quarantined — its Credential no longer works, and `perch relogin` repairs it |
| 20 | held: a lock somebody else has, or a `perch watcher check` with no current figure to decide on. Nothing is wrong and nothing was changed — ask again shortly |

`perch run` is the one command these do not describe once it has launched
something: what the client exited with is what Perch exits with, so a script
wrapping it reads the program's own code rather than Perch's. Everything that
stops a Run before the launch — a command line without `--`, an unknown Target,
a Group, a Quarantine, a Reconcile that could not be made — is in the table
above.

`perch upgrade` is the same once it has handed the work to a Channel: what
`brew` or `npm` exited with is what Perch exits with. `perch upgrade --check`
exits 0 whether or not there is a newer Release — it is a question, and
answering it is success either way, so branch on `--json`'s `upgrade_available`
rather than on the code.

`perch watcher status` is the same shape of question and exits 0 whether or not
a Service is installed — branch on `--json`'s `installed`, `running` and
`watching`, which are three different facts. `perch watcher uninstall` exits 15
when there was nothing to take back, and a Check or a Watcher that finds another
Watcher holding the lock exits 20.

## Where things are

- `~/.config/perch/registry.json` — Perch's own state, versioned.
- `~/.config/perch/.watch.lock` — held for as long as a Watcher runs, which is
  what makes it the only one on the machine. Given back however the process
  ends; a second Watcher says who holds it and waits rather than deciding
  alongside them.
- `~/.config/perch/trail.log`, and `~/.config/perch/trail.log.1` once the first
  has grown past a megabyte and been moved aside. What each command was asked
  and what it decided, which `perch probe` reads back. Never written where Perch
  has no home yet, so a machine Perch holds nothing on stays one.
- `~/.config/perch/watch.log` — where a Service's decisions go on macOS and
  Windows, whose service managers keep no log of their own. On Linux there is no
  such file: systemd captures standard output into the journal, so `journalctl
  --user -u perch-watch -f` is the line.
- The unit a Service is installed as, which is the one thing Perch writes
  *outside* `$PERCH_HOME` — `~/Library/LaunchAgents/cli.perch.watch.plist` on
  macOS, `~/.config/systemd/user/perch-watch.service` on Linux, and a Scheduled
  Task named `Perch\Watch` on Windows, which Windows keeps rather than Perch.
  `perch watcher uninstall` removes it, and so does `perch holdings purge`.
- `~/.config/perch/profiles/<account>/` — one directory per Account. Its path is
  what gives that Account a private Credential Store.
- `$PERCH_HOME` overrides `~/.config/perch`. Home is `$USERPROFILE` on Windows
  and `$HOME` elsewhere; a machine that cannot say where home is gets a refusal,
  never a write into the filesystem root. `~/.config` is created if it is not
  there, and the same path is used on every platform, Windows included, rather
  than `%APPDATA%` — one rule to document and to support, and `$PERCH_HOME` for
  anybody who wants a different one.
- `$PERCH_CLAUDE_BIN` overrides where `claude` is found. Without it, Perch walks
  `PATH` itself — consulting `PATHEXT` on Windows, so the `claude.cmd` an npm
  install leaves works from every shell.
- `$PERCH_NO_UPGRADE_CHECK` stops `perch version` asking whether a newer
  Release exists. Checked before the request, so nothing goes out. That check is
  the only place Perch looks for its own updates; `perch status` never touches
  the network.
- `$PERCH_INSTALL_DIR` is where the installer script puts the binary, in place
  of `~/.local/bin` and of `%LOCALAPPDATA%\Perch\bin` on Windows. `perch
  upgrade` reads it too, because it is what tells an Installation the installer
  made from a binary somebody unpacked by hand, which Perch refuses to write
  over.
- `$PERCH_VERSION` holds the installer script to one Release rather than the
  newest. Read by the installers rather than by Perch: `perch upgrade --release`
  sets it when it hands the work back to one, so it is yours to set only when you
  are running the installer yourself.

A Credential lives wherever the installed Claude Code would put it: the keychain
on macOS, reached by driving `/usr/bin/security`, and a `.credentials.json`
inside the Profile everywhere else — created readable by its owner alone, and
tightened if it is ever found looser. Perch drives `curl` by absolute path —
`/usr/bin/curl`, or `%SystemRoot%\System32\curl.exe` on Windows — to reach
Anthropic, with the URL, the headers and the body all handed over on standard
input: an access token passed as an argument would sit in the process table for
anything on the machine to read.
