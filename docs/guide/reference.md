# Reference

- [Commands](#commands)
- [Exit codes](#exit-codes)
- [Where things are](#where-things-are)

## Commands

| Command | What it does |
| ------- | ------------ |
| `perch status [--group] [--refresh] [--json]` | the active Account and how full it is |
| `perch list [--json]` | every Account, its Alias, Group, state and Utilization |
| `perch add [--group <name>\|--no-group] [--alias <name>]` | gain an Account by logging in, without disturbing the active one |
| `perch alias <name> <target>` / `perch alias <name> --unset` | name an Account, or free the name |
| `perch switch <target>` | make an Account active everywhere |
| `perch switch [<group>]` | Cycle to the best Account in a Group |
| `perch watch [--once]` | Cycle automatically when the Account you are on runs low |
| `perch run <target> [-- <command>]` | launch Claude Code as an Account, in this terminal alone |
| `perch tui` | the interactive view |
| `perch group add\|move\|rename\|remove\|list` | declare Groups and move Accounts between them |
| `perch config set\|unset\|get` | the rules Perch chooses Accounts by |
| `perch disable <target>` / `perch enable <target>` | keep an Account out of Cycling, or put it back |
| `perch relogin <target>` | repair an Account whose Credential stopped working |
| `perch remove <target> [--yes]` | give up an Account |
| `perch export <path>` / `perch import <path>` | write everything to one encrypted file, and put it back |
| `perch purge [--yes]` | give the machine back the state it had before Perch |
| `perch upgrade [--release <tag>] [--check] [--json] [--channel <name>] [--yes]` | replace this Perch with a newer Release, through the Channel that installed it |

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed (ADR 0007) |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account, no such Group |
| 13 | it collides with something that is already there — an Account added twice, a name already spoken for, a path an Export would have written over, an Import onto a Perch that already holds an Account |
| 14 | Perch understood it and will not accept it — an ambiguous name, a value out of range, a Group that has not said the watcher may act on it, a command that needs a terminal run where there is none |
| 15 | there was nothing to do — you are already on that Account, a check found nothing to do now, or Perch is holding nothing on this machine to purge |
| 16 | refused: a client is running against that Profile, so what is in it is not Perch's to write or to delete |
| 17 | a Cycle found nowhere to land — every Account in the Group is exhausted, or none is a candidate |
| 18 | a bare Cycle, or a watcher, on an Account nobody has declared interchangeable with anything (ADR 0017) |
| 19 | that Account is Quarantined — its Credential no longer works, and `perch relogin` repairs it (ADR 0023) |
| 20 | held: a lock somebody else has, or a `perch watch --once` with no current figure to decide on (ADR 0013). Nothing is wrong and nothing was changed — ask again shortly |

`perch run` is the one command these do not describe once it has launched
something: what the client exited with is what Perch exits with, so a script
wrapping it reads the program's own code rather than Perch's. Everything that
stops a Run before the launch — a command line without `--`, an unknown Target,
a Group, a Quarantine, a Reconcile that could not be made — is in the table
above.

`perch upgrade` is the same once it has handed the work to a Channel: what
`brew` or `npm` exited with is what Perch exits with. `perch upgrade --check`
exits 0 whether or not there is a newer Release — it is a question, and
answering it is success either way, so branch on `--json`'s `upgradeAvailable`
rather than on the code.

## Where things are

- `~/.config/perch/registry.json` — Perch's own state, versioned.
- `~/.config/perch/profiles/<account>/` — one directory per Account. Its path is what
  gives that Account a private Credential Store (ADR 0001).
- `$PERCH_HOME` overrides `~/.config/perch`. Home is `$USERPROFILE` on Windows and
  `$HOME` elsewhere; a machine that cannot say where home is gets a refusal,
  never a write into the filesystem root. `~/.config` is created if it is not
  there, and the same path is used on every platform, Windows included, rather
  than `%APPDATA%` — one rule to document and to support, and `$PERCH_HOME` for
  anybody who wants a different one.
- `$PERCH_CLAUDE_BIN` overrides where `claude` is found. Without it, Perch
  walks `PATH` itself — consulting `PATHEXT` on Windows, so the `claude.cmd`
  an npm install leaves works from every shell.
- `$PERCH_NO_UPGRADE_CHECK` stops `perch --version` asking whether a newer
  Release exists. Checked before the request, so nothing goes out (ADR 0039).
  That check is the only place Perch looks for its own updates; `perch status`
  never touches the network.

A Credential lives wherever the installed Claude Code would put it (ADR 0020):
the keychain on macOS, reached by driving `/usr/bin/security` (ADR 0008), and
a `.credentials.json` inside the Profile everywhere else — created readable by
its owner alone, and tightened if it is ever found looser. Perch drives `curl`
by absolute path — `/usr/bin/curl`, or `%SystemRoot%\System32\curl.exe` on
Windows — to reach Anthropic, with the URL, the headers and the body all
handed over on standard input: an access token passed as an argument would sit
in the process table for anything on the machine to read.
