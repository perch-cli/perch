# Each provider has a default

Accepted for the multiple-provider design; implementation is pending.

Claude and Codex have independent active Accounts and Default Profiles. A Switch
changes one provider's default. An isolated Run remains pinned to the Account it
launches, preserving ADR a-run-is-one-shot.

A Group may hold Accounts from both providers. Its declaration of
interchangeability applies within each provider: Claude Accounts substitute for
Claude Accounts and Codex Accounts for Codex Accounts. This qualifies
ADR a-group-is-a-declaration; membership never authorizes changing tools.

The Watcher chooses among the relevant provider's Accounts in its Scope.
Cooldowns and failure timing must not make a Claude Switch delay Codex Cycling,
or a Codex failure prevent Claude Cycling. Group preferences are shared initially;
cooldowns, polling, and failures are independent for each provider. The shared
preferences do not make provider-specific quota windows comparable.

## Selecting the tool

`perch run` accepts mutually exclusive `--claude` and `--codex` flags. Without
either, it tries the configured provider first, then the other installed CLI
(ADR run-has-a-provider-preference). If neither exists, a coding-tool launch
refuses. Selecting a tool does not convert an Account: a
Target from the other provider is refused with instructions to select its tool.
An explicit selection never silently falls back to the other tool.

A Switch infers its provider from a named Account. If both providers are
possible, including a mixed Group or an ambiguous untargeted Switch, it requires
`--claude` or `--codex`. The Run fallback is not permission to guess which active
Account somebody intends to change.

Provider flags are accepted before or after the Target, before `--`. After the
separator, a leading flag is forwarded to the selected tool; a program name
launches that program with its remaining arguments. Perch does not interpret
provider flags after the separator.

The arbitrary-program form uses the named Account's Profile without requiring
either coding CLI. An explicit provider flag checks that the Account matches;
the Run preference does not apply. A Group remains invalid as a Run Target,
because the invocation names one Account and remains pinned to it.

## What is not chosen

Cycling each running session independently requires session-specific routing
and a proven way to change the Account a live client uses. It changes what an
isolated Run promises and is outside the selected first milestone.

Changing a default does not promise that an existing client adopts it. The Codex
research finds cached authentication and guarded account reloads; unattended
Codex Switching remains conditional on live-client and refresh-coordination
experiments. The first milestone is explicit Account selection, isolated Run,
and Utilization display.

## Consequences

Active Accounts and interrupted Landings must be distinguished by provider.
Changes to persistent shape require versioned migration, and existing Profile
paths are not cosmetic names (ADR the-holdings-outlive-a-perch).
