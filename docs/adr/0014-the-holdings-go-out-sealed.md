# The Holdings go out sealed

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

## Amended: an import adopts nothing, lands nothing, and leaves nothing behind

The amendments above settled that import refuses a non-empty registry and left
three things to whoever wrote the command. All three turned out to be decisions.

**An import adopts nothing.** Every other command reads the registry through
adoption, which takes the existing Claude Code login over the first time Perch
runs (ADR 0009). Doing that here would make the machine hold one account on the
way to refusing itself for holding one — the command would be unable to run on
the machine it is written for. So import reads the registry directly, and the
login that is on the machine is left exactly where it is. Whoever imported can
switch onto it or ignore it; it is not Perch's to take.

**Nothing is made active.** The account that was active where the export was
taken is a fact about *that* machine's default profile, and an import writes
nothing there. Making one active would mean replacing the live credential of
whatever is logged in, which is precisely the thing `perch remove` goes to
lengths not to do. So the restore lands and the user switches.

**Anything that fails part way takes back what it placed.** A machine holding
some of an export is the partial restore this record exists to prevent, arriving
by accident rather than as a feature. Every profile the command touches is
recorded before it is written, so a credential that will not go into a store
takes the directory made for it back out along with every profile before it.
Safe to be that blunt about deleting, and only here: an import runs only on a
machine holding no accounts, so every profile it removes is one it made moments
ago.

The passphrase is prompted once rather than twice. Confirming is what you do to a
passphrase being *chosen*, because a file nobody can open is not discovered until
it is needed; one being *checked* is answered by the file itself a moment later.

## Amended: a purge asks for the word, keeps the live login, and finishes

The amendments above settled that purge offers an export first and left the rest
of the command to whoever wrote it. Four things turned out to be decisions.

**The word rather than a letter.** Every other confirmation in Perch is `[y/N]`,
and `y` is what fingers answer before eyes have read anything. This is the one
command nothing undoes, so the prompt lists the accounts by email address — the
login is what is being given up, and the address is what somebody would check
against their password manager — states that nothing brings them back, and wants
`purge` typed out. `--yes` answers ahead of time, the same idiom `perch remove`
uses, and it answers the export offer too: an export is a path somebody names and
a passphrase somebody types, neither of which a script can be asked for. Without
a terminal and without the flag, purge is refused and names the flag rather than
the terminal — unlike export, whose refusal names the terminal because there is
deliberately nothing that answers a passphrase ahead of time.

**The offered export goes through the same code as `perch export`**, one function
below the command, because purge holds the registry lock across the offer and
could not take it a second time. It carries the path refusals with it: a backup
command that destroys the file it was pointed at is the failure those checks
exist for, and a second caller is exactly how a check like that comes to be made
in one place and not the other. One refusal is purge's alone — a path under
Perch's own home, which the purge that offered it would take moments later.

**Credentials first, the home last.** A credential in the operating system's
keychain lives outside `~/.config/perch`, so the registry naming which items
there are is the only record of them. Removing the home first would strand them
with nothing left to find them by. Taking every credential first and the home
whole afterwards means a purge that failed anywhere leaves the registry standing,
so running it again finds what it already deleted already gone and finishes — and
a home left behind holding no registry, which is what a purge interrupted in its
last step leaves, is taken by the next one rather than reported as nothing to do.

**A live profile refuses a purge**, which is ADR 0005's rule at its extreme: a
purge does not write into those directories, it deletes them. Asked of every
account, because a purge is all or nothing and a refusal discovered half way
through is the partial state the check exists to prevent. Doubt counts as a
client, as it does for the carry: waiting costs a command run again, and not
waiting costs whatever that client had open.

Asked twice — before the questions and again after them — for the same reason
the registry hold is re-checked there, and over the same window: somebody may
have started a client while the passphrase was being typed, and an answer that
was true four prompts ago says nothing about the profile the next line deletes.
The first ask earns its place by not putting five questions to somebody who was
always going to be refused.

What a purge does not touch is the credential in the default profile. That is
Claude Code's own login rather than a copy Perch holds — the same line
`perch remove` draws when it gives up the last account — and a purge that logged
the user out of the tool they are using would be doing more than giving the
machine back.

## Verified

`age` 1.3.1 — Filippo Valsorda's Go implementation, the reference one — decrypts
what `perch export` writes, given the passphrase, on a machine that has never
heard of Perch. Checked by hand rather than in CI: the property is what the crate
promises, and installing the Go tool on three runners to re-check it every push
buys less than it costs. What CI does hold is that the file carries `age`'s armor
header and that its recipient is scrypt, which is the pair that makes `age -d`
recognize the file and ask for a passphrase.

The other direction holds too: `perch import` restores an armored file that `age
-p` wrote — registry, aliases, group configuration, quarantine reason and a
credential into the real keychain — on a machine where nothing Perch wrote was
involved. Both halves of the format are therefore somebody else's, which is the
whole of what taking the crate bought.
