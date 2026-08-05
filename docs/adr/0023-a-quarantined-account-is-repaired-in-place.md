# A quarantined account is repaired in place, and only the account named is touched

ADR 0006 leaves an account whose credential cannot be recovered — a rotation
lost between two writes, a refresh token Anthropic has retired. Perch keeps that
account rather than dropping it: an account that vanishes reads as data loss,
while a broken one reads as something needing attention. The state is a
quarantine, and it carries the reason it was raised, because "broken" and
"broken because Anthropic would not renew it" are different pieces of news.

Only a login produces a working credential, so the repair is a login. It runs in
a config directory of its own, exactly as `perch add` does (ADR 0009), and what
it produces is moved **into the profile the account already has**. The account
keeps its alias, its group, whether cycling may choose it, and its position in
the listing. Removing and re-adding would produce something that resembles the
account and is not it, and would hand the user the job of putting back settings
they never changed.

Only that account is touched. The login cannot reach the active account's
credential, so repairing one account never costs the session being worked in —
including when the login is abandoned, which changes nothing at all. A login
that authenticates somebody else is refused rather than accepted in place: the
alias and the group belong to the account the user named, and handing them to
whoever happened to be signed into the browser is not a repair.

The one exception is repairing the account you are **on**. Its fresh credential
is also written to the Default Profile, without a capture. Leaving it out would
repair an account into a profile nothing reads while Claude Code went on using
the credential that stopped working, and the ordinary way to make a credential
live — `perch switch` — declines to switch to the account that is already
active. Capturing first would be worse than useless here: what is live is the
very credential the login has just replaced, so the capture would write the
broken copy over the fresh one.

## Consequences

A relogin is allowed on a healthy account and behaves identically. Nothing about
the command depends on the quarantine, and a credential somebody suspects is
going wrong should not have to break first before it can be replaced.

An operation attempted against a quarantined account exits with a code of its
own. Every other refusal in Perch is answered by trying something else or trying
again; this one is answered by logging in, and a script branching on the exit
code should be able to tell that apart without parsing prose.

Because the reason is recorded, a quarantine raised by a build that did not
record reasons still has to load. It reads as quarantined for a reason nobody
wrote down, which is the honest reading of a flag.
