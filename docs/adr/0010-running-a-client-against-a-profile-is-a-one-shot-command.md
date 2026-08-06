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
The refusal is read off the command line before the parser sees it: clap would
report an unknown argument, which is true and is not what the person needs to be
told, and it exits with the parser's own code (2) either way.

After `--`, the first word decides what runs, and decides totally: a word
beginning with `-` is an argument for Claude Code, and anything else is the
program to launch with the rest as its arguments. Nothing is guessed, because
nothing beginning with `-` can name a program the operating system would find —
`PATH` is searched for names, a path is written with a `/`, and a file called
`-resume` is reached as `./-resume`. So `perch run dev -- --resume` resumes
Claude Code and `perch run dev -- npm test` runs `npm`, and only Claude Code is
looked for by the probe: a Run of `npm` on a machine with no client installed is
still a Run of `npm`.

This is the only path where profiles are used as live config directories rather
than as storage. It is therefore the only path that has to Reconcile — linking
memory, settings, and plugins into the profile before launch — and the only path
that copies a project entry across profiles (ADR 0003). A Switch needs neither.
