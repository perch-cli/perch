# Nothing drawn is obeyed

**Text on its way to a terminal goes through `host::Shown`, which takes out
whatever a terminal acts on rather than draws. The writers that lay out a column
— `utilization::cells`, `padded` and `write_labeled` — take one instead of a
`&str`, so a surface cannot render a value without having asked. The three that
write a sentence — `commands::say`, `Terminal::note` and the refusal `main`
prints — put what they are handed through `Shown::in_prose` themselves.**

## What went wrong

`is_unshowable` has been in the tree since the review that added it, and says the
harm plainly: `U+202E` reverses the rest of the line it lands in, and a
zero-width character hides the whole difference between two names. Three values
were held to it, each by its own copy of the same four lines — a Group or Alias
name in `validate_name`, an Account's address in `registry::validate`, an
organization in `probe::read_identity`. The second of those is a refusal in the
wrong place, and *What is refused and where* below is what it costs.

Three more reached `perch list` and `perch status` with nothing asked at all:

| value | where it comes from |
| --- | --- |
| a Quota Window's name | the usage endpoint, by way of `anthropic::windows_in` |
| an Account's plan | `subscriptionType`, out of a Credential file |
| an organization name | the registry, which `read_identity` does not guard |

The third is the one that says the shape rather than the oversight. The rule
*was* enforced on that value — at the boundary where Claude Code's `.claude.json`
is read, and nowhere on the registry path, so an Import or a hand edit walks past
it. `registry::validate` is public *because* an Import writes a registry without
reading one and what it accepts must not differ; a guard on one of the two routes
in is a guard on neither.

A `window` of `5-hour\u{1b}[2K\u{1b}[31mALL QUOTA GONE` is accepted by `save` and
by `load`, and both surfaces write it exactly as they hold it.

## What is refused and where

A value nobody chose is refused where it enters and never where it is read back.
An address comes out of `oauthAccount` beside the organization, so both are
refused in `probe::read_identity`; neither is refused in `registry::validate`.

The rule the address broke is the one stated two sections down about the other
three: a refusal in `validate` is met at `load`, and `load` is every command.
A registry v0.2.0 wrote holding `wo\u{1b}rk@example.com` — an address that build
accepted, its whole rule being one alphanumeric and an `@` — answers every
command with a refusal naming the file to edit, and `perch remove`, which is the
only way such an Account could ever go, is one of them. `migration::forward`
cannot carry it: an address is what the Profile directory, the keychain
namespace and every Alias are keyed on, so there is no rename to make.

What it buys is narrower than the other three: an address a terminal would obey
is still one nobody can type as a Target. It has an Alias, `perch list` draws it
stripped, and a machine with one has a working `perch`.

## Why a type and not a fourth copy of the rule

ADR an-invariant-gets-a-door: an invariant with more than one call site is
enforced by a type or by a lint, and the sites reach it through one door. Three
missed sites is not three patches.

The door is at the *writer* rather than at the value, and that is the whole of
the design. `cells` and `padded` are already the one place in Perch that knows a
terminal draws in cells rather than characters — `unicode-width` is linked for
them and for nothing else. A column that cannot be measured or padded without a
`Shown` is a column that cannot be added without the question being asked, and
the compiler names every site that has not.

## It strips; it does not refuse

A Group name and an Alias are refused, and that is right: a person chose them,
and a refusal is a thing they can go and act on.

These three are not chosen by anybody. A Quota Window's name is Anthropic's, a
plan is Claude Code's, and an organization is the login's. Refusing one in
`registry::validate` refuses at `load`, which takes every command with it —
including the ones that would repair it — over a value Perch itself wrote down.
That is the failure ADR a-registry-comes-forward exists to prevent, reached
through the fix for this one.

So the rule moves to where the harm is. A refusal there is impossible by
construction: `Shown::of` has nothing to fail at, and takes any `&str` on purpose
— a constructor that could refuse would put the decision back in the caller's
hands, which is what this exists to take away.

## The column is not the only writer

`cells`, `padded` and `write_labeled` cover a table and a labeled block, and
nothing else Perch writes is either. A sentence is a `format!`, and there is no
column to hang the question on:

