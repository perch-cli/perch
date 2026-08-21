# A crate must not cost a seam

Perch is written in Rust because the hot path is `perch status` — a command
people put in a shell prompt or an editor status line, where it runs several
times a minute (ADR a-figure-carries-its-age) — and because distribution then
needs no runtime on the target machine.

That case is narrower than it looks, and the narrowing is worth stating beside
it. Part of it is avoiding interpreter startup, and the credential path spawns a
subprocess anyway. What benefits is `perch status` reading cached Utilization,
which touches no keychain at all; the credential path is not where the language
choice pays off.

Perch then hand-rolls a number of things crates exist for. The rule those
individual answers come from: **a crate is taken unless it would sit on the
wrong side of a seam.** Perch has two seams that matter. The Host port is one —
every effect goes through `&dyn Host` so behavior tests drive real command code
against a fake, including a Windows `.cmd` and an unset `HOME` on whatever
machine the tests happen to run on. Fidelity to what Claude Code actually does
is the other: Perch shares files, keychain items and locks with a program it does
not control, and a crate that is *nearly* compatible is worse than code that is
exactly compatible.

## Taken

**Randomized temp names**, in the shape the standard library can give them
rather than through `tempfile`. `write_atomically` and `write_private_file` both
write beside their target and rename, at a name carrying the process id: a fixed
name is one two Perches writing the same file collide on, and one anybody who
can write the directory can pre-plant a symlink at. `tempfile::NamedTempFile`
would do this and more, but it works on real files and the fake Host has none —
the primitive belongs behind the port, and behind the port a pid is the whole of
what randomness buys here.

**Encryption**, from `age`, for what `perch holdings export` writes
(ADR the-holdings-go-out-sealed). It sits on neither seam: encryption is not an
effect the Host port carries, and it is not shared with Claude Code, so nothing
here has to be bug-compatible with anything. The property that decided it is
that an `age` file is decrypted by the standard `age` command — an Export is
meant to outlive the machine it was written on, and a backup readable only by the
tool that wrote it is a worse backup than one whose format somebody else
maintains. Against that, the alternative is three crates and a header format
Perch would version itself. It is the largest dependency Perch has by some way,
and that is the price of the format rather than of the code.

**Directory junctions**, from `junction`, on Windows only. A Reconcile shares a
directory into a Run's Profile as a junction, because that is the one directory
link a Windows without Developer Mode can make
(ADR everything-but-the-account) — and a junction is a reparse point the
standard library will not write. The alternative is a hand-built
`REPARSE_DATA_BUFFER` through `DeviceIoControl`, which is the class of `unsafe`
this document declines below. The call sits inside `RealHost`, so it costs no
seam: the fake makes and refuses the same three kinds of link on whatever
machine the tests run on. Symbolic and hard links stay with the standard
library, which carries both on every platform Perch runs on.

**Wiping**, from `zeroize`, for the buffers that hold a Credential. It sits on
neither seam and arrives in the build through `age`, so it adds nothing to
audit; and a compiler is entitled to elide a plain overwrite of memory nothing
reads again, which is the whole of what the crate is and why it cannot be
hand-rolled.

**String width**, from `unicode-width`, for the Listing that lays itself out in
columns. The width of a string is a pure function and sits on no seam. A
hand-rolled table of East Asian widths is the same data, kept by hand, going
wrong quietly.

## Not taken, and why

**`keyring`** — the one thing here that cannot be substituted. macOS anchors a
keychain item's access control to the binary that created it, so a Perch that
created its items would hand Claude Code items Claude Code cannot read
(ADR claude-code-chooses-the-store). Perch has to drive the same
`/usr/bin/security` Claude Code does. The idiomatic Rust answer — the `keyring`
or `security-framework` crate, in-process and dependency-free — would make each
build of Perch a different creator.

**`sysinfo`** — a process's start time is two fields on three platforms, and
`libc` and `windows-sys` carry vetted declarations of exactly those. A
process-table crate is a great deal of code, and generality, for a question with
one answer.

**`which`** — the program search is `PATHEXT`-aware because npm installs
`claude.cmd` (ADR an-assumption-is-probed), and it goes through the Host so a
test can pin the Windows behavior from macOS. A crate that consults the real
`PATH` cannot.

**`dirs`** — the home directory is `USERPROFILE` on Windows and `HOME`
elsewhere, and never `HOME` on Windows even when Git Bash sets it. That rule is
three lines and is a Host method, so an unset-home refusal is testable.

