# Choosing by eye

`perch tui` opens the interactive view: two tabs, one for where you are and one
for what governs you.

`Status` answers **where am I** — the Account Claude Code is about to run as, the
Accounts you could go to, and the Settings in force for you. `Config` answers
**what does each Scope declare, and let me change it**.

`Tab` moves between the tabs, the up and down arrows move down a sidebar, the
right arrow steps into the page beside it and the left arrow comes back, `q` and
Ctrl-C leave, and `r` reads current Utilization. `>` is the row the keys act on
and `*` is the active Account — characters rather than colours, so the view reads
the same over SSH on a terminal that has none.

## Status

`Overview` is the summary: who you are running as, by the name you gave them,
what a bare Switch would Cycle within, and how much is left.

```
$ perch tui
 Status | Config                                    active: someone@example.com
> Overview
  Accounts   Account       someone@example.com
  Config     Plan          pro
             Group         work — a bare Switch Cycles within it
             Headroom      18% (7-day is its fullest, as of 4m ago)

             5-hour 42% used resets 2026-08-04 15:00 UTC (in 3h) (as of 4m ago)
             7-day 82% used resets 2026-08-06 14:00 UTC (in 2d) (as of 4m ago)

             Across Group `work`:
             Reserve: 2 of 2 Accounts have Headroom, the best 39% left (as of 4m
               ago)
             5-hour emptiest 7% used across 2 Accounts (as of 4m ago)
             7-day emptiest 61% used across 2 Accounts (as of 4m ago)

q quit  Tab view  arrows move  Enter switch  x run  r refresh
```

Every figure carries its age (ADR 0015). Each Quota Window gets a row of its
own, because an Account has several at once and is limited by whichever fills
first — 90% that resets in twenty minutes and 90% that resets in four hours are
the same number and opposite advice. An Account nobody has ever read a figure
for says so rather than showing a zero.

**Headroom** is how much is left to spend, taken from the most constrained
window and naming which, so an Account is only ever as free as its fullest
window and a generous-looking figure never hides an exhausted one (ADR 0012).

**Across** the Scope is its **Reserve**: what the Group has left to draw on,
said as how many of its Accounts still have Headroom and how much the best of
them has, then one row per Quota Window kind — the emptiest Account in that
window, which is the best the Group can currently offer there. A Disabled or
Quarantined Account is not part of it: a Credential that does not work is quota
nothing can spend.

**There is no total, anywhere.** Never one pooled figure across Accounts.
Accounts sit on different plans and Perch only ever sees percentages — a `pro`
Account at 50% and a `max` Account at 50% do not have the same quota left, and
Perch never sees the allowance behind either. Summing or averaging them produces
a number that looks quantitative, isn't, and is exactly the kind of number
people plan around. So every percentage here is one an Account actually
reported, and the per-window rows are the only figure drawn across a Group's
Accounts at all, because within one window kind the comparison at least means
something. Being ungrouped is the absence of a declaration that Accounts are
interchangeable (ADR 0017), so those Accounts get no Reserve until
`perch config set cycle-ungrouped true` says a Cycle may move between them.

Where a Quarantine is what is in the way, the reason goes where the Headroom
would be, with the command that ends it. On a machine Perch has been left on
nobody, one line says so and points at the page that has something to do about
it.

`Accounts` is the full table, on a row of its own so that "where am I" and
"where could I go" are not competing for one page.

```
 Status | Config                                    active: someone@example.com
  Overview    Account               Alias     Group  State    Headroom
> Accounts >  overflow@example.com  overflow  work   enabled  39%
  Config    * someone@example.com   -         work   enabled  18%

q quit  Tab view  arrows move  Enter switch  x run  r refresh
```

They are listed in the order `perch switch` would rank them, with the Headroom
they were ranked on beside them, so the ranking is visible rather than hidden.
Cycling never leaves the Scope it started in (ADR 0002), so the listing is one
ranking per Scope, with the Scope the active Account is in first. A Disabled or
Quarantined Account is listed like any other and sorts below every Account a
Cycle would choose. The Accounts in no Group are listed in the order they were
added rather than ranked until `cycle-ungrouped` says a Cycle may move between
them — a ranking of Accounts Perch would not choose between is exactly the
hidden claim this listing exists not to make.

`Config` is what governs *you*: every Setting in force for the active Account's
Scope, with where each one came from.

```
 Status | Config                                    active: someone@example.com
  Overview
  Accounts   In force for Group `work`:
> Config
             strategy                   most-headroom  from Global
             watcher-may-act            false          from Global
             watcher-threshold-percent  55             set on `work`

q quit  Tab view  arrows move  Enter switch  x run  r refresh
```

Only what governs you: a Setting belonging to a Group you are not in is not a
rule about you, and does not appear. Where a value came from is **written out**
rather than shown only as a style, because this page is read once and has to
survive a pipe and a colour-blind palette.

## Config

The sidebar is `Global`, then `Ungrouped`, then every Group, then the row that
declares another. Each page shows that Scope's Settings as editable rows, with
the Accounts it governs beside them.

