---
title: "Configuration"
sidebar:
  order: 8
---

`perch config` changes the rules Perch chooses Accounts by, and asks nothing:
every capability Perch has is reachable from a script, because it has to be
complete over SSH and in CI.

## The Settings

| Key | Said about | Values | Default |
| --- | ---------- | ------ | ------- |
| `strategy` | any Scope | `most-headroom`, `soonest-reset` | `most-headroom` |
| `watcher-may-act` | any Scope | `true`, `false` | `false` |
| `watcher-threshold-percent` | any Scope | 0–100 | `80` |
| `watcher-margin-percent` | any Scope | 1–100 | `10` |
| `interchangeable` | `ungrouped` only | `true`, `false` | `false` |

## A Setting is said about the Scope it governs

A **Scope** — each Group, and the Accounts in no Group taken together — holds
its own full Settings, and there is nothing above it. A Setting nobody has said
anything about is the compiled-in default rather than somebody else's value, and
an Account carries nothing at all: every Setting there is describes how Perch
chooses *between* Accounts, and a rule for choosing has nothing to say to a set
of one.

So every `set` names its subject: `perch config set <scope> <key> <value>`. One
that names no Scope is refused, because a rule with no subject is a rule about
nothing — and there is no word for "everywhere" to reach for instead.

```
$ perch config set work watcher-threshold-percent 70
`watcher-threshold-percent` on Group `work` is now 70.
`perch watcher run` Switches within Group `work` once that much of the fullest Quota Window of the Account you are on has been used. [...]

$ perch config set watcher-threshold-percent 70
`perch config set watcher-threshold-percent 70` names no Scope, and every Setting is said about the Scope it governs — there is nothing above them for a value to be set at. `perch config set <scope> watcher-threshold-percent 70` sets one. `ungrouped` addresses the Accounts in no Group. Groups Perch holds: personal, work.   # exit 14

$ perch config get
ungrouped interchangeable true
ungrouped strategy most-headroom
ungrouped watcher-may-act false
ungrouped watcher-threshold-percent 80
ungrouped watcher-margin-percent 10
personal strategy most-headroom
personal watcher-may-act false
personal watcher-threshold-percent 80
personal watcher-margin-percent 10
work strategy soonest-reset
work watcher-may-act false
work watcher-threshold-percent 70
work watcher-margin-percent 10
```

**Reading is not writing.** A bare `perch config get` prints every Scope's
Config in full, and `perch config get <scope>` prints one Scope's: a read has no
subject to be wrong about, and a write does. Every line is the whole of the
`perch config set` that would restore it, so reading the Config and writing it
back are the same vocabulary and a script needs no parser.

There is no `perch config unset`. With nothing above a Scope there is nothing to
clear — a value is simply set to what it should be. (`perch alias <target>
--unset` is untouched: freeing a name is a different act.)

## Scopes

A Scope is a Group by name, or `ungrouped` for the Accounts in no Group — which
are a Scope so that there is somewhere to say how they are Cycled, and never a
Group: a Group is a declaration somebody made, and this is the absence of one.
No Group can be called `ungrouped`, or the Scope would answer to the name first.

`global` is refused as a Group name and as an Alias too, for a different reason:
it is the word people reach for when they mean *everywhere*, and there is no
everywhere. The refusal is where you find that out, which is a better place to
learn it than from a Setting that appeared to take.

`interchangeable` is carried by `ungrouped` alone and is absent from a Group's
page. It is the declaration that those Accounts may be Cycled among at all, and
a Group **is** that declaration — printing the line against a Group and then
refusing to set it would break the rule the whole command rests on.

The watcher therefore needs **two independent yeses** among the Accounts in no
Group: `interchangeable`, saying they are a set worth moving between, and
`watcher-may-act`, letting something move between them unasked. A Group needs
only the second. Neither implies the other, which is why they are two things
rather than one said twice.

## Strategy

The **strategy** is which Account a Cycle prefers when more than one would
serve. `most-headroom` takes the one with the most room left; `soonest-reset`
takes the one whose fullest Quota Window comes back soonest, so quota that was
about to be thrown away is spent rather than wasted. How headroom is *measured*
is not configurable — it is always the worst window — so a strategy reorders the
Accounts that have room and can never promote an exhausted one.

A strategy says which figure to prefer, not which figures to invent. Cached
figures do not always carry a reset time, and `soonest-reset` ranks an Account
whose figure does not above nothing at all: an Account that says when it comes
back is preferred to one that does not, and where none of them says, the Cycle
falls back to the room it can see and says that is what it did.

## The watcher's three

The **watcher's** three fields govern [`perch watcher`](watching.md) and nothing
else. `watcher-may-act` says whether it may Switch within that Scope at all, and
is off by default because a Scope only ever changes underneath you because you
said it could. It is said about the Scope it grants and reaches no other, so a
Group declared afterwards is a Group nobody has said anything about — and there
is no one command that withdraws the watcher everywhere, which is the price of
consent that cannot arrive by inheritance. `watcher-threshold-percent` is how
much of the fullest Quota Window of the Account you are on has to be used before
it moves you. Neither of them starts a Watcher: they take effect while one is
running — the loop in a terminal, a Service, or a scheduled Check — and not
otherwise.

Taking `watcher-may-act` back does not stop a Watcher that is already running.
It **holds** it: it reads nothing and moves nothing, says what is missing, and
starts deciding again the moment the grant comes back. The grant is about
whether it may *act*, and a held Watcher is not acting.

`watcher-margin-percent` is the second half of the same question, and a
different one: how *empty* a candidate has to be before moving to it is worth
doing, in points under the threshold. At the default 10 and a threshold of 80,
nothing above 70% is moved to. Two knobs rather than one because the ceiling is
the threshold less the margin, so a single knob moves both: raising the
threshold to 90 to reach a ceiling of 80 also delays when you are moved off,
which is the opposite of what a conservative destination rule wants.

`0` is refused. An Account is left at or over the threshold and a candidate is
set aside above the ceiling, so at a margin of nothing an Account at exactly 80%
would be both full enough to leave and clear enough to arrive at. A margin wider
than the threshold is fine, and is a Scope that will only move onto an Account
with nothing used at all — a coherent thing to ask for, reached from either
side.

**The two numbers beside them are arithmetic, so they are fixed rather than
offered:**

- How often it **reads** — two and a half minutes, derived from Anthropic's
  allowance of ~28-30 reads an hour rather than from anyone's taste. A Group
  configured to read every ten seconds would be a Group configured to spend that
  allowance and be refused.
- The **cooldown** — 15 minutes — which is the least it leaves between two
  Switches. A five-hour window moves slowly enough that fifteen minutes never
  misses a real crossing, which is arithmetic about the window rather than a
  taste.

## Reading it back

Every line `perch config get` prints is the whole of the `perch config set` that
would restore it, so reading the Config and writing it back are the same
vocabulary and a script needs no parser. Naming a Scope and a key prints that one
line. An unknown key or a value that means nothing is refused with exit code 14
and the ones that do mean something, so a script that mistyped a Setting does not
go on believing it took — and so is a `set` with no Scope in it, which is the
same mistake made about the subject rather than the value.
