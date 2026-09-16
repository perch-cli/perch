---
title: "When something goes wrong"
sidebar:
  order: 9
---

`perch probe` gathers everything Perch can see of this machine. `perch triage`
hands that to Claude Code, or to Codex when `run-provider` is `codex`, and
lets it investigate and write the report. Neither changes anything.

## What Perch can see of this machine

```
$ perch probe
Findings
  <account 2> is Quarantined, so Cycling will not choose it and a Switch to it refuses. `perch relogin <account 2>` is the way back. (exit 19)

Perch         0.3.8 (linux x86_64)
Binary        /somewhere/nobody/installs/perch
Claude Code   2.1.221, at /usr/bin/claude
Codex         0.50.0, at /usr/bin/codex
Home          <home>/.config/perch, Registry version 9
Active        claude: <account 1>; codex: nobody
Holdings      2 Accounts in 1 Group, 1 Quarantined, 0 Disabled
Watcher       no Service installed
Trail         4 lines, last written 2026-08-04 11:59:00Z

Assumptions
  held     claude: Claude Code is installed and reports a version
  held     claude: a Credential is kept in the keychain namespace, or the file, that the config directory derives
  held     claude: the keychain item is stored under the login name
  held     claude: the credential store holds a claudeAiOauth block
  held     claude: the identity file holds an oauthAccount block
  unread   claude: a session marker names its process and when the session started

Trail
  11:20:00  switch overflow  exit 19
  11:59:00  list  exit 0
```

The findings come first and the facts under them. Each finding carries the
exit code of the refusal it would cause. `Active` names the active Account for
each provider, or the Landing a killed Switch left. Each provider's row says
which CLI was found and where; Claude Code's assumptions are checked, Codex
reports its installation only.

Email addresses, Alias and Group names and your home directory come out as
placeholders, ready to paste. `<account 2>` is the same Account every time you
run it. `perch probe --raw` prints the names as they are. `--json` prints the
same as a document: each CLI's details and assumptions under `providers`,
keyed `claude` and `codex`, and `holdings.active` keyed the same way. A finding
about one CLI carries its provider.

The **Trail** is what each command was asked and what it exited with, two
lines per command. A command that started and never ended, whose process is
gone, is reported as one that died. Words after `--` are counted, not
recorded. The Trail is never exported, and a Purge takes it.

`perch probe` names the Watcher's log and does not read it. On Linux that is
the `journalctl` line it prints. It reads no network, brings no Registry
forward, and exits 0 whatever it finds.

## Letting an agent do it

```
$ perch triage
Starting Claude Code on what Perch sees of this machine, at /Users/you/.config/perch/triage/run-1787059012431.
```

Claude Code opens, asks what went wrong in your own words, reads the evidence
Perch just wrote, investigates this machine, searches the existing issues, and
drafts a report. It shows you the whole thing and posts nothing without your
yes. Where the fix is a Perch command, it names it and runs it only if you
agree. `perch config set --global run-provider codex` hands the session to
Codex instead.

Two copies of the Probe are written: one with your real addresses and paths,
which the agent works from, and one with placeholders, which goes into the
issue. `--raw` writes the real names to both. `--model <name>` passes a model
to the CLI it starts.

The agent never reads a Credential, never edits the Registry by hand, never
patches Perch's source, and never runs `perch holdings purge` or `perch
holdings import` as a fix. Anything that touches the Holdings comes after an
offer to write an Export. The newest three runs are kept under
`~/.config/perch/triage/`.

### When nothing gets launched

```
$ perch triage
Claude Code will not come up as this machine stands, so Perch has not launched it. The Probe found:
  you@example.com is the active Account and it is Quarantined: the provider would not renew its Credential.

What Perch can see of this machine is written down:
  /Users/you/.config/perch/triage/run-1787059012431/prompt.md
  /Users/you/.config/perch/triage/run-1787059012431/probe.raw.txt
  /Users/you/.config/perch/triage/run-1787059012431/probe.txt

Paste prompt.md into any coding agent to run the triage by hand, or open an issue at https://github.com/perch-cli/perch/issues.
```

`prompt.md` carries the whole playbook, so pasting it into any agent gets you
the same session.

A security problem never goes to a public issue.
[Report it privately](https://github.com/perch-cli/perch/security/advisories/new)
instead.
