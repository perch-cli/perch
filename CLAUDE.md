Always load the `unslop` skill if it is present.

## The Holdings survive an upgrade

The CLI surface moves freely: commands, flags, output prose, exit codes and the
shape of `--json`. Rename it, move it, cut it, and mark the entry
`[**breaking**]` in `CHANGELOG.md`. Exit codes and `--json` are read by a script
rather than by a person, so that entry is the only warning there will be.

The Holdings do not. A Profile, a Credential, the registry naming them and an
Export carrying all three are what a changelog entry cannot give back, so a
change to any of their shapes lands as a migration or as a
refusal-with-instructions — never as a file this Perch reads wrong. A Credential
Store is derived from its Profile's path, so moving `profiles/` is not a rename.

Which of the two is decided by what the refusal costs. An Export is a backup and
the Perch that wrote it can still open it, so an Export this build does not
understand is refused, naming the version that wrote it. A registry holds Groups,
Aliases and Settings that exist nowhere else, and starting over means logging in
to every Account — so a registry migrates forward from every version Perch has
written.

Both turn on one number: when the shape of the registry or of an Export moves,
its `version` moves with it. A shape that changes under a version that does not
is the one failure neither the migration nor the refusal can catch.

The case, and what 1.0 leaves open: ADR the-holdings-outlive-a-perch.

## Perch writes American English

Every word Perch writes itself — prose, comments, identifiers, output — is
American English: `behavior`, `license`, `color`, `recognize`, `serialize`.

Not a taste. serde puts `Serialize` and `Deserialize` in the tree, HTTP puts
`Authorization`, Anthropic puts `organization_uuid` and `utilization`. Every
American spelling Perch had before this rule was one it was forced into from
outside, and a second register beside them collides at the boundary. Vocabulary
that comes from outside is still quoted as its owner spells it, whichever
register that is.

`typos.toml` is the rule and CI enforces it. Three commands, because `typos`
skips any file its own config could live in:

```
typos
typos - < Cargo.toml
typos - < typos.toml
```

## A comment earns its place

Every comment in `src/` and `tests/` says one of four things:

1. **The road not taken.** Why this and not the alternative a reader reaches for.
2. **The invariant.** What this upholds, or what breaks if it moves.
3. **The gloss.** What this is in Perch's vocabulary, where the identifier cannot
   carry it. `EXIT_OK: i32 = 0` needs one; `0` is a convention, not a meaning.
4. **The citation.** Which decision in `docs/adr/` settled this.

*What the code does* is not on the list, and is deleted rather than trimmed.

State the fact. No rhetoric, no restatement for effect, no sentence whose work is
emphasis. `CONTEXT.md`'s nouns — Account, Credential, Headroom — are vocabulary
rather than jargon, and the shortest correct phrasing available.

| | job | cap |
| --- | --- | --- |
| `//!` | what the module is for, and the decisions governing it | 10 lines |
| `///` | the item's contract, and why it has this shape | 5 lines |
| `//` | a local surprise, at the site | 3 lines |

Argued once, at the widest scope that owns it: what the header says is not said
again at a site. **Over the cap is not a long comment — it is a decision with no
ADR.** Write the ADR, cite it, cut the comment to the fact. Do not reflow to fit.

A decision is cited once per file. Present tense only: a rejected alternative
stays, stated as a live alternative, and what the code *used to do* goes. A
test's name is its claim, so a test carries a comment only where the fixture
surprises.

This binds comments, not documents. `src/` states facts, `docs/adr/` makes the
case, `CONTEXT.md` defines terms — a deliberate split, not an oversight.

Worked examples and the reasoning behind each clause: `docs/agents/comments.md`.

## A citation names a slug

A decision is cited as `ADR <slug>`: `(ADR perch-does-not-draw)`. The slug is
the document's H1 kebab-cased, and is the tail of its filename in `docs/adr/` —
at most 30 characters, always hyphenated, never one word. The numeric prefix is a
sort key: it appears in no citation, and the document is found by globbing
`docs/adr/*-<slug>.md`. Band `N` occupies `N01` through `N99`, the bands running
in `CONTEXT.md`'s section order, so a new document appends inside its band.

One form everywhere — Rust, Markdown, TOML, YAML, TypeScript — and never a path
or a link, because the slug is the identity and a path names the number instead.
A citation names a document, never a section.

A citation addresses an agent reading the tree, so two places never carry one:
the guide under `pages/src/content/docs/`, and anything Perch says to a person.

Worked examples and the reasoning: `docs/agents/comments.md`.

## Agent skills

### Issue tracker

Issues and PRDs live as GitHub issues on `perch-cli/perch`, managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
