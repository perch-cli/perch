# Run has a provider preference

Implemented for Run. Codex live Switching remains a separate validation gate.

Run has one application-wide `run-provider` Setting, initially `claude`, with
`claude` and `codex` as its allowed values. It chooses the preferred coding tool
when no provider flag is supplied. It is not a Group preference or an inherited
default for Group Settings.

## Addressing the preference

The command grammar is:

```sh
perch config set --global run-provider codex
perch config get --global run-provider
```

The `--global` flag addresses application Settings explicitly, without treating
an Alias or Group as an application namespace. Existing
`perch config set <scope> <key> <value>` commands keep their meaning. Global
reads list application Settings when no key is named. An unqualified full Config
listing includes application Settings as well as each Scope's Settings.

This qualifies ADR a-setting-names-its-scope: Cycling preferences remain wholly
owned by a Scope, while a command-wide preference has an application owner.
There is no application fallback layer for Threshold, Strategy, or Watcher
permissions, and `run-provider` is not accepted on a Group or Ungrouped Scope.

## Selection order

1. An explicit `--claude` or `--codex` selects that tool. Both together are
   refused, and an unavailable explicit selection never falls back.
2. With neither flag, try the configured provider. If its CLI is absent, try the
   other provider. With no configured value, this is Claude first, then Codex.
3. If neither CLI exists, refuse the coding-tool launch with instructions.

Fallback is for an absent executable, not failed authentication, exhausted quota,
a Target from the other provider, or a child process that fails after starting.
A mismatched Target is refused with the matching provider flag suggested.
Changing the preference does not change either active Account or any running
session (ADR each-provider-has-a-default).

An arbitrary command after `--` does not use the preference: its named Account
determines the Profile, and neither coding CLI is required. A supplied provider
flag still has to match that Account. This preserves the custom-command form
in ADR a-run-is-one-shot.

## Persistence

The preference belongs to the Registry and travels in Export and Import. Registry
migration supplies `claude` when the old shape has no Run preference; no existing
Group Setting changes. Its addition moves the Registry version, and any changed
Export payload moves that format's version (ADR the-holdings-outlive-a-perch).

CLI and JSON changes carry breaking changelog entries. Registry version 7
persists the preference, and Export version 2 carries it with provider Holdings.
