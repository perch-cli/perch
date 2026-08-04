# Accounts are stored as profiles, and switching swaps the live credential

Claude Code derives its keychain service name from the config directory —
`Claude Code-credentials-<sha256(CLAUDE_CONFIG_DIR)[0:8]>`, or the bare
`Claude Code-credentials` when `CLAUDE_CONFIG_DIR` is unset. Every config
directory therefore has a private credential namespace.

Perch gives each account its own directory and uses that as the account's
store, so a stored credential lives in the operating system's keychain rather
than in a token file Perch invented. Making an account active is a Switch: Perch
writes that account's credential to the default config directory's store and
patches the `oauthAccount` block of `.claude.json` to match, so every client —
every terminal, the VS Code extension, the desktop app — picks it up.

Because each profile is a real config directory, Perch can also point a single
client at one directly by setting `CLAUDE_CONFIG_DIR` for that process alone.
That is the `run` path (ADR 0010), and it is what allows two accounts to work
concurrently in different terminals.

## Consequences

Switching writes a credential that a running Claude Code may hold, so it has to
cooperate with Claude Code's own OAuth refresh locks rather than inventing its
own scheme.

Patching only `oauthAccount`, rather than swapping `.claude.json` wholesale, is
deliberate: that file also holds project history, MCP configuration, and
settings, none of which belong to the account. Leaving it in place is what makes
all of that state follow the person across a switch for free.
