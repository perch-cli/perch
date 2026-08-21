# The whole Account lifecycle

The core operation is small: write a Credential, patch an Identity. A tool that
stopped there would leave the person answering the hard questions by hand — when
to switch, which Account has room left, what to do when a refresh token dies,
how to get Accounts onto a second machine.

Perch commits to the whole lifecycle instead: adding and removing Accounts,
Aliases and Groups, Utilization tracking, unattended Switching, Quarantine and
Repair, and encrypted Export and Import. Each of these answers a question the
Switch itself creates, so each is table stakes rather than a stretch goal.

## What was not chosen

**Shipping only the Switch, and leaving quota and recovery to the person.** It
is a smaller tool that pushes every difficult question back onto the one person
least equipped to answer it: somebody mid-task who has just run out of quota.

## Consequences

This is a large surface for a tool whose core operation is two writes, and it is
sequenced rather than built at once.
