# Running a client against a profile is a one-shot command

`perch run <target>` launches Claude Code against a chosen account by setting
`CLAUDE_CONFIG_DIR` for that process alone, leaving the active account and every
other terminal untouched. `perch run <target> -- <args>` forwards everything
after the separator, so any command can be run under an account, not just Claude
Code.

A subshell form — spawning `$SHELL` with the environment set, so a terminal
stays pinned to an account — was considered and dropped. It is a second way to
do the same thing, and a mode a user can be in without remembering they entered
it.

Shell-eval integration (`eval "$(perch env work)"`) was rejected outright: it
mutates the calling shell, needs per-shell installation, and cannot undo itself.

## Consequences

`--` is mandatory before passthrough arguments. `perch run dev --resume` is
genuinely ambiguous — the flag could belong to either program, and the argument
parser will claim it — so the bare form must fail with a message naming the fix.

This is the only path where profiles are used as live config directories rather
than as storage. It is therefore the only path that has to Reconcile — linking
memory, settings, and plugins into the profile before launch — and the only path
that copies a project entry across profiles (ADR 0003). A Switch needs neither.
