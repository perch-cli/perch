# A Setting names its Scope

**A Scope — each Group, and the Accounts in no Group taken together — holds its
own full Settings. There is nothing above it. Defaults are compiled-in
constants.**

So every `perch config set` is `<scope> <key> <value>`, every Setting has a
subject, and what nobody has said anything about is the compiled-in default
rather than somebody else's value. Nothing is two layers deep, and an Account
carries nothing at all: every Setting there is describes how Perch chooses
*between* Accounts, and a rule for choosing has nothing to say to a set of one.

## The per-Scope value was never the question

The strongest case for a layer is a work Group wanting a different threshold from
a personal one. That is a case for a per-Scope **value**, and it is silent on the
falling back: a design where each Scope simply holds its own threshold serves the
work Group and the personal one exactly as well, and serves them without a layer.

What a layer adds is a value in one place that a narrower place inherits — and
with it a state distinguishing "this Scope tracks the value above" from "this
Scope holds a value that happens to equal it". The two look identical wherever a
value is shown unless the display says which, so it has to say which, everywhere.
That is the whole of what is bought, and nothing needs it.

## A grant is said about the Scope it grants

`strategy` and `watcher-threshold-percent` are taste. `watcher-may-act` is
consent, and consent does not layer.

A yes said everywhere authorizes **every Scope, including ones that do not exist
yet**: `perch group add` would then yield a Watcher-enabled Group nobody has said
anything about. And it arrives by inheritance at the Accounts in no Group, where a
Group's two grants collapse into one — somebody granting the Watcher broadly means
"Cycle my work Groups unattended", and inheriting that straight through would
authorize moving them off a work Account onto their personal subscription
(ADR a-group-is-a-declaration).

Both are exceptions somebody would have to keep re-deciding, and an exception
written into a uniform layer is one that gets tidied away. Saying a grant only
about the Scope it grants makes the two grants structural instead: the case cannot
be written down.

**Consent is said and never asked.** Where a Scope grows to a second Account, the
two deliberate defaults start to matter and Perch says so — but a yes collected in
the middle of adding an Account is not the yes Perch promises when it says nothing
changes underneath somebody until they say it may. So what is printed beside the
rest of the report is a statement of what is now true and the command that would
change it (ADR perch-says-what-it-did), and the declaration is named before the
grant, because that is the one that has to come first.

**The cost is real and is accepted: no one command withdraws the Watcher
everywhere.** Pausing a held Service is a command per Scope
(ADR the-machine-runs-the-watcher). The grant was never the only brake — the loop
is a foreground process somebody kills and a Service is one they stop — and a
brake that works by blanket inheritance is the wrong brake for consent.

## The shallow end goes

A two-word `perch config set watcher-threshold-percent 70` works knowing nothing
about Scopes, which is progressive disclosure and a genuine property. It is given
up. It is shallow only until the second Group exists, at which point the layer has
to be learned anyway and the short form becomes ambiguous in the reader's head —
*did that change `work`?* — and the refusal that replaces it teaches the one idea
it is refusing over.

**Reading is not writing.** A bare `perch config get` prints every Scope's Config
in full. A read has no subject to be wrong about; a write does. That is the rule
rather than an exception to it.

**An implicit Scope is refused.** A missing Scope could mean the Scope of the
Account somebody is on — the shallow end restored without a layer. Elision works
where the unsaid noun is *always the same one* (ADR a-command-names-its-noun);
here the unsaid subject changes with whichever Account happens to be active, so
the identical command means different things on different days. Tolerable for a
Switch, which says what it did. Not tolerable for a rule that persists after the
sentence has scrolled away.

## `interchangeable` is the Ungrouped Scope's alone

The declaration that the Accounts in no Group may be Cycled among at all is a
Setting like any other, said about the Scope it governs:
`perch config set ungrouped interchangeable true`. A Group carries no such line,
because a Group **is** that declaration.

The name does not repeat its own Scope, which is the defect naming has elsewhere
(ADR a-command-names-its-noun). `may-cycle` was the runner-up — it pairs with
`watcher-may-act` and matches the predicate it already is — and is refused because
it recasts a declaration as a grant. The distinction is the whole reason the two
yeses are two rather than one thing said twice: two grants side by side read as
redundant, and a declaration beside a grant does not.

Printing `work interchangeable true` and then refusing to set it would break the
invariant everything here rests on — every line `perch config get` prints is the
tail of the `perch config set` that would restore it — so the honest form is
silence. One Scope carrying a key the others do not is an asymmetry that sits at
the Scope where it takes effect, rather than at a layer where it would mean
nothing.

## Nothing clears a Setting

There is no `unset`, because there is nothing to clear: a value is set to what it
should be. "Back to the compiled default" was considered and refused, because the
moment the default is a thing a command returns you to, `perch config get` wants
to annotate `(default)` beside values nobody set — and **a value that knows
whether it was set is the layer under another name**, with the tracking restored
and the idea back in the code. The annotation is refused with the command.

The word survives elsewhere: `perch alias <name> --unset` frees a name, which is a
different act.

## `global` is a reserved word

`global` is how people say "every Scope at once", and there is no everywhere. So
it is refused as a Group name and as an Alias, and somebody typing
`perch config set global watcher-may-act true` gets a refusal saying there is no
such Scope.

The reservation is not there because the guard is cheap. A Group name is a
Holding: it is written into the registry, every Setting it carries hangs off it,
and nothing outside the registry can reconstruct it
(ADR the-holdings-outlive-a-perch). Letting the name be taken means a Group that
answers to `global`, quietly absorbing every later `perch config set global …`
while every other Scope stays as it was — a Setting that appeared to take. The
refusal is where somebody learns Perch has no such layer, which is a better place
to learn it than that.

## Consequences

`perch config` keeps three Settings and a fourth on one Scope. Every `set` is
three words, and every line `get` prints is the tail of the `set` that would
restore it. A `set` that names no Scope is refused rather than landing somewhere,
and a key typed where a Scope goes is answered as the missing subject it is
rather than as a mistyped Group.

A Group name therefore cannot hold a space. `get` prints `<scope> <key> <value>`
and `set` reads it back by counting words, so a Group called `my work` would print
a four-word line that `set` cannot take. It is refused at the one moment somebody
can still choose another name.

The registry holds a map of `Settings` per Group and one record for the Accounts
in no Group, which carries their declaration beside their Settings. A Cycle and a
Setting name the Scope with one type, because the hazard that would keep them
apart — a layer within reach of the ranking — cannot occur where there is no
layer.

The registry's `version` moves when that shape moves
(ADR the-holdings-outlive-a-perch). A shape that changes under a version that
does not is the one failure a migration cannot catch.

No exit code changes and no new one is added. The refusals that gain sentences —
`global`, a `set` with no Scope, and `interchangeable` asked of a Group — land on
the register they already use.
