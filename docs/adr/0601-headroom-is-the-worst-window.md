# Headroom is the worst window

An Account has several Quota Windows at once — five-hour, seven-day, and a
weekly window per model — and they disagree. One Account can sit at 10% on its
five-hour window and 95% on its seven-day while another is the reverse, so the
two rank in opposite orders depending on which window is read.

Perch takes each Account's worst window and picks the Account whose worst is
best. Being blocked by any window blocks you completely, so this is the only
ranking that measures what actually stops work: when Perch reports 40% Headroom,
that is true of every window, and nothing surprising blocks you five minutes
later.

Ranking on the five-hour window alone is refused. It is the window you hit first
and the simplest to explain, and it will switch you onto an Account about to die
on its weekly limit — failing at the exact moment the feature exists to help,
which reads as the tool being broken. A weighted composite of the windows is
refused as unpredictable and undebuggable.

## The measurement, and the preference on top of it

This fixes how Headroom is *measured*, not which Account to prefer. Which
Account to prefer is the Scope's to say (ADR a-group-is-a-declaration) — the
most Headroom, or the soonest-resetting window so perishable quota is spent
rather than wasted — and every such Strategy reads Headroom the same way.

**A Strategy is an axis on top of the measurement and never a way round it.** It
reorders the Accounts the measurement leaves standing; it cannot promote one the
measurement rules out, so an exhausted Account is never chosen however soon it
comes back. Two rules follow, and neither is implied by that sentence alone:

- **A Strategy says which figure to prefer, not which figures to invent.** Asked
  to rank on the soonest reset where nothing has one, it ranks on the room it can
  see and says which of the two it chose on. Sorting on the order the Accounts
  were added would be switching on nothing.
- **A Strategy says which of the places worth going is preferred, not that
  nowhere is worth going.** Whether moving gains anything is settled against the
  Account being left on both orderings — the Strategy's, and the room alone — and
  the Strategy picks among the Accounts that clear it. Asked on its own ranking,
  a `soonest-reset` Scope pins you to the fullest Account in it, because the
  Account you are on is the one that resets soonest. Asked of the veto but not of
  the choice, one Account clearing it lets the Strategy hand back another that
  the Account being left beat on both counts.

## One window answers both questions

The window that decides an Account's Headroom is the window whose reset decides
how perishable that Headroom is, so one Quota Window answers both and the two
Strategies cannot end up reading different windows.

Two windows equally full is the ordinary case rather than the exotic one:
Anthropic answers in whole percentages, and an Account nothing has been spent on
is at nought in every window. **A tie is broken on perishability** — the soonest
reset first, and a window that says when it comes back ahead of one that does
not. An Account whose five-hour quota is thrown away in an hour, tied with a
per-model window carrying no reset, is constrained by the five-hour one.

**An Account nothing has ever been observed of is never read as room.** "No
figure" and "plenty of room" are opposite pieces of advice. It ranks below every
Account with room and above every Account whose window is full: treating an
unknown as good news is the mistake the ordering exists to prevent, and treating
it as bad news would refuse a fresh machine, where nothing has been observed at
all.

**An exhausted Account frees up when the last of its full windows resets**, not
the first, which would leave it blocked by the others. Where any of them does not
say, the wait is unknown and Perch will not guess at it — the wait is at least
that long, and an Account left out of that answer silently turns "the soonest
Perch can vouch for" into advice to wait longer than you have to.

## A reading is refused rather than half believed

A figure Perch cannot read is refused loudly rather than dropped quietly, and
this is the one place in Perch that prefers the loud failure. A window dropped in
silence becomes a window nothing would ever rank on: where the five-hour window
stops being a number — flattened, emptied, or quoted as a string — a weekly
window at 10% is the fullest Perch can see, so an Account whose five-hour window
is 98% full reports 90% Headroom, ranks top of its Scope, and is switched onto
dead on arrival.

So the reply is read as loosely as it can be about *which* windows there are, and
not loosely at all about whether one answers.

- **A window is whatever says how full it is.** The windows are whatever the
  reply holds rather than a fixed list, because an Account is limited by
  whichever window fills first, and a window dropped for carrying a name Perch
  had not been taught is one nothing would ever rank on.
- **A key that names a period and will not say how full it is is drift, and the
  reply is refused.** A key that names no period is a field beside the windows,
  which Perch is not entitled to an opinion about — read as windows, those fail
  the whole Refresh for every Account in one pass, in a message calling a thing
  that is not a window a Quota Window. The line is drawn on the *unit* rather
  than the count, or the rule holds for the periods in use and for nothing else.
- **The two windows every Account has are held to that standard when they are
  absent as well.** Drift can only refuse a window that is there and has stopped
  answering, so a `five_hour` the reply leaves out is the same loss arriving by
  omission. A per-model window the Account does not have arrives as `null` and is
  passed over: a window that is not there meters nothing, so it can never be the
  fullest one.
- **The paid-credit allowance is never a window.** Extra usage is what an Account
  draws on *after* its Quota Windows are full, so reading it as a window reads an
  allowance beyond the plan as a constraint on it — an allowance nine tenths
  spent would rank as an Account with a tenth of its room left, and drop down the
  Scope it should be leading.
- **A window cannot be less than empty or more than full.** A figure outside that
  is brought back into it rather than printed as "105% Headroom" in a sentence
  somebody is asked to act on.

## A Scope has no single figure

One Account is measured by its most constrained window. A Scope has no equivalent
single figure and never will: its Accounts sit on different plans, Perch only
ever sees percentages, and a `pro` Account at 50% and a `max` Account at 50% do
not have the same quota left. Summing or averaging them produces a number that
looks quantitative, is not, and is exactly the kind of number people plan around.

So the **Reserve** is how many of a Scope's Accounts still have Headroom and how
much the best of them has — a count and one Account's own figure, every part of
it something an Account reported rather than something Perch worked out. Where
nothing is left, what is in the way is the answer: "none" without a reason is a
Scope somebody stares at wondering which. Where a Reserve is said is
ADR the-listing-owns-the-set's.

## Consequences

Cycling skips Accounts that are exhausted, Disabled or Quarantined. When every
Account in the Scope is exhausted, Perch picks nothing and says so, naming which
Account resets soonest, rather than switching somewhere useless.

The Accounts a Cycle may choose and the Accounts a Scope has left to draw on are
one set, counted once each. An Account that is both Disabled and Quarantined is
still one Account, and a tally that puts it in both buckets can add up to more
Accounts than the Scope holds — a reason that does not survive being checked
teaches the reader to stop checking.
