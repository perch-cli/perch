## Nobody is using this yet

Perch has no installed base — not the author, not anyone else. Nothing on any
machine has to keep working.

So breaking changes are free, and are the preferred answer. Do not write
migration code, compatibility shims, deprecation periods, format upgraders, or
"a registry an older Perch wrote" fallbacks. Change the format, change the
paths, change the flags, and update the tests and docs to match. If a change
would need a migration to be safe for users, it does not — there are none.

Forward-looking guards that cost nothing are still worth it: refusing a registry
written by a *newer* Perch, for instance, is about the future rather than the
past. Reading what an older Perch wrote is not.

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

## Agent skills

### Issue tracker

Issues and PRDs live as GitHub issues on `perch-cli/perch`, managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
