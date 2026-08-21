# A Switch is written down first

A Switch leaves two copies of one Credential: the one in the Account's Profile
and the live one on the Default Profile. Only the live copy is used, and Claude
Code Rotates it whenever it likes — so by the time you switch away, the copy in
the outgoing Account's Profile can be several Rotations behind, and a retired
refresh token is dead.

So a Switch is three steps, not one. Perch Captures the live Credential back
into the outgoing Account's Profile, writes the incoming Account's Credential to
the live store, and patches `oauthAccount` to match. Skipping the Capture means
every Switch quietly poisons the Account being left behind, with the damage
surfacing only when you switch back to it. All three steps run under Claude
Code's own OAuth refresh locks, which is what stops a Refresh landing between
the Capture and the write.

One precondition stands over the three: **Perch does not move the live
Credential until it has written down that it is about to.** What it writes is a
Landing, and the rest of this document is what that is for.

## Nothing here commits as one, and the live Credential carries no owner

A Switch writes to three places: the outgoing Account's Credential Store, the
Default Profile's Credential Store, and `.claude.json`. On macOS the second is a
keychain and the third is a file, and nothing makes a keychain write and a file
write atomic together. Recording which Account is active writes a fourth place,
the registry. **There is no arrangement of these that commits as one.** Every
alternative that begins "write X first" only moves which pair can disagree.

The second constraint is sharper, and it closes off the escape the first
question reaches for. **The live Credential carries no owner.** `claudeAiOauth`
is an access token, a refresh token, an expiry, a scope list and a subscription
type — no address, no uuid. Deriving the outgoing Account from the live
Credential is therefore never inspection; it is only ever byte-equality against
copies Perch already holds, and a Rotation defeats it by construction.

So the question is not whether the window can be closed. It is whether every
state the window leaves behind can be **recovered from** — and, where it cannot,
whether Perch can at least know that.

The path that survives every check the Capture makes for itself is a crash
mid-Switch followed by a Switch to a **third** Account. `.claude.json` names the
outgoing Account, the registry names the outgoing Account, the two agree — and
both are wrong. That evidence is indistinguishable from an ordinary Capture of a
Rotation, which is the case the three steps exist to serve, so no rule reading
only that evidence can separate them: the Capture would write the *incoming*
Account's Credential into the *outgoing* Account's Profile, over the copy the
Switch's own first step had just correctly saved there.

It does not need a crash. The registry hold goes stale in ninety seconds, a
keychain that stops to ask the user for permission runs that out, and Perch is
then left detecting the disagreement, narrating it, and having nowhere to write
it down.

## What a Landing is, and where it is written

**`registry.active` is one field with three states** — no Account, a settled
Account, or a Landing naming the Account being left and the Account being
switched to. One field rather than two, so a registry naming both a settled
active Account and a different in-flight one cannot be written at all. The
registry carries one dangling-pointer check for `active`; a second field would
need a second, held by nothing but care.

**It is written after the Capture and before the Credential write.** The Capture
is safe to crash inside — it writes the live Credential into the Profile of the
Account the registry already names, which is where that Credential belongs, and
re-running repeats it harmlessly. A Landing written earlier would be one written
for every Switch that then refuses at step one, and one every refusal path has
to remember to clear.

That is also what disarms the stale hold. Where the hold has been burned by a
slow keychain during the Capture, the **Landing write** is the first thing that
meets it — so it fails with nothing moved, before step two rather than after it,
and the worst non-crash path becomes an ordinary refusal.

**In the registry, not a sidecar.** A sidecar could be written outside the perch
lock, which is the only argument for one, and that argument is self-defeating:
it puts two writers on one fact. A sidecar written *under* the lock is the
registry with an extra file that can go missing. A Landing exists to describe a
disagreement between the registry and the machine, and the two halves of one
fact should not be readable apart.

## Resolution is its own step, and the Capture answers a different question

A Switch path that loads a registry holding a Landing **resolves it first**, and
then runs against a registry that tells the truth. Resolution reads the live
Credential and asks, in order:

