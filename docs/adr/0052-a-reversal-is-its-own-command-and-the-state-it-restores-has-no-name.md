# A reversal is its own command, and the state it restores has no name

> **Carried out in #175.** Like ADR 0041 and ADR 0046 through 0051, this is the
> artifact of a planning effort rather than of a change, so it landed ahead of
> the work it describes instead of beside it. The tree now matches it:
> `Account.enabled` is `disabled` and present-only, `enabled_by_default` is
> gone, the State column empties for an Account in neither state, `--json`
> carries `"disabled"`, ADR 0047 has its fourth clause, and **Disabled**'s
> `_Avoid_` line names **reserved**. The command surface did not move.

ADR 0047 named this as one of the collapses it deliberately did not decide: two
top-level verbs for one boolean. `perch disable <target>` takes an Account out of
Cycling, `perch enable <target>` puts it back, and they share an implementation —
one `EnableCommand` enum, one `run`, 99 lines in `src/commands/enable.rs`, 294 in
`tests/enabling.rs`, and two dispatch arms at `main.rs:476` and `:481`. The guide
has printed them on one row since before either was questioned.

**The pair stays.** What is decided here is why, and the "why" turns out to
govern three things outside the command surface.

## The asymmetry is in the glossary and nowhere else

The case for collapsing rested on the pair being less symmetric than its names
claim: an Account that has never been disabled is not *enabled*, it simply is not
disabled. That reading is right about the domain and wrong about the repository,
and both halves matter.

`CONTEXT.md` has **Disabled** and no **Enabled**. One entry, defined as an
Account "you have taken out of Cycling and kept in every other way… Reversible".
The positive is the absence of the negative, and the model has never said
otherwise.

The code says the opposite twice. `Account.enabled` (`registry.rs:176`) is a bool
written on every Account, always — `#[serde(default = "enabled_by_default")]`
with no `skip_serializing_if`, where its four optional neighbours (`plan`,
`quarantine`, `group`, `utilization`) all omit themselves when absent. Every
registry fixture in the repository carries `"enabled":true`. And `state_of`
(`list.rs:244`) prints the literal word `enabled` in the state column, with
`--json` emitting `"enabled": true` from the one Account document both `perch
status` and `perch list` are served by.

So the disagreement is real, it is three places wide, and it is the glossary that
is right.

## A glossary names states; a command names acts

This is the whole finding, and it is what lets the model stay asymmetric while
the surface stays symmetric without either being a compromise.

`CONTEXT.md` is a list of ideas a person has to hold. **Disabled** is one;
"enabled" is not, because it is what every Account already is and holding it buys
nothing. A command surface is a list of acts a person performs. **Undoing is an
act you perform**, and it stays an act whether or not the state it restores has a
name.

Nothing ever *becomes* enabled except by having been disabled first, which is why
`perch enable` on a fresh Account is a no-op that says "was already enabled" and
exits 0. That is not evidence the command is redundant. It is what the undo of
nothing looks like, and the pair says so deliberately: neither verb ever reaches
exit 15 ("there was nothing to do"), because a script that runs twice has not
done anything wrong. `saying_it_twice_is_not_an_error_and_says_it_was_already_so`
pins it.

## Why not one verb and a flag

The rival shape was one name with the reversal on a flag — the shape `alias`
already has, and the only instance of it left once ADR 0051 deletes `config
unset`.

It was refused on the yardstick, and the arithmetic is against it more plainly
than the count suggests. Two verbs cost **one** idea, not two: a person holds
*Disabled*, and knowing `perch disable` tells them `perch enable` without a
lookup. `perch disable <target> --undo` inverts that — it is discoverable only by
reading the help, which is a lookup the pair does not charge. Under *conceptual
surface first*, a second name that needs no learning beats a first name that
needs a flag looked up. The tiebreak never fires: the maintenance surface is
already shared, one enum and one function, and a flag would not shrink it.

**`perch alias <name> --unset` is not a counter-example**, and the difference is
the argument list. Its two forms take *different* arguments — a name and a
Target, or a name alone — and `--unset` is what makes the Target's absence
deliberate rather than a missing operand. `main.rs:68` says exactly that in
clap's terms (`required_unless_present = "unset"`, `conflicts_with = "unset"`),
and the dispatch at `:460` records that the flag "needs no reading of its own".
`perch disable <target>` and `perch enable <target>` take the same argument in
both directions. A flag there marks nothing absent and carries only the polarity.

> **A flag may mark an argument's absence. It may not carry a verb's polarity.**

## ADR 0047's flag-or-verb test is silent here, not satisfied

Read literally, that test condemns both verbs: neither changes the meaning of the
exit code nor the lifetime of the command, so both are flags. The literal reading
is wrong, and leaving it unaddressed would hand the next reader a rule that
misfires.

The test was written to adjudicate `--once`, a capability that already sat on a
host verb and was being promoted off it. **A flag needs a verb to hang on.**
Here there is no host, so applying the test means first choosing which of the two
becomes the host — which is the question being asked, not an input to it.

