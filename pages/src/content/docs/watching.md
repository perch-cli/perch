---
title: "Watching"
sidebar:
  order: 5
---

`perch watcher run` does the Cycling for you. It is a loop in this terminal that
reads how full the Account you are on is, says what it made of that, and
Switches within the Group when the Account runs low — so you stop being the one
who notices.

```
$ perch watcher run
Watching you@example.com within Group `work`. Reading how full it is every 2m30s, and Switching within that Scope when its fullest Quota Window reaches 80% — to an Account at 70% or under, and never twice inside 15 minutes. Ctrl-C stops.
2026-08-04T12:00:00Z  waiting   40% used, fullest 5-hour
2026-08-04T12:02:30Z  switched  86% used, fullest 5-hour → overflow@example.com
^C
Stopped.
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
are most of them. A line names the figure that was read and what was decided
about it, so "why did it switch just then" is answerable without reading the
source. It goes to standard output and no file is written or rotated:
redirecting it is yours to do.

**The rounds that did what you asked stop at the figure.** `waiting` and
`switched` are the loop doing exactly what the opening line said it would do, so
they say what they read and — where it moved — where it went. The threshold is
in the opening line, once, because it does not change while the loop runs; the
words "under it" and "over it" are what `waiting` and `switched` already mean.
**The rounds that refused keep their whole sentence**: `cooling`, `nowhere`,
`held` and `refused` are rounds where nothing happened and you cannot see why,
so each of them says why.

Two more words end a watch rather than pace one. A round that finds another
Watcher has taken the watch over says `replaced` and stops, because deciding
alongside somebody else is the thing the lock exists to prevent. One interrupted
between reading a figure and acting on it says `stopped`. They are told apart on
purpose: `replaced` is a machine somebody is still watching, and `stopped` is one
nobody is.

**An em dash is where an explanation begins**, and it is on a line only where
something other than the ordinary happened. Every refusal has one. So does a
`switched` line that ranked a candidate on a figure it could not re-read this
round — the one case where a Switch can land you somewhere worse than it left,
and so the one thing about it worth saying.

**Which Account is on the line only where it changed.** The opening names the
one being watched and every `switched` line names the one it moved to, so the
rounds in between do not restate it.

**Stopping changes nothing and leaves nothing behind.** Ctrl-C gives the watcher
lock back, writes no file of the watcher's own, and leaves you on the Account it
last Switched to — every time, which is why the line on the way out is one
word.

**Only the Account you are on is read.** Anthropic allows roughly 28-30 reads an
hour per Account, and one Account read every two and a half minutes fits inside
that with room left for the `perch status --refresh` you type while it runs. The
Accounts it could move to are read at the moment a decision needs them and not
before — they are idle then by definition, so renewing them costs nobody a
session.

**It never acts on a figure it did not just read.** A read that fails holds the
decision rather than falling back on the cached figure, because a Switch made
on a cached figure is one you could have made yourself without leaving a
process running. A reply that arrives but says nothing Perch can read a Quota
Window out of is a failed read too — not an Account with nothing used, which is
the one reading that could never be over any threshold.

```
2026-08-04T12:00:00Z  held      unread — Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 2m30s.
2026-08-04T12:02:30Z  held      unread — Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 5m00s.
2026-08-04T12:07:30Z  nowhere   100% used, fullest 5-hour — Every Account in Group `work` is exhausted. overflow@example.com frees up soonest, at 2026-08-04 14:30 UTC (in 2h23m).
```

Neither of those ends the watch. Nowhere to go is resolved by waiting, which is
what the loop is already doing.

The same rule covers the Accounts it would move to. A round over the threshold
reads every candidate before it chooses, and a candidate whose read fails is
set aside, whatever Perch last cached for it: a Switch onto a figure the round
has not just read is a Switch made blind. Where no candidate could be read the
round is `held`, says which candidates and why, and is paced by the Back-off
below like any other read that failed. The figure on that line is the Account
you are on, which the round did read.

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
left. **The margin is what stops it**; the cooldown paces what the margin has
already allowed. Each is printed when it is what decided a round:

- a **margin**, 10 points under the threshold by default, which nothing is moved
  to unless it clears. At an 80% threshold that means nothing above 70%. A round
  with nowhere clear enough says there is nowhere better to go, which is the
  true answer — and it is the only one of the two that can say so, because a
  cooldown sets how often a pointless move happens and never stops it happening.
  `perch config set <scope> watcher-margin-percent <points>` moves it; the
  cooldown below is fixed.
- a **cooldown** of 15 minutes between one Switch and the next, whatever the
  figures do in between. A five-hour window moves slowly enough that fifteen
  minutes never misses a real crossing. It is checked before anything is read:
  a round that may not act has no business spending an allowance on figures it
  cannot use.

```
2026-08-04T12:02:30Z  nowhere   86% used, fullest 5-hour — Nothing in Group `work` is worth Switching to yet: overflow@example.com is at 74% used and nothing over 70% is worth moving to.
2026-08-04T12:05:00Z  cooling   86% used, fullest 5-hour — the last Switch was 2 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 12 minutes.
```

**A round that found nowhere to go does not read the candidates again for
fifteen minutes.** The interval is one Account's allowance, and reading every
candidate on top of it every 2m30s spends each candidate's allowance on
Accounts the round just refused. The loop keeps reading the Account you are on
at the interval — that figure is what `perch status` shows you — and a round
inside the rest says what the last reading of the candidates found and when
they will be asked again:

```
2026-08-04T12:05:00Z  nowhere   88% used, fullest 5-hour — Nothing in Group `work` is worth Switching to yet: overflow@example.com is at 74% used and nothing over 70% is worth moving to. The candidates were read 2 minutes ago, so they are not asked again for another 12 minutes.
```

It is the cooldown's fifteen minutes, for the cooldown's reason. A scheduled
check has no memory of its last round and reads the candidates every time it
is over the threshold, and a Switch you make yourself ends the rest: the
candidates are a different set.

An Account Perch has never read a figure for is set aside the same way. A
[Cycle](switching.md#cycling) you asked for will land on one — an unknown beats a
window that is certainly full — but unasked it would be a move onto an Account
the watcher knows nothing about, and no figure is not evidence of room.

None of it survives the loop. The cooldown lives in the running process and
nowhere else — stopping `perch watcher run` and starting it again is you saying
"go on then", and it starts with nothing to wait for. A scheduled check is the
one exception, and [it says why below](#watching-on-a-schedule).

What it does when it acts is a Switch and nothing else: the outgoing Credential
is Captured into its own Profile first, Claude Code's locks are taken, and a
Live Profile's token is never Renewed. Running while Claude Code is working is
the normal case, not the exception.

Two things leave it with nothing to do, at the first round or at any round
after. It **holds** on either rather than exiting — it says what is missing,
waits, and takes over the moment that changes:

```
$ perch watcher run
Started. Nothing is being decided yet; the next line says what is holding it. Ctrl-C stops.
2026-08-04T12:00:00Z  held      unread — you@example.com is in no Group, and nothing has said the Accounts in no Group are interchangeable at all [...] Asking again in 2m30s.

