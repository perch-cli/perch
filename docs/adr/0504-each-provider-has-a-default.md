# Each provider has a default

Accepted for the multiple-provider design. Implemented for both providers:
Codex Switches its file store, and says what it cannot see.

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

## How Codex switches

Codex's Default is `auth.json` under `CODEX_HOME` or `~/.codex`, and the file
carries the login's identity, so a Switch is Claude's first two steps and no
third: the outgoing copy is Captured into its Profile where the live one is the
same identity Renewed, then the incoming Profile's copy is written. A Landing
left in flight is settled by the identity in the live file, since a Renewal
moves the bytes but not whose they are. The active Codex Account is observed
against the Default home, the copy Codex Renews; its Profile copy is refreshed
by the next Capture.

Perch writes Codex's file store and nothing else. A Default whose
`config.toml` chooses another store is refused, naming the line that pins the
file store; a home Codex has never configured is pinned by the Switch itself.

Perch has no evidence of a Codex started outside it, and the Codex research
finds cached authentication and guarded account reloads. So the Switch does
not refuse under a running Codex, and instead says that one already open keeps
its Account until it is restarted. That is the note's whole job: the one fact
about the Switch the person cannot see and may have to act on.

## What is not chosen

Cycling each running session independently requires session-specific routing
and a proven way to change the Account a live client uses. It changes what an
isolated Run promises and is outside the selected first milestone.

Detecting a Codex started outside Perch, by process or by the files it writes,
would let the Switch refuse as Claude's does. It stays an alternative until a
pinned Codex release gives a record a Switch can corroborate; until then the
note carries what a refusal would.

## Consequences

Active Accounts and interrupted Landings must be distinguished by provider.
Changes to persistent shape require versioned migration, and existing Profile
paths are not cosmetic names (ADR the-holdings-outlive-a-perch).
