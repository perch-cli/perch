# A choice reads what it ranks

`perch switch` with no Target Cycles: it ranks the Accounts in a Scope and lands
on the best. Until now it ranked on whatever was cached, however old, because
ADR a-figure-carries-its-age extended the no-network rule from displaying a
figure to deciding on one.

That extension was wrong, and the way it reads on a terminal says why:

```
mads@example.com is already the best Account in Group `default`, with 100%
headroom, which is true of every one of its Quota Windows — 5-hour is its
fullest, as of 1m ago.
```

The rival it beat had not been read for twenty-one hours. **A displayed figure
carries its age and a ranked one does not**, so the age that is the whole of
Perch's promise about a number is the one thing a comparison throws away. The
sentence claims a comparison Perch is not in a position to make.

ADR a-watcher-knob-is-arithmetic already settled the principle, for the Watcher:
*"Candidates are read at the moment a decision is taken instead, which is also
the moment they are cheapest."* A bare `perch switch` is a moment a decision is
taken. The Watcher reads its candidates and the command somebody types does not,
which is one rule applied on one of the two paths that needs it.

**So a Cycle reads the Accounts it cannot rank without, and no others.**

## A stale figure is a Best Case

Reading every candidate on every `perch switch` is the cost that ADR
a-watcher-knob-is-arithmetic refused for the Watcher's loop: at 28-30 reads an
hour per Account, polling a Scope makes the size of a Group a scaling limit. A
command run from a shell alias would spend a Group's whole allowance in an
afternoon.

It is not necessary, because arithmetic answers most of it. Usage climbs within
a Quota Window and falls only when the window comes back, so **a window whose
reset is still ahead caps an Account at the Headroom it was last seen with.** One
past its reset, or silent about when it comes back, caps nothing. An Account's
Best Case is the worst of its windows, exactly as its Headroom is
(ADR headroom-is-the-worst-window).

A candidate whose Best Case loses to a figure Perch can vouch for cannot win, so
reading it would spend an allowance to confirm a refusal. The transcript above is
the ordinary case: the rival's 5-hour window was observed 21h ago and has
certainly come back, so it caps nothing, but its 7-day window was 9% used with a
reset still ahead and caps the Account at 91%. 91 loses to a trusted 100.
**Nothing is read, and the sentence stops being a guess.**

## The Account being left is read first

The Best Case of a rival is worth nothing without something to measure it
against, and the only figure that can serve is the Account being left. So the
reads happen in two passes: the Account being left, where what is cached is too
old to stand for it, and then the rivals its fresh figure does not already rule
out.

A figure is old enough to be only a Best Case at the Watcher's Refresh interval,
which is 150 seconds. Taken rather than restated: that number is already Perch's
answer to how fresh a figure has to be before it is acted on, and it is not a
sixth knob — the Threshold and the Margin remain the two anyone sets
(ADR a-watcher-knob-is-arithmetic).

The same interval decides a rival. A figure inside it stands for the Account
being left and would not stand for a rival is a rule with two clocks in it.

**Under `soonest-reset` a Best Case proves nothing.** That Strategy orders on a
reset time rather than on room, so a bound on room excludes nobody and every
stale candidate is read. The alternative is a second bound argued over reset
times, which would have to reason about a window that has both come back and
been spent since, and it is not worth what it buys.

## It ranks on what it has, rather than refusing

A read that is throttled, fails, or finds an Account Quarantined leaves the
cached figure standing, and the Cycle ranks on it and names the Account it could
not read.

**This is where a Cycle somebody typed parts from the Watcher**, which holds its
round rather than acting on a figure it did not just read. The Watcher acts
unasked and nobody is waiting on it, so a held round costs nothing. A person is
waiting on `perch switch`, and a refusal because Anthropic was throttling costs
them the thing they asked for while telling them nothing they could act on. It is
the bargain ADR a-figure-carries-its-age already strikes for `--refresh`, applied
to a decision rather than to a display, and the age of the figure is still said
out loud — now in a sentence naming the Account rather than in a column.

`--no-refresh` Cycles on the cache, for somebody offline or somebody who put
`perch switch` where latency matters.

## The reads happen after the Profile is asked

A Switch off a Profile a client is running against is refused whatever the
figures say (ADR a-profile-is-live-by-evidence). Asked first, then, or a Cycle
spends one hourly allowance per candidate to arrive at a refusal it could have
made for nothing.

## Consequences

**A refusal may not promise what a refresh would change.** "Nothing was changed —
`perch list default --refresh` reads current figures" is false advice after a
Cycle that just read them, so the Cycle is told whether the figures it ranked on
were current and the sentence ends there when they were.

**`perch switch` reaches the network.** It is slower, it can hang on a dead
endpoint, and a hook or shell prompt running it will feel that. No exit code
moves and no output shape changes, so nothing reading it as a script breaks;
`--no-refresh` is the way back to the old timing.

**Utilization is still displayed from cache everywhere.** `perch status`, `perch
list` and the figures a landing line prints are untouched. The rule here is about
ranking, which is the one place an age is thrown away rather than shown.

**Monotonic usage is an assumption, and it is held the safe way**
(ADR an-assumption-is-probed). If a Quota Window turns out to slide rather than
to reset, a figure bounds nothing, every Best Case collapses to 100, and a Cycle
reads every stale candidate. The mechanism gets weaker and never wrong, which is
why the bound is computed per window from `resets_at` rather than from the age of
the observation.

**The Watcher's burst is unchanged.** It still reads every candidate. The saving
would be near nothing: a Watcher only reads candidates at a crossing, where the
Account being left is at 80% or worse, so almost every Best Case clears that bar
and is read anyway. Narrowing it is a live alternative, waiting on a Scope big
enough for the arithmetic to matter.
