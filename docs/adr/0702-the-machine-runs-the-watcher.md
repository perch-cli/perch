# The machine runs the Watcher

`perch watcher install` writes a unit the machine's own service manager owns,
starts it, and from then on the Watcher comes up when you log in.
`perch watcher uninstall` takes it back, and `perch watcher status` says what is
there. The difference this buys is between a rotation that works because you
remembered to type something and one that works because the machine came up.

Perch does not fork, does not double-fork, writes no PID file, and never puts
itself in the background. The operating system starts the process and the
operating system stops it; Perch's contribution is a unit file and the honesty
to remove it again. Scheduling and supervision are the operating system's job,
and a Service is Perch agreeing with that rather than an exception to it.

What it costs is service lifecycle management on three platforms, and that is
the whole of what this record decides to pay. It buys no new hazard: a
`perch watcher check` under cron already mutates Credentials with nobody
watching, by the same mechanism and with the same hazards, so a Service adds a
way of arranging something that was already permitted.

## A supervised loop, not a scheduled Check

The obvious implementation is a timer running `perch watcher check` every two
and a half minutes. It needs no new code: the Check exists, its exit codes were
designed for a scheduler, and its Cooldown already survives between invocations
in the registry.

It is the wrong one, and the reason is the Back-off. A Back-off is memory the
loop keeps — it doubles when a Refresh cannot be read and the first Refresh that
works drops the whole of it (ADR a-watcher-knob-is-arithmetic). A Check has no
memory to keep it in, so a sequence of Checks has no Back-off at all: a
fixed-interval timer asks a refusing endpoint twenty-four times an hour, for
ever. Persisting it would work and would be wrong twice over — it would put
Anthropic's allowance in the registry beside the Group's Settings as if it were
one of them, and it would give the loop a file to write when the whole of the
Cooldown's reasoning is that a loop's pacing belongs to the loop.

So the Service runs the loop, unchanged. The supervisor's only job is to start it
and to keep it started; every question about when to ask, when to act and when to
wait is answered by the same code that answers it in a terminal. There is one
Watcher with three ways of being run, rather than two Watchers that have to be
kept in agreement.

The cost is named rather than hidden: a supervised loop spends the full
twenty-four reads an hour whether or not anybody is working. A timer could be
sparser, but the interval is derived from Anthropic's allowance rather than from
anyone's preference, so "poll less when idle" is not a knob this design has to
offer, and inventing one would mean inventing a definition of idle.

## It starts at login, and it is yours rather than the machine's

A LaunchAgent on macOS, a `systemd --user` unit on Linux, a Scheduled Task that
runs on logon on Windows. Never a LaunchDaemon, never a system unit, and
`install` refuses when it is run as root.

Every Profile Perch holds is under one person's home directory, and the registry
that says which Account is active is that person's. A Watcher started before
anybody logs in has no home to read and nothing to decide about; installed under
`sudo` it would watch root's registry, which is empty, while the person who
typed it wondered why nothing ever Switched. On macOS it would also have no
keychain, because there is no unlocked one before login
(ADR claude-code-chooses-the-store) — but the home directory argument is every
platform's and is sufficient on its own.

"Every time the machine starts up" is therefore delivered as "every time you log
in", and the guide says so in those words. Somebody who genuinely wants it on a
headless box before login wants `loginctl enable-linger`, which is theirs to
enable and not something an `install` should do quietly.

## Exactly one Watcher

Two loops for one person watch the same active Account and each keeps its own
Cooldown in memory, where neither can see the other's. That is the ping-pong the
Cooldown and the Margin exist to prevent, reintroduced by running the thing
twice.

So a Watcher takes a lock in Perch's home and holds it for as long as it runs,
renewing it as it goes. It is the one artifact a Watcher leaves behind, and it is
given back however the process ends.

A **Check** that cannot take it exits `20`, the code for a contended lock, which
ADR a-refusal-is-a-promise already defines as a promise that nothing was changed.
It takes the same lock as the loop because the in-memory Cooldown and the
recorded one cannot see each other — a Check firing while the Service runs is the
same double-Switch by a different route. Somebody running both a cron Check and a
Service gets a mailbox of 20s, which is true and is the right nudge to pick one.

A **loop** that cannot take it holds and comes back. A Service that exited here
would be one the supervisor restarts into the same exit, and the lock it is
waiting on may be a stale one left by a `kill -9`. The staleness window is long —
the longest Back-off plus a round, because that is the longest a healthy Watcher
can go without renewing — and it is *affordable* only because nothing exits on
it: holding turns "this machine is unusable for twenty minutes" into some held
rounds that say why. That trade is what lets the window be derived from the
loop's own arithmetic rather than picked to keep a crash loop short.

