# Perch takes back what it wrote

`install.sh` lands the binary in `~/.local/bin` and, when that is not on PATH,
prints the line that would put it there. It writes nothing. `install.ps1` has
the same shape and prints a `SetEnvironmentVariable` call. The question is
whether printing is the right thing on either platform, and the answer is that
it is on Unix and is not on Windows — which is one decision, not two, because
the same criterion decides both.

## The obvious objection is the wrong one

The first argument against writing anything is that a line in `~/.zshrc` would
outlive a Purge, and Purge is defined as giving the machine back. It does not
hold. A Purge already leaves the binary: `purge` takes Profiles, Credentials
and `~/.config/perch`, and has never had an opinion about `~/.local/bin/perch`.
Perch ships no uninstaller on any Channel, and never has — removal is
`brew uninstall`, `npm uninstall -g`, or `rm`, depending on how it arrived.

So the line a Purge draws is not "everything Perch put on this machine". It is
everything Perch **holds**, as against the Installation a Channel left behind.
A PATH entry sits on the Installation side, next to the binary it points at,
and Purge was never going to be the thing that removes it.

That matters because it is the argument someone will reach for first, and
reaching for it hides the one that actually decides this.

## What decides it is whether the writing can be undone

An rc file is a document the user owns and edits by hand. A line Perch appends
lands among lines they wrote themselves, and at removal time nothing can tell
the two apart — "was this line mine?" has no answer that does not involve Perch
leaving a record of its own edits for a later Perch to read back, which is a
great deal of machinery for one PATH entry. Writing there is not reversible in
any sense worth the word.

A Windows user PATH entry is not that. It is a structured registry value, and
removing exactly the segment that was added is an operation that can be
specified precisely and got right every time.

Two smaller asymmetries point the same way. The documented Unix install is
`curl -fsSL ... | sh`, which leaves the pipe on stdin, so `[ -t 0 ]` is false
and a consent prompt could only exist by reaching around the pipe to `/dev/tty`
to find a human the script was not handed. `irm ... | iex` executes in the
session the user is standing in, so a prompt there reaches a real console and
asks honestly. And the remedy being printed is not the same size on both: a
Unix user is handed `export PATH="..."`, while a Windows user gets a
110-character incantation that nobody wants to retype.

## So

**Unix advises.** It names the file for the shell `$SHELL` reports — `.bashrc`,
`.zshrc`, `config.fish` with `fish_add_path`, since fish cannot use an `export`
line — and falls back to the bare POSIX line for anything it does not
recognize. Where it created `~/.local/bin` itself and the user's `~/.profile`
already carries the Debian conditional, it says to start a new login shell
rather than handing out a second export, because on that machine the guard was
false at login and is the right answer now.

**Windows writes, with consent.** It prompts when `[Environment]::UserInteractive`
and prints the command otherwise, so a scripted install stays inert. The install
guide carries the one line that takes the entry back, because this is the first thing
Perch writes that outlives its own binary and leaving no documented way back is
the outcome least worth having.

An uninstaller was the alternative to that last point and was not taken. Perch
has no uninstaller for any other Channel, and ADR an-upgrade-asks-its-channel
already refused to build machinery for a problem nobody has reported — the half
of it that still stands, and the half ADR an-upgrade-asks-its-channel leaned on
when it decided that replacing an Installation belongs to the Channel that made
it for the same reason removing one does.

## What would reopen it

Somebody whose `~/.zshrc` Perch would have fixed, who gave up at the warning.
The Unix half of this is a bet that naming the right file is enough, and a
report that it is not is the thing that would move it.
