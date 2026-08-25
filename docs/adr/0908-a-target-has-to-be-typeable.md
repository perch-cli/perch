# A target has to be typeable

**A Group name and an Alias are held to an allow-list. The first character is
Unicode's `XID_Start`, `_`, or an ASCII digit; every later one is `XID_Continue`
or `-`. `host::unshowable_character_in` sits on top of it, because XID does not
answer that question.**

## What a deny-list accepted

`validate_name` was a list of what a name may not be: empty, whitespace,
whatever `unshowable_character_in` finds, the three reserved words, an `@`, and a
leading `-`. Everything else was a name. `🚀`, `dev★`, `dev.ops`, `dev/qa` and
`dev+1` were all Groups Perch would declare.

A Target is typed at a shell prompt, and Perch offers no other way in:
`perch switch`, `perch run`, `perch config set` and `perch group move` all take
the name. Often it is typed on the second machine, months after the first one
named it, off a `perch list` somebody is reading over SSH. A name of symbols is
one somebody has to produce from a keyboard before any command can reach it.

The deny-list also grew one clause per harm found. `@` for the address, the
leading `-` for `perch run`'s program, whitespace for `perch config get`'s round
trip, the unshowable set twice. Each was right and each arrived after the rule
had shipped. The allow-list is the same rule stated from the other end, and four
of those clauses fall out of it rather than being remembered: no `@`, no space
and no escape is `XID_Continue`, and no `-` is `XID_Start`.

## Every alphabet, and not ASCII

`XID_Start` and `XID_Continue` are what Unicode says an identifier is made of in
whatever script it is written in, so `café`, `日本`, `дом`, `한국`, `العربية` and
`日本-dev` are all names. This is not an ASCII rule and would be a poor one: the
person naming a Group is naming it for themselves, and a rule that let them name
it in English only would be Perch deciding which keyboard they have.

`-` is added to `XID_Continue` because it is the separator chosen names already
use, and the one `offerable_name` writes when it turns an organization into a
Group name.

## The ASCII digit

`XID_Start` refuses a digit, an identifier opening with one being a number to a
compiler. Nothing here reads a Target as a number and `2fa` is a name people give
things, so a digit opens a name.

An ASCII one only. The carve-out is for the case that comes up, and `XID_Start`
is where the rule comes from rather than something to reopen per script.

## What XID does not answer

`dev\u{FE00}` and `dev\u{3164}` are both well-formed identifiers, and both draw
as `dev`. That is the collision registry version 3 exists for
(ADR nothing-drawn-is-obeyed), so `unshowable_character_in` is asked ahead of the
allow-list: the allow-list would let both through, and it is the terminal rather
than Unicode that decides this one.

Asked ahead rather than after, so a name carrying an escape is refused in words
about what a terminal does with it rather than in words about identifiers.

## Confusables are left open

`dеv` with a Cyrillic `е` is accepted, and draws as `dev` beside a `dev` this
build also accepts. The allow-list does not close that.

Closing it means UTS #39: `Identifier_Status`, and mixed-script detection to tell
`dеv` from `dev`. The implementation is `unicode-security`, which costs about
38 KB in the binary and three more crates in the graph. It would also refuse
`日本-dev`, Han and Latin in one name being exactly what mixed-script detection is
for, and that is a name the section above says is a name.

So the gap is taken deliberately, and it is narrower than it looks: two Groups
that draw alike are two names one person chose for their own machine, and
`perch list` shows both rows. Reopening this needs a case where somebody else
chooses the name, and nobody else does.

## The registry moves for it

A name rule that refuses what this build itself wrote down is a refusal at
`load`, which is every command including `perch remove`
(ADR a-registry-comes-forward). So `CURRENT_VERSION` is 4. No step is added:
`forward` already lands every version on the rename pass, and what moves is
`CARRIED_TO` and which version's rules that pass is held to.

`acceptable` repairs by filtering the name to the characters the allow-list
carries, then dropping leading characters until one may open a name, then falling
back to `group` or `alias` where nothing is left. `dev★` becomes `dev`, `🚀`
becomes `group`, `-dev` becomes `dev`, and the `-N` suffix settles a collision as
it already did.

What the pass carries is bounded by history, as the two versions before it are:
a name version 3 accepted. `a_version_3_perch_accepted` writes out the clauses
that moved rather than reading them off `validate_name`, which no longer holds
them; the clauses that did not move it still asks of this build, because a rule
moving one of those is a rule owing a version of its own.

Version 1's bound is written out for the first time here, and it narrows. It
returned `true` for every name, which was survivable only because `acceptable`
answered `None` for the names version 1 itself refused. The repair no longer
does: it strips an `@` and a space rather than giving up on them. So a version 1
registry hand-edited to name a Group `a@b` or `one two` is named at `load` now
where it was silently renamed before, which is what every other version already
did with a hand edit.

Version 3 shipped in no release, and the number still moves. Both name rules
land in one version of Perch, so anyone coming from v0.2.0 goes from 1 to 4 and
meets version 3 only as a step. Amending 3 in place instead would put two rule
sets under one number, and a registry written by a build off `main` would be
read under the rules of the other one. That is the failure a version exists to
catch, and it is the one neither the migration nor the refusal can.

## What is rejected

**An address held to the same rule.** `probe::read_identity` keeps the whole-set
check and gains no allow-list. An address is given by the login rather than typed,
and an Alias exists so that nobody has to type one.

**NFC normalization during the repair.** It was on the table for folding `café`
into something the allow-list would take, and became pointless the moment `café`
is accepted outright.

**A deny-list with `.`, `/` and `+` added to it.** That is the next three clauses
of the rule this document replaces, found the same way the last four were.

**Mixed-script and confusable detection**, above.

## Consequences

- `perch group add` and `perch alias` refuse a name of symbols, naming the
  character as it draws and as it is spelled. A space quoted alone says nothing,
  and the punctuation that draws alike is many characters.
- `validate_name` loses three branches and gains two. `XID_Start` refuses a
  leading `-`, and neither `@` nor whitespace is `XID_Continue`, so the three
  that go were dead the moment the allow-list arrived.
- `offerable_name` offers less. An organization whose name carries a `.` or a `,`
  now produces no offer at all rather than a Group name spelled with one, and
  `perch add` asks the question with no default in front of it.
- A machine whose registry holds a now-refused name comes forward with it
  renamed and a note saying which, and every command works.
- `unicode-ident` is a dependency (ADR a-crate-must-not-cost-a-seam). It sits on
  neither seam, is already compiled on every build through `clap_derive`, and
  depends on nothing.

## The glossary

No new term. What a name may be made of is a rule about **Alias** and **Group**,
both of which `CONTEXT.md` already defines.
