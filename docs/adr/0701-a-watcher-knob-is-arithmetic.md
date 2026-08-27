# A Watcher knob is arithmetic

Somebody rotating between subscriptions wants to stop noticing that they ran
out. That wants something watching Utilization and Switching when a Threshold is
crossed, without being asked: `perch watcher run` as a loop in a terminal,
`perch watcher check` as one round for a scheduler, and a Service running the
same loop for you (ADR the-machine-runs-the-watcher). One policy in three
arrangements, and the policy is this record.

Five numbers pace it — the interval between Refreshes, the Back-off after a
Refresh that could not be read, the Cooldown between two Switches, the Margin
under the Threshold, and the Threshold itself. **Only the Threshold is anyone's
to set.** The other four are arithmetic, about Anthropic's allowance or about
how fast a Quota Window moves, and a Setting is where somebody's preference
enters rather than where a derivation is retyped. A Group that could poll every
ten seconds would be a Group configured to be refused.

## It Refreshes the active Account, every two and a half minutes

Anthropic's usage endpoint allows roughly 28-30 reads an hour per Account. Two
and a half minutes is twenty-four of them, which leaves room for the
`perch status --refresh` somebody types while the Watcher is running: a loop
that spent the whole allowance would answer the user's own question with a
throttle.

The same arithmetic read the other way is why only the active Account is
Refreshed. At twenty-four an hour each, a Group of two would already be at the
limit and a Group of four past it — so polling the whole Group would make the
size of a Group a scaling limit, to keep figures fresh that are read only at a
crossing. Candidates are read at the moment a decision is taken instead, which
is also the moment they are cheapest: a candidate is by definition an Account
nothing is running against, so ADR a-profile-is-live-by-evidence permits
Renewing it to ask.

## It never acts on a figure it did not just read

A Refresh that could not be read holds the round: nothing is decided, and
nothing is decided on the figure Perch already had. Acting on a cached figure
would be a Switch made on evidence the user already has, and the two costs are
not comparable — a held decision costs nothing, and a wrong Switch costs a
Capture, a Credential write and possibly an Account more exhausted than the one
it left. A reply that arrives carrying no Quota Window Perch can read is a
failed Refresh too, and not a reading of zero: an Account with nothing used is
the one reading that can never be over any Threshold.

This is the one place the Watcher diverges from every other surface. Elsewhere a
Refresh degrades the display rather than failing the command
(ADR a-figure-carries-its-age), because those surfaces show a person a number
they will judge for themselves. The Watcher shows nobody anything; it acts.

**The Back-off doubles from the interval and stops at twenty minutes.** It
starts at the ordinary two and a half rather than under it, so no retry ever
asks faster than a working loop does and the arithmetic above covers a failing
endpoint unchanged. At twenty minutes a persistent failure asks three times an
hour instead of twenty-four. It is bounded there because the endpoint coming
back does not announce itself and the only way the Watcher finds out is by
asking: a loop that had doubled its way to an hour would come back long after
the crossing it was left running for. The first Refresh that works drops the
whole of it rather than winding it down a step, because pacing the loop on a
failure that has stopped happening is pacing it on nothing.

A Back-off is the loop's own memory and nobody's to configure — a Group that
could set it could set it to nothing, and nothing is what the endpoint is
already refusing. A Cooldown paces Switches the Watcher makes; a Back-off paces
questions nobody is answering.

## The Margin refuses a destination nearly as full as the one being left

Usage on the Account you are on climbs. Usage on the Accounts you are not on
does not. Two Accounts therefore do not trade places — **they walk upward
together.** With no Margin at a Threshold of 80: A is at 80 and B at 79, so you
are moved to B; you burn B to 80; one Cooldown later you are moved back to A,
still at 80. That repeats every Cooldown, and each move lands you on an Account
with almost nothing left. The Cooldown sets how often the pointless move
happens and never stops it happening. Only the Margin does, by turning the round
into `Exhausted` — exit `17` — which is the true answer: there is nowhere better
to go.

**The Margin is ten points, relative to the Threshold.** Relative rather than
absolute, so it tracks the Threshold as it changes: "ten points under" still
means something once somebody sets the Threshold to 60, where a fixed 70 would
quietly stop meaning anything. A constant rather than a Setting because nobody
wants the low end and the high end is already reachable by moving the
Threshold — and the low end permits a contradiction, since an Account is left on
`>=` the Threshold and a candidate is set aside on `>` the ceiling, so at a
Margin of nothing an Account at exactly 80% would be full enough to leave and
clear enough to arrive at.

