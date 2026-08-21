# Everything but the Account

A configuration directory holds two kinds of thing. The Account is
`.credentials.json` and the `oauthAccount` block of `.claude.json`. Everything
else in it is the person: memory, settings, plugins, past work, plans, and
whatever the next Claude Code release adds.

**`oauthAccount` is the whole of what a Switch changes.** Perch writes the
Account's Credential into the default configuration directory's Store and patches
that one block to match, leaving the rest of `.claude.json` exactly where it is —
so every client picks the new Account up, and the project history, MCP
configuration and settings the file also holds follow the person across a Switch
for free. Swapping the file wholesale would move all of that too, and none of it
belongs to the Account.

That is why a Switch needs no pass over Shared State and the Run path does. A Run
launches a client against a Profile that is a live configuration directory of its
own rather than storage (ADR a-run-is-one-shot), so everything the person owns
has to be made reachable from it.

## What crosses is said as everything-but

Perch shares everything the default configuration directory holds except four
entries, held back for two different reasons.

`.credentials.json` and `.claude.json` are the Account rather than the person,
and keeping them apart is what a Profile is for.

`sessions` and `.oauth_refresh.lock` are neither. Each answers a question about
*this* configuration directory, and an answer about one directory means nothing in
another. `sessions` records which clients are running here: shared, one client's
Marker would be every Profile's answer, so a single `claude` in another terminal
would make every Account on the machine Live and every Switch refuse, and a Run's
own Marker would land in the default directory rather than in the Profile it is
about. The refresh lock is the same mistake with a worse ending — two
configuration directories contending for one lock, each reading the other's
modification time as its own, with no way out.

Nothing is lost by holding `sessions` back. It carries live-client Markers; the
transcripts and plans a person would miss are `projects`, `history.jsonl` and
`plans`, and all of those cross.

**The denylist is a rule about names, so what is written beside a held-back name
is held back too.** Every atomic write puts a temporary file holding the *whole*
of its target beside it, which is a complete copy of `.credentials.json` for as
long as a Switch is mid-write; a lock takeover puts a marker beside the refresh
lock; Claude Code's own config-file lock sits beside `.claude.json`. Matched by
prefix and without regard to case, because Windows answers to `.Credentials.json`
for the same file. The two errors do not cost the same: an entry wrongly held back
is one piece of Shared State somebody notices missing, and an entry wrongly
crossed is a Credential in another Account's Profile. So a name that merely begins
the same way and belongs to the person — a `sessions.md` — stays behind as well.

## An allowlist goes stale on somebody else's schedule

The alternative is naming what is shared. It is more precise, and it decays on
Claude Code's release schedule rather than Perch's: the four pieces of Shared
State this project first wrote down sat beside twenty-one entries in the installed
configuration directory, among them `plans`, `tasks`, `file-history` and
`session-env`, none of which existed when the sentence was written. Under an
allowlist a person's plans silently fail to follow them into a Run, which reads as
Perch losing their work.

The denylist fails the other way: a genuinely new Account-scoped file would be
shared until somebody noticed. That is rarer — it needs Anthropic to invent one —
and it is the kind of assumption Perch probes rather than believes
(ADR an-assumption-is-probed). So the set is read off the directory at Run time
rather than out of Perch's source, and a release that adds an entry needs no
change here.

## By link, always

A copy is not a degraded share, it is a different feature: edit `CLAUDE.md`
inside a Run and the real one silently does not have it, which inverts the one
thing Shared State promises. Where no link can be made the Run is refused, naming
the entry, what stopped it and what to do about it. A refusal is recoverable. A
silently diverged copy of somebody's memory is not.

Windows needs no fallback. Directory **junctions** and file **hardlinks** both
work without Developer Mode and without elevation; only symbolic links require
either, so a file there tries a symbolic link first — where the privilege is
there it is the better share, because it survives the file it names being
replaced — and falls back to a hard link. The fallback is Windows' alone, because
the repair that keeps a hard link honest is Windows' alone.

**A share is re-established before every Run, and a hard link is why.** A hard
link is a second name for a file and says nothing about being one, so it cannot
be recognized — and it goes stale silently: an editor that writes beside a file
and renames it into place leaves the default directory holding a new file and the
Profile still naming the old one, which is the divergence this whole rule exists
to prevent. Re-establishing before every Run keeps that window down to one Run.

The consequence is that Windows deletes a file it cannot identify where every
other platform refuses to, and that asymmetry cannot be closed. The guard a reader
reaches for — a hard link shares its target's modification time, so a file whose
time is anything else was not made here — refuses exactly the stale link it was
added to catch. What would work is Perch recording which entries it linked, which
is a file of its own with its own ways to be wrong: a record lost refuses shares
Perch did make, and a record kept past a removal deletes a file somebody put
there afterwards. It has not been judged worth that.

