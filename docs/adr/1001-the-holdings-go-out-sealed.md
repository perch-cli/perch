# The Holdings go out sealed

**`perch holdings export` is the only command that turns Credentials into a
file. That file is an `age` file in `age`'s own text encoding, encrypted with a
passphrase somebody types at a terminal, and it carries the whole of what Perch
holds or it is not written. `perch holdings import` is its exact inverse and
refuses a machine holding anything. `perch holdings purge` offers to write one
before it destroys anything.**

The point of the file is to be kept somewhere durable — a backup drive, a
password manager, another machine — which is precisely where a plaintext bundle
of refresh tokens granting full access to every Account somebody owns should
never sit. A file like that leaks by being backed up, synced or committed. So
the passphrase is required rather than offered: an optional one is one people
skip, and the failure is silent until it isn't.

Plaintext JSON is trivially restorable and inspectable, and shipping it honestly
would mean telling people to treat the file like an SSH private key, which most
will not. Offering no Export at all is the smallest attack surface, and it makes
`perch holdings purge` and `perch remove` unrecoverable and leaves no way to move
Accounts between machines without logging in again everywhere.

A forgotten passphrase means the Export is gone and re-login is the only path
back. That is the correct trade for one file holding every Credential at once.

## The format is `age`, and it is armored

`age` is taken as a crate. Encryption sits on neither of Perch's seams — it is
not an effect the Host port carries, and it is not shared with Claude Code, so
nothing here has to be bug-compatible with anything, which is the test
ADR a-crate-must-not-cost-a-seam sets. The alternative was three crates and a
header format Perch would version itself, for a bundle written perhaps twice a
year.

What decided it is that an `age` file can be decrypted by the standard `age`
command. This file is meant to outlive the machine it was written on, and a
backup readable only by the tool that wrote it is a worse backup than one whose
format somebody else maintains.

**Armored** is `age`'s own text encoding of the same file, which `age -d` reads
back without being told. Two things follow and both are wanted. The result is a
`str`, so an Export goes out through the Host port's ordinary private write
rather than needing a second, bytes-shaped way to write a file — every other
secret Perch writes is text, and one binary path would be one path where the
mode, the atomic replace and the failure cleanup are written again. And an
Export is something somebody may reasonably paste into a password manager, which
is a thing you can do with text and not with a blob. The cost is about a third
more bytes.

## The work is Perch's to fix, not the machine's

`age` decides scrypt's work factor by timing 2^10 rounds on the machine doing
the encrypting and doubling while that measurement stays under a second, and it
bounds what it will spend decrypting at about a second of work *there* plus four
doublings. Left alone, both numbers are properties of whichever machine happened
to be running, and whether an Export opens becomes a question about the pair of
machines it traveled between: a file written on a desktop and carried to a
laptop, or opened inside a container with less CPU than the one that wrote it,
is refused on that alone. A file meant to outlive the machine that wrote it
cannot have what opens it depend on the machine that opens it.

The calibration is worse than machine-dependent on the writing side, because it
has no floor. On anything CPU-starved enough that 2^10 rounds already take a
second — a quota'd CI container, a loaded laptop, a Pi — the doubling loop never
runs and the file is sealed at 1024 rounds, which is a GPU-hours problem for a
passphrase a person chose. Nothing in the file, in the report or in the read back
would say so.

So Perch pins both: 19 doublings to seal, and a ceiling of 20 to open. 19 is
above `age`'s own guess at a second of work, because the cost is paid once per
Export and once per Import on a file that is kept. Slow hardware pays seconds for
it, which is the right way round — the alternative is the hardware deciding how
well the backup is encrypted.

The ceiling is one doubling above what Perch writes rather than the 22 `age`'s
own guidance tops out at, because the factor is read out of the file's header and
sizes a buffer *before* the passphrase can be doubted: a header claiming 22 has
an Import asking the allocator for four gigabytes on the strength of a number a
stranger wrote. The cost is that an `age -a -p` file sealed above 2^20 — which a
fast machine's calibration can reach — is refused rather than opened, and the
refusal names `age -d`, which will open it.

## It takes everything and has no target

