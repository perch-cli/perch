# A figure carries its age

`/api/oauth/usage` allows roughly 28-30 requests an hour per Account, in a
rolling window that does not refill early. `perch status` is the kind of command
people put in a shell prompt or an editor status line, where it may run several
times a minute — enough to saturate an Account's entire hourly budget in the
first few minutes and leave the numbers unavailable exactly when a Switch is
needed.

So **every command that displays Utilization reads it from a local cache and
never blocks on the network to render.** Each figure is shown with its age, so a
stale number is visibly stale rather than quietly wrong. `--refresh` fetches.

## A refresh degrades the display rather than failing the command

`perch status --refresh` answers the question `perch status` answers — where does
my quota stand — and asks Anthropic first. Everything about that first step can
fail on its own: the hourly allowance is spent and does not refill early, a
client is running against the Profile so its Credential is not Perch's to renew
(ADR a-profile-is-live-by-evidence), Anthropic will not accept a Credential at
all, the network is not there.

**None of those turn the command into a failure.** Each is reported by name —
which Account, and why — the Account keeps whatever figure it last had, and the
command renders and exits zero. `--json` carries the same under `refresh`, which
is `null` when no refresh was asked for, so a script can tell "nobody asked" from
"asked, and it went fine".

That follows from a figure being an observation rather than a live reading. Every
surface already has room to show a figure that is minutes old, and does. A
refresh that failed the command would throw away a perfectly good cached answer
over the freshness of it, at exactly the moment somebody is deciding where to
switch. The figure's age already says how much to trust it.

The same rule makes each Account independent. One Account that cannot be read
leaves the others read and displayed, because a listing that lost every figure to
one broken Account answers worse than a listing with one gap in it — and the gap
is visible, since a figure nobody could read still says when it was last
observed.

## A cached figure outlives the window it describes

A `resets_at` in the past is ordinary rather than strange: the window has already
come back, and the percentage beside it is stale. **An elapsed reset is not a
fact about when an Account comes back, so nothing ranks it as one.** Such an
Account falls to the room it can see, beside every Account no reset could be read
for. Ranked as a reset, the key sorts earlier-is-better and the *stalest* figure
wins among Accounts whose windows have all come back.

It is said as stale rather than as absent, because "no cached figure says when it
comes back" is untrue of a cache that says exactly that about a time now past.
The clause names the clock time and leaves the wait unsaid: a wait rendered from
an instant already gone reads as "any moment now", which is the right thing to
say about a window somebody is waiting for and a contradiction two words from
"which has passed".

## How a figure is said

The age is the whole of the promise Perch makes about a cached number, so the way
one is rendered may not overstate it.

**A figure is never rounded into a state the Account is not in.** Rounding to
whole numbers renders 0.4% Headroom as `0` and 99.6% used as `100`, and both ends
of that are read as a state rather than as a number: an Account with 0.4% left is
perfectly choosable, and one at 99.6% used is one the Watcher is still deciding
about. The two edges say which side of the boundary they are on — `<1`, `>99` —
and a real nought and a real hundred still print as themselves.

**A figure is never rounded into a boundary it has not reached.** A percentage
printed beside a Threshold it is being judged against is widened until it differs
from that Threshold, and where no precision will say which side of it the figure
falls, the words do. The comparison is made on what Anthropic sent, so a rounded
79.6 against a Threshold of 80 reads "80% used … threshold 80% — under it": a
flat self-contradiction on the one line that is the evidence the policy works.

**An age never falls as a figure gets older, and a wait never grows as it
shortens.** One table of boundaries answers both, or the handover from minutes to
hours moves in one and not the other and an age that grew by a second falls by
half an hour. A wait's minutes round up where an age's round to nearest: whether
to wait for perishable quota is the decision a wait is read for, and a wait that
reads shorter than it is is the one direction that costs somebody something.

**A clock that ran backwards says so.** A figure stamped in the future is not
maximally fresh, in prose or in `--json`: clamping the age at nought is the one
reading that gets a stale figure trusted rather than doubted.

## Consequences

Utilization is displayed as an observation with a timestamp, not a live reading.
Every surface that shows it has room for "as of" alongside the number, and
`--json` carries the observation time so scripts can decide for themselves
whether it is fresh enough.

Switching decisions do not inherit this. Displaying a stale figure is honest
because the age is printed beside it, and ranking on one throws that age away
inside a comparison nobody can see — so a Cycle reads the Accounts it cannot rank
without (ADR a-choice-reads-what-it-ranks). The rule here is about the fetch a
display makes, and a display still never blocks on the network.

`--refresh` has no failure exit code of its own. A script that needs to know
whether the figures it is looking at are current reads `refresh` from `--json`
rather than branching on the exit status; a person reads the age beside each
figure, which is the same information said the way people read it.

Errors that stop the command before any figure could be shown — no active
Account, a Registry Perch cannot read — are unaffected, and fail with the codes
they already have. The rule is about the fetch, not about the command.

**The Watcher is the exception** (ADR a-watcher-knob-is-arithmetic). It shows
nobody anything; it acts. A round that could not read holds, rather than deciding
on the figure Perch already had.
