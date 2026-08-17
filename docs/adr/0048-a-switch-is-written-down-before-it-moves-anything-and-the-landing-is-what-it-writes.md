# A Switch is written down before it moves anything, and the Landing is what it writes

> **Carried out in #167.** Like ADR 0041, ADR 0042, ADR 0044, ADR 0045, ADR 0046
> and ADR 0047, this is the artifact of a planning effort rather than of a
> change, so it landed ahead of the work it describes instead of beside it.
> `registry.active` is now one field with three states, `switch::perform` and
> `switch::make_live` both write a Landing before the Credential moves,
> `switch::resolve_a_landing` is the step every Switch path takes first, `perch
> status` says a Switch was in flight, and the Watcher holds where the settling
> refuses.

The question arrived as an accusation `CONTEXT.md` makes against itself. The
**Landing** entry describes a window in which what is live and what Perch records
as active disagree, and then says of it: *"that is the one mistake this design
cannot recover from"*. A design that names its own unrecoverable failure mode is
inviting the question of whether the failure mode can be removed rather than
documented around.

It cannot be removed. It can stop being unrecoverable, and the difference is one
field.

## What is actually irreducible

A Switch writes to three places: the outgoing Account's Credential Store, the
Default Profile's Credential Store, and `.claude.json`. On macOS the second is a
keychain and the third is a file, and nothing makes a keychain write and a file
write atomic together. `Landing::record` then writes a fourth place, the
registry. **There is no arrangement of these that commits as one.** Every option
that begins "and then write X first" only moves which pair can disagree.

The second constraint is sharper, and it closes off the escape the question
reached for first. **The live Credential carries no owner.** `claudeAiOauth` is
an access token, a refresh token, an expiry, a scope list and a subscription type
(`probe.rs:164`) — no address, no uuid. Deriving the outgoing Account from the
live Credential itself is therefore never inspection; it is only ever
byte-equality against copies Perch already holds, and a Rotation defeats it by
construction.

So the question is not whether the gap can be closed. It is whether every state
the gap leaves behind can be **recovered from** — and, where it cannot, whether
Perch can at least know that.

## The hole is one path, and it is not the one you would guess

`capture` already declines four ways, and three of them are this hazard being
handled piecemeal. `Captured::NothingToSave` catches the live Credential being
byte-identical to the incoming one — which is the trace of a Switch interrupted
after step two, so **re-running the interrupted Switch repairs it**. The
same-Account branch catches switching *back* to the outgoing Account, and
**refuses**. `Corroboration` catches an Identity naming a third party.

What survives all four: crash mid-Switch, then a Switch to a **third** Account.
`.claude.json` still names the outgoing Account, the registry still names the
outgoing Account, the two agree — and both are wrong. That evidence is
indistinguishable from an ordinary Capture of a Rotation, which is the case
ADR 0006 exists to serve, so no rule reading only that evidence can separate
them. The Capture writes the *incoming* Account's Credential into the *outgoing*
Account's Profile, over the copy the Switch's own first step had just correctly
saved there.

**And it does not need a crash.** `switch.rs`'s own doc comment on `perform`
describes the path: the registry hold goes stale in ninety seconds, a keychain
that stops to ask the user for permission runs that out, `record` then cannot
save, and the user is told *"The Switch itself worked… Perch could not record
that, so its own view of which Account is active is behind until this is
fixed."* Perch detects the disagreement, narrates it, and has nowhere to write it
down. That is the finding this decision rests on: not a race nobody will hit, but
an omission Perch is already articulate about.

## What is decided

**A Landing is written down before the Credential moves.** One rule, stated as a
precondition rather than as a step: *Perch does not move the Credential until it
has written down that it is about to.*

**`registry.active` becomes one field with three states** — no Account, a settled
Account, or a Landing naming the Account being left and the Account being
switched to. One field rather than two, so a registry naming both a settled
active Account and a different in-flight one cannot be written at all. The
registry already carries one dangling-pointer check for `active`
(`registry.rs:1779`); a second field would need a second, held by nothing but
care.

**It is written after the Capture and before the Credential write.** The Capture
is safe to crash inside — it writes the live Credential into the Profile of the
Account the registry already names, which is where that Credential belongs, and
re-running repeats it harmlessly. Writing the Landing earlier would mean writing
one for every Switch that then refuses at step one, and every refusal path
remembering to clear it.

This is also what disarms the stale lock. If the hold has been burned by a slow
keychain during the Capture, the **Landing write** fails — and it fails with
nothing moved, before step two, rather than after it. Today's worst non-crash
path becomes an ordinary refusal.

**In the registry, not a sidecar.** A sidecar could be written outside the perch
lock, which is the only argument for one, and that argument is self-defeating: it
puts two writers on one fact. A sidecar written *under* the lock is the registry
with an extra file that can go missing. The Landing exists to describe a
disagreement between the registry and the machine, and the two halves of one fact
should not be readable apart.

## Resolution is its own step, and `capture` gains nothing

A Switch path that loads a registry holding a Landing **resolves it first**, then
runs the Switch against a registry that tells the truth. Resolution reads the
live Credential and asks, in order:

- equal to the incoming Account's stored copy → the incoming Account is active;
- equal to the outgoing Account's stored copy → the outgoing Account is active;
- equal to any other held Account's stored copy → that Account is active;
- the live store is empty → the outgoing Account, because nothing live is nothing
  a later Capture could destroy;
- anything else → **refused**, naming both readings.