The whole Registry — every Account, its Alias, its Group, whether Cycling may
choose it, why it is Quarantined, and what each Group carries — alongside every
Credential and each Profile's own `.claude.json`. Restoring the Credentials
alone would leave a new machine holding working Accounts stripped of every name
and rule their owner gave them, and restoring without the identity files would
put every Account into the degraded state adoption goes out of its way to keep
the first one out of, with nothing for a Run to Carry from
(ADR everything-but-the-account).

A per-Account form is refused. A selective Export is a partial restore, which is
the failure this whole format exists to prevent, wearing a feature's clothes. So
the surface is a path, and the command is the same command however many Accounts
Perch holds.

Nothing is Renewed and nothing is Rotated on the way. A Renewal may Rotate, and
a file that retired the refresh token of every Account in the course of recording
it would have broken the machine it was taken from. For the same reason a store
that is there and will not say what it holds stops the whole Export: reporting an
Account as having no Credential where the truth is a locked keychain writes a
file that restores to a machine of logins that do not work, and its owner finds
out on the day they need it.

Nothing is written over, either. The one argument is a path somebody typed, and
what the command does with it is replace the file there — so a mistyped path
would make the backup command destroy whatever it was pointed at, and an Export
landing on an older Export is the older backup gone. Naming a free path is a
keystroke.

## The passphrase is typed at a terminal, and there is no flag

Every other capability in Perch is reachable from a script, because it has to be
complete over SSH and in CI (ADR perch-does-not-draw). This is the exception,
and it is the rule an access token already travels under: a value passed as an
argument sits in `argv` where any process on the machine can read it off the
process table, and a value in a shell history outlives the command that used it.
So an Export without a terminal is refused, and the refusal names the terminal
rather than a way round it — an escape hatch here would be the whole of what the
required passphrase was for.

It is prompted twice and confirmed for the same reason it is required at all: a
passphrase mistyped once is a file nobody discovers is unreadable until the
machine it would have restored is gone. An Import prompts once. Confirming is
what you do to a passphrase being *chosen*; one being *checked* is answered by
the file itself a moment later.

## An Export is refused rather than migrated

An Export carries two versions: its own envelope, and the Registry traveling
inside it. Both are read off a shape that is only the versions, before the
document is read as an Export at all — because a newer Perch is exactly the thing
that writes a value this build has no variant for, and reading the document first
fails on that with serde's own words, telling somebody their backup file is
unreadable about a file that is perfectly well-formed.

A build that does not understand a version refuses and says which version wrote
the file. That is the answer an Export gets rather than a migration, because the
Perch that wrote it can still open it: refusing takes away nothing that is not
still there (ADR the-holdings-outlive-a-perch). Which means a change to the
shape of an Export moves its version with it.

## An Import refuses a machine that holds anything

Merging is where every hard case lives — the same Account on both sides one
Rotation apart, with no way to tell which Credential is live; an Alias meaning
different Accounts on two machines; a Group in both with different members. That
is a real feature and it is not this one. Refusing keeps an Import the exact
inverse of a Purge, and that pair is what makes moving to a new machine true. A
Registry that is there and holds nothing — what a Purge leaves — is as empty as a
machine Perch has never run on.

**An Import adopts nothing.** Every other command reads the Registry through
adoption, which takes the existing Claude Code login over the first time Perch
runs (ADR a-login-perch-does-not-need). Doing that here would make the machine
hold one Account on the way to refusing itself for holding one. So an Import
reads the Registry directly, and the login on the machine is left exactly where
it is: whoever imported can Switch onto it or ignore it.

**Nothing is made active, and nothing arrives having just been checked.** Being
active is a claim about which Credential is in *this* machine's Default Profile,
and making one active would mean replacing the live Credential of whatever is
logged in — the thing `perch remove` goes to lengths not to do. A Check is a
claim about a watcher, and no watcher has run here yet.

**Anything that fails part way takes back what it placed.** A machine holding
some of an Export is the partial restore this format exists to prevent, arriving
by accident rather than as a feature. What comes back out is what the Import
*made*, which is narrower than what it wrote into: a Profile directory nothing
names outlives every command that would have named it, and on macOS a Credential
Store's name is derived from that directory — so deleting one the Import merely
wrote into destroys the only name that could still reach a live Credential
beside it. Such a directory is left where it is, and said.