**`fd-lock` / `fs4`** — the locks Perch takes are Claude Code's, and Claude
Code's are directories taken with `mkdir`. A file-lock crate would take a lock
of its own design, which is a lock against nobody.

**`reqwest` / `ureq`** — the request goes to `curl` on stdin so that no access
token is ever an argument, and shelling out means one TLS story on the machine
rather than two.

**`rpassword`** — reading the export passphrase without the terminal showing it
is `ECHO` off, one line, `ECHO` back on, over declarations `libc` and
`windows-sys` already carry. It is a Host method either way, so a test can say
the passphrase went in by the path that hides it rather than the one that
echoes, from a machine with no terminal at all.

**`crossterm`, for the two things it could take over.** It is in the tree for
`perch tui`, which puts a second implementation of echo suppression and
terminal detection within reach. Both stay where they are. Routing
`Host::read_secret` through crossterm's raw mode would move the call rather than
the seam, and would put the process-global raw-mode flag under a command that is
not drawing anything. Terminal detection stays `std::io::IsTerminal` behind
`Host::is_interactive` for that reason and one more: every command asks it and
only one of them draws, so the answer must not depend on a crate the others are
unaware of. What crossterm does own is the thing it is for and there was never
hand-rolled code for — raw mode, the alternate screen, and key events, all
inside `tui/terminal.rs` behind `tui::Screen`, which is a seam of the same shape
as the Host port rather than a hole in it.

**A JSON library, for the splicing** — `.claude.json` belongs to the person
using Perch, not to Perch. Parsing and re-emitting it would reorder keys,
reformat numbers and drop the shape they wrote, invisibly. No format-preserving
JSON editor exists in Rust, so `json.rs` finds one value as a span of text and
replaces those bytes. `serde_json` is used everywhere the document *is* Perch's
own.

## Platform primitives are linked rather than shelled out

The refusal above is specific and does not generalize. What anchors a keychain
item's ACL to its creator attaches to nothing else: no such property comes with
asking whether a process is alive.

So the primitives the portable standard library does not offer — whether a
process exists, when it began, a directory's modification time, echo suppression
— are taken from `libc` and `windows-sys` as direct dependencies. Both crates
are already compiled in this build, pulled in transitively, so declaring them
adds no crate to the graph: the choice costs nothing and buys vetted
declarations. The alternative is hand-writing the Windows side too — `CreateFileW`
with `FILE_FLAG_BACKUP_SEMANTICS`, `SetFileTime`, `OpenProcess`, `CloseHandle`
— which is the class of `unsafe` whose mistakes nothing catches until something
is corrupted.

The terminal test is the one that went the other way: `std::io::IsTerminal` is
stable, portable, safe, and needs nothing.

How many primitives there are is deliberately not written down, here or in
`Cargo.toml`. It moves, and a number in a comment that moves is a comment that
is wrong.

Shelling out remains the rule where the ACL argument applies. `security` is
driven as a subprocess, and so is `curl` — by an absolute path per platform,
because that path is a security property rather than a convenience:
`Command::new("curl")` would let anything earlier on `PATH` receive an
`Authorization: Bearer` header.

## A declaration is a dependency

A crate that nothing imports is compile time and audit surface for no call site,
and it is invisible: nothing fails when the last use of one is deleted.

So the *choice* is what is worth settling ahead of time; the *declaration* buys
nothing until there is code behind it. The case this rule was drawn from was a
pair of crates chosen for a surface that had not been built — together the
largest subtree in the dependency graph, compiled on every build and audited on
every advisory, for no call site. `cargo machete` in CI is where the rule has a
running check behind it.

## Consequences

Perch carries hand-rolled code in several places on purpose, and each of them
has a reason of the same kind: a crate would either break the Host seam or break
fidelity with Claude Code. A future change that removes one of those reasons —
a Claude Code that publishes a contract, say — reopens the question, and should
say so here rather than in a commit message.

Setting a directory's modification time on Windows needs the backup-semantics
flag before the handle can be opened at all, so `touch` is platform-split code
— just platform-split code over declarations somebody else audits. The
`filetime` crate would package the whole thing and is left out to avoid a
dependency for one function; that is a one-line reversal if the hand-rolled
version proves awkward.

`&dyn Host` is the only port Perch has. Where an argument for a second one
appears, the test is whether it would have more than one real adapter: one
adapter is a hypothetical seam (ADR code-lives-where-it-reaches).
