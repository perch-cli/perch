# A crate must not cost a seam

Perch hand-rolls a number of things crates exist for, and each of those choices
was made once and then argued for again the next time somebody noticed. This
records the audit so the next person spends their attention on the ones that
are still open.

The rule the individual answers come from: **a crate is taken unless it would
sit on the wrong side of a seam.** Perch has two seams that matter. The Host
port is one — every effect goes through `&dyn Host` so behavior tests drive
real command code against a fake, including a Windows `.cmd` and an unset
`HOME` on whatever machine the tests happen to run on. Fidelity to what Claude
Code actually does is the other: Perch shares files, keychain items and locks
with a program it does not control, and a crate that is *nearly* compatible is
worse than code that is exactly compatible.

## Taken

**Randomized temp names**, in the shape the standard library can give them
rather than through `tempfile`. `write_atomically` and `write_private_file`
both write beside their target and rename, at a name that used to be a fixed
`<path>.perch-tmp`: predictable enough to pre-plant a symlink at, and shared by
two Perch processes writing the same file. The name now carries the process id.
`tempfile::NamedTempFile` would do this and more, but it works on real files
and the fake Host has none — the primitive belongs behind the port, and behind
the port a pid is the whole of what randomness buys here.

**Encryption**, from `age`, for what `perch export` writes (ADR 0014). It sits on
neither seam: encryption is not an effect the Host port carries, and it is not
shared with Claude Code, so nothing here has to be bug-compatible with anything.
The property that decided it is that an `age` file is decrypted by the standard
`age` command — an export is meant to outlive the machine it was written on, and
a backup readable only by the tool that wrote it is a worse backup than one whose
format somebody else maintains. Against that, the alternative was three crates
and a header format Perch would version itself. It is the largest dependency
Perch has by some way, and that is the price of the format rather than of the
code.

**Directory junctions**, from `junction`, on Windows only. A Reconcile shares a
directory into a Run's Profile as a junction, because that is the one directory
link a Windows without Developer Mode can make (ADR 0026) — and a junction is a
reparse point the standard library will not write. The alternative was a
hand-built `REPARSE_DATA_BUFFER` through `DeviceIoControl`, which is precisely
the class of `unsafe` ADR 0021 declined to write when it took `windows-sys`
rather than hand-rolling `GetProcessTimes`. The call sits inside `RealHost`, so
it costs no seam: the fake makes and refuses the same three kinds of link on
whatever machine the tests run on. Symbolic and hard links stay with the
standard library, which carries both on every platform Perch runs on.

## Not taken, and why

**`keyring`** — ADR 0008. macOS anchors a keychain item's ACL to the binary
that created it, so a Perch that created its items would hand Claude Code items
Claude Code cannot read. Perch has to drive the same `/usr/bin/security` Claude
Code does; a keychain crate is the one thing that cannot be substituted here.

**`sysinfo`** — ADR 0021. A process's start time is two fields on three
platforms, and `libc` and `windows-sys` carry vetted declarations of exactly
those. A process-table crate is a great deal of code, and generality, for a
question with one answer.

**`which`** — the program search is `PATHEXT`-aware because npm installs
`claude.cmd` (ADR 0007), and it goes through the Host so a test can pin the
Windows behavior from macOS. A crate that consults the real `PATH` cannot.

**`dirs`** — the home directory is `USERPROFILE` on Windows and `HOME`
elsewhere, and never `HOME` on Windows even when Git Bash sets it. That rule is
three lines and is a Host method, so an unset-home refusal is testable.

**`fd-lock` / `fs4`** — the locks Perch takes are Claude Code's, and Claude
Code's are directories taken with `mkdir` (ADR 0001). A file-lock crate would
take a lock of its own design, which is a lock against nobody.

**`reqwest` / `ureq`** — the request goes to `curl` on stdin so that no access
token is ever an argument (ADR 0021), and shelling out means one TLS story on
the machine rather than two. `std::io::IsTerminal` covers what a
terminal-detection crate would.

**`rpassword`** — reading the export passphrase without the terminal showing it
is `ECHO` off, one line, `ECHO` back on: `tcgetattr`/`tcsetattr` on unix and
`GetConsoleMode`/`SetConsoleMode` on Windows, both of which `libc` and
`windows-sys` already declare. That is the same trade ADR 0021 made when it
declined `sysinfo` for two fields, and it is a Host method either way — so a test
can say the passphrase went in by the path that hides it rather than the one that
echoes, from a machine with no terminal at all.

**A JSON library, for the splicing** — `.claude.json` belongs to the person
using Perch, not to Perch. Parsing and re-emitting it would reorder keys,
reformat numbers and drop the shape they wrote, invisibly. No format-preserving
JSON editor exists in Rust, so `json.rs` finds one value as a span of text and
replaces those bytes. `serde_json` is used everywhere the document *is* Perch's
own.

## Consequences

Perch carries hand-rolled code in seven places on purpose, and each of them has a
reason of the same kind: a crate would either break the Host seam or break
fidelity with Claude Code. A future change that removes one of those reasons —
a `perch tui` that owns its own terminal, say, or a Claude Code that publishes
a contract — reopens the question, and should say so here rather than in a
commit message.

## Reopened by `perch tui`, and settled the same way

Crossterm is now in the tree (ADR 0016), which puts a second implementation of
two things this document declined a crate for within reach. Both stay where
they are.

**Reading the export passphrase** stays `Host::read_secret` over
`tcgetattr`/`SetConsoleMode`, rather than crossterm's raw mode. Not because
crossterm could not turn echo off — it plainly can — but because the reason was
never the primitive: it is a Host method so a test can say the passphrase went
in by the path that hides it, from a machine with no terminal at all. Routing
it through crossterm would move the call, not the seam, and would put the
process-global raw-mode flag under a command that is not drawing anything.

**Terminal detection** stays `std::io::IsTerminal` behind
`Host::is_interactive`, for the same reason and one more: every command asks it
and only one of them draws, so the answer must not depend on a crate the other
twelve are otherwise unaware of.

What crossterm did take over is the thing it is for and there was never
hand-rolled code for: raw mode, the alternate screen, and reading key events —
all of it inside `tui/terminal.rs`, behind `tui::Screen`, which is a seam of the
same shape as the Host port rather than a hole in it.
