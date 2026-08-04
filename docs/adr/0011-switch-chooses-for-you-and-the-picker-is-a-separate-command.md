# `perch switch` chooses for you, and the picker is a separate command

Switching accounts happens when quota runs out, which is mid-task and under
mild frustration. The shortest command should therefore do the whole job:
`perch switch` picks an account within the current account's group — by the
group's configured strategy, reading headroom as ADR 0012 defines it — and
switches to it, without asking anything.

`perch switch <target>` names an account, alias, or group explicitly. `perch
tui` opens the interactive view — accounts, their groups, and their utilization
— for when the choice wants making by eye rather than by rule.

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
