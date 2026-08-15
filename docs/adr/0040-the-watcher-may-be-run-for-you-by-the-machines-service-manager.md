# The Watcher may be run for you by the machine's service manager

Supersedes the part of ADR 0013 that rejected a managed background daemon. The
rest of that record stands — the interval, the back-off curve, the cooldown, the
margin, the no-return, the exit-code table and the reasoning behind every one of
them are unchanged and are still where they were written. What changes is that
`perch watch` is no longer only something you type.

`perch service install` writes a unit the machine's own service manager owns,
starts it, and from then on the Watcher comes up when you log in.
`perch service uninstall` takes it back. `perch service status` says what is
there.

## The argument that was actually load-bearing

ADR 0013 gave two reasons for refusing a daemon. Only one of them was ever a
reason.

The first was safety: a daemon "would mutate credentials while nobody is
watching — every hazard in ADR 0006 firing unattended". That argument was spent
the moment the same record blessed `perch watch --once` under cron. A Check
scheduled at 02:00 mutates credentials with nobody watching, by exactly the same
mechanism, with exactly the same hazards. Whatever unattended switching costs,
Perch has been charging it since ADR 0013 shipped; a Service does not add a risk,
it adds a way of arranging one that was already permitted.

The second was cost: a daemon "would need service lifecycle management on three
platforms, in exchange for convenience the user's own scheduler already
provides". That one is real, and it is the whole of what this record decides to
pay. What it buys is not convenience in the abstract. It is the difference
between a rotation that works because you remembered to type something and one
that works because the machine came up.

"Scheduling is the operating system's job" survives intact, and this record
agrees with it harder than ADR 0013 did. Perch does not fork, does not
double-fork, writes no PID file, and never puts itself in the background. The
operating system starts the process and the operating system stops it. Perch's
contribution is a unit file and the honesty to remove it again.

## A supervised loop, not a scheduled Check

The obvious implementation is a timer running `perch watch --once` every two and
a half minutes. It needs no new code at all: the Check exists, its exit codes
were designed for a scheduler, and its cooldown already survives between
invocations in `registry.checks`.

It is the wrong one, and the reason is the Back-off.

A Back-off is memory the loop keeps. It doubles from the interval to twenty
minutes when a Refresh cannot be read, and the first Refresh that works drops the
whole of it. A Check has no memory to keep it in, so a sequence of Checks has no
Back-off at all — a fixed-interval timer asks a refusing endpoint twenty-four
times an hour, for ever, which is precisely the arithmetic ADR 0013 wrote the
Back-off to prevent. Persisting it would work, and would be wrong twice over: it
would put Anthropic's allowance in the registry beside the Group's settings, as
if it were one of them, and it would give the loop a file to write when the whole
of ADR 0013's cooldown reasoning is that a loop's pacing belongs to the loop.

So the Service runs the loop, unchanged. The supervisor's only job is to start
it and to keep it started; every question about when to ask, when to act and when
to wait is answered by the same code that answers it in a terminal. There is one
Watcher, with three ways of being run, rather than two Watchers that have to be
kept in agreement.

The cost is named rather than hidden: a supervised loop spends the full
twenty-four reads an hour whether or not anybody is working. A timer could be
sparser. But the interval is derived from Anthropic's allowance rather than from
anyone's preference — ADR 0013 is explicit that it is a constant and not a
setting — so "poll less when idle" is not a knob this design has to offer, and
inventing one would mean inventing a definition of idle.

## It starts at login, and it is yours rather than the machine's

A LaunchAgent, a `systemd --user` unit, a Scheduled Task that runs on logon.
Never a LaunchDaemon, never a system unit, and `install` refuses when it is run
as root.

Every Profile Perch holds is under one person's home directory, and the registry
that says which Account is active is that person's. A watcher started before
anybody logs in has no home to read and nothing to decide about. On macOS it
would also have no keychain: ADR 0008 has Perch shelling out to `security`, and
there is no unlocked keychain before login. Off macOS the credential is a
plaintext file (ADR 0020), so the keychain argument is macOS's alone — but the
home directory argument is every platform's, and it is sufficient.

"Every time the machine starts up" is therefore delivered as "every time you log
in", and the guide says so in those words. Somebody who genuinely wants it on a
headless box before login wants `loginctl enable-linger`, which is theirs to
enable and not something an `install` should do quietly.

## Exactly one Watcher

