# `perch switch` chooses for you, and the picker is a separate command

Switching accounts happens when quota runs out, which is mid-task and under
mild frustration. The shortest command should therefore do the whole job:
`perch switch` picks an account within the current account's group — by the
group's configured strategy, reading headroom as ADR 0012 defines it — and
switches to it, without asking anything.

`perch switch <target>` names an account, alias, or group explicitly. `perch
tui` opens the interactive view — accounts, their groups, and their utilization
— for when the choice wants making by eye rather than by rule.

The interactive view acts, and acts on exactly two things: switch and run. A
read-only picker is `perch list` with box-drawing characters, so the whole
justification for it is making a choice; confining it to those two keeps this
decision's constraint honest, since both have plain command forms and nothing
becomes TUI-only. `add`, `remove`, `purge` and `config` stay out — a keystroke
away from an irreversible act is the wrong ergonomics for the one surface being
navigated by arrow key.

## Considered Options

Making bare `perch switch` open the picker, with a separate subcommand for the
unattended form, was considered. It shows the evidence before spending quota,
but it costs an interaction at exactly the moment the user wants none, and
anyone rotating between subscriptions would type the unattended form every time.

## Consequences

The interactive view is one command among several rather than the primary
surface, so every capability it offers must exist non-interactively too — Perch
has to be complete over SSH, in scripts, and in CI. That means listing state and
setting group configuration both need plain command forms, not just the TUI.

Like every surface that shows utilization, the interactive view renders from
cache and never blocks on the network to draw its first frame (ADR 0015).

The view lists accounts in the order a cycle ranks them (ADR 0012), group by
group, with the headroom that order was made on beside them — so the ranking
`perch switch` makes is visible rather than hidden, and the two surfaces cannot
come to disagree about which account is better. Where no cycle would happen at
all, no order is shown as one: the accounts in no group are listed as held
rather than ranked until `cycle-ungrouped` says they are interchangeable (ADR
0017), because a ranking of accounts Perch would refuse to choose between is the
hidden claim this listing exists to prevent.

A run is not something the frame loop can take and come back from: it lasts as
long as somebody's session, so the view ends with it, the terminal goes back, and
the client is launched into it (ADR 0010). `perch tui` therefore exits with the
status the client exited with, like `perch run`. A switch is instant and the
picker is still worth looking at afterwards, so that one happens inside the loop
— and is held back while a refresh is out, because a refresh holds Perch's own
lock and a switch waiting on it would freeze the display.
