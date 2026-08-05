# A Live Profile is corroborated by when its process started

Claude Code records each running client as `sessions/<pid>.json` in the config
directory it was launched against, and leaves the marker behind when it dies.
Perch believed such a marker whenever the process it named was alive. On macOS
that is nearly always right. On Windows, where PIDs are recycled aggressively, a
dead session's marker plus a reassigned PID makes Perch declare a Profile Live
and refuse a Switch with exit 16 — permanently, until somebody deletes the file
by hand. The core command fails with no recourse, and it fails on the side that
looks like caution.

A marker is therefore evidence only when the process it names is alive *and*
started no later than the marker says the session did. A recycled PID necessarily
belongs to a process that started after the marker was written, so the check is
exact rather than heuristic.

The marker carries two candidate fields. `procStart` is the tempting one and was
rejected: on Windows it is a `FILETIME` in 100-nanosecond ticks since 1601, and it
will be something else entirely on Linux and macOS, so matching it means three
reverse-engineered encodings instead of one. `startedAt` is plain epoch
milliseconds and means the same thing on every platform, so Perch compares against
that and needs only each operating system's own answer to when a process began —
`GetProcessTimes`, `/proc/<pid>/stat`, `sysctl KERN_PROC_PID`.

## Consequences

This is a new named assumption in `probe.rs` — that a session marker names its
process and when it started — and it fails the way the others do, loudly and by
name. It degrades in the existing direction: a marker Perch cannot parse counts
as no evidence of a client, because a Profile is Live when something says so, not
when nothing does.

Three implementations of "when did this process start" is the largest single
piece of platform-specific code in Perch, for a correctness problem that is acute
on one platform and merely rare on the others. Checking the executable name of
the live PID instead would have been cheaper and Windows-only, and was rejected
for being asymmetric about a hazard that is universal, and for still misfiring
when the recycled PID happens to belong to another `claude`.
