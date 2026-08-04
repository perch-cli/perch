# perch

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

## Status

Early. Perch adopts the login you already have, adds further Accounts without
disturbing it, names them, holds Groups of Accounts you have declared
interchangeable, lists what you have, and switches to an Account you name.
Choosing an Account for you — `perch switch` with no target — is not built yet.

```
$ perch status
Adopted the Claude Code login as your first Profile: you@example.com (Acme, pro)
It is now the active Account. Claude Code 2.1.221.

Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

`perch status --json` prints the same information, with an observation time on
every Utilization figure.

Neither form ever touches the network: Utilization is served from cache with its
age shown, so `perch status` is cheap enough to put in a shell prompt
(ADR 0015).

`perch add` gains an Account by running a login in a Profile of its own, so the
Account you are using stays active and its session is untouched (ADR 0009).

## Switching

`perch switch <target>` makes an Account active everywhere — every terminal, the
editor extension, the desktop app — with no login flow.

```
$ perch switch overflow
`overflow` is an Alias for overflow@example.com.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour    12%  (as of 4m ago)
```

It is three steps in one order and never another (ADR 0006). The Credential you
are leaving is **Captured** back into its own Profile first, because Anthropic
retires a refresh token whenever it issues a new one — so the copy in an
Account's Profile is several Rotations behind by the time you switch away, and
skipping the Capture would quietly poison the Account you are leaving. Then the
incoming Credential is written to the Default Profile. Then the `oauthAccount`
block of `.claude.json` is patched to match, and only that block: your project
history, MCP configuration and settings live in the same file and belong to you
rather than to the Account (ADR 0001).

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

## What you have

`perch list` is the one place that answers it: every Account with its Alias, its
Group, whether it is a Cycle candidate, and how full it is.

```
$ perch list
  Account               Alias     Group  State                 Utilization
* someone@example.com   -         work   enabled               5-hour    42%  (as of 3m ago)
                                                               7-day     18%  (as of 3m ago)
  overflow@example.com  overflow  work   enabled, quarantined  never observed
  spare@example.com     -         none   disabled              5-hour    91%  (as of 2h ago)

* is the active Account.
```

An Account nobody has ever read a figure for says `never observed` rather than
`0%` — no figure and plenty of room are opposite pieces of advice. A
Quarantined Account stays listed and named, so an Account needing attention is
never mistaken for one that vanished; whether it is in the Cycling pool is said
alongside, because enabling a Quarantined Account would not repair it.

`perch status --group` is the same view narrowed to the Group the active
Account is in, so you can see where you would land before you switch. From an
Account in no Group it shows every ungrouped Account and says that Cycling will
not move between them until you say it may (ADR 0017).

`perch list --json` and `perch status --group --json` carry the same
information, with an observation time on every figure and the scope they were
narrowed to. Neither makes a network call. `--group` changes the question, so
it changes the document: `perch status --json` answers about one Account under
`active`, while the listings answer about a set under `accounts`, with the
active one named under `active_account`.

## Names

An Alias is a short name for an Account, so no command ever needs an email
address.

```
$ perch alias overflow overflow@example.com
`overflow@example.com` is an Account.
`overflow` now names overflow@example.com.

$ perch alias overflow --unset
`overflow` no longer names overflow@example.com.
```

Aliases and Group names share one namespace: neither can take a name the other
already has, and two names that differ only in case are one name. So the one
Target every command takes is never ambiguous. An Account answers to one Alias
at a time — naming an Account that already has a name replaces it, and says
which name it gave up.

A Target resolves as Alias, then Account email, then Group, and the command
says which one matched before it acts. A Target that matches nothing is refused
with exit code 12 and the names it nearly matched.

## Groups

A Group is your statement that a set of Accounts is interchangeable — another
work subscription, never your personal Account. Cycling will only ever move
between Accounts in one Group (ADR 0002).

```
$ perch group add work
$ perch group move overflow@example.com work
$ perch group list
work
  Accounts     overflow@example.com (as `overflow`)
  Strategy     most-headroom
  Watcher      off (would act at 80%)

In no Group
  Accounts     you@example.com
  Cycling      only moves between these when you say it may
```

`perch group move <target> none` takes an Account out of every Group, and a
Group that still holds Accounts is not removed until they have somewhere to go.
The configuration a Group carries — its strategy and the watcher's fields — is
stored and validated now and consumed by nothing yet; `perch config` will set
it, and the watcher is deferred entirely (ADR 0013).

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed (ADR 0007) |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account, no such Group |
| 13 | it collides with something Perch already holds |
| 14 | Perch understood it and will not accept it — an ambiguous name, a value out of range |
| 15 | there was nothing to do — you are already on that Account |
| 16 | refused: a client is running against that Profile, so its Credential is not Perch's to write |

## Where things are

- `~/.perch/registry.json` — Perch's own state, versioned.
- `~/.perch/profiles/<account>/` — one directory per Account. Its path is what
  gives that Account a private keychain namespace (ADR 0001).
- `$PERCH_HOME` overrides `~/.perch`.

Perch never writes a Credential to a file. Credentials live in the keychain, and
Perch drives `/usr/bin/security` to reach them (ADR 0008).

## Building

Requires macOS. The toolchain is pinned in `rust-toolchain.toml` — Rust 1.97.1,
edition 2024 — so rustup will fetch the right one on first build.

```
# touches nothing on the machine
cargo test --lib --test adoption --test status --test adding --test grouping \
           --test naming --test listing --test switching
# asserts beliefs against this machine
cargo test --test contract
# both
cargo test
```

The contract tests read and write items of their own in the login keychain,
under `Perch contract test-*`, and delete them again. They never write Claude
Code's item. Set `PERCH_SKIP_KEYCHAIN_CONTRACT=1` to skip them where the
keychain cannot be unlocked.

## Design

`CONTEXT.md` for the vocabulary, `docs/adr/` for the decisions.
