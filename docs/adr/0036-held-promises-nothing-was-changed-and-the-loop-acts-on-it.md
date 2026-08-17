# Held promises nothing was changed, and the loop acts on it

`PerchError::Busy` carries exit code 20, and the table in the guide states what
that means to whoever reads it: *"held: a lock somebody else has… Nothing is
wrong and nothing was changed — ask again shortly."*

That is not decoration. `perch watch` branches on it. Every other failure ends
the loop; a `Busy` is counted against the back-off, said out loud with when the
watcher will try again, and gone round again. The loop keeps running because the
code told it the machine is exactly as it was.

So `Busy` is a promise about the machine and not a description of the failure,
and the two come apart in one place: a hold that is lost *after* something has
been written. Another `perch` waited the artifact out and took it over while
this one was working, and this one now holds a registry behind the one on disk —
so `registry::save` refuses, correctly, because writing it back would revert
whatever the other command did. Read as a lock problem that is the most
retryable failure there is. Read as a promise it is false, because by then the
Credential has moved.

## The one that matters

`switch::Landing::record` writes which Account is active after the Switch has
landed. Its failure is the case: the incoming Credential is in the Default
Profile, Claude Code's own file names the incoming Account, and Perch's record
still names the outgoing one. Nothing about that is untouched, and nothing about
it is fixed by waiting.

`commands::watch` already says what to do with it, and says it about every
failure rather than about this one: *"the machine is part way through a Switch,
and a watcher that carried on watching would be deciding what to do next about a
machine nobody has looked at yet."* A `Busy` would carry on watching. That is
the whole of the decision.

It is worth being exact about what a continued loop would and would not cost,
because the tempting argument overstates it. It would **not** destroy a
Credential: `switch::capture` reads the Identity beside the live Credential
before it files anything, and a Credential whose Identity names somebody other
than the Account the registry calls outgoing is declined as `NotTheirs` rather
than written into the wrong Profile. That guard was put there for this exact
state — its comment ends "running the same `perch switch` again after it failed
to record itself is enough to reach it" — and the byte-for-byte check above it
is placed first for the same reason. What a continued loop would cost is
smaller and still not acceptable: every round after it reads and ranks the wrong
Account, because the registry names one Account and the machine is acting as
another, and it does so unattended and indefinitely.

## What follows

A refusal earns `Busy` when nothing has been written yet, and only then.
`lock::take` losing a contended lock qualifies: nothing has started.
`commands::still_ours` qualifies by construction — it is asked "before the first
irreversible thing", which is its whole reason for existing. A `registry::save`
that finds the hold lost qualifies where the save *is* the change, and does not
where a Credential moved first.

Where it does not qualify, the general failure code is the right answer, and the
loop stopping is the right behavior. A person has to look.

`perch watch --once` inherits this. A scheduler reading 20 comes back in five
minutes and expects the machine to have moved on; reading 1 it mails somebody.
Those are the two things a check can say, and a Switch that half happened is the
second.

## Consequences

The three sentences that describe a hold being lost are not merged into one
variant, and they should not be. `lock::take`'s contention, `still_ours`'s
question that waited too long, and `registry::save`'s refusal to revert another
command each say something different about what was and was not done, and the
exit code they earn depends on that rather than on all three being about a lock.

This is the reason a tidy-looking refactor is refused: routing all three through
`Busy` removes three hand-written sentences and, with them, the distinction the
loop is branching on. It has been proposed once on exactly that reasoning.
