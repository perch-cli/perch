---
title: "Backing up, moving machines, and giving the machine back"
sidebar:
  label: "Backing up and moving machines"
  order: 7
---

Three commands under one noun, and they are each other's inverse. Your Holdings
are everything Perch holds on this machine: an export writes them to one
encrypted file, an import puts them back, and a purge gives them up, leaving the
machine as it was before Perch.

## Backing up everything

`perch holdings export <path>` writes everything Perch holds to one encrypted
file: the whole registry — every Account, its Alias, its Group, whether Cycling
may choose it, why it is Quarantined where it is, and what each Group carries —
alongside every Credential. A dead machine, a mistaken `perch remove` or a new
laptop then costs you a file rather than a login for every subscription.

```
$ perch holdings export ~/perch-backup.age
This file holds a working Credential for every Account Perch has. It is encrypted with a passphrase you choose, and there is no way into it without one.
Passphrase:
Again:
Exported 3 Accounts to /Users/someone/perch-backup.age.
```

Keep the passphrase somewhere that is not beside the file. Without it there is
nothing in that file, and nothing Perch holds can get it back — which is what
the prompt says while you are still choosing one, and why the report does not
say it again.

**It takes everything and has no target.** There is no per-Account and no
per-Group form, because a selective export is a partial restore — which is the
failure the file exists to prevent, wearing a feature's clothes.

**The passphrase is required rather than offered**, prompted, confirmed, and
never shown as you type it. It cannot be passed as an argument and there is no
flag that answers ahead of time: an argument sits in the process table for
anything on the machine to read, and in a shell history afterwards. So this is
the one command in Perch that a script cannot drive — without a terminal it is
refused, and the refusal names the terminal rather than a way round it.

**The file is an `age` file**, in `age`'s text encoding, so the standard `age`
tool reads it on a machine that has never heard of Perch:

```
$ age --decrypt ~/perch-backup.age
Enter passphrase: 
{"version":1,"registry":{ ... },"credentials":{ ... }}
```

A forgotten passphrase means the export is gone and logging in again is the only
way back. That is the correct trade for a file holding every Credential at once.

Nothing is Renewed and nothing Rotated on the way — an export reads what is
stored — and an Account something is currently running against is read like any
other, because only *writing* into a Live Profile is refused. A store that is
there and will not say what it holds stops the whole export rather than shrinking
it: an export that quietly left one Account out would only be found wanting on
the day it was needed. An Account whose stores hold nothing is still exported,
Quarantine reason and all, and the command says which.

The path is read as somebody's typing rather than as an instruction. Nothing is
written over — a path that is already taken is refused, and checked again after
the passphrase, so a file that arrived while you were typing is safe too — and a
directory that is not there is refused rather than created, because one Perch
made for a path you typed would be a directory you did not ask for, at
permissions you did not choose.

## Moving to another machine

`perch holdings import <path>` is the exact inverse: it puts the whole registry
and every Credential back, so a new laptop arrives with the setup the old one
had rather than a pile of nameless logins.

```
$ perch holdings import ~/perch-backup.age
Passphrase:
Imported 3 Accounts from /Users/someone/perch-backup.age.
```

The passphrase is the one the file was written with, asked once rather than
twice — a passphrase being *chosen* is confirmed because a file nobody can open
is not found out about until it is needed, and one being *checked* is answered
by the file itself a moment later. Nothing is restored until it opens.

**Nothing arrives active.** An Import restores what Perch holds and does not
touch what Claude Code is logged in as, so `perch switch <target>` is what makes
one of them active. That is true of every Import, which is why it is said here
rather than by the command.

**It refuses a Perch that already holds an Account**, and names
`perch holdings purge` as the way to make room. Merging is where every hard case
lives — the same Account on both sides one Rotation apart, with no way to tell
which Credential is live; an Alias meaning different Accounts on two machines; a
Group that exists in both with different members. That is a real feature and it
is not this one. Refusing keeps an import the exact inverse of a purge, and that
pair is what makes moving machines true. There is no `--force`, because a flag
would be the merge wearing a shortcut's clothes.

**Credentials land where this machine keeps one.** The file records a Credential
against an email address and nothing about the store it came out of, so an
export taken on a Mac restores into files on Linux and the other way round,
without either side knowing about the other's store.

