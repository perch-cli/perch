# Utilization is served from cache, and refreshing it is deliberate

`/api/oauth/usage` allows roughly 28-30 requests per hour per account, in a
rolling window that does not refill early. `perch status` is the kind of command
people put in a shell prompt or an editor status line, where it may run several
times a minute — enough to saturate an account's entire hourly budget in the
first few minutes and leave the numbers unavailable exactly when a switch is
needed.

So every command that displays utilization — `status`, `list`, `tui` — reads it
from a local cache and never blocks on the network to render. Each figure is
shown with its age, so a stale number is visibly stale rather than quietly
wrong. `--refresh` fetches, and `perch watch` is what keeps the cache warm for
people who want it current without asking.

## Consequences

Utilization is displayed as an observation with a timestamp, not a live reading.
Every surface that shows it must have room for "as of" alongside the number, and
`--json` output must carry the observation time so scripts can decide for
themselves whether it is fresh enough.

Switching decisions inherit this. `perch switch` with no target ranks on cached
figures, which can be minutes old — so when a cycle lands on an account that
turns out to be fuller than the cache implied, that is expected behavior and
should be reported plainly rather than treated as a bug.