Collapsing the Margin into the Threshold is refused. Making the rule symmetric —
you are moved off above the line and never onto anything above it — is one
number with one meaning in both directions and repairs that asymmetry by
construction, but it is exactly a Margin of nothing with the comparison
tightened, so it re-buys the walk upward the Margin exists to stop. The concept
has to survive; only a knob for it does not.

The Margin sets candidates aside before the Strategy ranks them rather than
vetoing the winner afterwards. A `soonest-reset` Group would otherwise be told
there is nowhere to go while a perfectly empty Account sat behind the fullest
one, and the Strategy is entitled to prefer among whatever clears the ceiling.
An Account Perch has never read a figure for is set aside too: ranking puts an
unknown above a window that is certainly full, which is right for a Switch
somebody asked for, and unasked it is a move onto an Account the Watcher knows
nothing about.

## The Cooldown is written down, because a Watcher gets restarted

**Fifteen minutes.** A five-hour window moves slowly enough that fifteen
minutes never misses a real crossing, and often enough that a Watcher which has
just moved you is not about to move you again. That is arithmetic about how fast
a Quota Window moves rather than anyone's taste, and what is left for a person
to express — how promptly they are moved after a real crossing — has a right
answer. A Cooldown of a week against a five-hour window is a way of spelling *do
not watch*.

It is asked before the candidates are read rather than after, because a round
that may not act has no business spending an allowance on figures it cannot use.

**Every unasked Switch is written down, and both arrangements read it.** What
Switched, and when, is written against the Scope in the registry in the same
save as the Switch it paces — and the round ahead reads it back under the lock,
so the Cooldown a round is held by is the one that was on record when it
decided. Per Scope rather than per machine, because a Switch within one Group
has nothing to say about how soon another may move, and only a Switch that
happened writes one: a round that changed nothing has nothing to pace.

Scheduled, the Watcher is a sequence of processes and no one of them is it, so a
Cooldown in memory would be one that never held anything and a Check every
minute would Switch every minute whatever the Group said. That much was always
true. What this record got wrong is that the loop is any different: **a Service
restarts it.** `Restart=always` with `RestartSec=30` and launchd's `KeepAlive`
both bring the loop straight back — launchd with no give-up at all — so a
Cooldown the loop held in memory was one its own supervisor cleared thirty
seconds after a Switch, which is the Watcher moving you twice in a minute by
exactly the arrangement ADR the-machine-runs-the-watcher introduced.

The reasoning it replaces was that stopping the loop and starting it again is a
person saying "go on then". A supervisor is not a person, and nothing in the
process can tell the two apart. So the person who deliberately restarts a loop
now waits out what is left of a Cooldown, which is the cost of the Watcher not
being able to ask who restarted it — and the cheaper of the two mistakes, since
the other one is a machine that Switches on a crash loop.

The Cooldown being on disk does not make it the machine's. A registry is per
Perch home and the watch is held by a lock over that home, so there is exactly
one Watcher writing one record: two people watching pace themselves separately
because they are not sharing a registry to begin with.

**There is no no-return.** Nothing is Switched during a Cooldown, so nothing can
be Switched back either, and a rule barring the Account just left could never
change what Perch does. What it would cost is a Setting, a field on four types, a
parameter threaded through the ranking and into the sentence explaining why an
Account was passed over, four branches of advice, a row in the guide and an
entry in the glossary — for a branch that is unreachable by construction. A
Setting that prints a sentence about itself which is not true is the clearest
failure there is of the yardstick this record is taken by. The guard the rule
was for is kept here instead, at no surface at all:

> **If the Cooldown ever stops gating Switches outright — becoming per-Account,
> or pacing rather than barring — a no-return has to come back.** It is absent
> because the Cooldown subsumes it, not because returning immediately is
> acceptable.

## The Threshold is the one preference

How full is too full cannot be derived from Anthropic's allowance or from the
length of a window. Somebody who never wants to hit a wall mid-task sets 60;
somebody squeezing every drop sets 95; both are coherent, and nothing in the
endpoint's behavior prefers either. It is the one place a person's appetite for
risk enters the loop, and it is Overridable per Scope for the same reason — a
work Group wanting a different Threshold from a personal one.

It is reached at rather than passed: a Threshold of 80 is the figure somebody
set as the point they want to be moved at, and an 80 that waited for 81 would be
a Setting that means something other than what it says. A Threshold under the
Margin is in range, and is a Group that will only move onto an Account with
nothing used at all — a coherent thing to ask for, and refusing it would make
the order two `perch config set`s are typed in matter.

