# perch

Run Claude Code as whichever Claude account you want, without going through the
login flow again.

## Status

Early. Perch adopts the login you already have, adds further Accounts without
disturbing it, names them, holds Groups of Accounts you have declared
interchangeable, lists what you have, reads how full each one is, switches to
an Account you name, picks one for you when you name none, logs an Account in
again when its Credential stops working, and takes its configuration from a
script.

```
$ perch status
Adopted the Claude Code login as your first Profile: you@example.com (Acme, pro)
It is now the active Account. Claude Code 2.1.221.

Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

`perch status --json` prints the same information, with an observation time on
every Utilization figure.

Neither form touches the network unless you ask it to: Utilization is served
from cache with its age shown, so `perch status` is cheap enough to put in a
shell prompt (ADR 0015).

`perch add` gains an Account by running a login in a Profile of its own, so the
Account you are using stays active and its session is untouched (ADR 0009).

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

## Switching

`perch switch <target>` makes an Account active everywhere — every terminal, the
editor extension, the desktop app — with no login flow.

```
$ perch switch overflow
`overflow` is an Alias for overflow@example.com.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com (as `overflow`).
Utilization   5-hour    12%  (as of 4m ago)
```

It is three steps in one order and never another (ADR 0006). The Credential you
are leaving is **Captured** back into its own Profile first, because Anthropic
retires a refresh token whenever it issues a new one — so the copy in an
Account's Profile is several Rotations behind by the time you switch away, and
skipping the Capture would quietly poison the Account you are leaving. Then the
incoming Credential is written to the Default Profile. Then the `oauthAccount`
block of `.claude.json` is patched to match, and only that block: your project
history, MCP configuration and settings live in the same file and belong to you
rather than to the Account (ADR 0001).

All three run inside Claude Code's own OAuth refresh locks, taken in Claude
Code's order — the refresh lock, the legacy config-home lock, then the config
file lock — so a refresh cannot land between the Capture and the write. A lock
somebody is holding is waited on and then given up on; one whose holder died is
taken over.

Nothing else moves. Your memory, settings, plugins and project history are
Shared State: a Switch leaves them untouched, which is what makes them follow
you across Accounts for free.

A Switch onto a Profile a client is already running against is refused with exit
code 16 rather than writing a Credential something else is holding, and
switching to the Account that is already active does nothing and exits 15. If a
Switch fails part way, it says which Account is active now and what is where —
running it again finishes the job.

## Cycling

`perch switch` with no target picks the Account for you. It is the command you
type mid-task when quota just ran out, so it asks nothing, under any
circumstances (ADR 0011).

```
$ perch switch
Cycling within Group `work`.
overflow@example.com has the most room: 60% headroom, which is true of every one of its Quota Windows — 7-day is its fullest, as of 4m ago.
Captured you@example.com's live Credential into its own Profile.
Switched to overflow@example.com.
Utilization   5-hour    12%  (as of 4m ago)
              7-day     40%  (as of 4m ago)
That figure is what Perch last observed rather than what Anthropic says now. If overflow@example.com turns out fuller than it implied, the figure was stale — `perch status --refresh` reads a current one.
```

It Cycles **within the current Account's Group** and never leaves it, so a work
subscription running dry does not land you on your personal Account. `perch
switch <group>` Cycles within a Group you name instead.

Each Account is ranked by its **worst** Quota Window, and the Account whose
worst is best wins (ADR 0012). Being blocked by any window blocks you
completely, so that is the only ranking that measures what actually stops you
working: the headroom Perch reports is true of every one of that Account's
windows. Exhausted, disabled and Quarantined Accounts are never chosen, and an
Account nobody has ever read a figure for is ranked below every Account that
has one — no figure and plenty of room are opposite pieces of advice.

How headroom is measured is fixed; which Account to prefer is the Group's to
say. `perch config set <group> strategy soonest-reset` makes a Cycle take the
Account whose fullest window comes back soonest instead of the one with the
most room, so perishable quota is spent rather than wasted — see
[Configuration](#configuration). It reorders the Accounts that have room and
nothing else: an exhausted Account is never chosen however soon it resets.

Ranking reads the cache and never the network, so the figures can be minutes
old. Landing on an Account that turns out fuller than they implied is the cache
being stale rather than the Switch going wrong, which is why every Cycle says so
before you find out.

Three outcomes are honest non-outcomes. They perform no Switch, explain
themselves, and exit with a code of their own rather than pretending to have
worked.

```
$ perch switch
Cycling within Group `work`.
Every Account in Group `work` is exhausted, so there is nowhere useful to Switch. Nothing was changed.
you@example.com frees up soonest, at 2026-08-04 15:00 UTC (in 3h).   # exit 17

