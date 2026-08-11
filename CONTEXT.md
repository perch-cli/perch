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

**Remove**:
Giving one Account up for good: Perch forgets it, deletes the Credential it was
holding and frees the Alias it answered to, so nothing lists it and nothing
Cycles to it. Always exactly one Account — giving up everything at once is a
Purge. Only a fresh login brings the Account back, and as a new one.
_Avoid_: delete, drop, unregister

**Purge**:
Giving the machine back every piece of state Perch holds: every Profile, every
Credential Perch holds, and Perch's own registry, gone in one act. What Perch
holds rather than what a Channel left — the binary stays, and taking that back
belongs to the Installation it came from. Takes no Target, because it is never
about one Account — that is a Remove — and offers to write an Export first,
which is the only thing that makes it survivable.
_Avoid_: uninstall, reset, wipe, remove

**Export**:
Everything Perch holds, written to one file: the whole registry — every Account,
its Alias, its Group, whether Cycling may choose it, why it is Quarantined where
it is, and what each Group carries — alongside every Credential. Takes no Target,
because a selective one is a partial restore, which is the failure it exists to
prevent. Encrypted with a passphrase that is required rather than offered, and in
a format something other than Perch maintains, so a backup meant to outlive the
machine does not depend on Perch still existing. A backup is what one is *for*,
and the word is fine for that; the thing itself is an Export.
_Avoid_: dump, snapshot, archive

**Import**:
Putting an Export back: the whole registry and every Credential, onto a machine
that holds no Account. The exact inverse of a Purge, and refuses rather than
merging — the same Account on two machines one Rotation apart has no answer to
which Credential is live. Credentials land wherever this machine's Claude Code
keeps one, whatever store the file was written from. Nothing arrives active. A
restore is what one is *for*, and the word is fine for that; the act itself is an
Import.
_Avoid_: merge, load, adopt

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
Everything in a configuration directory that belongs to the person rather than
to the Account: memory, settings, plugins, past work, plans, and whatever the
next Claude Code release adds. Said as everything-but rather than as a list,
because the list grows on Claude Code's schedule and one written down here goes
quietly out of date — what it is not is the Credential, the file naming the
Account, and the directory of session markers, which belongs to the
configuration directory itself. A Switch leaves it untouched, so it follows you
across Accounts without effort; only the Run path has to work to reach it.
_Avoid_: common config, global config

**Reconcile**:
The pass Perch makes before a Run, making every piece of Shared State reachable
from the Profile it is launching by linking it there, and repairing links that
have broken or gone stale. What crosses is everything the Default Profile holds
except the Credential, the file naming the Account, the directory of session
markers and the refresh lock, read at Run time rather than from a list, so an entry Perch has never
heard of still follows you. Never by copying: a copy diverges the moment it is
edited, which is the opposite of what Shared State promises — so where no link
can be made the Run is refused rather than served one. A Switch needs no such
pass.
_Avoid_: sync, merge, heal

**Carry**:
The pass Perch makes before a Run over the one file Reconcile cannot link —
`.claude.json`, which holds the Account as well as the person — copying the
named keys that belong to the person into the Profile it is launching: onboarding,
tips, notifications, and the current directory's entry of `projects`. Named
rather than everything-but, which is the opposite direction from Reconcile and
deliberate: the same file holds figures read for one Account, and those must
never appear under another's name. Bounded to those keys, from the most recently
used Profile in the same Group, and only where nothing is running against the
Profile. Nothing here is load-bearing — a key that does not cross costs a dialog.
_Avoid_: copy, merge, seed, sync

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

**Headroom**:
How much of an Account is left to spend, taken from its most constrained Quota
Window — so an Account is only ever as free as its fullest window, and a
generous-looking figure never hides an exhausted one. What Accounts are
compared on, whoever is doing the choosing.
_Avoid_: capacity, room, slack, remaining

**Reserve**:
What a Group has left to draw on, said as how many of its Accounts still have
Headroom and how much the best of them has. Never one pooled figure: Accounts
sit on different plans and Perch only ever sees percentages, so quota does not
add up across Accounts and a number that implied it would be a lie.
_Avoid_: total, pool, aggregate, group quota

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
that Group's threshold. Either a loop you can see and kill, or a sequence of
Checks something else runs for you — never a service running in the background.
_Avoid_: daemon, monitor, background job

