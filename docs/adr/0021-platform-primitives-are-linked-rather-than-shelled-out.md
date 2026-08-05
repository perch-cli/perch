# Platform primitives are linked rather than shelled out

ADR 0008 says Perch drives `/usr/bin/security` rather than linking a keychain
crate, and a reader arriving at `libc` and `windows-sys` in `Cargo.toml` will
reasonably ask what changed. Nothing did: ADR 0008's argument is specific and
does not generalise. macOS anchors a keychain item's access control to the binary
that created it, so linking would make every Perch build a different creator and
turn a silent read into a modal prompt after an upgrade. No such property attaches
to asking whether a process is alive.

Perch needs three primitives its portable standard library does not offer:
whether a process exists, whether there is a terminal, and setting a directory's
modification time. On macOS these were three hand-written `extern "C"`
declarations. Windows has none of them under those names.

`isatty` is deleted outright in favour of `std::io::IsTerminal`, which is stable,
portable, safe, and needs nothing. The other two are taken from `libc` and
`windows-sys` as direct dependencies. Both crates are already compiled in this
build, pulled in transitively by `chrono`, `crossterm` and `ratatui`, so
declaring them adds no crate to the graph — the choice costs nothing and buys
vetted declarations. The alternative was to hand-write the Windows side too:
`CreateFileW` with `FILE_FLAG_BACKUP_SEMANTICS`, `SetFileTime`, `OpenProcess`,
`CloseHandle`. That is precisely the class of `unsafe` whose mistakes nothing
catches until something is corrupted, and its only remaining virtue would have
been consistency with a rule that was never about this.

## Consequences

Setting a directory's modification time on Windows still needs the backup-
semantics flag before the handle can be opened at all, so `touch` remains
platform-split code — just platform-split code over declarations somebody else
audits. The `filetime` crate would package the whole thing and was left out to
avoid a fourth dependency for one function; that is a one-line reversal if the
hand-rolled version proves awkward.

Shelling out remains the rule where ADR 0008's actual argument applies. `security`
is still driven as a subprocess, and so is `curl` — by an absolute path per
platform, because that path is a security property rather than a convenience:
`Command::new("curl")` would let anything earlier on `PATH` receive an
`Authorization: Bearer` header.
