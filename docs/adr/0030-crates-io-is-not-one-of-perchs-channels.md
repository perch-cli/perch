# crates.io is not one of Perch's Channels

Perch is written in Rust and is not published to crates.io. `publish = false`
in `Cargo.toml` is what enforces it, and it is there mostly so release-plz does
not try.

The name is taken — `perch` there is a Mastodon and Bluesky client, actively
published — but that is the smaller reason and it would be answered by
publishing as `perch-cli`.

The real one is that crates.io is a Channel for libraries. What it distributes
is source, and what installing from it means is `cargo install`: every user
compiles Perch, which requires a Rust toolchain they may not have and takes
minutes they should not spend. Every other Channel hands them a binary that
already exists. Nothing depends on Perch as a crate — the `[lib]` in
`Cargo.toml` exists so the binary and the tests can share code, not because
anything downstream links against it — so a crates.io entry would advertise an
API nobody consumes and freeze it by advertising it.

`cargo install --git` still works for anyone who wants the source path, which is
the whole of what a crates.io publication would have bought.

## What this decides elsewhere

The `[lib]` has no public API promise, which is why release-plz's semver check
is switched off: it would spend minutes every Release verifying a commitment
nobody is relying on.