$ perch switch
Cycling within Group `work`.
you@example.com is already the best Account in Group `work`, with 90% headroom, which is true of every one of its Quota Windows — 5-hour is its fullest, as of 4m ago. Nothing was changed.   # exit 15

$ perch switch
you@example.com is in no Group, so nothing has declared which Accounts it is interchangeable with. Nothing was changed.
Either put it in a Group with `perch group move you@example.com <group>`, or declare that every ungrouped Account is interchangeable with `perch config set cycle-ungrouped true`.   # exit 18
```

That last one is ADR 0017. An Account need not be in a Group — adoption leaves
the first one ungrouped — but being ungrouped is the *absence* of a declaration
that Accounts are interchangeable, not a weaker form of one. So bare `perch
switch` Cycles among ungrouped Accounts only when a global setting says it may,
and that setting is off until you turn it on.

## Reserving an Account

`perch disable` keeps an Account out of Cycling without giving it up — for the
subscription you are holding for one particular thing and would rather Perch did
not spend on something else.

```
$ perch disable spare
`spare` is an Alias for spare@example.com.
Disabled spare@example.com (as `spare`). Cycling will not choose it — it stays listed and named, and `perch switch` still switches to it when you name it.

$ perch enable spare
`spare` is an Alias for spare@example.com.
Enabled spare@example.com (as `spare`). It is a Cycle candidate again.
```

A disabled Account is excluded from Cycling and from nothing else. It keeps its
Alias, its Group and its stored Credential, `perch list` shows it as disabled,
and naming it on `perch switch` still switches to it — so putting it back needs
no login, only `perch enable`. Removing the Account is the blunt instrument this
exists to avoid.

Disabling every Account in a Group is allowed. A bare `perch switch` there then
reports having no candidate (exit 17) rather than quietly landing you on
something you had reserved.

## When an Account breaks

A Credential can stop working for good: Anthropic retires a refresh token, a
Rotation is lost between two writes, a login is ended somewhere else. Perch
never drops such an Account — an Account that vanishes reads as data loss, and a
broken one reads as something needing attention. It is **Quarantined**: still
listed, still named, shown as broken, and shown with the reason.

```
$ perch status --refresh
overflow@example.com is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

Cycling never chooses a Quarantined Account, and naming one on `perch switch` is
refused with exit code 19 rather than making a Credential live that does not
work — which would cost you the Account you are on. Enabling one does not repair
it: whether Cycling may choose an Account and whether its Credential works are
separate facts with separate fixes, so both are always said.

`perch relogin <target>` is the fix, and it repairs **in place**.

```
$ perch relogin overflow
`overflow` is an Alias for overflow@example.com.
Logging in again to repair overflow@example.com. someone@example.com stays active and its session is untouched.
Quit Claude Code when the login is done to come back here.

Repaired overflow@example.com (as `overflow`) — it is no longer Quarantined, and is a Cycle candidate again if it was one before.
Alias:   overflow
Group:   work
Cycling: may choose it
```

The Account keeps its Alias, its Group, whether Cycling may choose it and its
place in the listing — only the Credential is replaced. The login runs in a
directory of its own, so the Account you are working in is untouched throughout,
including when the login is abandoned, which changes nothing at all. A login as
a different Account is refused: an Alias you chose for one Account is not handed
to another because a browser was signed into somebody else.

