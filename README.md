# perch

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

Perch is for one person moving between logins they already hold — their own
accounts, on their own machine. It creates no accounts and authenticates
nobody; it only chooses between logins you have already made yourself.

## Installing

Perch is pre-1.0. Every release is real and works, but the command line may
still change between them, so no channel hands it to you by default — you ask
for it by name. macOS, Linux and Windows, on both Arm and Intel except Windows,
which is x64 only. Claude Code has to be installed for Perch to have anything
to switch between.

**Homebrew**, on macOS or Linux:

```sh
brew tap perch-cli/perch
brew install perch
```

**The installer**, on macOS or Linux:

```sh
curl -fsSL https://perch-cli.github.io/perch/install.sh | sh
```

**The installer**, on Windows:

```powershell
irm https://perch-cli.github.io/perch/install.ps1 | iex
```

**npm**, anywhere:

```sh
npm install -g perch-cli
```

Perch is pre-1.0 and the version number says so. It is published like anything
else on npm — `latest` is the newest release — because npm has no way for a
package to have no `latest`, and pretending otherwise only meant installing an
older version instead of a newer one.

**By hand**, from
[the releases page](https://github.com/perch-cli/perch/releases). Every release
carries one archive per platform, a `SHA256SUMS`, and signed build provenance.
The checksums say which bytes; the provenance says which workflow, in which
repository, at which commit produced them, which is the stronger claim:

```sh
gh attestation verify perch-v0.1.0-aarch64-apple-darwin.tar.gz --repo perch-cli/perch
```

Both installers check the checksum, and check the provenance too when `gh` is
installed and logged in — and refuse to install if that check fails.

**On macOS, if you download an archive in a browser**, Gatekeeper marks it as
quarantined, and the binary inside is unsigned: Perch skips Apple notarization
before 1.0. macOS will refuse to run it until you clear the mark:

```sh
xattr -d com.apple.quarantine perch
```

Homebrew, npm and both installers avoid this entirely — none of them set the
flag. Building from source does too; see [Building](#building).

## Status

Early. Perch adopts the login you already have, adds further Accounts without
disturbing it, names them, holds Groups of Accounts you have declared
interchangeable, lists what you have, reads how full each one is, switches to
an Account you name, picks one for you when you name none, watches the Account
you are on and Cycles when it runs low — in a loop or one check at a time for
your scheduler — runs a client as one Account without
switching to it, logs an Account in again when its Credential stops working,
gives one up when a subscription is retired, writes everything it holds to one
encrypted file and puts a whole machine back from one, gives the machine back
the state it had before Perch, draws all of it in an interactive view for when
the choice wants making by eye, and takes its configuration from a script.

```
$ perch status
Adopted the Claude Code login as your first Profile: you@example.com (Acme, pro)
It is now the active Account. Claude Code 2.1.221.

Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

`perch status --json` prints the same information, with an observation time on
every Utilization figure.

Neither form touches the network unless you ask it to: Utilization is served
from cache with its age shown, so `perch status` is cheap enough to put in a
shell prompt (ADR 0015).

`perch add` gains an Account by running a login in a Profile of its own, so the
Account you are using stays active and its session is untouched (ADR 0009).

```
$ perch add --group work --alias overflow
```

`--group <name>` says which Group the new Account joins, and `--no-group` says
it joins none. One of the two is required where there is no terminal, because
the Group is otherwise a question — the Account's organization is offered as a
default for you to confirm — and a script has nobody to answer it. `--alias
<name>` names the Account in the same breath, so it never has to be typed as an
email address.

## Reading current Utilization

`perch status --refresh` is the one command that fetches. Everything else — and
`status` without the flag — reads the figure Perch last observed and says how
old it is.

```
$ perch status --refresh
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   5-hour    42%  (as of just now)
              7-day     18%  (as of just now)
              7-day-opus  3%  (as of just now)
```

Every Quota Window an Account has is recorded — the five-hour window, the
seven-day one, and one per model — each with how full it is and when it next
resets. `--refresh` reads the Accounts it is about to show you and no others:
`perch status --refresh` reads the one you are on, `perch status --group
--refresh` reads the ones it offers as landing places. Anthropic allows roughly
28-30 reads an hour per Account and the allowance does not refill early
(ADR 0015), so nothing spends one on an Account you did not ask about.

Reading an Account's Utilization needs a valid access token for it, so an
Account whose token has expired has its Credential renewed first — but only when
no client is running against that Profile (ADR 0005). Anthropic retires a
refresh token when it issues a new one, so renewing a Credential a running
Claude Code is holding in memory would log that session out silently, mid-task.
The Rotated Credential is written back into its own Profile under the same locks
a Switch takes.

Nothing about a refresh turns `status` into a failure. A throttled read, an
Account whose Credential Anthropic will not accept, one whose Profile is in use
— each is reported by name and leaves that Account's cached figure standing,
while every other Account is still read. `--json` carries the same under
`refresh`, which is `null` when no refresh was asked for.

## Switching

`perch switch <target>` makes an Account active everywhere — every terminal, the
editor extension, the desktop app — with no login flow.

```
$ perch switch overflow
`overflow` is an Alias for overflow@example.com.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour    12%  (as of 4m ago)
```

It is three steps in one order and never another (ADR 0006). The Credential you
are leaving is **Captured** back into its own Profile first, because Anthropic
retires a refresh token whenever it issues a new one — so the copy in an
Account's Profile is several Rotations behind by the time you switch away, and
skipping the Capture would quietly poison the Account you are leaving. Then the
incoming Credential is written to the Default Profile. Then the `oauthAccount`
block of `.claude.json` is patched to match, and only that block: your project
history, MCP configuration and settings live in the same file and belong to you
rather than to the Account (ADR 0001).

All three run inside Claude Code's own OAuth refresh locks, taken in Claude
Code's order — the refresh lock, the legacy config-home lock, then the config
file lock — so a refresh cannot land between the Capture and the write. A lock
somebody is holding is waited on and then given up on; one whose holder died is
taken over.

Nothing else moves. Your memory, settings, plugins and project history are
Shared State: a Switch leaves them untouched, which is what makes them follow
you across Accounts for free.

A Switch onto a Profile a client is already running against is refused with exit
code 16 rather than writing a Credential something else is holding, and
switching to the Account that is already active does nothing and exits 15. If a
Switch fails part way, it says which Account is active now and what is where —
running it again finishes the job.

## Cycling

`perch switch` with no target picks the Account for you. It is the command you
type mid-task when quota just ran out, so it asks nothing, under any
circumstances (ADR 0011).

```
$ perch switch
Cycling within Group `work`.
overflow@example.com has the most room: 60% headroom, which is true of every one of its Quota Windows — 7-day is its fullest, as of 4m ago.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
That figure is what Perch last observed rather than what Anthropic says now. If overflow@example.com turns out fuller than it implied, the figure was stale — `perch status --refresh` reads a current one.
```

It Cycles **within the current Account's Group** and never leaves it, so a work
subscription running dry does not land you on your personal Account. `perch
switch <group>` Cycles within a Group you name instead.

Each Account is ranked by its **worst** Quota Window, and the Account whose
worst is best wins (ADR 0012). Being blocked by any window blocks you
completely, so that is the only ranking that measures what actually stops you
working: the headroom Perch reports is true of every one of that Account's
windows. Exhausted, disabled and Quarantined Accounts are never chosen, and an
Account nobody has ever read a figure for is ranked below every Account that
has one — no figure and plenty of room are opposite pieces of advice.

How headroom is measured is fixed; which Account to prefer is the Group's to
say. `perch config set <group> strategy soonest-reset` makes a Cycle take the
Account whose fullest window comes back soonest instead of the one with the
most room, so perishable quota is spent rather than wasted — see
[Configuration](#configuration). It reorders the Accounts that have room and
nothing else: an exhausted Account is never chosen however soon it resets.

Ranking reads the cache and never the network, so the figures can be minutes
old. Landing on an Account that turns out fuller than they implied is the cache
being stale rather than the Switch going wrong, which is why every Cycle says so
before you find out.

Three outcomes are honest non-outcomes. They perform no Switch, explain
themselves, and exit with a code of their own rather than pretending to have
worked.

```
$ perch switch
Cycling within Group `work`.
Every Account in Group `work` is exhausted, so there is nowhere useful to Switch. Nothing was changed.
you@example.com frees up soonest, at 2026-08-04 15:00 UTC (in 3h).   # exit 17

$ perch switch
Cycling within Group `work`.
you@example.com is already the best Account in Group `work`, with 90% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of 4m ago. Nothing was changed — `perch status --group --refresh` reads current figures.   # exit 15

$ perch switch
you@example.com is in no Group, so nothing has declared which Accounts it is interchangeable with. Nothing was changed.
Either put it in a Group with `perch group move you@example.com <group>`, or declare that every ungrouped Account is interchangeable with `perch config set cycle-ungrouped true`.   # exit 18
```

That last one is ADR 0017. An Account need not be in a Group — adoption leaves
the first one ungrouped — but being ungrouped is the *absence* of a declaration
that Accounts are interchangeable, not a weaker form of one. So bare `perch
switch` Cycles among ungrouped Accounts only when a global setting says it may,
and that setting is off until you turn it on.

## Watching

`perch watch` does the Cycling for you. It is a loop in this terminal that
reads how full the Account you are on is, says what it made of that, and
Switches within the Group when the Account runs low — so you stop being the one
who notices.

```
$ perch watch
Watching you@example.com in Group `work`. Reading how full it is every 2m30s, and Switching within the Group when its fullest Quota Window reaches 80% — to an Account at 70% or under, and never twice inside 15 minutes. Ctrl-C stops.
2026-08-04T12:00:00Z  waiting   you@example.com 40% used, fullest 5-hour; threshold 80% — under it, so nothing was wanted.
2026-08-04T12:02:30Z  switched  you@example.com 86% used, fullest 5-hour; threshold 80% — over it. Switched — overflow@example.com has the most room: 95% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of just now.
^C
Stopped. Nothing was left behind: the watcher holds no lock, writes no file of its own, and the Account you are on is the one it last Switched to.
```

It is **not a daemon** (ADR 0013). There is nothing to install, nothing to
manage, and nothing left behind when it stops: it holds no lock and takes no
marker across a wait, so Ctrl-C is safe wherever it lands. A Ctrl-C during a
Switch lets that Switch finish first. Wanting it truly unattended is what
[`--once` and your own scheduler](#watching-on-a-schedule) are for.

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
up. It is not the cooldown — that paces Switches you have said the watcher may
make, and this paces questions nobody is answering — and unlike the cooldown it
is not configurable: like the interval, it is arithmetic about Anthropic's
allowance rather than anybody's preference.

**It does not ping-pong.** Two Accounts hovering either side of the threshold
would otherwise trade places every couple of minutes, each Switch costing a
Capture and a Credential write for a few minutes of headroom. Three rules from
the Group's configuration stop it, and each one is printed when it is what
decided a round:

- a **margin** — `watcher-margin-percent`, 10 points by default — under the
  threshold, which nothing is moved to unless it clears. At an 80% threshold
  that means nothing above 70%.
- a **cooldown** — `watcher-cooldown-minutes`, 15 by default — between one
  Switch and the next, whatever the figures do in between. A five-hour window
  moves slowly enough that fifteen minutes never misses a real crossing.
- **no return** — `watcher-no-return`, on by default — which keeps the Account
  a Switch just left off the candidate list for one cooldown. It is not even
  read: a read for a choice that cannot be made is an allowance spent on
  nothing.

```
2026-08-04T12:02:30Z  nowhere   you@example.com 86% used, fullest 5-hour; threshold 80% — over it, and nowhere to go: Nothing in Group `work` is worth Switching to yet — overflow@example.com is at 74% used and nothing over 70% is worth moving to. Nothing was changed.
2026-08-04T12:05:00Z  cooling   you@example.com 86% used, fullest 5-hour; threshold 80% — over it, and too soon to move again: the last Switch was 2 minutes ago and this Group's cooldown leaves at least 15 minutes between two, so nothing moves for another 12 minutes.
```

An Account Perch has never read a figure for is set aside the same way. A
[Cycle](#cycling) you asked for will land on one — an unknown beats a window
that is certainly full — but unasked it would be a move onto an Account the
watcher knows nothing about, and no figure is not evidence of room.

None of it survives the loop. The cooldown lives in the running process and
nowhere else — stopping `perch watch` and starting it again is you saying "go
on then", and it starts with nothing to wait for. A scheduled check is the one
exception, and [it says why below](#watching-on-a-schedule).
`watcher-no-return` is measured in cooldowns, so a Group with
`watcher-cooldown-minutes 0` has no no-return either, whatever it is set to.

What it does when it acts is a Switch and nothing else: the outgoing Credential
is Captured into its own Profile first (ADR 0006), Claude Code's locks are
taken, and a Live Profile's token is never Renewed (ADR 0005). Running while
Claude Code is working is the normal case, not the exception.

Two things stop it, at the first round or at any round after, because there
would be nothing for it to do:

```
$ perch watch
you@example.com is in no Group, so nothing carries permission for the watcher to act on it. Nothing is being watched.
Put it in a Group with `perch group move you@example.com <group>`, then let the watcher act on that Group with `perch config set <group> watcher-may-act true`.   # exit 18

$ perch watch
Group `work` has not been told the watcher may act on it, so nothing is being watched. A Group only ever changes underneath you because you said it could.
`perch config set work watcher-may-act true` says it may.   # exit 14
```

`cycle-ungrouped` grants the watcher nothing. Permission to Switch **when you
ask** and permission to Switch **while nobody is looking** are different grants,
and the second has no owner when there is no Group to carry it.

Both permissions are read every round rather than only at the first, because
either can be taken back while the watcher is sleeping. A `perch switch` in
another terminal that leaves an ungrouped Account active, or a
`perch config set work watcher-may-act false`, stops a watcher that is already
running — on the message and exit code above, the ones it would have refused to
start on, rather than leaving it idling having decided it may do nothing.

### Watching on a schedule

`perch watch --once` takes one check and exits, saying what it decided in its
exit code. That is the whole of what an unattended watcher needs, because
scheduling is the operating system's job (ADR 0013): cron and systemd timers
already run things at an interval, keep them from overlapping, and capture what
they printed.

```cron
*/5 * * * * perch watch --once >> ~/.local/state/perch-watch.log 2>&1
```

```
$ perch watch --once
2026-08-04T12:00:00Z  switched  you@example.com 86% used, fullest 5-hour; threshold 80% — over it. Switched — overflow@example.com has the most room: 95% headroom.   # exit 0

$ perch watch --once
2026-08-04T12:05:00Z  cooling   overflow@example.com 90% used, fullest 5-hour; threshold 80% — over it, and too soon to move again: the last Switch was 5 minutes ago and this Group's cooldown leaves at least 15 minutes between two, so nothing moves for another 10 minutes.   # exit 15
```

It is the same policy as the loop, run once — the same threshold, cooldown,
margin and no return, and the same refusal to act on a figure it did not just
read. **The cooldown and the no return survive between invocations**, because
each `--once` is a fresh process and the sequence of them is the watcher: what
one check Switched, and when, is recorded against the Group in the registry for
the next one to be paced by. That is the one thing about the watcher that is
written down, and it is why a check every minute still Switches no more often
than the Group allows. The loop keeps the same two facts in memory instead —
two loops would be two people watching, and one pacing the other's decisions is
not what either of them asked for.

The exit codes are the [table below](#exit-codes), and a check reaches five of
them:

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
held back by the cooldown or the no return is `15` — there was nothing to do
*now* — and which of the rules held it is on the line rather than in the code,
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

## Running one Account in one terminal

`perch run <target>` launches Claude Code as an Account without changing which
one is active. It is the other half of `switch`: a Switch is about the whole
machine, and a Run is about one process.

```
$ perch run overflow
`overflow` is an Alias for overflow@example.com.
Running Claude Code as overflow@example.com (as `overflow`), in this terminal alone. you@example.com stays the active Account everywhere else.
```

It works by setting `CLAUDE_CONFIG_DIR` for that one process (ADR 0010). Nothing
is Captured, nothing is written to the Default Profile, and no Identity is
patched — so every other terminal, the editor extension and the desktop app go
on as the Account they were on. Two terminals running two Accounts is what the
command is for, not an edge case.

Because a Run uses a Profile as a live configuration directory rather than as
storage, it is the one path that has to **Reconcile** first, every time: your
memory, settings, plugins, past work and plans are linked into the Profile it is
about to launch, and links that have broken or gone stale are repaired (ADR
0026). What crosses is everything the Default Profile holds except the
Credential, the file naming the Account, the directory of session markers and
the refresh lock, read at Run time — so a directory a new Claude Code release invents follows you
without waiting for a Perch release. Never by copying, because a copy diverges
the moment it is edited: where no link can be made, the Run is refused rather
than served one, naming the entry and what to do about it.

One file cannot be linked. `.claude.json` holds the Account itself, so every
Profile keeps its own — and it also holds a good deal that is yours: whether you
have been through onboarding, which tips you have seen, and the trust and tool
approvals of the repository you are standing in. So that one file is **Carried**
key by key instead, from the most recently used Profile in the same Group, and
only into a Profile nothing is running against (ADR 0003). What crosses is a
named list rather than everything-but, because this file also holds figures
Anthropic gave for one Account — carrying those would show you one Account's
Utilization under another Account's name. Without it, the first Run of a new
Account lands you in a Claude Code that believes it has never been used, asking
for trust in the middle of your task.

A Group is not a Target here — it names a set of Accounts rather than one, and
there is no single Profile to point a process at — and an Account that is
Quarantined is refused with exit code 19 rather than launching a client that
would ask you to log in. The client's own exit code is Perch's, so `perch run`
can stand in a script wherever `claude` would.

### What a Run protects while it lasts

For as long as a Run is running, the Profile it launched is a **Live Profile**,
and Perch will not write into one (ADR 0027). Another terminal cannot Capture
into it, cannot Renew the Credential that client is holding — which would retire
the refresh token and log it out mid-task — and cannot copy `.claude.json` keys
over it.

Each of those refuses in the register its own command has. A Switch that cannot
Capture stops, exits 16, and names the process holding the Profile. A
`--refresh` shows you the cached figure instead and still succeeds, because a
refresh reports what it could not read rather than failing (ADR 0018). A
`.claude.json` key simply does not cross, because nothing on that path may
refuse a Run (ADR 0003) — the cost is one onboarding question, not a session.

Reading is untouched, which is the difference that matters. `perch switch` onto
the Account you are running lands normally: it copies that Credential into the
Default Profile and leaves the Profile itself alone. Its Utilization is read
without renewing anything, because an Account with a client running has a fresh
access token already. A Run and a Switch do not lock each other out.

It works by the marker Claude Code already uses to record a running client, so
a Run that was killed rather than closed leaves nothing behind that matters: a
marker naming a process that is gone, or a pid since taken by something younger,
makes no Profile Live (ADR 0022).

### Running with arguments, and running something else

Everything after `--` belongs to the program rather than to Perch, and reaches
it exactly as you typed it — including flags Perch has of its own.

```
$ perch run overflow -- --resume --model opus
$ perch run overflow -- npm test
```

The first word after `--` decides which program runs. A flag is Claude Code's,
so the first line resumes a session; anything else is the program to launch, so
the second runs `npm` with your Shared State reachable and `CLAUDE_CONFIG_DIR`
pointed at the Account's Profile. Nothing is guessed either way: a program you
could invoke by name never begins with a `-`.

`--` is required, and a flag typed without it is refused rather than claimed:

```
$ perch run dev --resume
`--resume` could be Perch's flag or the program's, and Perch will not guess which. Everything meant for the program you are running goes after `--`:

    perch run dev -- --resume

$ echo $?
2
```

Both readings of that line are real — Perch has a `--json` and so does Claude
Code — so Perch takes neither and hands you back the line that would have
worked. A program typed without the separator (`perch run dev npm test`) is told
the same thing: nothing but `--` follows a Target.

## Reserving an Account

`perch disable` keeps an Account out of Cycling without giving it up — for the
subscription you are holding for one particular thing and would rather Perch did
not spend on something else.

```
$ perch disable spare
`spare` is an Alias for spare@example.com.
Disabled spare@example.com (as `spare`). Cycling will not choose it — it stays listed and named, and `perch switch` still switches to it when you name it.

$ perch enable spare
`spare` is an Alias for spare@example.com.
Enabled spare@example.com (as `spare`). It is a Cycle candidate again.
```

A disabled Account is excluded from Cycling and from nothing else. It keeps its
Alias, its Group and its stored Credential, `perch list` shows it as disabled,
and naming it on `perch switch` still switches to it — so putting it back needs
no login, only `perch enable`. Removing the Account is the blunt instrument this
exists to avoid.

Disabling every Account in a Group is allowed. A bare `perch switch` there then
reports having no candidate (exit 17) rather than quietly landing you on
something you had reserved.

## When an Account breaks

A Credential can stop working for good: Anthropic retires a refresh token, a
Rotation is lost between two writes, a login is ended somewhere else. Perch
never drops such an Account — an Account that vanishes reads as data loss, and a
broken one reads as something needing attention. It is **Quarantined**: still
listed, still named, shown as broken, and shown with the reason.

```
$ perch status --refresh
overflow@example.com is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

Cycling never chooses a Quarantined Account, and naming one on `perch switch` is
refused with exit code 19 rather than making a Credential live that does not
work — which would cost you the Account you are on. Enabling one does not repair
it: whether Cycling may choose an Account and whether its Credential works are
separate facts with separate fixes, so both are always said.

`perch relogin <target>` repairs it, and repairs it **in place**.

```
$ perch relogin overflow
`overflow` is an Alias for overflow@example.com.
Logging in again to repair overflow@example.com. someone@example.com stays active and its session is untouched.
Quit Claude Code when the login is done to come back here.

Repaired overflow@example.com (as `overflow`) — it is no longer Quarantined, and is a Cycle candidate again if it was one before.
Alias:   overflow
Group:   work
Cycling: may choose it
```

The Account keeps its Alias, its Group, whether Cycling may choose it and its
place in the listing — only the Credential is replaced. The login runs in a
directory of its own, so the Account you are working in is untouched throughout,
including when the login is abandoned, which changes nothing at all. A login as
a different Account is refused: an Alias you chose for one Account is not handed
to another because a browser was signed into somebody else.

Relogging in the Account you are **on** also makes its fresh Credential the live
one, because a repair only its own Profile can see would leave the Account broken
everywhere it is actually used (ADR 0023). A healthy Account may be relogged in
too — nothing about the command depends on the Quarantine.

## Giving up an Account

`perch remove <target>` is for the subscription that has been retired. It forgets
the Account and deletes the Credential Perch holds for it, so it stops being
listed, stops being a Cycle candidate, and the Alias it answered to comes free.

```
$ perch remove spare
`spare` is an Alias for spare@example.com.
Removed spare@example.com (as `spare`). The Credential Perch held for it is deleted, and nothing lists it or Cycles to it now.
The Alias `spare` is free to use again.
```

Removing the Account you are **on** is the case that needs care, because the live
Credential belongs to it. Perch names the Account it will leave active, lands on
it first, and asks before any of it happens (ADR 0024).

```
$ perch remove work
`work` is an Alias for someone@example.com.
someone@example.com (as `work`) is the active Account. overflow@example.com (as `overflow`) will be made active first, so nothing is left running as an Account Perch has forgotten — `perch switch <target>` first if you would rather land somewhere else. The login being given up goes with it: holding it again would mean `perch add`.
Remove someone@example.com (as `work`)? [y/N]: y
overflow@example.com (as `overflow`) is the active Account now — its Credential is the live one.
Removed someone@example.com (as `work`). The Credential Perch held for it is deleted, and nothing lists it or Cycles to it now.
The Alias `work` is free to use again.
```

The Account it lands on is one in the same Group where there is one, because a
Group is your own statement that those Accounts are interchangeable — never a
Quarantined Account, whose Credential does not work, and never a disabled one,
which is an Account you have said should not be chosen for you. It is not ranked
on how full it is: it is named before you agree to it, and `perch switch` is how
you choose differently.

Removing the last Account, or the active one when nothing is left that Perch
would land on, is allowed and confirmed the same way. It says that Perch will
hold no active Account afterwards, and it does not log you out: the live
Credential is not Perch's to take away, but the copy Perch holds is deleted, so
whatever replaces the live one ends that login for good.

`--yes` agrees in advance. Without a terminal and without the flag, a removal
that would have asked is refused rather than assumed, and end of input is a no.
The Group the Account was in stays declared — a Group is something you said, not
a summary of where the Accounts happen to be.

## Backing up everything

`perch export <path>` writes everything Perch holds to one encrypted file: the
whole registry — every Account, its Alias, its Group, whether Cycling may choose
it, why it is Quarantined where it is, and what each Group carries — alongside
every Credential. A dead machine, a mistaken `perch remove` or a new laptop then
costs you a file rather than a login for every subscription (ADR 0014).

```
$ perch export ~/perch-backup.age
This file holds a working Credential for every Account Perch has. It is encrypted with a passphrase you choose, and there is no way into it without one.
Passphrase:
Again:
Exported 3 Accounts to /Users/someone/perch-backup.age, with everything the registry says about them: their Aliases, their Groups, whether Cycling may choose them, and what each Group carries.
Keep the passphrase somewhere that is not beside the file. Without it there is nothing in there, and nothing Perch holds can get it back.
```

**It takes everything and has no target.** There is no per-Account and no
per-Group form, because a selective export is a partial restore — which is the
failure the file exists to prevent, wearing a feature's clothes.

**The passphrase is required rather than offered**, prompted, confirmed, and
never shown as you type it. It cannot be passed as an argument and there is no
flag that answers ahead of time: an argument sits in the process table for
anything on the machine to read, and in a shell history afterwards. So this is
the one command in Perch that a script cannot drive — without a terminal it is
refused, and the refusal names the terminal rather than a way round it.

**The file is an `age` file**, in `age`'s text encoding, so the standard `age`
tool reads it on a machine that has never heard of Perch:

```
$ age --decrypt ~/perch-backup.age
Enter passphrase: 
{"version":1,"registry":{ ... },"credentials":{ ... }}
```

A forgotten passphrase means the export is gone and logging in again is the only
way back. That is the correct trade for a file holding every Credential at once.

Nothing is Renewed and nothing Rotated on the way — an export reads what is
stored — and an Account something is currently running against is read like any
other, because only *writing* into a Live Profile is refused. A store that is
there and will not say what it holds stops the whole export rather than shrinking
it: an export that quietly left one Account out would only be found wanting on
the day it was needed. An Account whose stores hold nothing is still exported,
Quarantine reason and all, and the command says which.

The path is read as somebody's typing rather than as an instruction. Nothing is
written over — a path that is already taken is refused, and checked again after
the passphrase, so a file that arrived while you were typing is safe too — and a
directory that is not there is refused rather than created, because one Perch
made for a path you typed would be a directory you did not ask for, at
permissions you did not choose.

## Moving to another machine

`perch import <path>` is the exact inverse: it puts the whole registry and every
Credential back, so a new laptop arrives with the setup the old one had rather
than a pile of nameless logins.

```
$ perch import ~/perch-backup.age
This file is encrypted with the passphrase it was written with. Nothing is restored until it opens.
Passphrase:
Imported 3 Accounts from /Users/someone/perch-backup.age, with everything the registry said about them: their Aliases, their Groups, whether Cycling may choose them, and what each Group carries.
Nothing is active: an Import restores what Perch holds and does not touch what Claude Code is logged in as. `perch switch <target>` makes one of them active.
```

**It refuses a Perch that already holds an Account**, and names `perch purge` as
the way to make room. Merging is where every hard case lives — the same Account
on both sides one Rotation apart, with no way to tell which Credential is live;
an Alias meaning different Accounts on two machines; a Group that exists in both
with different members. That is a real feature and it is not this one. Refusing
keeps an import the exact inverse of a purge, and that pair is what makes moving
machines true. There is no `--force`, because a flag would be the merge wearing a
shortcut's clothes.

**Credentials land where this machine keeps one.** The file records a Credential
against an email address and nothing about the store it came out of, so an export
taken on a Mac restores into files on Linux and the other way round, without
either side knowing about the other's store (ADR 0020).

**Nothing is made active.** The Account that was active where the export was
taken is a fact about that machine, and an import writes nothing into this one's
Default Profile — whatever Claude Code is logged in as goes on running until you
`perch switch`. The login that is already on the machine is not adopted either:
it is left exactly where it is, to switch onto or to ignore.

The passphrase is required and prompted once — a wrong one fails before anything
at all is written, and is told apart from a file that is not an export. Without a
terminal it is refused for the same reason `perch export` is, and there is no
flag.

An import that fails part way leaves nothing behind: every Profile it made comes
back out, so there is no half-populated registry and no orphaned Profile, and the
file can simply be imported again. A registry inside the file written by a newer
Perch is refused rather than half-read. An Account the export carried no
Credential for is still restored, Quarantine reason and all, and the command says
which — `perch relogin` is what ends that.

## Giving the machine back

`perch purge` is the other half of that pair: every Profile, every Credential
Perch holds and Perch's own registry, gone in one act, so the machine is the one
you had before you installed it. It is what makes room for an import, and what
you run on the laptop you are handing on.

```
$ perch purge
Perch holds 3 Accounts: someone@example.com, overflow@example.com, spare@example.com.
A Purge deletes every one of their Profiles, every Credential Perch holds for them, and /Users/someone/.config/perch itself. Nothing undoes it: only a fresh login brings an Account back, and it comes back as a new one.
Claude Code goes on running as whatever it is logged in as — the live Credential is not Perch's to take away.
Write an Export first? [Y/n]: y
Where to write it: /Users/someone/perch-backup.age
This file holds a working Credential for every Account Perch has. It is encrypted with a passphrase you choose, and there is no way into it without one.
Passphrase:
Again:
Exported 3 Accounts to /Users/someone/perch-backup.age, with everything the registry says about them: their Aliases, their Groups, whether Cycling may choose them, and what each Group carries.
Keep the passphrase somewhere that is not beside the file. Without it there is nothing in there, and nothing Perch holds can get it back.
Type `purge` to give the machine back: purge
Purged 3 Accounts. Every Profile, every Credential Perch held and /Users/someone/.config/perch are gone, and Perch is holding nothing on this machine.
Claude Code is still logged in as whatever it was: the live Credential was not Perch's to take away. `perch import` puts an Export back on a machine like this one.
```

**It takes no target.** Giving up one Account is `perch remove`, which is
deliberately narrow — two verbs for one act is exactly the ambiguity the shared
Alias and Group namespace exists to prevent, so a purge does not accept one.

**It offers an export first, and takes no for an answer.** Perch cannot verify
that an export exists — the file is wherever you put it, possibly on another
machine — so requiring one is a check that checks nothing. Offering to write one
is not: it is a keystroke, and it is the only thing that makes a purge
survivable. An export that cannot be written — a path that is taken, a directory
that is not there, a passphrase mistyped — stops the purge with everything still
where it was, and a path *inside* `~/.config/perch` is refused, because that file
would go with the purge that offered it.

**The prompt wants the word.** It lists the Accounts by email address rather than
by Alias — what is being given up is the login, and that is the address you would
check against your password manager — says that nothing undoes it, and wants
`purge` typed out. A `y` is what fingers answer before eyes have read anything.
`--yes` answers ahead of time and writes no export, which is the whole of what a
script can be asked for; without a terminal and without the flag, a purge is
refused and end of input is never agreement.

**Whatever Claude Code is logged in as is left exactly where it is.** The live
Credential in the Default Profile is Claude Code's own rather than a copy Perch
holds, and a purge that logged you out of the tool you are using would be doing
more than giving the machine back. A purge is refused while a client is running
against one of the Profiles, for the same reason every other write into a Live
Profile is (ADR 0005) — a purge deletes those directories, and what is in them
belongs to whatever is holding them. That is checked before the questions and
again after them, because a client started while you were typing was not running
when the first check ran.

The Credentials go first and the registry naming them goes last, which is what
makes a purge that stopped part way finishable: a keychain item lives outside
`~/.config/perch`, so the record of which items there are is the last thing to
go. A purge that failed anywhere — a store that would not give its Credential up,
a directory that would not go — leaves the registry standing, and running it
again finds what it already deleted already gone and carries on.

## What you have

`perch list` is the one place that answers it: every Account with its Alias, its
Group, whether it is a Cycle candidate, and how full it is.

```
$ perch list
  Account               Alias     Group  State                 Utilization
* someone@example.com   -         work   enabled               5-hour    42%  (as of 3m ago)
                                                               7-day     18%  (as of 3m ago)
  overflow@example.com  overflow  work   enabled, quarantined  never observed
  spare@example.com     -         none   disabled              5-hour    91%  (as of 2h ago)

* is the active Account.
overflow@example.com (as `overflow`) is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

An Account nobody has ever read a figure for says `never observed` rather than
`0%` — no figure and plenty of room are opposite pieces of advice. A
Quarantined Account stays listed and named, so an Account needing attention is
never mistaken for one that vanished; whether it is in the Cycling pool is said
alongside, because enabling a Quarantined Account would not repair it. The
reason it broke is written out under the table rather than squeezed into a
column, with the one command that puts it right.

`perch status --group` is the same view narrowed to the Group the active
Account is in, so you can see where you would land before you switch. From an
Account in no Group it shows every ungrouped Account and says that Cycling will
not move between them until you say it may (ADR 0017).

`perch list --json` and `perch status --group --json` carry the same
information, with an observation time on every figure and the scope they were
narrowed to. Neither makes a network call. `quarantined` is `null` for an
Account that works and an object — `reason` and `detail` — for one that does
not, so a script asking whether it is set reads the same answer it always did
and now gets the reason with it. `--group` changes the question, so
it changes the document: `perch status --json` answers about one Account under
`active`, while the listings answer about a set under `accounts`, with the
active one named under `active_account`. An Account itself is described the same
way in both — the same keys, the same answers — so a script written against one
can be pointed at the other.

## Choosing by eye

`perch tui` opens the interactive view: the Accounts and their Utilization, in
two tabs.

```
$ perch tui
 Accounts | Utilization                             active: someone@example.com
   Account               Alias     Group  State     Headroom
>  overflow@example.com  overflow  work   enabled   93%
 * someone@example.com   -         work   enabled   58%
   spare@example.com     -         none   disabled  never observed

q  quit   Tab  view   Up/Down  move   Enter  switch   x  run   r  refresh
```

`Tab` and the left and right arrows move between the views, the up and down
arrows move the cursor, `q` and Ctrl-C leave, and `r` reads current Utilization.
`>` is the row the keys act on and `*` is the active Account — characters rather
than colours, so the view reads the same over SSH on a terminal that has none.

The Accounts are listed in the order `perch switch` would rank them, with the
Headroom they were ranked on beside them, so the ranking is visible rather than
hidden. Cycling never leaves the Group it started in (ADR 0002), so the listing
is one ranking per Group, with the Group the active Account is in first. A
Disabled or Quarantined Account is listed like any other and sorts below every
Account a Cycle would choose — they are shown, and shown as out of the running.
The Account at the top of a Group is the one a Cycle within it prefers, which is
where a bare `perch switch` lands unless you are already on it — landing where
you already are is the one thing a Cycle will not do.

The Accounts in no Group come last, and are listed in the order they were added
rather than ranked. Being ungrouped is the absence of a declaration that
Accounts are interchangeable rather than a weaker form of one, so `perch switch`
refuses to Cycle among them until `perch config set cycle-ungrouped true` says
it may (ADR 0017) — and a ranking of Accounts Perch would not choose between is
exactly the hidden claim this listing exists not to make. Their Headroom is
still shown; it is a figure rather than a running order. Set `cycle-ungrouped`
and they rank like any Group.

The Utilization view is the same Accounts in the same order, with their figures
at the two levels there are honest figures for. The order is shared because the
cursor is: two orders would make `Tab` move what `Enter` acts on.

```
 Accounts | Utilization                             active: someone@example.com
Group `work`
  Reserve: 2 of 2 Accounts have Headroom, the best 39% left (as of 4m ago)
  5-hour   emptiest   7% used across 2 Accounts (as of 4m ago)
  7-day    emptiest  61% used across 2 Accounts (as of 4m ago)

>  overflow@example.com   Headroom 39%  (7-day is its fullest, as of 4m ago)
      5-hour     7% used  resets 2026-08-04 14:00 UTC (in 2h)  (as of 4m ago)
      7-day     61% used  resets 2026-08-08 12:00 UTC (in 4d)  (as of 4m ago)

 * someone@example.com    Headroom 18%  (7-day is its fullest, as of 4m ago)
      5-hour    42% used  resets 2026-08-04 15:00 UTC (in 3h)  (as of 4m ago)
      7-day     82% used  resets 2026-08-06 14:00 UTC (in 2d)  (as of 4m ago)

In no Group
  Cycling only moves between these when you say it may.

   spare@example.com      Headroom never observed

q  quit   Tab  view   Up/Down  move   Enter  switch   x  run   r  refresh
```

Each Account gets a block rather than a line, because it has several Quota
Windows at once and is limited by whichever fills first: one line per Account
would have to pick one of them, and the one it picked would be the one hiding
the other. Each window says its fill, when it comes back and how old the reading
is — 90% that resets in twenty minutes and 90% that resets in four hours are the
same number and opposite advice. An Account nobody has ever read a figure for
says so rather than showing a zero; "no figure" and "plenty of room" are
opposite pieces of advice.

Beside each Account's name is its **Headroom**: how much is left to spend, taken
from its most constrained window and naming which, so an Account is only ever as
free as its fullest window and a generous-looking figure never hides an
exhausted one (ADR 0012). It is the figure the ranking on the Accounts view was
made on, and the rows underneath are what it can be checked against.

Above each Group is its **Reserve**: what the Group has left to draw on, said as
how many of its Accounts still have Headroom and how much the best of them has.
A Disabled or Quarantined Account is not part of it — a Credential that does not
work is quota nothing can spend — and where a Group holds one it says so on a
line of its own rather than dropping it from a count that would then not add up
to the Accounts on screen. Under the Reserve, one row per Quota Window kind: the
emptiest Account in that window, which is the best the Group can currently offer
there. Fill and room are both percentages and are never left to be told apart by
context: a Reserve says how much is *left* and a window row says how much is
*used*, and the row says the word. That is what answers "fine on the weekly window, empty on the five-hour
one", which is the case a single figure per Account hides.

**There is no total, anywhere.** Never one pooled figure across Accounts.
Accounts sit on different plans and Perch only ever sees percentages — a `pro`
Account at 50% and a `max` Account at 50% do not have the same quota left, and
Perch never sees the allowance behind either. Summing or averaging them produces
a number that looks quantitative, isn't, and is exactly the kind of number
people plan around. So every percentage on this view is one an Account actually
reported, and the per-window rows are the only figure drawn across a Group's
Accounts at all, because within one window kind the comparison at least means
something.

The Accounts in no Group get a heading and no Reserve. Being ungrouped is the
absence of a declaration that Accounts are interchangeable (ADR 0017), and a
figure for what a set has left to draw on is a figure about a set — so it
appears once `perch config set cycle-ungrouped true` says a Cycle may move
between them, and not before.

### It acts, and acts on exactly two things

`Enter` Switches to the Account under the cursor and `x` Runs it. That is the
whole of what the view does, and it is deliberate: ADR 0011's justification for
an interactive view at all is making a choice, and both of those have plain
command forms, so nothing becomes TUI-only.

`add`, `remove`, `purge` and `config` stay out. A keystroke away from an
irreversible act is the wrong ergonomics for the one surface being navigated by
arrow key, so nothing destructive is reachable here at all.

A Switch from the view is the same Switch as everywhere — the outgoing
Credential Captured first, Claude Code's locks taken, the Identity patched — and
what `perch switch` would have printed is what appears above the keys. Switching
to the Account that is already active says there is nothing to do rather than
rewriting Credentials for nothing.

A Run hands the terminal over: the view ends, the terminal goes back, and Claude
Code starts against that Account's Profile with the active Account untouched. It
returns when the client exits, with the status the client exited with — the same
as `perch run`, because it is `perch run`.

Selecting a Quarantined Account names `perch relogin` rather than failing
obscurely, whichever of the two keys was pressed, and nothing is changed.

```
 Accounts | Utilization                             active: someone@example.com
   Account               Alias     Group  State                 Headroom
 * someone@example.com   -         work   enabled               58%
>  overflow@example.com  overflow  work   enabled, quarantined  93%

overflow@example.com is Quarantined: Anthropic would not renew its Credential.
Nothing was changed — switching to it would make a Credential live that no
longer works, and cost you the Account you are on. `perch relogin
overflow@example.com` logs it in again in place, keeping its Alias, its Group
and whether Cycling may choose it.
q  quit   Tab  view   Up/Down  move   Enter  switch   x  run   r  refresh
```

The first frame is drawn from cache and never waits on the network (ADR 0015),
which is why it appears at once and why every figure on it says how old it is. A
picker that hangs before showing anything is worse than one showing numbers a
few minutes old and saying so.

`r` is the only thing here that reads from Anthropic, and it is taken off the
frame loop: the
display goes on redrawing, goes on answering the keys and goes on leaving on
Ctrl-C while Anthropic is being waited on. It reads the Accounts on screen and
no others, like every other Refresh. One that fails leaves every figure standing
with the age it had and says what could not be read (ADR 0018).

Nothing here is only here. The interactive view is one command among several
rather than the primary surface (ADR 0011), so everything it shows and
everything it does has a plain command form — `perch list`, `perch status`,
`perch config`, `perch switch`, `perch run` — and where there is no terminal to
draw in, `perch tui` refuses and names them rather than trying.

The terminal is given back however the view ends: on `q`, on an error, and on a
panic. A TUI that dies in raw mode is one you have to `reset` your way out of.

## Names

An Alias is a short name for an Account, so no command ever needs an email
address.

```
$ perch alias overflow overflow@example.com
`overflow@example.com` is an Account.
`overflow` now names overflow@example.com.

$ perch alias overflow --unset
`overflow` no longer names overflow@example.com.
```

Aliases and Group names share one namespace: neither can take a name the other
already has, and two names that differ only in case are one name. So the one
Target every command takes is never ambiguous. An Account answers to one Alias
at a time — naming an Account that already has a name replaces it, and says
which name it gave up.

A Target resolves as Alias, then Account email, then Group, and the command
says which one matched before it acts. A Target that matches nothing is refused
with exit code 12 and the names it nearly matched.

## Groups

A Group is your statement that a set of Accounts is interchangeable — another
work subscription, never your personal Account. Cycling will only ever move
between Accounts in one Group (ADR 0002).

```
$ perch group add work
$ perch group move overflow@example.com work
$ perch group list
work
  Accounts     overflow@example.com (as `overflow`)
  Strategy     most-headroom
  Watcher      off (would act at 80%, onto 70% or better, at most every 15m)

In no Group
  Accounts     you@example.com
  Cycling      only moves between these when you say it may
```

`perch group move <target> none` takes an Account out of every Group, and
`perch group remove <name>` gives up the Group itself — refused while it still
holds Accounts, so the Accounts in it are never quietly left somewhere they
cannot Cycle from.

## Configuration

`perch config` changes how a Group behaves, and asks nothing: every capability
Perch has is reachable from a script, because it has to be complete over SSH
and in CI (ADR 0011).

```
$ perch config set work strategy soonest-reset
`strategy` on Group `work` is now soonest-reset.
A Cycle within `work` prefers the Account whose fullest Quota Window resets soonest, so perishable quota is spent rather than wasted. Headroom is still measured by the worst window (ADR 0012), so an exhausted Account is still never chosen however soon it comes back.

$ perch config get
cycle-ungrouped true
work strategy soonest-reset
work watcher-may-act false
work watcher-threshold-percent 80
work watcher-cooldown-minutes 15
work watcher-margin-percent 10
work watcher-no-return true
```

Three words name a Group; two do not. Most configuration belongs to a Group,
because a Group is what carries the rules governing Cycling within it (ADR
0002) — but whether bare `perch switch` may Cycle among the Accounts in **no**
Group is about Accounts with no Group to carry it, so it is global and is
addressed by naming none (ADR 0017).

| Key | Belongs to | Values | Default |
| --- | ---------- | ------ | ------- |
| `strategy` | a Group | `most-headroom`, `soonest-reset` | `most-headroom` |
| `watcher-may-act` | a Group | `true`, `false` | `false` |
| `watcher-threshold-percent` | a Group | 0–100 | `80` |
| `watcher-cooldown-minutes` | a Group | 0–10080 | `15` |
| `watcher-margin-percent` | a Group | 0–100 | `10` |
| `watcher-no-return` | a Group | `true`, `false` | `true` |
| `cycle-ungrouped` | no Group | `true`, `false` | `false` |

The **strategy** is which Account a Cycle prefers when more than one would
serve. `most-headroom` takes the one with the most room left; `soonest-reset`
takes the one whose fullest Quota Window comes back soonest, so quota that was
about to be thrown away is spent rather than wasted. How headroom is *measured*
is not configurable — it is always the worst window (ADR 0012) — so a strategy
reorders the Accounts that have room and can never promote an exhausted one.

A strategy says which figure to prefer, not which figures to invent. Cached
figures do not always carry a reset time, and `soonest-reset` ranks an Account
whose figure does not above nothing at all: an Account that says when it comes
back is preferred to one that does not, and where none of them says, the Cycle
falls back to the room it can see and says that is what it did.

The **watcher's** five fields govern [`perch watch`](#watching) and nothing
else. `watcher-may-act` says whether it may Switch within the Group at all, and
is off by default because a Group only ever changes underneath you because you
said it could. The other four are its policy:
`watcher-threshold-percent` is how much of the fullest Quota Window of the
Account you are on has to be used before it moves you;
`watcher-margin-percent` is how far under that a candidate has to sit to be
worth moving to; `watcher-cooldown-minutes` is the least it will leave between
two Switches; and `watcher-no-return` keeps the Account it just left off the
candidate list for one cooldown. None of them switches anything on: they take
effect while the loop is running in a terminal, and not otherwise.

How often it reads is deliberately *not* configurable. Two and a half minutes
is derived from Anthropic's allowance of ~28-30 reads an hour rather than from
anyone's taste, and a Group configured to read every ten seconds would be a
Group configured to spend that allowance and be refused.

A margin at or above the threshold is not refused — it is a Group that will only
move onto an Account with nothing used at all. Refusing it would make the order
you type two `perch config set`s in matter.

Every line `perch config get` prints is the tail of the `perch config set` that
would restore it, so reading the configuration and writing it back are the same
vocabulary. Naming one setting prints its value alone, for a script to read
without parsing prose; naming a Group prints what that Group carries. An unknown
key or a value that means nothing is refused with exit code 14 and the ones that
do mean something, so a script that mistyped a setting does not go on believing
it took.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed (ADR 0007) |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account, no such Group |
| 13 | it collides with something that is already there — an Account added twice, a name already spoken for, a path an Export would have written over, an Import onto a Perch that already holds an Account |
| 14 | Perch understood it and will not accept it — an ambiguous name, a value out of range, a Group that has not said the watcher may act on it, a command that needs a terminal run where there is none |
| 15 | there was nothing to do — you are already on that Account, a check found nothing to do now, or Perch is holding nothing on this machine to purge |
| 16 | refused: a client is running against that Profile, so what is in it is not Perch's to write or to delete |
| 17 | a Cycle found nowhere to land — every Account in the Group is exhausted, or none is a candidate |
| 18 | a bare Cycle, or a watcher, on an Account nobody has declared interchangeable with anything (ADR 0017) |
| 19 | that Account is Quarantined — its Credential no longer works, and `perch relogin` repairs it (ADR 0023) |
| 20 | held: a lock somebody else has, or a `perch watch --once` with no current figure to decide on (ADR 0013). Nothing is wrong and nothing was changed — ask again shortly |

`perch run` is the one command these do not describe once it has launched
something: what the client exited with is what Perch exits with, so a script
wrapping it reads the program's own code rather than Perch's. Everything that
stops a Run before the launch — a command line without `--`, an unknown Target,
a Group, a Quarantine, a Reconcile that could not be made — is in the table
above.

## Where things are

- `~/.config/perch/registry.json` — Perch's own state, versioned.
- `~/.config/perch/profiles/<account>/` — one directory per Account. Its path is what
  gives that Account a private Credential Store (ADR 0001).
- `$PERCH_HOME` overrides `~/.config/perch`. Home is `$USERPROFILE` on Windows and
  `$HOME` elsewhere; a machine that cannot say where home is gets a refusal,
  never a write into the filesystem root. `~/.config` is created if it is not
  there, and the same path is used on every platform, Windows included, rather
  than `%APPDATA%` — one rule to document and to support, and `$PERCH_HOME` for
  anybody who wants a different one.
- `$PERCH_CLAUDE_BIN` overrides where `claude` is found. Without it, Perch
  walks `PATH` itself — consulting `PATHEXT` on Windows, so the `claude.cmd`
  an npm install leaves works from every shell.

A Credential lives wherever the installed Claude Code would put it (ADR 0020):
the keychain on macOS, reached by driving `/usr/bin/security` (ADR 0008), and
a `.credentials.json` inside the Profile everywhere else — created readable by
its owner alone, and tightened if it is ever found looser. Perch drives `curl`
by absolute path — `/usr/bin/curl`, or `%SystemRoot%\System32\curl.exe` on
Windows — to reach Anthropic, with the URL, the headers and the body all
handed over on standard input: an access token passed as an argument would sit
in the process table for anything on the machine to read.

## Building

Builds and runs on macOS, Linux and Windows, with the same command surface
everywhere. The toolchain is pinned in `rust-toolchain.toml` — Rust 1.97.1,
edition 2024 — so rustup will fetch the right one on first build.

```
# touches nothing on the machine: every suite but the contract ones, which are
# held back by a feature rather than by a list somebody has to maintain
cargo test

# asserts beliefs against this machine, so it wants Claude Code installed
cargo test --features contract --test contract --test contract_credentials \
           --test contract_sessions --test contract_links

# both
cargo test --all-features
```

On macOS the contract tests read and write items of their own in the login
keychain, under `Perch contract test-*`, and delete them again. They never
write Claude Code's item. Set `PERCH_SKIP_KEYCHAIN_CONTRACT=1` to skip them
where the keychain cannot be unlocked — it is macOS-only, because only macOS
compiles those tests in. The file-store contract tests need no opt-out: they
touch only a temporary directory of their own.

## Design

`CONTEXT.md` for the vocabulary, `docs/adr/` for the decisions.

## Licence

Two of them, at your option, which is the Rust ecosystem's convention and not an
accident. MIT is the shorter and better-known of the pair but says nothing at all
about patents; Apache-2.0 grants them expressly, and cannot be combined with
GPLv2. Offering both leaves that choice where it belongs, with whoever is
downstream.

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