## Permission holds the loop rather than stopping it

A machine that is not arranged for watching holds the loop: no active Account,
an ungrouped one nobody has declared interchangeable, a Scope that has not said
the Watcher may act, nothing to adopt yet. Under a supervisor an exit there is a
crash loop — `Restart=always` respawns into the same exit until systemd's start
limit leaves the unit failed and needing a manual `reset-failed`, and launchd's
`KeepAlive` respawns every ten seconds indefinitely. It cannot be fixed per
platform either: systemd can be told `SuccessExitStatus=14 18`, and launchd
cannot express it at all, because its `KeepAlive` knows only zero from non-zero.

Nothing about the safety of the grant changes, because a holding Watcher acts on
nothing: it does not Refresh, it does not rank, it does not Switch. Stopping
*acting* and stopping *existing* are different things, and the second is only
right for a loop somebody is sitting in front of — a Service that exits because
you have not granted permission yet is a Service you have to remember to start
again after you do, and one whose permission was withdrawn on Tuesday is
watching again the moment it is granted on Wednesday.

A permission hold reads the registry rather than the network, so it costs
nothing and needs no Back-off. It is still re-read every round rather than only
at the first, and it re-checks on the ordinary interval rather than a faster one
of its own: granting permission and waiting up to two and a half minutes is not
a cost worth a second cadence to explain.

`perch watcher check` keeps `14` and `18`. A Check is one process reporting to a
scheduler, and a scheduler needs to be told that this machine is not arranged for
what it was asked to do.

## Identical holds are said once, not every round

A Service writes to a log nobody reads until something is wrong, and a permission
hold repeats until somebody changes a Setting — possibly for weeks. At one line
every two and a half minutes that is five hundred and seventy-six identical
lines a day, and what they bury is the one line that matters.

So consecutive holds with the same reason are said in full on the way in, once an
hour while nothing changes, and always on the way out. The hourly line carries
how long it has been holding, which is the proof of life repetition was standing
in for and a better one: a Watcher that has been held since 09:14 is obviously
stuck in a way that the same sentence four hundred times is not. A hold that has
changed either its reason or the cadence it promises has changed, and is said
again.

## A Service that cannot start gives up rather than restarting for ever

Everything a Watcher can hold on, it holds on, so a Service that will not
*start* is a machine somebody has to look at: a registry that will not parse, a
Claude Code that cannot be probed, a home directory that has gone. Restarting
into that for ever is a loop nobody ever sees, because the only place it is
visible is a log nobody is reading.

systemd is therefore given a start limit derived from the restart interval —
five tries inside five minutes, against a restart of thirty seconds — because
its own defaults of five starts inside ten seconds could never trip against
that, and `systemctl --user status` then says `failed` and why. launchd has no
equivalent and gets a throttle interval alone, which bounds how fast it retries
but not how long. That is a real difference between the two platforms rather
than something worth emulating with a wrapper.

## Standard output is still the only sink

Perch writes the decision line to standard output and gains no logging
subsystem, no rotation, no levels and no configuration. The unit file can route
that better than Perch can.

On Linux systemd captures it into the journal, which rotates it, retains it and
makes `journalctl --user -u perch-watch -f` work without Perch knowing the word
journal. On macOS launchd is pointed at `$PERCH_HOME/watch.log`, and on Windows
the task redirects to the same path, appending rather than truncating so a log
survives a logout. Both streams go to the one file, because a refusal that
landed in a second file would be the line missing from the sequence somebody is
reading.

Those two files grow without bound. At one line every two and a half minutes
that is of the order of twenty megabytes a year, which is a real cost and a
smaller one than the code that would cap it. If somebody's log becomes a
problem, that is the reason to write the rotation, and not before. Putting the
file inside `$PERCH_HOME` is what makes it Perch's to remove: it is swept by a
Purge with everything else Perch holds.

## What the unit records, and why it is not what you would guess

A unit file names an absolute path and is read by a service manager with almost
no environment, so three things are captured at install time.

