# Installing

Perch is pre-1.0. Every release is real and works, but the command line may
still change between them, so no channel hands it to you by default — you ask
for it by name. macOS, Linux and Windows, on both Arm and Intel except Windows,
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
gh attestation verify perch-v0.1.0-aarch64-apple-darwin.tar.gz --repo perch-cli/perch
```

Both installers check the checksum, and check the provenance too when `gh` is
installed and logged in — and refuse to install if that check fails.

## On macOS, if you download an archive in a browser

Gatekeeper marks it as quarantined, and the binary inside is unsigned: Perch
skips Apple notarization before 1.0. macOS will refuse to run it until you clear
the mark:

```sh
xattr -d com.apple.quarantine perch
```

Homebrew, npm and both installers avoid this entirely — none of them set the
flag. Building from source does too.

## From source

Builds and runs on macOS, Linux and Windows, with the same command surface
everywhere. The toolchain is pinned in `rust-toolchain.toml` — Rust 1.97.1,
edition 2024 — so rustup will fetch the right one on first build.

```
# touches nothing on the machine: every suite but the contract ones, which are
# held back by a feature rather than by a list somebody has to maintain
cargo test

# asserts beliefs against this machine, so it wants Claude Code installed
cargo test --features contract --test contract --test contract_credentials \
           --test contract_sessions --test contract_links

# both
cargo test --all-features
```

On macOS the contract tests read and write items of their own in the login
keychain, under `Perch contract test-*`, and delete them again. They never
write Claude Code's item. Set `PERCH_SKIP_KEYCHAIN_CONTRACT=1` to skip them
where the keychain cannot be unlocked — it is macOS-only, because only macOS
compiles those tests in. The file-store contract tests need no opt-out: they
touch only a temporary directory of their own.