**Nothing is made active.** The Account that was active where the export was
taken is a fact about that machine, and an import writes nothing into this one's
Default Profile — whatever Claude Code is logged in as goes on running until you
`perch switch`. The login that is already on the machine is not adopted either:
it is left exactly where it is, to switch onto or to ignore.

The passphrase is required and prompted once — a wrong one fails before anything
at all is written, and is told apart from a file that is not an export. Without a
terminal it is refused for the same reason `perch holdings export` is, and there
is no flag.

An import that fails part way leaves nothing behind: every Profile it made comes
back out, so there is no half-populated registry and no orphaned Profile, and the
file can simply be imported again. A registry inside the file written by a newer
Perch is refused rather than half-read. An Account the export carried no
Credential for is still restored, Quarantine reason and all, and the command says
which — `perch relogin` is what ends that.

## Giving the machine back

`perch holdings purge` is the other half of that pair: every Profile, every
Credential Perch holds and Perch's own registry, gone in one act, so the machine
is the one you had before you installed it. It is what makes room for an import,
and what you run on the laptop you are handing on.

```
$ perch holdings purge
Perch holds 3 Accounts: someone@example.com, overflow@example.com, spare@example.com.
A Purge deletes every one of their Profiles, every Credential Perch holds for them, and /Users/someone/.config/perch itself. Nothing undoes it: only a fresh login brings an Account back, and it comes back as a new one.
Claude Code goes on running as whatever it is logged in as.
Write an Export first? [Y/n]: y
Where to write it: /Users/someone/perch-backup.age
This file holds a working Credential for every Account Perch has. It is encrypted with a passphrase you choose, and there is no way into it without one.
Passphrase:
Again:
Exported 3 Accounts to /Users/someone/perch-backup.age.
Type `purge` to give the machine back: purge
Purged 3 Accounts, and /Users/someone/.config/perch is gone.
The Export is at /Users/someone/perch-backup.age, and holds a working Credential for every Account. Keep it somewhere you would keep those. `perch holdings purge` will not write over it.
```

The question is where a Purge says what it will take and what it will leave, so
the report afterwards says only what it took. `perch holdings import` is what
puts an Export back on a machine in that state.

**It takes no target.** Giving up one Account is `perch remove`, which is
deliberately narrow — two verbs for one act is exactly the ambiguity the shared
Alias and Group namespace exists to prevent, so a purge does not accept one.

**It offers an export first, and takes no for an answer.** Perch cannot verify
that an export exists — the file is wherever you put it, possibly on another
machine — so requiring one is a check that checks nothing. Offering to write one
is not: it is a keystroke, and it is the only thing that makes a purge
survivable. An export that cannot be written — a path that is taken, a directory
that is not there, a passphrase mistyped — stops the purge with everything still
where it was, and a path *inside* `~/.config/perch` is refused, because that file
would go with the purge that offered it.

**The prompt wants the word.** It lists the Accounts by email address rather than
by Alias — what is being given up is the login, and that is the address you would
check against your password manager — says that nothing undoes it, and wants
`purge` typed out. A `y` is what fingers answer before eyes have read anything.
`--yes` answers ahead of time and writes no export, which is the whole of what a
script can be asked for; without a terminal and without the flag, a purge is
refused and end of input is never agreement.

**A Service goes too, and goes first.** Where
[`perch watcher install`](watching.md#having-the-machine-run-it) has left one,
the Purge says so among the things it is about to take and removes its unit, so
nothing starts at your next login. First, because nothing may be Switching
Credentials into Profiles this is deleting. It is the one thing a Purge takes
that lives outside Perch's own directory.

**Whatever Claude Code is logged in as is left exactly where it is.** The live
Credential in the Default Profile is Claude Code's own rather than a copy Perch
holds, and a purge that logged you out of the tool you are using would be doing
more than giving the machine back. A purge is refused while a client is running
against one of the Profiles, for the same reason every other write into a Live
Profile is — a purge deletes those directories, and what is in them belongs to
whatever is holding them. That is checked before the questions and again after
them, because a client started while you were typing was not running when the
first check ran.

The Credentials go first and the registry naming them goes last, which is what
makes a purge that stopped part way finishable: a keychain item lives outside
`~/.config/perch`, so the record of which items there are is the last thing to
go. A purge that failed anywhere — a store that would not give its Credential up,
a directory that would not go — leaves the registry standing, and running it
again finds what it already deleted already gone and carries on.
