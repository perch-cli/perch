# Configuration

`perch config` changes the rules Perch chooses Accounts by, and asks nothing:
every capability Perch has is reachable from a script, because it has to be
complete over SSH and in CI (ADR 0011).

## The Settings

| Key | Said at | Values | Default |
| --- | ------- | ------ | ------- |
| `strategy` | Global, any Scope | `most-headroom`, `soonest-reset` | `most-headroom` |
| `watcher-may-act` | Global, any Scope | `true`, `false` | `false` |
| `watcher-threshold-percent` | Global, any Scope | 0–100 | `80` |
| `cycle-ungrouped` | Global only | `true`, `false` | `false` |

## Two layers, and which one you meant

Config is **two layers deep** (ADR 0002). Every Setting exists at **Global**,
where it is the value that applies until something narrower is said. A **Scope**
— a Group, or the Accounts in no Group taken together — holds an **Override**
for a Setting it wants different and **Inherits** the rest. Nothing is three
layers deep, and an Account carries nothing at all: every Setting there is
describes how Perch chooses *between* Accounts, and a rule for choosing has
nothing to say to a set of one.

Which layer is meant is read off how many words you said. Three name a Scope
and set an Override; two set Global's default. `unset` is the same vocabulary
one word shorter.

```
$ perch config set watcher-threshold-percent 70
`watcher-threshold-percent` at Global is now 70.
The Ungrouped Scope, Group `personal` and Group `work` Inherit it, and go on following Global as it changes.
...

$ perch config set work strategy soonest-reset
`strategy` on Group `work` is now soonest-reset.
A Cycle within Group `work` prefers the Account whose fullest Quota Window resets soonest, so perishable quota is spent rather than wasted. Headroom is still measured by the worst window (ADR 0012), so an exhausted Account is still never chosen however soon it comes back.

$ perch config get
cycle-ungrouped true
strategy most-headroom
watcher-may-act false
watcher-threshold-percent 70
work strategy soonest-reset

$ perch config unset work strategy
`strategy` on Group `work` is Inherited from Global again, which says most-headroom. It follows Global from now on rather than holding a value of its own.
```

`perch config get` on its own prints Global's Config and then every Override
there is. `perch config get <scope>` prints every Setting in force for one
Scope, each as the tail of the `set` that would restore it — so **the layer a
value came from is the number of words that would set it again**. A Setting a
Group Inherits reads back as `watcher-threshold-percent 80`; one it Overrides
reads back as `work watcher-threshold-percent 95`.

Inherit is a state and not an absence. A Scope that Inherits tracks Global as
Global changes; a Scope holding an Override that happens to equal Global's does
not. So a threshold set once at Global reaches every Group that has not said
otherwise, and changing your mind is one edit rather than four.

## Scopes

A Scope is a Group by name, or `ungrouped` for the Accounts in no Group — which
are a Scope so that there is somewhere to say how they are Cycled, and never a
Group: a Group is a declaration somebody made, and this is the absence of one
(ADR 0017). No Group can be called `ungrouped`, or the Scope would answer to the
name first.

**Global is addressed by naming no Scope at all** — `perch config set <key>
<value>` — so there is no word for it to be typed as. `global` is refused as a
Group name and as an Alias for that reason: it is the word people reach for when
they mean Global, and a Group answering to it would take
`perch config set global strategy soonest-reset` as an Override while Global
stayed exactly as it was.

`cycle-ungrouped` is Global's alone, because the Accounts it governs have no
Group to carry it. It is also the one place the layering is deliberately not
uniform: **`watcher-may-act` does not Inherit into the Ungrouped Scope.** It is
gated behind `cycle-ungrouped`, so the watcher acts on ungrouped Accounts only
where both are on. A Global "yes" is a statement about your work Groups, and
Inheriting it straight through would authorise moving you onto your personal
subscription — the failure Groups exist to prevent, arriving by a route nobody
typed.

## Strategy

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

## The watcher's two

The **watcher's** two fields govern [`perch watch`](watching.md) and nothing
else. `watcher-may-act` says whether it may Switch within the Group at all, and
is off by default because a Group only ever changes underneath you because you
said it could. `watcher-threshold-percent` is how much of the fullest Quota
Window of the Account you are on has to be used before it moves you. Neither of
them starts a Watcher: they take effect while one is running — the loop in a
terminal, a Service, or a scheduled Check — and not otherwise.

Taking `watcher-may-act` back does not stop a Watcher that is already running.
It **holds** it: it reads nothing and moves nothing, says what is missing, and
starts deciding again the moment the grant comes back (ADR 0040). The grant is
about whether it may *act*, and a held Watcher is not acting.

**How full is too full is the only preference in the loop** (ADR 0046). The
three numbers beside it are arithmetic, so they are fixed rather than offered:

- How often it **reads** — two and a half minutes, derived from Anthropic's
  allowance of ~28-30 reads an hour rather than from anyone's taste. A Group
  configured to read every ten seconds would be a Group configured to spend that
  allowance and be refused.
- The **margin** — 10 points under the threshold — which is what a candidate has
  to be clear of the threshold by to be worth moving to. Nobody wants the low
  end, where a destination barely emptier than the Account you are on is
  accepted; the high end is already reachable by moving the threshold.
- The **cooldown** — 15 minutes — which is the least it leaves between two
  Switches. A five-hour window moves slowly enough that fifteen minutes never
  misses a real crossing, which is arithmetic about the window rather than a
  taste.

A threshold under the margin is not refused — it is a Group that will only move
onto an Account with nothing used at all, which is a coherent thing to ask for.
The margin does not get to veto the one Setting that is a preference.

## Reading it back

Every line `perch config get` prints is the tail of the `perch config set` that
would restore it, so reading the Config and writing it back are the same
vocabulary and a script needs no parser. Naming a Scope and a key prints that
one line, which is both the value and where it came from. An unknown key or a
value that means nothing is refused with exit code 14 and the ones that do mean
something, so a script that mistyped a Setting does not go on believing it took.
`perch config unset` at Global is refused too: Global's values are always set,
so there is nothing above them to Inherit from.
