# A refusal is a promise

**Perch carries two error idioms on purpose. An expected failure is a `PerchError`
carrying the exit code a script reads. An unexpected one is a panic, through the hook in
`report.rs`. Anything that starts as a panic and turns out to be an outcome a user can act
on moves across.**

Exit codes are part of the interface: a shell prompt or a script needs to tell "this
account is gone" from "the keychain is locked" from "Perch does not recognize this Claude
Code" without parsing prose. `PerchError` carries one per variant and the compiler checks
that every variant has one. Routing those through a type-erased report would trade that
check for a downcast, and a colored backtrace is the wrong thing to print at a command
people put in a shell prompt.

A panic is a different animal: it is a bug rather than an outcome, and a bug deserves a
report worth pasting. What that report needs is the version, the platform, where to send
it, and how to get a backtrace when the first run did not carry one. The runtime's own hook
already prints the payload, the location and the backtrace, so Perch's sits on top of it
and adds those four things in a dozen lines. A span-formatted backtrace from a dedicated
crate is genuinely nicer to read, and it is not nicer than not shipping four crates for it.

`RUST_BACKTRACE=0` is how the runtime is told *not* to print one, so the variable's mere
presence is not the question: read as "already asked for", it withholds the suggestion from
exactly the person who has no backtrace to send.

The same sentence about what a bug is and where to send it is shared with
`registry::save`'s refusal to write a Registry no later command could read — not a panic,
but a bug all the same, and one the person in front of it can do nothing about.

## Held is a promise about the machine

`PerchError::Busy` carries `EXIT_HELD`, and the guide states what that means to whoever
reads it: *a lock somebody else has… Nothing is wrong and nothing was changed — ask again
shortly.*

That is not decoration. The Watcher's loop branches on it. Every other failure ends the
loop; a `Busy` is counted against the back-off, said out loud with when the Watcher will
try again, and gone round again. The loop keeps running because the code told it the
machine is exactly as it was.

So **`Busy` is a promise about the machine rather than a description of the failure**, and
the two come apart in one place: a hold lost *after* something has been written. Another
`perch` waited the artifact out and took it over while this one was working, so this one
holds a Registry behind the one on disk, and `registry::save` refuses — correctly, because
writing it back would revert whatever the other command did. Read as a lock problem that is
the most retryable failure there is. Read as a promise it is false, because by then the
Credential has moved.

### The one that matters

Recording which Account is active happens after a Switch has landed. Its failure is the
case: the incoming Credential is in the Default Profile, Claude Code's own file names the
incoming Account, and Perch's record still names the outgoing one. Nothing about that is
untouched, and nothing about it is fixed by waiting. A watcher that carried on watching
would be deciding what to do next about a machine nobody has looked at yet, and a `Busy`
is what would make it carry on.

Being exact about the cost matters, because the tempting argument overstates it. A
continued loop would **not** destroy a Credential: a Capture reads the Identity beside the
live Credential before it files anything, and a Credential whose Identity names somebody
other than the Account the Registry calls outgoing is declined rather than written into the
wrong Profile — a guard put there for this exact state
(ADR a-switch-is-written-down-first). What a continued loop costs is smaller and still not
acceptable: every round after it reads and ranks the wrong Account, because the Registry
names one Account and the machine is acting as another, and it does so unattended and
indefinitely.

### What follows

> **A refusal earns `Busy` when nothing has been written yet, and only then.**

Losing a contended lock qualifies: nothing has started. `commands::still_ours` qualifies by
construction — it is asked before the first irreversible thing, which is its whole reason
for existing (ADR one-door-to-the-registry). A save that finds the hold lost qualifies
where the save *is* the change, and does not where a Credential moved first.

Where it does not qualify, the general failure code is the right answer and the loop
stopping is the right behavior. A person has to look.

`perch watcher check` inherits this. A scheduler reading `EXIT_HELD` comes back in five
minutes and expects the machine to have moved on; reading a general failure it mails
somebody. Those are the two things a Check can say, and a Switch that half happened is the
second.

## Three sentences about a lost hold stay three

Losing a contended lock, a question that waited too long, and a refusal to revert another
command each say something different about what was and was not done, and the exit code
each earns depends on that rather than on all three being about a lock. Routing them through
one variant removes three hand-written sentences and, with them, the distinction the loop is
branching on. It has been proposed once on exactly that reasoning, which is why it is
written down here.

## What a failure carries besides its code

**A note is added to what failed rather than said instead of it, and never changes the exit
code the failure earned.** A step that fails part way through a sequence has to say what
happened *and* what the machine is holding now, and those are two different pieces of
knowledge: the failure belongs to whatever failed, and what it left belongs to whatever was
running the sequence. The variants that carry structure rather than a sentence already exit
as a general failure, so folding one of them loses the shape and nothing a caller could act
on.

Where a note matters most is `Busy`: a lock somebody else is holding is the one failure that
resolves on its own, and a `remove` or a `relogin` noting what it left behind must not cost
the scheduler reading the code the fact that retrying works.

**A Quarantine's reason travels beside its sentence rather than only inside it.** The command
that discovers a Quarantine is the one that has to record it, and a reason inferred back out
of the message is a reason that goes wrong the day a second kind of Quarantine is raised
nearby.

**A file Perch could not make sense of does not say which kind of nonsense it was.** Every
way serde declines a perfectly well-formed document — an unknown variant, a missing field, a
number out of range — reads as a syntax error if the sentence claims one, and sends somebody
looking past the half of it that says what is actually wrong.

**A document written by a newer Perch is refused in one sentence, naming what it was, how far
ahead it is, and that upgrading is the way through.** The two formats Perch versions owe the
reader the same three things, so it is said once for both. A Registry migrates forward and an
Export is refused, and which of the two a format gets turns on what the refusal costs
(ADR the-holdings-outlive-a-perch); this is the wording the refusal uses. The version question
is asked on its own, ahead of parsing, because a newer Perch is exactly what writes a value
this build has no variant for — and parsed first, that fails in serde's words about a document
that is perfectly well-formed, with nothing in the sentence saying the build in front of the
reader is simply too old.

## Consequences

Every variant has an exit code, no two codes mean the same thing, and the arms that map them
are spelled out rather than caught by a wildcard: a variant added later would otherwise be
folded into a general failure silently, and nothing would notice. A few extra arms are the
price of the compiler asking.

`report.rs` holds the panic hook and nothing else. The exit codes are `error.rs`'s, the
sentences are each command's, and the rule connecting them is here.

The Watcher's loop is what makes this a contract rather than a convention. Anything that
changes which refusals earn `EXIT_HELD` changes what the loop does unattended, which is why
the boundary is stated as a rule rather than left to each call site.
