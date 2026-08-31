# Fable is spent first

An Account's windows do not recover at the same pace. The five-hour window
comes back the same afternoon; the Fable weekly window comes back once a week,
and it meters the model somebody chose on purpose. A person who runs Fable
wants every Account's Fable weekly drained before anything else is touched —
and worst-window Headroom, correct as it is about what could block, says
nothing about which window is worth draining first.

ADR headroom-is-the-worst-window names a Scope saying which of its windows
count as the live alternative 1.0 does not offer. This takes it up in the
narrowest form that serves: one Setting, one model, two tiers.

## The Setting

`prefer-fable` is a per-Scope Setting, off by default. Off, nothing anywhere
changes: Headroom alone decides, exactly as before.

On, the Scope's one ranking — the Cycle's, the Watcher's and the Listing's,
which are the same ranking (ADR the-listing-owns-the-set) — becomes two tiers:

- **First, the Accounts that can serve Fable now**: none of the windows a Fable
  request spends from — five-hour, seven-day, and the Fable weekly — is full.
  They are ordered by the Fable weekly window, so Fable drains evenly across
  the Scope before anything else is spent.
- **Second, the rest with anything left**: ordered by the fullest window that
  is not Fable's. An Account whose Fable weekly alone is full is not exhausted
  under this Setting — it is where a Fable-spent Scope falls through to, chosen
  once and then held on, with the Scope told its Fable is spent everywhere.

A Strategy orders within a tier and never across one: `soonest-reset` in the
first tier reads the Fable weekly's reset, and no Strategy promotes an Account
out of the second tier while the first has anywhere to go.

The first tier is ordered by the Fable weekly rather than by the worst of the
three, deliberately. While Fable is the only per-model window carrying a
figure, the worst of the windows Fable spends from *is* the worst window — a
Setting measured that way would change nothing until the endgame, and a knob
whose only effect is its edge case reads as broken. The Fable weekly is the
one reading that changes the choice while Fable remains.

What is not changed: whether an Account may be chosen at all, and when the
Watcher wants to move, are still the measurement's to say. The Threshold still
reads the fullest window, so a full Fable weekly still crosses it and a full
five-hour window still disqualifies a first-tier candidate.

## Loud when it matches nothing

The Setting keys on the window named `7-day-fable` — the name the reply's own
scope gives it (ADR a-window-comes-from-limits). Anthropic owns that name, and
a renamed flagship would leave the Setting matching nothing. Matching nothing
must not pass in silence: a Scope with `prefer-fable` on and no observed
Account reporting a Fable window is told so where the ranking is said — the
Listing — and ranks on Headroom alone meanwhile, because tiers keyed on a
window nobody reports order nothing, and a Setting that silently stops meaning
anything is the failure a changelog cannot warn about. The rename itself lands
the way every registry shape does — a new
Setting and a migration forward (ADR a-registry-comes-forward) — and adding
`prefer-fable` moves the registry's shape too, so it moves the version with it.

An Account whose cache holds no Fable window ranks in the first tier the way an
unobserved figure always ranks: below every Account with known Fable room,
above every full one. "No figure" is not "no ceiling", and a Refresh settles it.

## Refused

- **A `preferred-model` naming any model.** One flagship window exists, its
  name is Anthropic's to move, and a Setting whose *value* rots when a display
  name changes fails silently — the boolean fails loudly, above. The next
  flagship is its own Setting and its own decision.
- **Inferring the model in use from successive figures.** The Watcher acts only
  on a figure it just read; a delta between two readings is remembered
  evidence, confused by idle sessions, cache hits and a second session on
  another model.
- **Acting on Fable coming back.** After the fall-through, fresh Fable weekly
  elsewhere does not itself move you: the Watcher's one reason to act is the
  Account you are on running low, and you drift back at the next Threshold
  crossing or by asking. A second reason to act is a live alternative, priced
  as more Switches into a live credential setup.

## Consequences

With `prefer-fable` on, a Scope's Fable is exhausted account by account, the
fall-through Account carries the remaining general quota, and the one surprise
— sitting on that Account while Fable stands recovered elsewhere — is bounded
by the next Threshold crossing and was accepted with its price named.
