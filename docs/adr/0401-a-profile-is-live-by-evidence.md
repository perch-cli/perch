# A Profile is Live by evidence

Reading an Account's Utilization needs a valid access token for it, so ranking
Cycle candidates means Renewing Credentials for Accounts nobody is using.
Anthropic Rotates refresh tokens — a Refresh may return a new one, retiring the
family — so Renewing a Credential a running Claude Code still holds in memory
logs that session out, silently, mid-task.

Perch therefore Renews only Profiles with no client running, and writes the
Rotated Credential back under the locks Claude Code takes. Nothing is lost:
Cycle candidates are idle by definition, and an Account actually in use has a
fresh access token already, so its Utilization is readable without Renewing
anything.

## A Marker is evidence, and a pid alone is not

Claude Code records each running client as `sessions/<pid>.json` in the config
directory it was launched against, and leaves the Marker behind when it dies. A
pid on its own therefore proves nothing: operating systems hand pids out again,
aggressively on Windows, so a dead session's Marker beside a reassigned pid
would make Perch declare a Profile Live and refuse a Switch with exit 16 —
permanently, until somebody deleted the file by hand. The core command would
fail with no recourse, on the side that looks like caution.

A Marker is evidence only where the process it names is alive *and* began no
later than the Marker says the session did. A recycled pid necessarily belongs
to a process that started after the Marker was written, so the check is exact
rather than heuristic.

The Marker carries two candidate fields and `startedAt` is the one compared.
`procStart` is the tempting one: on Windows it is a `FILETIME` in
100-nanosecond ticks since 1601, and something else entirely on Linux and macOS,
so matching it means three reverse-engineered encodings instead of one.
`startedAt` is plain epoch milliseconds and means the same thing everywhere, so
Perch needs only each operating system's own answer to when a process began —
`GetProcessTimes`, `/proc/<pid>/stat`, `sysctl KERN_PROC_PID`.

Three implementations of "when did this process start" is the largest single
piece of platform-specific code in Perch, for a correctness problem that is
acute on one platform and merely rare on the others. Checking the executable
name of the live pid instead is cheaper and Windows-only, and is refused for
being asymmetric about a hazard that is universal, and for still misfiring where
the recycled pid belongs to another `claude`.

The comparison allows five seconds of slack, because the two clocks it reads are
not one clock on every platform. On macOS and Windows a process's start is fixed
at creation; on Linux it is recomputed at every read, since `/proc/<pid>/stat`
gives the start in ticks since boot and the kernel derives `btime` as realtime
minus uptime — so it moves by exactly as much as anything that steps the wall
clock. Without the margin an NTP correction of a couple of seconds makes a live
process look younger than the session it has just recorded, the Marker is
dismissed, and the next Renewal Rotates the Credential the running client is
holding. The margin costs nothing in the direction the ordering is for: a pid
the operating system has handed out again belongs to a process that began a
whole session later rather than a few seconds.

A Marker that cannot be read or understood is no evidence at all — a Profile is
Live when something says so, not when nothing does. That a Marker names its
process and when it started is a named assumption, and it fails the way the
others do, loudly and by name (ADR an-assumption-is-probed).

Perch writes Markers as well as reading them, in the shape it reads, because it
makes Profiles Live too: a Run does (ADR a-run-is-one-shot), and so does a login
(ADR a-login-perch-does-not-need).

## Writing into a Live Profile is refused, reading out of one is not

The two directions do not point the same way, so both are stated rather than
discovered.

**Refused**: a Capture, a Renewal, and a `.claude.json` key write. Each writes
*into* the Profile, under a client that holds those files open and rewrites them
on its way out.

They are refused in the three registers those paths already have, and are
deliberately not leveled to one. A Capture stops the Switch with exit code 16,
because a Switch that cannot Capture must not proceed
(ADR a-switch-is-written-down-first). A Renewal degrades to the cached figure
and the command still exits 0, because a Refresh reports what it could not read
rather than failing (ADR a-figure-carries-its-age). A `.claude.json` key simply
does not cross, silently, because nothing on the Carry path may refuse a Run
(ADR everything-but-the-account) — a person who answers one onboarding question
has lost less than one who was refused a client. Only the first of the three is
a failed command, and leveling them would contradict two decisions.

A Renewal is refused for every directory the Account's Credential could be in
use from, not only the one being written. A Rotation retires the refresh token
for an *Account* rather than for a file, so every copy dies together: `perch run
<the active Account>` puts a client on that Account's own Profile while the copy
a Renewal would replace sits in the Default Profile, and Renewing it logs that
client out just the same.

**Allowed**: `perch switch` onto an Account with a Run against it. A Switch
reads that Profile's Credential and writes it to the Default Profile, leaving
the Live Profile untouched. Refusing it would make a Run and a Switch lock each
other out for no reason at all — the Account you are running in one terminal is
exactly the one you would want active in the others — so the refusal sits on the
outgoing Account, where the Capture actually lands.

One case is both at once and stays refused: `perch switch X` where X is already
the active Account, which is the repair for an interrupted Switch. X is the
outgoing Account there as well as the incoming one, so the Capture would write
into the very Profile the Run is holding. That is the outgoing rule doing its
job rather than an exception to the incoming one.

The same reasoning puts a second liveness check into `perch relogin`, after the
login rather than only before it. The one before it is minutes stale by the time
the browser comes back, and what follows it writes a fresh Credential into the
Account's own Profile — so a Run started during the login would be written
under. The login itself runs in a directory of its own, so it can never be what
that check finds.

**Allowed**: reading Utilization. An Account with a client running has a fresh
access token by this decision's own reasoning, so its figures are readable
without Renewing anything. The Watcher is not blinded by a Run.

## Consequences

The liveness ask is taken under Claude Code's locks rather than ahead of them.
It is a statement about a moment and taking a lock can take seconds, so a
`claude` started during that wait is one an earlier answer never saw.

A `sessions` directory that is there and will not be read — the root-owned one a
`sudo claude` leaves — establishes nothing, which is not the same as nothing
running against the Profile. It is told apart from Live by name, because a
caller deciding what to do next has to tell them apart.

Perch writes into a directory Claude Code owns. A Marker Perch wrote carries the
two fields that make it evidence and one saying who wrote it; it invents no
`sessionId` and no version, because a file claiming to be a Claude Code session
when it is not is worse than one that is plainly Perch's. If a future Claude
Code reads these files for anything beyond liveness, that is the assumption that
breaks.
