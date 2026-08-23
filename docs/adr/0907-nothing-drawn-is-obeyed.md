# Nothing drawn is obeyed

**Text on its way to a terminal goes through `host::Shown`, which takes out
whatever a terminal acts on rather than draws. The writers that lay out a column
— `utilization::cells`, `padded` and `write_labeled` — take one instead of a
`&str`, so a surface cannot render a value without having asked.**

## What went wrong

`is_unshowable` has been in the tree since the review that added it, and says the
harm plainly: `U+202E` reverses the rest of the line it lands in, and a
zero-width character hides the whole difference between two names. Three values
were held to it, each by its own copy of the same four lines — a Group or Alias
name in `validate_name`, an Account's address in `registry::validate`, an
organization in `probe::read_identity`.

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

The three values already guarded are refused, and that is right: a person chose
them, and a refusal is a thing they can go and act on.

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

## What it does not buy

Two organizations differing only in a formatting character draw as one name.
That is the *"hides the whole difference between two names"* harm, surviving the
fix — and it is survivable exactly where these three values sit, because none of
them is a Target anybody types. Where a name is an identifier it is still
refused at the moment it is chosen, and that is the half of the rule this does
not replace.

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

**Escaping rather than stripping, as `\u{202E}`.** More honest, and it widens a
column by six cells per character to say something about a value nobody can
correct. Perch has no escape vocabulary and this is not the place to start one.

**Filtering the writer the commands are handed.** It would cover every surface at
once, including ones nobody has written. It would also cover `--json`, where
`serde_json` has already escaped the character correctly and a second pass would
corrupt a document a script is parsing.

## Consequences

- A new column in `perch list`, or a new labeled row in `perch status`, does not
  compile until its value is a `Shown`.
- `window_width_across` measures the stripped form, so a name's width and the
  bytes written for it cannot disagree.
- `--json` is untouched: `serde_json` escapes a control character as six literal
  characters, which is what a parser wants and what a terminal draws.
- The three refusals in `validate_name`, `registry::validate` and
  `probe::read_identity` stay. They are about names somebody chose, and they
  refuse rather than strip.

## The glossary

No new term. **Shown** is the type's name and not a noun of the domain: what it
holds is a value the vocabulary already has a word for, on its way to a terminal.
