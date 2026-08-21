---
title: "Running one Account in one terminal"
sidebar:
  order: 6
---

`perch run <target>` launches Claude Code as an Account without changing which
one is active. It is the other half of `switch`: a Switch is about the whole
machine, and a Run is about one process.

```
$ perch run overflow
`overflow` is an Alias for overflow@example.com.
Running Claude Code as overflow@example.com (as `overflow`), in this terminal alone. you@example.com stays the active Account everywhere else.
```

It works by setting `CLAUDE_CONFIG_DIR` for that one process. Nothing is
Captured, nothing is written to the Default Profile, and no Identity is patched
— so every other terminal, the editor extension and the desktop app go on as the
Account they were on. Two terminals running two Accounts is what the command is
for, not an edge case.

Because a Run uses a Profile as a live configuration directory rather than as
storage, it is the one path that has to **Reconcile** first, every time: your
memory, settings, plugins, past work and plans are linked into the Profile it is
about to launch, and links that have broken or gone stale are repaired. What
crosses is everything the Default Profile holds except the Credential, the file
naming the Account, the directory of Markers and the refresh lock, read at Run
time — so a directory a new Claude Code release invents follows you without
waiting for a Perch release. Never by copying, because a copy diverges the
moment it is edited: where no link can be made, the Run is refused rather than
served one, naming the entry and what to do about it.

One file cannot be linked. `.claude.json` holds the Account itself, so every
Profile keeps its own — and it also holds a good deal that is yours: whether you
have been through onboarding, which tips you have seen, and the trust and tool
approvals of the repository you are standing in. So that one file is **Carried**
key by key instead, from the most recently used Profile in the same Group, and
only into a Profile nothing is running against. What crosses is a named list
rather than everything-but, because this file also holds figures Anthropic gave
for one Account — carrying those would show you one Account's Utilization under
another Account's name. Without it, the first Run of a new Account lands you in
a Claude Code that believes it has never been used, asking for trust in the
middle of your task.

A Group is not a Target here — it names a set of Accounts rather than one, and
there is no single Profile to point a process at — and an Account that is
Quarantined is refused with exit code 19 rather than launching a client that
would ask you to log in. The client's own exit code is Perch's, so `perch run`
can stand in a script wherever `claude` would.

## What a Run protects while it lasts

For as long as a Run is running, the Profile it launched is a **Live Profile**,
and Perch will not write into one. Another terminal cannot Capture into it,
cannot Renew the Credential that client is holding — which would retire the
refresh token and log it out mid-task — and cannot copy `.claude.json` keys over
it.

Each of those refuses in the register its own command has. A Switch that cannot
Capture stops, exits 16, and names the process holding the Profile. A
`--refresh` shows you the cached figure instead and still succeeds, because a
refresh reports what it could not read rather than failing. A `.claude.json` key
simply does not cross, because nothing on that path may refuse a Run — the cost
is one onboarding question, not a session.

Reading is untouched, which is the difference that matters. `perch switch` onto
the Account you are running lands normally: it copies that Credential into the
Default Profile and leaves the Profile itself alone. Its Utilization is read
without renewing anything, because an Account with a client running has a fresh
access token already. A Run and a Switch do not lock each other out.

It works by the **Marker** Claude Code already uses to record a running client,
so a Run that was killed rather than closed leaves nothing behind that matters:
a Marker naming a process that is gone, or a pid since taken by something
younger, makes no Profile Live.

## Running with arguments, and running something else

Everything after `--` belongs to the program rather than to Perch, and reaches
it exactly as you typed it — including flags Perch has of its own.

```
$ perch run overflow -- --resume --model opus
$ perch run overflow -- npm test
```

The first word after `--` decides which program runs. A flag is Claude Code's,
so the first line resumes a session; anything else is the program to launch, so
the second runs `npm` with your Shared State reachable and `CLAUDE_CONFIG_DIR`
pointed at the Account's Profile. Nothing is guessed either way: a program you
could invoke by name never begins with a `-`.

`--` is required, and a flag typed without it is refused rather than claimed:

```
$ perch run dev --resume
`--resume` could be Perch's flag or the program's, and Perch will not guess which. Everything meant for the program you are running goes after `--`:

    perch run dev -- --resume

$ echo $?
2
```

Both readings of that line are real — Perch has a `--json` and so does Claude
Code — so Perch takes neither and hands you back the line that would have
worked. A program typed without the separator (`perch run dev npm test`) is told
the same thing: nothing but `--` follows a Target.
