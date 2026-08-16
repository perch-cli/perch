# Watching

`perch watcher run` does the Cycling for you. It is a loop in this terminal that
reads how full the Account you are on is, says what it made of that, and
Switches within the Group when the Account runs low — so you stop being the one
who notices.

```
$ perch watcher run
Watching you@example.com in Group `work`. Reading how full it is every 2m30s, and Switching within the Group when its fullest Quota Window reaches 80% — to an Account at 70% or under, and never twice inside 15 minutes. Ctrl-C stops.
2026-08-04T12:00:00Z  waiting   you@example.com 40% used, fullest 5-hour; threshold 80% — under it, so nothing was wanted.
2026-08-04T12:02:30Z  switched  you@example.com 86% used, fullest 5-hour; threshold 80% — over it. Switched — overflow@example.com has the most room: 95% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of just now.
^C
Stopped. The watcher lock is given back, no file of its own was written, and the Account you are on is the one it last Switched to.
```

The watcher acts only where a Scope has said it may, which is off by default —
see [`watcher-may-act`](configuration.md).

**Perch never backgrounds itself.** There is no `--detach` and nothing to kill
by pid: what the loop takes across a wait is one lock and nothing else, so
Ctrl-C is safe wherever it lands, and a Ctrl-C during a Switch lets that Switch
finish first. Wanting it running without a terminal open is what
[`perch watcher install`](#having-the-machine-run-it) is for, and wanting it on
somebody else's schedule is what
[`perch watcher check`](#watching-on-a-schedule) is for.

**There is one Watcher per person per machine**, whichever way it is being run.
A second `perch watcher run` says who holds the watcher lock and waits for it
rather than deciding alongside them — two loops watch the same Account and each
keeps its Cooldown in memory, which is exactly the pacing the Cooldown exists to
impose, undone by being run twice.

**Every decision is printed, including the ones where nothing happens** — which
are most of them. A line names what was read, the threshold it was read
against, what was decided and why, so "why did it switch just then" is
answerable without reading the source. It goes to standard output and no file
is written or rotated: redirecting it is yours to do.

**Only the Account you are on is read.** Anthropic allows roughly 28-30 reads
an hour per Account, and one Account read every two and a half minutes fits
inside that with room left for the `perch status --refresh` you type while it
runs. The Accounts it could move to are read at the moment a decision needs
them and not before — they are idle then by definition, so renewing them costs
nobody a session (ADR 0005).

**It never acts on a figure it did not just read.** A read that fails holds the
decision rather than falling back on the cached figure, because a Switch made
on a cached figure is one you could have made yourself without leaving a
process running. A reply that arrives but says nothing Perch can read a Quota
Window out of is a failed read too — not an Account with nothing used, which is
the one reading that could never be over any threshold.

```
2026-08-04T12:00:00Z  held      you@example.com unread; threshold 80% — nothing current to decide on, so nothing was decided: Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 2m30s.
2026-08-04T12:02:30Z  held      you@example.com unread; threshold 80% — nothing current to decide on, so nothing was decided: Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 5m00s.
2026-08-04T12:07:30Z  nowhere   you@example.com 100% used, fullest 5-hour; threshold 80% — over it, and nowhere to go: Every Account in Group `work` is exhausted, so there is nowhere useful to Switch. Nothing was changed. overflow@example.com frees up soonest, at 2026-08-04 14:30 UTC (in 3h).
```

Neither of those ends the watch. Nowhere to go is resolved by waiting, which is
what the loop is already doing.

**A failing read is asked about less and less often, and never more often than
a working one.** That growing wait is the **Back-off**: it doubles with each
failure — 2m30s, 5m, 10m — and stops at twenty minutes, so a transient failure
is recovered from at the ordinary cadence and a persistent one asks three times
an hour instead of twenty-four. It is bounded because the endpoint coming back
does not announce itself: the only way to find out is to ask. The first read
that works drops the whole of it. Every held line says which failure held it
and when it will ask again, so a hold never reads as a watcher that has given
up. It is not the cooldown — that paces Switches the watcher makes, and this
paces questions nobody is answering — and like the interval it is arithmetic
about Anthropic's allowance rather than anybody's preference.

**It does not move you somewhere no better.** Usage climbs on the Account you
are on and stands still on the ones you are not, so two Accounts either side of
the threshold do not trade places — they walk upward together, and every Switch
costs a Capture and a Credential write to land you somewhere with almost nothing
left. **The margin is what stops it** (ADR 0046); the cooldown paces what the
margin has already allowed. Both are fixed, and each is printed when it is what
decided a round:

- a **margin** of 10 points under the threshold, which nothing is moved to
  unless it clears. At an 80% threshold that means nothing above 70%. A round
  with nowhere clear enough says there is nowhere better to go, which is the
  true answer — and it is the only one of the two that can say so, because a
  cooldown sets how often a pointless move happens and never stops it happening.
- a **cooldown** of 15 minutes between one Switch and the next, whatever the
  figures do in between. A five-hour window moves slowly enough that fifteen
  minutes never misses a real crossing. It is checked before anything is read:
  a round that may not act has no business spending an allowance on figures it
  cannot use.

```
2026-08-04T12:02:30Z  nowhere   you@example.com 86% used, fullest 5-hour; threshold 80% — over it, and nowhere to go: Nothing in Group `work` is worth Switching to yet — overflow@example.com is at 74% used and nothing over 70% is worth moving to. Nothing was changed.
2026-08-04T12:05:00Z  cooling   you@example.com 86% used, fullest 5-hour; threshold 80% — over it, and too soon to move again: the last Switch was 2 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 12 minutes.
```

An Account Perch has never read a figure for is set aside the same way. A
[Cycle](switching.md#cycling) you asked for will land on one — an unknown beats a
window that is certainly full — but unasked it would be a move onto an Account
the watcher knows nothing about, and no figure is not evidence of room.

None of it survives the loop. The cooldown lives in the running process and
nowhere else — stopping `perch watcher run` and starting it again is you saying
"go on then", and it starts with nothing to wait for. A scheduled check is the
one exception, and [it says why below](#watching-on-a-schedule).

What it does when it acts is a Switch and nothing else: the outgoing Credential
is Captured into its own Profile first (ADR 0006), Claude Code's locks are
taken, and a Live Profile's token is never Renewed (ADR 0005). Running while
Claude Code is working is the normal case, not the exception.

Two things stop it, at the first round or at any round after, because there
would be nothing for it to do:

```
$ perch watcher run
you@example.com is in no Group, and nothing has said the Accounts in no Group are interchangeable at all — so there is nowhere for the watcher to Switch it to. Nothing is being watched.
`perch config set cycle-ungrouped true` says they are, and `perch config set ungrouped watcher-may-act true` then says the watcher may act on them. [...]   # exit 18

$ perch watcher run
Group `work` has not been told the watcher may act on it, so nothing is being watched. Nothing only ever changes underneath you because you said it could.
`perch config set work watcher-may-act true` says it may.   # exit 14
```

`cycle-ungrouped` grants the watcher nothing on its own, and neither does a
`watcher-may-act` set at Global. Permission to Switch **when you ask** and
permission to Switch **while nobody is looking** are different grants, and among
the Accounts in no Group both have to be given: a Global "yes" is a statement
about your Groups, and Inheriting it there would authorise moving you off a work
Account onto your personal subscription (ADR 0017).

Both permissions are read every round rather than only at the first, because
either can be taken back while the watcher is sleeping. A `perch switch` in
another terminal that leaves an ungrouped Account active, or a
`perch config set work watcher-may-act false`, **holds** a watcher that is
already running: it says what is missing and goes on asking, and starts deciding
again the moment the grant comes back (ADR 0040). It reads nothing and moves
nothing while it holds — a grant withdrawn is a watcher that stops *acting*,
which is what the grant is about, rather than one that stops existing and has to
be remembered about later.

`perch watcher check` still exits on it, with the codes below, because a Check
is one process reporting to a scheduler and the scheduler has to be told.

## Having the machine run it

`perch watcher install` has your machine run the loop for you, starting when you
log in — a LaunchAgent on macOS, a `systemd --user` unit on Linux, a Scheduled
Task on Windows (ADR 0040). It is the *same loop*, supervised: same interval,
same policy, same decision log. Perch writes the unit and hands the job over.

```
$ perch watcher install
Installed the Service. It runs /opt/homebrew/bin/perch as a LaunchAgent. It starts when you log in, and it is running now.
Its decisions go to /Users/you/.config/perch/watch.log.

$ perch watcher status
A Service is installed as a LaunchAgent, and is running.
Its unit is /Users/you/Library/LaunchAgents/cli.perch.watch.plist.
It runs /opt/homebrew/bin/perch.
Its decisions go to /Users/you/.config/perch/watch.log.
A Watcher is running on this machine and holds the watcher lock.

$ perch watcher uninstall
The Service is stopped and its unit is gone. Nothing starts at login any more, and `perch watcher run` in a terminal is unaffected.
```

**At login, and yours rather than the machine's.** Never a system service and
never at boot: every Profile Perch holds is under your home directory, and on
macOS there is no unlocked keychain before somebody logs in. Installing it under
`sudo` is refused for that reason.

**Where the decisions go** differs by platform, because the log is the service
manager's job rather than Perch's. On Linux systemd captures standard output
into the journal — `journalctl --user -u perch-watch -f`. On macOS and Windows
the unit points at `watch.log` inside Perch's own directory, which means
`perch holdings purge` sweeps it with everything else.

**An unchanged hold is said once an hour**, not every round. In a terminal a
repeated line is proof the loop is awake; in a log nobody reads until something
is wrong, five hundred identical lines a day are what bury the one that matters.
So a hold says itself in full when it starts, says how long it has been going
once an hour, and says what it cost when it ends.

**`install` is idempotent**, and re-running it is the repair for a Service that
stopped coming up: `perch upgrade` moves the binary, and `perch watcher status`
says when the unit names one that is no longer there. An Upgrade re-points the
Service at the new binary by itself, and says so if it could not.

## Watching on a schedule

`perch watcher check` takes one round and exits, saying what it decided in its
exit code. That is the whole of what an unattended watcher needs, because
scheduling is the operating system's job (ADR 0013): cron and systemd timers
already run things at an interval, keep them from overlapping, and capture what
they printed.

Pick this *or* a Service, not both: a Check that finds a Watcher running exits
`20` and does nothing, so a machine with both gets a cron mailbox full of them.

```cron
*/5 * * * * perch watcher check >> ~/.local/state/perch-watch.log 2>&1
```

```
$ perch watcher check
2026-08-04T12:00:00Z  switched  you@example.com 86% used, fullest 5-hour; threshold 80% — over it. Switched — overflow@example.com has the most room: 95% headroom.   # exit 0

$ perch watcher check
2026-08-04T12:05:00Z  cooling   overflow@example.com 90% used, fullest 5-hour; threshold 80% — over it, and too soon to move again: the last Switch was 5 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 10 minutes.   # exit 15
```

It is the same policy as the loop, run once — the same threshold, cooldown and
margin, and the same refusal to act on a figure it did not just read. **The
cooldown survives between invocations**, because each Check is a fresh
process and the sequence of them is the watcher: when one check Switched is
recorded against the Group in the registry for the next one to be paced by. That
stamp is the one thing about the watcher that is written down, and it is why a
check every minute still Switches no more often than the cooldown allows. The
loop keeps the same fact in memory instead — two loops would be two people
watching, and one pacing the other's decisions is not what either of them asked
for.

The exit codes are [the full table](reference.md#exit-codes), and a check reaches
five of them:

| Code | What a check decided |
| ---- | -------------------- |
| 0 | it Switched |
| 15 | nothing to do now — under the threshold, inside the cooldown, or a client was holding the Profile |
| 17 | a Switch was wanted and every candidate was exhausted |
| 18 | the Account it is on is in no Group, so nothing carries permission |
| 20 | held: the figures were stale and the Refresh that would have replaced them failed |

`20` is the one code the watcher added, and it exists because a scheduler
retrying in five minutes has to tell a figure it could not read from a Group
with nowhere to go: the first resolves itself and the second does not. A Switch
held back by the cooldown is `15` — there was nothing to do *now* — and which of
the rules held it is on the line rather than in the code,
because a script can do nothing different about it and a person reading a cron
mailbox wants to know.

An Account whose Credential has stopped working is `20` as well — a figure that
cannot be read is a figure that cannot be read — and the line names the
Quarantine and the `perch relogin` that repairs it.

Anything that stops a check from deciding exits as it would from any other
command: `14` for a Group that has not said the watcher may act, `12` for no
active Account at all, `11` for a keychain nobody can reach, and whatever
failed for a Switch that made the incoming Credential live and then could not
finish. A check reports what it decided, and a machine part way through a
Switch is not a decision.