Relogging in the Account you are **on** also makes its fresh Credential the live
one, because a repair only its own Profile can see would leave the Account broken
everywhere it is actually used (ADR 0023). A healthy Account may be relogged in
too — nothing about the command depends on the Quarantine.

## What you have

`perch list` is the one place that answers it: every Account with its Alias, its
Group, whether it is a Cycle candidate, and how full it is.

```
$ perch list
  Account               Alias     Group  State                 Utilization
* someone@example.com   -         work   enabled               5-hour    42%  (as of 3m ago)
                                                               7-day     18%  (as of 3m ago)
  overflow@example.com  overflow  work   enabled, quarantined  never observed
  spare@example.com     -         none   disabled              5-hour    91%  (as of 2h ago)

* is the active Account.
overflow@example.com (as `overflow`) is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

An Account nobody has ever read a figure for says `never observed` rather than
`0%` — no figure and plenty of room are opposite pieces of advice. A
Quarantined Account stays listed and named, so an Account needing attention is
never mistaken for one that vanished; whether it is in the Cycling pool is said
alongside, because enabling a Quarantined Account would not repair it. The
reason it broke is written out under the table rather than squeezed into a
column, with the one command that puts it right.

`perch status --group` is the same view narrowed to the Group the active
Account is in, so you can see where you would land before you switch. From an
Account in no Group it shows every ungrouped Account and says that Cycling will
not move between them until you say it may (ADR 0017).

`perch list --json` and `perch status --group --json` carry the same
information, with an observation time on every figure and the scope they were
narrowed to. Neither makes a network call. `--group` changes the question, so
it changes the document: `perch status --json` answers about one Account under
`active`, while the listings answer about a set under `accounts`, with the
active one named under `active_account`.

## Names

An Alias is a short name for an Account, so no command ever needs an email
address.

```
$ perch alias overflow overflow@example.com
`overflow@example.com` is an Account.
`overflow` now names overflow@example.com.

$ perch alias overflow --unset
`overflow` no longer names overflow@example.com.
```

Aliases and Group names share one namespace: neither can take a name the other
already has, and two names that differ only in case are one name. So the one
Target every command takes is never ambiguous. An Account answers to one Alias
at a time — naming an Account that already has a name replaces it, and says
which name it gave up.

A Target resolves as Alias, then Account email, then Group, and the command
says which one matched before it acts. A Target that matches nothing is refused
with exit code 12 and the names it nearly matched.

## Groups

A Group is your statement that a set of Accounts is interchangeable — another
work subscription, never your personal Account. Cycling will only ever move
between Accounts in one Group (ADR 0002).

```
$ perch group add work
$ perch group move overflow@example.com work
$ perch group list
work
  Accounts     overflow@example.com (as `overflow`)
  Strategy     most-headroom
  Watcher      off (would act at 80%)

In no Group
  Accounts     you@example.com
  Cycling      only moves between these when you say it may
```

`perch group move <target> none` takes an Account out of every Group, and a
Group that still holds Accounts is not removed until they have somewhere to go.

## Configuration

`perch config` changes how a Group behaves, and asks nothing: every capability
Perch has is reachable from a script, because it has to be complete over SSH
and in CI (ADR 0011).

```
$ perch config set work strategy soonest-reset
`strategy` on Group `work` is now soonest-reset.
A Cycle within `work` prefers the Account whose fullest Quota Window resets soonest, so perishable quota is spent rather than wasted. Headroom is still measured by the worst window (ADR 0012), so an exhausted Account is still never chosen however soon it comes back.

