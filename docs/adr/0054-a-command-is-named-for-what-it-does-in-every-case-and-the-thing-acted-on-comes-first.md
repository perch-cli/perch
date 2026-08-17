# A command is named for what it does in every case, and the thing acted on comes first

Two singleton Account verbs were the last of the surface left unexamined. ADR
0047 settled that both belong at the top level with the Account elided, so
placement was never in question — only whether either earns a *command*, and
`alias` was set against `add --alias`, `relogin` against a Repair reached some
other way.

Both stay, and neither pairing survives contact. What the examination is actually
worth is three rules the surface has been following without ever writing down,
and one command that has been breaking the third since the beginning.

## `add --alias` was never the rival

`--alias` fires only at creation. **Nothing else in the repository can change an
Account's name afterwards**: `group rename` is a Group's, and its `CONTEXT.md`
entry rules Accounts out in as many words — "the name an Account answers to is
its Alias".

So deleting `perch alias` does not relocate a capability, it deletes one. An
Alias would be fixed at `add` time, and the only way to change one would be a
`remove` and a re-`add` — which is precisely the "resembles the Account and is
not it" failure ADR 0023 exists to refuse, arrived at from the other direction.

A flag on a creation command and a command acting on an existing Account cannot
be alternatives, because they cannot reach the same states. The ticket's pairing
was a category error, and saying so is the finding.

## The converse, which is the real question

With `perch alias` kept, `add --alias` is a second path to one end state — the
shape ADR 0047 refused when it ruled *one name, one place*.

It is not the same path. At the moment `perch add` returns, **the Account has no
name but the email address you have just learned from the login**, so naming it
afterwards means typing that email: the exact thing an Alias exists to prevent.

> **The shortcut earns its place because the long way costs the very thing the
> feature exists to remove.**

ADR 0047's refusal is about two *spellings of one act*. This is one act composing
two, and the test that separates them is whether the "duplicate" path is
available without paying that cost. Here it is not.

`add --group` stands on identical ground — `group move <target> <group>` needs
the same email — so the two survive or fall together, and cutting both would
leave `perch add` producing an Account reachable only by email until you type its
email once.

## `relogin` is not the glossary's word, and that is correct

There is a pattern in the surface, and it is cleaner than expected. **Every
command whose act the glossary names takes the glossary's word**: `remove` →
Remove, `purge` → Purge, `export` → Export, `import` → Import, `switch` → Switch,
`run` → Run, `upgrade` → Upgrade, `group rename` → Rename, `watcher check` →
Check. The commands that do not take a glossary word — `add`, `list`, `status`,
`disable`/`enable` — are exactly the ones where the glossary names no act: ADR
0053 declined "listing", ADR 0052 declined **Enabled**.

**`relogin` is the single case where a word for the act exists and the command
declines it.** On the pattern alone, `perch repair <target>` is the fix.

The outlier is principled, and ADR 0023's own Consequences say why: **the command
is wider than the act**. **Repair** is defined narrowly — "Logging a *Quarantined*
Account in again in place" — while the command is allowed on a healthy Account
and "behaves identically... a Credential somebody suspects is going wrong should
not have to break first before it can be replaced".

So `perch repair work` on a healthy Account is a false sentence. The only ways to
make it true are widening **Repair**, which the sweep's own terms forbid — the
vocabulary in `CONTEXT.md` is fixed — or narrowing the command to Quarantined
Accounts alone, which deletes a capability to make a name fit.

And the `re-` is load-bearing in its own right. `perch add` is *also* a login
(`login::perform` has exactly two callers, `add.rs:65` and `relogin.rs:57`), so
`perch login` would collide with it conceptually while `relogin` says precisely
what distinguishes them: again, and in place.

### The rule

> **A command takes the glossary's word for its act only where the command and
> the act are the same size. Where the command is wider, it is named for what it
> does in every case rather than for the case that matters most.**

`relogin` is what the command does in every case; a Repair is what it does in the
case that matters. That is the whole of the exception, and stating it turns an
anomaly into an admitted one — which is what stops a future `perch repair` being
admitted by nothing at all.

## The collapse into `add`, refused

`perch add [<target>]` — bare for a new Account, targeted for a Repair — is the
serious rival, and ADR 0053 is precedent *for* it: `perch list [<scope>]` had
just collapsed two commands by making the argument the discriminator.

It fails on preconditions. `add` refuses an Account that already exists;
`relogin` requires one. The union command's behaviour would **invert** on its
optional argument rather than widen, which is the opposite of what `perch list`
does — there the argument narrows one shape, here it would swap two. And all
three of `add`'s flags — `--group`, `--no-group`, `--alias` — are meaningless for
a Repair, so the union would carry three flags that contradict its own argument.

Hanging the Repair off whatever surfaces a Quarantine fails harder. After ADR
0049 nothing surfaces one as an *act*: `status` and `list` are renderings that
ADR 0053 has just given one shape each, and ADR 0052 established that a flag
needs a verb to hang on.

