---
title: "Backing up, moving machines, and giving the machine back"
sidebar:
  label: "Backing up and moving machines"
  order: 7
---

Your Holdings are everything Perch holds on this machine: every Account, its
Alias, its Group, its Settings and its Credential. `perch holdings export`
writes them to one encrypted file, `perch holdings import` puts them back, and
`perch holdings purge` gives them up.

## Backing up everything

```
$ perch holdings export ~/perch-backup.age
Choose a passphrase. Nothing opens the Export without it.
Passphrase:
Again:
Exported 3 Accounts.
```

Keep the passphrase somewhere that is not beside the file. Without it the
Export cannot be opened, and logging in again is the only way back.

The Export takes everything and has no Target. The passphrase is typed at the
prompt, never shown, and there is no flag or argument for it, so this is the
one command a script cannot drive. The file is an `age` file in its text
encoding, which the standard `age` tool also decrypts.

Nothing is renewed on the way. An Account something is running against is read
like any other. A Credential Store that will not say what it holds stops the
whole Export.

A path that is already taken is refused rather than written over, and a
directory that does not exist is refused rather than created.

## Moving to another machine

```
$ perch holdings import ~/perch-backup.age
Passphrase:
Imported 3 Accounts.

$ perch list
  Account               Alias     Group  State                  Headroom        Utilization
  you@example.com       -         work   -                      never observed  never observed
  overflow@example.com  overflow  work   -                      never observed  never observed
  spare@example.com     -         none   disabled, quarantined  never observed  never observed

spare@example.com: Anthropic would not renew its Credential.
`perch relogin spare@example.com` repairs it.
```

Nothing arrives active. Claude Code goes on as whatever it is logged in as
until you `perch switch <target>`:

```
$ perch status
Perch holds no active Account. `perch switch <target>` makes one of the 3 it holds active.   # exit 12
```

The login already on the new machine is not adopted. Credentials land wherever
this machine's Claude Code keeps one, so an Export from a Mac restores onto
Linux and the other way round. An Account the Export carried no Credential
for arrives Quarantined, and `perch relogin` ends that.

An Import refuses a Perch that already holds an Account. There is no `--force`
and no merge:

```
$ perch holdings import ~/perch-backup.age
Perch already holds 3 Accounts and 1 Group, and an Import lands only on an empty machine. `perch holdings purge` makes room.   # exit 13
```

A wrong passphrase fails before anything is written. An Import that fails part
way takes back every Profile it made, and the file can be imported again. An
Export written by a newer Perch is refused, naming the version that wrote it.

## Giving the machine back

```
$ perch holdings purge
Perch holds 3 Accounts: you@example.com, overflow@example.com, spare@example.com.
A Purge deletes their Profiles, their Credentials, and /Users/you/.config/perch itself. Nothing undoes it.
Claude Code goes on running as whatever it is logged in as.
Write an Export first? [Y/n]: y
Where to write it: /Users/you/perch-backup.age
Choose a passphrase. Nothing opens the Export without it.
Passphrase:
Again:
Exported 3 Accounts.
Type `purge` to give the machine back: purge
Purged 3 Accounts.
```

A Purge takes no Target. Giving up one Account is `perch remove`. It offers an
Export first and takes no for an answer. An Export that cannot be written
stops the Purge with everything still in place, and a path inside
`~/.config/perch` is refused.

The prompt wants the word `purge`, not `y`. `--yes` answers every question
ahead of time and writes no Export. Without a terminal and without the flag, a
Purge is refused.

A Service `perch watcher install` left is stopped and removed first. Whatever
Claude Code is logged in as is left where it is. A Purge is refused while a
client is running against one of the Profiles, checked before the questions
and again after them. A Purge that stopped part way can be run again, and
carries on from what is still there.
