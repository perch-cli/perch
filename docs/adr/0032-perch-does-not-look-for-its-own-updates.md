# Perch does not look for its own updates

> **Superseded by ADR an-upgrade-asks-its-channel.** Perch now carries
> `perch upgrade`, and `perch --version` says when a newer Release exists. Most
> of the argument below survived — there is still no schedule, no cache and no
> age, and `perch status` is still silent on the network — but the title is no
> longer true, and the reopener this ADR asked for is not what happened.
> ADR an-upgrade-asks-its-channel says which parts it kept and what it gave up.
> Left here as written, because what it refused is the thing that has to go on
> being refused.

Most distributed CLIs tell you when a newer version exists. Perch does not, and
will not add it without a reason that has actually happened.

`perch --version` says what is installed. Homebrew and npm already watch for
their own users. What that leaves is people who installed through the shell
installer and have no package manager watching for them, and the argument for a
version check is entirely about them.

## Why the cost is higher here than usual

A version check is a periodic outbound request, a cache to hold the answer, an
age to reason about, and a failure mode when the request does not come back.
Perch already carries exactly that shape for Utilization — served from cache,
with the age shown, degrading rather than failing (ADR a-figure-carries-its-age)
— and it took two ADRs to get right for a figure the program is actually about.

A second one would be the same machinery for something the program is not about,
in a tool whose value proposition is that it is careful with credentials.
"Perch phones home on a schedule" is a sentence that has to be true, explained
and bounded, and it buys a notification.

There is a smaller point that decides it against the convenient version. The
obvious implementation asks GitHub on some interval and caches the answer, which
means `perch status` — cheap enough for a shell prompt, and deliberately silent
on the network (ADR a-figure-carries-its-age) — stops being either.

## What would reopen it

Somebody stranded on an old version who installed through the shell installer.
That is a real report, not a hypothetical, and the answer to it might be a
version check or might be that the installer is easy enough to re-run. Until
then this is a feature nobody has asked for, paid for in the one currency Perch
is trying not to spend.