## A Purge asks for the word, and finishes

**It offers an Export first.** Perch cannot verify that one exists — the file is
wherever its owner put it, possibly on another machine — so requiring one is a
check that checks nothing. Offering to write one is not: it is a keystroke, and
it is the only artifact that makes a Purge survivable. The offer goes through the
same code as `perch holdings export`, one function below the command, because a
Purge holds the Registry lock across the offer and could not take it a second
time. It carries the path refusals with it, plus one of its own: a path under
Perch's home is refused, because the Purge that offered it would take the file
moments later.

**The word rather than a letter.** Every other confirmation in Perch is `[y/N]`,
and `y` is what fingers answer before eyes have read anything. This is the one
command nothing undoes, so the prompt lists the Accounts by email address — the
login is what is being given up, and the address is what somebody would check
against their password manager — states that nothing brings them back, and wants
`purge` typed out. `--yes` answers ahead of time, the idiom `perch remove`
already uses, and it answers the Export offer too: an Export is a path somebody
names and a passphrase somebody types, neither of which a script can be asked
for. Without a terminal and without the flag a Purge is refused, and that refusal
names the flag rather than the terminal — unlike an Export's, because there is
deliberately nothing that answers a passphrase ahead of time.

**A live Profile refuses a Purge**, which is
ADR a-profile-is-live-by-evidence's rule at its extreme: a Purge does not write
into those directories, it deletes them. Asked of every Account, because a Purge
is all or nothing and a refusal discovered half way through is the partial state
the check exists to prevent. Doubt counts as a client: waiting costs a command
run again, and not waiting costs whatever that client had open. Asked twice —
before the questions and again after them — because somebody may have started a
client while the passphrase was being typed, and an answer that was true four
prompts ago says nothing about the Profile the next line deletes. The first ask
earns its place by not putting five questions to somebody who was always going to
be refused.

**A Purge walks the directory rather than the Registry.** The Registry is not the
whole account of what Perch holds, and the difference is not exotic: a login
abandoned at the browser step leaves a working Credential under `pending/`, and a
Profile whose store would not empty is deliberately kept where it is. What makes
that fatal rather than untidy is that a Credential Store is derived from its
Profile's path, so taking Perch's home whole destroys the only name that could
ever reach a keychain item outside it. So one walk finds every directory Perch
holds, the refusal and the deletion filter it the same way, and a directory that
cannot be listed or cannot be named stops the Purge rather than reading as
nothing to do.

**Credentials first, the home last.** A Credential in the operating system's
keychain lives outside Perch's home, so the Registry naming which items are there
is the only record of them, and removing the home first would strand them with
nothing left to find them by. Taking every Credential first and the home whole
afterwards means a Purge that failed anywhere leaves the Registry standing, so
running it again finds what it already deleted already gone and finishes — and a
home left behind holding no Registry, which is what a Purge interrupted in its
last step leaves, is taken by the next one rather than reported as nothing to do.

What a Purge does not touch is the Credential in the Default Profile. That is
Claude Code's own login rather than a copy Perch holds — the same line
`perch remove` draws when it gives up the last Account — and a Purge that logged
somebody out of the tool they are using would be doing more than giving the
machine back.

## Verified

`age` 1.3.1 — Filippo Valsorda's Go implementation, the reference one — decrypts
what `perch holdings export` writes, given the passphrase, on a machine that has
never heard of Perch. Checked by hand rather than in CI: the property is what the
crate promises, and installing the Go tool on three runners to re-check it every
push buys less than it costs. What CI holds is that the file carries `age`'s
armor header and that its recipient is scrypt, which is the pair that makes
`age -d` recognize the file and ask for a passphrase.

The other direction holds too: `perch holdings import` restores an armored file
that `age -p` wrote — Registry, Aliases, Group configuration, Quarantine reason
and a Credential into the real keychain — on a machine where nothing Perch wrote
was involved. Both halves of the format are therefore somebody else's, which is
the whole of what taking the crate bought.
