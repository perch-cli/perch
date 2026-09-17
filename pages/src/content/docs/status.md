---
title: "Seeing what you have"
sidebar:
  order: 3
---

`perch status` is about the Account you are on. `perch list` is about a set of
them: everything Perch holds, or one Scope of it. Neither touches the network
unless you pass `--refresh`.

Each provider has its own active Account. `perch status` reports one, and
`perch list` marks it: the provider whose Accounts you hold, or with both held
the Run preference, unless `--claude`, `--codex` or `--provider <name>` names
the other.

## The Account you are on

```
$ perch status
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   5-hour  42%  (as of 4m ago)
              7-day   18%  (as of 4m ago)
```

The figures are the ones Perch last observed, with their age. Nothing is
fetched, so this can sit in a shell prompt. A Quarantine is said above the
figures when there is one. An Account never read says `never observed`.

## Every Account

```
$ perch list
  Account               Alias     Group  State        Headroom        Utilization
* you@example.com       -         work   -            58%             5-hour  42%  (as of 4m ago)
                                                                      7-day   18%  (as of 4m ago)
  overflow@example.com  overflow  work   quarantined  never observed  never observed
  spare@example.com     -         none   disabled     9%              5-hour  91%  (as of 4m ago)
                                                                      7-day   20%  (as of 4m ago)

* is the active Account.
overflow@example.com (as `overflow`): the provider would not renew its Credential.
`perch relogin overflow@example.com` repairs it.
```

**State** is `disabled`, `quarantined`, both, or empty. **Headroom** is what is
left in the Account's worst Quota Window, the one figure a Cycle ranks on.
**Utilization** is every window, one line each.

The rows come out in the order a Cycle ranks them, Group by Group, your Group
first. The top row of your Group is where a bare `perch switch` lands. Disabled
and Quarantined Accounts sort below every Account a Cycle would choose. The
Accounts in no Group are listed in the order they were added until you run
`perch config set ungrouped interchangeable true`.

The reason an Account is Quarantined is under the table, one line per broken
Account, and the repair once beneath them.

### One Scope of it

```
$ perch list work
Group `work`
  Account               Alias     State        Headroom        Utilization
* you@example.com       -         -            58%             5-hour  42%  (as of 4m ago)
                                                               7-day   18%  (as of 4m ago)
  overflow@example.com  overflow  quarantined  never observed  never observed

* is the active Account.
Reserve: 1 of 1 Account has Headroom, the best 58% left (as of 4m ago)
1 Quarantined, so nothing Cycles to it.
overflow@example.com (as `overflow`): the provider would not renew its Credential.
`perch relogin overflow@example.com` repairs it.
```

`perch list <group>` is where a Cycle could take you. The **Reserve** is how
many Accounts a Cycle may choose still have Headroom, and how much the best of
them has. It is never one pooled figure. Disabled and Quarantined Accounts are
named under the count rather than inside it. A `Read 8h ago at the oldest.`
line appears when the count rests on a reading older than the one it quotes.

```
$ perch list ungrouped
In no Group
  Account            Alias  State     Headroom  Utilization
  spare@example.com  -      disabled  9%        5-hour  91%  (as of 4m ago)
                                                7-day   20%  (as of 4m ago)

Cycling off — `interchangeable` is false.
```

`perch list ungrouped` is the Accounts in no Group. The Reserve appears there
once `interchangeable` is on. A bare `perch list` shows no Reserve.

`perch list` keeps working when Perch holds no active Account, which is the
state `perch status` sends you to `perch switch` to leave.

## Reading current Utilization

```
$ perch status --refresh
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   5-hour      42%  (as of just now)
              7-day       18%  (as of just now)
              7-day-opus   3%  (as of just now)
```

`--refresh` reads the Accounts about to be shown and no others: `perch status
--refresh` reads the one you are on, `perch list <scope> --refresh` that
Scope's, and `perch list --refresh` every Account Perch holds. Every Quota
Window Anthropic reports is recorded, with when it resets.