$ perch watcher run
Started. Nothing is being decided yet; the next line says what is holding it. Ctrl-C stops.
2026-08-04T12:00:00Z  held      unread — Group `work` has not been told the watcher may act on it [...] Asking again in 2m30s.
```

`perch watcher check` is the one that exits on these, with the codes in the
table below, because a Check is one process reporting to a scheduler.

`interchangeable` grants the watcher nothing on its own. Declaring a set of
Accounts substitutable and letting something move between them **while nobody is
looking** are different things, and among the Accounts in no Group both have to
be said. A Group needs only the second, because a Group *is* the first.

A `watcher-may-act` is said about the Scope it grants and reaches no other, so
letting the watcher into your work Group authorizes that Group and nothing else
— not the Accounts in no Group, and not a Group you declare tomorrow. The price
is that there is no one command that withdraws the watcher everywhere: it is one
per Scope. A brake that worked by blanket inheritance would be the wrong brake
for consent.

Both permissions are read every round rather than only at the first, because
either can be taken back while the watcher is sleeping. A `perch switch` in
another terminal that leaves an ungrouped Account active, or a `perch config set
work watcher-may-act false`, **holds** a watcher that is already running: it
says what is missing and goes on asking, and starts deciding again the moment
the grant comes back. It reads nothing and moves nothing while it holds — a
grant withdrawn is a watcher that stops *acting*, which is what the grant is
about, rather than one that stops existing and has to be remembered about later.

`perch watcher check` still exits on it, with the codes below, because a Check
is one process reporting to a scheduler and the scheduler has to be told.

## Having the machine run it

`perch watcher install` has your machine run the loop for you, starting when you
log in — a LaunchAgent on macOS, a `systemd --user` unit on Linux, a Scheduled
Task on Windows. It is the *same loop*, supervised: same interval, same policy,
same decision log. Perch writes the unit and hands the job over.

```
$ perch watcher install
Installed the Service. It runs /opt/homebrew/bin/perch as a LaunchAgent.
It finds Claude Code at /Users/you/.local/bin/claude, carried in the unit rather than looked up on the service manager's own PATH.
Its decisions go to /Users/you/.config/perch/watch.log.

$ perch watcher status
A Service is installed as a LaunchAgent, and is running.
Its unit is /Users/you/Library/LaunchAgents/cli.perch.watch.plist.
It runs /opt/homebrew/bin/perch.
Its decisions go to /Users/you/.config/perch/watch.log.
A Watcher is running on this machine and holds the watcher lock.