**A link into the default directory that no longer stands for anything is taken
away.** Claude Code takes its locks by `mkdir`
(ADR claude-code-chooses-the-store), and `mkdir` at a dangling link fails exactly
as it does when the lock is held — so a link left behind is a Profile whose client
waits for a lock nobody holds. Only links, only ones pointing into the default
directory, and only ones pointing at something gone: everything else in a Profile
is somebody's, and this is a repair rather than a tidy-up. A link sitting at a
held-back name goes whether or not its target is still there, because the denylist
is enforced where links are made and nothing would ever look at that one again.

**An entry that contains the Profile is never crossed.** A `CLAUDE_CONFIG_DIR`
and a `PERCH_HOME` can be arranged so that a crossing entry holds the Profile
several levels inside it, and linking it in makes a subtree that contains the link
that made it — after which anything walking that Profile, Claude Code's plugin and
memory discovery among it, descends for ever. Containment is asked of the path and
of what one hop of link resolution reaches, because one hop is what a dotfile
manager makes.

## `.claude.json` is handled key by key, by a named set

The one file that cannot be linked is the one holding both kinds of thing. So the
keys that belong to the person are copied into the Profile a Run is launching,
each as a bounded read and a bounded write: the value is found as a span of text
and spliced in, so nothing outside the named keys is reformatted, reordered or
read at all, and the file is never parsed and never written back wholesale.

**What crosses is named rather than inverted, which is the opposite of the
direction the directory takes.** Inside this one file, Account-scoped state is
common enough that naming what crosses is the safer direction: `oauthAccount` is
the Account, and `cachedUsageUtilization`, `modelAccessCache`,
`overageCreditGrantCache` and `orgModelDefaultCache` are answers Anthropic gave
about one subscription. Carrying those across would put one Account's figures in
front of another Account's name (ADR a-figure-names-its-account).

What qualifies is state Claude Code accumulates about the person, whose absence
costs a dialog rather than costing correctness: whether onboarding is done, which
tips have been seen, which notifications have been dismissed, and the entry of
`projects` for the directory the Run was typed in — that directory's trust, tool
approvals and MCP configuration. Without it, somebody running a second Account
against a repository they are already working in re-accepts the trust dialog and
re-approves tool permissions mid-task, which is the tool failing at the moment it
is most needed. Only that directory: the rest of `projects` is every other
repository the person has ever opened, and an Account does not need the tool
approvals of work it is not doing.

The list is a thing to maintain, and short on purpose. A key Claude Code invents
and this never learns about is one dialog, once.

## Where the person's state is read from

**From the most recently used Profile in the same Group.** Tool approvals are
permissions, and carrying a work Account's approvals into a personal Account's
session is the one crossing worth preventing; within a Group the Accounts have
been declared interchangeable (ADR a-group-is-a-declaration), so the freshest
answer wins with no conflict policy needed. An Account's own earlier state crosses
to its own Profile whatever Group it is in, because there is no crossing in it —
`perch run` on the Account somebody is already on is how a session is kept out of
the way of a later Switch.

An Account's state is not always in its Profile. The active Account works in the
default configuration directory — that is what being active means
(ADR claude-code-chooses-the-store) — and its own Profile holds only what Perch
stored there, so the candidates are the directories the Accounts are actually used
in. The Profile being launched is never itself a candidate: it is written by every
Run, so it would outrank the person's own directory from the first Run onwards and
freeze what crosses at whatever the first Run copied.

The write happens only where nothing is running against the Profile, discounting
the Run that is starting. A client holds this file open and rewrites it wholesale
on its way out, so a Profile with one running is a Profile Perch has nothing
useful to say about — while the Run's own claim on the Profile is what stops
another `perch` deleting the directory out from under it
(ADR a-profile-is-live-by-evidence), and read as an ordinary client it would
decline every Carry there is.

## Nothing here can refuse a Run

Nothing to copy from, a client already running, a file that is not the shape it
should be, a source that stops mid-token so the span spliced in would not be
JSON, a write the filesystem will not take: all of them leave the launch alone. A
Run that happened is worth more than a key that crossed, and a person who answers
one onboarding question has lost less than one who was refused a client.

The write that fails is the one worth remarking on — meeting the onboarding
questions on every Run with nothing to explain it is worse than meeting them once
— so that one is said, naming the file, and the client is launched regardless.

This is the one asymmetry in the rule: **the linking pass refuses and the copying
pass does not.** Shared State that cannot be reached is the person's memory
missing; a key that did not cross is a dialog. The Run path is therefore the only
one that can fail for a filesystem reason, and that failure names the entry and
the reason, because the fix — enabling Developer Mode, moving the Profile off a
filesystem that carries no links, moving aside whatever is in the way — is not
one Perch can apply for them.
