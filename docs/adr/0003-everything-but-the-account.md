# Everything but the Account

This applies only to the `run` path. A Switch leaves `.claude.json` in place and
patches one block of it, so project state never moves and none of this arises.

On the `run` path each profile is a live config directory of its own, and Claude
Code splits project state across two places. Transcripts live in a directory
Perch can share by symlink, but per-project tool approvals, trust, and MCP
configuration live under the `projects` key of `.claude.json` — the same file
that holds account identity, which must stay per-profile. The file cannot be
shared or split.

So before launching a client, Perch copies a single entry — `projects[<current
working directory>]` — from the most recently used profile in the same group.
Without it, someone running a second account against a repository they are
already working in re-accepts the trust dialog and re-approves tool permissions
mid-task, which is the tool failing at the moment it is most needed.

## Consequences

Perch writes a file Claude Code owns. The write is deliberately bounded: one
key, for one directory, written only when no client is running for that profile.
It is a single-key read and a single-key write, not a merge of the whole file.

The copy is scoped to a group on its own merits, not because groups scope shared
state generally — ADR a-group-is-a-declaration explicitly abandoned that. Tool
approvals are permissions, and carrying a work account's approvals into a
personal account's session is the one crossing worth preventing. Within a group
the accounts have been declared interchangeable, so most-recently-used wins with
no conflict policy needed.

## Amended: one key becomes a named set

`projects[<cwd>]` was the only key worth carrying when this was written, and it
no longer is. The same file has since accumulated a great deal of state that
belongs to the person rather than the account — `hasCompletedOnboarding`,
`lastOnboardingVersion`, `tipsHistory`, `seenNotifications` among them — and
none of it can be linked across, because the file holding it also holds
`oauthAccount` and must stay per-profile (ADR everything-but-the-account).

So a first `perch run` against a fresh profile lands the user in a Claude Code
that believes it has never been used: onboarding, first-run tips, notifications
already dismissed once. That is the failure this ADR was written to prevent,
arriving through a different key.

The copy therefore takes a **named set** of person-keys alongside
`projects[<cwd>]`, from the most recently used profile in the same group, under
the same precondition and the same bounded read-and-write.

The set is named rather than inverted — copy everything except `oauthAccount`
would be the ADR everything-but-the-account denylist argument, and it is wrong
for this one file. `.claude.json` also holds caches keyed to the account that
filled them: `cachedUsageUtilization`, `modelAccessCache`,
`overageCreditGrantCache`, `orgModelDefaultCache`. Carrying those across
accounts would feed Perch's own utilization story figures read for somebody
else. Inside this file account-scoped state is common enough that naming what
crosses is the safer direction, which is the opposite of the direction the
directory takes.

The list is consequently a thing to maintain. It is short, it is only ever
about first-run friction, and a key that goes missing costs a dialog rather
than correctness.

Two things follow from "the most recently used profile in the same group" that
are worth writing down, because both are about *where* an account's state is
rather than about which account it may come from.

An account's state is not always in its profile. The active account works in
the default profile — that is what being active means
(ADR claude-code-chooses-the-store) — and its own profile holds only what Perch
stored there. So the candidates are the directories the accounts are actually
used in, and for the active account that is `~/.claude`. Reading its profile
instead would copy a file nobody has touched since the account was added.

An account's own state crosses to its own profile whatever group it is in,
because there is no crossing in it. `perch run` on the account you are already
on is a real command — it is how a session is kept out of the way of a later
switch — and it lands in a profile rather than in the directory that account
has been working in all day. Nothing is being carried between accounts there,
so the group has nothing to say about it. The profile being launched is never
itself a candidate: it is written by every Run, so it would outrank the
person's own directory from the first Run onwards and freeze what crosses at
whatever the first one copied.

Nothing here can refuse a Run. A client already running against the profile,
nothing to copy from, a file that is not the shape it should be, a write the
filesystem will not take: all of them leave the launch alone, because a Run
that happened is worth more than a key that crossed. The write that fails is
the one worth remarking on — meeting the onboarding questions on every Run with
nothing to explain it is worse than meeting them once — so that one is said,
and the client is launched regardless.