$ perch config get
cycle-ungrouped true
work strategy soonest-reset
work watcher-may-act false
work watcher-threshold-percent 80
```

Three words name a Group; two do not. Most configuration belongs to a Group,
because a Group is what carries the rules governing Cycling within it (ADR
0002) — but whether bare `perch switch` may Cycle among the Accounts in **no**
Group is about Accounts with no Group to carry it, so it is global and is
addressed by naming none (ADR 0017).

| Key | Belongs to | Values | Default |
| --- | ---------- | ------ | ------- |
| `strategy` | a Group | `most-headroom`, `soonest-reset` | `most-headroom` |
| `watcher-may-act` | a Group | `true`, `false` | `false` |
| `watcher-threshold-percent` | a Group | 0–100 | `80` |
| `cycle-ungrouped` | no Group | `true`, `false` | `false` |

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

The **watcher's** two fields are stored and validated and read by nothing: the
watcher is deferred entirely (ADR 0013), and every message about them says so.
`watcher-may-act` is off by default, because a Group only ever changes
underneath you because you said it could.

Every line `perch config get` prints is the tail of the `perch config set` that
would restore it, so reading the configuration and writing it back are the same
vocabulary. Naming one setting prints its value alone, for a script to read
without parsing prose; naming a Group prints what that Group carries. An unknown
key or a value that means nothing is refused with exit code 14 and the ones that
do mean something, so a script that mistyped a setting does not go on believing
it took.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | fine |
| 1 | something else went wrong |
| 2 | the command line was not understood |
| 10 | refused: an assumption about the installed Claude Code failed (ADR 0007) |
| 11 | the keychain is locked, denied, or unavailable |
| 12 | there is no such thing — no login, no such Account, no such Group |
| 13 | it collides with something Perch already holds |
| 14 | Perch understood it and will not accept it — an ambiguous name, a value out of range |
| 15 | there was nothing to do — you are already on that Account |
| 16 | refused: a client is running against that Profile, so its Credential is not Perch's to write |
| 17 | a Cycle found nowhere to land — every Account in the Group is exhausted, or none is a candidate |
| 18 | a bare Cycle from an Account nobody has declared interchangeable with anything (ADR 0017) |
| 19 | that Account is Quarantined — its Credential no longer works, and `perch relogin` is the fix (ADR 0023) |

## Where things are

- `~/.perch/registry.json` — Perch's own state, versioned.
- `~/.perch/profiles/<account>/` — one directory per Account. Its path is what
  gives that Account a private Credential Store (ADR 0001).
- `$PERCH_HOME` overrides `~/.perch`. Home is `$USERPROFILE` on Windows and
  `$HOME` elsewhere; a machine that cannot say where home is gets a refusal,
  never a write into the filesystem root.
- `$PERCH_CLAUDE_BIN` overrides where `claude` is found. Without it, Perch
  walks `PATH` itself — consulting `PATHEXT` on Windows, so the `claude.cmd`
  an npm install leaves works from every shell.

A Credential lives wherever the installed Claude Code would put it (ADR 0020):
the keychain on macOS, reached by driving `/usr/bin/security` (ADR 0008), and
a `.credentials.json` inside the Profile everywhere else — created readable by
its owner alone, and tightened if it is ever found looser. Perch drives `curl`
by absolute path — `/usr/bin/curl`, or `%SystemRoot%\System32\curl.exe` on
Windows — to reach Anthropic, with the URL, the headers and the body all
handed over on standard input: an access token passed as an argument would sit
in the process table for anything on the machine to read.

## Building

Builds and runs on macOS, Linux and Windows, with the same command surface
everywhere. The toolchain is pinned in `rust-toolchain.toml` — Rust 1.97.1,
edition 2024 — so rustup will fetch the right one on first build.

```
# touches nothing on the machine
cargo test --lib --test adoption --test status --test adding --test grouping \
           --test naming --test listing --test switching --test cycling \
           --test refreshing --test storing
# asserts beliefs against this machine
cargo test --test contract
# both
cargo test
```

On macOS the contract tests read and write items of their own in the login
keychain, under `Perch contract test-*`, and delete them again. They never
write Claude Code's item. Set `PERCH_SKIP_KEYCHAIN_CONTRACT=1` to skip them
where the keychain cannot be unlocked — it is macOS-only, because only macOS
compiles those tests in. The file-store contract tests need no opt-out: they
touch only a temporary directory of their own.

## Design

`CONTEXT.md` for the vocabulary, `docs/adr/` for the decisions.
