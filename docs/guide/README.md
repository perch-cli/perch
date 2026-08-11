# Perch guide

The [front page](https://perch-cli.github.io/perch/), or the
[README](https://github.com/perch-cli/perch/blob/main/README.md) it says the same
thing as, is enough to install Perch and get moving between Accounts. These pages
are the whole of what each command does, and why it does it that way.

| Guide | What it covers |
| ----- | -------------- |
| [Installing](installing.md) | Homebrew, the installers, npm, by hand, verifying a download, building from source |
| [Accounts](accounts.md) | `add`, `alias`, `disable`, `enable`, `relogin`, `remove` — and what a Quarantine is |
| [Seeing what you have](status.md) | `status`, `list`, reading current Utilization, the JSON |
| [Switching, Cycling and Groups](switching.md) | `switch`, Cycling within a Group, `group` |
| [Watching](watching.md) | `perch watch`, and `--once` under cron or a systemd timer |
| [Running one Account in one terminal](running.md) | `run`, Live Profiles, passing arguments through |
| [Backing up and moving machines](backup.md) | `export`, `import`, `purge` |
| [Choosing by eye](tui.md) | `perch tui` — both tabs, every key |
| [Configuration](configuration.md) | `config`, the two layers, every Setting |
| [Reference](reference.md) | every command, every exit code, every path and environment variable |

For the vocabulary these pages use — Account, Profile, Credential, Group, Scope,
Quota Window, Capture, Rotation, Live Profile — see
[`CONTEXT.md`](https://github.com/perch-cli/perch/blob/main/CONTEXT.md). For the
decisions behind them, the numbered ADRs in
[`docs/adr/`](https://github.com/perch-cli/perch/tree/main/docs/adr).