Before this, nothing stopped three `perch watch` loops running in three
terminals. ADR 0013 contemplated it — "two people running two loops still pace
themselves separately" — but that is about two *people*, with two homes and two
registries. Two loops for one person watch the same active Account and each keeps
its own Cooldown in memory, where neither can see the other's. That is the
ping-pong the Cooldown and the Margin exist to prevent, reintroduced by running
the thing twice.

So a Watcher takes a lock in Perch's home and holds it for as long as it runs.
A **Check** that cannot take it exits `20`, which is the code `lock.rs` already
produces for a contended lock and which ADR 0036 already defines as a promise
that nothing was changed. It takes the same lock as the loop because the
in-memory Cooldown and the one in `registry.checks` cannot see each other — a
Check firing while the Service runs is the same double-switch by a different
route. Somebody running both a cron Check and a Service gets a mailbox of 20s,
which is true and is the right nudge to pick one.

A **loop** that cannot take it holds and comes back, for exactly the reason the
section below repeals the permission exits: a Service that exited here would be
one the supervisor restarts into the same exit, and the lock it is waiting on
may be a stale one left by a `kill -9`. The staleness window is long — the
longest back-off plus a round, because that is the longest a healthy Watcher can
go without renewing — and it is *affordable* only because nothing exits on it.
Holding turns "this machine is unusable for twenty minutes" into some held
rounds that say why. That trade is the whole reason the window can be derived
from the loop's own arithmetic rather than picked to keep a crash loop short.

This is the first artifact `perch watch` has ever left behind, and it repeals
the promise the loop printed on the way out. The promise was worth keeping when
it was free. It is not free any more, and a lock released when the process ends
is a smaller thing to leave behind than a second Watcher fighting the first.

## Permission holds the loop rather than stopping it

ADR 0013 has the loop stop when the grant is taken back: `14` when a Group
withdraws `watcher-may-act`, `18` when a `perch switch` elsewhere leaves an
ungrouped Account active. The reasoning was that "a loop still running on
permission that has been withdrawn is the exact thing 'nothing changes underneath
you unless you said it could' is about".

Under a supervisor that is a crash loop. `Restart=always` respawns into the same
exit until systemd's start limit trips and leaves the unit failed, needing a
manual `reset-failed`; launchd's `KeepAlive` respawns every ten seconds
indefinitely. It cannot be fixed per platform either: systemd can be told
`SuccessExitStatus=14 18`, and launchd cannot express it at all — its `KeepAlive`
knows only zero from non-zero.

So the exits are repealed and both become a `Held` round. The safety argument is
untouched, because a holding Watcher acts on nothing: it does not Refresh, it
does not rank, it does not Switch. ADR 0013 conflated stopping *acting* with
stopping *existing*, which was correct when the only Watcher was a loop in front
of a person — a terminal loop idling for ever tells that person nothing — and is
incorrect for one nobody is sitting in front of. A Service that exits because you
have not granted permission yet is a Service you have to remember to start again
after you do.

A permission hold reads the registry rather than the network, so it costs
nothing and needs no Back-off. It is still re-read every round rather than only
at the first, exactly as ADR 0013 required, and it re-checks on the ordinary
interval rather than a faster one of its own: granting permission and waiting up
to two and a half minutes is not a cost worth a second cadence to explain.

`perch watch --once` keeps `14` and `18` unchanged. A Check is one process
reporting to a scheduler, and a scheduler needs to be told that this machine is
not arranged for what it was asked to do.

## Identical holds are said once, not every round

ADR 0013 required that "every held round says which failure held it and when the
watcher will ask again", and that a hold saying neither "reads as a watcher that
has given up". That was written about a person watching a terminal, where the
repeated line is the proof of life.

A Service writes to a log nobody reads until something is wrong, and a
permission hold repeats until somebody changes a setting — possibly for weeks.
At one line every two and a half minutes that is five hundred and seventy-six
identical lines a day, and what they bury is the one line that matters.

So consecutive holds with the same reason are said on the way in, once an hour
while nothing changes, and always on the way out. The hourly line carries how
long it has been holding, which is the proof of life the original rule wanted and
a better one than repetition: a Watcher that has been held since 09:14 is
obviously stuck in a way that the same sentence four hundred times is not.

## Standard output is still the only sink

