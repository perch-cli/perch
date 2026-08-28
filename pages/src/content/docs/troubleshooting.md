---
title: "When something goes wrong"
sidebar:
  order: 9
---

Two commands for the same moment. `perch probe` gathers everything Perch can see
of this machine, and `perch triage` hands that to Claude Code and lets it do the
investigating and the writing. Neither changes anything: no registry brought
forward, no line added to the Trail, nothing repaired behind your back.

## What Perch can see of this machine

`perch probe` gathers what a report needs and would otherwise be typed by hand:
which Perch and which Claude Code, how Perch was installed, where its files are,
what the Holdings hold, which of its assumptions about Claude Code still hold,
and what has been run here lately.

```
$ perch probe
Findings
  <account 3> is Quarantined, so Cycling will not choose it and a Switch to it
  refuses. `perch relogin <account 3>` is the way back. (exit 19)

Perch         0.2.0 (linux x86_64), installed by npm
Claude Code   2.1.221, at <home>/.local/share/claude/bin/claude
Home          <home>/.config/perch, registry version 5
Active        <account 1>
Holdings      4 Accounts in 2 Groups, 1 Quarantined, 0 Disabled
Watcher       installed, running, may act somewhere
Its log       <home>/.config/perch/watch.log
Trail         412 lines, last written 2026-08-28 09:16:41Z
...
```

The judgment comes first and the facts sit under it, so the facts still stand
where the judgment is wrong. A finding only ever restates something Perch
already works out to decide a refusal, and carries the code that refusal exits
with.

Email addresses, Alias and Group names and your home directory come out as
placeholders, because the point of this is pasting it somewhere else. The
numbers are the Account's place in the registry, so `<account 3>` is the same
Account every time you run it — two reports a week apart are comparable.
`--raw` prints the names as they are, and is worth checking before you paste.

It reads and judges and repairs nothing. No network, no registry brought
forward, no line added to the Trail, and it exits `0` whatever it finds.

Every command writes two lines to the **Trail** as it goes, one when it starts
and one when it ends: what was typed, and what it exited with. Words after `--`
go to Claude Code and are counted rather than recorded. A start with no end
whose process is gone is a command that died without a word, and `perch probe`
says so — one whose process is still running is not, which is why the Watcher
sitting in its loop is never reported as a failure. The Watcher adds a line of
its own when it Switches. The Trail is not one of the Holdings — it is never
exported, and a Purge takes it with everything else.

`perch probe` names the Watcher's own log and does not read it. On macOS and
Windows that is `watch.log` beside the registry; on Linux systemd keeps it, and
what the Probe prints is the `journalctl` line to run.

## Letting an agent do it

Typing out a bug report is the last thing anybody wants to do at the moment they
need to. `perch triage` hands the job to the Claude Code you already have.

```
$ perch triage
What Perch can see of this machine is at
<home>/.config/perch/triage/run-1787059012431. Starting Claude Code, which will
ask what went wrong.
```

The agent asks what went wrong in your own words, reads the evidence Perch just
wrote, investigates this machine, searches the existing issues, and drafts a
report. It shows you the whole thing and posts nothing without an explicit yes.
Where the fix is a Perch command, it says which and runs it only if you agree.

Perch itself only gathers. Two copies of the probe are written: one with your
real email addresses and paths, which is what the agent works from, and one with
placeholders, which is the copy that goes into the issue. That redaction is
Perch's own rather than something the agent is asked to remember, so an address
does not reach a public issue because a model was careless. `--raw` writes the
real names to both, and is for debugging the command itself.

Some things are off limits and the agent is told so: it never reads a Credential,
never edits the registry by hand, never patches Perch's source, and never runs
`perch holdings purge` or `perch holdings import` as a fix. Anything that touches
the Holdings at all comes after an offer to write an Export first.

`--model` passes a model straight through to Claude Code, for when its default is
not the one you want.

### When nothing gets launched

If Claude Code is not installed, or the Account you are on is Quarantined, the
session would open at a login prompt instead of a triage. Perch does not launch
it, says which of those it found, and tells you where the three files are:

```
$ perch triage
Claude Code will not come up as this machine stands, so Perch has not launched
it. The Probe found:
  you@example.com is the active Account and it is Quarantined: Anthropic would
  not renew its Credential.

What Perch can see of this machine is written down:
  <home>/.config/perch/triage/run-1787059012431/prompt.md
  <home>/.config/perch/triage/run-1787059012431/probe.raw.txt
  <home>/.config/perch/triage/run-1787059012431/probe.txt

Paste prompt.md into any coding agent to run the triage by hand, or open an issue
at https://github.com/perch-cli/perch/issues.
```

`prompt.md` carries the whole playbook, so pasting it into any agent gets you the
same session. The newest three runs are kept and older ones are dropped, and a
Purge takes the lot with the rest of what Perch holds.

A **security problem** never goes to a public issue.
[Report it privately](https://github.com/perch-cli/perch/security/advisories/new)
instead, because Perch holds Claude Code credentials.
