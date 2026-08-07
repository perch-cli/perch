# Export is encrypted with a required passphrase

`perch export` is the only command that turns keychain-held secrets into a file.
The point of that file is to be kept somewhere durable — a backup drive, a
password manager, another machine — which is precisely where a plaintext bundle
of refresh tokens granting full access to every account someone owns should
never sit. A file like that leaks by being backed up, synced, or committed.

Export therefore encrypts with a passphrase, prompted on export and required on
import. Not optional: an optional passphrase is one people skip, and the failure
is silent until it isn't.

## Considered Options

Plaintext JSON is trivially restorable and inspectable. Rejected because
shipping it honestly would mean telling users to treat the file like an SSH
private key, which most will not.

Offering no export at all is the smallest attack surface, but it makes `perch
purge` and `perch remove` unrecoverable and leaves no way to move accounts
between machines without logging in again everywhere.

## Consequences

A forgotten passphrase means the export is gone, and re-login is the only path
back. That is the correct trade for a file holding every credential at once.

## Amended: the file is an age file, and it holds the whole machine

This record required a passphrase and named no primitive, and said nothing
about what goes in the file or what happens on the way back in.

**The format is age**, taken as a crate, in passphrase mode. ADR 0025's rule is
that a crate is taken unless it would sit on the wrong side of a seam, and
encryption sits on neither seam — it is not an effect the Host port carries,
and it is not shared with Claude Code, so nothing here has to be bug-compatible
with anything. Against that, the alternative was three crates and a header
format Perch would version itself, for a bundle written perhaps twice a year.

The property that decided it is that an age file can be decrypted by the
standard `age` command. This file is meant to outlive the machine it was
written on, and a backup readable only by the tool that wrote it is a worse
backup than one whose format somebody else maintains.

**Export takes everything and has no target.** The whole registry — aliases,
groups, whether cycling may choose an account, group configuration — alongside
every credential. Restoring the credentials alone would leave a new machine
holding working accounts stripped of every name and rule the user gave them. A
per-account export was considered and dropped: a selective export is a partial
restore, which is the failure mode this record exists to prevent, wearing a
feature's clothes.

**Import refuses a non-empty registry**, and names `perch purge` as the way to
make room. Merging is where every hard case lives — the same account on both
sides one rotation apart, with no way to tell which credential is live; an
alias meaning different accounts on two machines. That is a real feature and it
is not this one. Refusing keeps import the exact inverse of purge, which is the
pair that makes moving to a new machine true.

**Purge offers to export first.** Perch cannot verify that an export exists —
the file is wherever the user put it, possibly on another machine — so
requiring one is a check that checks nothing. Offering to write one is not: it
is a keystroke, and it is the only artifact that makes purge survivable.

## Amended: the file is armored, and the passphrase is typed at a terminal

The amendment above settled the format and the contents and left two things to
whoever wrote the command. Both turned out to be decisions rather than details.

**The file is armored** — `age`'s own text encoding of the same `age` file,
which `age -d` reads back without being told. Two things follow and both are
wanted. The result is a `str`, so an export goes out through the Host port's
ordinary private write rather than needing a second, bytes-shaped way to write a
file: every other secret Perch writes is text, and one binary path would be one
path where the mode, the atomic replace and the failure cleanup are written
again. And an export is something a person may reasonably paste into a password
manager, which is a thing you can do with text and not with a blob. The cost is
about a third more bytes, on a file written perhaps twice a year.

**The passphrase is typed at a terminal, and there is no flag.** Every other
capability in Perch is reachable from a script, because it has to be complete
over SSH and in CI (ADR 0011). This one is the exception, and it is the same
rule an access token travels under (ADR 0021): a value passed as an argument
sits in `argv` where any process on the machine can read it off the process
table, and a value in a shell history outlives the command that used it. So
`perch export` without a terminal is refused, and the refusal names the terminal
rather than a way round it — an escape hatch here would be the whole of what the
required passphrase was for.

It is prompted twice and confirmed for the same reason it is required at all: a
passphrase mistyped once is a file nobody discovers is unreadable until the
machine it would have restored is gone.

**Nothing is written over.** The one argument is a path somebody typed, and what
the command does with it is replace the file there. A mistyped path would
otherwise make the backup command destroy whatever it was pointed at, and an
export landing on an older export is the older backup gone — which is the
opposite of what a file that accumulates is for. A path that is taken is refused,
and naming a free one is a keystroke.

## Verified

`age` 1.3.1 — Filippo Valsorda's Go implementation, the reference one — decrypts
what `perch export` writes, given the passphrase, on a machine that has never
heard of Perch. Checked by hand rather than in CI: the property is what the crate
promises, and installing the Go tool on three runners to re-check it every push
buys less than it costs. What CI does hold is that the file carries `age`'s armor
header and that its recipient is scrypt, which is the pair that makes `age -d`
recognise the file and ask for a passphrase.