A read that fails leaves the cached figure standing and says so above the
table. The command still succeeds:

```
$ perch status --refresh
you@example.com: Anthropic is rate-limiting reads of this Account (about 28-30 an hour).
Account       you@example.com
Organization  Acme
Plan          pro
Utilization   5-hour  42%  (as of 4m ago)
              7-day   18%  (as of 4m ago)
```

An Account whose access token has expired is renewed first, unless a client is
running against its Profile. While a Watcher is running, a `--refresh` of the
Account it watches inside its 2m30s interval shows the figure the Watcher just
read, and says so.

## JSON

```
$ perch status --json
{
  "active": {
    "account_uuid": "account-uuid-1",
    "active": true,
    "alias": null,
    "disabled": false,
    "email": "you@example.com",
    "group": "work",
    "headroom": {
      "percent": 58.0,
      "state": "room"
    },
    "organization": "Acme",
    "plan": "pro",
    "profile_dir": "/Users/you/.config/perch/profiles/you-example-com",
    "quarantined": null,
    "utilization": {
      "never_observed": false,
      "observed_at": "2026-08-04T11:56:00+00:00",
      "windows": [
        {
          "group": "session",
          "observed_at": "2026-08-04T11:56:00+00:00",
          "observed_seconds_ago": 240,
          "resets_at": null,
          "used_percent": 42.0,
          "window": "5-hour"
        },
        {
          "group": "weekly",
          "observed_at": "2026-08-04T11:56:00+00:00",
          "observed_seconds_ago": 240,
          "resets_at": null,
          "used_percent": 18.0,
          "window": "7-day"
        }
      ]
    }
  },
  "landing": null,
  "refresh": null
}
```

`jq .active.utilization` is what a shell prompt reads off `status`. An Account
has the same keys in both documents.

`headroom.state` is `room`, `exhausted` or `never-observed`, and `percent` is a
number only under `room`. `quarantined` is `null` for an Account that works and
an object with `reason` and `said` for one that does not.

`landing` is `null` unless the machine is part way through a Switch, when it
names `leaving` and `arriving`. In that window `active` can be `null`, so read
`landing` before concluding there is no Account.

`refresh` is `null` when no `--refresh` was asked for. Otherwise its `accounts`
carry one entry per Account read, with an `outcome` of `observed`, `throttled`,
`just_read`, `failed`, `quarantined` or `stopped`, and a `detail`. `kept` says
whether the figures reached Perch's own record, and `not_kept` says why not.

```
$ perch list --json
{
  "active_account": "you@example.com",
  "landing": null,
  "refresh": null,
  "scope": {
    "kind": "all",
    "name": null
  },
  "sections": [
    {
      "accounts": [ … ],
      "order": "ranked",
      "reserve": {
        "best": {
          "email": "you@example.com",
          "observed_at": "2026-08-04T11:56:00+00:00",
          "percent": 58.0
        },
        "candidates": 1,
        "exhausted": 0,
        "never_observed": 0,
        "oldest_observed_at": "2026-08-04T11:56:00+00:00",
        "out_of_the_running": 1,
        "with_headroom": 1
      },
      "scope": {
        "kind": "group",
        "name": "work"
      }
    },
    {
      "accounts": [ … ],
      "order": "held",
      "reserve": null,
      "scope": {
        "kind": "ungrouped",
        "name": null
      }
    }
  ]
}
```

The listing arrives in `sections`, one per Scope, each with its `order`.
`.sections[0].accounts[0]` is where a bare `perch switch` lands. A section
whose `order` is `held` is in no meaningful order.

Every section carries its Scope's `reserve`, including the bare listing.
`with_headroom`, `exhausted` and `never_observed` add up to `candidates`, and
those plus `out_of_the_running` add up to the section's `accounts`. `reserve`
is `null` where nothing has declared the Scope's Accounts interchangeable, and
for a Scope holding nobody.