**The strongest argument for keeping the command is one no rival can answer:
Perch hands the user this command by name, in prose, from six places.** The
Quarantine refusal (`registry.rs:159`), both stale-Landing narrations
(`switch.rs:713`, `:738`), the Export warning (`export.rs:217`) and two
`observe.rs` notes all print `perch relogin <target>` as the next step, and
`EXIT_QUARANTINED` exists because "no amount of retrying, enabling or
re-targeting repairs it, and `perch relogin` does". A next step printed in an
error message has to be one typeable word, not a mode of another command.

## `alias` survives as the name

The worry was that it is a noun where the others are verbs. It dissolves twice
over. **It is not unique** — ADR 0053 kept `perch status`, so a noun-shaped name
is not disqualifying on this surface. And **`alias` is a verb in English and the
oldest idiom in command-line naming**, so a person meets it already knowing what
it does.

`perch rename` is refused by the vocabulary rather than by taste: **Rename** is
reserved for Groups, and the two are different acts — a Group's name is its
identity and carries its Overrides, its Accounts and its Cooldown, while an Alias
is optional and detachable, which is why `--unset` exists here and has no
counterpart there. `perch name` loses the idiom and buys nothing.

## The argument order, which is the one thing that changes

Perch's convention is unanimous and has never been written down: **the thing
acted on comes first.** `group rename <from> <to>`, `group move <target>
<group>`, `perch list [<scope>]` after ADR 0053, and every single-argument
command take the subject first and the new value second.

`perch alias <name> <target>` is the sole inversion — the only command in Perch
where the value precedes the subject.

The shell idiom does not defend it. `alias ll='ls -l'` is **one `name=value`
token, not two positional arguments**, so it supplies the word and is silent on
the order; Perch's current form matches the shell no better than the flipped one
would. With the idiom deflated, only Perch's own convention is left.

| Now | Then |
| --- | --- |
| `perch alias <name> <target>` | `perch alias <target> <name>` |
| `perch alias <name> --unset` | `perch alias <target> --unset` |

**The flip is a strict superset.** Because a Target resolves an Alias before an
email (`CONTEXT.md`, **Target**), `perch alias work --unset` keeps working
unchanged for the person who knows the name and starts working for the person who
knows only the email. The sentence `registry.rs:1202` prints — "Free it with
`perch alias {held} --unset` first" — stays literally correct without an edit.

The constraint set is unchanged in both directions: the Target must resolve, the
new name must be free. A transposition typo is caught exactly as often as it is
today.

**`--unset` stays a flag**, and ADR 0052 is only half the reason. The other half
is safety: a bare missing argument would make `perch alias work` silently free a
name, which lets a half-typed command destroy one. A flag marking an absent
argument also makes the destructive reading deliberate.

### The rule

> **The thing a command acts on is its first argument. What it is being given
> comes after.**

It has governed every other command since the beginning and was never recorded,
which is exactly why the one command that broke it did so unnoticed. Writing it
down is worth more than the flip; the flip is what makes writing it down honest.

## The fifth clause

ADR 0047's "Admitting a command later" gains one:

> **5. A command is named for what it does in every case.** Where `CONTEXT.md`
> names the act and the command does no more than that act, the command takes
> that word. Where the command is wider than the named act, it is named for the
> whole of what it does.

This amends ADR 0047 in that one clause and nothing else, in the style ADR 0050
set and ADR 0052 followed. Its decision, its table and its counts are untouched.

## Consequences

**The surface does not move: fifteen names, twenty-seven forms.** No command is
added or removed, no exit code changes, no flag is added or removed, and no
capability moves. One command's two positional arguments swap.

**`CONTEXT.md` gains nothing — the seventh consecutive decline**, and for a third
distinct reason. ADR 0045 through ADR 0050 declined entries for having no idea to
add; ADR 0053 declined "listing" for being the codebase's word rather than a
person's. Here both acts are already named: one by an entry the command
deliberately does not spell (**Repair**), one by an entry that names the thing
rather than the giving of it (**Alias**). A glossary that named every act a
command performs would be a second copy of the surface.

**Nothing else in the glossary moves either.** **Repair**'s citation of `perch
relogin` stays correct, and its Phase-zero clause is already claimed by ADR 0041.
**Alias** describes replacement — "An Account answers to one Alias at a time" —
which the flip does not touch.

**This supersedes nothing and amends ADR 0047 in one clause.** ADR 0023 is
untouched and still governing: the relogin on a healthy Account, which its
Consequences allow, is now the reason the command is named what it is rather than
a footnote to it.

**What was actually bought.** Two rules and one refusal with its reasons. The
naming rule admits `relogin` and would refuse a `perch repair` that meant the
same thing; the argument-order rule was unanimous and unwritten, and is now
neither. Both singletons were re-affirmed rather than churned, which is the
outcome the sweep's own Notes said to expect and the one it is hardest to write
down honestly.
