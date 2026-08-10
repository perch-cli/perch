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

## Amended: the declaration is back, and the terminal is a seam of its own

`perch tui` exists, so the code the declaration was waiting for is here.
Ratatui 0.30 and crossterm 0.29 are in `Cargo.toml` — crossterm at the version
`ratatui-crossterm` resolves to, which is the whole point of naming it, and
ratatui with its default features off: the calendar and the rest are widgets
Perch does not draw, and a binary somebody downloads pays for them.

The part that was not settled ahead of time is where the terminal sits. It is
an effect outside the process, which is the Host port's whole subject, and it
is deliberately not a Host method. A `Host` that knew about frames would be one
every non-TUI test carried and every fake had to answer for, and what is on the
far side of the call is not a primitive like `mkdir` — it is ratatui's `Frame`.
So `perch tui` owns its terminal through a seam of its own, `tui::Screen`, with
two methods: draw this model, and wait this long for a keystroke. The fake
draws into a buffer and hands back a scripted keystroke, so the real frame loop
is driven with no terminal at all — the same bargain the Host port makes, made
locally.

A Refresh is the other thing the frame loop must not do itself, and it goes to
a thread with a `RealHost` of its own. That Host keeps its remarks rather than
printing them: `Host::note` goes to stderr, which is exactly where the frames
are, so a note about the machine would land in the middle of one. The loop
shows them where it shows everything else a Refresh could not do.

The rule is the Host's rather than that thread's. The Switch the picker performs
runs against the *process's* Host, so a remark it provoked — a Credential
written to a store Perch would rather not have used — was printed onto the
alternate screen and thrown away with it. `Host::print_remarks` is the one thing
about frames the port does know, and it knows it only as "keep these for now":
`perch tui` turns printing off for exactly as long as it holds the screen, reads
what was kept, and shows it in the frame beside what the command said. A fake
never printed a remark in the first place, which is why nothing here was
testable until the real Host could be told to behave the same way.