| what it says | the value nobody chose |
| --- | --- |
| `adopt::report` | `subscriptionType`, out of a Credential file |
| `adopt::report` | the leading token of `claude --version` |
| every `probe::refusal` | the same version, quoted back |
| `anthropic::drifted`, `went_missing` | a key out of the usage reply |
| `perch watcher status` | a path read out of the installed unit |

The first is the one that says the shape. `perch status` draws that plan
stripped and the remark adoption writes says it raw, on the first command
anybody runs, to a terminal — one value, two surfaces, one of them asked.

So the rule moves to the writer again, one level out: the three that write a
sentence ask for themselves. `Shown::in_prose` is the same strip keeping the
newline, which is Perch's own rather than something a terminal acts on — a cell
holds one line by construction and a refusal is several, so the two constructors
are a difference between writers rather than a question put to a caller.

## The set is Unicode's rather than a list

The set was picked by hand, and it grew holes. `U+FE00` after a letter that has
no variant form draws as nothing; so do `U+3164`, `U+FFA0` and `U+034F`; and
`U+2065` sat in the gap between `2060..2064` and `2066..206F`, two halves of one
range written as two.

Constructed: `perch group add dev`, then `perch group add dev\u{FE00}`. Both are
accepted, `perch group list` draws two rows both reading `dev`, and Perch's own
confirmation line says *"Declared the Group `dev`"* twice. Which one
`perch switch dev` finds is not decided by anything — the sentence `validate`
already uses to refuse two names that differ only in case.

So the set is Unicode's `Default_Ignorable_Code_Point`, taken whole. That
property is this question already answered, and a subset chosen for it is a
second definition of the same rule — which is how the holes arose. The cost is
that `perch group add \u{2764}\u{FE0F}` is now refused, naming the character;
`\u{2764}` is not.

Refusing a name this build itself accepted and wrote down is a rule joining
`validate_name` with nothing to carry what is already on disk, which is the
refusal `load` makes every command. The registry moves to version 3 for it, and
the step renames — but only what version 2 accepted. A name no Perch of that
version ever wrote is a hand edit, and is still named at `load` and left.

## What a boundary refuses, once the set is Unicode's

Refusing at entry was decided when the set was `Cc` and a handful of formatting
characters — every one of which a line of `curl` config or a `security` command
also cannot hold. Taking the whole `Default_Ignorable_Code_Point` set broke that
coincidence: `U+FE0F` is the emoji presentation selector, so `"organizationName":
"Acme \u2600\ufe0f"` is a name Anthropic may hold and a line holds perfectly well.

Refused at `probe::read_identity`, that value ended Adoption. `perch status` on a
first run exited `EXIT_PROBE_REFUSED` and `perch add` could not complete, over a
value nobody can change from Perch — the failure this document exists to prevent,
one boundary further out than where it was looked for.

So the two questions are two predicates. `host::control_character_in` is `Cc`
and answers for framing; `host::unshowable_character_in` is the whole set and
answers for drawing.

Which one a boundary asks turns on whether the value is ever *typed*. An address
is a Target — `perch switch someone@example.com` — so two that draw alike have
no single answer, and the section below's argument that this harm is survivable
rests on none of these values being a Target. `registry::validate` refuses
neither address nor organization on the stated grounds that `probe::read_identity`
refuses both, so an address narrowed there is one nothing refuses at all. It gets
the whole set. An organization is drawn and never typed, and is Anthropic's to
spell rather than anybody's to change, so it gets `Cc`.

`validate_name` gets the whole set for the plainer reason: its value is chosen at
a prompt and can be chosen again.

## What it does not buy

Two organizations differing only in a formatting character draw as one name.
That is the *"hides the whole difference between two names"* harm, surviving the
fix — and it is survivable exactly where these three values sit, because none of
them is a Target anybody types. Where a name is an identifier it is still
refused at the moment it is chosen, and that is the half of the rule this does
not replace.

## What the strip takes, once the set is Unicode's

The set answers *may this name be chosen*. It does not answer *what does a
terminal draw*, and `Shown` was given it only because the two questions shared a
predicate. Some of its members have no effect on themselves at all: a variation
selector, the zero width joiner, a conjoining jamo filler and a Khmer inherent
vowel each decide how the character beside them is drawn.

