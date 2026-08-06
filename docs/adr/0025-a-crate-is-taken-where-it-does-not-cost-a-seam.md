# A crate is taken where it does not cost a seam

Perch hand-rolls a number of things crates exist for, and each of those choices
was made once and then argued for again the next time somebody noticed. This
records the audit so the next person spends their attention on the ones that
are still open.

The rule the individual answers come from: **a crate is taken unless it would
sit on the wrong side of a seam.** Perch has two seams that matter. The Host
port is one — every effect goes through `&dyn Host` so behaviour tests drive
real command code against a fake, including a Windows `.cmd` and an unset
`HOME` on whatever machine the tests happen to run on. Fidelity to what Claude
Code actually does is the other: Perch shares files, keychain items and locks
with a program it does not control, and a crate that is *nearly* compatible is
worse than code that is exactly compatible.

## Taken

**Randomised temp names**, in the shape the standard library can give them
rather than through `tempfile`. `write_atomically` and `write_private_file`
both write beside their target and rename, at a name that used to be a fixed
`<path>.perch-tmp`: predictable enough to pre-plant a symlink at, and shared by
two Perch processes writing the same file. The name now carries the process id.
`tempfile::NamedTempFile` would do this and more, but it works on real files
and the fake Host has none — the primitive belongs behind the port, and behind
the port a pid is the whole of what randomness buys here.

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
Windows behaviour from macOS. A crate that consults the real `PATH` cannot.

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

**A JSON library, for the splicing** — `.claude.json` belongs to the person
using Perch, not to Perch. Parsing and re-emitting it would reorder keys,
reformat numbers and drop the shape they wrote, invisibly. No format-preserving
JSON editor exists in Rust, so `json.rs` finds one value as a span of text and
replaces those bytes. `serde_json` is used everywhere the document *is* Perch's
own.

## Consequences

Perch carries hand-rolled code in six places on purpose, and each of them has a
reason of the same kind: a crate would either break the Host seam or break
fidelity with Claude Code. A future change that removes one of those reasons —
a `perch tui` that owns its own terminal, say, or a Claude Code that publishes
a contract — reopens the question, and should say so here rather than in a
commit message.
