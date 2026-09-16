---
title: "Configuration"
sidebar:
  order: 8
---

`perch config` reads and changes provider installation settings, application
preferences, and the policy used to choose Accounts. Commands do not prompt.

## Files

Perch keeps one `config.json` in its configuration directory. It contains
`global`, `providers`, `scope_defaults`, `groups`, `ungrouped`, and `accounts`.
Accounts carry their provider, description, Alias, Group, and enabled state.
Credentials stay in native provider stores.

Each provider has a directory under `providers/`, with its Profiles and pending
logins. Its `state.json` records its active Account, observations, Quarantines,
and Watcher pacing. Group identities keep that pacing attached to the same Group
when its name changes.

This prelaunch layout requires a fresh installation. Perch refuses old layouts
with instructions instead of migrating them. Keep any old configuration aside
until you have finished setting up the new installation. Open an older Export
with the Perch build that wrote it.

## Application preferences

```sh
perch config set --global run-provider codex
perch config set --global run-fallback installed
perch config set --global watcher-paused true
perch config get --global
```

| Key | Values | Default |
| --- | --- | --- |
| `run-provider` | `claude`, `codex` | `claude` |
| `run-fallback` | `installed`, `disabled` | `installed` |
| `watcher-paused` | `true`, `false` | `false` |

The Run preference applies when no provider flag is supplied. Installed fallback
allows another enabled CLI when the preferred CLI is unavailable. Explicit
provider selection does not fall back. Global Watcher pause blocks every
provider's unattended Switching; clearing it restores the existing permissions.

## Provider installation

```sh
perch config set --provider codex enabled true
perch config set --provider codex cli-path /opt/bin/codex
perch config get --provider codex
perch config set --provider codex cli-path auto
```

Both providers are enabled by default. `auto` clears the configured CLI path.
Perch then uses `PERCH_CLAUDE_BIN` or `PERCH_CODEX_BIN` when supplied, followed by
PATH discovery. Provider support is registered in Perch itself; adding a JSON
entry does not install a new adapter.

## Scope policy and inheritance

A Scope is a named Group or `ungrouped`, which contains Accounts in no Group.
`none` also names those Accounts. These two words and `global` are reserved,
so they cannot become Group names or Aliases.
Settings resolve in this order:

1. Compiled defaults.
2. `scope_defaults`, shared by Scopes.
3. The Scope's own overrides.
4. A provider override within that Scope.

```sh
perch config set --defaults watcher-threshold-percent 80
perch config set work watcher-threshold-percent 75
perch config set work --provider claude watcher-threshold-percent 70
perch config get --effective work --provider claude
```

`--effective` shows resolved policy and the source of each value. Set a policy
override to `inherit` to remove it and use the preceding level again:

```sh
perch config set work --provider claude watcher-threshold-percent inherit
perch config set work strategy inherit
```

| Policy | Values | Compiled default |
| --- | --- | --- |
| `strategy` | `most-headroom`, `soonest-reset` | `most-headroom` |
| `watcher-threshold-percent` | 0–100 | `80` |
| `watcher-margin-percent` | 1–100 | `10` |

`most-headroom` prefers the Account with the most capacity remaining.
`soonest-reset` prefers usable capacity that resets sooner. A mixed Group has a
separate Cycle for each provider; Claude capacity cannot substitute for Codex
capacity.

The Watcher considers leaving an Account when its fullest applicable window
reaches the threshold. A destination must fit below the threshold minus the
margin. A margin greater than the threshold admits only an empty destination.

## Explicit Watcher permission

```sh
perch config set work --provider claude watcher-may-act true
perch config set ungrouped interchangeable true
perch config set ungrouped --provider claude watcher-may-act true
```

Watcher permission belongs to a specific Scope and provider. It is off by
default and is never inherited from Scope defaults. A mixed Group must name the
provider when granting permission. Permission for Claude grants nothing to
Codex or a future provider.

Ungrouped Accounts additionally require `interchangeable=true` before Cycling
may choose between them. A Group already declares that its members are
interchangeable within each provider. Neither permission overrides global pause
or an unsupported provider capability. Codex live Switching is currently
unsupported.

## Provider policy options

Claude can prefer its Fable workload capacity:

```sh
perch config set work --provider claude option.preferred_workload fable
perch config get work --provider claude option.preferred_workload
perch config set work --provider claude option.preferred_workload inherit
```

The provider validates native options and identifies the quota windows used to
rank that workload. Unknown options and unsupported values are refused. The
`preferred-workload` boolean is also available on an ordinary Scope page for
the selected provider's default workload preference.

## Reading settings

```sh
perch config get
perch config get work
perch config get work --provider claude
perch config get work --provider claude strategy
perch config get --defaults
```

A named setting prints its value alone. Unknown settings and invalid values are
refused with exit code 14. Use explicit provider selection when inspecting a
mixed Group's policy.
