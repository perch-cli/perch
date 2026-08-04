# perch

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

## Status

Early. Perch adopts the login you already have, adds further Accounts without
disturbing it, and holds Groups of Accounts you have declared interchangeable.
Switching between Accounts is not built yet.

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
cargo test --lib --test adoption --test status --test adding --test grouping
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
