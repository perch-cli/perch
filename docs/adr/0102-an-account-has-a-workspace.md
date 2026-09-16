# An Account has a Workspace

The shared identity contract supports a stable subject with an optional Workspace.
Codex enrollment supplies both. Claude login and discovery derive the subject
from the native account UUID and optional organization UUID; email changes do not
move its Profile. A native login without an account UUID is refused. Provider
Defaults and Watcher timing are independent.

An Account in Perch is a selectable provider identity, optionally in a Workspace. One
OpenAI login can supply separate personal and company entries. These are not
two OpenAI logins, and Perch must not present them as such. Email alone cannot
distinguish these entries or the same person's Claude and Codex entries.

Codex identity uses the provider, authenticated user identity, and Workspace
identity. Email, plan, and Workspace display name are descriptive values, not
the unique key. Repeatedly adding the same provider identity and Workspace must
not create another independently ranked Account or duplicate its apparent quota.

The exact mapping from Codex's protocol fields to this identity must be verified
against the supported client. A missing user or Workspace identifier does not
mean personal access. Refuse a new ambiguous entry with instructions rather than
inventing an identity from an email or unverified token claims. Workspace-specific
login and Utilization attribution remain evidence gates before support is claimed
(ADR codex-owns-its-renewal).

## Names remain unambiguous

Aliases and Group names retain one namespace across Perch. For example,
`codex-personal` and `codex-work` can name two Workspace entries belonging to one
OpenAI login. Each entry can belong to a different Group.

A selected provider narrows matches for an email. If several Workspace entries
still match, the command refuses and names their Aliases as the disambiguation
route. It never selects the first entry. An Alias naming the other provider is
still a mismatch, not permission to override the selected tool.

## Stable storage identity

Storage identity derives from the provider, authenticated subject, and optional
Workspace. Email, plan, and organization name describe the Account and do not
participate in that derivation. An absent Workspace differs from every named
Workspace. An empty identifier is invalid, not an alternate spelling of absence.

The provider decides whether a missing Workspace is meaningful. Codex requires
one and refuses an identity without it, including one read from configuration or
an Export. The shared contract does not invent a Workspace for providers that do
not use that concept.

This prelaunch redesign permits a fresh installation and removes historical
migration behavior (ADR a-fresh-provider-layout). Unsupported layouts are refused
with their files preserved. Registry references use the stable Account key;
Alias and display changes must not move its Profile or retarget a Landing.

## Export and Import

Move the Registry version when its shape changes and the Export version when
its payload changes. Preserve Workspace identity, storage binding, and provider
distinctions in the Export; reconstruct the destination machine's appropriate
Credential Store on Import. Unsupported Export versions are refused with the
writer version and instructions, rather than partially restored. These are
applications of ADR the-holdings-outlive-a-perch, not a second compatibility policy.

## What is not chosen

One entry per email collapses different access contexts. One entry per login
session duplicates one Account whenever the user signs in again. Naming a
Profile after a mutable email or Workspace label makes a rename capable of
orphaning its Credential. Stable identity avoids all three failures.
