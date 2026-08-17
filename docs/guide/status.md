# Seeing what you have

Two commands answer it, and they answer two different questions. `perch status`
is about the Account you are on, in detail. `perch list` is about a set of them
as a table — everything Perch holds, or one Scope of it. Neither touches the
network unless you ask it to.

- [The Account you are on](#the-account-you-are-on)
- [Every Account](#every-account)
- [Reading current Utilization](#reading-current-utilization)
- [JSON](#json)

## The Account you are on

```
$ perch status
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

The Account, who it belongs to, what plan it is on, and how full it is — with a
Quarantine said above the figures when there is one, because a broken Credential
is the news and the numbers are the detail.

Utilization is served from cache with its age shown, so `perch status` is cheap
enough to put in a shell prompt (ADR 0015). That cheapness is a property of not
asking for a refresh rather than of this command: `perch list` is exactly as
cheap.

## Every Account

`perch list` is the one place that answers it, at every breadth (ADR 0053): every
Account with its Alias, its Group, whether it is a Cycle candidate, how much
Headroom it has left and how full each of its Quota Windows is.

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

### One Scope of it

`perch list <scope>` narrows the same table to one Scope of it — a Group by
name, or `ungrouped` for the Accounts in no Group, the same word `perch config`
addresses one by. Narrowed to a Group, what you are looking at is where a Cycle
could take you, which is what to read before you switch.

```
$ perch list work
Group `work`
  Account               Alias     State        Headroom        Utilization
* someone@example.com   -         -            58%             5-hour  42%  (as of 3m ago)
                                                               7-day   18%  (as of 3m ago)
  overflow@example.com  overflow  quarantined  never observed  never observed

* is the active Account.
Reserve: 1 of 1 Account has Headroom, the best 58% left (as of 3m ago)
1 Quarantined, so nothing Cycles to it.
overflow@example.com (as `overflow`) is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

The Group column goes, because the heading has already answered it. Narrowed to
the ungrouped, the table says what Cycling will and will not do with them
unasked (ADR 0017).

**The Reserve is what this Scope has left to draw on**: how many of the Accounts
a Cycle may choose still have Headroom, and how much the best of them has, with
the age of the reading that figure came from. It is a count and one Account's
own figure, and never one pooled number — Accounts sit on different plans and
Perch only ever sees percentages, so a `pro` Account at 50% and a `max` Account
at 50% do not have the same quota left, and adding them would produce a figure
that looks quantitative and is not.

An Account a Cycle may not choose is not part of what the Scope has, so a
Disabled or Quarantined one is named under the count rather than inside it — the
counts on screen add up to the Accounts on screen. Where nothing is left, the
line says what is in the way rather than only "none": exhausted, never observed,
or out of the running. And where the count rests on a reading older than the one
it quotes, a `Read 8h ago at the oldest.` line says so, because a count of
Accounts rests on all of their readings rather than on the freshest.

Among the ungrouped, the Reserve appears only once you have run `perch config set
ungrouped interchangeable true`. Until then nothing has declared those Accounts a
set, and what they have left *between them* is precisely the claim nobody has
made (ADR 0017) — the same reason they are held rather than ranked.

**A bare `perch list` says no Reserve** (ADR 0058). It is one table across every
Scope at once with the Group as a column and no heading to name which Scope a
sentence would be about, so each line would have to name its own — a heading
smuggled into a sentence already as wide as a terminal. `--json` is the exception
and carries one per section, because a section names its Scope in a key.

The Scope is named rather than implied, so `perch list` keeps working when Perch
holds no active Account — which is precisely the state `perch status` sends you
to `perch switch` to leave.

## Reading current Utilization

`--refresh` is the one thing in Perch that fetches. Without it — on either
command — you get the figure Perch last observed, with how old it is.

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
resets.

**A refresh reads the Accounts it is about to show you and no others.** So
`perch status --refresh` reads the one you are on, `perch list <scope> --refresh`
reads that Scope's, and `perch list --refresh` reads every Account Perch holds.
Anthropic allows roughly 28-30 reads an hour per Account and the allowance does
not refill early (ADR 0015), so nothing spends one on an Account you did not ask
about.

Reading an Account's Utilization needs a valid access token for it, so an
Account whose token has expired has its Credential renewed first — but only when
no client is running against that Profile (ADR 0005). Anthropic retires a
refresh token when it issues a new one, so renewing a Credential a running
Claude Code is holding in memory would log that session out silently, mid-task.
The Rotated Credential is written back into its own Profile under the same locks
a Switch takes.

Nothing about a refresh turns either command into a failure. A throttled read, an
Account whose Credential Anthropic will not accept, one whose Profile is in use
— each is reported by name and leaves that Account's cached figure standing,
while every other Account is still read. `--json` carries the same under
`refresh`, which is `null` when no refresh was asked for.

## JSON

`perch status --json` and `perch list [<scope>] --json` carry the same
information, with an observation time on every figure. Neither makes a network
call without `--refresh`. `quarantined` is `null` for an Account that works and
an object — `reason` and `detail` — for one that does not, so a script asking
whether it is set reads the same answer it always did and now gets the reason
with it.

Each document answers its own command's question, so the two shapes differ:
`perch status --json` answers about one Account under `active`, and the listing
answers about a set under `sections`, with the active one named under
`active_account` and the Scope it was asked for under `scope`. An Account itself
is described the same way in both — the same keys, the same answers — so a script
written against one can be pointed at the other. `jq .active.utilization` is what
a shell prompt reads off `status`.

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
      "reserve": {
        "candidates": 1,
        "with_headroom": 1,
        "exhausted": 0,
        "never_observed": 0,
        "out_of_the_running": 1,
        "best": {
          "email": "someone@example.com",
          "percent": 58.0,
          "observed_at": "2026-08-04T11:57:00+00:00"
        },
        "oldest_observed_at": "2026-08-04T11:57:00+00:00"
      },
      "accounts": [ … ]
    },
    {
      "scope": { "kind": "ungrouped", "name": null },
      "order": "held",
      "reserve": null,
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

**Every section carries its Scope's `reserve`, at every breadth** — including the
bare listing, where the table says none (ADR 0058). The table's silence is about
having nowhere to put the sentence; a section has somewhere, because it names its
own Scope in a key. As fields rather than the rendered sentence, because a prose
sentence in a document is a thing scripts end up regexing. The counts are
over the Accounts a Cycle may choose, so `with_headroom`, `exhausted` and
`never_observed` add up to `candidates`, and those plus `out_of_the_running` add
up to the section's own `accounts`. `best` is one Account's own figure, unrounded
and named, and `null` where nothing here has Headroom. `oldest_observed_at` is
the weakest reading the counts rest on.

`reserve` is `null` where nothing has declared the Scope's Accounts
interchangeable, saying the same "there is no answer here" the table says by
silence — and `null` for a Scope holding nobody, which the empty `accounts`
beside it tells apart.

Every Account carries `headroom` beside its `utilization`: a `state` of `room`,
`exhausted` or `never-observed`, and a `percent` that is a number under `room`
and `null` under the other two. Unrounded, unlike the column — no figure and
plenty of room are opposite pieces of advice, and neither of them is `0`.
