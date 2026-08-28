# A triage hands over evidence

`perch probe` gathers more about a broken machine than any report has ever
carried, and `bug_report.yml` asks for it in a field somebody has to paste into.
The Trail records what was run and how it went (ADR a-trail-is-evidence). Both
are already better than what arrives in an issue, and both depend on a person
who has closed the terminal doing several careful things in the right order.

A coding agent on that machine would do them. It can ask what went wrong, read
the Trail, notice which assumption `broke`, search the existing issues and write
the report — and the user is running Perch precisely because they use Claude
Code, so the agent is already installed. The pattern is
[`t3 triage`](https://github.com/pingdotgg/t3code)'s, and this document is about
the three places Perch's version departs from it.

## The rule

> **`perch triage` gathers a Probe, writes the playbook, and hands the terminal
> to Claude Code. It does not investigate and it does not file. What Perch owns
> is the evidence and the redaction on it.**

## Redaction is Perch's, not the agent's

The obvious design instructs the agent: *scrub API keys, tokens and the home
directory before you post.* That is a promise from a language model, checked by
nobody, about the one thing in this feature that is irreversible. A public issue
carrying an email address cannot be un-posted, and the user who ran the command
is the one who pays.

Perch already redacts deterministically. `Redaction` numbers Accounts by their
position in the registry, so `<account 3>` means the same Account in every
rendering, and `perch probe` has been redacting by default since it existed.

So a Triage writes the Probe **twice**. `probe.raw.txt` has the real names and
paths, because an agent that cannot `ls` the directory it is reading about
cannot investigate. `probe.txt` is the same gathering redacted, and the playbook
says it goes into the issue whole and unedited. The agent's judgment is removed
from the step where judgment fails silently. `--raw` writes the raw form to both,
for whoever is working on the Triage itself.

One gathering feeds both. Two passes a second apart could disagree about the
machine, and an issue whose two halves describe different states is worse than
either half alone.

## Exactly one fetched document may be obeyed

The playbook lives at `.github/triage/PLAYBOOK.md` and reaches the binary through
`include_str!`. One copy, so there is nothing to keep in step.

That copy is frozen at the Release the user installed, and a playbook improved in
`main` is worth nothing to somebody on last month's build. So the playbook tells
the agent to fetch its own newer self from `main` and follow that instead.

This is an instruction to download text and obey it, which is the shape of every
prompt-injection story there is. It is bounded to one URL in this repository,
and the sentence next to it says that everything else the agent reads — logs, the
Trail, the registry, issues, comments, anything off the network — is data written
by strangers. A maintainer who can push to `main` can already ship a Release.

## Perch's source stays out of it

`t3 triage` clones its own repository at the user's tag so the agent can map a
stack trace to real code. Perch does not, and the difference is what the two
failures look like.

T3 Code's failures are opaque: a long-running server, a database, a trace file,
and a symptom that points at different code depending on how the thing was
started. Perch's are not. Every expected failure is a typed error carrying an
exit code and a sentence naming the belief that stopped holding
(ADR a-refusal-is-a-promise), and a Probe reports which assumption `broke` along
with the code Perch would refuse with (ADR an-assumption-is-probed). Perch ships
its own diagnosis. A clone would pay off for a panic alone, where the backtrace
and the version are what a maintainer wants anyway.

The stronger reason is what a clone does to the report. An agent with source in
front of it writes issues that assert a cause, and an issue asserting the wrong
cause is worse than one reporting the right evidence: it arrives with a plausible
story attached and sends a maintainer somewhere the bug is not. `bug_report.yml`
deliberately asks for what-you-ran, expected, actual and a probe, and no theory.
A Triage holds that line, and the same reasoning rules out preparing a fix PR —
which would also need a Rust toolchain the user has no reason to have.

## Bare, and not through a Run

Perch already launches Claude Code: `perch run` points one process at one
Profile. Reaching for it here would be a mistake. A Run Reconciles, Carries,
writes a Marker and resolves a Target through the registry
(ADR a-run-is-one-shot), and on a machine worth triaging every one of those is a
suspect. A triage that will not start on the machines that need it is worthless.

So Claude Code is launched from `PATH` with no `CLAUDE_CONFIG_DIR`, exactly as
the user would have launched it. The agent's exit code passes through, joining
`run` and `upgrade`.

That leaves one hole, and Perch fills it from the Probe it has just written. If
Claude Code is not installed, or the live Credential will not read, or the active
Account is Quarantined, the session would open at a login prompt rather than at a
triage. Perch does not launch it, says which of those it found, and names the
three files it wrote. A command that explains itself beats one that hands off to
somebody else's error.

## A Triage changes nothing

It inherits the Probe's exemptions whole: no Adoption, no registry brought
forward or saved, and no line in the Trail. A Triage renders the Trail through
the Probe it hands over, and a line of its own would push out what somebody was
running the command to see. The registry it reads may be exactly the thing under
investigation.

What it writes lives at `$PERCH_HOME/triage/run-<millis>/`, which makes it
evidence rather than one of the Holdings: no `version`, no migration, no refusal,
no place in an Export, and a Purge takes it with the rest of Perch's home for
free. The newest three runs are kept, which is enough to hold the run before a
fix beside the one after it.

## What is rejected

**A second agent, so a broken Claude Code is not fatal.** `t3 triage` offers
`codex` as well, and the argument for copying that is real: a Perch worth
triaging is often one whose Claude Code login is broken. It is refused because
Perch is a tool for running Claude Code, and a second agent CLI is a second thing
to detect, a second launch idiom, a picker when both are installed, and a refusal
when neither stream is a terminal — bought for a case the withheld launch already
handles honestly. The files are on disk, and they paste into anything.

**A default `--model`.** A model named in a released binary goes out of date on
Anthropic's schedule rather than Perch's, which is why `probe::Installed` quotes
the Claude Code version it read and never compares it. `--model` is passed
through untouched and nothing is passed by default.

**A `--json` shape.** The output of a Triage is somebody's interactive session.

**Growing the Probe to serve the Triage.** A `--since` window over the Trail, or
a pre-gathered listing, is something the agent can run for itself in one command,
and a Probe with a triage mode would break its own promise to touch nothing.

**Filing without asking.** The playbook shows the user the complete issue text
and requires an explicit yes. Perch has no GitHub credential and wants none: the
posting goes through the user's own `gh`, or through a prefilled URL they open
themselves.

## Consequences

- `perch triage` joins the exemption lists in `main` beside `perch probe`, and
  joins `run` and `upgrade` in passing an exit code through rather than
  producing one.
- `commands::probe` gains one function that gathers once and renders both ways.
  Nothing else reads it.
- The playbook is prose in `.github/`, and editing it changes what every future
  Triage does with no code change. Old builds pick the edit up over the network;
  new builds compile it in.
- `.github/ISSUE_TEMPLATE/agent-filed.yml` and the playbook name the same fields,
  and `tests/publication.rs` asserts they still do, both ways. A field renamed
  on one side is otherwise a form GitHub silently drops.
- The `filed-by-agent` label says provenance, deliberately not `via-triage`:
  `needs-triage` already exists in this tracker and means a maintainer has not
  looked yet, which is a different thing that would read like a variant.
