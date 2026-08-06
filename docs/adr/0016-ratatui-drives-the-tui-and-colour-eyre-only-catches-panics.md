# Ratatui drives the TUI, and colour-eyre only catches panics

Three crates are settled ahead of the code that uses them, so the choice is
recorded rather than rediscovered.

`perch tui` (ADR 0011) is built on **ratatui** over **crossterm**. Ratatui
depends on crossterm itself, so naming crossterm directly keeps one version in
the tree rather than two — Perch and `ratatui-crossterm` resolve to the same
one, which matters because a terminal in raw mode is process-global state and
two crossterm versions would each think they owned it. Both are in
`Cargo.toml` from now, before `perch tui` exists, so the dependency set is
settled while the surface is still small.

**colour-eyre** is installed for its panic hook and nothing else. Perch's own
failures are typed: `PerchError` carries the exit code a script reads — 10 for
a probe refusal, 11 for a locked keychain, 12 for a target that is not there —
and the compiler checks that every variant has one. Routing those through a
type-erased `Report` would trade that check for a downcast, and a coloured
backtrace is the wrong thing to print at a command people put in a shell
prompt. A panic is a different animal: it is a bug rather than an outcome, and
a bug deserves a report worth pasting.

## Consequences

Perch carries two error idioms on purpose. Expected failures are `PerchError`
and exit codes; unexpected ones are panics with a colour-eyre report. Anything
that starts as a panic and turns out to be an outcome a user can act on should
move across, not stay.

## Amended: a declaration waits for the code that uses it

Two changes, one reason.

Ratatui and crossterm were declared from the start, before anything imported
them, so that the dependency set was settled while the surface was small. That
was the wrong half of the decision to act on early.

The *choice* is what is worth settling ahead of time, and this document is
where it is settled — including the reason crossterm is named directly rather
than left to ratatui, which is the part that would have been rediscovered. The
*declaration* buys nothing until there is code behind it: together they were
the largest subtree in the dependency graph, compiled on every build and
audited on every advisory, for no call site. They come back with `perch tui`,
at the versions named above.

Colour-eyre is the same shape of cost with the code already written. Its whole
use is a panic hook — `report.rs` installs that and explicitly discards the
error hook — and it brings eyre, backtrace, owo-colors and indenter along for
it. What a bug report actually needs is the version, the platform, where to
send it, and how to get a backtrace; the runtime's own hook already prints the
payload, the location and the backtrace when one is asked for. So Perch's hook
now sits on top of the runtime's and adds those four things in a dozen lines,
and says how to re-run with `RUST_BACKTRACE=1` — which colour-eyre's prettier
report never did.

What is given up is the span-formatted backtrace, which is genuinely nicer to
read. It is not nicer than not shipping four crates for it, and the choice is
recorded here so it is a decision rather than a drift.
