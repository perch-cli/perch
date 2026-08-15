# The binary is driven to prove its surface, and never the behaviour behind it

> **Decided, not yet carried out.** The suite described below does not exist
> yet; it is tracked at #155. Like ADR 0041 and ADR 0042, this is the artifact
> of a planning effort rather than of a change, so it lands ahead of the work it
> describes. Read it as a decision that has been taken, not as a report of the
> tree's current shape.

ADR 0041 removed the Dogfood suite, and named one thing that went with it and
did not close: *argv, exit codes and rendered lines do not close at all — nobody
notices a broken `conflicts_with` or an unmapped exit code by using a tool
correctly. But these were never Dogfood's to hold: they need a suite that drives
the binary, and they are decided elsewhere.*

This is elsewhere. A suite drives the binary. It is much smaller than that
sentence suggests, and the reason it is small is the whole of what is worth
writing down.

## What was actually uncovered

The question arrived phrased as though `src/main.rs` were a 795-line blind spot:
clap wiring, flag names, `conflicts_with`, `required_unless_present`, exit-code
mapping, none of it under test except through a suite that has now been deleted.

That is not the shape of it. Of those 795 lines, 244 are a `#[cfg(test)]` module
holding eleven tests that run under an ordinary `cargo test`. They assert the
exit-code mapping directly — that a command which worked is nought, that a
failure carries its own code, that a refusal is said on stderr *after* standard
output has been flushed, so a complaint never overtakes the lines it is about.
They assert `version_asked_for` against the bare question and five near misses.
And they assert accept-and-refuse tables for six commands, each one a list of
the narrowings that must *not* parse: `perch export <path> --passphrase hunter2`,
`perch purge --group work`, `perch tui --json`.

So the flags are held. What is left when those are taken away is three things,
and only three:

- **The dispatch arms.** Twenty match arms, each mapping a parsed `Command` to a
  handler and a hand-built args struct. `Command::Disable` reaches
  `enable::run(EnableCommand::Disable)`, and nothing but the reader stops it
  reaching `Enable`. `StatusArgs { group, refresh, json }` carries two booleans
  of the same type, and swapping them compiles.
- **That the process exits with the code.** `ended_as` is asserted as a
  function. That its answer reaches `std::process::exit` and survives to a
  shell is asserted by nothing, because `main` cannot be called from a test.
- **`--help`.** Never rendered by anything.

That is the residue this ADR weighs. It is a much smaller thing than the
question implied, and the answer is sized to it.

## The ledger is empty, and that is not an acquittal

Twenty-seven commits have touched `main.rs`. Two of them were deep reviews — #37
and #116, eighteen findings between them — and neither found a wiring defect.
#116's only change to `main.rs` was help-text prose.

The temptation is to read that as ADR 0043 read its own count: the defect class
has no track record, so decline. It does not carry, and the difference is worth
being precise about. ADR 0043 could count because the reviews it was counting
*read prose*; their findings were evidence about prose. Nothing has ever driven
this binary. An empty ledger here says nobody has looked, which is not the same
claim at all, and treating it as one would be the reasoning that lets an unlit
room stay unlit because nothing has been seen in it.

So the finding is real. What it is worth is capped by what it is: twenty match
arms of three lines each, changing about twice a year, in a file every reviewer
of a new command already opens. That buys an assertion that the surface exists
and routes correctly. It does not buy a second place where Perch's behaviour is
proved.

## What the suite reaches

Everything reachable on a machine with no Claude Code installed, no keychain
consulted and no network answered.

That is not a compromise; it happens to be nearly all of the residue.
`PERCH_HOME` and `CLAUDE_CONFIG_DIR` are ordinary user-facing environment
variables — `service.rs` carries both into a generated unit file — so a scratch
home costs no test-only escape hatch and no `#[cfg]`. Point them at a temporary
directory and the binary keeps its own state there, exactly as it would for
somebody who set them deliberately.

