# A login Perch does not need

Anyone installing Perch already has an Account logged into Claude Code. Perch
copies that Credential into a Profile of its own and records the Account as
active, rather than asking for a login it does not need. That is Adoption, and
it happens on whichever command is run first.

Adoption leaves two copies of one Credential — which is exactly where every
Switch leaves things (ADR a-switch-is-written-down-first), so it starts the
system in its steady state rather than adding a case.

## A login runs in a directory of its own

Every Account after the first arrives through a login, and Perch never spends
one in the Default Profile. The login runs in a config directory Perch made for
it, at `perch_home/pending/login-<millis>`, with `CLAUDE_CONFIG_DIR` pointing
the launched client there; what it leaves behind is moved into the Profile
afterwards. A Profile is named after the Account it holds and which Account that
is only becomes knowable once the login has finished, so the directory's name
records the moment it started instead.

The active Account is never read, never written and never logged out. Gaining an
Account and repairing one (ADR a-broken-account-is-repaired) both leave the
session being worked in exactly where it was, including when the login is
abandoned, which changes nothing at all. This falls out of Profiles being real
config directories (ADR claude-code-chooses-the-store), and it is worth
protecting.

Nothing outlives the command: the directory is removed whether the login worked
or not, and every way out of the command takes it back out again.

## An abandoned login is expected, and is reaped

A login writes a complete, working Credential into the directory it runs in.
Ctrl-C delivers SIGINT to the whole foreground group, and "quit Claude Code when
the login is done" is the documented flow, so abandonment is ordinary rather
than exceptional: Perch dies without unwinding and the Credential stays. Each
one is named after a different moment, so they accumulate — live refresh tokens
for Accounts the person believes they never added, invisible on macOS because
they are keychain items.

So every command reaps them on the way to what it was actually asked for, on two
conditions. The first is age: thirty minutes, which is not generous for a login
that wants a password manager, a second factor and a browser that opened on the
wrong profile. The second is that nothing is running against the directory,
which is what age cannot promise — a login somebody is still driving in another
terminal must never be reaped out from under them.

That second condition is the same evidence every other write asks for
(ADR a-profile-is-live-by-evidence): a session marker naming a process that is
still the one that wrote it. The marker is Perch's own, because Perch is waiting
on the login exactly as a Run waits on its client. A `claude` sitting on an
OAuth prompt in a directory it has never had a session in is the least likely
thing to write a marker, so depending on it would be the wrong way round.

Reaping is silent and best-effort throughout, and a directory whose age cannot
be established is left alone: being wrong in that direction costs a stale
directory, and being wrong in the other costs somebody the login they are in the
middle of.

## What was not chosen

**Only ever snapshotting the live Credential.** A design with one place to log
in means adding an Account starts with logging out of the one being used —
losing a session to gain an Account. Perch never requires that.

## Consequences

Both commands that spend a browser login end where the login ends: they say who
logged in, and nothing about whether that was who was wanted. What to do with
the result differs — `perch add` refuses an Account Perch already holds, and
`perch relogin` refuses every Account but the one named — so the refusal is the
caller's rather than the login's.

The registry lock is never held across a login. That is minutes of somebody
else's time, and the commands that spend it take the lock afterwards, against a
registry read fresh.
