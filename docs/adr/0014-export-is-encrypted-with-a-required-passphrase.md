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
