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

What the code does is not on the list. A comment that reads the syntax back is
deleted, not trimmed.

### Strict and straight

State the fact. No rhetoric, no restatement for effect, no sentence whose work is
emphasis. "150s is 24 reads an hour", not "which is the same arithmetic read the
other way".

Perch's vocabulary is not jargon. `CONTEXT.md`'s nouns — Account, Credential,
Headroom, Landing, Quarantine — are the shortest correct way to say the thing.
Use them.

### Three tiers, and each argues once

| | job | cap |
| --- | --- | --- |
| `//!` | what the module is for, and the decisions governing it | 10 lines |
| `///` | the item's contract, and why it has this shape | 5 lines |
| `//` | a local surprise, at the site | 3 lines |

**Argued once, at the widest scope that owns it.** What the header says is not
said again at a site.

**Over the cap is not a long comment — it is a decision with no ADR.** Write the
ADR, cite it, and cut the comment to the fact. Do not reflow to fit.

### A decision is cited once per file

The header cites what governs the module. A site cites only a decision it is
itself the subject of: the constant that decision fixed, the branch it added.
Never both. `src/watch.rs` cites ADR 0013 eleven times; once is the rule.

### Perch's own past is not a comment

Present tense. A comment describes the code as it stands.

A **rejected alternative stays**, stated as a live alternative — "Claude Code
jitters the same wait; Perch takes these locks once per command, so a fixed wait
has nothing to spread out."

**What the code used to do goes**, whatever it was defending. `used to`, `no
longer`, `was never`, `the old`: that is git history's job.

### Tests

A test's name is its claim, because a failure prints the name.
`a_refresh_that_fails_across_a_threshold_crossing_never_switches` needs no doc
comment restating it.

A test carries a comment only where the fixture is surprising — which trace
crosses the threshold, why this Credential is spent — under the 3-line cap.

### This binds comments, not documents

`src/` states facts. `docs/adr/` makes the case. `CONTEXT.md` defines terms. The
ADRs keep their prose deliberately: an argument compressed to bullets stops being
answerable by the next reader, which is what ADR 0043 exists to prevent. Do not
apply this section to `docs/` or `CONTEXT.md`.

### Worked examples

**Keeps as written.** `src/lock.rs`, `WAIT_MILLIS` — three lines, a rejected
alternative, no history:

```rust
/// How long to wait between attempts. Claude Code jitters the same wait; Perch
/// takes these locks once per command rather than in a loop, so a fixed wait
/// has nothing to spread out.
```

**Rephrases.** `src/watch.rs`, `REFRESH_INTERVAL_MILLIS` — eleven lines to four.
ADR 0013 goes because the module header holds it; ADR 0015 stays because the
constant is its subject:

```rust
/// How long the watcher waits between Refreshing the Account it is on.
///
/// 150s is 24 reads an hour. The endpoint allows 28-30 per Account (ADR 0015),
/// so a concurrent `perch status --refresh` still fits. Refreshes one Account:
/// at 24/hour each, a Group of two is already at the limit.
```

**Deletes.** `src/watch.rs`, above `still_holding_line` — narrates what a
decision used to require, then argues with it. Nine lines go; the fact that
survives is that a hold is said once, with its citation:

```rust
/// ADR 0013 had every held round say which failure held it, and a hold whose
/// line said neither that nor when it would ask again "reads as a watcher that
/// has given up". That was written about a person watching a terminal, where the
/// repeated line *is* the proof of life.
```

### CI checks two of these

The line caps and one-citation-per-file are a script. The four kinds and the
history rule are judgment. **Passing the check is not passing the standard.**

## Agent skills

### Issue tracker

Issues and PRDs live as GitHub issues on `perch-cli/perch`, managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
