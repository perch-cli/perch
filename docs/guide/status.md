# Seeing what you have

Two commands answer it. `perch status` is about the Account you are on, and
`perch list` is about all of them. Neither touches the network unless you ask it
to.

- [The active Account](#the-active-account)
- [Reading current Utilization](#reading-current-utilization)
- [Every Account](#every-account)
- [JSON](#json)

## The active Account

```
$ perch status
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

Utilization is served from cache with its age shown, so `perch status` is cheap
enough to put in a shell prompt (ADR 0015).

`perch status --group` is the same view narrowed to the Group the active Account
is in, so you can see where you would land before you switch. From an Account in
no Group it shows every ungrouped Account and says that Cycling will not move
between them until you say it may (ADR 0017).

## Reading current Utilization

`perch status --refresh` is the one command that fetches. Everything else — and
`status` without the flag — reads the figure Perch last observed and says how
old it is.

```
$ perch status --refresh
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   5-hour    42%  (as of just now)
              7-day     18%  (as of just now)
              7-day-opus  3%  (as of just now)
```

Every Quota Window an Account has is recorded — the five-hour window, the
seven-day one, and one per model — each with how full it is and when it next
resets. `--refresh` reads the Accounts it is about to show you and no others:
`perch status --refresh` reads the one you are on, `perch status --group
--refresh` reads the ones it offers as landing places. Anthropic allows roughly
28-30 reads an hour per Account and the allowance does not refill early
(ADR 0015), so nothing spends one on an Account you did not ask about.

Reading an Account's Utilization needs a valid access token for it, so an
Account whose token has expired has its Credential renewed first — but only when
no client is running against that Profile (ADR 0005). Anthropic retires a
refresh token when it issues a new one, so renewing a Credential a running
Claude Code is holding in memory would log that session out silently, mid-task.
The Rotated Credential is written back into its own Profile under the same locks
a Switch takes.

Nothing about a refresh turns `status` into a failure. A throttled read, an
Account whose Credential Anthropic will not accept, one whose Profile is in use
— each is reported by name and leaves that Account's cached figure standing,
while every other Account is still read. `--json` carries the same under
`refresh`, which is `null` when no refresh was asked for.

## Every Account

`perch list` is the one place that answers it: every Account with its Alias, its
Group, whether it is a Cycle candidate, how much Headroom it has left and how
full each of its Quota Windows is.

```
$ perch list
  Account               Alias     Group  State        Headroom        Utilization
* someone@example.com   -         work   -            58%             5-hour  42%  (as of 3m ago)
                                                                      7-day   18%  (as of 3m ago)
  overflow@example.com  overflow  work   quarantined  never observed  never observed
  spare@example.com     -         none   disabled     9%              5-hour  91%  (as of 2h ago)

* is the active Account.
overflow@example.com (as `overflow`) is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

**State** says only what has been done to an Account, so it is empty for the one
nothing has: `disabled`, `quarantined` and `disabled, quarantined` are the only
things it prints. The two are separate facts with separate fixes — being out of
the Cycling pool is a decision you made, and a Quarantine is a Credential that
stopped working — so an Account in both says both.

**Headroom** is what is left in the Account's *worst* Quota Window, which is the
one honest measure of how much of it you can still spend: being blocked by any
window blocks you completely (ADR 0012). It is a different figure from the
Utilization beside it — Utilization is every window, one line each, and Headroom
is the single number a Cycle sorts on. The Account above has 42% of its 5-hour
window and 18% of its 7-day one spent, so 58% is left in every one of them.

**The rows come out in the order a Cycle ranks them**, Group by Group, with the
Group you are standing in first — so the top row of your Group is where a bare
`perch switch` would land, and the listing and the Switch cannot come to
disagree about which Account is better (ADR 0049). An Account a Cycle would
never choose — Disabled, or Quarantined — sorts below every one it would,
whatever its Headroom says, because where it sits is what says it is out of the
running.

The Accounts in no Group are the exception: they are listed in the order they
were added rather than ranked, because nothing has declared them
interchangeable and a bare `perch switch` refuses among them (ADR 0017). Their
Headroom is still shown as the figure it is. `perch config set ungrouped
interchangeable true` is the declaration, and once it is made they are ranked
like any Group.

An Account nobody has ever read a figure for says `never observed` rather than
`0%` — no figure and plenty of room are opposite pieces of advice. A
Quarantined Account stays listed and named, so an Account needing attention is
never mistaken for one that vanished; if it has also been taken out of the
Cycling pool the State column says both, because enabling a Quarantined Account
would not repair it. The reason it broke is written out under the table rather
than squeezed into a column, with the one command that puts it right.

## JSON

`perch status --json`, `perch list --json` and `perch status --group --json`
carry the same information, with an observation time on every figure and the
scope they were narrowed to. Neither listing makes a network call.
`quarantined` is `null` for an Account that works and an object — `reason` and
`detail` — for one that does not, so a script asking whether it is set reads the
same answer it always did and now gets the reason with it.

`--group` changes the question, so it changes the document: `perch status --json`
answers about one Account under `active`, while the listings answer about a set
under `sections`, with the active one named under `active_account`. An Account
itself is described the same way in both — the same keys, the same answers — so a
script written against one can be pointed at the other.

**A document says what its order is, or it does not have one.** The listing is
ranked and it is ranked per Scope, so its Accounts arrive in `sections` rather
than in one flat array: each section carries the `scope` it is of, an `order` of
either `ranked` or `held`, and its `accounts` in that order.

```json
{
  "scope": { "kind": "all", "name": null },
  "active_account": "someone@example.com",
  "sections": [
    {
      "scope": { "kind": "group", "name": "work" },
      "order": "ranked",
      "accounts": [ … ]
    },
    {
      "scope": { "kind": "ungrouped", "name": null },
      "order": "held",
      "accounts": [ … ]
    }
  ],
  "refresh": null
}
```

So `.sections[0].accounts[0]` is the Account a bare `perch switch` would land on,
and a section whose `order` is `held` is saying that its Accounts are in no
meaningful order at all — a flat array would have let a script read a ranking out
of that, which is the one claim this listing exists not to make.

Every Account carries `headroom` beside its `utilization`: a `state` of `room`,
`exhausted` or `never-observed`, and a `percent` that is a number under `room`
and `null` under the other two. Unrounded, unlike the column — no figure and
plenty of room are opposite pieces of advice, and neither of them is `0`.
