# A Provider owns its tool

Each supported Provider implements one shared interface. Commands select the
Provider and ask for an operation; the adapter owns tool-specific authentication,
Credential storage, launching, observation, and Default activation. A catalog
registers adapters and is the source of supported identifiers, CLI discovery,
and capabilities. A new provider must not require branches in existing commands.

A capability distinguishes an unavailable operation from a failed operation.
A provider can expose isolated Runs while declining live Switching. The common
Watcher and Cycle logic operate on one Provider and never compare capacity from
different tools. Each Provider keeps independent active and pacing state.

Provider implementations live under `src/providers/`. Claude Code is the tool;
Anthropic is its service client, private to the Claude integration. Shared
Account selection, Groups, configuration, and output remain outside adapters.