```
 Status | Config                                    active: someone@example.com
> Global        Global
  Ungrouped     What applies where nothing narrower is said.
  work          cycle-ungrouped            false
  + new Group   strategy                   most-headroom
                watcher-may-act            false
                watcher-threshold-percent  80

q quit  Tab view  arrows move/step  Spc flip  Esc inherit  n name
```

```
 Status | Config                                    active: someone@example.com
  Global      > someone@example.com   Group `work`
  Ungrouped     overflow              Overrides 1 Setting; the dimmed rows are
> work                                  Global's.
  + new Group                         strategy                   most-headroom
                                      watcher-may-act            false
                                      watcher-threshold-percent  55
                                      alias                      (none)
                                      cycling-may-choose         true
                                      group                      work
                                      plan                       pro
                                      quarantine                 none

q quit  Tab view  arrows move/step  Spc flip  Esc inherit  n name
```

An **Inherited** value is shown dimmed, with Global's value on the row rather
than a blank: what is in force is what you need to know, and a blank would send
you to Global to find out. The dimming is what says this Scope declares nothing
of its own — on a page navigated with a cursor, where a dimmed value reads as
"not set here".

`Space` flips a true/false Setting. The left and right arrows step a Strategy
between the readings there are and a percentage in fives; `Home` and `End` take
a number to the ends of its range. `Esc` clears an Override so the Scope
Inherits Global again — and at Global it does nothing and says why, because
Global has nothing above it to Inherit from. `n` types a name, which is the one
place the panel takes typed input: a name is the only value with no natural
step, and a collision with an existing Alias or Group is refused as you confirm
it, before anything is written.

`n` names three things and nothing else: a new Group on the last row of the
sidebar, the Group the sidebar is on — the field opens with its current name in
it, so correcting four characters is four keystrokes — and the selected Account
on its `alias` row. On `Global` and on `Ungrouped` it says neither is a Group
somebody named, because Global is what applies where nothing narrower is said
and being Ungrouped is the absence of a declaration rather than one (ADR 0017).
A rename here is `perch group rename`, so it keeps what the Group carries and is
refused in the same words the command would have used.

**There is no save button.** A change is written when it is made. Stepped values
are debounced — the write follows the last keystroke rather than each one,
because holding an arrow from 0 to 80 is otherwise sixteen writes and sixteen
lock acquisitions. That is a deferred write and not a save button: nothing has
to be remembered, and walking away loses nothing.

## It writes what it can unwrite

Settings, Aliases, whether Cycling may choose an Account, which Group it is in,
declaring a Group, renaming one, `Enter` to Switch and `x` to Run. That is the
whole of what the view does, and the line is **reversibility** (ADR 0034): the
TUI may write what it can unwrite. Renaming a Group is on the right side of that
line because a rename keeps everything the Group carries — renaming it back
restores exactly what was there.

`add`, `remove`, `relogin`, `purge`, `export` and `import` stay out, and so does
deleting a Group — the Accounts survive it and become Ungrouped, but that
Group's Overrides do not, and a value nobody can get back is exactly the loss
this rule refuses. `perch group remove` still does it.

Every write **is** the command: a Setting written here is written by
`perch config`, an Alias by `perch alias`, a Group move or a rename by
`perch group`. So the refusals, the ranges and the locking are theirs rather
than a second copy kept in step by hand — and what the command printed is what
appears above the keys.

The exclusive lock is taken per edit and given straight back, never held for the
life of the screen: holding it would make an open TUI a denial of service
against `perch watcher run`, which takes the same lock every round. The cost is
that an edit can be refused — by another `perch` holding the lock, or while a
Refresh is out — and a refusal is shown where a failed Refresh is shown, with the
row back to what was actually written rather than showing a value that never was.

A Switch from the view is the same Switch as everywhere — the outgoing
Credential Captured first, Claude Code's locks taken, the Identity patched — and
switching to the Account that is already active says there is nothing to do
rather than rewriting Credentials for nothing. A Run hands the terminal over:
the view ends, the terminal goes back, and Claude Code starts against that
Account's Profile with the active Account untouched, returning with the status
the client exited with — the same as `perch run`, because it is `perch run`.

Selecting a Quarantined Account names `perch relogin` rather than failing
obscurely, whichever of the two keys was pressed, and nothing is changed.

The first frame is drawn from cache and never waits on the network (ADR 0015),
which is why it appears at once and why every figure on it says how old it is. A
picker that hangs before showing anything is worse than one showing numbers a
few minutes old and saying so.

`r` is the only thing here that reads from Anthropic, and it is taken off the
frame loop: the display goes on redrawing, goes on answering the keys and goes
on leaving on Ctrl-C while Anthropic is being waited on. It reads the Accounts
on screen and no others, like every other Refresh. One that fails leaves every
figure standing with the age it had and says what could not be read (ADR 0018).

Nothing here is only here. The interactive view is one command among several
rather than the primary surface (ADR 0011), so everything it shows and
everything it does has a plain command form — `perch list`, `perch status`,
`perch config`, `perch switch`, `perch run` — and where there is no terminal to
draw in, `perch tui` refuses and names them rather than trying.

The terminal is given back however the view ends: on `q`, on an error, and on a
panic. A TUI that dies in raw mode is one you have to `reset` your way out of.
