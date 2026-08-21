# perch

[![CI](https://img.shields.io/github/actions/workflow/status/perch-cli/perch/ci.yml?branch=main&label=CI)](https://github.com/perch-cli/perch/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/codecov/c/github/perch-cli/perch?label=coverage)](https://codecov.io/gh/perch-cli/perch)
[![Latest release](https://img.shields.io/github/v/release/perch-cli/perch?label=release)](https://github.com/perch-cli/perch/releases/latest)
[![npm](https://img.shields.io/npm/v/perch-cli?label=npm)](https://www.npmjs.com/package/perch-cli)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue)](pages/src/content/docs/installing.md)
[![Rust](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fperch-cli%2Fperch%2Fmain%2Frust-toolchain.toml&query=%24.toolchain.channel&label=rust&prefix=v)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

Perch is for one person moving between logins they already hold — their own
accounts, on their own machine. It creates no accounts and authenticates nobody;
it only chooses between logins you have already made yourself.

```
$ perch switch
Switched to overflow@example.com, the most room in Group `work`.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
```

## Install

Perch is pre-1.0. Every release is real and works, but the command line may
still change between them, and the changelog marks every change that breaks
something. macOS, Linux and Windows. Claude Code has to be installed
already, for Perch to have anything to switch between.

```sh
brew tap perch-cli/perch && brew install perch      # Homebrew, on macOS or Linux

curl -fsSL https://perch-cli.github.io/perch/install.sh | sh    # the installer

npm install -g perch-cli                                        # npm
```

On Windows, `irm https://perch-cli.github.io/perch/install.ps1 | iex`.

Installing by hand, verifying a release's checksum and build provenance, the
macOS quarantine flag and building from source are all in
**[the install guide](pages/src/content/docs/installing.md)**.

## Getting started

**1. See where you are.** The first command you run adopts the login already on
the machine, so nothing has to be logged into again.

```
$ perch status
Adopted the Claude Code login as your first Profile: you@example.com (Acme, pro)
It is now the active Account. Claude Code 2.1.221.

Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

**2. Add another Account.** The login runs in a Profile of its own, so the
Account you are using stays active and its session is untouched. `--group` says
which Accounts this one is interchangeable with, and `--alias` saves you typing
an email address ever again.

```
$ perch add --group work --alias overflow
```

**3. Switch to it.** Everywhere at once — every terminal, the editor extension,
the desktop app — with no login flow. Your memory, settings, plugins and project
history do not move: they are yours rather than the Account's.

```
$ perch switch overflow
```

**4. Or let Perch choose.** With no target, `switch` Cycles within the Group the
current Account is in, taking the Account with the most room left. It asks
nothing, because it is what you type mid-task when quota just ran out.

```
$ perch switch
```

**5. Stop being the one who notices.** `perch watcher run` reads how full the
Account you are on is, prints what it made of that, and Cycles when it runs low.
Ctrl-C is safe wherever it lands. Nothing changes underneath you until you say it
may:

```
$ perch config set work watcher-may-act true
$ perch watcher run
```

`perch watcher install` has your machine run that same loop for you, starting
when you log in — a LaunchAgent, a `systemd --user` unit, or a Scheduled Task,
whichever your machine has. Perch never backgrounds itself: it writes the unit
and hands the job over, and `perch watcher uninstall` takes it back.

One more worth knowing early: `perch run <target>` launches Claude Code as one
Account in one terminal without changing which is active.

## Commands

| Command | What it does | More |
| ------- | ------------ | ---- |
| `perch status` | the active Account and how full it is | [guide](pages/src/content/docs/status.md) |
| `perch list` | every Account — or one Scope of them — with its Alias, Group, state, Headroom and Utilization, ranked as a Cycle would | [guide](pages/src/content/docs/status.md#every-account) |
| `perch add` | gain an Account by logging in, without disturbing the active one | [guide](pages/src/content/docs/accounts.md#adding-an-account) |
| `perch alias` | name an Account, so no command needs its email address | [guide](pages/src/content/docs/accounts.md#naming-an-account) |
| `perch switch` | make an Account active everywhere, or Cycle within a Group | [guide](pages/src/content/docs/switching.md) |
| `perch watcher` | Cycle automatically when the Account you are on runs low, in a terminal or as a Service | [guide](pages/src/content/docs/watching.md) |
| `perch run` | launch Claude Code as an Account, in this terminal alone | [guide](pages/src/content/docs/running.md) |
| `perch group` | declare which Accounts are interchangeable | [guide](pages/src/content/docs/switching.md#managing-groups) |
| `perch config` | the rules Perch chooses Accounts by | [guide](pages/src/content/docs/configuration.md) |
| `perch disable` / `enable` | keep an Account out of Cycling, or put it back | [guide](pages/src/content/docs/accounts.md#keeping-an-account-out-of-cycling) |
| `perch relogin` | repair an Account whose Credential stopped working | [guide](pages/src/content/docs/accounts.md#when-an-account-breaks) |
| `perch remove` | give up an Account | [guide](pages/src/content/docs/accounts.md#giving-up-an-account) |
| `perch holdings export` / `import` | back up everything Perch holds to one encrypted file, and put it back | [guide](pages/src/content/docs/backup.md) |
| `perch holdings purge` | give the machine back the state it had before Perch | [guide](pages/src/content/docs/backup.md#giving-the-machine-back) |
| `perch upgrade` | replace this Perch with a newer Release, through the channel that installed it | [guide](pages/src/content/docs/installing.md#upgrading) |

Every command has `--help`, and the flags, the exit codes and the paths Perch
writes are in the [reference](pages/src/content/docs/reference.md).

## How it thinks

A few things are worth knowing before the details, because most of Perch follows
from them:

- **Utilization is served from cache**, with the age of every figure shown.
  `--refresh` is the one thing that fetches, so both `perch status` and
  `perch list` are cheap enough to sit in a shell prompt
  (ADR a-figure-carries-its-age).
- **A Group is a declaration that Accounts are interchangeable.** Cycling never
  leaves the Group it started in, so a work subscription running dry does not
  land you on your personal Account (ADR a-group-is-a-declaration).
- **An Account is ranked by its worst Quota Window.** Being blocked by any
  window blocks you completely, so that is the only ranking that measures what
  actually stops you working — and there is no pooled total, anywhere
  (ADR headroom-is-the-worst-window).
- **The Credential you leave is Captured first.** Anthropic retires a refresh
  token whenever it issues a new one, so a Switch that skipped this would
  quietly poison the Account you were leaving
  (ADR a-switch-is-written-down-first).
- **Nothing is written into a Profile a client is running against.** Renewing a
  Credential Claude Code is holding in memory would log that session out
  mid-task (ADR a-profile-is-live-by-evidence).
- **An Account that breaks is Quarantined, never dropped.** It stays listed and
  named with the reason, and `perch relogin` repairs it in place
  (ADR a-broken-account-is-repaired).
- **Nothing is interactive.** Every command reads its arguments, does its work
  and exits, so Perch is complete over SSH, in scripts and in CI — and the
  ranking a Cycle makes is shown by `perch list` rather than drawn
  (ADR perch-does-not-draw).

## Guides

[The guide](pages/src/content/docs/) is the whole of what each command does, and
why it does it that way. The same pages, rendered and searchable, are at
[perch-cli.github.io/perch](https://perch-cli.github.io/perch/):

- [Installing](pages/src/content/docs/installing.md)
- [Accounts](pages/src/content/docs/accounts.md) — adding, naming, keeping out of
  Cycling, repairing, giving up
- [Seeing what you have](pages/src/content/docs/status.md) — `status`, `list`,
  Utilization, the JSON
- [Switching, Cycling and Groups](pages/src/content/docs/switching.md)
- [Watching](pages/src/content/docs/watching.md) — the loop,
  `perch watcher install`, and `perch watcher check` under cron
- [Running one Account in one terminal](pages/src/content/docs/running.md)
- [Backing up and moving machines](pages/src/content/docs/backup.md)
- [Configuration](pages/src/content/docs/configuration.md)
- [Reference](pages/src/content/docs/reference.md) — commands, exit codes, paths

## Design

[`CONTEXT.md`](CONTEXT.md) for the vocabulary, [`docs/adr/`](docs/adr/) for the
decisions.

## License

Two of them, at your option, which is the Rust ecosystem's convention and not an
accident. MIT is the shorter and better-known of the pair but says nothing at all
about patents; Apache-2.0 grants them expressly, and cannot be combined with
GPLv2. Offering both leaves that choice where it belongs, with whoever is
downstream.

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
