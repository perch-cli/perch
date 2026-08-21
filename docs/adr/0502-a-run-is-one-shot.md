# A Run is one shot

`perch run <target>` launches Claude Code against a chosen Account by setting
`CLAUDE_CONFIG_DIR` for that process alone, leaving the active Account and every
other terminal untouched. `perch run <target> -- <args>` forwards everything
after the separator, so any command can be run under an Account, not just Claude
Code.

A subshell form — spawning `$SHELL` with the environment set, so a terminal
stays pinned to an Account — is refused. It is a second way to do the same thing,
and a mode a user can be in without remembering they entered it. Shell-eval
integration (`eval "$(perch env work)"`) is refused outright: it mutates the
calling shell, needs per-shell installation, and cannot undo itself.

## The separator is mandatory, and the first word after it decides

`perch run dev --resume` is genuinely ambiguous — the flag could belong to either
program, and the argument parser will claim it — so the bare form fails with a
message naming the fix. The refusal is read off the command line before the
parser sees it: clap would report an unknown argument, which is true and is not
what the person needs to be told, and it exits with the parser's own code (2)
either way.

After `--`, the first word decides what runs, and decides totally: a word
beginning with `-` is an argument for Claude Code, and anything else is the
program to launch with the rest as its arguments. Nothing is guessed, because
nothing beginning with `-` can name a program the operating system would find —
`PATH` is searched for names, a path is written with a `/`, and a file called
`-resume` is reached as `./-resume`. So `perch run dev -- --resume` resumes
Claude Code and `perch run dev -- npm test` runs `npm`, and only Claude Code is
looked for by the probe: a Run of `npm` on a machine with no client installed is
still a Run of `npm`.

## A Run makes its Profile Live, and writes that evidence itself

A Profile with a Run against it is a Live Profile, and Perch already refuses to
write into one (ADR a-profile-is-live-by-evidence). What produces the evidence is
the Run: without it, a Run is protected only where Claude Code happens to write a
Marker of its own, and `perch run <target> -- npm test` is protected not at all.

Perch writes `sessions/<pid>.json` into the Profile before the launch and
removes it after, naming **its own** process and recording `startedAt` as the
moment the Run began. Perch waits for the program it launched, so that pid is
alive for exactly as long as the Run, and it is knowable before the launch,
which the child's is not. Naming the child instead is refused on that alone: the
Marker has to exist before the process it protects starts working, or the window
it leaves open is the first thing anybody does in a session.

The Marker corroborates itself by construction. Perch's process began strictly
before Perch wrote the file, so the corroboration check passes while the Run
lives and fails the moment that pid belongs to anybody else. That is what makes
a Run killed rather than exited safe: the Marker outlives the process and says
nothing without it. Closing the terminal is how a session usually ends, so this
is the ordinary path rather than the exception.

A Run that cannot write its Marker is refused rather than launched unguarded.
Everything else a Run cannot do is a remark — a key that did not Carry costs a
dialog — but this one is the whole protection, and a session nothing is
protecting can be Captured or Renewed out from under its author mid-task. A
person told beforehand has lost a command; one told nothing loses their work.

## Consequences

This is the only path where Profiles are used as live config directories rather
than as storage. It is therefore the only path that has to Reconcile — linking
memory, settings and plugins into the Profile before launch — and the only path
that copies a project entry across Profiles
(ADR everything-but-the-account). A Switch needs neither.

`Host` grows one primitive, this process's pid, which is the smallest thing a Run
needs in order to name itself.