The third of those is the widened ownership rule, and it is a **fallback** rather
than a sweep: it reads every held Account's Credential, which on macOS is a
keychain prompt each, and it is reached only where a Landing says a Switch was in
flight. On the ordinary path the two named Accounts answer first and nobody pays
for the rest.

The part worth stating plainly: **`capture` gains no new branch.** It answers
*is there a Rotation to save*; resolution answers *who is active*. Those are
different questions, and the four declines in `capture` today are what it looks
like when one function is made to answer both — `NothingToSave`, `NotTheirs` and
both `Corroboration` arms are owner-inference wearing a Capture's clothes.
Against a settled registry, `capture` is the function it was always written to
be. The Landing pays for itself by making the *next* special case unnecessary
rather than by adding one.

## The three steps keep their order

Reversing steps two and three — patching the Identity before writing the
Credential — independently converts the destroying path into a refusal, costs
nothing and adds no concept. It was considered and refused, and the reason is a
consequence of the Landing rather than an argument against reordering on its own
terms.

`switch.rs` gives two reasons for patching last, and the second is the one that
matters here: the Identity is the only witness Perch has to what actually
**moved**. Writing it ahead of the move turns it into a witness to what was
merely **intended**. The Landing is now the record of intent, and a machine with
two records of intent and no record of fact is worse than today's.

## Both doors, not one

`perch relogin` and `perch remove` reach the Default Profile through `make_live`
(`commands/relogin.rs:110`, `commands/remove.rs:296`), which writes the
Credential and then patches the Identity — the same two writes, the same gap.
`NotLanded` exists because of it and draws the same distinction `Landing` does.

**`make_live` writes a Landing too.** Skipping the Capture is what makes it
different going in; it makes no difference coming out. Guarding one door of two
is not a smaller rule than guarding both — it is a longer one, because the
exception has to be written down and justified, and `perch remove` of the active
Account is the door somebody walks through while deleting things.

## Being seen

**`perch status` says a Switch was in flight and not recorded**, as a line and as
a `--json` field, and **exits 0**. Half of why this hazard has survived is that a
machine mid-Landing is indistinguishable from a healthy one, so nobody looks. A
non-zero exit was refused on ADR 0018's register — status reports what it found
rather than judging it — and because a transient state should not fail somebody's
script.

**The Watcher holds rather than stops.** It is a Switch path, so it resolves; and
where resolution refuses, nobody is there to answer. Its own entry already
describes the behavior this wants: *"told otherwise it holds rather than stops,
and says so."* An unresolved Landing is a reason it may not act, indistinguishable
in kind from being under Threshold or inside a Cooldown, and it gets the same
treatment. Stopping would turn a state one `perch relogin` clears into a dead
Watcher somebody finds hours later.

## The corner that stays undecidable

A Landing in flight, and a live Credential matching nobody's stored copy: a
Rotation after the interruption, and nothing on the machine says whose. This is
**refused**, naming both readings, in the register
`the_live_credential_is_unaccounted_for` already writes in, with `perch relogin`
as the way through either way.

It is not a Quarantine. Nothing is lost in that state and the live Credential very
likely works; Quarantine is for a Credential established to be unusable, and
saying otherwise would cost the word its meaning. Nor is the user given a flag to
assert whose it is — they have no better evidence than Perch does, and a wrong
assertion is silent.

**This is the honest limit of the decision.** The bar is *never silently*, not
*never*: Perch cannot always tell what happened, but after this it always knows
when it cannot.

## The glossary

**One entry changes.** **Landing** widens from *"a Switch that has happened and is
not yet written down"* to a Switch under way and not yet finished — the record now
exists for a window in which the Switch has not happened yet, which is the whole
point of it. It loses *"that is the one mistake this design cannot recover from"*
and gains what a Perch that finds one knows: which two Accounts the live
Credential could belong to. Its last sentence, that only whether it *moved* is
answerable earlier, is untouched and still true.

The name survives rather than being replaced. This record is the Landing made
durable, and inventing a second word for the state a Landing already names is the
trade this sweep's yardstick refuses.

**Switch stays at three steps.** The three are what change the machine; the
Landing write changes only what Perch has written down about itself. Folding it
into the entry that defines the act would make that entry describe Perch's
bookkeeping, and the Landing entry is the one about the bookkeeping.

**Capture is untouched**, and nothing new gains an entry.

## Consequences

**This supersedes nothing and amends nothing.** ADR 0006 is untouched: Capture
before every Switch stands, the three steps stand, and its Consequences paragraph
— a Credential rotated and lost before it can be Captured means a fresh login —
stays true word for word. What changes is that Perch now refuses on the way into
that state instead of causing it.

That is the finding, and it is the same one ADR 0047 reached about the command
surface: **this is not a decision being revisited, it is one that was never
made.** ADR 0006 chose the three steps and said what an interruption costs.
Nobody ever chose what Perch does on *finding* an interruption, which is why the
answer lived in a glossary entry, as a confession.

The carry-out is not chained behind the sweep's other removals. It is a small
deep change to `switch.rs` and `registry.rs`, where ADR 0047's rename is broad and
shallow across `main.rs` and the commands; the two files that overlap, `watch.rs`
and `commands/status.rs`, are cheaper to rebase a rename over than a behavior
change. Sequencing a data-safety change behind three renames would be sequencing
by convenience rather than by cost.

No exit code changes and no new one is added. The refusal is a `Conflict`, which
is what the two refusals it joins already are.