Within that, the commands that never ask what Claude Code is installed are
`list`, `config`, `group`, `alias`, `enable`/`disable`, and `status` without
`--refresh`. Every one of them is a real dispatch arm reaching a real handler
over real bytes through `RealHost`, and between them they carry most of the
exit codes worth claiming — `EXIT_NOT_FOUND` for a target that is not there,
`EXIT_INVALID` for a threshold that is not a percentage, `EXIT_CONFLICT` for a
name already spoken for. Add `--version`, `--help`, an unknown subcommand and
the parse refusals, and the residue is closed but for the arms that probe.

## Why it stops there

The arms it does not reach are the ones that ask `claude --version` first:
`switch`, `add`, `run`, `relogin`, `watch`. Reaching them means a real Claude
Code installed on the machine running the tests, which means a feature gate
beside `contract`, which means a suite only CI runs.

That is refused, and refused by name, because it is Dogfood.

Not something like Dogfood — the same object. A suite held back by a feature
flag, dependent on what happens to be installed on the machine, proving
behaviour that 24,500 lines of ungated behaviour tests already prove, at a level
where a failure reports an exit code and a blob of stdout instead of naming the
assertion that broke. ADR 0041 removed exactly that shape eleven days ago, and
the argument that removed it does not weaken for the suite being written in a
different file. A stand-in loses to what it stands in for, and for behaviour the
thing it would stand in for is the fake-driven suite, which is faster, hermetic,
runs on every platform, and says what it means when it fails.

**So the line is: this suite asserts the surface, never the behaviour behind
it.** The operational form of that line, the one to hold against the pressure to
extend it, is: **if it needs a real Claude Code installed, it has crossed.** The
probe is not an incidental obstacle to be worked around with a fixture or a stub
binary on `PATH`. It is the boundary marker. A command that must be told what
Claude Code is has behaviour to prove, and behaviour is proved with the fakes.

## How

`std`, and no new dependency. Cargo sets `CARGO_BIN_EXE_perch` at compile time
for every integration test, so `Command::new(env!("CARGO_BIN_EXE_perch"))` is
the built binary with nothing added to the manifest.

`assert_cmd` is the ecosystem's answer and would give better failure output.
It is declined, and not on dependency-count grounds — ADR 0025 takes a crate
where it costs no seam, and a test helper costs none. It is declined on ADR
0043's, which has just finished deciding that an assertion states the claim it
makes. `predicates`' combinator style is a second grammar for writing
assertions, arriving in the same month as the rule that says there is one. A
helper returning the exit code, standard output and standard error says the same
things in the idiom the other thirty test binaries already use.

## What each level claims

There are now two places that assert the command surface, which is only
tolerable if the division is written down.

**The parser tests in `main.rs` claim what parses and what does not.** A
refusal belongs there: `Cli::try_parse_from(...).is_err()` is a sharper claim
than an exit code of 2 and a clap message that is not Perch's to hold still, and
it does not spend a process to make it.

**The binary suite claims what the process then does** — which arm was reached,
what it printed, what it exited with.

Neither asserts the other's half. A new command's narrowings go in the table in
`main.rs`; its arm goes in the binary suite.

## Consequences

The suite is `tests/invoking.rs` — ungated, so `cargo test` runs it, and CI runs
it on macOS, Ubuntu and Windows with everything else. It costs a process spawn
per case against a binary the build has already produced. The coverage job takes
the default feature set and instruments the binary target, so its lines count.

`CONTEXT.md` gains nothing. ADR 0041 is about to empty its **Proving it works**
section, every entry of which describes Dogfood, and an empty section is not a
reason to fill one. Vocabulary is the surface this whole sweep is judged by, and
a test file is not a domain concept — nobody using Perch has to hold it. What
that section becomes is #143's.

`main.rs` is not restructured. Making dispatch a pure function so it could be
asserted in-process would introduce a seam to serve a test, and the binary
proves the same arms without one.

Nothing about the fakes changes. `host::fake` remains the only way behaviour is
driven, which is the point of refusing band (d): the `Host` port's justification
is exactly as strong after this decision as before it.

#143 — whether the harness shape is right — loses its last blocker. What this
settles for it is the level: the fakes keep behaviour, the binary keeps the
surface. It inherits a thirty-first test binary and the naming convention
question that comes with it, and it is free to rename all of them.
