# A quarantined account is repaired in place, and only the account named is touched

ADR 0006 leaves an Account whose Credential cannot be recovered — a Rotation
lost between two writes, a refresh token Anthropic has retired. Perch keeps that
Account rather than dropping it: an Account that vanishes reads as data loss,
while a broken one reads as something needing attention. The state is a
Quarantine, and it carries the reason it was raised, because "broken" and
"broken because Anthropic would not renew it" are different pieces of news.

Only a login produces a working Credential, so the repair is a login. It runs in
a config directory of its own, exactly as `perch add` does (ADR 0009), and what
it produces is moved **into the Profile the Account already has**. The Account
keeps its Alias, its Group, whether Cycling may choose it, and its position in
the listing. Removing and re-adding would produce something that resembles the
Account and is not it, and would hand the user the job of putting back settings
they never changed.

Only that Account is touched. The login cannot reach the active Account's
Credential, so repairing one Account never costs the session being worked in —
including when the login is abandoned, which changes nothing at all. A login
that authenticates somebody else is refused rather than accepted in place: the
Alias and the Group belong to the Account the user named, and handing them to
whoever happened to be signed into the browser is not a repair.

The one exception is repairing the Account you are **on**. Its fresh Credential
is also written to the Default Profile, without a Capture. Leaving it out would
repair an Account into a Profile nothing reads while Claude Code went on using
the Credential that stopped working, and the ordinary way to make a Credential
live — `perch switch` — declines to Switch to the Account that is already
active. Capturing first would be worse than useless here: what is live is the
very Credential the login has just replaced, so the Capture would write the
broken copy over the fresh one.

## Consequences

A relogin is allowed on a healthy Account and behaves identically. Nothing about
the command depends on the Quarantine, and a Credential somebody suspects is
going wrong should not have to break first before it can be replaced.

An operation attempted against a Quarantined Account exits with a code of its
own. Every other refusal in Perch is answered by trying something else or trying
again; this one is answered by logging in, and a script branching on the exit
code should be able to tell that apart without parsing prose.

Because the reason is recorded, a Quarantine raised by a build that did not
record reasons still has to load. It reads as Quarantined for a reason nobody
wrote down, which is the honest reading of a flag.