It keeps its `watcher-` prefix, which groups the two Settings the Watcher owns
and separates a Watcher rule from `strategy`, which governs Cycles nobody is
watching.

## It acts only where two statements have been made

A Scope that has not said `watcher-may-act` is a Scope the Watcher decides
nothing about, and an Account in no Group needs `interchangeable` as well —
being interchangeable at all is its own yes, and it is a declaration somebody
makes rather than a grant the Watcher inherits (ADR a-group-is-a-declaration).

Both are read every round rather than only at the first, because either can be
taken back while the loop is sleeping: a `perch switch` in another terminal can
leave an ungrouped Account active, and a Group can be told the Watcher may no
longer act on it. Nothing changes underneath you unless you said it could. A
loop holds on a withdrawn statement rather than stopping
(ADR the-machine-runs-the-watcher); a Check exits `18` for the ungrouped Account
and `14` for the Group, because a scheduler has to be told that this machine is
not arranged for what it was asked to do.

## The decision log is one line a round

Every round prints one line to standard output, including the rounds where
nothing happens, which are most of them. The log is the whole of the evidence
that the policy works — it is what makes *why did it Switch just then*
answerable without reading the source — so a line names the figure that was
read and then what was decided about it, in that order, every time.

What follows the figure is where ADR perch-says-what-it-did cuts. `waiting` and
`switched` are the loop doing what the opening line said it would do, so they
stop at the figure: the Threshold is the opening's to declare once for the run,
and a verdict the status word already gives is a verdict said twice. `cooling`,
`nowhere`, `held` and `refused` are refusals — nothing happened, and the reader
cannot see why — so they keep their sentence whole. A `switched` line still
names where it went, which is the one thing about it nobody could have
predicted, and says what could not be re-read on the way, which is the one thing
that can make a Switch land somewhere worse than it left.

Every held round says which failure held it and when the Watcher will ask again.
A hold whose line said neither reads as a Watcher that has given up.

## What a Check reports

`perch watcher check` reports in its exit code: `0` Switched, `15` nothing to do
now, `17` a Switch was wanted and every candidate was exhausted, `18` ungrouped,
`20` held because the figures were stale.

Three outcomes share `15`, and deliberately: under the Threshold, inside the
Cooldown, and a Switch the machine turned away all leave a scheduler the same
thing to do — nothing now, and come back at the next Check. Which of them it was
is on the decision line, where a person reading a cron mailbox needs it and a
script can do nothing with it. A code per outcome would be a distinction nobody
could act on, and the refusal a person typed already has `16` of its own.

Anything that stops a Check from deciding keeps the code its failure earned,
before the round or during it: a Scope that has not said the Watcher may act
exits `14`, and a Switch that made the incoming Credential live and then failed
exits with whatever failed. Those are failures rather than decisions, and
folding them into the table would report `0` or `15` about a machine that is
part way through a Switch. The table is what a Check *decided*.

A Quarantined active Account is a figure that cannot be read, so a Check holds
and exits `20` as it would for a throttle. `19` says more — it is the one
failure `perch relogin` repairs — and it is not in the table a Check promises,
so that distinction is carried on the decision line instead. Neither is
anything a scheduler can act on: both mean this Check decided nothing.

## Consequences

A Switch that changed nothing is a decision; one that changed something and then
failed stops the loop. A Capture refused because a client is running against the
outgoing Profile (ADR a-profile-is-live-by-evidence) leaves the machine exactly
as it was and clears itself when that client exits, so it is printed and the
loop goes on watching. A Switch that made the incoming Credential live and then
failed has left the machine part way through, and a Watcher that carried on
polling would be deciding what to do next about a machine nobody has looked at.

Everything the Watcher does when it acts is a Switch, whole: the outgoing
Credential is Captured first (ADR a-switch-is-written-down-first), Claude Code's
locks are taken, and a Live Profile's token is never Renewed. Running while
Claude Code is working is the normal case rather than the exception.

`perch config` carries two Watcher Settings — `watcher-may-act` and
`watcher-threshold-percent` — beside `strategy`, and `interchangeable` on the
Ungrouped Scope alone. **All four pacing concepts survive as concepts.** Threshold,
Margin, Cooldown and Back-off are each still an idea and each keeps its glossary
entry: a person meets these words in a `cooling` line and in a held round, and a
term you meet but cannot look up is worse conceptual surface than one you can.
What is settable is smaller than what is named.