$ perch watcher uninstall
The Service is stopped and its unit is gone.
```

An install that succeeds has started the Service and arranged for it to start
again when you log in, so it says neither; an install that could not start it
says so and names the repair. An uninstall that succeeds has stopped it and
taken the unit back, and leaves `perch watcher run` in a terminal exactly as it
found it.

**At login, and yours rather than the machine's.** Never a system service and
never at boot: every Profile Perch holds is under your home directory, and on
macOS there is no unlocked keychain before somebody logs in. Installing it under
`sudo` is refused for that reason.

**Claude Code travels in the unit.** A service manager starts the Watcher with
almost no PATH of its own, so `install` finds `claude` the way every other
command does — your PATH, or `$PERCH_CLAUDE_BIN` if you set it — and writes the
answer into the unit. An install that finds none still succeeds, says the
Service will hold, and re-running `perch watcher install` once Claude Code is
there carries it in.

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
scheduling is the operating system's job: cron and systemd timers already run
things at an interval, keep them from overlapping, and capture what they
printed.

Pick this *or* a Service, not both: a Check that finds a Watcher running exits
`20` and does nothing, so a machine with both gets a cron mailbox full of them.

```
*/5 * * * * perch watcher check >> ~/.local/state/perch-watch.log 2>&1
```

```
$ perch watcher check
2026-08-04T12:00:00Z  switched  86% used, fullest 5-hour → overflow@example.com   # exit 0

$ perch watcher check
2026-08-04T12:05:00Z  cooling   90% used, fullest 5-hour — the last Switch was 5 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 10 minutes.   # exit 15
```

A check's line is a round's line, cut the same way: the figure it read, where a
Switch went, and a whole sentence wherever it refused. It prints no opening line
of its own — there is no run for one to open — so the threshold, the interval
and the cooldown are not restated for a check either. They are the Scope's
settings, and [`perch config`](configuration.md) is where they are read back.

**The Account is named where the line is about a particular one**: the Account a
Switch moved to, and the `perch relogin` that repairs a Quarantined one. A check
that decided nothing does not name it, because the Account a check reads is
whichever one is active — `perch status` is the question that answers that, and
a machine running checks is running them against one Perch.

The stamp stays a full date and time for the same reason: a check's line is
often the only thing in a cron mailbox, or one line among a month of them in the
file the crontab above appends to, read cold by somebody who was not there.

It is the same policy as the loop, run once — the same threshold, cooldown and
margin, and the same refusal to act on a figure it did not just read. **The
cooldown survives between invocations**, because each Check is a fresh
process and the sequence of them is the watcher: when one check Switched is
recorded against the Group in the Registry for the next one to be paced by. That
stamp is the one thing about the watcher that is written down, and it is why a
check every minute still Switches no more often than the cooldown allows. The
loop keeps the same fact in memory instead — two loops would be two people
watching, and one pacing the other's decisions is not what either of them asked
for.

The exit codes are [the full table](reference.md#exit-codes). Five of them are a
check saying what it decided:

| Code | What a check decided |
| ---- | -------------------- |
| 0 | it Switched |
| 15 | nothing to do now — under the threshold, inside the cooldown, a client was holding the Profile, or the Account it went to turned out to be Quarantined |
| 17 | a Switch was wanted and every candidate was exhausted |
| 18 | the Account it is on is in no Group, so nothing carries permission |
| 20 | held: a lock somebody else has — a Watcher was already running — or the figures were stale and the Refresh that would have replaced them failed. Nothing is wrong and nothing was changed — ask again shortly |

Three more are the machine not being arranged for a check at all, and a cron
wrapper meets the first of them before anything else:

| Code | What is in the way |
| ---- | ------------------ |
| 14 | nothing has said the watcher may act — `perch config set <scope> watcher-may-act true` is the grant, and it is off until somebody makes it |
| 12 | no Account is active, so there is nothing to watch |
| 11 | the keychain would not answer, so the Credential could not be read |

`20` is the one code the watcher added, and it exists because a scheduler
retrying in five minutes has to tell a figure it could not read from a Group
with nowhere to go: the first resolves itself and the second does not. A Switch
held back by the cooldown is `15` — there was nothing to do *now* — and which of
the rules held it is on the line rather than in the code,
because a script can do nothing different about it and a person reading a cron
mailbox wants to know.

An Account whose Credential has stopped working is `20` when the *Refresh* is
what found out — a figure that cannot be read is a figure that cannot be read —
and the line names the Quarantine and the `perch relogin` that repairs it. Where
the Switch itself is what found out, it is `15` instead: the Quarantine has been
written down, the Account is passed over from the next round onwards, and there
genuinely is nothing to do now. Either way the line says which Account and which
repair, so a cron mailbox is not left reading the code alone.

Anything that stops a check from deciding exits as it would from any other
command: `14` for a Group that has not said the watcher may act, `12` for no
active Account at all, `11` for a keychain nobody can reach, and whatever
failed for a Switch that made the incoming Credential live and then could not
finish. A check reports what it decided, and a machine part way through a
Switch is not a decision.
