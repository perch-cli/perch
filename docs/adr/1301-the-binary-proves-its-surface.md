# The binary proves its surface

A suite drives the binary. It is much smaller than that sentence suggests, and
the reason it is small is the whole of what is worth writing down.

## What is left over

`src/main.rs` reads like a blind spot: clap wiring, flag names,
`conflicts_with`, `required_unless_present`, exit-code mapping. Most of it is
already held, and by the file itself. Its own `#[cfg(test)]` module runs under
an ordinary `cargo test` and asserts the exit-code mapping directly — that a
command which worked is nought, that a failure carries its own code, that a
refusal is said on stderr *after* standard output has been flushed, so a
complaint never overtakes the lines it is about. It asserts
`version_asked_for` against the bare question and its near misses. And it holds
an accept-and-refuse table per command: the narrowings that must *not* parse,
`perch holdings export <path> --passphrase hunter2`,
`perch holdings import <path> --group work`.

What is left when those are taken away is three things, and only three:

- **The dispatch arms.** A match arm per command form, each mapping a parsed
  `Command` to a handler and a hand-built args struct. `Command::Disable`
  reaches `enable::run(EnableCommand::Disable)`, and nothing but the reader
  stops it reaching `Enable`. `StatusArgs { group, refresh, json }` carries two
  booleans of the same type, and swapping them compiles.
- **That the process exits with the code.** `ended_as` is asserted as a
  function. That its answer reaches `std::process::exit` and survives to a
  shell is asserted by nothing, because `main` cannot be called from a test.
- **`--help`.** Rendered by nothing else.

That is the residue, and the suite is sized to it.

## An empty ledger here is not an acquittal

No review of `main.rs` has ever found a wiring defect. That reads like the
argument ADR perch-says-what-it-did makes from its own count — the defect class
has no track record, so decline — and it does not carry. That count was of
reviews which *read prose*, so their findings were evidence about prose.
Nothing has ever driven this binary. An empty ledger where nobody has looked is
not evidence, and treating it as one is the reasoning that lets an unlit room
stay unlit because nothing has been seen in it.

So the finding is real, and what it is worth is capped by what it is: a handful
of three-line arms, changing about twice a year, in a file every reviewer of a
new command already opens. That buys an assertion that the surface exists and
routes correctly. It does not buy a second place where Perch's behavior is
proved.

## What the suite reaches, and why it stops there

Everything reachable on a machine with no Claude Code installed, no keychain
consulted and no network answered — which happens to be nearly all of the
residue rather than a compromise. `PERCH_HOME` and `CLAUDE_CONFIG_DIR` are
ordinary user-facing environment variables, so a scratch home costs no
test-only escape hatch and no `#[cfg]`: point them at a temporary directory and
the binary keeps its own state there, exactly as it would for somebody who set
them deliberately. The commands that never ask what Claude Code is installed
carry most of the exit codes worth claiming — `EXIT_NOT_FOUND` for a target
that is not there, `EXIT_INVALID` for a threshold that is not a percentage,
`EXIT_CONFLICT` for a name already spoken for.

The arms it does not reach are the ones that ask `claude --version` first:
`switch`, `add`, `run`, `relogin`, `watch`. Reaching them means a real Claude
Code on the machine running the tests, which means a feature gate, which means
a suite only CI runs — proving behavior that the ungated suites already prove,
at a level where a failure reports an exit code and a blob of stdout instead of
naming the assertion that broke. A stand-in loses to what it stands in for, and
for behavior the thing it would stand in for is the fake-driven suite, which is
faster, hermetic, runs on every platform, and says what it means when it fails.

**So the line is: this suite asserts the surface, never the behavior behind
it.** The operational form, the one to hold against the pressure to extend it:
**if it needs a real Claude Code installed, it has crossed.** The probe is not
an incidental obstacle to be worked around with a fixture or a stub binary on
`PATH`; it is the boundary marker. A command that must be told what Claude Code
is has behavior to prove, and behavior is proved with the fakes.

## The observer may be replaced; the subject may not

The rejected shape is the hermetic end-to-end suite: a scratch `PERCH_HOME`, a
stub on `PERCH_CLAUDE_BIN`, a planted Credential, a recorded transcript instead
of Anthropic. The stub is why it is rejected. A suite that replaces `claude`
with a script printing a version string has rebuilt `FakeHost` at a higher
price and in a worse language.

Read carelessly that forbids something it should allow, so the refinement is
part of the rule. `perch run <target> -- <program>` reaches the probe only
where nothing was named after `--`; name anything else and the Run is a Run in
full, so a test can run `perch run alias -- <perch> --version` and read off the
real filesystem what Reconcile actually linked. What has been replaced there is
who is *watching*, and since what Reconcile does it does to disk, the watcher
can be anything at all.

The question that separates the two: *what would still be proved if the
replacement were perfect?* A flawless fake client proves nothing whatever about
a Renewal. A flawless fake observer changes nothing about the links Reconcile
made, because it never went near them.

## What each level claims

Two places assert the command surface, which is only tolerable if the division
is written down.

**The parser tests in `main.rs` claim what parses and what does not.** A
refusal belongs there: `Cli::try_parse_from(...).is_err()` is a sharper claim
than an exit code of 2 and a clap message that is not Perch's to hold still,
and it does not spend a process to make it.

**The binary suite claims what the process then does** — which arm was reached,
what it printed, what it exited with.

Neither asserts the other's half. A new command's narrowings go in the table in
`main.rs`; its arm goes in the binary suite.

## Consequences

The suite is `tests/invoking.rs`, ungated, so `cargo test` runs it and CI runs
it on macOS, Ubuntu and Windows with everything else. It costs a process spawn
per case against a binary the build has already produced, and the coverage job
instruments the binary target, so its lines count.

`std` and no new dependency: Cargo sets `CARGO_BIN_EXE_perch` at compile time
for every integration test, so `Command::new(env!("CARGO_BIN_EXE_perch"))` is
the built binary with nothing added to the manifest. `assert_cmd` would give
better failure output and is declined — not on dependency-count grounds, since
ADR a-crate-must-not-cost-a-seam takes a crate where it costs no seam and a
test helper costs none, but because `predicates`' combinator style is a second
grammar for writing assertions where ADR perch-says-what-it-did says there is
one. A helper returning the exit code, standard output and standard error says
the same things in the idiom the other test binaries already use.

`main.rs` is not restructured. Making dispatch a pure function so it could be
asserted in-process would introduce a seam to serve a test, and the binary
proves the same arms without one.

Nothing about the fakes changes. `host::fake` remains the only way behavior is
driven, which is the point of refusing the gated suite: the `Host` port's
justification is exactly as strong after this decision as before it.
