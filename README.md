# perch

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

## Status

Early. The walking skeleton is in: Perch adopts the login you already have and
reports on it. Switching between Accounts is not built yet.

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

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed (ADR 0007) |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account |

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
cargo test --lib --test adoption --test status   # touches nothing on the machine
cargo test --test contract                       # asserts beliefs against this machine
cargo test                                       # both
```

The contract tests read and write items of their own in the login keychain,
under `Perch contract test-*`, and delete them again. They never write Claude
Code's item. Set `PERCH_SKIP_KEYCHAIN_CONTRACT=1` to skip them where the
keychain cannot be unlocked.

## Design

`CONTEXT.md` for the vocabulary, `docs/adr/` for the decisions.