ADR 0013 refused a rotated logfile on the grounds that "a rotated logfile is what
a daemon needs because nobody is watching, and the whole of this record is that
Perch is not one". Perch is one now, and the refusal survives anyway, because
the unit file can route standard output better than Perch can.

On Linux systemd captures it into the journal, which rotates it, retains it and
makes `journalctl --user -u perch-watch -f` work without Perch knowing the word
journal. On macOS launchd is pointed at `$PERCH_HOME/watch.log`, and on Windows
the task redirects to the same path. Perch writes the decision line to standard
output exactly as it always has, and gains no logging subsystem, no rotation, no
levels and no configuration.

The macOS and Windows files grow without bound. At one line every two and a half
minutes that is of the order of twenty megabytes a year, which is a real cost and
a smaller one than the code that would cap it. If somebody's log becomes a
problem, that is the reason to write the rotation, and not before.

Putting the file inside `$PERCH_HOME` is what makes it Perch's to remove. It is
swept by a Purge with everything else Perch holds.

## What the unit records, and why it is not what you would guess

A unit file names an absolute path, and it is read by a service manager with
almost no environment. Two things therefore have to be captured at install time.

The **binary**: not `argv[0]`, and not always the fully resolved target either.
Under npm the thing on `PATH` is a JavaScript shim that execs a platform package,
and it needs `node` on a `PATH` that a `systemd --user` unit does not have — so
there the shim is resolved through to the real binary. Under Homebrew resolving
is exactly wrong: the target is a version-stamped path inside the Cellar, so a
unit pointing at it breaks on the next `brew upgrade`, while the symlink in
`bin` is stable across every Release. The rule is the most stable path that runs
without a shell, which is a different answer per Channel and not a single call to
`canonicalize`.

The **environment**: `PERCH_HOME` and `CLAUDE_CONFIG_DIR` are read from the
process environment, and are typically set in a shell profile that no service
manager will ever source. A Service that silently watched `~/.config/perch` while
its owner works out of `PERCH_HOME=~/work/perch` would be watching an empty
registry and reporting that there was nothing to do. Both are written into the
unit when — and only when — they are actually set.

Both are re-checked by `perch service status`, which says whether the recorded
binary is still there. `perch upgrade` moves it (ADR 0039), so this is a
condition that arises in ordinary use rather than only under misuse.

## What other commands owe a running Service

**A Purge stops and removes the Service before it deletes anything**, and refuses
outright if it cannot. This is ADR 0024's shape one level up: removing the active
Account lands somewhere before it deletes anything, and giving the whole machine
back stops the thing that writes to it before it starts. A Watcher racing a Purge
is one process writing a captured Credential into a Profile directory another is
deleting, and there is no partial success there worth having.

**An Upgrade re-installs the Service afterwards.** ADR 0039 hands the work to
`brew` or `npm`, neither of which knows a unit exists; on Unix the running
process keeps its inode and goes on executing the old binary until something
restarts it. So `perch upgrade` rewrites the unit with the freshly resolved path
and restarts it. A failure there is a warning rather than a failed Upgrade: the
binary really is newer, and `perch service install` is idempotent precisely so
that the fix is one command.

## Consequences

`SIGTERM` has to be handled, and was not. `perch watch` listened for `SIGINT`
alone, which is what a person types; a service manager stops a process with
`SIGTERM`, whose default action is immediate death. Killed between the Credential
reaching the Default Profile and the Identity being patched, that is a Landing
nobody wrote down. So `SIGTERM` sets the same flag `SIGINT` does, is acted on at
the wait, and lets an in-flight Switch finish — and the stop grace is pinned to
thirty seconds on every platform rather than inherited as systemd's ninety and
launchd's twenty.

Windows gets a Scheduled Task rather than a Windows Service. A true service
would mean the service control manager, a service entry point and a session-zero
process with no user profile; a logon task runs in the user's own context, which
is where the registry and the Profiles are. It is registered non-interactively so
that no console window appears at every login.

No exit code is new. A blocked Check is `20`, an
`uninstall` with nothing to remove is `15`, and `status` is a question that
succeeds either way, as `perch upgrade --check` already does.

The three platform arrangements cannot be proved behind `FakeHost`, because what
is being asserted is that launchd, systemd and Task Scheduler do what they say.
They are proved by a Dogfood phase that declares a service manager as what it
needs of a machine, so the Preflight counts it only where there is one — and by
one Attended phase, because logging out and back in again is an act only a person
can perform (ADR 0038).