The section above is the same argument arriving at the writer. `U+FE0F` is a
name Anthropic may hold, and `perch status` drew `Acme \u2600\ufe0f` as
`Acme \u2600`. A ZWJ family sequence drew as three people, and `cells` measured
the six columns of the stripped form where a terminal draws two. Drawing a value
as something its owner does not spell is the harm `Shown` exists to prevent,
reached from the other side.

So `host::composes_with_its_neighbor` is a third predicate carved out of
`is_unshowable`, and `Shown` keeps what it names: the variation selectors and
their supplement, the zero width joiner, `U+034F`, the Mongolian free variation
selectors, the Khmer inherent vowels and the two conjoining jamo fillers.

`U+202E` is the case the rule has to survive. Its effect is on the characters
beside it too, and it is the harm this document opens with. The line is
composition rather than proximity: `U+202E` rearranges what is beside it and
`U+200C` breaks a ligature apart, while a variation selector composes into one
glyph. The standalone Hangul fillers `U+3164` and `U+FFA0` are letters with no
glyph rather than slots in a syllable block, and go with them.

The strip moving is not a name rule moving. Nothing on disk is keyed on what a
terminal draws, `validate_name` and `probe::read_identity` still ask the whole
set, and no registry version moves.

## What is rejected

**A checked `Deserialize`, so nothing unshowable enters the registry.** It is a
refusal at `load` wearing another hat, and lands on the values with no author.

**Three more `control_character_in` blocks in `registry::validate`.** The fourth
copy of a paragraph already written three times, and it leaves the fifth value to
a reader.

**Two constructors, one for Perch's own words and one for everything else.** A
`Shown::authored(&'static str)` would carry a real guarantee, and it would also
be a decision at every site — *is this authored?* — which is the reader's
judgment a door exists to remove. One constructor has nothing to get wrong, and
is a no-op on everything Perch writes itself.

**Reading the composing set out of a crate's Unicode tables.**
`Variation_Selector`, `Grapheme_Extend` and a `Hangul_Syllable_Type` of
`Leading_Jamo` or `Vowel_Jamo` name most of it, and none of them names the
zero width joiner. The set it is
carved out of is hand-written, so a second data source is a second definition
that drifts from the first, which is how the last set grew holes.

**Escaping rather than stripping, as `\u{202E}`.** More honest, and it widens a
column by six cells per character to say something about a value nobody can
correct. Perch has no escape vocabulary and this is not the place to start one.

**Filtering the `Write` the commands are handed.** It would cover `--json`,
where `serde_json` has already escaped the character correctly and a second pass
would take those six characters out of a document a script is parsing. Filtering
`say` is not that: `say_json` writes its own line, and the split is one line of
code in one file rather than a rule about which sink a caller reached for.

## Consequences

- A new column in `perch list`, or a new labeled row in `perch status`, does not
  compile until its value is a `Shown`.
- A new sentence needs nothing: `say`, `note` and the refusal printer ask for it.
  A new *writer* is the thing to watch, and there are three.
- `window_width_across` measures the stripped form, so a name's width and the
  bytes written for it cannot disagree.
- The strip and the refusal part company at the carve-out and nowhere else:
  `perch group add` refuses a name carrying `U+FE0F`, and `perch status` draws
  an organization carrying one whole.
- `--json` is untouched: `serde_json` escapes a control character as six literal
  characters, which is what a parser wants and what a terminal draws.
- `validate_name` refuses on the whole set, because a Group name and an Alias
  are chosen at a prompt and can be chosen again. `probe::read_identity` refuses
  the address on the whole set, because an address is a Target, and the
  organization on `Cc` alone, because it is drawn and never typed.
  `registry::validate` refuses neither.
- The unshowable set moving is a name rule moving, so it moves the registry
  version with it and owes a step. `migration::forward` chains from whichever
  version a document claims, and every step lands on the rename pass.

## The glossary

No new term. **Shown** is the type's name and not a noun of the domain: what it
holds is a value the vocabulary already has a word for, on its way to a terminal.
