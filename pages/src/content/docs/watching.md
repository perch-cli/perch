---
title: "Watching"
sidebar:
  order: 5
---

`perch watcher run` does the Cycling for you. It is a loop in this terminal
that reads how full the Account you are on is and Switches within the Group
when it runs low. `perch watcher install` has the machine run the same loop,
and `perch watcher check` runs one round for a scheduler.

## Watching in a terminal

```
$ perch watcher run
Watching you@example.com within Group `work`, every 2m30s. Switching at 80% to an Account at 70% or under. Ctrl-C stops.
2026-08-04T12:00:00Z  waiting   40% used, fullest 5-hour
2026-08-04T12:02:30Z  switched  86% used, fullest 5-hour → overflow@example.com
^C
Stopped.
```

The opening line is the policy in force: the interval, the threshold, the
ceiling a candidate has to be under, and the cooldown. `perch config set
<scope> watcher-threshold-percent` and `watcher-margin-percent` move the two
percentages. The interval and the cooldown are fixed.

Every round prints one line: the time, the word it decided on, the figure it
read. `waiting` and `switched` are the loop doing what the opening line said.
Every other word is a round where nothing happened, and the sentence after the
dash says why. Only the Account you are on is read each round. The Accounts
it could move to are read when a decision needs them.

Perch never backgrounds itself. There is no `--detach`. Ctrl-C is safe
wherever it lands, gives the watcher lock back, and leaves you on the Account
it last Switched to. A second `perch watcher run` says who holds the lock and
waits for it. Everything goes to standard output, and no file is written.

The watcher acts only where a Scope has said it may. Until then it holds:

```
$ perch watcher run
Started. The next line says what is holding it. Ctrl-C stops.
2026-08-04T12:00:00Z  held      unread — Group `work` does not let the watcher act, so nothing is watched. `perch config set work watcher-may-act true` does. Asking again in 2m30s.
```

A hold is not an exit. Run the command it names in another terminal and the
loop starts deciding on its next round. Taking the grant back holds a running
watcher the same way. The Accounts in no Group need `interchangeable` on as
well as `watcher-may-act`.

## What a round says when it does not move

```
$ perch watcher run
Watching you@example.com within Group `work`, every 2m30s. Switching at 80% to an Account at 70% or under. Ctrl-C stops.
2026-08-04T12:00:00Z  nowhere   86% used, fullest 5-hour — Nothing within Group `work` is worth Switching to yet: overflow@example.com is at 74% used and nothing over 70% is worth moving to.
2026-08-04T12:02:30Z  nowhere   88% used, fullest 5-hour — Nothing within Group `work` is worth Switching to yet: overflow@example.com is at 74% used and nothing over 70% is worth moving to. The candidates were read 2 minutes ago, so they are not asked again for another 12 minutes.
```

`nowhere` is a round over the threshold with no candidate under the ceiling,
or with every candidate exhausted. Candidates a round refused are not read
again for fifteen minutes. A login, a Group move or a Switch you make yourself
changes who they are, and the next round reads the new set.

```
2026-08-04T12:02:30Z  switched  86% used, fullest 5-hour → overflow@example.com
2026-08-04T12:05:00Z  cooling   90% used, fullest 5-hour — the last Switch was 2 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 12 minutes.
```

`cooling` is a round inside the fifteen minutes after a Switch. Stopping the
loop and starting it again starts with nothing to wait for.

```
2026-08-04T12:00:00Z  held      unread — Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 2m30s.
2026-08-04T12:02:30Z  held      unread — Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 5m00s.
2026-08-04T12:07:30Z  held      unread — Anthropic is rate-limiting reads of this Account's usage, so nothing current could be read. Asking again in 10m00s.
```

`held` with `unread` is a read that failed. The watcher does not act on a
cached figure. It waits, and the wait doubles with each failure up to twenty
minutes. The first read that works puts it back to 2m30s. A candidate that
could not be read is set aside for that round, whatever the cache says.

`replaced` ends the loop: another Watcher has taken the lock. `stopped` is a
Ctrl-C that arrived between reading a figure and acting on it.

## Having the machine run it

```
$ perch watcher install
Installed the Watcher. It checks every 150 seconds.

$ perch watcher status
A Service is installed as a LaunchAgent, and is running.
Its unit is /Users/you/Library/LaunchAgents/cli.perch.watch.plist.
It runs /opt/homebrew/bin/perch.
Its decisions go to /Users/you/.config/perch/watch.log.
A Watcher is running on this machine and holds the watcher lock.

$ perch watcher uninstall
The Service is stopped and its unit is gone.
```

The Service is the same loop, started when you log in: a LaunchAgent on macOS,
a `systemd --user` unit on Linux, a Scheduled Task on Windows. It is installed
for your user, and `sudo perch watcher install` is refused.

The unit carries each enabled provider's CLI: the configured `cli-path`, else
`PERCH_CLAUDE_BIN` or `PERCH_CODEX_BIN`, else the first on your PATH that runs
under the service manager's own environment. It also carries `PERCH_HOME`,
`CLAUDE_CONFIG_DIR` and `CODEX_HOME`, and none of the shell's credentials.
`perch watcher status` says where the unit and the log are. On Linux the
decisions go to the journal, and the status line says the command to read them:

```
$ perch watcher status
A Service is installed as a systemd user unit, and is running.
Its unit is /home/you/.config/systemd/user/perch-watch.service.
It runs /usr/local/bin/perch.
Its decisions go to journalctl --user -u perch-watch -f.
A Watcher is running on this machine and holds the watcher lock.
```

An install that finds no `claude` or no `codex` still succeeds and names the
provider it could not carry; `perch watcher install` again once that CLI is
installed carries it. A carried Codex does not make the Watcher Cycle Codex
Accounts. Re-running `install` is also the repair after the binary moves;
`perch upgrade` does that for you and says if it could not. In a log, a hold that has not changed is
said once an hour rather than every round.

`perch watcher status` exits 0 whether or not anything is installed. Branch on
`--json`'s `installed`, `running` and `watching`:

```
$ perch watcher status --json
{
  "any_scope_may_act": true,
  "binary": "/opt/homebrew/bin/perch",
  "binary_exists": true,
  "installed": true,
  "log": "/Users/you/.config/perch/watch.log",
  "log_said": "/Users/you/.config/perch/watch.log",
  "platform": "launchagent",
  "running": false,
  "unit": "/Users/you/Library/LaunchAgents/cli.perch.watch.plist",
  "watching": false
}
```

## Watching on a schedule

```
$ perch watcher check
2026-08-04T12:00:00Z  switched  86% used, fullest 5-hour → overflow@example.com

$ perch watcher check
2026-08-04T12:05:00Z  cooling   90% used, fullest 5-hour — the last Switch was 5 minutes ago and the cooldown leaves at least 15 minutes between two, so nothing moves for another 10 minutes.
# exit 15
```

`perch watcher check` takes one round and exits, saying what it decided in the
exit code. It is for cron or a systemd timer:

```
*/5 * * * * perch watcher check >> ~/.local/state/perch-watch.log 2>&1
```

Pick a schedule or a Service, not both. A Check that finds a Watcher running
does nothing and exits as held. The policy is the loop's, and the cooldown survives
between Checks: when a Check Switched is recorded against the Group, so a Check
every minute still moves no more often than every fifteen. A Check has no
memory of the candidates it read, and reads them every time it is over the
threshold.

The line says which rule held it, and names the Account and the repair where
one is Quarantined. What each code means is in the
[reference](reference.md#exit-codes).
