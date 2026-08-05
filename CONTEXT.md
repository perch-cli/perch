# Perch

Perch runs Claude Code as whichever Claude account you want, without going
through the login flow again. This context covers what an account is, how Perch
holds one locally, and what it means to make one active.

## Accounts

**Account**:
A single login you hold with Anthropic. Perch does not create or authenticate
these — it only chooses between ones you have already logged into.
_Avoid_: user, login, session

**Credential**:
The OAuth secret that proves the caller is a given Account. Held in a Credential
Store, never by Perch, and never rendered or logged.
_Avoid_: token, key, secret

**Identity**:
The non-secret description of an Account — its email address, organization, and
plan. What Claude Code displays to say who you are.
_Avoid_: profile, metadata, account info

**Alias**:
A short name you give an Account so you never have to type its email address.
Aliases and Group names share one namespace, so neither can shadow the other.
An Account answers to one Alias at a time.
_Avoid_: nickname, label, tag

**Target**:
What a command is told to act on — an Alias, an Account's email address, or a
Group name, resolved in that order. Every command that acts on one thing takes
exactly one, and because the shared namespace makes a collision impossible it
has exactly one meaning. Which kind matched is said when the command acts.
_Avoid_: selector, subject, handle

## Profiles

**Profile**:
Perch's local handle on one Account: a directory Claude Code would treat as its
whole configuration. Because a Credential Store is derived from the directory, a
Profile is what lets a stored Credential live where Claude Code would put it
rather than in a file Perch invented.
_Avoid_: slot, vault entry, workspace

**Credential Store**:
Where the installed Claude Code keeps one Profile's Credential — the operating
system's keychain, or a file inside the Profile. Which one is the platform's
answer rather than Perch's, and a Profile's Credential is held in exactly one at
a time.
_Avoid_: keychain, vault, backend

**Default Profile**:
The Profile Claude Code falls back to when it is told nothing. The Account whose
Credential is written here is the active one, for every client.
_Avoid_: global, main, root

**Group**:
A set of Accounts you have declared interchangeable, such as several
subscriptions belonging to the same person. Perch only ever Cycles within a
Group, and a Group carries the settings that govern when it may do so unasked.
_Avoid_: flock, team, org, pool

## What belongs to whom

**Shared State**:
The configuration that belongs to the person rather than the Account — memory,
settings, plugins, and past work. A Switch leaves it untouched, so it follows
you across Accounts without effort; only the Run path has to work to reach it.
_Avoid_: common config, global config

**Reconcile**:
The pass Perch makes before a Run, ensuring every piece of Shared State is
reachable from the Profile it is launching — establishing the links, repairing
broken ones, and falling back to copies on platforms that cannot link. A Switch
needs no such pass.
_Avoid_: sync, merge, heal

## Quota

**Quota Window**:
A rolling period Anthropic meters an Account's usage over. An Account has
several at once — a five-hour and a seven-day window, and a weekly window per
model — and is limited by whichever fills first.
_Avoid_: limit, budget, allowance

**Utilization**:
How full a Quota Window currently is, and when it next resets. The evidence a
person weighs when choosing an Account, and what Perch ranks on when choosing
for them.
_Avoid_: bucket, usage, quota level

**Rotation**:
Anthropic replacing an Account's refresh token with a new one, retiring the old
one. Reserved for this sense only: moving from one Account to the next is a
Cycle, never a rotation, and buying a fresh access token is a Renewal, which
only sometimes Rotates anything.
_Avoid_: refresh, renewal

**Renewal**:
Exchanging an Account's refresh token for a working access token, so Perch can
ask Anthropic a question as that Account. Only permitted where no client is
running against the Profile, because a Renewal may Rotate.
_Avoid_: refresh, token refresh

**Refresh**:
Reading an Account's Utilization from Anthropic instead of from cache — what
`perch status --refresh` does, and the only thing that spends network budget.
Said of a figure and never of a token: a token is Renewed, and what Anthropic
does to it is a Rotation.
_Avoid_: fetch, update, poll

**Quarantine**:
The state of an Account whose Credential is no longer usable and cannot be
recovered from anything Perch holds. It stays listed and named as broken until
you log into it again, and carries the reason it broke.
_Avoid_: dead, expired, invalid

**Repair**:
Logging a Quarantined Account in again in place, so it keeps its Alias, its
Group, whether Cycling may choose it and its position rather than being rebuilt.
What `perch relogin` does, and the only thing that ends a Quarantine.
_Avoid_: fix, restore, re-add, reauth

## Making an account active

**Switch**:
Making an Account active everywhere, by capturing the outgoing Credential,
writing the incoming one to the Default Profile, and patching the Identity to
match. Every client picks it up, so only one Account is ever switched to.
_Avoid_: swap, projection, activate

**Capture**:
Copying the live Credential back into the Profile of the Account it belongs to,
so a Rotation that happened while it was active is not lost when you Switch away.
_Avoid_: backup, save, snapshot

**Cycle**:
Choosing which Account to Switch to rather than being told — by Utilization,
within a Group. What Perch does when you name no target, and what the watcher
does on your behalf.
_Avoid_: rotate, next, advance

**Disabled**:
An Account you have taken out of Cycling and kept in every other way. It stays
listed, keeps its Alias and its Group and its Credential, and a Switch that
names it still lands — it is only never chosen for you. Reversible, and never a
statement about whether the Account works, which is Quarantine.
_Avoid_: excluded, paused, off, archived

**Strategy**:
Which Account a Cycle prefers when more than one would serve — the one with the
most headroom, or the one whose Quota Window resets soonest so perishable quota
is not wasted. Each Group carries its own.
_Avoid_: policy, mode, algorithm, preference

**Watcher**:
The foreground process that Cycles on your behalf when the Account you are on
runs low. It acts only within a Group that has been told it may, and only above
that Group's threshold.
_Avoid_: daemon, monitor, background job

**Run**:
Launching a client against a chosen Profile without changing which Account is
active. Scoped to the one invocation, so several Accounts can be running at once
in different terminals.
_Avoid_: use, session, launch

**Live Profile**:
A Profile with a client currently running against it, evidenced by a session
marker naming a process that is still the one that wrote it. Perch treats a Live
Profile's Credential as untouchable, because something else is holding it.
_Avoid_: active, running, in-use
