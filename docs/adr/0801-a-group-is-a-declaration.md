# A Group is a declaration

A Group is the user's statement that a set of Accounts substitutes for one
another. When quota runs out, Cycling should move somebody to an Account they
would actually accept — another work subscription, never their personal one — and
nothing about where Accounts sit or how they were bought says which those are.
So it is said rather than inferred: `perch switch` with no target chooses within
the current Account's Group and never leaves it.

Isolation is not what Groups are for. There is one configuration directory, so
memory, settings, plugins and past work are shared unconditionally
(ADR everything-but-the-account) and there is no isolation on offer to scope.
What is left is the narrower job, and a Group does it whole: an empty Group is
still a Group, and an Account is never quietly dropped out of one.

A Group also carries the configuration that governs Cycling within it — whether
the Watcher may switch Accounts there unattended, at what Utilization threshold,
and which Strategy it prefers (ADR a-setting-names-its-scope). Unattended
switching is off by default, so a Group only ever changes underneath somebody
because they said it could.

## Being in no Group is the absence of the declaration

An Account need not be in a Group. Adoption records the existing login with no
Group at all (ADR a-login-perch-does-not-need), and `perch add` offers the
organization as a default the user may decline, so an ungrouped Account is the
ordinary starting state rather than an edge case.

Bare `perch switch` from there Cycles among the other ungrouped Accounts **only
where `interchangeable` says it may**, and that is off until somebody sets it.
With it off Perch switches nowhere and says why: that these Accounts have not
been declared interchangeable, and that the fix is either `perch group move` or
that one Setting.

This is the rule above followed to its conclusion. Being ungrouped is the absence
of the declaration, not a weaker form of it. Cycling freely among ungrouped
Accounts would move somebody from their work subscription onto their personal one
without their ever having been told the two were substitutable — precisely the
failure Groups exist to prevent.

`interchangeable` is the Ungrouped Scope's own and no Group carries it, because a
Group **is** that declaration rather than something that holds one. Modeling the
ungrouped pool as a Group under a reserved name would make a Group mean two
contradictory things: a declaration the user made, and one Perch made for them.

Turning it on is a declaration about every ungrouped Account at once, present and
future — including the next one `perch add` creates. Anybody wanting a narrower
statement than that wants a Group, which is what Groups are for.

## Two grants, and only one of them is a Group

Permission to switch when asked and permission to switch while nobody is looking
are different grants. A Group is the first; `watcher-may-act` is the second, and
neither implies the other.

So a Group needs only the grant — being a Group is the declaration — and the
Accounts in no Group need both, because the second grant has no owner where there
is no Group to carry it. The Watcher acts there only where `interchangeable` and
`watcher-may-act` are both on, and it reads them as two statements rather than
one said twice (ADR a-watcher-knob-is-arithmetic).

That is why the two are not collapsed into one key, and why the declaration is
not renamed to pair with the grant. A person declaring their Accounts
substitutable and a person handing Perch permission to act unasked are saying
different things, and two grants side by side would read as one of them said
redundantly.

## What was considered instead

**Inferring Groups from the Account's organization UUID.** Three subscriptions
bought personally each carry their own organization UUID, so inference splits
them and fails precisely the case Groups exist to serve. Perch may offer the
organization as a default when an Account is added, and the user confirms it.

**Dropping Groups entirely for a per-Account disable flag.** Close in value, and
it loses on two Groups with several Accounts each: Cycling should follow whichever
Account somebody is on without their toggling flags. That, plus having somewhere
to hang per-Scope configuration, is what keeps them.

**Each ungrouped Account as its own singleton.** Bare `perch switch` would then
always report "already on the best Account" — and adoption leaves the first
Account ungrouped, so out of the box the command the whole tool exists for would
do nothing and say nothing explaining why.

**One implicit pool of ungrouped Accounts, Cycling freely.** The convenient
reading and the wrong one, refused on the grounds above.

**Refusing bare `perch switch` from an ungrouped Account outright, with no
Setting to change it.** Somebody holding three subscriptions bought personally,
with no interest in Groups, would have to learn the concept to reach the one
command that matters to them. The Setting lets them make the same declaration a
Group makes, once, without the machinery.

**The word "flock".** Perch has already spent its whimsy budget on its own name,
and `--group` reads better on a command line.

## Consequences

A Group can be renamed, and the rename keeps everything the Group carries: its
Settings, its Accounts, and the cooldown the Watcher is pacing it by. Doing it by
hand — an add, a move per Account and a remove — would lose every Setting
somebody deliberately said.

Bare `perch switch` has an honest non-outcome here alongside "every Account in
the Group is exhausted" and "you are already on the best one"
(ADR headroom-is-the-worst-window). Like both of those it performs no Switch,
explains itself, and exits with a distinct code rather than pretending to have
worked.
