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

Ratatui and crossterm are in the dependency tree before anything imports them.
Until `perch tui` lands they cost compile time and nothing else — an accepted
price for settling the choice early.