- equal to the incoming Account's stored copy → the incoming Account is active;
- equal to the outgoing Account's stored copy → the outgoing Account is active;
- equal to any other held Account's stored copy → that Account is active;
- the live store is empty → the outgoing Account, because nothing live is
  nothing a later Capture could destroy;
- anything else → **refused**, naming both readings.

The third is the widened ownership rule, and it is a **fallback** rather than a
sweep: it reads every held Account's Credential, which on macOS is a keychain
prompt each, and it is reached only where a Landing says a Switch was in flight.
On the ordinary path the two named Accounts answer first and nobody pays for the
rest.

**The Capture gains no branch from any of this.** It answers *is there a
Rotation to save*; resolution answers *who is active*. Those are different
questions, and a Capture's declines about ownership are what it looks like when
one function is made to answer both. Against a settled registry the Capture is
the function it was written to be, and the Landing pays for itself by making the
*next* special case unnecessary rather than by adding one.

Everything reading the live Credential to settle a Landing reads it under Claude
Code's own locks, and saves inside them too, so the window they close is the
whole of read-decide-record rather than the read alone. A Rotation *since the
interruption* defeats resolution and is accepted; one Perch could have locked
out is not.

## Both doors, not one

`perch relogin` and `perch remove` reach the Default Profile without Capturing
first, and each has its own reason for it. A `perch relogin` of the Account you
are on would Capture the broken Credential over the fresh one the login has just
written — the loss arriving as tidiness. A `perch remove` would Capture into a
Profile it is about to delete (ADR a-removal-lands-first). What they share is the
same two writes and the same window, so **that door writes a Landing too.**
Skipping the Capture is what makes it different going in; it makes no difference
coming out. Guarding one door of two is not a smaller rule than guarding both —
it is a longer one, because the exception has to be written down and justified,
and `perch remove` of the active Account is the door somebody walks through
while deleting things (ADR a-removal-lands-first).

## The three steps keep their order

Patching the Identity before writing the Credential independently converts the
destroying path into a refusal, costs nothing and adds no concept. It is still
refused, and the reason is a consequence of the Landing rather than an argument
about reordering on its own terms: the Identity is the only witness Perch has to
what actually **moved**, and writing it ahead of the move turns it into a witness
to what was merely **intended**. The Landing is the record of intent, and a
machine with two records of intent and no record of fact is worse than one with
a record of each.

## Being seen

**`perch status` says a Switch was in flight and not recorded**, as a line and
as a `--json` field, and **exits 0**. Half of why this hazard survives unnoticed
is that a machine mid-Landing is indistinguishable from a healthy one, so nobody
looks. A non-zero exit is refused because status reports what it found rather
than judging it, and because a transient state should not fail somebody's
script.

**The Watcher holds rather than stops.** It is a Switch path, so it resolves;
and where resolution refuses, nobody is there to answer. An unresolved Landing is
a reason it may not act, indistinguishable in kind from being under Threshold or
inside a Cooldown, and it gets the same treatment. Stopping would turn a state
one `perch relogin` clears into a dead Watcher somebody finds hours later.

## The corner that stays undecidable

A Landing in flight, and a live Credential matching nobody's stored copy: a
Rotation after the interruption, with nothing on the machine to say whose. This
is **refused**, naming both readings, with `perch relogin` as the way through
either way.

It is not a Quarantine. Nothing is lost in that state and the live Credential
very likely works, and Quarantine is for a Credential established to be
unusable — saying otherwise would cost the word its meaning. Nor is the user
given a flag to assert whose it is: they have no better evidence than Perch does,
and a wrong assertion is silent.

**This is the honest limit.** The bar is *never silently*, not *never*: Perch
cannot always tell what happened, but it always knows when it cannot.

## Consequences

The Capture settles the coherence question without any mtime or hash comparison.
The live copy is authoritative while it is live, and it is written back at the
one moment Perch controls, so a Profile's stored Credential is understood to be
stale whenever that Account is the active one.

Where a Credential is Rotated and lost before it can be Captured — a crash
between the two writes, or a machine that dies mid-Refresh — that Account needs
a fresh login. Quarantine is therefore not a feature to defer: it is the
terminal state of this design and exists from the start.

The refusals this adds are `Conflict`s, which is what the refusals they join
already are, so no exit code changes and none is added.
