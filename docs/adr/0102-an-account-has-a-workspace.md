# An Account has a Workspace

An Account is a selectable provider identity, optionally in a Workspace. One
OpenAI login used for personal access and for company access supplies two
entries, and they are not two logins. Email alone distinguishes neither those
two nor the same person's Claude and Codex Accounts, so it cannot be what an
Account is.

An identity is therefore the provider, the authenticated user, and the Workspace
where there is one. Email, plan and Workspace display name describe an Account
and decide nothing: they change without making another Account, and a change to
one moves no Profile. Adding the same user and Workspace again finds the Account
already held rather than making a second that would rank separately and read as
twice the quota.

Each Provider supplies those fields from its own login. Claude Code's are the
account UUID and the optional organization UUID, and a login carrying no account
UUID is refused rather than identified by its address. Codex's are the
`chatgpt_user_id` and `chatgpt_account_id` claims of the id token its login
writes, with the Workspace checked against the Credential's own `account_id` and
refused where the two disagree; the email and plan come from the same claims as
description. A Codex Workspace is a UUID with no display name, so a Codex
Account carries no organization name at all.

**Whether an absent Workspace means anything is the Provider's to say.** Codex
requires one and refuses an identity without it, wherever that identity arrived
from — a login, a configuration file, an Export. The shared contract permits an
absent Workspace and invents one for nobody. An absent Workspace differs from
every named one, and an empty identifier is invalid rather than another spelling
of absence.

## Names remain unambiguous

Aliases and Group names hold one namespace across Perch. `codex-personal` and
`codex-work` name two Workspace entries of one OpenAI login, and each entry
belongs to whichever Group its owner puts it in.

A selected provider narrows the matches for an email. Where several entries
still match, the command refuses and names their Aliases as the way through; it
never takes the first. An Alias naming the other provider is a mismatch rather
than permission to override the selected tool.

## Stable storage identity

Where an Account's Profile sits derives from the provider, the authenticated
user and the optional Workspace, and from nothing else. The three are hashed
with the user identifier's length ahead of it, so no two different pairs can
flatten to one key by being re-split at a different point, and the provider's
own word stands in front of the digest so two Providers' keys are never mistaken
for each other.

Email, plan and organization name take no part in that derivation, which is what
makes a rename free: the Registry refers to an Account by its key, and an Alias
or a display name changing moves no Profile and retargets no Landing.

The layout this key is written into is new, and an older one is refused with its
files left where they are rather than migrated
(ADR a-fresh-provider-layout).

## Export and Import

The Registry version moves when its shape moves and the Export version when its
payload does. An Export carries the Workspace identity, the storage binding and
which Provider each Account belongs to, and an Import rebuilds the destination
machine's own Credential Store from them. An Export version this build does not
understand is refused, naming the version that wrote it, rather than partly
restored. These are applications of ADR the-holdings-outlive-a-perch rather than
a second compatibility policy.

## What is not chosen

**One entry per email** collapses two access contexts into one. **One entry per
login session** duplicates an Account every time somebody signs in again.
**Naming a Profile after the email or the Workspace label** lets a rename orphan
a Credential. A stable identity is what none of the three has.
