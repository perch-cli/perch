# A Linux build is static

There are two Linux Targets, one per architecture, and both are
`-unknown-linux-musl` rather than `-unknown-linux-gnu`.

A `-gnu` build links against the glibc of the machine that built it and refuses
to start on anything older. The machine that built it is a GitHub runner, whose
glibc moves when GitHub moves it, so the set of Linuxes a Release runs on would
be decided by an image upgrade nobody here made and nobody would notice until
somebody on an older distribution reported a binary that would not start. A
static musl build has no such edge: it runs on Alpine, on a long-term-support
distribution years behind, and on whatever the runner is today.

## It costs nothing here specifically

The usual objections to musl do not apply to Perch.

Nothing needs a C toolchain — every dependency is pure Rust, which is why the
build works on a runner with no musl packages installed at all and the
`musl-tools` step is belt and braces rather than a requirement.

Nothing is dynamically loaded. On Linux a Credential lives in a file inside the
Profile (ADR claude-code-chooses-the-store), not in a system keyring, so there
is no libsecret to link against — the one thing that would have made a static
binary genuinely worse.

And musl's allocator, which is the reason performance-sensitive programs avoid
it, is arithmetic Perch never does: it reads a Registry, writes a credential,
and prints a table.

## The consequence worth writing down

There is no `libc` field on the npm platform packages, and it would be wrong to
add one. `libc: ["musl"]` reads as "this binary needs musl", and npm would
refuse to install it on a glibc machine — which is precisely backwards. A static
binary needs neither. The Linux Artifacts are named by architecture alone for
the same reason: there is nothing for a person to choose between.