**Check**:
One round of the Watcher taken on its own — `perch watch --once` — for a
scheduler to run. The loop's policy exactly, run once, saying what it decided in
its exit code rather than in a line somebody is watching. What paces it is
recorded, because each Check is a fresh process and the sequence of them is the
Watcher.
_Avoid_: poll, tick, cron job

**Threshold**:
How full the Account you are on has to be before the Watcher wants to move off
it. A Group's, as a percentage of the fullest Quota Window.
_Avoid_: limit, cap, trigger

**Margin**:
How far under the Threshold a candidate has to sit before moving to it is worth
doing. What stops two Accounts either side of the Threshold from trading places
every few minutes.
_Avoid_: buffer, hysteresis, gap, slack

**Cooldown**:
The least wall-clock the Watcher leaves between one Switch and the next,
whatever the figures do in between. It also names how long the Account a Switch
just left stays off the candidate list. It belongs to the Watcher rather than to
the machine: the loop carries it in memory and forgets it when it stops, and a
Check records it against its Group, because there is no loop for it to live in
and the Check after it would otherwise be paced by nothing.
_Avoid_: backoff, debounce, rate limit, throttle

**Back-off**:
How much longer the Watcher leaves it before asking again after a Refresh it
could not read — doubling with each failure, bounded, and dropped entirely by
the first Refresh that works. Not a Cooldown: a Cooldown paces Switches the
Watcher may make and is the Group's to set, and a Back-off paces questions
nobody is answering and is arithmetic about Anthropic's allowance.
_Avoid_: cooldown, retry delay, throttle

**Run**:
Launching a program against a chosen Profile without changing which Account is
active. Scoped to the one invocation, so several Accounts can be running at once
in different terminals. Claude Code unless something else is named after `--`,
which is mandatory before anything is: a flag typed without it could belong to
either program, and guessing is the one thing a Run will not do. A Run makes the
Profile it launches a Live Profile for as long as it lasts.
_Avoid_: use, session, launch

**Live Profile**:
A Profile with a client currently running against it, evidenced by a session
marker naming a process that is still the one that wrote it. A Run makes one and
writes that evidence itself. Writing into a Live Profile is refused — a Capture,
a Renewal, a `.claude.json` key — because something else is holding those files;
reading out of one is not, so a Switch onto the Account still lands and its
Utilization is still readable. A marker whose process is gone, or whose pid has
been taken by something younger, makes nothing Live.
_Avoid_: active, running, in-use

## Getting Perch onto a machine

**Release**:
One version of Perch, published: the binaries for every Target, the checksums
that identify them, and the notes saying what changed. Made from a tag, which
is made from a version nobody typed — the commits since the last Release decide
it. A Release is the thing every Channel points at rather than a thing each one
holds a copy of, so what Homebrew installs and what a browser downloads are the
same bytes.
_Avoid_: version, build, drop, publish

**Target**:
One machine shape Perch is built for — an architecture and an operating system,
named as the Rust toolchain names it. Five of them, each built on a machine of
its own architecture. Not a platform: macOS is a platform and has two Targets.
_Avoid_: platform, arch, triple, variant

**Artifact**:
One file belonging to a Release: an archive holding a binary and both licences,
or the checksums, or the signed provenance for either. What an Artifact claims
about itself is checkable — the checksums say which bytes, and the provenance
says which workflow, in which repository, at which commit, produced them.
_Avoid_: asset, file, package, download

**Channel**:
One route by which somebody installs Perch — Homebrew, npm, an installer
script, or the Release page itself. Each has its own idea of what "the default"
means, and only some of them can be asked to withhold it: Homebrew's is a Tap
nobody adds by accident, npm's cannot be withheld at all (ADR 0031). A Channel
distributes a Release; it never builds one.
_Avoid_: registry, source, distribution, repo

**Tap**:
The repository Homebrew installs Perch from, and Perch's Channel for anyone on
Homebrew. Separate from Perch's own repository because Homebrew requires it to
be, and named `homebrew-perch` because Homebrew requires that too — what a
person types is neither. Adding one is a deliberate act, which is what makes it
the right place for a Perch that is not finished.
_Avoid_: formula, bottle, brew repo

**Installation**:
What a Channel left on this machine: the binary, and on Windows the PATH entry
that makes it runnable. The counterpart to what Perch *holds* — an Installation
outlives a Purge, and taking one back belongs to the Channel that made it
rather than to any Perch command (ADR 0033).
_Avoid_: install, setup, deployment, footprint
