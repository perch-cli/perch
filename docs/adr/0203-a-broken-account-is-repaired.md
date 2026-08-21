# A broken Account is repaired

ADR a-switch-is-written-down-first leaves an Account whose Credential cannot be
recovered — a Rotation lost between two writes, a refresh token Anthropic has
retired. Perch keeps that Account rather than dropping it: an Account that
vanishes reads as data loss, while a broken one reads as something needing
attention. The state is a Quarantine, and it carries the reason it was raised,
because "broken" and "broken because Anthropic would not renew it" are different
pieces of news.

Only a login produces a working Credential, so the repair is a login. It runs in
a config directory of its own, exactly as `perch add`'s does
(ADR a-login-perch-does-not-need), and what it produces is moved **into the
Profile the Account already has**. The Account keeps its Alias, its Group,
whether Cycling may choose it, and its position in the listing. Removing and
re-adding would produce something that resembles the Account and is not it, and
would hand the person the job of putting back settings they never changed.

Only that Account is touched. The login cannot reach the active Account's
Credential, so repairing one Account never costs the session being worked in. A
login that authenticates somebody else is refused rather than accepted in place:
the Alias and the Group belong to the Account that was named, and handing them
to whoever happened to be signed into the browser is not a repair.

## Repairing the Account you are on

The one exception. Its fresh Credential is also written to the Default Profile,
without a Capture.

Leaving it out would repair an Account into a Profile nothing reads while Claude
Code went on using the Credential that stopped working, and the ordinary way to
make a Credential live — `perch switch` — declines to Switch to the Account that
is already active. Capturing first would be worse than useless: what is live is
the very Credential the login has just replaced, so the Capture would write the
broken copy over the fresh one.

## Consequences

A relogin is allowed on a healthy Account and behaves identically. Nothing about
the command depends on the Quarantine, and a Credential somebody suspects is
going wrong should not have to break first before it can be replaced.

An operation attempted against a Quarantined Account exits with a code of its
own. Every other refusal in Perch is answered by trying something else or trying
again; this one is answered by logging in, and a script branching on the exit
code should be able to tell that apart without parsing prose.
