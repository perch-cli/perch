# crates.io is not one of Perch's Channels

Perch is written in Rust and is not published to crates.io. `publish = false`
in `release-plz.toml` is what enforces it.

Not in `Cargo.toml`, which is where it was and where it did not work. release-plz
reads that field as "this package is not one of mine" and skips the package
entirely — no version decision, no changelog, no release pull request, and no
error saying why. The workflow went green on every push to `main` and quietly
did nothing, which is how v0.1.0 came to be tagged by hand.

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

Nothing now stops a person typing `cargo publish` by hand, where `Cargo.toml`
used to. That is accepted rather than worked around: the name on crates.io is
taken by somebody else's live crate, so the command fails on its own, and no
automation runs it.
