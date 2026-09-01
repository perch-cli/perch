---
title: "Installing"
sidebar:
  order: 1
---

Perch is pre-1.0. Every release is real and works, but the command line may
still change between them, and the changelog marks every change that breaks
something. macOS, Linux and Windows, on both Arm and Intel except Windows,
which is x64 only. Claude Code has to be installed for Perch to have anything
to switch between.

## Homebrew

On macOS or Linux:

```sh
brew tap perch-cli/perch
brew install perch
```

## The installer

On macOS or Linux:

```sh
curl -fsSL https://perch-cli.github.io/perch/install.sh | sh
```

On Windows:

```powershell
irm https://perch-cli.github.io/perch/install.ps1 | iex
```

It offers to add its directory to your user PATH, which belongs to the
Installation rather than to anything Perch holds — so a Purge leaves it. To
remove it, naming your `PERCH_INSTALL_DIR` instead if you set one:

```powershell
[Environment]::SetEnvironmentVariable('Path', ((([Environment]::GetEnvironmentVariable('Path', 'User') -split ';') | Where-Object { $_ -ne "$env:LOCALAPPDATA\Perch\bin" }) -join ';'), 'User')
```

## npm

Anywhere:

```sh
npm install -g perch-cli
```

Perch is pre-1.0 and the version number says so. It is published like anything
else on npm — `latest` is the newest release — because npm has no way for a
package to have no `latest`, and pretending otherwise only meant installing an
older version instead of a newer one.

## By hand

From [the releases page](https://github.com/perch-cli/perch/releases). Every
release carries one archive per platform, a `SHA256SUMS`, and signed build
provenance. The checksums say which bytes; the provenance says which workflow,
in which repository, at which commit produced them, which is the stronger claim:

```sh
gh attestation verify perch-v<version>-<target>.tar.gz --repo perch-cli/perch
```

Both installers check the checksum, and check the provenance too when `gh` is
installed and logged in — and refuse to install if that check fails. Where `gh`
is not there, they say so rather than passing over it quietly, so the line you
read tells you which checks were actually made:

```
perch: checksum ok
perch: provenance not checked — that needs 'gh' installed and logged in. The checksum above is the strongest check made.
```

## Upgrading

```sh
perch upgrade
```

It works out which of the channels above installed this Perch and hands the work
back to that one — `brew upgrade perch` on Homebrew, `npm update -g perch-cli`
on npm — because those binaries belong to Homebrew and npm, and a Perch that
wrote over one would be reverted at the next `brew upgrade` or thrown away at
the next `npm install`. It prints the command before running it. Perch replaces itself only where the
installer script put it — `~/.local/bin`, `%LOCALAPPDATA%\Perch\bin` on
Windows, or `$PERCH_INSTALL_DIR` if you set one — by re-running that same
installer, which is what it did before this command existed.

A binary you unpacked from the releases page by hand belongs to no channel, and
Perch will not write over a file it did not put there. Re-run the installer to
move to a managed installation, or use `--channel homebrew|npm|installer` to
say which one this really is — for a relocated or symlinked binary, say.

To ask without installing anything:

```sh
perch upgrade --check           # what is installed, what is newest, from where
perch upgrade --check --json    # the same, for a script
```

`--check` exits 0 either way, so branch on `upgrade_available` in the JSON rather
than on the exit code.

`--release` takes a particular one, with or without the leading `v`:

```sh
perch upgrade --release v0.2.0
```

Going backwards is allowed and is confirmed first, because a Perch older than
the one that last wrote your registry will refuse to read it — `--yes` says you
have accounted for that. Homebrew installs whatever its formula names and cannot
be pointed at an older release, so `--release` is refused there rather than
quietly ignored; the installer script takes `PERCH_VERSION` if you need to hold
a particular one.

On Windows, an npm installation prints the command instead of running it: npm
would be replacing `perch.exe` while it is the running process, and Windows will
not allow that. Run it from a terminal where Perch is not running. An installer
installation on Windows is fine — it renames the running binary aside, which
leaves a `perch.exe.old` beside it that the next upgrade clears.

### Being told about new releases

`perch version` says what is installed, and adds a line when a newer release
exists. It is the only thing in Perch that looks — `perch status` never touches
the network, and nothing else mentions upgrades. The check happens only when
you are at a terminal, is abandoned after two seconds, and says nothing at all
if it fails, so a machine that is offline or behind a proxy just doesn't get the
line.

To switch it off entirely:

```sh
export PERCH_NO_UPGRADE_CHECK=1
```

That is checked before the request rather than after it, so nothing goes out.

## On macOS, if you download an archive in a browser

Gatekeeper marks it as quarantined, and the binary inside is unsigned. Perch
does not use Apple notarization. macOS will refuse to run it until you clear the
mark:

```sh
xattr -d com.apple.quarantine perch
```

Homebrew, npm and both installers avoid this entirely — none of them set the
flag. Building from source does too.

## From source

Builds and runs on macOS, Linux and Windows, with the same command surface
everywhere. The toolchain is pinned in `rust-toolchain.toml` — Rust 1.97.1,
edition 2024 — so rustup will fetch the right one on first build.

```sh
# builds a release binary and puts it on ~/.cargo/bin, which is on your PATH
# if rustup set it up
cargo install --path .

# the same binary, left at target/release/perch for you to put somewhere
cargo build --release
```

The tests, if you want to run them first:

```
# touches nothing you own: every suite but `your_machine.rs`, which is held
# back by a feature rather than by a list somebody has to maintain
cargo test

# reads and writes state you own — your login keychain, your ~/.claude, the
# Claude Code you have installed — so it wants Claude Code installed
cargo test --features your-machine --test your_machine

# both
cargo test --all-features
```

On macOS `your_machine.rs` reads and writes items of its own in the login
keychain, under `Perch test-*`, and deletes them again. It never writes Claude
Code's item. Set `PERCH_SKIP_KEYCHAIN=1` to skip those where the keychain
cannot be unlocked — they are macOS-only, because only macOS compiles them in.
The rest of the suite needs no opt-out: it touches only temporary directories
of its own.

## Once it is installed

Run `perch status`. The first command you run adopts the Claude Code login
already on the machine as your first Account, and
[Accounts](accounts.md#adopting-the-login-you-already-have) picks up from
there.
