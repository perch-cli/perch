# Perch manages the whole account lifecycle, and is written in Rust

The core operation is small: write a credential, patch an identity. A tool that
stopped there would still leave the user answering the hard questions by hand —
when to switch, which account has room left, what to do when a refresh token
dies, how to get accounts onto a second machine.

Perch commits to the whole lifecycle instead: adding and removing accounts,
aliases and groups, utilization tracking, unattended switching, quarantine and
recovery, and encrypted export and import. These are treated as table stakes,
not stretch goals, because each one exists to answer a question the switch
itself creates.

Rust, because the hot path is `perch status` — a command people put in a shell
prompt or an editor status line, where it runs several times a minute (ADR
0015) — and because distribution then needs no runtime on the target machine.

## Considered Options

Shipping only the switch, and leaving quota and recovery to the user, was
considered and rejected. It is a smaller tool that pushes every difficult
question back onto the person least equipped to answer it: someone mid-task who
has just run out of quota.

## Consequences

This is a large surface for a tool whose core operation is two writes, and the
scope has to be sequenced rather than built at once. Deciding what ships first
is the first thing any specification has to resolve.