The reductio is `perch group add` and `perch group remove`. Neither changes an
exit code's register or a command's lifetime; both are verbs; nobody has ever
proposed `perch group add <name> --undo`. A test that condemns them is being
asked a question it was not built for.

So the test stands unamended and gains a boundary: it adjudicates a capability
*within* a command's scope, and never how many commands one capability is reached
by.

## The fourth clause

ADR 0047's "Admitting a command later" gains one:

> **4. A reversal takes its own name.** Where a command's whole effect is to undo
> another's, it is its own command rather than a flag on the one it undoes. The
> flag-or-verb test does not reach the question.

This amends ADR 0047 in that one clause and nothing else — the precedent is ADR
0050 amending ADR 0007 in a single sentence. Its decision, its table and its
counts are untouched.

## What follows outside the surface

The finding is that the positive state has no name. Three places currently give
it one, and all three follow.

### The registry field

`enabled: bool` becomes `disabled: bool`, present-only — `#[serde(default,
skip_serializing_if)]`, exactly the shape `quarantine` has, and for the reason
`quarantine`'s own doc comment already gives: "the registry is something a person
may open, and a healthy Account reads more clearly for saying nothing about its
health". That argument was written about a different field and applies here word
for word. `enabled_by_default` disappears.

The honest cost: `cycle.rs:731` gains a negation (`!disabled && !quarantined`)
and `cycle.rs:764` loses one. That is a wash in the code, and the gain is
entirely in the file — which is a surface a person reads, and the only one this
change improves.

`deny_unknown_fields` means a registry carrying `enabled` is refused rather than
migrated. `CLAUDE.md` settles that: there are no users, and reading what an older
Perch wrote is not the kind of guard worth keeping.

### The listing column

The state cell empties for the ordinary Account, rendered as the placeholder the
Alias column already uses for nothing-to-say. `disabled`, `quarantined` and
`disabled, quarantined` are the only things it says.

`state_of`'s recorded reason survives intact — "Both are always said, because
they are separate facts with separate fixes" is an argument about **Quarantine**,
and it is unaffected: the two facts are still separately said, and `enabled,
quarantined` simply becomes `quarantined`, which is more legible about the fix
rather than less. What goes is a word printed on every row but one to say that
nothing is reserving the Account.

### The JSON key

`"enabled": true` becomes `"disabled": bool`, and stays present on every Account
unconditionally. ADR 0043's distinction is the one that applies: a machine
reading a shape is not a person reading a sentence, and a script that has to test
for a key's presence to learn a bool has been given a worse contract, not a
truer one. The key follows the field's name; only the human-facing cell empties.

## The word Reserve

Surfaced while resolving this, and cheap enough to fix with it.

`CONTEXT.md:167` defines **Reserve** as "What a Group has left to draw on, said
as how many of its Accounts still have Headroom and how much the best of them
has", with `src/reserve.rs` behind it. Meanwhile `enable.rs`'s module doc opens
"reserving an Account for a purpose without giving it up", the guide's section
heading is "Reserving an Account", and two tests in `tests/enabling.rs` are named
for it. One word, two ideas, and **Disabled**'s `_Avoid_` line — "excluded,
paused, off, archived" — does not catch it in either direction.

**Disabled**'s `_Avoid_` line gains **reserved**. The four prose sites take the
entry's own language instead: *keeping an Account out of Cycling*. No
user-facing string changes, which is why this costs nothing to do here rather
than later.

ADR 0047 excluded *naming* from its scope and this does not reopen it — a command
keeping a name it has is not a naming decision, and a glossary term being used
for two things is a vocabulary collision, which is the one kind of naming every
ticket on this sweep is obliged to look at.

## The glossary

**No entry is added and no definition changes.** **Enabled** was considered and
refused: it would hold no idea beyond "not Disabled", which is precisely the
finding. Amending **Disabled** to name its reverse was refused too — it already
says "Reversible", and spelling out a path would put one in an entry that
deliberately cites none.

The only edit is one word on one `_Avoid_` line.

## Consequences

**The command surface does not move.** Top-level names stay at ADR 0047's
sixteen — fifteen once `perch tui` goes — and invocable forms at twenty-eight,
then twenty-seven. Both verbs keep their names, their placement, their exit codes
and their output.

**This supersedes nothing and amends ADR 0047 in one clause.** ADR 0012
("Cycling skips accounts that are exhausted, disabled, or quarantined") and ADR
0024 ("never being chosen for you is the whole of what disabled means") use the
word as a lowercase state and are untouched. ADR 0002's rejected alternative —
"dropping groups entirely and using a per-account disable flag was seriously
considered" — is the same trade seen from the other side and stays rejected; a
Group declares a set interchangeable, a disable reserves one Account, and
collapsing them would make reserving an Account require inventing a Group for it.

**What was actually bought.** Nothing on the surface, and one rule: that a
capability's reversal is not a flag, with the boundary on ADR 0047's flag-or-verb
test that makes the rule safe to apply. The three spellings that follow are not
the point of the decision — they are what a model and a repository agreeing looks
like once one of them has been declared right.