The **binary**: not `argv[0]`, and not always the fully resolved target either.
Under npm the thing on `PATH` is a JavaScript shim that execs a platform
package, and it needs `node` on a `PATH` a `systemd --user` unit does not have —
but the resolved path is already the platform binary, so resolving is right
there. Under Homebrew resolving is exactly wrong: the target is a
version-stamped path inside the Cellar, so a unit pointing at it breaks on the
next `brew upgrade`, while the symlink in `bin` is stable across every Release.
The rule is the most stable path that runs without a shell, which is a different
answer per Channel (ADR an-upgrade-asks-its-channel) rather than a single call
to `canonicalize`.

The **environment**: `PERCH_HOME` and `CLAUDE_CONFIG_DIR` are read from the
process environment and are typically set in a shell profile no service manager
will ever source. A Service silently watching `~/.config/perch` while its owner
works out of `PERCH_HOME=~/work/perch` would be reporting, correctly and
uselessly, that there is nothing to do. Both are written into the unit when — and
only when — they are actually set, and nothing else from the shell is: a unit
that captured the whole environment would bake a `PATH`, an `SSH_AUTH_SOCK` and
whatever secret the installing shell was holding into a file on disk.

The **Claude Code**: resolved at install time by the same search every command
uses — so an explicit `PERCH_CLAUDE_BIN` passes through as itself — and written
into the unit under that name. The service manager's `PATH` is not the
installer's: launchd hands a LaunchAgent `/usr/bin:/bin:/usr/sbin:/sbin`, which
holds no `claude` anybody installs today, so a unit carrying no answer leaves
the Service holding on "no `claude` was found on PATH" from its first round
while `install`, `probe` and `watcher status` all report health. `PATH` itself
is still not carried: the one thing the Watcher needs from it is where `claude`
is, and that fits in a variable the idempotent `install` re-resolves. An
install that finds none writes none and says the Service will hold, rather than
refusing — Claude Code arriving later is ordinary, and the repair is the same
one command.

`perch watcher status` reads the binary and the log back out of the installed
unit rather than recomputing them, because whether the unit and the machine
have come apart is the whole of what it is asking, and a value worked out again
from the machine would agree with the machine by construction.

Every one of these formats is line-oriented or shell-parsed, so a value that
cannot be held in one is refused before anything is written: a `PERCH_HOME` with
a newline in it would otherwise close `Environment=` and write whatever follows
as further unit directives, into a file systemd loads at every login.

## What other commands owe a running Service

**A Purge stops and removes the Service before it deletes anything**, and
refuses outright if it cannot. This is ADR a-removal-lands-first's shape one
level up: removing the active Account lands somewhere before it deletes
anything, and giving the whole machine back stops the thing that writes to it
before it starts. A Watcher racing a Purge is one process writing a captured
Credential into a Profile directory another is deleting, and there is no partial
success there worth having. What it asks is the watcher lock rather than the
service manager, because a Watcher somebody typed in a terminal is the same
hazard and no Service was ever installed on that machine.

**An Upgrade re-installs the Service afterwards.** An Upgrade hands the work to
`brew` or `npm`, neither of which knows a unit exists, and on Unix the running
process keeps its inode and goes on executing the old binary until something
restarts it. So `perch upgrade` rewrites the unit with the freshly resolved path
and restarts it. A failure there is a warning rather than a failed Upgrade: the
binary really is newer, and `perch watcher install` is idempotent precisely so
that the fix is one command.

## Consequences

`SIGTERM` sets the same flag `SIGINT` does and is acted on at the wait, so a
stop lets an in-flight Switch finish. A person types Ctrl-C; a service manager
stops a process with `SIGTERM`, whose default action is immediate death — and
killed between the Credential reaching the Default Profile and the Identity
being patched, that is a Landing nobody wrote down. The stop grace is pinned to
thirty seconds on every platform rather than inherited as systemd's ninety and
launchd's twenty: what has to fit inside it is a Capture, a Credential write and
an Identity patch under Claude Code's locks.

Windows gets a Scheduled Task rather than a Windows Service. A true service
would mean the service control manager, a service entry point and a session-zero
process with no user profile; a logon task runs in the user's own context, which
is where the registry and the Profiles are. It is registered non-interactively so
that no console window appears at every login.

No exit code is new. A blocked Check is `20`, an `uninstall` with nothing to
remove is `15`, and `status` is a question that succeeds either way, as
`perch upgrade --check` already does.

The three platform arrangements cannot be proved behind `FakeHost`, because what
is being asserted is that launchd, systemd and Task Scheduler do what they say.
What is proved automatically is everything up to them: the text of each unit,
the commands driven, and that a path written into a unit is the path read back
out of it. That they are honored is proved by using it
(ADR using-it-is-the-proof), including the one act only a person can perform,
which is logging out and back in again.
