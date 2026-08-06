# Perch writes the current project's entry into .claude.json on the run path

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
state generally — ADR 0002 explicitly abandoned that. Tool approvals are
permissions, and carrying a work account's approvals into a personal account's
session is the one crossing worth preventing. Within a group the accounts have
been declared interchangeable, so most-recently-used wins with no conflict policy
needed.

## Amended: one key becomes a named set

`projects[<cwd>]` was the only key worth carrying when this was written, and it
no longer is. The same file has since accumulated a great deal of state that
belongs to the person rather than the account — `hasCompletedOnboarding`,
`lastOnboardingVersion`, `tipsHistory`, `seenNotifications` among them — and
none of it can be linked across, because the file holding it also holds
`oauthAccount` and must stay per-profile (ADR 0026).

So a first `perch run` against a fresh profile lands the user in a Claude Code
that believes it has never been used: onboarding, first-run tips, notifications
already dismissed once. That is the failure this ADR was written to prevent,
arriving through a different key.

The copy therefore takes a **named set** of person-keys alongside
`projects[<cwd>]`, from the most recently used profile in the same group, under
the same precondition and the same bounded read-and-write.

The set is named rather than inverted — copy everything except `oauthAccount`
would be the ADR 0026 denylist argument, and it is wrong for this one file.
`.claude.json` also holds caches keyed to the account that filled them:
`cachedUsageUtilization`, `modelAccessCache`, `overageCreditGrantCache`,
`orgModelDefaultCache`. Carrying those across accounts would feed Perch's own
utilization story figures read for somebody else. Inside this file
account-scoped state is common enough that naming what crosses is the safer
direction, which is the opposite of the direction the directory takes.

The list is consequently a thing to maintain. It is short, it is only ever
about first-run friction, and a key that goes missing costs a dialog rather
than correctness.
