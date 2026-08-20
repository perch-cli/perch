# ADR inventory — what each of the 63 decides, and whether it still holds

Evidence for [The target ADR set: what survives, what merges, what dies](https://github.com/perch-cli/perch/issues/246).
Produced by [Inventory all 63 ADRs](https://github.com/perch-cli/perch/issues/243).

**This document decides nothing.** Every ADR was read in full and checked against
the code that implements it, by eight readers working one area each. Where a
reader judged rather than observed, the judgment is labeled. Where two readers
disagreed, both readings are kept.

Each of the 63 sections records six things: the decision in one sentence; whether
it still holds, cited to the file that confirms or contradicts it; its supersession
edges and whether each is whole or partial; reasoning that outlives the document;
candidate merge partners; and citation counts with a judgment of whether they are
load-bearing or decorative.

---

## The five findings that should change how the reset is planned

### 1. A supersession grep is wrong in both directions

Nine ADRs carry a banner that reads as "superseded in full." **Six actually are**
— 0002, 0017, 0035, 0037, 0038, 0042. Three are not:

- **0011** — banner names 0049, but adds "Two halves are carried forward rather
  than repealed," and 0049's body never claims the supersession. All ten of
  0011's code citations are for a *surviving* half, including
  `src/host/fake.rs:103`, where an `Effect` variant exists to enforce it.
- **0032** — keeps most of its argument. 0039 reciprocates with a section titled
  "What this gives up, which ADR 0032 was protecting." Its no-cache and
  silent-`status` clauses are live at `src/upgrade.rs:372`.
- **0034** — 0042 exempts one clause by name (`perch config unset`). That clause
  was then deleted by **0051, which never mentions 0034 or 0042**.

And the corpus misses decisions killed silently from outside their chain. Two
confirmed:

- **0034's exempted clause**, above.
- **0013's "it never acts on an ungrouped account"** — false in the tree
  (`src/commands/watch.rs:653`), killed by 0017's amendment and 0051. No ADR
  records it.

**No ADR in this corpus has a `Status:` header.** Supersession is prose. Nothing
mechanical can find a dead ADR here.

### 2. ADR 0046 contradicts itself about its own boundary

The single most dangerous document to merge from. 0046 declares it supersedes
"the whole of ADR 0013's 'Amended: the numbers this asked for' section, and
nothing else" — then lists nine clauses of 0013 that survive, **seven of which sit
inside the section it just declared superseded in full**: the interval (0013:90),
the back-off (:112), the cooldown placement (:146), the `--once` record (:153),
the ungrouped refusal (:189), both grants (:198), the exit-code table (:217).

Read literally, 0046 kills the interval and the exit-code table in one sentence
and preserves them in the next. 0013's own header ("Nothing else below moved") is
the reading that matches the tree.

Two of the nine it lists as surviving are false: the daemon rejection was already
dead by 0040 — *six ADRs earlier* — and the ungrouped refusal is contradicted by
the code. **A merge that trusts 0046's boundary statement will delete live,
heavily-cited decisions.** 0013 has 91 citation sites, the most of any ADR.

### 3. The merge clusters collide, so pairwise merging cannot build the target set

Readers working different areas proposed clusters that claim the same documents:

| Document | Claimed by | And by |
|---|---|---|
| **0008** | 0001 + 0008 + 0020 ("where a Credential lives") | 0008 + 0021 + 0025 ("dependency policy") |
| **0027** | 0022 + 0027 (reader/writer halves) | 0010 + 0027 (the design of `perch run`) |
| **0051** | 0002 + 0017 + 0051 (scoping) | 0046 + 0051 (Setting surface) |

This is not reader error — it is what happens when documents are filed by
chronology and read by subject. **The target set has to pick an axis and let some
documents split rather than merge.**

There is also written resistance to consolidation that has to be overruled
explicitly: **0043, 0044, 0045 and 0050 each declare "four axes, no overlap."**

### 4. Citation count is misleading, in both directions

**ADR 0004 has zero citations anywhere in the tree** and is not dead. Nothing is
*shaped by* it in a way a comment could point at; its testable clauses were
delegated at writing time to 0015 and 0029, so citers reach for those. Its scope
half has **no merge partner anywhere in the 63** — delete it and "what Perch is
for" goes unrecorded.

Conversely **ADR 0016's surviving half is cited nowhere in the code that
implements it** — `src/report.rs` restates its reasoning in its own words with no
tag.

### 5. Going public makes one ADR stronger, not weaker

The stance change is not uniformly a deletion job. **ADR 0041 closes by depending
on the thing `CLAUDE.md` denies:**

> This decision is paid for by the commitment to actually use Perch… If nobody
> lives with the tool… this ADR is a straight loss rather than a trade.

against `CLAUDE.md`'s "Nobody is using this yet — not the author, not anyone
else." Nothing in the tree resolves it. The announcement resolves it in 0041's
favor.

The two documents that argue *from* no-installed-base:

- **0062:162** — "There is no installed base and no compatibility to keep, so an
  older version is never the safer one; it is only the older one."
- **0052** — argues the same way about reversal.

And the new stance lands nowhere in ADR 0031, where it was expected. It lands on
`CLAUDE.md`'s "do not write migration code" rule, which it reverses, and on
`src/registry.rs:20` — the refuse-a-newer-registry guard. `CHANGELOG.md` already
uses `[**breaking**]` markers inside 0.x minors, so half the stance is current
practice with no document claiming it.

---

## Two sentences that must survive the reset

ADR 0042 is superseded in full and has **two** load-bearing sentences, in two
different places:

1. *"a surface which writes at all pulls in locking, deferral, refusal and
   rollback, and a surface that only reads and acts does not"* — carried forward
   by **0049**. Three of its four terms have live referents: locking
   (`src/commands/mod.rs:150`), refusal (`src/registry.rs:2345`), rollback
   (`src/import.rs:350`). Only *deferral* died with the panel. Sharpest instance:
   `src/commands/status.rs:37`, where bare `status` takes a shared read and
   `--refresh` takes the exclusive lock.
2. *"reversibility really was the wrong axis"* — carried **only by 0057**, and
   only as a quotation. 0049 did not preserve it.

Deleting 0042 costs nothing. **Rewriting 0049 or 0057 carelessly is what would
lose it.**

Related, and needing a ruling: `src/commands/service.rs:17` cites "reversibility
being the line this codebase keeps drawing (ADR 0033, ADR 0034)" — a live citation
resting on the exact axis 0042 and 0057 both reject. ADR 0033 alone carries the
claim correctly.

---

## The guide's exposure, counted

**60 ADR mentions across 9 of the 10 files** in `pages/src/content/docs/`:

| File | Mentions |
|---|---|
| `status.md` | 12 |
| `watching.md` | 9 |
| `reference.md` | 8 |
| `configuration.md` | 7 |
| `running.md` | 7 |
| `switching.md` | 7 |
| `accounts.md` | 5 |
| `backup.md` | 4 |
| `index.mdx` | 1 (a pointer to `docs/adr/`, not a citation) |
| `installing.md` | 0 |

These are inline parentheticals in user-facing prose. The guide teaches by
pointing at ADR numbers.

---

## Defects found in living documents

Cheap to fix, and several are self-indicting.

- **ADR 0054 claims an amendment to 0047 that was never written into 0047.**
  Clause 4 (from 0052) is in 0047's "Admitting a command later" list with an
  attribution line; the list stops at four.
- **The surface count is wrong in three ADRs.** 0052, 0053 and 0054 all close on
  "fifteen names, twenty-seven forms." It is fifteen names and **twenty-six** —
  0053 subtracted `perch tui`, nobody subtracted `perch config unset` after 0051
  deleted it. 0052 supplies its own epigram: *"a wrong number left standing is
  read as a right one."*
- **ADR 0026 carries an amendment credited to no ADR at all** — the
  `.oauth_refresh.lock` denylist entry, live at `reconcile.rs:59`.
- **ADR 0041's "The repo is left with no test that drives `perch` as a process"
  is false** — `tests/invoking.rs:50` does exactly that, deliberately, per 0044.
- **ADR 0023's last Consequence is unimplementable** — a Quarantine "raised by a
  build that did not record reasons." `Quarantine` (`src/registry.rs:62`) is a
  closed four-variant enum with no reasonless arm. It is the
  read-what-an-older-Perch-wrote guard `CLAUDE.md` forbids, and the code already
  declined it.
- **ADR 0024 overclaims twice**: "`perch remove` is the only command that
  destroys something" (`perch holdings purge` does too, and
  `src/commands/remove.rs:4` repeats it), and "the second command… with nobody at
  the terminal" (there are now four).
- **ADR 0043 lost its mechanism** — `said()` and `tests/browsing.rs` were deleted
  in `7c36fa1`. The rule survives and is cited at eight sites; the machinery
  section and both quoted examples are dead text.
- **ADR 0045 self-reverses in place** via an "in part by #204" note.

### Decayed clauses inside living documents

- **0015** names `tui` as a surface, and claims "`perch watch` is what keeps the
  cache warm" — false; the loop Refreshes one Account by design (`src/watch.rs:41`).
- **0020**'s "the format stays at version 1" — `CURRENT_VERSION` is 2.
- **0021** counts "four" platform primitives; the code links at least seven, and
  `Cargo.toml:22` deliberately refuses to write the number down.
- **0022**'s "the check is exact rather than heuristic" — `CLOCK_STEP_MARGIN_MILLIS
  = 5_000` (`probe.rs:1233`) exists because Linux recomputes process start from a
  `btime` that moves with the wall clock. Recorded in no ADR.
- **0025**'s "Reopened by `perch tui`" section describes crossterm as "now in the
  tree" and names `tui/terminal.rs`.
- **0017**'s Consequences opens "Perch gains global configuration," falsified by
  0051 and never corrected.
- **0012**'s closing sentence names the picker.
- **0009**'s stated mechanism — login runs in `perch_home/pending/login-<millis>`,
  not the new Profile's directory. Recorded only in code comments.
- **0006** describes the running Switch as three writes; it is four (0048, which
  explicitly refused to amend 0006).
- **0028**'s "the signal path a TUI depends on"; **0039**'s path to
  `packaging/pages/install-test.sh`, now `packaging/install-test.sh`.

### Stale citations in code

- `src/registry.rs:3509` — test comment explaining the `global` reservation by
  0051's dead reason.
- `tests/configuring.rs:10` — "the global ungrouped-Cycling setting," falsified by
  0051.

---

## Verdict tally

| Verdict | Count | ADRs |
|---|---|---|
| Dead by own text | 6 | 0002, 0017, 0035, 0037, 0038, 0042 |
| Dead in fact, unrecorded | 1 | 0034 (last clause killed by 0051) |
| Superseded in part | 4 | 0011, 0013, 0016, 0032 |
| Holds, with decayed clauses | 14 | 0006, 0009, 0012, 0015, 0020, 0021, 0022, 0024, 0025, 0028, 0039, 0041, 0043, 0045 |
| Holds | 38 | the rest |

Only **0035** is genuinely hollow among the survivors — every mechanism it names
(mdBook, `SUMMARY.md`, `docs/guide/`, `.github/actions/mdbook`) is absent, and
0062 already quotes the two sentences that survive.


---

# Appendix: the full supersession graph


Extracted by grepping `docs/adr/*.md` for supersession, repeal, amendment and
refusal language, then reading the sentence that establishes each edge. No ADR in
this corpus carries a `Status:` header — supersession is stated in prose, usually
as a blockquote banner at the top of the superseded document.

## Superseded in full

| Dead | By | Sentence |
|---|---|---|
| 0002 | 0051 | "Superseded in full by ADR 0051." |
| 0017 | 0051 | "Superseded in full by ADR 0051." |
| 0035 | 0062 | "Superseded by ADR 0062. mdBook is replaced by Astro and Starlight" |
| 0037 | 0041 | "Superseded by ADR 0041. The suite was removed entire (#148)" |
| 0038 | 0041 | "Superseded by ADR 0041. The removal landed in #148" |
| 0042 | 0049 | "ADR 0042 is superseded in full — both halves of its decision are void" |

## Superseded in part

| Partly dead | By | Scope of the supersession |
|---|---|---|
| 0013 | 0040 | "Supersedes the part of ADR 0013 that rejected a managed background daemon." |
| 0013 | 0046 | "This supersedes the whole of ADR 0013's 'Amended: the numbers this asked for' section — and only this section." Superseding 0013 whole was **refused**: "It would restate a mostly-correct record." |
| 0016 | 0049 | "Superseded in part by ADR 0049. This file is three decisions and only two…" |
| 0011 | 0049 | Banner names 0049 and the picker is genuinely gone, but the same banner says "Two halves are carried forward rather than repealed" — and 0049's own body never claims the supersession. **Partial.** All ten of 0011's code and test citations are for the *surviving* half ("every capability exists non-interactively", "bare `perch switch` asks nothing"), including `src/host/fake.rs:103`, where an entire `Effect` variant exists to enforce it. |
| 0034 | 0042 | Banner reads as full ("The Config tab is removed — and `perch tui` with it"), and 0049 confirms the chain ("ADR 0034 stays superseded by 0042"). But 0042 exempts one clause by name: "One thing here is not repealed: `perch config unset` stays… because a two-layer configuration needs a way back to Inherit." **Partial, not full.** That surviving clause was then killed by **ADR 0051**, which deleted `perch config unset` without naming 0034 or 0042 — so the chain 0011→0034→0042→0049 does not record its own end. |
| 0032 | 0039 | Banner names 0039 but keeps most of the argument: "there is still no schedule, no cache and no age, and `perch status` is still silent on the network — but the title is no longer true". 0039 reciprocates with a section titled "What this gives up, which ADR 0032 was protecting". **Partial, not full** — the mechanical grep misreads this one. |

## Explicit refusals to supersede

These are the precedent chain, and they matter more than the edges: three separate
documents chose partial supersession on purpose and said why.

- **0046 → 0013**: "Superseding ADR 0013 whole was refused. It would restate a mostly-correct record."
- **0051 → 0017**: "Superseding ADR 0017 whole was considered and refused, on ADR 0046's precedent."
- **0059 → 0056**: "ADR 0056's Consequences is amended in place rather than superseded." Cites both: "ADR 0046 and ADR 0051 both set the precedent for refusing to supersede a…"

## Amends without superseding

- 0048 → 0006: "This supersedes nothing and amends nothing. ADR 0006 is untouched."
- 0052 → 0047: "This supersedes nothing and amends ADR 0047 in one clause."
- 0054 → 0047: "This supersedes nothing and amends ADR 0047 in one clause."
- 0053 → 0047: "This supersedes nothing, and ADR 0047 is answered rather than amended."
- 0051 → 0046: "ADR 0046 is answered, not superseded."
- 0045, 0050: "This ADR supersedes nothing."

## What this already settles for the target-set ticket

Six documents are dead by their own text: **0002, 0017, 0035, 0037, 0038, 0042**.
Three more read as dead to a grep and are not:

- **0032** is superseded only in part; its no-cache and silent-`status` clauses are
  live and cited from `src/upgrade.rs:372`.
- **0034** is superseded only in part; its one exempted clause was later killed by
  a third document (0051) that never mentioned it. It is dead *in fact*, but no
  single edge says so.
- **0011** is superseded only in the picker half. Its banner says "Two halves are
  carried forward rather than repealed", and every one of its ten code citations is
  for a surviving half.

The lesson for the reset: **a grep for supersession banners overstates what is
dead in two directions at once** — it calls partial edges whole, and it misses
decisions killed silently from outside the chain. They need no argument to remove — only a decision about where
their carried reasoning lands.

**0042's load-bearing sentence has already been carried twice, and is still true.**
ADR 0049 quotes it while voiding the rest, and **ADR 0057 independently rebuilds an
argument on it** ("wrong for the reason ADR 0042 already found it wrong").
Three of its four terms have live referents in code — locking
(`src/commands/mod.rs:150`), refusal (`src/registry.rs:2345`), rollback
(`src/import.rs:350`); only *deferral* died with the panel. The sharpest single
instance is `src/commands/status.rs:37`, where bare `status` takes a shared read
and `--refresh` takes the exclusive lock.

**But 0042 has two load-bearing sentences, not one.** ADR 0057 borrows a *second*
one — "reversibility really was the wrong axis" — which 0049 did **not** carry
forward. So collapsing 0042 must rescue two sentences into two different homes:
the writes-pull-in-locking rule (0049 already has it) and the wrong-axis rule
(only 0057 has it, and only as a quotation).

The risk is the opposite of what the map assumed: deleting 0042 costs nothing,
**rewriting 0049 or 0057 carelessly is what would lose it.**

**0013 survives both its partial supersessions**, and has 91 citation sites — the
most of any ADR. Whatever is left standing after 0040 and 0046 have taken their
sections is what the survivor must say.

---

# Appendix: candidate merge clusters

Offered as evidence. Clusters overlap on purpose — see finding 3.


- **0008 + 0021 + 0025** — one dependency policy in three files. 0025's "Not taken" already restates both; 0021 exists only to reconcile with 0008. (slot F)
- **0056 + 0059** — one decision in two passes. 0059 amends 0056 in place and refuses to supersede it. (slot F)
- **0028 + 0029 + 0030 + 0031** — 0028 self-declares it: 0029–0031 "are most of the decisions in ADR 0029 through 0031" a generator would have made. 0028 is the umbrella. (slot G)
- **0002 + 0017** — one question (which Accounts may a Cycle move between) answered for the two cases there are. 0017 opens calling itself "ADR 0002's rule followed to its conclusion"; shared premise sentence, shared code path. **0051 does not join them** — different question, different object. (slot D)
- **0015 + 0018** — 0018 has no independent premise and says so: "The reasoning is ADR 0015's, followed through." (slot D)
- **0049 + 0053 + 0058 (+ 0060)** — applications of one rule: the Listing owns every claim about a set. 0053 quotes 0049 to close its own case; 0058 says "ADR 0049 gains a reader rather than an amendment". (slots D, E)
- **0012 + 0046** — the same "arithmetic is not taste" distinction applied to two subsystems. (slot D)
- **0043 + 0061** — two halves of one subject. 0043's opening specimen is the exact seven-line `perch switch` output that 0061 deleted, so 0043 now quotes output that does not exist. (slot E)
- **0055 + 0056 + 0059 + 0060** — a "where code lives" cluster. (slot E)

# Decayed clauses inside living documents

- 0015 names `tui` as a surface (removed by 0049), and claims "`perch watch` is what keeps the cache warm" — false: the loop Refreshes one Account by design (`src/watch.rs:41`). (slot D)
- 0012's closing sentence names the picker. (slot D)
- 0017's Consequences opens "Perch gains global configuration," which 0051 falsified and did not correct. (slot D)
- 0025's whole "Reopened by `perch tui`" section describes crossterm as "now in the tree" and names `tui/terminal.rs`. (slot F)
- 0021 counts "four" platform primitives; the code links at least seven, and `Cargo.toml:22` deliberately refuses to write the number down. (slot F)
- 0028's "the signal path a TUI depends on" — no TUI; the path survives with a different rationale. (slot G)
- 0039 cites `packaging/pages/install-test.sh`, now `packaging/install-test.sh`. (slot G)

# Edges with no source node

- **0026 "Amended again"** (the `.oauth_refresh.lock` entry) names no ADR — an in-place correction with no document behind it. Its first amendment is by 0027. (slot D)

# Live citations resting on repealed premises

- `src/commands/service.rs:17` — "reversibility being the line this codebase keeps drawing (ADR 0033, ADR 0034)". 0034 is superseded by 0042, titled "reversibility was the wrong line"; 0057 rejects the axis again. 0033 alone carries the claim correctly. Needs a ruling. (slots E, G)
- `src/registry.rs:3509` — stale test comment explaining the `global` reservation by 0051's dead reason. (slot D)
- `tests/configuring.rs:10` — "the global ungrouped-Cycling setting", falsified by 0051. (slot D)

## From slot A

- **0001 + 0008 + 0020** — one decision ("where a Credential lives and how it is reached") split across three documents by chronology, not subject. Note the collision: slot F independently proposed **0008 + 0021 + 0025** as a dependency-policy merge. 0008 is claimed by two clusters and cannot join both.
- **0022 + 0027** — reader half and writer half of one mechanism. 0027's correctness argument is unintelligible without 0022's rule, and 0022 has no second reader.
- **0003 + 0026** — the same question answered in opposite directions (named set vs. denylist), for the directory and for the one file that cannot be linked. 0026's own text says 0003 decided it.
- **0010 + 0027** — together the complete design of `perch run`. (0027 is now claimed by two clusters as well.)

### Decayed clauses
- 0020's "the format stays at version 1" — `CURRENT_VERSION` is 2, moved by 0051 for an unrelated reason. The clause it defended (fields dropped, nothing migrated) is intact.
- 0022's "the check is exact rather than heuristic" — no longer literally true. `CLOCK_STEP_MARGIN_MILLIS = 5_000` (`probe.rs:1233`) exists because Linux recomputes process start from a `btime` that moves with the wall clock. Recorded in no ADR.
- 0009's stated mechanism — the login runs in `perch_home/pending/login-<millis>`, not the new Profile's directory. Principle intact; the change is recorded only in `login.rs` and `commands/add.rs` comments.
- 0006 describes the running Switch as three writes; it is now four (the Landing lands between Capture and move, per 0048, which explicitly refused to amend 0006).

### ADR 0004 — zero citations, and not dead
Its zero citation count is **altitude, not death**. Nothing is *shaped by* it in a
way a comment could point at; its two testable clauses were delegated at writing
time to 0015 (hot path) and later 0029 (runtime-free distribution), and citers
reach for those instead. Its scope half has **no merge partner anywhere in the 63**
— if it dies, "what Perch is for" is unrecorded. This is the one place where
citation count is actively misleading as evidence.

## From slots B and H

- **0047 + 0052 + 0053 + 0054** — one rule and three clauses of it. 0052 and 0054 each close in *identical words*: "This amends ADR 0047 in that one clause and nothing else… Its decision, its table and its counts are untouched." 0053 declines to be an amendment in the same breath. **0057 is not part of this cluster** — it is about the registry lock, not the surface, and shares only the files. (slot B)
- **0048 + 0006** — 0048 calls itself the decision 0006 never made, and preserves 0006 word for word. (slot B; slot A independently flagged the same pair from the other side)
- **0023 + 0054** — 0054's naming argument is drawn entirely from 0023's Consequences. (slot B)
- **0045 + 0050** — 0050 renamed the binaries 0045 placed and reuses its closing paragraph verbatim. Caveat below. (slot H)

### The anti-merge argument that has to be answered
**0043, 0044, 0045 and 0050 each explicitly declare "four axes, no overlap."** Any
merge touching them must overrule three documents that say in their own text they
do not overlap. This is the strongest written resistance to consolidation anywhere
in the corpus.

### Defects found in living documents
- **ADR 0054's clause 5 was never written into ADR 0047.** Clause 4 (from 0052) is in 0047's "Admitting a command later" list with an attribution line; the list stops at four. 0054 claims an amendment that left no trace in the amended document. (slot B)
- **The surface count is wrong in three ADRs.** 0052, 0053 and 0054 all close on "fifteen names, twenty-seven forms". It is fifteen names and **twenty-six** forms — 0053 subtracted `perch tui`, nobody subtracted `perch config unset` after 0051 deleted it. 0052 supplies its own epigram: "a wrong number left standing is read as a right one." (slot B)
- **0041's clause "The repo is left with no test that drives `perch` as a process" is false** — `tests/invoking.rs:50` does exactly that, deliberately, per 0044. (slot H)
- **0043 lost its mechanism**: `said()` and `tests/browsing.rs` were deleted in `7c36fa1` (ADR 0049). The rule survives and is cited at eight code sites; the machinery section and both quoted example assertions are dead text. (slot H)
- **0045 self-reverses in place** via an "in part by #204" note restoring the `CONTEXT.md` heading with **Behavior** alone; `CONTEXT.md:459` matches. Only the glossary refusal reversed. (slot H)
- **ADR 0023's last Consequence is dead and unimplementable** — a Quarantine "raised by a build that did not record reasons". `Quarantine` (`src/registry.rs:62`) is a closed four-variant enum with no reasonless arm. It is exactly the read-what-an-older-Perch-wrote guard `CLAUDE.md` forbids, and the code already declined it. (slot B)
- **0024 overclaims twice**: "`perch remove` is the only command that destroys something" (`perch holdings purge` does too, and `src/commands/remove.rs:4` repeats it), and "the second command… with nobody at the terminal" (there are now four). (slot B)

### The stance contradiction, in the corpus rather than in CLAUDE.md
**ADR 0041's closing dependency contradicts `CLAUDE.md` head-on.** 0041: *"This
decision is paid for by the commitment to actually use Perch… If nobody lives with
the tool… this ADR is a straight loss rather than a trade."* `CLAUDE.md`: "Nobody
is using this yet — not the author, not anyone else." Nothing in the tree resolves
it. The announcement resolves it in 0041's favor, which makes 0041 *stronger*, not
weaker — worth noting for the stance ticket.

### The guide's ADR citations, counted
**60 mentions across 9 of the 10 files** in `pages/src/content/docs/`: `status.md`
(12), `watching.md` (9), `reference.md` (8), `configuration.md` (7), `running.md`
(7), `switching.md` (7), `accounts.md` (5), `backup.md` (4), `index.mdx` (1 — a
pointer to `docs/adr/` rather than a citation). **`installing.md` has none.** These
are inline parenthetical cites in user-facing prose: the guide teaches by pointing
at ADR numbers. This is the whole surface constraint 9 has to clear.

### The no-installed-base sentence, verbatim (ADR 0062:162)
> There is no installed base and no compatibility to keep, so an older version is
> never the safer one; it is only the older one.

## From slot C (the Watcher)

- **0015 + 0018** — confirmed independently from the Watcher side: "the same decision in two…". Slot D reached the same pair from the cycling side. This is the strongest two-document merge in the corpus.
- **0013 + 0040** — 0040 is 0013's partial successor; merging 0013's surviving arithmetic into it is the natural shape.
- **0046 + 0051 + 0017 + 0002** — 0046 decides *which* Watcher numbers are anyone's to set; 0051 decides the *shape* of the Setting surface; 0017 and 0002 decide the Scope. Slot D argued 0002+0017 merge and 0051 stands beside; slot C sees 0046 as a fourth leg.
- **0056 + 0059 + 0055** — 0055's rejection of a port is the best surviving evidence for 0025's rule, and 0056/0059 ask the same seam question of a different module.
- **0050 + 0007** — 0050 amends 0007 ("one amendment and no supersession").

### The contradiction at the center of the Watcher knot

**ADR 0046 contradicts itself on where its own supersession boundary falls**, and
this is the single most dangerous thing in the corpus for a merge pass.

0046 declares it supersedes "the whole of ADR 0013's 'Amended: the numbers this
asked for' section, and nothing else." It then lists nine clauses of 0013 that
survive — but **seven of those nine sit *inside* the section it just declared
superseded in full**: the interval (0013:90), the back-off (:112), the cooldown
placement (:146), the `--once` record (:153), the ungrouped refusal (:189), both
grants (:198), the exit-code table (:217). Read literally, 0046 kills the interval
and the exit-code table in one sentence and preserves them in the next.

0013's own header ("Nothing else below moved") is the reading that matches the
tree, and is the one to carry forward. **A merge that trusts 0046's own boundary
statement will delete live, heavily-cited decisions.**

Two of the nine clauses 0046 lists as surviving are simply false:
- "the rejection of a managed daemon" — already superseded by **ADR 0040**, which
  is *earlier*. 0046 re-affirmed a clause that had been dead for six ADRs.
- "never acting on an ungrouped Account" — false in the tree
  (`src/commands/watch.rs:653`), killed by 0017's amendment and 0051. **No ADR
  records this.** Second confirmed instance of a decision killed silently from
  outside its chain.

---

# The 63, in order

## ADR 0001 — Accounts are stored as profiles, and switching swaps the live credential


### 1. The decision, in one sentence

An Account is held as its own Claude Code config directory (a Profile), whose
Credential Store is derived from that directory's path, and making one active is
a write of its Credential into the Default Profile's store plus a patch of the
`oauthAccount` block of `.claude.json` — never a wholesale swap of that file.

### 2. Does it still hold?

**Holds in part** — the conclusion holds; the stated mechanism is now macOS-only.

Live:
- The Profile-per-Account directory: `registry.rs:1655` `profile_dir_for`, under
  `perch_home/profiles/` (`registry.rs:1576`).
- The derivation `Claude Code-credentials-<sha256(dir)[0:8]>`, with the bare name
  for the default directory: `probe.rs:380` `service_name_for`, `probe.rs:388`
  `short_hash`.
- Patching only `oauthAccount`: `switch.rs:1320` `patch_identity` → `probe.rs:1282`
  `patch_oauth_account`, spliced as text rather than reparsed (`json.rs:1-11`), so
  project history, MCP config and settings survive byte-for-byte.
- Pointing one process at a Profile with `CLAUDE_CONFIG_DIR`: `commands/run.rs:150`.
- The Consequences clause about cooperating with Claude Code's own locks:
  `lock.rs:1-8`, `probe.rs:822` `locks_for` (three named locks, all `held_by:
  "Claude Code"`).

Not live as written: "a stored credential lives in the operating system's keychain
rather than in a token file Perch invented" is true only on macOS. Off macOS the
keychain is a store that never holds anything (`credentials.rs:32`, `:49-53`) and
the Credential is a 0600 file inside the Profile — ADR 0020's correction, which
0020 itself frames as leaving 0001's conclusion intact.

### 3. Supersession

Supersedes nothing. **Partially superseded by ADR 0020**, which is explicit that
the supersession is of mechanism only: *"Off macOS the service-name derivation of
ADR 0001 — `sha256(CLAUDE_CONFIG_DIR)[0:8]` — is simply irrelevant: the Credential
lives inside the Profile directory, so a Profile's isolation comes from the
directory itself rather than from a namespace derived out of it. The conclusion of
ADR 0001 survives unchanged; only its mechanism is macOS's."* 0020's opening
sentence — *"ADR 0001 and ADR 0008 were written against a macOS-only Perch, and
both assume a Credential is held in the operating system's keychain"* — names
exactly which clause it is taking.

The `run` path clause is developed rather than superseded by ADR 0010 (which 0001
already forward-cites) and, for what crosses into the Profile, by ADR 0026/0003.

### 4. Reasoning that outlives the document

> "Patching only `oauthAccount`, rather than swapping `.claude.json` wholesale, is
> deliberate: that file also holds project history, MCP configuration, and
> settings, none of which belong to the account. Leaving it in place is what makes
> all of that state follow the person across a switch for free."

This is the origin of the Shared State / Account split that CONTEXT.md, ADR 0026
and ADR 0003 all now depend on, and it does not depend on any keychain claim.

Secondarily:

> "Switching writes a credential that a running Claude Code may hold, so it has to
> cooperate with Claude Code's own OAuth refresh locks rather than inventing its
> own scheme."

### 5. Others deciding the same thing

- **ADR 0020** — the strongest candidate anywhere in my slice. 0020 restates
  0001's conclusion, corrects its mechanism, and then carries the *whole* of the
  live Credential-Store decision (composite reader, primary/fallback, supersession
  of the unwritten copy, 0600/0700). Reading 0001 without 0020 is misleading on
  every non-Mac; reading 0020 without 0001 loses only the `oauthAccount`-patching
  paragraph, which is a different decision that happens to share a document.
- **ADR 0008** — the other half of what 0020 corrects. 0001 + 0008 + 0020 are one
  decision ("where does a Credential live, and how is it reached") split across
  three documents by chronology rather than by subject.
- **ADR 0010** — 0001's third paragraph *is* ADR 0010's premise, and 0010 cites it
  back.

### 6. Citations

18 total: 8 in `docs/`, 10 in code and elsewhere — `tests/switching.rs`,
`src/reconcile.rs`, `src/probe.rs`, `src/carry.rs`, `src/switch.rs`, `src/lock.rs`,
`src/json.rs`, `pages/.../reference.md`, `pages/.../switching.md`, plus ADRs 0003,
0009, 0020, 0025.

**Load-bearing in code, attributive on the site.** `lock.rs:5` cites it to explain
why Perch takes somebody else's lock; `json.rs:6` to explain why the file is
spliced rather than parsed; `reconcile.rs:301` to explain why a dangling link is
not inert (`mkdir` at one fails exactly as a held lock does); `carry.rs:230` to
explain why the active Account's state is in `~/.claude` and not in its Profile.
Each is a "why the code is this shape" cite. The two `pages/` cites are trailing
parentheses attached to sentences that already state the fact — a pointer, not an
explanation.

---

## ADR 0002 — Groups scope cycling and carry per-group configuration


### 1. The decision, in one sentence
A Group is nothing but the user's own declaration that a set of Accounts is
interchangeable — it exists to bound a Cycle ("`perch switch` with no target
chooses within the current account's group, and never leaves it") and to be the
thing that carries the rules governing Cycling within it, and it is never
inferred from an organization UUID.

Note the title reads worse than the body: "carry" there is the verb *to carry
configuration*, and it now collides head-on with the glossary noun **Carry**
(the `.claude.json` pass). Nothing in this ADR is about that Carry.

### 2. Does it still hold?
**Holds**, and is one of the most thoroughly implemented decisions in the tree.

- A Cycle never leaves its Scope: `src/cycle.rs:18` states it as one of the
  module's two rules, and `src/cycle.rs:56-68` (`scope_for`) is the only place a
  bare `perch switch` gets a Scope from — `Some(group) => Scope::Group(group)`,
  and nothing widens it.
- A Group carries the three Settings the body names: `src/registry.rs:331-343`
  is exactly `strategy`, `watcher_may_act`, `watcher_threshold_percent`.
- "Unattended switching is off by default": `src/registry.rs:345-353`,
  `watcher_may_act: false`, with the ADR cited in the field's own doc at
  `:333-335`; asserted at `tests/grouping.rs:80-85`.
- Organization is offered, never inferred: `src/commands/add.rs:244-248`
  (`resolve_group`) — *"The organization is offered and never assumed: three
  subscriptions bought personally each carry their own organization, so
  inferring from it would split exactly the case Groups exist to serve (ADR
  0002)"* — and `src/commands/group.rs:1` names the whole command for it.
- "flock" is still rejected: the word appears only in `CONTEXT.md`'s `_Avoid_`
  list for **Group**.

One clause of the *Considered Options* has quietly become the design rather than
the runner-up: "Dropping groups entirely and using a per-account disable flag was
seriously considered and is close in value." Perch now ships *both* — Groups and
a per-Account **Disabled** flag (`src/commands/enable.rs`, `Account::disabled`) —
so the option was not rejected, it was absorbed for a different purpose. Nothing
in the ADR is falsified by that; it is just no longer the fork it describes.

### 3. Supersession
Supersedes nothing. Its **amendment only** — "Global carries the defaults and a
Group Overrides them" — is superseded, and the banner is explicit that this is
partial and which half survives:

> **Superseded in full by ADR 0051.** Every sentence below is about Global,
> Override, Inherit, the two layers or the word-count idiom, and none of the five
> exists any more […] **The body above is untouched, and better than untouched.**

0051 says the same from its own side: *"**ADR 0002's amendment […] — superseded
in full.** […] **ADR 0002's body is untouched**, and better than untouched […]
The second leg Groups stand on is strengthened rather than cut."*

So: **body live, amendment dead.** The amendment's text is still physically
present in the file under a banner.

### 4. Reasoning that outlives the document
> "a group is the declaration of which accounts are interchangeable"

and

> "Unattended switching is off by default, so a group only ever changes
> underneath you because you said it could."

Both are quoted or paraphrased at seven live sites (`src/cycle.rs`,
`src/registry.rs:333-339`, `src/config.rs:62`, `src/commands/watch.rs:645-648`,
`src/commands/remove.rs:218`, `src/commands/list.rs:77`, `src/commands/add.rs:248`)
and would have to be restated somewhere if this document died.

Also worth carrying, because it is the reason inference is refused and it is a
general claim about identity rather than about Groups:

> "three subscriptions bought personally each carry their own organization UUID,
> so inference would split them and fail precisely the case it exists to serve"

### 5. Others deciding the same thing
- **ADR 0017** — the strongest candidate, and 0017 says so itself: *"This is ADR
  0002's rule followed to its conclusion."* 0002 answers "which Accounts may a
  Cycle move between?" for the Group case; 0017 answers the same question for the
  case where there is no Group. One question, two halves of one answer.
- **ADR 0051** — *not* a third answer to that question, despite the surface
  similarity. See the judgment written out under 0051 §5.
- **ADR 0011/0049** — 0002's "chooses within the current account's group" is the
  same sentence 0011 spent a document on; 0049 killed 0011's picker and moved the
  ranking to `perch list`. Related, not the same decision.

### 6. Citations
32 total — 11 in `docs/`, 21 in code and elsewhere, across `README.md`,
`tests/{adding,listing,configuring,grouping}.rs`, five other ADRs, and
`src/{cycle,registry,config}.rs`, `src/commands/{remove,list,group,add}.rs`.

**Load-bearing, almost without exception.** `src/commands/add.rs:244-248` and
`src/registry.rs:333-339` both restate the ADR's *reason* in the doc comment
before citing it; `src/commands/remove.rs:234` uses it to justify what a removal
deliberately does *not* do; `src/commands/list.rs:123` uses it to explain why
there is no one ranking across every Account. The nearest thing to decorative is
`tests/grouping.rs:84`, a bare tag on an assertion message — but even there the
message states the rule ("unattended switching is off until the user says
otherwise") rather than only naming the number.

---

## ADR 0003 — Perch writes the current project's entry into .claude.json on the run path


### 1. The decision, in one sentence

On the Run path only, and never on a Switch, Perch copies a **named set** of
`.claude.json` keys — `projects[<cwd>]` plus the person-keys — from the most
recently used Profile in the same Group into the Profile being launched, bounded,
only when nothing is running against that Profile, and never able to refuse the
Run.

### 2. Does it still hold?

**Holds**, including its own amendment, essentially clause for clause.

- Named set, not inverted: `carry.rs:47` `PERSON_KEYS = ["hasCompletedOnboarding",
  "lastOnboardingVersion", "tipsHistory", "seenNotifications"]`, with the module
  doc at `carry.rs:13-18` restating why the direction is opposite to Reconcile's.
- The excluded caches are named in the doc comment above the constant
  (`cachedUsageUtilization`, `modelAccessCache`, `overageCreditGrantCache`,
  `orgModelDefaultCache`).
- `projects[<cwd>]` alone, one entry: `carry.rs:185-216`.
- Single-key read and single-key write, not a merge: `json.rs` splice API
  (`value_at` / `set_value_at`).
- The quiet precondition: `probe.rs:1119` `anything_running` — *"the Carry writes
  into a Profile only when it is quiet (ADR 0003), and doubt is the same answer as
  a client for that purpose."*
- Most-recently-used within the Group, with the Default Profile standing in for the
  active Account and the launched Profile never a candidate: `carry.rs:227-233`.
- "Nothing here can refuse a Run": `carry::carry` returns `()` and is called
  without `?` at `commands/run.rs:126`.

### 3. Supersession

Supersedes nothing. Superseded by nothing. It **amends itself** in-document under
`## Amended: one key becomes a named set`, which is a genuine widening of the
decision ("one key" in the title is now false).

It cites ADR 0002 for a rejection it inherits (*"The copy is scoped to a group on
its own merits, not because groups scope shared state generally — ADR 0002
explicitly abandoned that"*), and ADR 0026 for why this one file cannot be linked.
ADR 0027 leans on it for the third refusal register (*"A `.claude.json` key simply
does not cross, silently, which is ADR 0003's amendment"*) without amending it.

### 4. Reasoning that outlives the document

> "Inside this file account-scoped state is common enough that naming what crosses
> is the safer direction, which is the opposite of the direction the directory
> takes."

And, on the cost asymmetry that decides every failure mode on this path:

> "Nothing here can refuse a Run. … a Run that happened is worth more than a key
> that crossed."

Both are quoted almost verbatim in `carry.rs`'s module doc, so they are already
carried; the document's death would not lose them, but a survivor should state
them.

### 5. Others deciding the same thing

- **ADR 0026** — the same question ("what of the Default Profile reaches the
  Profile a Run launches") answered in the two opposite directions, for the
  directory and for the one file that cannot be linked. 0026's own text says
  `.claude.json` *"is therefore the one file handled key by key, which ADR 0003
  decided"*. These read as one decision with two halves, and the tests already
  treat them as neighbors (`tests/carrying.rs`, `tests/reconciling.rs`).
- **ADR 0027** — its "sessions stops crossing" section is an edit to 0026's
  denylist, i.e. to the same rule 0003 is the exception to.
- **ADR 0005** — 0005's Consequences says outright: *"The liveness check this
  requires is the same precondition ADR 0003 already placed on writing
  `projects[<cwd>]`."* Two documents, one precondition.

### 6. Citations

21 total: 7 in `docs/`, 14 in code and elsewhere — `tests/carrying.rs`,
`tests/importing.rs`, `src/probe.rs`, `src/carry.rs`, `src/export.rs`,
`src/host/mod.rs`, `src/host/fake.rs`, `src/import.rs`, `src/commands/run.rs`,
`src/switch.rs`, `pages/.../running.md`, plus ADRs 0005, 0010, 0026, 0027, 0045.

**Load-bearing.** `carry.rs:2` is the module's reason for existing; `probe.rs:1114`
uses it to justify resolving doubt towards "not quiet"; `host/mod.rs:377` marks the
one host capability *"allowed by exactly this path"*; `tests/carrying.rs:136`
names the failure the ADR was written to prevent as the test's subject.

---

## ADR 0004 — Perch manages the whole account lifecycle, and is written in Rust


### 1. The decision, in one sentence

Perch's scope is the whole account lifecycle rather than the switch alone — add
and remove, aliases and groups, utilization, unattended switching, quarantine and
recovery, encrypted export and import, all treated as table stakes — and it is
written in Rust so `perch status` is cheap and distribution needs no runtime.

### 2. Does it still hold?

**Holds**, though it is the one ADR in this slice that reads as a charter rather
than a rule.

Every named lifecycle item shipped:

| Claim | Where |
| --- | --- |
| adding and removing accounts | `src/commands/add.rs`, `src/commands/remove.rs` |
| aliases and groups | `src/commands/alias.rs`, `src/commands/group.rs` |
| utilization tracking | `src/utilization.rs`, `src/observe.rs`, `src/commands/status.rs` |
| unattended switching | `src/watch.rs`, `src/service.rs`, `src/commands/watcher.rs` |
| quarantine and recovery | `registry.rs` `Quarantine`, `src/commands/relogin.rs` |
| encrypted export and import | `src/export.rs`, `src/import.rs`, `src/commands/holdings.rs` |

Rust: the crate is Rust with no runtime dependency, and the hot-path argument is
still live as ADR 0015's cache. The only clause that is **unverifiable from code**
is the Consequences paragraph — *"the scope has to be sequenced rather than built
at once. Deciding what ships first is the first thing any specification has to
resolve"* — which is a process instruction to a specification that no longer
exists as an artifact in the tree.

### 3. Supersession

Supersedes nothing, is superseded by nothing, and — uniquely in this slice — is
not referred to by any other ADR. Its Rust half was extended without citation by
ADR 0008 (*"Part of the case for Rust was avoiding interpreter startup, and the
credential path spawns a subprocess anyway"*), ADR 0021 and ADR 0025; its
distribution half by ADR 0029 (musl, static). None of those name it.

### 4. Reasoning that outlives the document

> "These are treated as table stakes, not stretch goals, because each one exists to
> answer a question the switch itself creates."

That is the actual load-bearing sentence: it is the test by which any future
scope question ("does this belong in Perch?") gets answered, and it is stated
nowhere else in the tree.

Also worth carrying, from the rejected option:

> "It is a smaller tool that pushes every difficult question back onto the person
> least equipped to answer it: someone mid-task who has just run out of quota."

### 5. Others deciding the same thing

The document is two decisions in one, and they have different partners.

- The **scope** half has no partner. Nothing else in the 63 decides what Perch is
  for. If it dies, that decision is unrecorded.
- The **language/runtime** half sits with **ADR 0008** (which explicitly qualifies
  the Rust argument), **ADR 0021** (platform primitives linked rather than shelled
  out), **ADR 0025** (a crate is taken where it does not cost a seam) and
  **ADR 0029** (musl, static — the "no runtime on the target machine" clause
  cashed out). Those four plus this one paragraph are one decision about what
  Perch is built from.
- **ADR 0015** is the only place the `perch status` hot-path premise is actually
  tested against reality, and 0004 already forward-cites it.

### 6. Citations

**0 total — 0 in `docs/`, 0 in code.** Verified independently: `grep -rn "ADR
0004\|0004-perch-manages"` across the whole tree outside `.git` returns nothing.
It is the only ADR in this slice with no inbound reference of any kind.

Why: nothing in the tree is *shaped by* it in a way a comment could point at. The
other eight ADRs here all decide a rule that some function has to obey, so the
function cites the rule; 0004 decides the program's extent and its language, and
there is no single site where "Perch also does quarantine" or "Perch is Rust" is
the reason a line looks the way it does — the evidence for it is the file listing.
Its two testable clauses were both delegated at the time of writing: the hot path
to ADR 0015 and (later) the runtime-free distribution to ADR 0029, and citers
reach for those instead. Zero citations here is an artifact of the document's
altitude, not evidence that it is dead.

---

## ADR 0005 — Perch refreshes tokens only for profiles with no client running


### 1. The decision, in one sentence

A Renewal is refused for any Profile a client is currently running against —
because Anthropic may Rotate on renewal and retire the token family a live
Claude Code is holding in memory — and where a Renewal is permitted the rotated
Credential is written back under Claude Code's own lock; nothing is lost,
because an Account in use already has a fresh access token and its Utilization
can be read without renewing anything.

### 2. Does it still hold?

**Holds**, in both halves, and the second half is easy to miss.

The refusal is `observe::refuse_if_live` (`src/observe.rs:667`), asked twice —
once before the locks are taken so an Account that was never going to be renewed
says so without queuing, and again inside `renew_under_the_lock` where "the
answer is the one that counts" (`src/observe.rs:694`). Its doc comment is ADR
0005's argument restated: *"renewing a Credential a running Claude Code has in
memory logs that session out silently, mid-task."* It is asked of *every*
directory the Credential could be in use from, not only the one being written,
"because a Rotation kills every copy of it at once" — a strengthening of the ADR
rather than a departure from it.

The "nothing is lost" half is `usable_at` in `observe::usable_token`
(`src/observe.rs:646`): a Credential whose access token is still good returns
without touching the network or the locks at all, which is exactly the ADR's
"an account actually in use has a fresh access token already".

The rotated-credential write-back under Claude Code's lock is
`renew_under_the_lock` (`src/observe.rs:721`), through `lock::under(host,
probe::locks_for(store), …)`.

The Consequences clause — "the liveness check this requires is the same
precondition ADR 0003 already placed on writing `projects[<cwd>]` … worth
building carefully and exactly once" — also holds: there is exactly one
liveness primitive, `probe::live_clients`, and `src/carry.rs`,
`src/observe.rs`, `src/switch.rs`, `src/purge.rs` and `src/commands/run.rs` all
go through it.

One clause has been *narrowed* by later work without being contradicted. The ADR
frames the rule as being about "ranking auto-switch candidates"; the Watcher now
also asks liveness of the **outgoing** Profile before the candidate Refresh
burst, as a typed witness (`switch::Idle`, ADR 0055), which is the same rule
enforced one step earlier.

### 3. Supersession

Supersedes nothing. Superseded by nothing — no ADR carries a supersession header
naming it, and no later ADR's body claims to overturn it. It is cited as a
still-governing constraint by ADR 0013 (line 51), ADR 0018 (line 7), ADR 0014
(line 167) and ADR 0027 (line 80); ADR 0027 explicitly leans on it rather than
replacing it: *"access token by ADR 0005's own reasoning, so its figures are
readable without…"*.

### 4. Reasoning that outlives the document

> "Anthropic rotates refresh tokens — a refresh may return a new one,
> invalidating the token family — so refreshing a credential that a running
> Claude Code still holds in memory logs that session out, silently, mid-task."

And the asymmetry that makes the rule cost nothing:

> "auto-switch candidates are idle by definition, and an account actually in use
> has a fresh access token already, so its usage can be read without refreshing
> anything."

Both are claims about Anthropic and about what "in use" implies, not about any
Perch surface, and would survive the document.

### 5. Others deciding the same thing

- **ADR 0027 (`a-run-makes-a-profile-live-and-a-switch-reads-through-it`)** is
  the strongest partner. 0027 decides the general rule — a Live Profile is
  refused a *write* and permitted a *read* — and 0005 is one instance of it (a
  Renewal is a write). 0005 predates the concept and states the instance;
  0027 states the rule and cites 0005 for the reasoning. Merging leaves one
  record of "what Live means and what it forbids".
- **ADR 0022 (`a-live-profile-is-corroborated-by-when-its-process-started`)**
  supplies the evidence rule the same check rests on. 0005 + 0022 + 0027 read as
  three sections of one decision about liveness.
- **ADR 0006 (`the-live-credential-is-captured-before-every-switch`)** is
  adjacent rather than the same: it is about not *losing* a Rotation, 0005 about
  not *causing* one under a live session. Related hazard, different act.

### 6. Citations

44 total — 10 in `docs/`, 34 in code and elsewhere. Files include
`src/observe.rs`, `src/switch.rs`, `src/probe.rs`, `src/profile.rs`,
`src/purge.rs`, `src/commands/{relogin,remove,watch}.rs`, `tests/{refreshing,
switching,watching,relogging_in,running,importing,purging,removing}.rs`,
`pages/src/content/docs/{status,watching,backup}.md`, `README.md`.

**Load-bearing.** The recurring phrase is *"the mid-task logout ADR 0005 exists
to prevent"* (`src/switch.rs:404`, `:623`, `:1025`, `:1472`,
`tests/switching.rs:905`, `tests/running.rs:679`), which explains *why* the
guard is where it is rather than tagging it. `src/observe.rs:660` and
`src/purge.rs:49` each carry the full argument. `src/commands/watch.rs:969` is
the one that reads most like a derivation: *"…so ADR 0005 permits Renewing it to
ask"* — the ADR being used to authorize an act, not to label one.

---

## ADR 0006 — The live credential is captured back into its profile before every switch


### 1. The decision, in one sentence

A Switch is three ordered steps — Capture the live Credential back into the
outgoing Account's Profile, write the incoming one to the live store, patch
`oauthAccount` — all under Claude Code's own OAuth refresh locks, because the live
copy is authoritative while it is live and skipping the Capture silently poisons
the Account being left.

### 2. Does it still hold?

**Holds**, and it is the most heavily depended-on ADR in the slice.

- The three steps, in order, inside `lock::under(host, probe::locks_for(&store),
  …)`: `switch.rs:467` (locks), `:485` `prepare`, `:488` `capture`, `:504`
  `store_credential`, `:512` `patch_identity`. The module doc at `switch.rs:1-29`
  restates the ordering argument and says *"The order is not a preference"*.
- Capture-first as unconditional: `switch.rs:979` — the alternative path
  (`make_live` without a Capture) is walled off and documented as such.
- Locks: `probe.rs:822` `locks_for` returns three specs all `held_by: "Claude
  Code"`.
- "That account needs a fresh login" → Quarantine as a from-the-start terminal
  state: `registry.rs` `Quarantine`, `credentials.rs:106-110` (*"'no Credential' is
  terminal — it Quarantines an Account (ADR 0006)"*), `commands/relogin.rs:5`,
  `observe.rs:850`, `observe.rs:869-875`.
- "A profile's stored credential is understood to be stale whenever that account is
  the active one": `observe.rs:542`, and `carry.rs:227-233` relies on the same fact
  for a different purpose.

Two facts about the current shape that the ADR's text does not describe, neither
of which contradicts it:

1. There is now a fourth write in the sequence — the **Landing**, recorded between
   the Capture and the Credential move (`switch.rs:490-500`, `write_it_down` at
   `switch.rs:537`). ADR 0048 introduced it and is emphatic that this does not
   disturb 0006 (see field 3).
2. The Capture's success line is no longer printed. ADR 0061 cut it, arguing from
   0006: *"ADR 0006 makes the Capture happen before every Switch without exception,
   which is exactly what makes [the line] predictable — it is the ordinary case
   announcing that it was ordinary."* The five non-ordinary outcomes still speak
   (`commands/switch.rs:205`–`258`). The behavior is unchanged; the output is not.

### 3. Supersession

Supersedes nothing. **Superseded by nothing, and one ADR goes out of its way to
say so.** ADR 0048:

> "**This supersedes nothing and amends nothing.** ADR 0006 is untouched: Capture
> before every Switch stands, the three steps stand, and its Consequences paragraph
> — a Credential rotated and lost before it can be Captured means a fresh login —
> stays true word for word. What changes is that Perch now refuses on the way into
> that state instead of causing it."

and

> "**Capture is untouched**, and nothing new gains an entry."

ADR 0048 frames its own contribution as *"this is not a decision being revisited,
it is one that was never made. ADR 0006 chose the three steps and said what an
interruption costs. Nobody ever chose what Perch does on *finding* an
interruption."* Treat the 0006 → 0048 edge as **additive, not supersessive** — but
note that the running code's step sequence is now four writes, so a merge that
kept only 0006's text would understate what `switch.rs` does.

ADR 0023 is the downstream half of 0006's Consequences: *"ADR 0006 leaves an
Account whose Credential cannot be recovered."* ADR 0009 leans on 0006 to argue
adoption adds no case. ADR 0061 edits only the prose.

### 4. Reasoning that outlives the document

> "Capture also settles the coherence question without any mtime or hash
> comparison: the live copy is authoritative while it is live, and it is written
> back at the one moment Perch controls. A profile's stored credential is
> understood to be stale whenever that account is the active one."

This is a general rule about where truth lives, and it is what `observe.rs`,
`carry.rs` and `commands/status.rs` all read the machine by. It would be the single
biggest loss if the document were deleted rather than merged.

And:

> "Quarantining an account in that state is therefore not a feature to defer past
> v1; it is the terminal state of this design and has to exist from the start."

### 5. Others deciding the same thing

- **ADR 0048** — same act, same file, adjacent instant; 0048 refuses to supersede
  but the two together are the complete description of a Switch, and neither is
  complete alone. If any pair in the 63 is a merge that has to be careful about
  whole-vs-partial, it is this one — 0048's refusal is explicit and must survive
  the merge as a statement about what was *not* revisited.
- **ADR 0023** — the repair for the state 0006 names as terminal. One lifecycle,
  two documents.
- **ADR 0020** — its "a successful write to the primary store removes any copy in
  the fallback" is 0006's poisoning argument applied to a second axis, and 0020
  says so: *"the reason is ADR 0006's."*
- **ADR 0005** — the other rule about not writing a Credential somebody is holding.

### 6. Citations

72 total: 12 in `docs/`, 60 in code and elsewhere. The widest spread in the slice —
`README.md`, `CHANGELOG.md`, `CONTEXT.md`, eight test suites, ten source modules,
two guide pages, and ADRs 0009, 0013, 0020, 0023, 0040, 0048, 0061.

**Load-bearing, and unusually so.** `observe.rs:814` marks the exact line that is
*"the one write ADR 0006 calls unrecoverable"*; `credentials.rs:106-110` uses it to
justify why an unreadable fallback must not be promoted to "no Credential";
`profile.rs:56` cites it for why the superseded copy is removed —
*"ADR 0006's silent poisoning, arriving by the back door"*; `commands/relogin.rs:64`
for why the liveness check comes before the browser round trip. These are not tags;
several of them are the only written reason a branch exists.

---

## ADR 0007 — Assumptions about Claude Code are probed at runtime, and Perch refuses when they fail


### 1. The decision, in one sentence
Every reverse-engineered belief about the installed Claude Code lives behind one
module that answers "what do we believe, and how confident are we?", every
dangerous operation is gated on its verdict, and a failed belief produces a named
refusal quoting the assumption and the Claude Code version rather than a silent
miswrite.

### 2. Does it still hold?
**Holds.** The module exists and says so in its own first line —
`src/probe.rs:1` *"What Perch believes about the installed Claude Code, and how
confident it is (ADR 0007)"*, and `src/probe.rs:5-8` *"They live here, in one
module, and nowhere else — so when Claude Code drifts, there is exactly one place
that stops recognizing it, and every dangerous operation is gated on the verdict
it returns."* The named assumptions are `src/probe.rs:24-33`
(`mod assumption`: `INSTALLED`, `ACCOUNT_NAME`, `CREDENTIAL_SHAPE`,
`CREDENTIAL_LOCATION`, `IDENTITY_BLOCK`, `SESSION_MARKER`). The refusal carries
both the assumption and the version: `src/error.rs:63`
`#[error("Perch declined to act: {assumption} ({detail}), Claude Code {version}")]`,
raised at `src/probe.rs:1393`, exiting `EXIT_PROBE_REFUSED = 10`
(`src/error.rs:21`, `src/error.rs:301`). `Installed` is a value probed once per
command precisely so the refusal can name a version (`src/probe.rs:51-70`,
`src/commands/switch.rs:58-64`).

The amendment at the top is also live and correct: there is no `contract`
feature and no contract suite; the weekly run is
`.github/workflows/ci.yml:16-17` (`schedule: cron "0 6 * * 1"`), with the
comment at `:13-15` *"`your_machine.rs` exists to find Claude Code drift before
a user does … A weekly run is what turns 'we would have noticed' into 'we
did'."* The suite is `tests/your_machine.rs`, gated by `your-machine`
(`Cargo.toml` `[features] your-machine`, `[[test]] name = "your_machine"`), run
at `ci.yml:215`.

One qualification: `Verdict` (`src/probe.rs:237`) has only two constructed
variants in the tree — `Verdict::Recognized` (`:1372`) and `Verdict::NoLogin`
(`:1388`). Most gating happens through the typed refusal rather than a verdict
match; the ADR's "gated on its answer" is realized as `Result<…>` refusals from
`probe::` functions (`src/login.rs:46-124`, `src/switch.rs:1122`), which is the
same thing operationally but not the shape the Consequences paragraph implies.

### 3. Supersession
Supersedes nothing. **Amended by ADR 0050, explicitly partial and in one
sentence** — its own header says so: *"**Amended by ADR 0050**, in one sentence
and with no supersession. … Everything else here stands: probing rather than
assuming, and refusing at runtime, is exactly as sound after that decision as
before."* ADR 0050's own header confirms the carry-out landed: *"the feature is
`your-machine`, the two surviving binaries are `tests/your_machine.rs` and
`tests/corroboration.rs`"*.

### 4. Reasoning that outlives the document
Two, both general:

> None is a public contract, and the drift is continuous

and the consequence, which is the architectural claim rather than the Claude
Code one:

> These assumptions cannot be scattered through the codebase as bare paths and
> struct definitions. They belong behind a single module that owns the question
> "what do we believe about the installed Claude Code, and how confident are
> we?", with every dangerous operation gated on its answer.

That second one has already been applied to a *second* upstream: `src/anthropic.rs:4-6`
— *"None of this is a published contract, so it is held the way [`crate::probe`]
holds Claude Code's internals (ADR 0007): one module carries every assumption"*.
The principle survives independent of Claude Code.

### 5. Others deciding the same thing
- **ADR 0020** (*a Credential lives wherever Claude Code puts it*) — the same
  activity: a reverse-engineered belief about Claude Code, established by reading
  a specific bundle version, recorded as a named assumption. 0020 is arguably one
  of 0007's assumptions written up at length (`CREDENTIAL_LOCATION`).
- **ADR 0022** (*a live Profile is corroborated by when its process started*) —
  likewise `SESSION_MARKER`, one of the six named assumptions, given its own file.
- **ADR 0050** — already the amender; a merge that folds 0050's consent rule into
  0007 would put the drift-catching mechanism next to the drift claim it serves.
- **ADR 0001 / ADR 0003** — the service-name hash and the `.claude.json` layout are
  named in 0007's opening list as things it governs.

### 6. Citations
23 total: 11 in `docs/`, 12 in code and elsewhere —
`tests/switching.rs`, `tests/your_machine.rs`, `src/probe.rs`, `src/anthropic.rs`,
`src/error.rs`, `src/commands/switch.rs`, `.github/workflows/ci.yml`,
`pages/src/content/docs/reference.md`, plus ADRs 0020, 0025, 0026, 0045, 0050,
0052 and `docs/agents/domain.md`.

**Load-bearing.** `src/error.rs:62` explains why the variant carries an
`assumption` field at all; `src/anthropic.rs:4-6` cites it to justify a module's
whole shape for a *different* upstream; `src/commands/switch.rs:59-63` cites it
to explain why the version is read once rather than twice. `ci.yml:13-15` is the
weakest and is still explanatory. None are bare tags.

---

## ADR 0008 — Keychain access shells out to `/usr/bin/security` rather than using a Rust crate


### 1. The decision, in one sentence
Because macOS anchors a keychain item's ACL to the creating binary, Perch drives
the same `/usr/bin/security` Claude Code drives — never a linked keychain crate
— and accepts four named constraints of that CLI (hex `-X` over `-i` stdin so no
secret reaches `argv`; a 4096-byte stdin buffer with an argv fallback; exit 44
means not-found and everything else means unavailable; `-w` returns hex, so ASCII
JSON only).

### 2. Does it still hold?
**Holds.** All five parts are in the tree.

- The binary: `src/keychain.rs:29` `pub const SECURITY_BIN: &str = "/usr/bin/security";`
  with the doc *"Never a build of Perch, never a crate — see ADR 0008."* Spawned
  at `src/host/real.rs:312-320` (`fn security`), reached from `keychain_get`
  (`:655`), `keychain_set` (`:676`), `keychain_delete` (`:717`).
- No keychain crate: neither `keyring` nor `security-framework` appears in
  `Cargo.toml` or `Cargo.lock`.
- `-X` over `-i`: `src/keychain.rs:100-107` builds
  `add-generic-password -U -s … -a … -X <hex>` and the whole line is `Zeroizing`.
- The 4096 fallback: `src/keychain.rs:36` `STDIN_BUFFER_LIMIT: usize = 4096`,
  `:41` `STDIN_SAFETY_MARGIN: usize = 256`, `WritePath::{Stdin, Argv}` at `:57-64`,
  taken at `src/host/real.rs:684-711`. Perch went one better than the ADR: the
  argv fallback now *tells the user as it happens* (`real.rs:695` — *"A
  Credential was too large for `security`'s stdin buffer, so it was …"*).
- Exit 44: `src/keychain.rs:32` `EXIT_ITEM_NOT_FOUND: i32 = 44`, `fn classify`
  at `:67-80` splitting `NotFound` from `Unavailable`, and
  `EXIT_KEYCHAIN_UNAVAILABLE = 11` at `src/error.rs:23`.
- `-w` hex: `src/keychain.rs:185` `fn decode_password_output`.

Two things the ADR did not have, which strengthen rather than contradict it:
`fn inert` (`src/keychain.rs:112`) refuses control characters in a service or
account name because `security -i` is line-oriented and reports a failed
sub-command on stderr while exiting 0 — a fifth constraint of the same kind. And
the split of protocol from spawning is now explicit: `src/keychain.rs:20-23`
*"What is here is the protocol and none of the spawning … The process itself is
started in [`crate::host::real`]."*

One clause is narrower than written: the ADR's premise that a Credential is in
*the* keychain is macOS-only now. `src/credentials.rs:51` reads
`Platform::MacOs => [keychain, plaintext]` — off macOS the keychain store never
holds anything (`src/credentials.rs:32-33`: *"The primary on macOS, and on every
other platform a store that simply never holds anything"*). That narrowing is
ADR 0020's, below.

### 3. Supersession
Supersedes nothing; nothing supersedes it. Two **partial** relationships, both
established from the other side:

- **ADR 0020 narrows its scope without superseding it**, in its opening
  sentence: *"ADR 0001 and ADR 0008 were written against a macOS-only Perch, and
  both assume a Credential is held in the operating system's keychain. That
  assumption is macOS's, not Claude Code's."* 0020 leaves 0008's argument intact
  and confines its subject matter to one platform.
- **ADR 0021 declares itself an explicit non-supersession** — see below.

### 4. Reasoning that outlives the document
> That binary never changes, so creator == reader across upgrades of both tools
> — which is also what lets Perch read the item Claude Code wrote in the first
> place.

The generalization it seeds is used across the repo: a shared resource whose
identity is anchored to the accessing binary must be reached by the same binary
the other party uses. ADR 0025 restates it as one of Perch's two seams —
*"Fidelity to what Claude Code actually does … a crate that is *nearly*
compatible is worse than code that is exactly compatible."*

Also worth carrying, because it is a fact about Perch's performance story rather
than about keychains:

> Part of the case for Rust was avoiding interpreter startup, and the credential
> path spawns a subprocess anyway. … the credential path will not be where Rust
> pays off.

### 5. Others deciding the same thing
- **ADR 0021 and ADR 0025** — the three are one dependency policy in three
  files. 0025's *"Not taken, and why"* already re-states 0008 (**`keyring`**)
  and 0021 (**`sysinfo`**, **`rpassword`**) in two paragraphs each, and 0021
  exists solely to reconcile itself with 0008. This is the strongest merge
  candidate in the slice.
- **ADR 0020** — decides where a Credential lives, which is the question 0008
  answers for one platform.
- **ADR 0001** — the service-name derivation `sha256(CLAUDE_CONFIG_DIR)[0:8]`
  is what 0008's `security` calls are addressed with.

### 6. Citations
32 total: 9 in `docs/`, 23 in code and elsewhere — `src/keychain.rs`,
`src/credentials.rs`, `src/profile.rs`, `src/service.rs`, `src/host/real.rs`,
`src/host/fake.rs`, `src/host/mod.rs`, `src/commands/service.rs`, `src/error.rs`,
`tests/storing.rs`, `tests/scheduling.rs`, `tests/switching.rs`,
`pages/src/content/docs/reference.md`, plus ADRs 0020, 0021, 0025, 0038, 0040, 0056.
The second-most-cited ADR in the slice.

**Load-bearing, and unusually far-traveled.** `src/host/mod.rs:548` cites it to
justify the existence of the `Keys` trait; `src/service.rs:24` cites it for a
conclusion the ADR never states — *"on macOS there is no unlocked keychain
before somebody logs in (ADR 0008) — so a system-wide arrangement would be a
Watcher with nothing it could read"*, i.e. why a Service is per-user and starts
at login; `src/host/mod.rs:110` cites it to explain why an HTTP header never
travels in `argv`, generalizing the `-X`/`-i` rule to `curl`;
`src/host/fake.rs:793` cites it to explain why the fake models locked-vs-absent
as two states. Not one of the four read is a bare tag.

---

## ADR 0009 — The existing login is adopted as the first profile


### 1. The decision, in one sentence

Perch never asks for a login it does not need: the Claude Code login already on the
machine is copied into a Profile and recorded active on first run, and every
Account after it is gained by driving a *fresh* login without touching the session
you are in.

### 2. Does it still hold?

**Holds**, with one mechanism drift in the Consequences paragraph.

- Adoption on first run: `adopt.rs:25` `ensure_adopted` / `:45`
  `ensure_adopted_exclusively`, both falling through to `adopt` at `:61` when
  `registry::load` returns `None`. Module doc at `adopt.rs:1-8` restates the
  two-copies-is-the-steady-state argument verbatim.
- Recorded as active: `adopt.rs:107` `registry.settle(Some(email))`.
- The CONTEXT.md carve-out ("bar an Import and a Purge") is real:
  `commands/import.rs:45` and `commands/purge.rs:68` call `registry::load`
  directly rather than `ensure_adopted`.
- Never logging out to add: `login.rs:1-13` — *"The active Account is never read,
  never written and never logged out by either of them."*

**Drift:** the ADR says *"Every profile after the first is created by launching
Claude Code inside the new profile's directory to log in there."* The login now
runs in a **scratch directory of its own** — `registry::pending_login_dir`
(`registry.rs:1681`, `perch_home/pending/login-<millis>`) — and what it produced is
moved into the Profile afterwards (`commands/add.rs:65`, then `:151`
`profile::create`). `commands/add.rs:3-7` states the ADR's sentence and then
corrects it in the next clause. The principle is intact and strengthened (an
abandoned login now costs a directory that is reaped, not a half-built Profile);
the named mechanism is not what the code does.

### 3. Supersession

Supersedes nothing, superseded by nothing, and nothing claims to amend it. Three
ADRs build on it without disturbing it:

- ADR 0023: *"a config directory of its own, exactly as `perch add` does (ADR
  0009)"* — the repair reuses the login mechanism.
- ADR 0014: *"runs (ADR 0009). Doing that here would make the machine hold one
  account on the way to giving one up"* — the Import/Purge carve-out, decided
  there rather than here.
- ADR 0017: *"Group at all (ADR 0009)"* — the adopted Account arrives Ungrouped.

The pending-login-directory change is uncaptured by any ADR I can find; it lives
only in `login.rs` and `commands/add.rs` comments.

### 4. Reasoning that outlives the document

> "A design that only ever snapshots the live credential has just one place to log
> in, so adding an account means first logging out of the one you are using —
> losing your session to gain an account. Perch never requires that."

This is a general claim about why Profiles are directories rather than a token
store, independent of adoption, and it is the argument `login.rs` and `add.rs` both
open with.

And the smaller one, about not adding a case:

> "that is exactly the state ADR 0006 exists to handle — it is where every Switch
> leaves things — so adoption does not add a case, it starts the system in its
> steady state."

### 5. Others deciding the same thing

- **ADR 0023** — `perch relogin` and `perch add` are the same login mechanism
  pointed at two outcomes; 0023 says so in its own text. The rule "a login runs in
  a directory of its own and the active session is never touched" is decided in
  0009 and reused in 0023, and it is currently written down accurately in neither.
- **ADR 0014** — decides the two commands that are *exempt* from adoption, which is
  a clause of 0009's rule living in another document (and in CONTEXT.md's
  **Adoption** entry, which is where the exemption is actually spelled out).
- **ADR 0001** — 0009 says it *"falls out of profiles being real config
  directories (ADR 0001)"*; the two share a premise but decide different things.

### 6. Citations

13 total: 4 in `docs/`, 9 in code and elsewhere — `CONTEXT.md`,
`tests/relogging_in.rs`, `tests/watching.rs`, `src/adopt.rs`, `src/login.rs`,
`src/profile.rs`, `src/commands/export.rs`, `src/commands/add.rs`,
`src/switch.rs`, `pages/.../accounts.md`, plus ADRs 0014, 0017, 0023.

**Load-bearing.** `adopt.rs:1` and `login.rs:5` are module charters; `profile.rs:3`
uses it to explain why both entry paths converge on one read-back guard;
`commands/export.rs:40` cites it for the one case where adoption must *not* fire.
The `pages/.../accounts.md:34` cite is attributive.

---

## ADR 0010 — Running a client against a profile is a one-shot command


### 1. The decision, in one sentence

`perch run <target> [-- …]` points one process at a Profile with `CLAUDE_CONFIG_DIR`
and nothing else — no subshell mode, no shell-eval — with `--` mandatory before any
passthrough, the first word after `--` deciding totally what runs, and the Run path
being the only one that Reconciles and Carries.

### 2. Does it still hold?

**Holds**, clause for clause.

- One process, environment scoped to it: `commands/run.rs:148-152`
  `exec_interactive(..., &[("CLAUDE_CONFIG_DIR", &profile…)])`, with the module doc
  at `:1-13` stating nothing is held for the client's lifetime.
- No subshell form and no `perch env`: neither appears in `main.rs`'s command enum
  (the 15 top-level commands are Add, Alias, Config, Disable, Enable, Group,
  Holdings, Relogin, Remove, Run, List, Switch, Status, Upgrade, Watcher).
- `--` mandatory, refused off the raw command line before clap sees it, with the
  parser's own exit code 2: `commands/run.rs:227`
  `refuse_a_flag_without_the_separator`, wired at `main.rs:354`;
  `PerchError::NotUnderstood` → `EXIT_NOT_UNDERSTOOD = 2` (`error.rs:19`, `:303`).
  The refusal prints the line that would have worked
  (`commands/run.rs:371-415` tests).
- First word decides totally, `-` means Claude Code's argument:
  `commands/run.rs:183` `what_to_launch`. The empty string is treated as a leading
  `-` would be — a case the ADR did not anticipate and the code names.
- "Only Claude Code is looked for by the probe": `commands/run.rs:195-201` —
  `probe::claude_bin` is reached only in the Claude Code arm.
- The only path that Reconciles and Carries: `commands/run.rs:113` `reconcile`,
  `:126` `carry`; `reconcile.rs:5` cites 0010 for exactly that; `switch.rs:20-23`
  states the negative (*"A Switch needs none of it"*).

### 3. Supersession

Supersedes nothing, superseded by nothing, amended by nothing. **ADR 0027 extends
it** — a Run now also writes its own liveness Marker — but 0027 presents that as
closing a gap rather than as revisiting 0010, and never names 0010 as amended.

Two ADRs take 0010 as a premise: ADR 0026 (*"A Run launches a client against a
Profile that is a live configuration directory rather than storage (ADR 0010), so
every piece of Shared State has to be reachable from it"*) and ADR 0011.

Note a stale *inbound* cite outside my slice: ADR 0011 says *"the client is
launched into it (ADR 0010). `perch tui` therefore exits with the…"* — `perch tui`
was killed by ADR 0049, so 0011's sentence is dead around a live 0010 cite.

### 4. Reasoning that outlives the document

> "It is a second way to do the same thing, and a mode a user can be in without
> remembering they entered it."

— the rejection of the subshell form, and a general test Perch can apply to any
future stateful mode.

And the argument that makes the `--` rule non-arbitrary:

> "Nothing is guessed, because nothing beginning with `-` can name a program the
> operating system would find — `PATH` is searched for names, a path is written
> with a `/`, and a file called `-resume` is reached as `./-resume`."

This survives verbatim at `commands/run.rs:172-180`.

### 5. Others deciding the same thing

- **ADR 0027** — the same command, and the half 0010 left undecided (what makes the
  Profile a Run launches untouchable while it lasts). 0010 + 0027 is the full
  design of `perch run`; neither is complete alone.
- **ADR 0026** and **ADR 0003** — everything the Run path has to do to the Profile
  *before* the launch, split between "by link, denylist" and "by copy, named set".
  Four documents (0010, 0026, 0003, 0027) currently describe one command.
- **ADR 0011** — the other half of the "which command chooses for you" split, and
  it cites 0010; but its `perch tui` clause is dead, so it should be read before
  merging in either direction.

### 6. Citations

12 total: 4 in `docs/`, 8 in code and elsewhere — `tests/running.rs`,
`src/reconcile.rs`, `src/switch.rs`, `src/host/real.rs`, `src/commands/run.rs`,
`pages/.../running.md`, plus ADRs 0001, 0011, 0026.

**Load-bearing.** `commands/run.rs:1` is the module charter; `:172` and `:227` cite
it as the specification of two specific parsing rules; `reconcile.rs:5` uses it to
say why Reconcile exists at all. The lowest citation count in the slice, which
reflects the decision being concentrated in one module rather than being
uninfluential.

---

## ADR 0011 — `perch switch` chooses for you, and the picker is a separate command


### 1. The decision, in one sentence
Bare `perch switch` Cycles within the current Account's Group and asks nothing,
`perch switch <target>` names one explicitly, the interactive picker is a
separate `perch tui` confined to switch-and-run, and — the clause that turned out
to matter most — every capability the picker offers must also exist
non-interactively so Perch is complete over SSH, in scripts and in CI.

### 2. Does it still hold?
**Holds in part.**
- Live: bare `perch switch` Cycles and asks nothing —
  `src/commands/switch.rs:10-13` ("With no Target the Account is chosen rather
  than named … It asks nothing, under any circumstances (ADR 0011)"), enforced by
  `tests/cycling.rs:390-398` against `Effect::Asked`, which
  `src/host/fake.rs:103-105` exists to record ("a command that must never ask one
  — bare `perch switch` (ADR 0011) — can be held to it").
- Live: the non-interactivity constraint, which is now the *only* thing anything
  cites 0011 for — `src/main.rs:74-75`, `src/commands/config.rs:4-5`,
  `src/commands/purge.rs:184-186`, `tests/configuring.rs:3-4`,
  `tests/removing.rs:369`, `tests/purging.rs:515`.
- Live but re-attributed: the listing job (Cycle order, Headroom beside it,
  Ungrouped held rather than ranked) is implemented in `src/listing.rs:57-97`
  (`Section { ranked }`, `"ranked"`/`"held"`) and `src/commands/list.rs:1-16`,
  which credits ADR 0012, 0017 and 0049 and never 0011.
- Dead: `perch tui` and everything about it. No `tui` in `COMMANDS`
  (`tests/invoking.rs:150-153`), no `src/tui`, no ratatui. So the whole third and
  fourth paragraphs — the picker acting on exactly two things, `add`/`remove`/
  `purge`/`config` staying out, exiting with the client's status, a switch inside
  the frame loop held back while a refresh is out — describe a subsystem that
  does not exist.

### 3. Supersession
Supersedes nothing. Superseded by **ADR 0049, partially and explicitly** — the
banner ADR 0011 carries states the split itself:

> **Superseded by ADR 0049.** … What is void is the half this document could not
> argue for: it ruled the picker out of every moment it named, then gave it a
> command anyway "for when the choice wants making by eye rather than by rule",
> and never said when that moment was. **Two halves are carried forward rather
> than repealed.** The first is the decision in the title — bare `perch switch`
> Cycles and `perch switch <target>` names, which 0049 leaves as the whole of
> choosing. The second is the job the listing below describes, which outlived its
> surface…

Note that ADR 0049's own body never says "supersedes ADR 0011"; it supersedes
ADR 0042 in full and ADR 0016 in part, and argues 0011's picker clause away
(`docs/adr/0049…:12-27`). The supersession edge is recorded only on 0011's side.
ADR 0047 adds a re-affirmation that is *not* a supersession: "**ADR 0011 is
re-affirmed without touching its text.** Its constraint — every capability the
interactive view offers must exist non-interactively … — is untouched and still
governing."

### 4. Reasoning that outlives the document
Two, and both are already load-bearing elsewhere:

> every capability it offers must exist non-interactively too — Perch has to be
> complete over SSH, in scripts, and in CI.

> Switching accounts happens when quota runs out, which is mid-task and under
> mild frustration. The shortest command should therefore do the whole job

The second is the premise ADR 0047 leans on for its frequency tiebreak. The first
has ten citation sites in code and tests and no other home: if this document dies
without that sentence being rehoused, six comments cite a deleted record for the
rule they exist to obey.

### 5. Others deciding the same thing
- **ADR 0049** is the obvious partner — it is the document that killed the picker
  and took custody of the listing job, and 0011's two surviving halves are stated
  in 0049's own prose (`:65-69`, `:123-137`). A merge is one document about "what
  chooses, and what shows the choice".
- **ADR 0012** owns the ranking 0011's first clause invokes; 0011 does not decide
  the ranking, it decides who invokes it.
- **ADR 0017** owns held-rather-than-ranked, which 0011 restates.
- **ADR 0047** re-affirms the non-interactivity constraint by reference; if 0011
  merges anywhere, 0047's re-affirmation has to be re-pointed.

### 6. Citations
32 total — 22 in `docs/`, 10 in code and tests. Files: `tests/configuring.rs`,
`tests/removing.rs`, `tests/purging.rs`, `tests/cycling.rs`, `tests/invoking.rs`,
`src/main.rs`, `src/commands/switch.rs`, `src/commands/config.rs`,
`src/host/fake.rs`, `src/commands/purge.rs`, plus
`pages/src/content/docs/{configuration,switching}.md` and ADRs 0014, 0016, 0024,
0034, 0042, 0047, 0049.

**Load-bearing, uniformly — and all of one clause.** Every code and test site
cites 0011 for either "asks nothing" or "complete over SSH and in CI", never for
the picker. `src/host/fake.rs:103-105` justifies the existence of an entire
`Effect` variant by it; `src/commands/purge.rs:184-186` derives the shape of a
refusal message from it. Not one citation depends on anything ADR 0049 voided.

---

## ADR 0012 — Cycling ranks accounts by their most constrained window


### 1. The decision, in one sentence
An Account's headroom **is** its worst Quota Window and the Account whose worst
is best wins — which fixes how headroom is *measured* and deliberately leaves
*which Account to prefer* as a separate, configurable axis on top of it.

### 2. Does it still hold?
**Holds.**

- `src/cycle.rs:5-16` states both halves as the module's first rule, including
  the separation of measurement from Strategy: *"A Strategy reorders the
  candidates and cannot promote one that the measurement rules out."*
- `src/cycle.rs:266` `headroom_of`, `src/cycle.rs:395` ("The measurement ADR 0012
  fixes: the worst Quota Window"), `src/cycle.rs:114` `Headroom::ranking` taking
  a `Strategy` as an argument to *reorder*, never to re-measure.
- The Consequences hold too. "Cycling skips accounts that are exhausted,
  disabled, or quarantined" — `src/cycle.rs:510` filters exhausted,
  `cycle::is_a_candidate` filters disabled and quarantined. "When every account
  in the group is exhausted, Perch picks nothing and says so, naming which
  account resets soonest" — `src/cycle.rs:491-495` → `everyone_is_exhausted` at
  `:925-965`, which picks the soonest *future* reset and counts the ones it
  cannot name.
- The decision has grown a defensive perimeter it did not ask for:
  `src/anthropic.rs` cites it seven times (`:410`, `:446`, `:479`, `:496`,
  `:761`, `:782`, `:814`, `:854`) as "the ADR 0012 failure" — a window Perch
  silently drops instead of counting, which would make an Account's *worst*
  window invisible and hand the ranking a number that is true of only some
  windows. That is 0012's claim enforced at the parsing boundary.

**One clause has decayed.** The closing Consequence — "The ranking gives *the
picker* an honest single number per account, with the per-window detail beneath
it" — names a subsystem that no longer exists. `perch tui` and its picker were
removed entire by ADR 0049 (`docs/adr/0049…:8-9`, "all 8,418 lines of it"), and
the surface it describes is now `perch list` (`src/commands/list.rs:10`, "The
rows come out in the order a Cycle ranks them (ADR 0012)"). The *content* of the
sentence survived the move; only the noun is dead.

### 3. Supersession
**None in either direction.** No banner on the file, and nothing in the other 62
supersedes it. It is one of the load-bearing undisturbed ADRs.

### 4. Reasoning that outlives the document
> "Being blocked by any window blocks you completely, so this is the only ranking
> that measures what actually stops you working: when Perch reports 40% headroom,
> that is true of every window, and nothing surprising blocks you five minutes
> later."

and, separately — the sentence that keeps Strategy from eating the measurement:

> "This fixes how headroom is *measured*, not which account to prefer. […] every
> such strategy reads headroom the same way."

Both are quoted almost verbatim into `src/cycle.rs:5-16` already, so a survivor
that carried them would lose nothing.

### 5. Others deciding the same thing
- **ADR 0046** ("The Watcher's numbers are arithmetic, and only the Threshold is
  a preference") — the same distinction, one layer up. 0012 separates the
  measurement (fixed) from the preference (`strategy`); 0046 separates the
  watcher's pacing arithmetic (constants in `src/watch.rs`) from the one genuine
  preference (`watcher-threshold-percent`). Two applications of *"arithmetic is
  not taste"* to two subsystems; a merge would be a real consolidation rather
  than a filing convenience.
- **ADR 0058** — the Reserve is explicitly built on top of 0012's measurement
  (`src/reserve.rs:3-8`) and is the argument for why the *Scope* has no
  equivalent single figure. 0012 and 0058 together are "one honest number per
  Account, and no honest number per Scope", which reads as one idea stated from
  two ends.
- **ADR 0011/0049** for the "picker" clause specifically.

### 6. Citations
34 total — 9 in `docs/`, 25 in code and elsewhere, across `README.md`,
`tests/{listing,configuring,cycling,common}.rs`, four other ADRs,
`src/{watch,cycle,anthropic,config,registry,reserve}.rs`,
`src/commands/list.rs`, and three guide pages.

**Load-bearing, and unusually so.** The `src/anthropic.rs` cluster is the clearest
case in this slice: `:479` (*"is precisely what ADR 0012 exists to refuse"*),
`:496` (*"the ADR 0012 failure above arriving as a `null` instead of as a `98`"*)
and `:854` explain why a parser refuses drift rather than tolerating it — the
code's shape is unreadable without the ADR. `src/config.rs:208` and `:214` use it
to write the user-facing sentence explaining what a Strategy can and cannot do.
Nothing sampled was a bare tag.

---

## ADR 0013 — Unattended switching is a foreground watcher, not a daemon


This is the densest supersession knot in the corpus and the most-cited ADR (91
sites). Fields 2 and 3 below are unusually long because the exact boundaries are
the point.

### 1. The decision, in one sentence

The title's claim is dead; what the record actually decides, and still decides,
is the Watcher's **arithmetic and reporting contract** — poll only the active
Account, every two and a half minutes because that is what Anthropic's ~28-30
reads an hour allows, never act on a figure you did not just refresh, back off
by doubling to twenty minutes on a failed Refresh, pace Switches by a cooldown
that lives in the loop's memory (and in the registry for a scheduled Check,
per Group), set candidates aside by a margin before the Strategy ranks them, and
report what a round decided in a fixed exit-code table.

### 2. Does it still hold?

**Holds in part**, and the split is not the one the title suggests.

**Live, with cites:**

- *Polls only the active Account.* `commands::watch::one_round` Refreshes the
  active Account alone; candidates are only reached through `considered`
  (`src/commands/watch.rs:1209`), which cannot be called without both `Cooled`
  and `Idle` witnesses — so a candidate Refresh at any other moment does not
  compile (ADR 0055).
- *Two and a half minutes, and it is a constant.* `watch::REFRESH_INTERVAL_MILLIS
  = 150_000` (`src/watch.rs:45`), whose doc reproduces the allowance arithmetic
  and cites ADR 0013 by name.
- *Back-off doubles from the interval, bounded at twenty minutes, dropped whole
  by the first Refresh that works.* `watch::LONGEST_WAIT_MILLIS = REFRESH_INTERVAL_MILLIS * 8`
  (`src/watch.rs:57`) and `Backoff::could_not_read` (`src/watch.rs:117`).
- *Never acts on a figure it did not just refresh.* A failed Refresh produces
  `Outcome::Held` with no `Fullest` on the `Round` at all — the figure is
  `Option` and absent is "the whole of why nothing was decided"
  (`src/watch.rs:~890`).
- *The cooldown lives in the loop; a Check records it against its Group.*
  `watch::Recently` in memory (`src/watch.rs:~555`) versus `registry::Checked`
  (`src/registry.rs:517-541`), whose doc comment is 0013's paragraph condensed
  and cites it.
- *The margin sets candidates aside before the Strategy ranks them.*
  `Policy::ceiling` (`src/watch.rs:402`) feeding `set_aside`, with
  `src/cycle.rs:536`, `:607`, `:1594` naming "the failure ADR 0013 sets
  candidates aside to prevent".
- *A Switch that changed nothing is a decision; one that moved and then failed
  stops the loop.* `Err(NotSwitched { error, moved: true }) => Err(error)`
  (`src/commands/watch.rs:1159`), answered before every other arm.
- *The exit-code table.* `src/error.rs:12-57`: `0`, `14`, `15`, `16`, `17`,
  `18`, `19`, `20`, with `EXIT_HELD`'s doc quoting 0013's own reason for `20`
  existing separately from `17`. A Check returns `round.outcome.exit_code()`
  (`src/commands/watch.rs:152`).
- *Decision log to standard output.* `src/watch.rs:19-21`, and the loop's
  `say_it`.
- *The grant is off by default.* `Settings::watcher_may_act: false`
  (`src/registry.rs:349`).
- *"Scheduling is the operating system's job."* Alive and explicitly
  re-affirmed by ADR 0040; `src/service.rs:5-8` says Perch "writes a unit and
  hands the job over".

**Dead:**

- *The title, and the rejection of a managed background daemon.* `perch watcher
  install` writes a real unit (`src/commands/service.rs:36`,
  `src/service.rs:14-20`).
- *"It leaves nothing behind."* The loop takes a lock and holds it
  (`registry::watcher_lock_spec`, `src/registry.rs:1783`; `take_the_watch`,
  `src/commands/watch.rs:350`). The *file* half survives — `src/watch.rs:559`
  still says `perch watcher run` "writes no file of its own".
- *The loop stops on a withdrawn grant (`14` / `18`).* Now a `Held` round:
  `Ok(Turn::NotArranged(why)) => …Spoken::held(…)` (`src/commands/watch.rs:243-250`),
  with the ADR 0040 reasoning inline. A Check still exits `14`/`18`
  (`src/commands/watch.rs:145-152`).
- *"Every held round says which failure held it and when the watcher will ask
  again."* Coalesced: `watch::Speak` / `watch::Holding` with
  `STILL_HOLDING_MILLIS = 3_600_000` (`src/watch.rs:156-290`).
- *"It never acts on an ungrouped account."* **This one is dead and no ADR
  records it as superseded.* `commands::watch::permitted`
  (`src/commands/watch.rs:653-683`) admits the Ungrouped Scope on two yeses —
  `interchangeable` plus `watcher-may-act` — and the guide text in
  `src/commands/watcher.rs:32-36` says so. 0013's stated reason ("the second
  has no owner when there is no group to carry it") stopped being true when ADR
  0017's amendment made the Ungrouped Accounts a Scope and ADR 0051 gave that
  Scope Settings outright. Neither 0017's amendment nor 0051 names ADR 0013.
- *The cooldown, margin and no-return as per-group Settings.* Constants and
  deletion respectively (ADR 0046; see below), pinned by
  `tests/configuring.rs:390-419`.

**Stale but not decayed** (ADR 0047's rule): every path in the record — `perch
watch`, `perch watch --once` — is now `perch watcher run` and `perch watcher
check` (`src/main.rs:748-780`).

### 3. Supersession

0013 supersedes nothing. It is superseded **in part, twice**, and the two parts
are declared with different words and do not tile cleanly.

**(a) By ADR 0040 — partial, and named clause by clause.** 0040's opening
sentence:

> "Supersedes the part of ADR 0013 that rejected a managed background daemon.
> The rest of that record stands — the interval, the back-off curve, the
> cooldown, the margin, the no-return, the exit-code table and the reasoning
> behind every one of them are unchanged and are still where they were written."

0013's own header enumerates what 0040 repealed:

> "Three things below were repealed and are named here so nobody implements them
> from this record: the loop no longer **stops** when a grant is withdrawn — `14`
> and `18` become a `Held` round … a Watcher now **takes a lock** and is one per
> person per machine, so 'it leaves nothing behind' is no longer the whole truth;
> and an identical hold is now said **once an hour** rather than every round …
> `perch watch --once` keeps `14` and `18` unchanged."

So 0040's supersession is four clauses: the daemon rejection, the two permission
exits *for the loop only*, "leaves nothing behind", and the every-round hold
line. 0040 explicitly preserves "Scheduling is the operating system's job" —
*"survives intact, and this record agrees with it harder than ADR 0013 did"*
(0040:34).

**(b) By ADR 0046 — one whole section, and a refusal to take more.** 0046's
closing:

> "**This supersedes the whole of ADR 0013's 'Amended: the numbers this asked
> for' section, and nothing else in that record.** That section is the part that
> has decayed, and it is cleanly severable."

And the refusal, which is the load-bearing sentence:

> "Superseding ADR 0013 whole was refused. It would restate a mostly-correct
> record at length, and a third correction header on one file is the thing this
> map's yardstick exists to refuse: a reader would need three passes to learn
> what the Watcher does. Superseding the amendment alone puts the Watcher's
> numbers in one place — the interval, the back-off, the cooldown, the margin,
> the threshold, and which single one of them is anyone's to set."

0013's in-file header agrees on the extent but disagrees on the *effect*:

> "**Superseded in full by ADR 0046.** This section — and only this section — is
> no longer the governing record. Everything above it stands exactly as
> written. … Nothing else below moved. The interval and why it is a constant,
> the back-off curve, the cooldown living in the loop while a `--once` Check
> records it against its Group, the ungrouped refusal, both grants read every
> round, and the exit-code table are all still this record's."

**The two documents contradict each other on where the boundary falls.** 0046
says *"Everything **above** it stands and is still governing: the rejection of a
managed daemon, polling only the active Account, the two-and-a-half-minute
interval and why it is a constant, never acting on a figure it did not just
refresh, the Margin setting candidates aside before the Strategy ranks them, the
cooldown living in the loop and a `--once` Check recording it, never acting on an
ungrouped Account, both grants read every round, and the whole exit-code table."*
Seven of those nine clauses are **not** above the amendment heading — they are
paragraphs *inside* the section 0046 has just declared superseded in full
(interval at 0013:90, back-off at :112, cooldown placement at :146, `--once`
record at :153, ungrouped at :189, both grants at :198, exit codes at :217).
Read literally, 0046 kills the interval and the exit-code table in one sentence
and preserves them in the next. 0013's own header ("Nothing else below moved")
is the reading that matches the tree, and is the one to carry forward.

Two of the nine clauses 0046 listed as surviving are, in fact, false:

- *"the rejection of a managed daemon"* — already superseded by ADR 0040, which
  is **earlier** than 0046. 0046 re-affirmed a clause that had been dead for six
  ADRs.
- *"never acting on an ungrouped Account"* — false in the tree
  (`src/commands/watch.rs:653`), by way of ADR 0017's amendment and ADR 0051.

**What is left standing after both.** 0013 remains the governing record for:
polling only the active Account; the 2.5-minute interval and the allowance
arithmetic that makes it a constant; never acting on an un-refreshed figure and
why the Watcher diverges from 0015/0018 here; the back-off curve, its bound and
its whole-drop; the cooldown living in the loop's memory while a Check records
it per Group in the registry; the margin setting candidates aside *before*
ranking (mechanism only — 0046 replaced its rationale); the exit-code table
including the three outcomes that deliberately share `15`; the decision log
going to standard output; "scheduling is the operating system's job"; and
"everything the watcher does is a Switch" (Capture first per 0006, Claude Code's
locks, never a live Profile's token per 0005). Every claim in the record's
**title** is gone.

### 4. Reasoning that outlives the document

Several, and they are the reason a merge cannot be a deletion.

The preference-versus-arithmetic test, which ADR 0046 later turns back on 0013
itself and which is now the repo's yardstick for any Watcher number:

> "It is a constant rather than a setting: it is derived from Anthropic's
> allowance rather than from anyone's preference, and a group configured to poll
> every ten seconds would be a group configured to be refused."

Why the Watcher is the one surface that will not act on cache — a claim about
the difference between showing and acting:

> "This is the one place the watcher diverges from every other surface. … those
> surfaces show a person a number they will judge for themselves. The watcher
> shows nobody anything; it acts."

The asymmetry between a held decision and a wrong one:

> "a held decision costs nothing, and a wrong switch costs a capture, a
> credential write, and possibly an account more exhausted than the one it left."

Why an unreadable reply is not a reading of zero:

> "A reply that arrived but carries no quota window perch can read is a failed
> refresh too, and not a reading of zero — an account with nothing used is the
> one reading that can never be over any threshold."

Why a back-off is bounded rather than unbounded:

> "the endpoint coming back does not announce itself and the only way the
> watcher finds out is by asking — a loop that had doubled its way to an hour
> would come back long after the crossing it was left running for."

Why two grants are two grants:

> "Permission to switch when asked and permission to switch while nobody is
> looking are different grants."

(Still true, and still implemented — only 0013's conclusion from it, that the
Ungrouped Scope cannot carry the second, has fallen.)

Why three outcomes share exit `15`:

> "A code per outcome would be a distinction nobody could act on."

### 5. Others deciding the same thing

- **ADR 0040 and ADR 0046** are its two partial successors and are the obvious
  merge set: 0013's surviving arithmetic + 0046's constants + 0040's
  arrangements is one record of "what the Watcher does". 0046 refused this
  merge, on the grounds that a third correction header is worse than a partial
  supersession — that refusal is itself evidence for the merge ticket rather
  than against it, since a *rewrite* (as opposed to a third header) answers the
  objection.
- **ADR 0047** (`a-command-names-the-noun-it-is-about…`) collapsed the three
  arrangements onto one noun and is where the paths in 0013 went. It is a naming
  decision, not a Watcher decision — adjacent, not a merge partner.
- **ADR 0012** (`cycling-ranks-accounts-by-their-most-constrained-window`) owns
  the ranking 0013's margin filters into. Different question (how to rank vs.
  when to move), so not a merge.
- **ADR 0018 / ADR 0015** are the surfaces 0013 defines itself *against*; the
  contrast is load-bearing to all three and would have to survive any merge.

### 6. Citations

**91 total — 29 in `docs/`, 62 in code and elsewhere.** The most-cited ADR in
the corpus. Files: `src/{watch,switch,registry,service,cycle,probe,config,error,
main}.rs`, `src/host/mod.rs`, `src/commands/{watch,watcher,service,switch}.rs`,
`tests/{watching,scheduling,servicing,configuring,grouping}.rs`,
`pages/src/content/docs/{watching,reference}.md`, and six other ADRs.

**Load-bearing, overwhelmingly.** Four sites read:

- `src/watch.rs:44` — *"Said here, because this is the number it is the reason
  for (ADR 0013)"*, attached to `REFRESH_INTERVAL_MILLIS`; the ADR is being used
  to justify where a constant lives.
- `src/error.rs:53-56` — `EXIT_HELD`'s doc reproduces 0013's argument for `20`
  existing apart from `17`: *"a scheduler retrying shortly needs to tell the two
  apart — only one of them resolves itself."*
- `src/switch.rs:90-97` — the `Reason` enum's doc cites 0013's *"cooldown that
  did not survive the process"* to explain why the Check record and the Switch
  must reach the registry in the **same save**; the citation carries the
  ordering constraint.
- `src/probe.rs:527` — *"the systemd timer that ADR 0013 says is where an
  unattended watcher belongs"* explains why an unset `USER` must not fail every
  command.

A minority are closer to decorative — `src/config.rs:353` and `:545` tag
`watcher-may-act` with "(ADR 0013)" and no argument, and
`tests/configuring.rs:14` uses it as a pointer to another suite. But the balance
is heavily explanatory, and several sites (the `same save` one especially) are
the only written record of an ordering the code depends on.

---

## ADR 0014 — Export is encrypted with a required passphrase


### 1. The decision, in one sentence

The Holdings go out as one armored `age` file sealed with a passphrase that is
required, typed at a terminal and never a flag; the file is all-or-nothing in
both directions, so an Import refuses a machine that holds an Account, adopts
nothing, activates nothing and takes back what it placed on failure, and a Purge
offers to write one first, demands the word `purge`, takes Credentials before
the home, and refuses while any Profile is Live.

### 2. Does it still hold?

**Holds** — every clause of the base record and all four amendments is
implemented, and the amendments are where nearly all the decision now lives.

- age crate, passphrase mode, armored: `Cargo.toml:67` (`age = { version =
  "0.12.1", default-features = false, features = ["armor"] }`) and
  `src/export.rs:388` (`age::encrypt_and_armor(&recipient(passphrase), …)`). The
  work factors are pinned at `src/export.rs:36-69` with the ADR's own
  outlive-the-machine reason quoted: "ADR 0014 wants this file to outlive the
  machine that wrote it, and it cannot do that while what opens it depends on
  the machine that opens it."
- Passphrase at a terminal, no flag, prompted twice: `src/commands/export.rs:45`
  (`refuse_without_a_terminal(host, "perch holdings export")`) and
  `agreed_passphrase` at `src/commands/export.rs:171-177`. Echo suppression is a
  Host primitive (`src/host/mod.rs:657-659`, citing 0014).
- Nothing is written over: `refuse_an_occupied_path` (`src/commands/export.rs:135`),
  asked once before the passphrase and again immediately before the write
  (`:47`, `:101`).
- Import refuses a non-empty registry, adopts nothing, activates nothing, rolls
  back: `src/commands/import.rs:9-16` states all three in the module header;
  `import::refuse_a_machine_that_is_not_empty` at `src/commands/import.rs:46`;
  rollback is `Placed`/`Placed::undo` (`src/import.rs:128-132`, `:350-379`).
  Passphrase prompted once here, per the amendment.
- Purge: `THE_WORD: &str = "purge"` (`src/commands/purge.rs:39`), `--yes`
  (`src/main.rs` `Holdings::Purge`), the shared export path
  (`commands::export::write_the_export`, `src/commands/export.rs:74`, whose doc
  cites 0014 for why purge cannot go through `run`), Credentials first and the
  home last (`src/purge.rs:11-15`, `erase` at `:177-201`), and the Live-Profile
  refusal (`src/purge.rs:48-93`, `refuse_while_anything_is_running`) asked before
  the questions and again after (`src/commands/purge.rs:142`).
- Verified section: the CI half is real — `tests/exporting.rs` is one of the
  suites carrying the armor-header and scrypt-recipient assertions.

One drift, cosmetic rather than substantive: the ADR spells the commands `perch
export`, `perch import`, `perch purge`, and they are now `perch holdings export |
import | purge` (ADR 0047's noun, ADR 0054's shape). Every refusal message in the
code uses the new spelling; the ADR's text does not.

### 3. Supersession

Supersedes nothing and is superseded by nothing. It has grown by four *Amended*
sections instead — "the file is an age file, and it holds the whole machine";
"the file is armored, and the passphrase is typed at a terminal"; "an import
adopts nothing, lands nothing, and leaves nothing behind"; "a purge asks for the
word, keeps the live login, and finishes" — plus a *Verified* section. The
amendments explicitly frame themselves as filling in what the base left open:
"This record required a passphrase and named no primitive, and said nothing about
what goes in the file or what happens on the way back in."

ADR 0021 carries a reciprocal amendment rather than a supersession: "`perch
export` prompts for a passphrase, and a passphrase must not be echoed as it is
typed (ADR 0014) — which the portable standard library has no way to ask for."

### 4. Reasoning that outlives the document

Several, and they are the general kind:

- *"a selective export is a partial restore, which is the failure mode this
  record exists to prevent, wearing a feature's clothes."* This sentence is
  quoted back, nearly verbatim, at three separate defensive checks in
  `src/import.rs` (`:216`, `:630`, `src/export.rs:103`) — it has become the
  repo's name for a class of bug.
- *"a backup readable only by the tool that wrote it is a worse backup than one
  whose format somebody else maintains."*
- *"a value passed as an argument sits in `argv` where any process on the machine
  can read it off the process table, and a value in a shell history outlives the
  command that used it"* — stated here as the general rule an access token also
  travels under.
- *"requiring one is a check that checks nothing"* (on why Purge offers an Export
  rather than demanding one).
- *"a second caller is exactly how a check like that comes to be made in one
  place and not the other."*

### 5. Others deciding the same thing

- **ADR 0025** (a crate is taken where it does not cost a seam). 0014's
  format decision is an application of 0025's test and says so; `Cargo.toml:62`
  reproduces the argument. One decision reasoned twice, not two decisions.
- **ADR 0021** (platform primitives are linked rather than shelled out). Its
  fourth-primitive amendment exists only because of 0014's no-echo rule; the
  argv/process-table rule is stated in both.
- **ADR 0047** (`perch holdings` as the noun the three verbs sit under) and
  **ADR 0054** (naming). 0014 is where "none of the three takes a Target" is
  argued and `src/commands/holdings.rs` is where it is asserted — the noun ADR
  and the policy ADR describe one command family.
- **ADR 0057** (one door for registry-only commands) supplies the other half of
  0014's lock reasoning: `still_ours` exists for `remove`, `purge`, `import` and
  `export` precisely because 0014 makes them hold the lock across a human
  question.

### 6. Citations

32 total — 5 in `docs/`, 27 elsewhere: `Cargo.toml`, `tests/exporting.rs`,
`tests/importing.rs`, `tests/purging.rs`, `src/export.rs`, `src/import.rs`,
`src/purge.rs`, `src/host/mod.rs`, `src/host/fake.rs`, `src/commands/{mod,
holdings,export,import,purge}.rs`, `src/main.rs`, `pages/src/content/docs/backup.md`,
and ADRs 0020, 0021, 0025.

**Load-bearing, and unusually so.** Every site read explains a shape rather than
tagging it: `Cargo.toml:62-67` gives the crate-and-armor argument, `src/export.rs:45`
gives the work-factor ceiling its reason, `src/host/mod.rs:659` explains why echo
suppression is on the port, and `src/import.rs:188/216/630` each name 0014 as the
failure the check is against. The ADR is the primary documentation for three
modules.

---

## ADR 0015 — Utilization is served from cache, and refreshing it is deliberate


### 1. The decision, in one sentence
Because `/api/oauth/usage` allows ~28-30 requests an hour per Account and does
not refill early, no display path ever blocks on the network: every surface
renders the cached figure **with its age**, `--refresh` is the only thing that
fetches, and `--json` must carry the observation time.

### 2. Does it still hold?
**Holds in part** — the rule holds completely; two of the three surfaces it
names, and the command it names as the cache-warmer, have been renamed or
removed under it.

Live:
- `src/utilization.rs:1-9` is the rule as the module's whole reason: *"Every
  surface that shows Utilization renders it from cache, and shows each figure
  with its age […] Only a `--refresh` fetches — on either surface — and it
  fetches before rendering rather than while: nothing here reaches the network."*
- Age on every figure: `src/utilization.rs:179-183` (`(as of {age})`).
- `--json` carries the observation time: `src/utilization.rs:195`, `:216`, and
  `:224` adds `observed_seconds_ago`.
- `status`: `src/commands/status.rs:3`, `:68`. `list`: `src/commands/list.rs:8`,
  `:195`.
- "Switching decisions inherit this": `src/cycle.rs:28-30` — *"Nothing here
  reaches the network or the filesystem: ranking is on the cached figures and
  their ages (ADR 0015)"* — and `src/cycle.rs:144`, `:933`, `:1640` all reason
  from *a cached figure outlives the window it describes*, which is this ADR
  pushed further than it went itself.
- The allowance arithmetic is still the reason for a live constant:
  `src/watch.rs:33-45`, `REFRESH_INTERVAL_MILLIS = 150_000` — "twenty-four of
  them" an hour.

Decayed, all naming rather than substance:
- **"`status`, `list`, `tui`"** — `perch tui` was removed entire by ADR 0049. Two
  surfaces, not three.
- **"`perch watch` is what keeps the cache warm"** — the command is now `perch
  watcher run` (`src/main.rs:300`, `:748-755`), and more importantly the loop
  Refreshes **one** Account, not the Scope: `src/watch.rs:41-44`, *"It is the
  Refresh of **one** Account […] at twenty-four an hour each, a Group of two
  would already be at the limit and a Group of four past it."* So the watcher
  keeps *the active Account's* figure warm and cannot keep the cache warm; the
  ADR's phrasing overclaims against the code that cites it.

### 3. Supersession
**None in either direction.** No banner. ADR 0018 builds on it explicitly
without amending it (*"The reasoning is ADR 0015's, followed through"*), and
ADR 0007's amendment by 0050 does not reach here.

### 4. Reasoning that outlives the document
> "Utilization is displayed as an observation with a timestamp, not a live
> reading."

and the consequence that is a general claim about honesty rather than about
caching:

> "Each figure is shown with its age, so a stale number is visibly stale rather
> than quietly wrong."

and the one that pre-emptively defends a bug report:

> "when a cycle lands on an account that turns out to be fuller than the cache
> implied, that is expected behavior and should be reported plainly rather than
> treated as a bug."

### 5. Others deciding the same thing
- **ADR 0018** ("A refresh degrades the display rather than failing the
  command") — the strongest merge partner anywhere in this slice. 0018 says of
  itself: *"The reasoning is ADR 0015's, followed through. […] A refresh that
  failed the command would throw away a perfectly good cached answer over the
  freshness of it."* 0018 has no independent premise; it is 0015's consequence
  worked out for the failure case.
- **ADR 0013** — owns the watcher's Refresh interval, which is 0015's arithmetic
  spent. `src/watch.rs:33-45` cites both in one doc comment, which is what a
  single merged document would look like.
- **ADR 0019** — its Consequences section is written entirely against 0015's
  allowance ("Only the second counts against the usage allowance ADR 0015 is
  about"). Adjacent, not the same decision.

### 6. Citations
61 total — the highest in this slice. 17 in `docs/`, 44 in code and elsewhere,
across `README.md`, `CHANGELOG.md`, seven test binaries, ten other ADRs,
`src/{cycle,utilization,watch,probe,reserve,registry,anthropic,observe}.rs`,
`src/host/{mod,fake}.rs`, `src/commands/{switch,status,list}.rs` and a guide page.

**Load-bearing.** `src/watch.rs:33-45` cites it to justify a specific integer and
shows the division. `src/cycle.rs:144` and `:933` cite it for a subtle inference
the ADR never made explicit — that an elapsed `resets_at` in a cached reading is
not a fact about when an Account comes back. `src/reserve.rs:150-152` cites it to
explain why a Reserve line carries "Read Nm ago at the oldest". The breadth is
high enough that a handful of the test-side mentions are closer to tags, but
every source-side site sampled explains a shape.

---

## ADR 0016 — Ratatui drives the TUI, and color-eyre only catches panics


### 1. The decision, in one sentence
Three crate choices settled ahead of their call sites — ratatui over crossterm
for `perch tui`, crossterm named directly so one version of a process-global raw
mode is in the tree, and color-eyre installed for its panic hook only — with the
last of these establishing that Perch carries two error idioms on purpose:
`PerchError` with a compiler-checked exit code for expected failures, a panic
report for bugs.

### 2. Does it still hold?
**Holds in part — and the halves fail and succeed for opposite reasons, from the
same fact.** Neither `ratatui`, `crossterm`, nor `color-eyre` appears anywhere in
`Cargo.toml` or `Cargo.lock`. That single fact kills one half and confirms the
other.

**The ratatui/crossterm half is dead.** There is no `src/tui/` directory, no
`tui::Screen`, no `Host::print_remarks` (the only surviving "remarks" in
`src/host/mod.rs:672` is a doc line about `note`), and no mention of ratatui or
crossterm anywhere in `src/` or `tests/`. The only trace left in the tree is an
obituary: `Cargo.toml:89` justifying `unicode-width` — *"A crate of its own now
that ratatui is gone"* — and `.github/workflows/ci.yml:109-112`, the unused-
dependency check that exists because of this: *"This is the check that would
have caught ratatui and crossterm sitting unused (ADR 0016)."* Both Amended
sections go with it: the first (the declaration waits) and the second (the
declaration is back, `tui::Screen`, the Refresh thread's own Host).

**The color-eyre half is live, and its liveness is exactly the absence of
color-eyre.** The ADR's second Amended section repealed the crate and replaced it
with a hand-written hook — so `color-eyre` being missing from `Cargo.toml` is the
decision being *implemented*, not contradicted. `src/report.rs:21-27`
`install_panic_hook` takes the runtime's hook and layers Perch's four facts on
top; `src/report.rs:57-64` `this_is_a_bug()` supplies version, OS and arch and
the issues URL; `src/report.rs:70-82` `bug_report` adds the `RUST_BACKTRACE=1`
suggestion the ADR promised — *"which color-eyre's prettier report never did"*.
114 lines, and the file's own header at `:1-11` restates the two-error-idiom rule
almost verbatim from the ADR. The typed side is `src/error.rs`: `EXIT_PROBE_REFUSED
= 10` (`:21`), `EXIT_KEYCHAIN_UNAVAILABLE = 11` (`:23`), `EXIT_NOT_FOUND = 12`
(`:25`) — the three the ADR names by number, unchanged — and `fn exit_code`
(`:299-317`) is written with every variant spelled out rather than a `_`, which is
the "the compiler checks that every variant has one" clause enforced. Eleven exit
codes now where the ADR named three, all in the same scheme.

So: **ratatui half dead by absence; color-eyre half live by the same absence.**

### 3. Supersession
Supersedes nothing. **Superseded in part by ADR 0049**, and this is the most
carefully drawn partial supersession in the slice. Its own header:

> **Superseded in part by ADR 0049.** This file is three decisions and only two
> of them were the picker's. The ratatui-over-crossterm choice is void, and so
> is the second Amended section below … **What stands is the color-eyre repeal
> and the two-error-idiom rule** … None of that was ever about drawing, and
> superseding this file whole would have taken `report.rs`'s charter with it.

ADR 0049 says the same from its side, at `0049…md:143-152`: *"Its ratatui-over-
crossterm choice and its second Amended section — `tui::Screen`, the Refresh
thread's own Host, `Host::print_remarks` — are superseded here. **Its color-eyre
repeal and the two-error-idiom rule stand**."* ADR 0049 also reaches into this
slice twice more, both times to *refuse* a change: *"**ADR 0025 is not
amended.** … Crossterm leaving closes the reopening without moving a line"*, and
*"**`unicode-width` stays declared**, with its justification rewritten."*

### 4. Reasoning that outlives the document
Two, and both are already the surviving half:

> Perch carries two error idioms on purpose. Expected failures are `PerchError`
> and exit codes; unexpected ones are panics with a color-eyre report. Anything
> that starts as a panic and turns out to be an outcome a user can act on should
> move across, not stay.

and, from the first Amended section, the general dependency rule that outlived
the two crates it was written about:

> The *choice* is what is worth settling ahead of time … The *declaration* buys
> nothing until there is code behind it: together they were the largest subtree
> in the dependency graph, compiled on every build and audited on every advisory,
> for no call site.

That second sentence is the direct ancestor of the CI job at
`ci.yml:109-115`, which is a rule with no ADR of its own.

### 5. Others deciding the same thing
- **ADR 0025** (*a crate is taken where it does not cost a seam*) — the surviving
  half of 0016 is a crate-policy decision (repeal color-eyre for a dozen lines),
  decided by exactly the arithmetic 0025 uses on `rpassword` and `sysinfo`.
  0016's repeal predates and prefigures 0025's rule.
- **ADR 0061** (*Perch says what it did and explains itself only when it
  refused*) — the two-error-idiom rule is about what Perch prints when it fails,
  and 0061 is about what Perch prints. The natural home for "expected failures
  are `PerchError` and exit codes" is beside "explains itself only when it
  refused".
- **ADR 0049** — the superseder; anything left of 0016 could be carried there.

### 6. Citations
5 total: 4 in `docs/`, 1 in code — the whole non-docs citation set is
`.github/workflows/ci.yml`. Docs cites are ADR 0049 and ADR 0025 (`0025…md:105`,
*"Crossterm is now in the tree (ADR 0016)"* — itself now false and marked history
by 0049). The joint-lowest cite count in the slice, and nothing in `src/` names it.

**Decorative in count, load-bearing in the one that exists.** `ci.yml:109-112` is
genuinely explanatory — the comment gives the reason the job exists and names the
failure it is a memorial to. But `src/report.rs` and `src/error.rs`, which are
where the surviving half of this ADR actually lives, cite it **nowhere** —
`report.rs`'s header restates the reasoning in its own words without a tag. The
live half of this document is entirely uncited from the code implementing it.

---

## ADR 0017 — Ungrouped accounts cycle only when asked


### 1. The decision, in one sentence
Being ungrouped is the *absence* of the declaration that Accounts are
interchangeable rather than a weaker form of it, so a bare `perch switch` from an
ungrouped Account refuses — naming both ways out — unless a setting carried by
the Ungrouped Scope alone says those Accounts are interchangeable, and that
setting is off by default.

### 2. Does it still hold?
**Holds**, with its body corrected in place in one sentence and its amendment
superseded.

- The refusal, naming both ways out, is `src/cycle.rs:56-68` (`scope_for`):
  `None if registry.ungrouped.interchangeable => Ok(Scope::Ungrouped)`, else
  `PerchError::NotInterchangeable` whose message names both `perch group move …`
  and `perch config set ungrouped interchangeable true`.
- The distinct exit code the Consequences demand exists:
  `EXIT_NOT_INTERCHANGEABLE = 18` (`src/error.rs:48`, mapped at `:310`).
- The setting is carried by the Ungrouped Scope alone, and asking a Group for it
  is a refusal: `src/config.rs:61-67` (`carried_by`), `:73-84`, and
  `src/registry.rs:548`.
- The gate is shared with the ranking rather than duplicated:
  `src/cycle.rs:762-767` (`may_cycle_within`), read by `src/listing.rs:88-98` so
  a Section that is not ranked also has no Reserve.
- "Turning the setting on is a declaration about every ungrouped Account at once,
  present and future": `src/config.rs:192-199` says exactly that back to the user
  when it is set.
- **The two-yeses rule, which 0051 found unenforced, is now structural.**
  `src/commands/watch.rs:645-668` asks `may_cycle_within` first and
  `settings.watcher_may_act` second, and the second is read off the Scope's own
  Settings (`:672-674`) with no layer to fall through from. The comment at
  `:645-648` states the rule and cites 0017; the refusal text at `:658-663`
  teaches it.

Three sentences in the body are now false or stale:
- **Corrected in place, as 0051 required.** "The setting is global rather than
  per-Group" now reads "The setting is the Ungrouped Scope's own." Confirmed in
  the file.
- **Not corrected.** The Consequences still open "Perch gains global
  configuration, where before all configuration hung off a Group. `perch config`
  needs a form that addresses a setting belonging to no Group". There is no
  Global (`src/registry.rs:504-513`: *"There is no such Scope"*); the form is
  `<scope> <key> <value>` (`src/commands/config.rs:64-68`). 0051 settled the
  second half of that paragraph explicitly and left the first half standing.
- The key's *name* changed: `cycle-ungrouped` → `interchangeable`
  (`src/config.rs:51`), which 0017 never names, so nothing there is wrong.

### 3. Supersession
Supersedes nothing. It is superseded **in part, deliberately and with the refusal
recorded**, by ADR 0051. Three distinct scopes, and the file states all three:

Its amendment, whole:
> **Superseded in full by ADR 0051.** Its core claim survives and is
> strengthened: the Accounts in no Group are a Scope, and they now hold Settings
> outright rather than Overrides over something else. What goes is "reads Global"
> and the non-uniform-layering paragraph below […]

One body sentence, corrected rather than left standing:
> One sentence in the body above is corrected in place rather than left standing:
> "The setting is global rather than per-Group" is now "The setting is the
> Ungrouped Scope's own". The two sentences of reasoning under it are untouched […]

And the explicit **refusal** to supersede the whole, from 0051's side:
> Superseding ADR 0017 whole was considered and refused, on ADR 0046's precedent:
> supersede the section that decayed and leave a mostly-correct record standing
> rather than restating it at length. 0017's record is longer and cleaner than
> 0013's was, and superseding it entire to correct one sentence whose *reasoning*
> survives would bury three Considered Options that are the only written record
> of why ungrouped Accounts do not Cycle freely.

The stated scope of what survives is therefore: **the whole body except one
sentence (now rewritten), and specifically the three Considered Options, which
0051 names as the reason it did not take the document.**

### 4. Reasoning that outlives the document
> "A Group is the declaration that a set of Accounts is interchangeable; being
> ungrouped is the absence of that declaration, not a weaker form of it."

> "modeling the ungrouped pool as a Group with a reserved name would make a Group
> mean two contradictory things — a declaration the user made, and one Perch made
> for them."

> "Two independent yeses before anything moves underneath you: one saying these
> Accounts are interchangeable at all, one saying something may act on them
> unasked."

The third is in the superseded *amendment*, and 0051 kept the rule while killing
the paragraph — so if 0017's amendment is ever physically deleted, that sentence
must be carried, and it already is, at `src/commands/watch.rs:645-648`.

### 5. Others deciding the same thing
- **ADR 0002** — see 0002 §5. 0017 opens by calling itself 0002 followed to its
  conclusion, and the two share the same premise sentence verbatim in the code.
  My reading is that **0002 and 0017 are one decision found twice**, and the only
  reason they are two documents is that Perch shipped Groups before it noticed
  the Ungrouped case.
- **ADR 0051** — related but a different question; see 0051 §5.
- **ADR 0058** — 0058 leans on 0017 rather than restating it: *"The Ungrouped
  stay silent until `interchangeable` is declared. Not a special case:
  `cycle::may_cycle_within` is the same gate […] and ADR 0017 already says what
  it means."* Reader, not partner.

### 6. Citations
72 total — the most-cited ADR in this slice. 20 in `docs/`, 52 in code and
elsewhere, across nine test binaries, five other ADRs,
`src/{listing,cycle,error,main,config,registry}.rs`,
`src/commands/{group,watcher,mod,add,service,list,watch}.rs` and four guide pages.

**Load-bearing.** `src/commands/watch.rs:645-663` cites it twice inside one
refusal and reproduces its reasoning in the user-facing text. `src/listing.rs:91`
cites it to justify why a Section that is not ranked also has no Reserve — a
consequence 0017 itself never anticipated. `src/config.rs:61-67` cites 0002 and
0017 together to decide which Scope carries a key. `src/error.rs` ties it to an
exit code. The guide pages are the closest to decorative but still state the rule
they cite.

---

## ADR 0018 — A refresh degrades the display rather than failing the command


### 1. The decision, in one sentence

A failed fetch under `perch status --refresh` never turns the command into a
failure: each failure is reported by name against the Account it belongs to, the
Account keeps its last figure, the other Accounts are still read and shown, the
command exits zero, and `--json` carries the outcome under `refresh` — `null`
when no refresh was asked for, so a script can tell "nobody asked" from "asked,
and it went fine".

### 2. Does it still hold?

**Holds**, in every clause including the `null` subtlety.

- *No failure exit code of its own.* `observe::refresh` returns a `Report`, not a
  `Result` (`src/observe.rs:249`), so a caller cannot propagate a fetch failure
  as an error. `commands::status::run` calls it and then renders unconditionally
  (`src/commands/status.rs:73-87`).
- *Each failure reported by name — which Account and why.* `observe::Attempt`
  carries `named` and `outcome`, and `Attempt::note` produces the per-Account
  sentence (`src/observe.rs:105-125`). The `named` field's own doc records why it
  exists: the Watcher's decision line was the one surface naming an Account by
  raw address.
- *Each Account independent.* The `for email in emails` loop in
  `observe::refresh` charges each Account's outcome to its own `Attempt` and
  keeps going.
- *`--json` carries it under `refresh`, `null` when nobody asked.*
  `Report::document` returns `serde_json::Value::Null` when `!self.asked`
  (`src/observe.rs:225-233`), and the `asked` field's doc records the exact bug
  the ADR's rule prevented — an empty Group answering `null` for a `--refresh`
  that *was* passed.
- *The rule is about the fetch, not the command.* Errors before any figure could
  be shown still fail: `active_email` returns `PerchError::NotFound`
  (`src/commands/status.rs:97`).
- *A per-Account refusal rather than a per-command one, even for a shared
  Profile.* `src/observe.rs:734-737` cites 0018 for exactly this: *"A refusal for
  this Account alone rather than for the command, which is what ADR 0018 asks of
  everything in this module."*
- *The Watcher inherits it.* `src/commands/watch.rs:261` — an unreadable round is
  held and gone round again *"(ADR 0013, ADR 0018)"*.

ADR 0053 confirms the surface list went stale without the decision decaying:
*"ADR 0015 lists the surfaces that show Utilization as `status`, `list`, `tui`.
Those citations go stale and the decisions do not decay."* `perch tui` is gone
(ADR 0049); `perch status --refresh` and `perch list --refresh` remain.

### 3. Supersession

Supersedes nothing; superseded by nothing. ADR 0053 states this explicitly:
*"**ADR 0015 and ADR 0018 are untouched.**"* (0053:213). ADR 0048 cites it as the
register a decision was taken on rather than amending it: *"non-zero exit was
refused on ADR 0018's register — status reports what it found"* (0048:163).

### 4. Reasoning that outlives the document

> "A refresh that failed the command would throw away a perfectly good cached
> answer over the freshness of it, and would do so at exactly the moment a
> person is trying to decide where to switch."

And the claim that generalizes past `--refresh` to every set-shaped surface:

> "a listing that lost every figure to one broken Account answers worse than a
> listing with one gap in it — and the gap is visible, since a figure nobody
> could read still says when it was last observed."

### 5. Others deciding the same thing

- **ADR 0015 (`utilization-is-served-from-cache`)** is the same decision in two
  halves, and 0018 says so in as many words: *"The reasoning is ADR 0015's,
  followed through."* 0015 decides Utilization is an observation with an age;
  0018 decides what follows when the observation cannot be refreshed. Neither
  reads whole without the other, and both are cited together at
  `docs/adr/0032*.md:24` (*"degrading rather than failing (ADR 0015, ADR
  0018)"*), which is the corpus already treating them as one rule.
- **ADR 0061** (`perch-says-what-it-did-and-explains-itself-only-when-it-refused`)
  is the general form of 0018's "each failure reported by name". Related
  register, different scope — probably a citation, not a merge.

### 6. Citations

17 total — 8 in `docs/`, 9 in code and elsewhere. `src/observe.rs`,
`src/registry.rs`, `src/host/real.rs`, `src/commands/watch.rs`,
`tests/{quarantining,reporting,scheduling}.rs`,
`pages/src/content/docs/running.md`, and five ADRs.

**Load-bearing.** `src/observe.rs:736` uses the ADR to decide the *scope* of a
refusal (this Account, not this command). `src/host/real.rs:96` uses it to
distinguish two failure modes: *"because ADR 0018 has an answer for that one and
no answer for a…"*. `src/registry.rs:801` cites it for reporting a Landing
*"rather than judging it"*. The three `tests/quarantining.rs` sites
(`:79`, `:148`, `:489`) are `result.expect("a refresh degrades the display
rather than failing it (ADR 0018)")` — those are the closest to decorative, but
even they put the rule in the failure message, which is where a future reader
needs it.

---

## ADR 0019 — A figure is recorded only against the Account it was read for


### 1. The decision, in one sentence
Before caching any Utilization figure, Perch asks the profile endpoint whose
access token it just used and records **nothing** if the answer is somebody else
— because the live Credential lives in the Default Profile, which Perch is not
the only thing that writes, and a figure filed under the wrong Account is the one
kind of wrong answer this design cannot afford: a plausible one.

### 2. Does it still hold?
**Holds**, and has been tightened in one place and extended to a second recording.

- The check itself: `src/observe.rs:914-951` (`confirm`), called before the
  usage read at `src/observe.rs:411` inside `read_off`. The mismatch branch
  returns `Outcome::Failed` naming the Account the token really belongs to,
  exactly as the ADR's last paragraph describes.
- Ordering, and therefore the Consequence "Only the second counts against the
  usage allowance ADR 0015 is about, and the first is spent before it": `confirm`
  runs, then `perch.renew()`, then `anthropic::utilization` — `src/observe.rs:411-419`.
- "A reply that names nobody is not evidence of anything, and does not stop the
  read": `src/anthropic.rs:176-187` (`whose`) now returns
  `Refused::Unrecognized` rather than `None` for that case, and
  `src/observe.rs:944-949` notes it aloud and continues. `src/anthropic.rs:158-175`
  explains why: folding "named nobody" into "shape Perch does not know" meant
  *"the day Anthropic renamed `email_address`, the ADR 0019 guard would have
  become a no-op for every Account, for ever, with nothing printed anywhere."*
- **Tightened beyond the ADR's text.** `src/observe.rs:930-938`: an HTTP failure
  used to reach the carve-out and no longer does — *"`/api/oauth/profile`
  returning 503 during an incident while `/api/oauth/usage` keeps answering is
  nothing about who the token belongs to, and read as permission it cached one
  Account's figures under another's."* The ADR's carve-out is for *drift in a
  reply*, and the code now enforces exactly that width and no wider.
- **Extended to a second kind of recording the ADR did not mention.**
  `src/observe.rs:424-445` (`only_off_a_credential_that_is_theirs`) applies the
  same rule to a Quarantine, on local evidence, because a Quarantine is a
  recording too and a terminal one. This is 0019's principle reused, not 0019.
- The forward-looking Consequence still holds as written: *"Nothing acts on that
  yet — the refusal names the Account the token really belongs to and stops
  there."* Nothing in the tree re-adopts a login found this way.

### 3. Supersession
**None in either direction.** No banner; nothing supersedes or amends it. It is
cited *by* 0012 and 0015's neighborhoods rather than being touched by them.

### 4. Reasoning that outlives the document
> "Figures cached under the wrong Account would not look wrong. They would look
> like that Account having spent quota it never spent […] It is the one kind of
> wrong answer this design cannot afford: a plausible one."

That sentence is the general claim — a wrong answer that is indistinguishable
from a right one is worth more machinery than a wrong answer that announces
itself — and it is already load-bearing at `src/observe.rs:936` and
`src/anthropic.rs:479` (where 0012's parser cites the same shape of hazard).

Second, the liveness analogy, which is a claim about evidence generally:
> "A reply that names nobody is not evidence of anything […] the same rule
> liveness already follows, where a Profile is Live because something says so
> rather than because nothing does."

### 5. Others deciding the same thing
- **ADR 0022** ("a live profile is corroborated by when its process started") —
  the same epistemics: a claim is admitted only when something positively
  attests it, and an absence attests nothing. 0019 cites the parallel in its own
  fourth paragraph, which is what a merge would formalize.
- **ADR 0007** ("assumptions about Claude Code are probed, not assumed") — 0019
  is that rule applied to Anthropic's replies rather than to Claude Code's disk
  layout, and 0026 already cross-cites 0007 for the same move.
- **ADR 0006** ("the live credential is captured before every switch") — same
  premise, that the Default Profile is written by things other than Perch, used
  for a different act. A merged "the Default Profile is not Perch's alone"
  document would take a paragraph from each.

### 6. Citations
12 total — **0 in `docs/`**, 12 in code and tests: `tests/carrying.rs`,
`tests/refreshing.rs`, `tests/quarantining.rs`, `src/carry.rs`,
`src/observe.rs`, `src/anthropic.rs`.

**Load-bearing, and the most purely code-facing ADR in this slice** — no other
ADR cites it at all, so its entire footprint is in the implementation.
`src/anthropic.rs:158-175` is a twelve-line doc comment whose whole subject is
how a refactor nearly turned the guard into a no-op. `src/observe.rs:929-938`
uses the ADR to draw the exact boundary of a carve-out. `src/carry.rs:344` and
`tests/carrying.rs:188` extend it to a third surface — the `.claude.json` keys a
Carry must not copy, because they hold figures read for another Account. Not one
bare tag among the sites sampled.

---

## ADR 0020 — A credential lives wherever Claude Code puts it


### 1. The decision, in one sentence

Perch stores a Credential exactly where the installed Claude Code would and nowhere
else — keychain on macOS, `<config dir>/.credentials.json` everywhere else — reading
across both stores as a composite (platform picks the write target, evidence picks
what is believed), removing the copy in whichever store was not written, creating
credential files 0600 and their directories 0700 *at creation*, and dropping the
recorded store fields from `registry.json` in favor of derivation.

### 2. Does it still hold?

**Holds**, and the code is if anything more careful than the document.

- Two stores, platform-ordered: `credentials.rs:39` `stores_for` — macOS
  `[keychain, plaintext]`, everywhere else `[plaintext, keychain]`.
- Evidence decides belief: `credentials.rs:89` `read` — primary first, fallback on
  anything else, and a locked keychain reported only when it is the whole story.
  *"A locked keychain is therefore a failure when it is the whole story and a
  detail when it is not."*
- The plaintext store is `join(<config dir>, ".credentials.json")`:
  `probe.rs:472` `CREDENTIALS_FILE`, `:489` `credentials_file_for`.
- Superseding the unwritten copy: `profile.rs:81` `store_credential` →
  `supersede` / `supersede_or_fail`. The code goes further than the ADR: the
  rarer direction (fallback written, primary copy removed) **fails** rather than
  remarking, because a stale copy in the *preferred* store would win over what was
  just written (`profile.rs:96-110`). `profile.rs:59` says so explicitly.
- Read-back after every write (0008's truncation hazard): `profile.rs:47-51`.
- 0600 / 0700 at creation, never chmod-after: `host/mod.rs:260` `PRIVATE_FILE_MODE`,
  `:264` `PRIVATE_DIR_MODE`, `:440-447`; `profile.rs:15-28` `make_dir` — *"Created
  that way rather than tightened, because a Profile is Perch's to make and there is
  no window to leave."* The tighten-and-mention-once path exists at
  `host/real.rs:1670`.
- Derivation rather than recording: `registry.rs:3361` asserts a serialized registry
  contains none of `keychain_service`, `keychain_account`, `profile`, `dir`.
  `short_hash` is pinned by test as the ADR promised.
- The loudest contract test exists and does what the ADR asked:
  `tests/your_machine.rs:447`
  `claude_code_reads_the_credential_perch_would_write_for_a_config_directory`, plus
  `:96` for the hashed-namespace case.
- `CLAUDE_SECURESTORAGE_CONFIG_DIR` is not read anywhere — grep confirms.
- Profiles under `~/.config/perch/profiles/`: `registry.rs:1576` `perch_home`,
  `:1655` `profile_dir_for`.

**Two small factual drifts, neither behavioral:**

1. *"the format stays at version 1"* — `CURRENT_VERSION` is now `2`
   (`registry.rs:18-26`), moved by ADR 0051 for an unrelated reason. The clause the
   sentence was defending (fields dropped, nothing migrated) is unchanged.
2. The identifier `primary_and_fallback_failed` appears nowhere in Perch — but the
   ADR attributes that name to *Claude Code's* composite reader, not Perch's, so
   this is not a drift at all. Worth noting so a later reader does not chase it.

### 3. Supersession

**Partially supersedes ADR 0001 and ADR 0008**, and is explicit about the boundary:

> "ADR 0001 and ADR 0008 were written against a macOS-only Perch, and both assume a
> Credential is held in the operating system's keychain. That assumption is macOS's,
> not Claude Code's."

and, decisively for whole-vs-partial:

> "The conclusion of ADR 0001 survives unchanged; only its mechanism is macOS's."

ADR 0008 is not superseded at all in its own domain — it still decides *how* the
macOS keychain is reached, and `credentials.rs:32` and `profile.rs:49` both cite it
live. 0020 narrows 0008's *scope of applicability* (one platform of three) without
touching its content.

Nothing supersedes 0020. ADR 0029 depends on it (*"Profile (ADR 0020), not in a
system keyring, so there is no libsecret to link"*), and its own Consequences were
amended in place by later observation of the Linux build — a self-amendment, marked
by *"Linux was inference when this was written and is now observation."*

### 4. Reasoning that outlives the document

The single most portable sentence in the slice:

> "the platform chooses where Perch *writes*, because a Profile that `perch add`
> has just created holds no evidence to read; and evidence chooses what Perch
> *believes*, because an existing store can be looked at. This is ADR 0007's rule
> applied one level down: probed, not assumed."

Second, the security argument that justifies the whole rejected alternative:

> "DPAPI decrypts for any process running as the user, and the Secret Service
> unlocks with the login session, so malware running as the user either has the
> Credential already or can wait one Switch for it."

Third, the mode trap, which is a general filesystem rule and not about credentials
at all:

> "`mkdir -p` leaves an existing directory's mode alone, so a Profile made by an
> ordinary `create_dir_all` and only then written into privately stays 0755, and
> every test that asks after the fact still sees a 0600 file inside it. Whatever
> creates a Profile has to be the thing that makes it private."

Fourth, the design constraint that makes adoption possible:

> "the property that makes adoption work at all: that Perch reads what Claude Code
> wrote, and writes what Claude Code will read."

### 5. Others deciding the same thing

- **ADR 0001** and **ADR 0008** — see field 3. All three decide "where does a
  Credential live and how is it reached", and 0020 is the only one of the three
  that is true on every platform. The strongest three-way merge candidate I found.
- **ADR 0007** — 0020 describes itself as *"ADR 0007's rule applied one level
  down"*, twice. Not a merge (0007 is the general rule), but the citation is the
  reason 0020's design is shaped as it is.
- **ADR 0006** — the supersede-the-other-copy rule is 0006's argument on a second
  axis, by 0020's own statement (*"the reason is ADR 0006's"*).
- **ADR 0014** — 0020 hands the portable-Credential case to it explicitly (*"The
  portable case — a Credential leaving the machine — is covered separately and
  differently by ADR 0014's required passphrase"*), so the two together are the
  complete answer to "how is a Credential protected".

### 6. Citations

46 total: 5 in `docs/`, 41 in code and elsewhere — `SECURITY.md`, seven test
suites plus `tests/common/mod.rs` and `tests/your_machine.rs`,
`src/credentials.rs`, `src/login.rs`, `src/probe.rs`, `src/profile.rs`,
`src/import.rs`, `src/registry.rs`, `src/host/{mod,real,fake}.rs`,
`src/commands/mod.rs`, two guide pages, plus ADRs 0029, 0040, 0045.

**Load-bearing, with the highest code-to-docs ratio in the slice (41:5).**
`credentials.rs:1` is the module charter; `profile.rs:45` enumerates *"the three
things a write owes (ADR 0020)"* as the reason every write funnels through one
function; `host/mod.rs:259` fixes `PRIVATE_FILE_MODE` to it; `host/mod.rs:445`
cites it for why the mode is set at creation; `host/fake.rs:1594` cites it for the
Windows ACL carve-out in the fake. `tests/storing.rs:1` names the ADR as the
suite's subject, which is ADR 0045's naming rule working.

---

## ADR 0021 — Platform primitives are linked rather than shelled out


### 1. The decision, in one sentence
The primitives the portable standard library does not offer — process existence,
a directory's modification time, and (by amendment) terminal echo suppression —
are taken as `extern` declarations from `libc` and `windows-sys` rather than
hand-written `unsafe` or a wrapper crate, because ADR 0008's creator-identity
argument is specific to the keychain and does not generalize; shelling out
remains the rule where that argument applies.

### 2. Does it still hold?
**Holds, with a stale count.** The mechanism is exactly as decided.

- Both crates are declared, with the "already transitive" justification intact:
  `Cargo.toml:96-100` — *"Both already in the build transitively — libc through
  sha2's cpufeatures, windows-sys through clap's anstream — so declaring them
  adds no crate and buys vetted declarations for the primitives std does not
  carry (ADR 0021)."* Note the transitive path has changed since the ADR was
  written (which said "`chrono`, `crossterm` and `ratatui`") and the conclusion
  survives the change.
- `isatty` is gone in favor of the standard library: `src/host/real.rs:877-878`,
  `fn is_interactive` → `use std::io::IsTerminal;`.
- Process liveness and start time: `src/host/real.rs:1874` / `:1895`
  (`process_alive`, unix and Windows) and `:1699`/`:1733`/`:1781`/`:1830`
  (`process_started_at`, four platform arms).
- Directory mtime remains platform-split code over vetted declarations, exactly
  as the Consequences predicted: `src/host/real.rs:1922` (`libc::utimes`) and
  `:1949` (`CreateFileW` with `FILE_FLAG_BACKUP_SEMANTICS` + `SetFileTime`).
  `filetime` is still not a dependency, so the "one-line reversal" was never
  taken.
- Echo suppression, the amendment's fourth: `src/host/real.rs:1113-1185`
  (`tcgetattr`/`tcsetattr` with `ECHO`) and `:1204-1212` (`GetConsoleMode`/
  `SetConsoleMode`), with `src/host/real.rs:1084-1086` citing the ADR — *"a
  platform primitive linked rather than taken as a crate, which is what ADR 0021
  decided for the last two of these."* It is a Host method (`Terminal::read_secret`),
  which is the part the amendment said pays for itself twice.
- Shelling out where 0008's argument applies: `SECURITY_BIN` (above) and
  `src/host/real.rs:21-27` for `curl` — *"Perch shells out for the same reason it
  shells out to `security` (ADR 0008) … Always by absolute path, because the path
  is a security property rather than a convenience."* One nuance the ADR does not
  record: `curl_bin()` (`real.rs:28+`) now falls back to a `PATH` walk when the
  absolute path is absent, for NixOS and similar — the absolute path is still
  first and almost always, and the comment argues the case at length.

**Stale:** the count is no longer four. `real.rs` links at least three more
primitives on the same reasoning — `geteuid` for `user_id` (`:449`, and
`:439-441` cites the ADR for it: *"Linked rather than shelled out to (ADR 0021):
the whole of it is one `geteuid`, and `id -u` would be a process spawned to
answer a question the C library already holds"*), signal handlers for
`listen_for_interrupts` (`:1563-1580` unix, `:1595-1600` Windows), and a raw
one-byte stdin read (`:1034-1047`). `Cargo.toml:22-28` already lists four
categories — *"process liveness, file times, terminal echo, signal handlers"* —
and deliberately refuses to write the number down: *"The count is deliberately
not written down here: it moves, and a number in a comment that moves is a
comment that is wrong."* The ADR is the place that still writes it down.

### 3. Supersession
Supersedes nothing and is superseded by nothing. Its whole first paragraph is an
**explicit refusal to supersede ADR 0008**, and this is the sentence that
reconciles the pair:

> ADR 0008 says Perch drives `/usr/bin/security` rather than linking a keychain
> crate, and a reader arriving at `libc` and `windows-sys` in `Cargo.toml` will
> reasonably ask what changed. Nothing did: ADR 0008's argument is specific and
> does not generalize.

with the reciprocal boundary stated in its Consequences:

> Shelling out remains the rule where ADR 0008's actual argument applies.

**They do not contradict.** 0021 does not carve out an exception 0008 occupied;
it is the other way round — 0008's argument was never general, and 0021 is the
default rule for everything 0008 does not reach. The discriminator is named
precisely: *"No such property attaches to asking whether a process is alive."*
The code enforces both simultaneously in one file: `src/host/real.rs` spawns
`/usr/bin/security` at `:312` and calls `libc::geteuid()` at `:449`.

Amended once, by itself (*"a fourth primitive, and the rule holding"*).

### 4. Reasoning that outlives the document
> That is precisely the class of `unsafe` whose mistakes nothing catches until
> something is corrupted, and its only remaining virtue would have been
> consistency with a rule that was never about this.

This is the test the repo reuses: `Cargo.toml:103-108` invokes it verbatim for
`junction` on Windows — *"precisely the class of `unsafe` ADR 0021 declined to
write"* — and ADR 0025's *Directory junctions* paragraph does the same. Also
outliving the document:

> that path is a security property rather than a convenience:
> `Command::new("curl")` would let anything earlier on `PATH` receive an
> `Authorization: Bearer` header.

which `src/host/mod.rs:107-110` and `src/commands/export.rs:14-18` both apply to
things that are not `curl`.

### 5. Others deciding the same thing
- **ADR 0025** — same decision procedure, and 0025's *Not taken* section already
  contains 0021's reasoning twice (`sysinfo`, `rpassword`) at enough length to
  stand alone. 0021 and 0025 differ mainly in that 0021 is a case and 0025 is the
  rule the case was generalized into.
- **ADR 0008** — the reconciled pair; a merged dependency-policy ADR would need
  0008's exception stated inside it, which is what 0021's first paragraph
  already is.
- **ADR 0014** — the amendment's whole occasion (a passphrase must not be
  echoed) is 0014's decision; the fourth primitive exists to serve it.
- **ADR 0026** — the junction case in Cargo.toml sits at the 0021/0025/0026
  junction.

### 6. Citations
23 total: 6 in `docs/`, 17 in code and elsewhere — `Cargo.toml`,
`src/host/real.rs`, `src/upgrade.rs`, `src/service.rs`, `src/commands/service.rs`,
`src/commands/upgrade.rs`, `src/commands/export.rs`, `tests/upgrading.rs`,
`tests/servicing.rs`, plus ADRs 0014, 0025, 0039.

**Load-bearing, and generalized beyond its subject.** `Cargo.toml:22-28` cites it
to justify a *lint* (`unsafe_op_in_unsafe_fn = "deny"`), which is the ADR
deciding something outside its own text; `src/host/real.rs:439-441` cites it for
a primitive the ADR never counted; `src/upgrade.rs:465-468` cites it for
something that is not a primitive at all — *"The `brew` beside the Cellar rather
than the first one on `PATH`, for the reason ADR 0021 gives about every program
Perch runs"* — i.e. the absolute-path rule, applied to Homebrew;
`src/commands/export.rs:14-18` cites it for the argv/process-table rule applied
to a passphrase. Four sites, four different clauses, none decorative.

---

## ADR 0022 — A Live Profile is corroborated by when its process started


### 1. The decision, in one sentence

A session marker is evidence of a running client only when the process it names is
alive **and** began no later than the marker's `startedAt` says the session did —
matched against `startedAt` (plain epoch millis) rather than the platform-encoded
`procStart`, which requires three per-OS answers to "when did this process begin".

### 2. Does it still hold?

**Holds**, with one qualification the document does not carry.

- The corroboration itself: `probe.rs:1143` `clients_in`, and specifically
  `probe.rs:1191-1196` — a pid is pushed only when
  `process_began.timestamp_millis() <= session_began + CLOCK_STEP_MARGIN_MILLIS`.
- `startedAt` is the only field deserialized: `probe.rs:1211-1215`
  `struct SessionMarker { #[serde(rename = "startedAt")] started_at: Option<i64> }`.
  `procStart` appears nowhere in `src/`.
- Three OS implementations, exactly as predicted: `host/real.rs:1699`
  (`/proc/<pid>/stat` + `btime`), `:1733` (macOS), `:1781` (`GetProcessTimes`), and
  a `None` stub at `:1830`.
- Named assumption in `probe.rs` failing loudly and by name: `probe.rs:31`
  `assumption::SESSION_MARKER`, raised via `live_clients` (`probe.rs:1046`).
- Degrades in the existing direction — a marker that cannot be parsed is no
  evidence: `probe.rs:1170` `Marker::SaysNothing => continue`, with the rule spelled
  out (*"a Profile is Live when something says so rather than when nothing does"*).
- Exit 16 when a Switch is refused for liveness: `error.rs:39` `EXIT_PROFILE_LIVE =
  16`, reached via `NotIdle::Live → PerchError::ProfileLive` (`switch.rs:1395`).

**Qualification:** the ADR claims *"A recycled PID necessarily belongs to a process
that started after the marker was written, so the check is exact rather than
heuristic."* The comparison now carries a 5-second slack —
`CLOCK_STEP_MARGIN_MILLIS` (`probe.rs:1233`) — because on Linux `/proc/<pid>/stat`'s
start time is recomputed from a `btime` that moves with the wall clock, so an NTP
correction made a Run's own live process look younger than the session it had just
recorded. The comment there argues the margin costs nothing in the direction the
ordering is for. The check is therefore *nearly* exact, and the ADR's word "exact"
is no longer literally true. Nothing in `docs/` records this.

Also: macOS uses `proc_pidinfo` rather than the `sysctl KERN_PROC_PID` the ADR
names, on ADR 0025's grounds — *"the libc crate declares the former's argument
struct and not the latter's `kinfo_proc`"* (`host/real.rs:1728-1733`). Mechanism
drift, same primitive.

### 3. Supersession

Supersedes nothing (it replaces an unrecorded prior behavior — *"Perch believed
such a marker whenever the process it named was alive"* — rather than a prior ADR).
Superseded by nothing.

**Extended by ADR 0027**, which is the writer half to this reader half and says so
in as many words: *"Perch already knows how to recognize one — a session marker
naming a process that is still the one that wrote it (ADR 0022) — and already
refuses to write into one. What it did not do was produce that evidence for the
sessions it launches itself."* 0027 also derives the self-corroboration property
from 0022's rule rather than restating it. No clause of 0022 is amended.

ADR 0041 cites it for what a real machine proves. CONTEXT.md's **Marker** entry
carries the rule and the cite.

### 4. Reasoning that outlives the document

> "A marker is therefore evidence only when the process it names is alive *and*
> started no later than the marker says the session did."

and the field-choice argument, which is a general rule about matching against
cross-platform data:

> "`procStart` is the tempting one and was rejected: on Windows it is a `FILETIME`
> in 100-nanosecond ticks since 1601, and it will be something else entirely on
> Linux and macOS, so matching it means three reverse-engineered encodings instead
> of one. `startedAt` is plain epoch milliseconds and means the same thing on every
> platform."

and the degradation direction, which `probe.rs` treats as the governing rule for
five separate branches:

> "a marker Perch cannot parse counts as no evidence of a client, because a Profile
> is Live when something says so, not when nothing does."

### 5. Others deciding the same thing

- **ADR 0027** — the single strongest merge candidate in this slice after
  0001/0020. One reads markers, the other writes them; the correctness argument for
  the write (*"Perch's process began strictly before Perch wrote the file, so the
  ADR 0022 check passes"*) is unintelligible without 0022's rule, and 0022's rule
  has no second reader.
- **ADR 0005** — the precondition 0022 makes answerable. `probe.rs:1226` names both
  in one sentence: *"which is the whole of what ADR 0005 and ADR 0022 exist to
  prevent."*
- **ADR 0007** — 0022 introduces itself as *"a new named assumption in `probe.rs`
  … and it fails the way the others do, loudly and by name"*, i.e. it is an
  instance of 0007's rule.
- **ADR 0021 / 0025** — the three-implementations cost 0022 accepts is what those
  two decide the shape of, and the macOS drift above was decided on 0025's grounds.

### 6. Citations

24 total: 4 in `docs/`, 20 in code and elsewhere — `CONTEXT.md`,
`tests/{adding,running,conformance,corroboration,switching,your_machine}.rs`,
`tests/common/mod.rs`, `src/probe.rs`, `src/login.rs`, `src/host/{mod,real,fake}.rs`,
`pages/.../running.md`, plus ADRs 0027 and 0041.

**Load-bearing.** `host/mod.rs:596` attaches it to the `process_started_at` port
declaration and restates the rule as that method's contract; `probe.rs:1036` is the
rule as `clients_in`'s doc; `probe.rs:1212` explains why `SessionMarker` reads
exactly one field; `probe.rs:1218-1232` is a four-paragraph argument keyed to the
ADR for why a constant exists at all; `host/fake.rs:886` and `:914` describe the two
fake states built *for* the ADR's hazard. `tests/corroboration.rs:1` is a whole
suite named for it.

---

## ADR 0023 — A quarantined account is repaired in place, and only the account named is touched


### 1. The decision, in one sentence
An Account whose Credential cannot be recovered is kept and marked Quarantined
with the reason it broke, and `perch relogin` repairs it by logging in inside a
config directory of its own and moving the result into the Profile the Account
already has — keeping its Alias, Group, Cycling eligibility and position —
refusing a login that authenticates somebody else, and additionally writing the
fresh Credential to the Default Profile (without a Capture) when the Account
being repaired is the active one.

### 2. Does it still hold?
**Holds in part** — every operative clause is live, one Consequence is dead.
- In place, keeping everything: `src/commands/relogin.rs:1-19` restates it
  verbatim; the repair writes into the Account's existing Profile and the
  registry entry is never rebuilt.
- Login in its own directory, active session untouched: `login::perform` at
  `src/commands/relogin.rs:74-78`; `login::perform` has exactly the two callers
  ADR 0054 names (`add.rs`, `relogin.rs`).
- A login that authenticates somebody else is refused:
  `refuse_a_different_account(&registry, &account, &produced.identity)?`,
  `src/commands/relogin.rs:79`.
- The active-Account exception, no Capture: `make_live` is the path
  (`src/switch.rs:601`), and `make_live` "Skipping the Capture is what makes it
  different going in" (ADR 0048's words, implemented there).
- Allowed on a healthy Account: `src/commands/relogin.rs:17-19`, and ADR 0054
  makes this the reason the command is called `relogin`.
- Its own exit code: `EXIT_QUARANTINED = 19`, `src/error.rs:52`, mapped from
  `PerchError::Quarantined` at `src/error.rs:311` and documented at
  `pages/src/content/docs/reference.md:46`.
- **Dead:** "a Quarantine raised by a build that did not record reasons still has
  to load. It reads as Quarantined for a reason nobody wrote down." There is no
  such state. `Quarantine` (`src/registry.rs:62-77`) is a four-variant enum with
  no unknown/reasonless arm, `#[serde(rename_all = "kebab-case")]`, and every
  variant has a `because()` sentence (`:82-94`). This clause is exactly the
  reading-what-an-older-Perch-wrote guard `CLAUDE.md` forbids, and the code has
  already declined to keep it.

### 3. Supersession
Supersedes nothing; superseded by nothing. Two later documents lean on it without
amending it. ADR 0054: "**This supersedes nothing and amends ADR 0047 in one
clause.** ADR 0023 is untouched and still governing: the relogin on a healthy
Account, which its Consequences allow, is now the reason the command is named
what it is rather than a footnote to it." ADR 0054 also reuses 0023 sideways:
deleting `perch alias` "would be a `remove` and a re-`add` — which is precisely
the 'resembles the Account and is not it' failure ADR 0023 exists to refuse,
arrived at from the other direction."

### 4. Reasoning that outlives the document
> Removing and re-adding would produce something that resembles the Account and
> is not it, and would hand the user the job of putting back settings they never
> changed.

Already generalized past its subject: ADR 0054 uses it as the argument for
keeping a whole command. A second, narrower one worth carrying:

> An operation attempted against a Quarantined Account exits with a code of its
> own. Every other refusal in Perch is answered by trying something else or
> trying again; this one is answered by logging in

### 5. Others deciding the same thing
- **ADR 0006** is the parent — 0023 opens "ADR 0006 leaves an Account whose
  Credential cannot be recovered", and `Quarantine::RotationLost`'s doc comment
  calls itself "ADR 0006's crash between two writes" (`src/registry.rs:67-71`).
  The Quarantine state is arguably one decision spread over the two documents.
- **ADR 0054** decides what this command is *called* and why, on evidence taken
  entirely from 0023's Consequences. They are two halves of one record about
  `perch relogin`.
- **ADR 0048** shares the `make_live` door and the "repairing the Account you are
  on" path, and names `perch relogin` as the way out of an unresolvable Landing.
- **ADR 0057** rules on `relogin`'s locking shape (shape 2, no `still_ours`) and
  on it not being destructive.

### 6. Citations
8 total — 5 in `docs/`, 3 in code and elsewhere. Files: `README.md`,
`src/commands/relogin.rs`, `pages/src/content/docs/accounts.md`,
`pages/src/content/docs/reference.md`, ADR 0054.

**Load-bearing.** Both `relogin.rs` sites (`:328`, `:365`) cite 0023 for a
specific hazard — "the very next `perch switch` would Capture that broken copy
over the fresh one (ADR 0023)" — and one of them explicitly marks the limit of
0023's defense ("ADR 0023 names this hazard and defends against it inside
`relogin`, but not on the ordinary command a user reaches for next"), which is a
comment doing real reasoning rather than tagging. The `reference.md:46` cite is
closer to decorative (an exit-code table row).

---

## ADR 0024 — Removing the active Account lands somewhere before it deletes anything


### 1. The decision, in one sentence
`perch remove` destroys an Account, and where that Account is the active one it
first names a successor and lands on it — same Group where possible, never a
Quarantined or disabled Account, not ranked, agreed to in front of the user —
then deletes the Credential, then writes the registry, in that order, so that the
one unretryable state (registry forgets the Account while its Credential is still
on disk) cannot occur.

### 2. Does it still hold?
**Holds**, clause for clause, and the ordering argument is intact.
- Module doc restates it and cites it: `src/commands/remove.rs:10-14`.
- Successor: `successor()` at `src/commands/remove.rs:239-250` — Group first
  (`in_its_group`), otherwise `candidates().next()`, gated by
  `cycle::is_a_candidate`, which is where "never Quarantined, never disabled"
  now lives (`:223-231` explains the delegation: "a Remove choosing an Account no
  Cycle would is the same divergence arriving through a third door").
- Not ranked: `candidates().next()` takes registry order, never
  `cycle::ranked`.
- Named and agreed to first: `Consequence::is_asked_about` (`:66-74`) and
  `agreed()` (`:257-276`), and the asked cases are exactly "active" or "last
  Account left".
- Order landing → delete → save: `run()` at `:141-166`, with `land_on` before
  `delete_the_credential_and_its_profile` before `registry::save`, and the save
  carrying a `with_note` about the exact unretryable state (`:160-165`).
- The pair of refused Profiles, shared with `relogin`:
  `refuse_while_anything_is_running` → `switch::refuse_if_live_anywhere` with
  `WHY_THE_DEFAULT_PROFILE` (`:170-194`); `relogin.rs:35-44` has the same
  constant with the same doc comment.
- Nowhere-to-land still allowed: `remaining == 0` is an asked case rather than a
  refusal (`:66-74`).
- Two clauses have gone stale without going wrong:
  - "`perch remove` is the only command that destroys something" — `perch
    holdings purge` destroys the whole Holdings (`src/commands/purge.rs`), and
    ADR 0057's own table treats `remove` and `purge` as the two destructive
    commands. `remove.rs:4` repeats the overclaim.
  - "which makes `perch remove` the second command after `perch add` that has
    something to say on a machine with nobody at the terminal" — there are now at
    least four: `add.rs:57`, `remove.rs:268`, `purge.rs:189`,
    `commands/upgrade.rs:292`.

### 3. Supersession
Supersedes nothing; superseded by nothing; amended by nothing. ADR 0052 names it
as untouched: "ADR 0012 … and ADR 0024 ('never being chosen for you is the whole
of what disabled means') use the word as a lowercase state and are untouched."
ADR 0048 changes the machinery underneath it — `remove` is now a Switch path that
calls `switch::resolve_a_landing` first and `make_live` writes a Landing
(`remove.rs:82`, `:359-366`) — without touching 0024's decision.

### 4. Reasoning that outlives the document
> Each step is only taken once the one that could still be undone has succeeded

and, verbatim, the sentence the codebase has since generalized twice:

> So Perch names the Account it will leave active, and lands on it first.

`src/commands/service.rs:302-305` calls this "ADR 0024's shape one level up.
Removing the active Account lands somewhere before it deletes anything; giving
the whole machine back stops the thing that writes to it before it starts", and
`src/commands/purge.rs:152-153` says "This is ADR 0024's rule at the scale of the
whole machine: land somewhere before deleting anything." Also worth carrying:

> Cycling ranks because it chooses unasked and has to justify itself (ADR 0012);
> this choice is named in front of the user and does not happen until they agree
> to it

— a general rule about when a ranking is owed, which nothing else states.

### 5. Others deciding the same thing
- **ADR 0057** is the closest: it rules on `remove`'s lock shape, its double
  liveness ask, and why `remove.rs:296-316` writes `active` mid-destruction —
  which is 0024's ordering rule seen from the locking side. They are not the same
  decision, but they are the two records about how `perch remove` is sequenced,
  and 0057 cites the ordering as load-bearing.
- **ADR 0048** now owns the write-it-down-first half of every path `remove`
  takes.
- **ADR 0002** supplies the Group-first premise; **ADR 0012** the reason not to
  rank; **ADR 0005** the Live-Profile refusals.
- Nothing else decides the successor rule.

### 6. Citations
9 total — 2 in `docs/`, 7 in code and elsewhere. Files: `tests/removing.rs`,
`tests/servicing.rs`, `src/commands/remove.rs`, `src/commands/purge.rs`,
`src/commands/service.rs`, `pages/src/content/docs/accounts.md`, ADR 0040.

**Load-bearing, and unusually so — two sites cite it for a rule they are
applying by analogy rather than obeying.** `src/commands/purge.rs:152` and
`src/commands/service.rs:302` both invoke "ADR 0024's shape/rule one level up" to
justify stopping the Service before a Purge; `tests/servicing.rs:807` names it as
"ADR 0024's shape, ADR 0040's rule". `tests/removing.rs:770` uses it to explain
why a lock failure is a retry rather than a fault. Only the `accounts.md` cite is
plain reference.

---

## ADR 0025 — A crate is taken where it does not cost a seam


### 1. The decision, in one sentence
The rule is *a crate is taken unless it would sit on the wrong side of a seam*,
where Perch has exactly two seams — the Host port and fidelity to what Claude
Code actually does — and the document records the completed audit of eleven
individual answers so the same argument is not had again.

### 2. Does it still hold?
**Holds, with two of its individual answers now describing a world that is
gone.** The rule and both seams are live.

Taken, and still taken:
- **`age`** — `Cargo.toml:63` `age = { version = "0.12.1", default-features =
  false, features = ["armor"] }`, with the file's own comment restating the ADR's
  argument including *"an `age` file is read by the standard `age` command"*.
- **`junction`** — `Cargo.toml:109`, Windows-only, `default-features = false`,
  with the comment citing 0025 for the "costs no seam" clause.
- **Randomized temp names in the shape the standard library gives them** —
  `src/host/mod.rs:721-736`, `temp_beside`, carrying the pid: *"a crate that
  generates a better one would want a real filesystem, and this sits behind the
  Host port where there is not always one (ADR 0025)."*

Not taken, and still not taken: no `keyring`, `security-framework`, `sysinfo`,
`which`, `dirs`, `fd-lock`, `fs4`, `reqwest`, `ureq`, `rpassword` or
`tempfile` in `Cargo.toml` or `Cargo.lock`. `src/json.rs` still splices by span
rather than parsing. `Host::read_secret` is still `tcgetattr`/`SetConsoleMode`
(`src/host/real.rs:1084`).

Two crates have arrived since, both decided by the ADR's rule but not recorded
in it: **`zeroize`** (`Cargo.toml:76`, whose comment argues the case in the ADR's
own terms — *"It sits on neither of Perch's seams (ADR 0025) and is already in the
tree through `age`"*) and **`unicode-width`** (`Cargo.toml:94`, and
`src/utilization.rs:37-39` — *"`unicode-width` sits on no seam — the width of a
string is a pure function of it — which is the test ADR 0025 actually sets"*).
The document does not list either; the Consequences' *"reopens the question, and
should say so here rather than in a commit message"* has been honored in Cargo.toml
and in `src/utilization.rs`, not here.

Dead sections, in a document that otherwise holds:
- The `Consequences` sentence *"A future change that removes one of those
  reasons — a `perch tui` that owns its own terminal, say"* — that future arrived
  and then left.
- The whole final section, **"Reopened by `perch tui`, and settled the same
  way"**, describes crossterm being *"now in the tree"*, which it is not, and
  ends by describing `tui/terminal.rs` and `tui::Screen`, which do not exist.
  ADR 0049 rules on this explicitly rather than leaving it to be discovered:
  *"**ADR 0025 is not amended.** … Crossterm leaving closes the reopening
  without moving a line, because the answer was already that both stay where
  they are. The section becomes history rather than a live tension."*
- `Consequences` says *"seven places"*; the *Not taken* list has eight entries
  and the number was already approximate.

### 3. Supersession
Supersedes nothing and is superseded by nothing, and has been *deliberately left
unamended* twice from outside — by ADR 0049 (quoted above) and by ADR 0056:

> ADR 0025 stands unamended, and so does ADR 0055's restatement of it: `&dyn
> Host` remains the only port Perch has. This is nine names for one port's
> surfaces, not nine ports.

It functions as the consolidation point for ADRs 0008, 0014, 0021 and 0026,
restating each without superseding any.

### 4. Reasoning that outlives the document
The rule itself, which is the most portable sentence in the slice:

> **a crate is taken unless it would sit on the wrong side of a seam.**

and the naming of the two seams, especially the second, which is a claim about
sharing a machine with a program you do not control:

> Fidelity to what Claude Code actually does is the other: Perch shares files,
> keychain items and locks with a program it does not control, and a crate that
> is *nearly* compatible is worse than code that is exactly compatible.

and the smallest of the three, which decides an entire class of case:

> A file-lock crate would take a lock of its own design, which is a lock against
> nobody.

### 5. Others deciding the same thing
- **ADR 0008 and ADR 0021** — the two cases this document generalizes; each is
  already summarized inside it. If the three merge, 0025 is the survivor and the
  other two become its longest entries.
- **ADR 0056** — decides that `&dyn Host` is one port with nine names, which is a
  ruling on the first of 0025's two seams; 0056 explicitly declines to amend 0025
  and instead restates it.
- **ADR 0014** (`age`), **ADR 0026** (`junction`) — each supplies the occasion for
  one of 0025's *Taken* entries; 0025 duplicates their reasoning.
- **ADR 0016**'s surviving half — repealing color-eyre for a dozen hand-written
  lines is a 0025 judgment made before 0025 existed.

### 6. Citations
17 total: 7 in `docs/`, 10 in code and elsewhere — `Cargo.toml`, `src/cycle.rs`,
`src/export.rs`, `src/host/real.rs`, `src/host/mod.rs`, `src/utilization.rs`,
plus ADRs 0014, 0039, 0044, 0049, 0055, 0056.

**Load-bearing throughout, and used as an active test rather than a tag.**
`src/utilization.rs:37-39` applies the rule to a crate the document does not
mention and says which of the two seams it clears; `src/cycle.rs:2149-2153`
applies it to a *dev*-dependency the document does not mention — *"a fixed seed,
printed in every failure … and a dev-dependency for twenty lines of arithmetic
is not that"*; `src/host/mod.rs:689` cites it for the "one port" claim;
`src/host/mod.rs:727` cites it to explain why the temp-name randomness is a pid.
This is the ADR whose citations most often decide something new.

---

## ADR 0026 — Reconcile shares by denylist, and never copies


### 1. The decision, in one sentence
Two independent rulings: **which** entries cross from the Default Profile into
the Profile a Run launches is decided by denylist enumerated at Run time (not by
an allowlist, which would go stale on Claude Code's release schedule), and **by
what mechanism** is always a link and never a copy — where no link can be made
the Run is refused, naming the entry and the reason.

### 2. Does it still hold?
**Holds**, with the denylist grown from two entries to four by its own two
amendments, and a fifth rule added underneath it that the ADR does not mention.

- Both rules are the module's stated reason for existing:
  `src/reconcile.rs:9-20`.
- The denylist: `src/reconcile.rs:59-64`, `HELD_BACK: [&str; 4]` =
  `.credentials.json`, `.claude.json`, `sessions`, `.oauth_refresh.lock`. The
  doc comment above it (`:27-58`) ends: *"So the denylist ADR 0026 wrote as two
  entries is four, and the last two are one rule — an entry that answers a
  question about *this* directory means nothing in another one"* — which is the
  ADR's second amendment restated at the definition site.
- Enumerated at Run time, not listed in source: `reconcile` walks `shared` and
  filters against `HELD_BACK` (`src/reconcile.rs:160`), and the test at `:412`
  is driven off `HELD_BACK` rather than off a parallel list.
- **Undocumented extension:** the filter at `src/reconcile.rs:143-160` also holds
  back names *prefixed* by a held-back name — `.credentials.json.perch-tmp.4242`,
  `.claude.json.lock`, `.oauth_refresh.lock.perch-takeover`, `sessions.perch-tmp.*`
  (test cases at `:446-450`) — because a Run starting mid-write would otherwise
  enumerate a complete copy of `.credentials.json` and link it into another
  Account's Profile. That is a real refinement of "which entries cross" that
  lives only in the code.
- Link, never copy: `src/reconcile.rs:249-269` picks among `Link::Junction`,
  `Link::Symbolic` and `Link::Hard` per platform and file/directory, and
  `:350-361` is the refusal — *"Perch shares by linking and never by copying,
  because a copy diverges the moment it is edited (ADR 0026) — so the Run is
  refused rather than served one."*
- The Windows argument holds: junctions for directories and hardlinks for files
  are exactly the two kinds offered when `windows` is true, and symlinks are only
  ever tried, never required.
- The Consequence "`.claude.json` cannot be linked […] It is therefore the one
  file handled key by key" is `src/carry.rs` in its entirety.
- `CONTEXT.md`'s **Reconcile** entry matches the four-entry denylist word for
  word ("except the Credential, the file naming the Account, the directory of
  Markers and the refresh lock").

### 3. Supersession
Supersedes nothing, and is superseded by nothing. It is **amended twice, both
partially and both in place**, and only the "Which entries cross" section is
touched:

> **Amended by ADR 0027.** `sessions` is held back as well. […] The example below
> stands as it was written; the denylist is now three entries rather than two.

> **Amended again.** The refresh lock — `.oauth_refresh.lock` — is held back too,
> and for the same reason as `sessions` […] The denylist is four entries, and the
> last two are one rule.

The second amendment names no ADR — it is an in-place correction with no document
behind it, which is worth flagging to the assembler as an edge with no source
node. The "By what mechanism" section and both Considered-Options arguments are
untouched.

### 4. Reasoning that outlives the document
> "A copy is not a degraded share, it is a different feature: edit `CLAUDE.md`
> inside a Run and the real one silently does not have it, which inverts the one
> thing Shared State promises."

> "A refusal is recoverable. A silently diverged copy of somebody's memory is
> not."

and the denylist-vs-allowlist argument, which is a general claim about which way
to be wrong when a list you do not own is growing:

> "It is more precise and it goes stale on Claude Code's release schedule rather
> than Perch's. […] The denylist fails the other way: a genuinely new
> Account-scoped file would be shared until somebody noticed. That is rarer — it
> needs Anthropic to invent one."

### 5. Others deciding the same thing
- **ADR 0003** ("Perch writes one key of `.claude.json`") — 0026's own
  Consequences hand it the one file the denylist cannot help with, and
  `src/carry.rs` is the module that implements the hand-off. Together they are
  "everything crosses except the Account, and the one file that is both is done
  key by key" — one rule with two mechanisms.
- **ADR 0027** ("a run makes a profile live and a switch reads through it") —
  already amends 0026 and supplies the reason `sessions` is held back; the two
  documents share the same paragraph of argument.
- **ADR 0007** — 0026 cites it directly for the denylist's failure mode: *"it is
  the kind of assumption Perch already probes rather than believes (ADR 0007)."*
- **ADR 0010** — 0026 opens by deriving its whole premise from it (a Profile is a
  live configuration directory rather than storage).

### 6. Citations
21 total — 8 in `docs/`, 13 in code and elsewhere: `Cargo.toml`,
`tests/conformance.rs`, `tests/reconciling.rs`, five other ADRs,
`src/{probe,reconcile,lock}.rs`, `src/host/{fake,mod}.rs`.

**Load-bearing.** `src/host/mod.rs:508` cites it to explain why the port offers
three link kinds and refuses to fall back to a copy; `tests/conformance.rs:1082`
calls itself *"the half of this port that ADR 0026 turns on"*, i.e. the ADR is
what makes the suite exist. `src/reconcile.rs:56` cites it to record the drift
between what the ADR wrote and what the constant now holds — the most useful kind
of citation there is. The `Cargo.toml` mention is the one that reads as a tag,
justifying a dependency.

---

## ADR 0027 — A Run makes a Profile Live, and a Switch reads through it


### 1. The decision, in one sentence

`perch run` writes its own session marker naming **Perch's** pid before the launch
and removes it after (refusing the Run if it cannot), which makes the Profile Live;
writing into a Live Profile is refused in three different registers (Capture: exit
16; Renewal: degrade to cache, exit 0; `.claude.json` key: silently skipped) while
reading out of one is not; and `sessions` — plus the refresh lock — is held back
from Reconcile.

### 2. Does it still hold?

**Holds**, every clause.

- The marker written before the launch, naming Perch's own pid, removed on drop:
  `commands/run.rs:111` `let _live = probe::claim(host, &profile)?;` →
  `probe.rs:970` `claim`, `probe.rs:936` `impl Drop for Claim`. Written atomically,
  and a `sessions` that is a link is refused rather than written through
  (`probe.rs:975-999`) — a hazard the ADR did not name.
- Three fields and no invented `sessionId`: `probe.rs:908`
  `session_marker` → `{"pid", "startedAt", "writtenBy": "perch"}`.
- A Run that cannot write its marker is refused: the `?` at `commands/run.rs:111`,
  with the refusal text ending *"Nothing was launched."*
- Claimed **before** Reconcile and Carry rather than immediately before the launch
  — a tightening of the ADR, argued at `commands/run.rs:96-110` (a concurrent
  `perch remove` would otherwise delete the directory mid-Reconcile). The Carry then
  discounts the Run's own pid: `probe.rs:1129` `anything_running_but`.
- **Refused / Capture, exit 16**: `switch.rs:1426` `refuse_if_live_in` →
  `NotIdle::Live` → `PerchError::ProfileLive` → `EXIT_PROFILE_LIVE = 16`
  (`error.rs:39`).
- **Refused / Renewal, degrades, exit 0**: `observe.rs:667` `refuse_if_live` returns
  `Outcome::Failed` inside a `Step`, i.e. the cached figure is what you see.
- **Refused / Carry key, silent**: `carry::carry` returns `()`; `probe.rs:1114-1121`
  is the doc for why doubt reads as a client here.
- Renewal refused for **every directory the Credential could be in use from**:
  `observe.rs:563-590` — `in_use_from` is two directories for the active Account,
  with the ADR cited at `:582`.
- **Allowed / Switch onto an Account with a Run**, but refused where that Account is
  also the outgoing one: `tests/switching.rs:730` asserts the first, and the
  outgoing-side rule is `switch.rs:1409` `refuse_if_live`.
- **Second liveness check in `perch relogin`, after the login**:
  `commands/relogin.rs:119-131`, cited to 0027, with the note that the login ran
  against a directory of its own.
- **Allowed / reading Utilization** while a Run is on: `tests/refreshing.rs:306`.
- **`sessions` stops crossing, and the denylist is four**: `reconcile.rs:59`
  `HELD_BACK = [CREDENTIALS_FILE, IDENTITY_FILE, SESSIONS, REFRESH_LOCK]`.
- **`Host` grows one primitive**: `host/mod.rs:582` `fn process_id(&self) -> u32`,
  documented with 0027's own argument.

### 3. Supersession

Supersedes nothing. Superseded by nothing.

**Partially amends ADR 0026** — and the partiality is explicit on both sides. 0027:
*"This amends ADR 0026, which named `sessions` as an example of the Shared State
that must follow a person into a Run. It cannot."* 0026 carries the matching
banner in its own text, and confines the amendment to one line of the denylist:

> "**Amended by ADR 0027.** `sessions` is held back as well. … The example below
> stands as it was written; the denylist is now three entries rather than two."

0026 was then amended a second time, uncredited to any ADR, for the refresh lock —
*"The denylist is four entries, and the last two are one rule"* — which is the
state `reconcile.rs:59` is in.

It **extends without amending** ADR 0022 (writer half to 0022's reader half),
ADR 0010 (the Run gains a guarantee), ADR 0003 (borrows its amendment's rule for
the silent register), ADR 0018 (borrows its rule for the degrading register) and
ADR 0005.

### 4. Reasoning that outlives the document

The refusal-register principle, which is the general claim and the reason the three
paths are deliberately not leveled:

> "They are refused in the three different registers those paths already have, and
> deliberately not leveled to one. … Only the first of the three is a failed
> command, and saying otherwise would contradict two ADRs."

The cost asymmetry that decides which failures may refuse a Run:

> "Everything else a Run cannot do is a remark — a key that did not Carry costs a
> dialog — but this one is the whole protection … A person told beforehand has lost
> a command; one told nothing loses their work."

The self-corroboration construction, which is a general technique rather than a
Perch fact:

> "Perch's process began strictly before Perch wrote the file, so the ADR 0022
> check passes while the Run lives and fails the moment that pid belongs to anybody
> else. That is what makes a Run killed rather than exited safe: the marker outlives
> the process, and says nothing without it."

The Rotation-is-per-Account rule, which governs `observe.rs` independently of Runs:

> "A Rotation retires the refresh token for an *Account* rather than for a file, so
> every copy dies together."

And the third-kind-of-denylist-entry rule, now covering two entries:

> "an entry that answers a question about *this* directory meaning nothing in
> another one."

### 5. Others deciding the same thing

- **ADR 0022** — see 0022 field 5. Read/write halves of one mechanism.
- **ADR 0026** — 0027 edits its denylist and adds a third rule for what may not
  cross; the denylist has since taken a fourth entry under that same rule with no
  ADR at all. Any merge of 0026 must carry 0027's clause or the denylist in the
  document will disagree with `reconcile.rs:59`.
- **ADR 0010** — 0010 + 0027 is the complete `perch run`.
- **ADR 0005** — 0027's Renewal refusal *is* 0005's rule, widened from one directory
  to every directory a Credential could be in use from. That widening is a real
  decision and it lives in 0027 rather than in 0005.
- **ADR 0018** — 0027 cites it for the degrading register and depends on it holding.

### 6. Citations

45 total: **3 in `docs/`, 42 in code and elsewhere** — the most lopsided ratio in
the slice. `README.md`, eleven test suites plus `tests/common/mod.rs`,
`src/login.rs`, `src/probe.rs`, `src/watch.rs`, `src/reconcile.rs`,
`src/observe.rs`, `src/switch.rs`, `src/host/{mod,fake}.rs`,
`src/commands/{run,relogin}.rs`, `pages/.../running.md`, plus ADRs 0013 and 0026.

**Load-bearing throughout, and in one place the ADR is doing work outside its own
subject.** `host/mod.rs:582` justifies a whole port method by it; `probe.rs:983`
cites it for the *ordering* of the claim inside `claim`; `reconcile.rs:334` for why
`sessions` may not be linked; `observe.rs:582` for why `in_use_from` holds two
directories; `login.rs:65-70` extends 0027's argument to a case the ADR never
mentions — *"Perch is waiting on this login exactly as a Run waits on its client —
so ADR 0027's argument that a Run may corroborate its own Profile holds here word
for word."* `host/fake.rs` cites it five times to describe fake states built
specifically for the hazard (*"ADR 0027's reason for existing was a state no test
could build"*), which is a decision reshaping the test double.

## ADR 0028 — A Release is assembled by workflows this repository owns


### 1. The decision, in one sentence

`dist` (the cargo-dist generator) was evaluated and refused, so this repository
writes and owns all four packaging formats itself, with **one owner per
Artifact**: release-plz decides the version, writes `CHANGELOG.md` and stops at
the tag, and `release.yml` owns every file the Release consists of.

### 2. Does it still hold?

**Holds**, in every clause that is about the pipeline.

- One owner per Artifact: `release-plz.toml:8` `git_release_enable = false`,
  with the comment stating exactly the ADR's reason ("Letting release-plz create
  the Release too would publish a page with nothing on it"); the tag is its last
  act (`release-plz.toml:14-15`).
- `.github/workflows/release.yml:6-8` — the workflow starts on `push: tags: ["v*"]`
  and nothing else.
- Draft-then-publish, the property the ADR says a two-owner arrangement cannot
  have: `release.yml:160-163` (comment) and `:233-238` — `gh release create …
  --draft`, then `gh release edit … --draft=false` after archives, `SHA256SUMS`
  and `attest-build-provenance` are all on it.
- The four owned formats exist: `pages/public/install.sh`,
  `pages/public/install.ps1`, `packaging/homebrew/formula.sh`,
  `packaging/npm/build.mjs` + `packaging/npm/perch-cli/`.
- The "answered where it arises" costs are all real: shellcheck and the
  PowerShell parser on every pull request (`.github/workflows/ci.yml:237-243`,
  `:265-276`), `sh packaging/install-test.sh` (`ci.yml:249-251`),
  `sh packaging/npm/build-test.sh` (`ci.yml:263`), and the formula generator
  reading every checksum before writing anything so a missing archive is not
  half a formula (`packaging/homebrew/formula.sh:33-38`).
- The npm claim the ADR uses against `dist` is live and load-bearing:
  `packaging/npm/perch-cli/bin/perch.js:4-13` — no postinstall anywhere, one
  optional dependency per platform, with the ADR's own sentence about
  `npm ci --ignore-scripts` restated in the file.
- The provenance claim is live: `release.yml:196-198` and the installers'
  `gh attestation verify` (`pages/public/install.sh:134-142`).

**One stale clause of fact, not of decision.** "the npm shim was verified
against the real binary including the signal path a TUI depends on" — there is
no TUI (ratatui is gone: `Cargo.toml:89` "now that ratatui is gone"; ADR 0049
"Perch does not draw"). The signal path itself survives and its rationale has
been rewritten around `perch watcher run` rather than a TUI
(`packaging/npm/perch-cli/bin/perch.js:63-79`), and nothing in CI asserts it —
`packaging/npm/build-test.sh` asserts only the assembled tree, so that
verification remains the one-time manual act the ADR reports in the past tense.

### 3. Supersession

Supersedes nothing. Superseded by nothing; no document marks it.

Forward reach, not supersession: 0035 and 0063 both cite it as settled
reasoning. ADR 0035:100 — "That is ADR 0028's reasoning unchanged and now covers
the guide too: a sentence that is wrong is fixed by a merge rather than by
cutting a version." Note that 0035, 0062 and 0063 all cite "ADR 0028, ADR 0031"
as *the reason the site exists at all*, and neither 0028 nor 0031 contains any
sentence about a site or a versionless URL. That reason is real but is written
down only in the citing documents.

### 4. Reasoning that outlives the document

Three, verbatim:

- "The pipeline has one owner per Artifact. … That is why the GitHub Release is
  created as a draft and published only once the archives, the checksums and the
  attestations are all on it — there is no moment when the page exists and the
  files do not, which is not a property a two-owner arrangement can have."
- "for a program that holds credentials, "npm has the bytes" is a materially
  different claim from "npm ran a script that fetched something"."
- "A tool absorbs upstream change — a new npm resolution rule, a Homebrew
  formula deprecation — and this does not. When one of those lands it lands
  here, as a broken release." (The stated, unpaid-back cost; it survives any
  merge.)

### 5. Others deciding the same thing

- **0029, 0030, 0031** — 0028 identifies them as its own consequences in its own
  words: "they are most of the decisions in ADR 0029 through 0031, and each one
  would have been settled by whatever the generator emitted rather than by
  anybody here." That is a self-declaration that the four are one decision seen
  from four angles. 0028 is the natural umbrella.
- **0063** ("only the installers answer to a merge") — decides publication
  *timing* for the same artifacts 0028 decides ownership of; 0063:24-27 leans on
  0028 for it.
- **0035 / 0062** — adjacent (the site that serves the installers), cross-slice.

### 6. Citations

4 total: 3 in `docs/` (0035, 0062, 0063), **0 in code**. The one Artifact-owner
decision that most needs a comment gets one *without* the tag —
`release-plz.toml:1-11` and `release.yml:160-163` both restate 0028's reasoning
verbatim and name no ADR. **Judgment: the three doc citations are load-bearing**
(0035:100 and 0063:27 each carry the argument forward and depend on it), but
0028 is invisible from the code that implements it — the strongest evidence for
it is uncited prose.

---

## ADR 0029 — A Linux build is musl and static


### 1. The decision, in one sentence

Both Linux Targets are `-unknown-linux-musl` and statically linked, so the set
of Linuxes a Release runs on is not decided by whatever glibc a GitHub runner
happens to have — and the consequence is that the npm platform packages must
carry **no** `libc` field.

### 2. Does it still hold?

**Holds**, every clause.

- `.github/workflows/release.yml:97-99` — `x86_64-unknown-linux-musl` and
  `aarch64-unknown-linux-musl`, and no `-gnu` target anywhere.
- The "belt and braces" `musl-tools` step is exactly that:
  `release.yml:121-123`, conditional on `endsWith(matrix.target, '-musl')`.
- The consequence: `packaging/npm/build.mjs:25-27` — "No `libc` field on the
  Linux packages, deliberately. … declaring `libc: ["musl"]` would be npm
  refusing to install a binary that" runs everywhere. No `libc` key is emitted.
- Named by architecture alone: `packaging/npm/build.mjs:32-33`,
  `packaging/homebrew/formula.sh:65-70`, `pages/public/install.sh:55-56`.
- The premise about Linux credentials still holds (ADR 0020): on Linux the
  Credential is a file in the Profile, so there is no libsecret to link.

The "arithmetic Perch never does" clause is unverifiable and uncontradicted.

### 3. Supersession

Supersedes nothing, superseded by nothing.

### 4. Reasoning that outlives the document

- "A `-gnu` build links against the glibc of the machine that built it and
  refuses to start on anything older. The machine that built it is a GitHub
  runner, whose glibc moves when GitHub moves it, so the set of Linuxes a Release
  runs on would be decided by an image upgrade nobody here made".
- "`libc: ["musl"]` reads as "this binary needs musl", and npm would refuse to
  install it on a glibc machine — which is precisely backwards. A static binary
  needs neither."

### 5. Others deciding the same thing

- **0028** — 0029 is one of the "decisions downstream of the archives" 0028
  names; it is the Target matrix half of the same packaging decision.
- **0020** (a Credential lives wherever Claude Code puts it) — 0029's central
  cost argument is a consequence of 0020, not an independent finding.
- Cross-slice consumer: `src/service.rs:29` reads the Target list as the reason
  `Platform::Other` may be treated as Linux, which makes 0029 a premise of the
  Service decision (0040).

### 6. Citations

2 total: 1 doc (0028), 1 code (`src/service.rs:29`). **Load-bearing.**
`src/service.rs:25-35` uses 0029 as a premise in an argument — five Targets, two
of them musl, therefore "the set of machines that are neither macOS nor Windows
and are running a Perch is exactly the set running Linux". `build.mjs:25-27`
carries the whole of 0029's most consequential clause and does not tag it, so
the citation count understates the reach.

---

## ADR 0030 — crates.io is not one of Perch's Channels


### 1. The decision, in one sentence

Perch is not published to crates.io, enforced by `publish = false` in
`release-plz.toml` rather than in `Cargo.toml` (where it silently made
release-plz skip the package entirely), because crates.io distributes source and
would advertise a `[lib]` API nobody consumes.

### 2. Does it still hold?

**Holds.**

- `release-plz.toml:26` `publish = false`, with `:17-25` spelling out why it
  cannot live in `Cargo.toml` — verbatim the ADR's argument, tagged `(ADR 0030)`.
- `Cargo.toml:16` carries the counter-pointer: "crates.io is not one of Perch's
  channels (ADR 0030), but that is settled in" release-plz.toml.
- The `[lib]` exists for the stated reason: `Cargo.toml:154-156`, and
  `release-plz.toml:52` `semver_check = false` with the comment "a promise
  nobody is relying on" — which is 0030's own "What this decides elsewhere"
  section.
- One clause the ADR does *not* mention has since been added and is a second
  consequence of the same fact: `release-plz.toml:47` `git_only = true`, because
  release-plz was otherwise diffing Perch against the unrelated `perch` crate on
  crates.io. This strengthens rather than contradicts 0030 — the name collision
  the ADR calls "the smaller reason" turned out to cost something concrete.

### 3. Supersession

Supersedes nothing, superseded by nothing.

### 4. Reasoning that outlives the document

- "release-plz reads that field as "this package is not one of mine" and skips
  the package entirely — no version decision, no changelog, no release pull
  request, and no error saying why."
- "crates.io is a Channel for libraries. What it distributes is source, and what
  installing from it means is `cargo install`: every user compiles Perch".
- "a crates.io entry would advertise an API nobody consumes and freeze it by
  advertising it."

### 5. Others deciding the same thing

- **0028** — named by 0028 as one of "the decisions in ADR 0029 through 0031"
  that a generator would have made instead. Same packaging decision, different
  face.
- **0031** — 0030 says which Channels Perch does *not* have; 0031 says how each
  Channel it does have behaves. They are two halves of "what a Channel is here",
  and CONTEXT.md's **Channel** entry enumerates four and cites only 0031.

### 6. Citations

2 total, **both in code**, zero in docs: `release-plz.toml` and `Cargo.toml`.
**Load-bearing, and unusually so** — `release-plz.toml:17-25` is a ~10-line
comment that reproduces the ADR's whole failure story, and `Cargo.toml:16`
exists purely to stop a future reader putting `publish = false` back in the
place where it broke. These are the two files where the mistake would recur, and
both are guarded.

---

## ADR 0031 — Being pre-1.0 is carried by each Channel's own idea of default


### 1. The decision, in one sentence

Perch's being unfinished is signaled per-Channel in that Channel's own terms —
Homebrew by being a Tap you must add deliberately — and the two Channels with no
such term (npm's `latest`, the GitHub Release's `latest` endpoint) are published
normally rather than distorted, so being pre-1.0 is said in prose and in the
version number instead.

### 2. Does it still hold?

**Holds** as shipped behavior; every clause is implemented.

- npm published to `latest` during 0.x: `release.yml:317-320` (`npm publish …
  --access public --provenance`, no `--tag`), with `:283-289` carrying the ADR's
  argument and the tag `ADR 0031` at `:308`.
- "`release.yml` asserts that it does": `release.yml:333-359`, the
  `latest points at this release` step, polling `npm view --prefer-online
  perch-cli@latest version` against the version just published, failing after
  five minutes. This is the fix for the check the ADR describes as "reporting
  success the whole time because it only compared `latest` against the version
  being published".
- Homebrew is a Tap: `release.yml:364-410` pushes to the tap repository;
  `README.md:31` and `docs/releasing.md:124-127` document `brew tap
  perch-cli/perch`.
- Not marked as a prerelease: `release.yml:219-238`, comment "Not marked as a
  prerelease, even during 0.x. … marking it otherwise would empty the
  `releases/latest` endpoint that the installers ask." The endpoint is
  `src/upgrade.rs:21` `LATEST_URL`, and `src/upgrade.rs:18-20` names 0031 as the
  reason it is populated.
- "said where people read": `README.md:27`, `pages/src/content/docs/index.mdx:141`,
  `pages/src/content/docs/installing.md:7,52`, `docs/releasing.md:132-142`.
- The "What ends it" floor is process, **unverifiable from code** beyond the
  existence of a weekly scheduled run; no 1.0 has been tagged (`Cargo.toml:3`,
  `version = "0.2.0"`).

### 3. Supersession

Supersedes nothing, superseded by nothing. No document marks it.

`src/upgrade.rs:18-20` is the only place 0031 reaches into shipped Rust, and it
reaches *forward*: ADR 0039's `LATEST_URL` works only because 0031 refused the
prerelease flag.

### 4. Reasoning that outlives the document

- "The registry attaches `latest` to a package's first version whatever `--tag`
  said, and then refuses to remove it … So the choice was never between
  "`npm install perch-cli` fails" and "it works". It was between "it works" and
  "it silently installs whichever version `latest` happened to be stuck on,
  forever"."
- "The Release page is the source of truth every other Channel points at, and a
  source of truth that lies about which version is newest is worse than an
  unadorned one."
- "marking it otherwise would empty the `releases/latest` endpoint that both
  installers ask, which would break the Channel in order to decorate it."
- "Being pre-1.0 is said where people read — the README, the version number
  itself — rather than carried by making one Channel behave unlike every other
  package on it."

### 5. Others deciding the same thing

- **0028 / 0029 / 0030** — 0028 names 0031 as one of its own consequences.
- **0063** ("the guide describes a Perch you can install") and **0035** — both
  cite 0031 for why the site exists at the root; that reasoning is written in
  *them*, not in 0031.
- No merge partner outside packaging.

### 6. Citations

8 total: 5 in docs (CONTEXT.md, `docs/releasing.md`, 0035, 0062, 0063), 3 in
code-and-elsewhere (`src/upgrade.rs`, `.github/workflows/release.yml`, and
`docs/releasing.md` counted there). **Mixed.**
Load-bearing: `release.yml:219-223` and `:302-308` each carry the whole argument
and would be inexplicable without it; `src/upgrade.rs:18-20` states a genuine
dependency ("the reason ADR 0031 refused to mark a Release as a prerelease:
doing so would empty this"); CONTEXT.md's **Channel** entry cites it for a
specific fact ("npm's cannot be withheld at all"). Decorative: the 0035 / 0062 /
0063 sites, which cite 0031 for a claim 0031 does not make.

---

### Question 2 — the surface the new 0.x semver-lite stance touches

The new stance (public announcement; breaking changes allowed on minor bumps and
recorded in `CHANGELOG.md`; on-disk state gets a migration or a refusal, never
silent corruption) touches **six** sentences of 0031 and no others. Marked, not
rewritten:

1. **Title** — "Being pre-1.0 is carried by each Channel's own idea of default."
   The carrier moves: under the new stance it is carried by `CHANGELOG.md`'s
   breaking markers and by a refusal-or-migration on disk. The Channels stop
   being where the statement lives.

2. **¶1 S1** — "Perch is published before it is finished." Softly touched: the
   new stance's claim is not that Perch is unfinished but that its breakage is
   disclosed.

3. **¶1 S2** — "Every Release is real and installable; **none of them should be
   what somebody arrives at without meaning to.**" This is the load-bearing
   sentence the new stance contradicts head-on. A public announcement is an
   invitation to arrive at it on purpose; the second clause is the premise the
   whole document is built on and is the one that goes.

4. **Homebrew ¶** — "`brew tap perch-cli/perch` is already a deliberate act
   nobody performs by accident, **so the Channel is the opt-in** and no second
   formula is needed." The *fact* survives (a Tap is still a Tap); the
   *justification* — that opt-in is what pre-1.0 needs — is what the new stance
   removes. `docs/releasing.md:141-143` already concedes the honest version:
   "Only one of the three is actually opt-in … the one-liner on GitHub Pages
   hands a pre-1.0 binary to anyone who pastes it, and always did."

5. **npm ¶, last sentence** — "Being pre-1.0 is said where people read — the
   README, the version number itself — rather than carried by making one Channel
   behave unlike every other package on it." The list of places is now
   incomplete: `CHANGELOG.md` becomes the primary one, and the version number
   acquires a specific meaning (minor = may break) it did not have here.

6. **"## What ends it" ¶1, both sentences** — "Tagging `1.0.0` is the whole of
   it." and "nothing else changes shape, because nothing else is holding a
   version back." Under semver-lite, 1.0.0 stops being the switch this paragraph
   describes: nothing is being held back *now*, and what 1.0 would change is
   which component of the version carries a break — a different claim from the
   one written here.

**Untouched, and worth saying so:** the entire npm history paragraph (the
`--tag dev` failure, the `400` on DELETE, the false-green check); the whole
GitHub Releases paragraph (the `releases/latest` endpoint argument, which ADR
0039 now depends on); and the "floor for taking it" paragraph — four consecutive
green weekly contract suites, which is a 1.0 gate the new stance does not move.

**And a gap:** 0031 contains **no sentence at all** about on-disk state,
`CHANGELOG.md`, or what a breaking change is. The migration-or-refusal half of
the new stance lands nowhere in this document. It lands on `CLAUDE.md`'s "Do not
write migration code … there are none" (which the new stance directly reverses),
on `src/registry.rs:20-27` and `:1848-1855` (the refusal of a registry written by
a newer Perch — already the "refusal, never silent corruption" half, and already
CLAUDE.md's declared forward-looking exception), and on ADR 0039's downgrade
confirmation, which is built on that refusal. `CHANGELOG.md` already carries
`[**breaking**]` markers inside 0.x minor bumps, so the recording half of the new
stance is current practice with no document claiming it.

---

## ADR 0032 — Perch does not look for its own updates


### 1. The decision, in one sentence

Perch will not check for newer versions of itself — no periodic request, no
cache, no age, nothing on `perch --version` — until a specific named report
arrives, because that machinery costs the same as the Utilization machinery
(0015, 0018) and buys only a notification in a tool whose value is care with
Credentials.

### 2. Does it still hold?

**Holds in part**, and the document says so itself.

Dead, by its own banner and by shipped code: `perch upgrade` exists
(`src/commands/upgrade.rs`, `src/upgrade.rs`, `src/main.rs:255` `Command::Upgrade`),
and `perch --version` makes a network request (`src/main.rs:360-368` →
`upgrade::version_report` → `upgrade::notice` → `upgrade::newest`,
`src/upgrade.rs:373-441`). The title is false.

Live, and asserted:
- No schedule, no cache, no age — `src/upgrade.rs:372-375`: "No cache and no
  interval: this happens because somebody typed a command, and the machinery for
  holding an answer and reasoning about its age is the thing ADR 0032 refused and
  ADR 0039 did not build."
- `perch status` still silent on the network. Nothing outside `src/main.rs`
  calls `upgrade::notice` or `version_report` (grep across `src/`), so no other
  command reaches out.
- The bounds: `PERCH_NO_UPGRADE_CHECK` checked before the request
  (`src/upgrade.rs:426-428`, asserted at `tests/upgrading.rs:910-919` including
  `host.http_calls().is_empty()`); non-tty suppression
  (`src/upgrade.rs:433-435`, `tests/upgrading.rs:924-930`); a 2-second cap
  (`src/upgrade.rs:28`, `tests/upgrading.rs:953-965`); silent abandonment on any
  failure (`tests/upgrading.rs:935-949` — offline, 403, non-JSON, tagless).

### 3. Supersession

**Superseded by ADR 0039, and the supersession is explicitly partial.** Both
documents say so, and this is the whole of question 1.

0032's own banner:

> **Superseded by ADR 0039.** Perch now carries `perch upgrade`, and
> `perch --version` says when a newer Release exists. Most of the argument below
> survived — there is still no schedule, no cache and no age, and `perch status`
> is still silent on the network — but the title is no longer true, and the
> reopener this ADR asked for is not what happened. ADR 0039 says which parts it
> kept and what it gave up. Left here as written, because what it refused is the
> thing that has to go on being refused.

0039's opening: "Supersedes ADR 0032, which said Perch does not look for its own
updates. It now does, in two places and on purpose, and the title of that ADR is
no longer true. **What survives of its argument is set out at the end, because
most of it does.**"

0039's closing section, "What this gives up, which ADR 0032 was protecting", is
the itemized accounting: "`perch --version` now makes a network request, and it
did not before. That is the cost, and it is a real one: 0032 named `perch
--version` as the thing that simply says what is installed." And: "What is *not*
given up is the shape 0032 actually refused. There is no schedule, no cache, no
age to reason about and no staleness to explain — the machinery it took ADR 0015
and ADR 0018 to get right for Utilization is not built a second time for this."
And: "And **`perch status` stays silent on the network**, which was the specific
harm 0032 identified".

So: **superseded in part, by a document that names the parts, with no
contradiction between the documents and the shipped code.** Precisely — the
title and the `perch --version` clause are dead; the anti-machinery argument and
the `perch status` clause are alive and are load-bearing constraints on 0039's
implementation. There is no undocumented drift here at all; this is the
best-documented supersession in the slice.

One further fact the assembler should have: 0039 records that 0032's own
reopener did **not** fire — "ADR 0032 named its reopener precisely … No such
report arrived. The author asked for the command because Perch now has Releases
and packages and it felt missing, which is exactly the reasoning 0032 was written
to resist." 0039 records this rather than dressing it up, and states the bet
being made. That admission is itself part of what a merge would have to carry.

0032 also lends a half to ADR 0033: 0033 says "ADR 0032 already refused to build
machinery for a problem nobody has reported — the half of it that still stands,
and the half ADR 0039 leaned on".

### 4. Reasoning that outlives the document

- "A version check is a periodic outbound request, a cache to hold the answer,
  an age to reason about, and a failure mode when the request does not come back.
  Perch already carries exactly that shape for Utilization … and it took two ADRs
  to get right for a figure the program is actually about."
- ""Perch phones home on a schedule" is a sentence that has to be true,
  explained and bounded, and it buys a notification."
- "`perch status` — cheap enough for a shell prompt, and deliberately silent on
  the network (ADR 0015) — stops being either."
- "this is a feature nobody has asked for, paid for in the one currency Perch is
  trying not to spend."

All four survive 0039 intact and are the constraints 0039 built inside.

### 5. Others deciding the same thing

- **0039** — the successor; a merge is the obvious move, but 0032's refusals
  would have to be carried whole, because 0039's implementation is only
  defensible with them attached.
- **0015 / 0018** — the machinery 0032 points at as the precedent. Not merge
  partners; 0032 is a *citation* of them.
- **0033** — shares the "no machinery for a problem nobody reported" principle
  and says so explicitly.

### 6. Citations

6 total: 4 in docs (0033, 0039 ×2, and its own banner), 2 in code
(`src/upgrade.rs:377`, `tests/upgrading.rs:864`). **Load-bearing.**
`src/upgrade.rs:372-377` uses 0032 to explain why `newest()` has no cache;
`tests/upgrading.rs:864-866` heads the whole `perch --version` block with "The
half of this that gives up what ADR 0032 was protecting", so the tests are
organized around 0032's surviving argument. A superseded ADR that is still
structuring a test file is unusual and is evidence the partial supersession is
real rather than clerical.

---

## ADR 0033 — The installer writes PATH only where it can be unwritten


### 1. The decision, in one sentence

`install.sh` prints the PATH line and writes nothing while `install.ps1` prompts
and writes a user PATH entry — one decision, not two, because the criterion is
whether the write can be *undone exactly*: an rc file is a document its owner
edits by hand and a line in it is indistinguishable from theirs, and a Windows
user PATH entry is a structured registry value from which one segment can be
removed precisely.

### 2. Does it still hold?

**Holds**, every clause, including the fine detail.

- `pages/public/install.sh:23` `INSTALL_DIR="${PERCH_INSTALL_DIR:-$HOME/.local/bin}"`;
  `:160-166` — "Nothing below writes to any file. … so the installer advises here
  and writes only on Windows, where a PATH entry is a registry value that can be
  removed exactly (ADR 0033)."
- Names the file for `$SHELL`, with fish's `fish_add_path` and a POSIX fallback
  for anything unrecognized: `install.sh:200-243`.
- The Debian-conditional case is implemented exactly as written, including the
  `$HOME`-unexpanded grep the ADR's reasoning requires:
  `install.sh:177-198` and `:210-217`.
- `install.ps1:168-178` — the reversibility argument tagged `(ADR 0033)`;
  `:191-200` — prompts on `[Environment]::UserInteractive` and prints the command
  otherwise.
- The documented way back is in the install guide, one line:
  `pages/src/content/docs/installing.md:37-42`.
- The Purge/Installation line the ADR draws is now glossary: CONTEXT.md's
  **Installation** — "an Installation outlives a Purge, and taking one back
  belongs to the Channel that made it rather than to any Perch command (ADR
  0033)".
- "Perch ships no uninstaller on any Channel, and never has" — still true;
  `perch watcher uninstall` removes a Service (Perch's own), not an Installation.
- The Unix installer writing nothing is *asserted*, not merely intended:
  `packaging/install-test.sh:204-215` fails on any stray file or directory under
  the fabricated home outside the install directory.

One incidental drift: the ADR says a Windows user "gets a 110-character
incantation". The current `installing.md` line is longer, and `install.ps1`'s
default is `%LOCALAPPDATA%\Perch\bin` (`install.ps1:18`), which the ADR does not
name. Neither changes the decision.

### 3. Supersession

Supersedes nothing; superseded by nothing.

It **leans on** 0032 and is leaned on by 0039. 0033's own words: "ADR 0032
already refused to build machinery for a problem nobody has reported — the half
of it that still stands, and the half ADR 0039 leaned on when it decided that
replacing an Installation belongs to the Channel that made it for the same
reason removing one does." 0039 returns the compliment: "That is the same line
ADR 0033 drew and the glossary already carried".

### 4. Reasoning that outlives the document

- "the line a Purge draws is not "everything Perch put on this machine". It is
  everything Perch **holds**, as against the Installation a Channel left behind."
  — this is now CONTEXT.md's Holdings/Installation split and 0039's whole
  premise.
- "An rc file is a document the user owns and edits by hand. A line Perch
  appends lands among lines they wrote themselves, and at removal time nothing
  can tell the two apart".
- "That matters because it is the argument someone will reach for first, and
  reaching for it hides the one that actually decides this." — a general claim
  about ADR writing.
- "the documented Unix install is `curl -fsSL ... | sh`, which leaves the pipe
  on stdin, so `[ -t 0 ]` is false and a consent prompt could only exist by
  reaching around the pipe to `/dev/tty`" — versus `irm … | iex`, which "executes
  in the session the user is standing in". A durable fact about the two install
  idioms.

### 5. Others deciding the same thing

- **0039** — the strongest partner in the slice. Both are *the same line*
  (Holdings vs Installation) applied to two acts: 0033 to removing/writing PATH,
  0039 to replacing. 0039 says so outright, and `install.ps1:155-160` places the
  `perch.exe.old` residue "on the side of the line ADR 0033 drew". They read as
  one decision about who owns an Installation, twice.
- **0034 / 0042** — `src/commands/service.rs:16-18` cites "reversibility being
  the line this codebase keeps drawing (ADR 0033, ADR 0034)". **Cross-slice
  hazard:** ADR 0042 is titled "the TUI does not write and *reversibility was the
  wrong line*". 0042 unseated reversibility as the criterion for the TUI; 0033
  still uses it as the criterion for PATH, and `commands/service.rs` still cites
  both. Whether 0042's rejection reaches 0033 is not settled by any document —
  worth a ruling.
- **0052** (a reversal is its own command) — adjacent notion of reversibility.

### 6. Citations

7 total: 2 in docs (CONTEXT.md, 0039), 5 in code (`pages/public/install.sh`,
`pages/public/install.ps1`, `src/commands/service.rs`). **Load-bearing.**
`install.sh:160-166` and `install.ps1:168-178` each state the criterion in the
place the code enacts it; `install.ps1:155-160` uses 0033 to justify where a
stray file may sit. The one weak site is `src/commands/service.rs:17`, which
pairs it with 0034 — a document superseded by 0042 on exactly the word being
cited.

---

## ADR 0034 — The TUI writes configuration, and reversibility is the line


### 1. The decision, in one sentence

`perch tui` may write anything it can unwrite — the Config tab, an Alias, and
whether Cycling may choose an Account — with a lock taken and released per edit,
debounced stepped values, and text entry admitted exactly once; and `perch config`
gains `unset` so the panel can reach no state a script cannot.

### 2. Does it still hold?

**Dead, twice over.**

The subject no longer exists. There is no `src/tui/`, no `src/commands/tui.rs`,
no `tests/browsing.rs`, no `docs/guide/tui.md`, and `ratatui` and `crossterm` are
absent from `Cargo.toml` — the only trace is `Cargo.toml:89`, which keeps
`unicode-width` as "A crate of its own now that ratatui is gone." Nothing in
`src/` mentions a debounce, a `Pending`, raw mode or a text mode. `perch tui` is
not a `Command` variant in `src/main.rs`.

The one thing 0042 preserved is also gone. `perch config unset` does not exist:
`ConfigCommand` in `src/commands/config.rs:56-75` holds `Set` and `Get` and
nothing else, and `Inherit`, `Override` and `Global` appear nowhere in
`src/config.rs`, `src/commands/config.rs` or `src/main.rs`. It was deleted by
**ADR 0051**, whose banner reads "`Override` and `Inherit` are gone … `perch
config unset` is deleted", and whose §"`unset` goes, and nothing replaces it"
gives the reason: "With no Inherit there is nothing to clear." ADR 0051 names
neither 0034 nor 0042 while doing it. So the sole surviving clause of this
document was repealed by a third decision that did not know it was repealing one.

The only surviving `perch config unset`-shaped thing is a different act:
`perch alias <name> --unset` (`src/main.rs:63-69`), which 0051 explicitly keeps
as "a different act, freeing a name rather than clearing a layer."

**One live code comment still cites this document's repealed axis.**
`src/commands/service.rs:17` — "reversibility being the line this codebase keeps
drawing (ADR 0033, ADR 0034)". It is the only citation of 0034 outside `docs/`,
and it invokes exactly the axis 0042 and 0057 both say was wrong.

### 3. Supersession

Supersedes ADR 0011's list-of-four ("`add`, `remove`, `purge` and `config` stayed
out") in part, on the argument that "The reasoning was right and the list was
wrong."

Superseded by **ADR 0042**, and the supersession is **partial by its own text**.
0034's banner: "**Superseded by ADR 0042.** … One thing here is not repealed:
`perch config unset` stays, on its own merits rather than the panel's, because a
two-layer configuration needs a way back to Inherit whoever is doing the
clearing." 0042 states the same reservation from its side, under "What ADR 0034
keeps": "One thing, and it is kept on merits rather than by inheritance."

The chain is deliberate and asserted twice. 0042: "ADR 0034 stays superseded by
this document; a chain is fine, and the route is worth as much as the
destination." 0049 repeats it verbatim: "ADR 0034 stays superseded by 0042; a
chain is fine, and the route is worth as much as the destination." So 0049 does
**not** supersede 0034 — it supersedes 0042 in full and leaves 0034 where 0042
put it.

**Net position: 0034 is now empty.** Its decision was repealed by 0042; the
single clause 0042 exempted was deleted by 0051. Nothing it decided survives.

### 4. Reasoning that outlives the document

Two sentences, and both survive *only as things later documents quote in order to
disagree with them*:

- *"A Setting is not irreversible. Nothing one can be set to destroys anything."*
  Still true as an observation, and 0042 says so ("The observation was right and
  the conclusion is repealed"). It is not load-bearing anywhere now.
- *"the panel cannot reach a state a script cannot"* — the constraint that grew
  `unset`. 0042 promotes the *general* form of it and credits ADR 0011: "the
  constraint ADR 0011 imposed — that every interactive capability exist
  non-interactively too — improved the CLI in the one instance where it bit."
  That generalization belongs to 0011, not here, and 0049 collects the receipt
  ("removing it removes a name and no surface").

The one about reversibility — "the TUI may write what it can unwrite" — is the
opposite of outliving: it is quoted in two later ADRs (0042, 0057) as the wrong
axis. ADR 0057:129-133 — "it is wrong for the reason ADR 0042 already found it
wrong: 'reversibility really was the wrong axis.'"

### 5. Others deciding the same thing

- **ADR 0042 and ADR 0049** — the same subject, and the two documents that killed
  it. Any survivor is one of those.
- **ADR 0051**, which is where the last of it actually died; a merged record
  would have to say so, because neither 0042 nor 0049 records it.
- **ADR 0011** — 0034 exists only to move a line 0011 drew, and 0042 puts the
  line back where 0011 had it while saying 0011's text stands unamended.

### 6. Citations

11 total — 10 in `docs/` (ADRs 0042 and 0049 only), 1 elsewhere:
`src/commands/service.rs`.

**Decorative and, worse, stale.** The lone code site is a parenthetical tag
(`(ADR 0033, ADR 0034)`) appended to a claim about reversibility, which is the
one claim this document is no longer authority for; ADR 0033 alone carries that
sentence correctly. Everything else that cites 0034 cites it to overturn it.

---

## ADR 0035 — The guide is written once and the site renders it


### 1. The decision, in one sentence

The guide lives as one copy of markdown in the repository and the site renders
that copy rather than a duplicate of it — with mdBook as the renderer, a
hand-written landing page carved out of the book, and `tests/publication.rs`
holding the three hand-kept indexes in step.

### 2. Does it still hold?

**Holds in part**, and the surviving part is only the title sentence.

Live: one copy of the markdown, rendered by the site and read on GitHub —
`pages/src/plugins/guide-links.ts:11` names it (*"one copy of the markdown is
what ADR 0035 promised and ADR 0062 carried forward"*), and
`tests/publication.rs:1-14` asserts against it. Live too: the installers at the
root of the deployment, `pages/public/install.sh` and `pages/public/install.ps1`,
asserted by `the_installers_stay_at_the_root_of_the_site`
(`tests/publication.rs:214`).

Dead in every mechanism it named:
- `docs/guide/` does not exist. `ls docs/` gives `adr agents assets releasing.md`.
- `docs/theme/perch.css` does not exist. `pages/src/styles/perch.css` carries two
  of its rules and its header says the rest was deliberately not ported.
- `.github/actions/` does not exist — there is no `.github/actions/mdbook`, no
  tarball, no `sha256` pin. `.github/workflows/ci.yml:315-319` says
  `--frozen-lockfile` *"is what replaced the mdBook tarball's `sha256`"*.
- `packaging/pages/index.html` does not exist; `packaging/` holds `homebrew`,
  `npm` and `install-test.sh`. The splash is `pages/src/content/docs/index.mdx`.
- `SUMMARY.md` and `create-missing = false` do not exist; the sidebar is
  autogenerated (`pages/astro.config.ts`, the `sidebar` comment).
- The "three lists" section is void: `every_guide_page_is_named_by_every_index_of_the_guide`
  (`tests/publication.rs:156`) now iterates a one-element array —
  `let indexes = [("README.md", …)]`.

### 3. Supersession

Supersedes nothing. **Superseded by ADR 0062, explicitly and expressly in part.**
0035's own header does the carving: *"Two things below are void: the section
headed 'The landing page is not part of the book', and mdBook as what renders the
guide, along with the pin and the stylesheet that answered for it."* And then:
*"What survives entire is the sentence this document is named for — the guide is
written once and the site renders it — and so does the constraint underneath all
of it: the installers are served from the root of the deployment, from URLs that
do not move."* 0062 agrees from its side: *"What is carried forward from ADR 0035
entire, and is the reason this change is small: **the guide is written once and
the site renders it.**"*

A third edge: 0035's clause that the site deploys from `main` was carried into
0062 and then killed by **ADR 0063** — *"The clause about deploying from `main`
is void, and carrying it forward from ADR 0035 without reopening it was the
mistake."* So 0035 is superseded by 0062 in part and one of its surviving
clauses is separately voided by 0063.

### 4. Reasoning that outlives the document

Two, both already claimed by successors but stated here first:

- *"the guide is written once and the site renders it"* — the title sentence,
  which 0062 says survives entire.
- *"None of this fails loudly on its own. mdBook drops a page nobody listed in
  `SUMMARY.md` without a word; a link to a heading that has been reworded is a
  404 somebody else finds; an installer URL that moves is a command already
  pasted into shells that now downloads nothing. So each of those is asserted."*
  That is the general reason `tests/publication.rs` exists at all, and it is
  paraphrased in that file's header today with no renderer in it.

### 5. Others deciding the same thing

**ADR 0062** — the same decision retaken with a different renderer; 0062 already
absorbs 0035's reasoning and quotes what survives. **ADR 0063** — the third
document in the same chain, deciding *when* what 0035/0062 built is published.
0028 and 0031 are cited by all three as the reason the site exists at the root at
all, but they decide releases and channels rather than the guide.

### 6. Citations

11 total — 10 in `docs/`, 1 elsewhere: `docs/adr/0062`, `docs/adr/0045`,
`docs/adr/0063`, `pages/src/plugins/guide-links.ts`.

**Load-bearing**, thinly. The one code site (`guide-links.ts:6-15`) is the model
case: the whole plugin exists because two spellings of a link have to be
reconciled *and there is only one copy of the markdown*, and the comment says so.
Every other citation is another ADR arguing with it.

---

## ADR 0036 — Held promises nothing was changed, and the loop acts on it


### 1. The decision, in one sentence

`PerchError::Busy` (exit `20`) is a **promise about the machine** — nothing was
changed, ask again shortly — rather than a description of a lock failure, so a
refusal may only earn it where nothing has been written yet; a lost hold
discovered at `registry::save` after a Credential has moved gets the general
failure code instead, and the three sentences about a lost hold are deliberately
not merged into one variant.

### 2. Does it still hold?

**Holds in part** — the decision holds, one clause of the "What follows" section
does not, and it was traded away deliberately.

Live:

- *The loop branches on `Busy` and only on `Busy`.* `src/commands/watch.rs:262`:
  `Err(PerchError::Busy(why)) => held_before_a_round(…)`, with
  `Err(other) => return Err(other)` on the next line. The comment names 0013 and
  0018.
- *`registry::save` refuses with a general failure, not `Busy`, when the hold is
  lost.* `src/registry.rs:2345-2361`, comment: *"A general failure rather than
  `Busy`, and deliberately (ADR 0036). `Busy` promises that nothing was changed,
  and `perch watcher run` branches on that promise by going round again — but
  this save is reached after a Switch has already moved a Credential as often as
  before anything has been written, and from here there is no telling which."*
- *`lock::take` losing a contended lock earns `Busy`.* `src/error.rs:312`
  (`PerchError::Busy(_) => EXIT_HELD`), and `take_the_watch`
  (`src/commands/watch.rs:363`) treats it as a hold.
- *A Check inherits it.* `commands::watch::check` (`src/commands/watch.rs:111`
  and `:141`) prints the held line to standard output and returns `EXIT_HELD`.
- *The three sentences are not merged.* Three distinct hand-written messages
  survive at `src/lock.rs`, `src/commands/mod.rs:137` and
  `src/registry.rs:2350`.
- *`switch::Landing::record` still exists and is still the case that matters*
  (`src/switch.rs:317`, `:368`), and the guard the ADR relies on —
  `switch::capture` declining a Credential whose Identity is somebody else's —
  is at `src/switch.rs:1188` (*"in a state Perch itself produces.
  `Landing::record` files the incoming…"*).

Not live:

- 0036 states *"`commands::still_ours` qualifies by construction — it is asked
  'before the first irreversible thing', which is its whole reason for
  existing"*, i.e. it should earn `Busy`. The code returns `PerchError::Other`
  and says why, citing 0036 itself (`src/commands/mod.rs:132-136`): *"A general
  failure rather than `Busy`, which reads as the obvious tidy-up and is not (ADR
  0036). Nothing **has** been written here … so the promise `Busy` makes would
  hold; what does not hold is folding this together with the two other sentences
  about a lost hold, one of which is reached after a Credential has moved."* So
  the code follows 0036's *Consequences* (do not merge the three) at the cost of
  0036's *What follows* (still_ours earns `Busy`). The two paragraphs of the ADR
  are in tension and the tree resolved it against the earlier one.

Also worth recording: the loop now returns `Ok(Outcome::Refused{..})` rather
than raising for a `Busy` arriving from *inside* `perform`
(`src/commands/watch.rs:1177-1185`). The comment is explicit that this was a bug
the ADR did not anticipate — a `Busy` from Claude Code's own lock was being
charged to the **Back-off**, *"whose whole definition is 'questions nobody is
answering'"*, dragging the Refresh cadence toward twenty minutes. That is a
refinement of 0036, not a contradiction: the promise is unchanged, only which
counter pays for it.

### 3. Supersession

Supersedes nothing; superseded by nothing. ADR 0040 leans on it — *"a **Check**
that cannot take it exits `20`, which is the code `lock.rs` already produces for
a contended lock and which ADR 0036 already defines as a promise that nothing
was changed"* (0040:102-104). ADR 0061 preserves it explicitly: *"ADR 0036
preserved rather than disturbed: a hold promises nothing was changed…"*
(0061:103), and `src/watch.rs:18` carries the same sentence.

### 4. Reasoning that outlives the document

> "So `Busy` is a promise about the machine and not a description of the
> failure."

And the argument against the tidy refactor, which is a general claim about error
taxonomies:

> "routing all three through `Busy` removes three hand-written sentences and,
> with them, the distinction the loop is branching on."

And the cost accounting, which is the model of the honest-cost move CLAUDE.md
prizes:

> "What a continued loop would cost is smaller and still not acceptable: every
> round after it reads and ranks the wrong Account, because the registry names
> one Account and the machine is acting as another, and it does so unattended
> and indefinitely."

### 5. Others deciding the same thing

- **ADR 0013's exit-code table** decides what `20` *means to a scheduler*; 0036
  decides what `20` *promises about the machine*. These are two halves of one
  contract and 0036 explicitly derives from 0013's `commands::watch` comment. A
  strong merge candidate into whatever survives of 0013.
- **ADR 0048** (`a-switch-is-written-down-before-it-moves-anything`) reshapes the
  exact state 0036's "the one that matters" section is about — `registry.active`
  is now one field with three states and a Landing is written before the
  Credential moves. 0048 does not name 0036, and 0036's argument survives
  intact, but a merged Watcher/Switch record would want them adjacent.
- **ADR 0061** governs the same held line's prose. Adjacent, not the same
  decision.

### 6. Citations

9 total — 3 in `docs/`, 6 in code and elsewhere: `CHANGELOG.md`,
`tests/watching.rs`, `src/watch.rs`, `src/registry.rs`, `src/commands/mod.rs`,
plus ADRs 0040 and 0061. The smallest citation count in this slot.

**Load-bearing, uniformly.** Every one of the three source sites is an
explanation of a choice that looks wrong without it: `src/registry.rs:2348` and
`src/commands/mod.rs:132` both refuse `Busy` *because of* 0036 and say so at
length; `src/watch.rs:1339` cites it for why refusals keep their prose whole
under ADR 0061's cutting. `tests/watching.rs:756` puts it in an assertion
message alongside 0061. There are no bare tags.

---

## ADR 0037 — Dogfood runs on real machines and proves only what each one holds


### 1. The decision, in one sentence

Build a suite that replaces nothing — no stubbed `claude`, no recorded
transcript — runs on the developer's real machine, opens with a Preflight that
counts and says out loud how many of its phases this machine can prove, opens
every run with a Repair, and refuses to run at all on a machine the setup wizard
has not marked.

### 2. Does it still hold?

**Dead.** The subsystem is absent from the tree entirely.
`grep -ril dogfood` over the repository returns only `CHANGELOG.md` and seven
files under `docs/adr/` — no hit in `src/`, `tests/`, `.github/` or `Cargo.toml`.
Specifically: `src/dogfood.rs`, `src/dogfood/phases.rs` and
`src/bin/dogfood-setup.rs` do not exist (`src/bin/` does not exist);
`tests/dogfood.rs` does not exist; `Cargo.toml`'s `[features]` holds
`your-machine` and `fakes` and no `dogfood`; there is no Dogfood job in
`.github/workflows/ci.yml`. `CONTEXT.md` has no **Preflight**, **Phase**,
**Phase zero**, **Attended** or **Attestation** entry, and **Marker** at
`CONTEXT.md:339` is now the session marker.

What survives is not the decision: the *four gaps* are still an accurate
statement of what is unproved, and three of the four have been answered
elsewhere (see 0041 and 0044).

### 3. Supersession

Supersedes nothing. **Superseded by ADR 0041** — as to the decision, **whole**;
as to the prose, expressly preserved. 0041: *"Both are repealed here, and the
subsystem they describe — `src/dogfood.rs`, `src/dogfood/phases.rs`,
`src/bin/dogfood-setup.rs`, `tests/dogfood.rs`, the `dogfood` feature and the CI
job that exercises it — is to be removed entire."* And 0041 explicitly refuses a
partial repeal: *"Why there is no reduced core."*

But 0037's own header carves out reasoning that is *not* repealed: *"Left here as
written, because the four gaps are still the right list and the rejection of a
stubbed `claude` still holds: ADR 0041 repeals the suite, not that argument."*
So: whole supersession of the decision, with two named survivals of argument.

### 4. Reasoning that outlives the document

Three, and all three are live somewhere in the tree today:

- The rejection of the stub, in its general form: *"a suite that replaces
  `claude` with a script printing a version string has rebuilt `FakeHost` at a
  higher price and in a worse language."* ADR 0044 reuses exactly this to refuse
  a stub `claude` on `PATH` (`tests/invoking.rs:15-18` restates it).
- The test that separates a legitimate replacement from an illegitimate one:
  *"what would still be proved if the replacement were perfect?"* — with the
  gloss *"The observer may be replaced; the subject may not."*
- *"The failure this exists to prevent is a green run that quietly proved a
  third of what it was asked to."* That sentence is alive in `ci.yml:209-211`,
  which passes `--nocapture` to the `your-machine` step because *"a suite that
  skipped itself and a suite that asserted look identical otherwise."*

### 5. Others deciding the same thing

**ADR 0038** — not a merge partner so much as the second half of one document:
0038 exists only to change what a Phase is, and both were repealed by one
sentence of 0041. **ADR 0041** — the repeal; 0037, 0038 and 0041 are three
documents about one subsystem's life and death. **ADR 0050** is the live
successor to the *question* 0037 opened (what may a test do to the developer's
own machine, and how is that asked for) — `Cargo.toml`'s `your-machine` feature
comment and `tests/your_machine.rs`'s header are where 0037's bargain now lives,
under a different and narrower rule.

### 6. Citations

5 total — all 5 in `docs/`: `docs/adr/0041`, `docs/adr/0038`. Zero citations in
code, which is the expected shape for an ADR whose code is gone.

**Decorative by necessity** — both citing documents cite it to repeal it or to
build on it, so the references are argumentative rather than explanatory of any
code that exists.

---

## ADR 0038 — A human performs, Perch judges, and attendance is never assumed


### 1. The decision, in one sentence

A Dogfood Phase may ask a person to *do* things and never to *decide* whether it
passed; attendance is claimed by `PERCH_DOGFOOD_ATTENDED=1` and then checked
against `Host::is_interactive`, so an opt-in nothing could answer refuses at the
top of the run instead of stalling in the middle of it; and the Preflight's
figure becomes an upper bound because a Renewal phase cannot know in advance
whether it can prove anything.

### 2. Does it still hold?

**Dead**, with one named survival.

Dead: `PERCH_DOGFOOD_ATTENDED` appears nowhere in the tree (the `dogfood` grep
returns only `docs/adr/` and `CHANGELOG.md`). There is no `Phase`, no `Needs`,
no `Halt`, no `Attestation`, no `Perch::ask` — no `src/dogfood*` at all. There is
nothing in the repository that asks a person to perform a step during a test.

Alive: `Host::is_interactive`, exactly as the header promises. Defined at
`src/host/mod.rs:646`, implemented at `src/host/real.rs:877` and
`src/host/fake.rs:2312`, and called from `src/commands/add.rs:57`,
`src/commands/remove.rs:268`, `src/commands/purge.rs:189`,
`src/commands/upgrade.rs:292`, `src/upgrade.rs:434` and `src/commands/mod.rs:225`.
The header's list said *"`add`, `remove`, `purge`, `tui` and `upgrade`"*; `tui` is
gone (ADR 0049), the other four are exactly right.

### 3. Supersession

Supersedes nothing; amends ADR 0037 (it changes what a Phase is). **Superseded by
ADR 0041**, and its own header claims a whole supersession with one explicit
exception: *"The removal landed in #148, so there is no Phase left to attend and
the rules below have no subject."* Then: *"One thing here is *not* repealed —
`Host::is_interactive` survives untouched."* 0041's side agrees and says why the
survival is not a partial supersession: *"`Host::is_interactive` stays. ADR
0038's reasoning about attendance was Dogfood-specific, but the primitive is used
by `add`, `remove`, `purge`, `tui` and `upgrade`, and none of that changes."*

Note the asymmetry worth recording: 0038 says two of its ideas are *released* by
0041 rather than migrated — *"named in ADR 0041 as ideas deliberately released
rather than overlooked: the Preflight's figure, and the refusal to let a verdict
be a keystroke."*

### 4. Reasoning that outlives the document

Three, and 0041 §"Two ideas released on purpose" already re-states two of them
verbatim so they are recoverable if 0038 dies:

- *"A verdict that is a keystroke cannot be told apart from a `y` typed to get
  out of the way, on the fourth browser round trip, at eleven at night."*
- *"Detection alone is not enough, because a terminal is not a person."* —
  with the mirror clause *"A flag alone is not enough either"*, i.e. an opt-in
  and a capability check must point opposite ways. This is a general claim about
  interactivity that no longer has a Dogfood subject but still describes what
  `Host::is_interactive` is and is not good for.
- The honest-skip rule: *"What it forbids is a run that **quietly** proved a
  third of what it was asked to. A run-time skip is named, counted and in the
  report, with a reason somebody can act on."* This is live in a place 0038 never
  reached: `tests/your_machine.rs` prints `skipping: …` lines and `ci.yml:215`
  runs it with `--nocapture` for precisely this reason.

### 5. Others deciding the same thing

**ADR 0037** — one subsystem, two documents, one repeal. **ADR 0041** — the
repeal, which already carries 0038's two best sentences forward. **ADR 0050** —
the live document on when a test may touch what the developer owns; it decides by
a build flag rather than by an attendance claim, but it is the same underlying
question (consent, not capability) and its own words are *"a gate asks for
consent"*.

### 6. Citations

4 total — all 4 in `docs/`: `docs/adr/0040`, `docs/adr/0041`. `docs/adr/0040:268`
carries a Dogfood clause that 0040's own amendment header then voids
(*"Both phases went with the Dogfood suite (#148)"*). No code citations.

**Decorative** — one is a repeal, the others are inside amendment headers.

---

## ADR 0039 — An Upgrade goes through the Channel that made the Installation


### 1. The decision, in one sentence

`perch upgrade` exists and **routes rather than overwrites**: it works out which
of three Channels left this Installation from the path of its own executable
alone, hands the work to `brew` or `npm`, replaces the binary itself only for
the installer Channel (by running the installer script embedded with
`include_str!`), refuses a binary nothing placed, and adds one line to
`perch --version` bounded so tightly that 0032's refusals still hold.

### 2. Does it still hold?

**Holds**, in every clause, and it is the most heavily implemented ADR in the
slice.

- Routing: `src/commands/upgrade.rs:97-129` — Homebrew → `homebrew_command`,
  npm → `npm_command`, installer → `replace_it_ourselves`.
- `brew` derived from the prefix above the Cellar, not from `PATH`:
  `src/upgrade.rs:466-484`.
- npm-on-Windows prints instead of running, and reports `NothingToDo` rather
  than success: `src/commands/upgrade.rs:110-126`.
- Channel from the path, npm checked first: `src/upgrade.rs:148-175`, with the
  Cellar-under-npm-prefix reasoning at `:152-157`.
- `PERCH_INSTALL_DIR` above everything, per-platform defaults:
  `src/upgrade.rs:97-120`, matching `install.sh:23` and `install.ps1:18`.
- Anything else refuses: `src/commands/upgrade.rs:175-205`, naming the path and
  pointing at `--channel`.
- Embedded installer: `src/upgrade.rs:512-530` (`include_str!` of
  `../pages/public/install.ps1` / `install.sh`), written private, no execute bit,
  run with `PERCH_VERSION`, removed either way
  (`src/commands/upgrade.rs:361-410`).
- Windows rename-then-move with rollback: `install.ps1:125-161` ("A failed move
  puts the working binary back").
- Says what it checked, including the skipped one: `install.sh:129-143` quoting
  0039 directly, asserted at `packaging/install-test.sh:198-202`.
- Backwards allowed and named, with the registry consequence spelled out:
  `src/commands/upgrade.rs:275-315`, resting on `src/registry.rs:20-27` and
  `:1848-1855`.
- The `--version` line and all four of its bounds: see ADR 0032 above.
- One consequence 0039 states and the code implements beyond it: the Service is
  rewritten and restarted after a routed upgrade
  (`src/commands/upgrade.rs:131-152`, `src/commands/service.rs:443`,
  `tests/servicing.rs:236, 474, 962`).

One documentation error: 0039 cites the installer test as
`packaging/pages/install-test.sh`. The file is `packaging/install-test.sh` — it
was moved back out of `packaging/pages/` (git: `rename packaging/{pages => }/install-test.sh`),
which ADR 0062:173-175 records. A stale path, not a stale decision.

### 3. Supersession

**Supersedes ADR 0032, partially and explicitly.** See ADR 0032 §3 above for
the quotations; they are the answer to question 1 and are reproduced there in
full. In short: 0039 kills 0032's title and its `perch --version` clause, keeps
0032's anti-machinery argument and its `perch status` clause, and is the only
document in this slice that itemizes what it kept.

Superseded by nothing.

### 4. Reasoning that outlives the document

- "Replacing one is the same kind of act as removing one, and it was never going
  to be true that Perch could remove an Installation only by asking the Channel
  but replace one behind its back."
- "Every Channel points at the same Release and installs the same bytes, so a
  Channel stamped into the binary at build time would mean five builds per Target
  instead of one — paying for the answer in the place that is hardest to keep
  honest."
- "overwriting a file at a path Perch never wrote is the one irreversible thing
  this command could do."
- "A silently skipped provenance check is the single thing a tool built around
  being careful with Credentials should not do quietly."
- ""Perch downloads a script from the internet and executes it" is a sentence a
  program that holds Credentials should not have to defend".
- "the checksum and provenance checks would exist twice, in two languages, to
  drift apart at the first fix applied to one of them."
- "an upgrade that did not happen is a better outcome than a machine with no
  Perch on it."
- On the detached-PowerShell alternative: "it does the one dangerous step at the
  moment Perch has lost any means of reporting that it failed."
- "The confirmation names that consequence rather than asking a bare "are you
  sure", which is a question nobody has ever answered with information."
- "What was refused outright is *two* HTTP mechanisms, which is worse than
  either."
- The honesty paragraph: "The author asked for the command because Perch now has
  Releases and packages and it felt missing, which is exactly the reasoning 0032
  was written to resist."

This is the richest ADR in the slice for §4; several of these are general claims
that would be lost outright on deletion.

### 5. Others deciding the same thing

- **0032** — its predecessor, partially superseded. If anything in this slice
  merges, it is these two, and the merge must preserve 0032's four refusals
  verbatim or 0039 becomes indefensible.
- **0033** — same Holdings/Installation line, applied to a different act; each
  cites the other. Strong three-way candidate with 0032.
- **0040** (the Watcher may be run by the machine's service manager) —
  cross-slice; 0039's post-upgrade Service refresh is jointly decided
  (`src/commands/upgrade.rs:134` cites "ADR 0039, ADR 0040"). Not a merge, but a
  hard edge the assembler must not cut.
- **0021** (platform primitives are linked rather than shelled out) — 0039
  applies its absolute-path rule to `brew`, `npm` and `powershell.exe`
  (`src/commands/upgrade.rs:419-436`, which calls it "ADR 0021's rule, and the
  one program Perch runs that was outside it"). A live dependency.
- **0025** (a crate is taken where it does not cost a seam) — 0039 defers an
  HTTP crate on 0025's terms and says the argument belongs elsewhere.
- **0031** — 0039 depends on it: `LATEST_URL` is only populated because 0031
  refused the prerelease flag.

### 6. Citations

42 total — by a wide margin the most-cited ADR in the slice — 6 in docs, 36 in
code and elsewhere, across `src/main.rs`, `src/upgrade.rs`,
`src/commands/upgrade.rs`, `src/probe.rs`, `src/service.rs`,
`src/commands/service.rs`, `src/host/{mod,fake,real}.rs`, `tests/upgrading.rs`,
`tests/servicing.rs`, `packaging/install-test.sh`, `pages/public/install.sh`,
`pages/public/install.ps1`, `pages/src/content/docs/reference.md`, CONTEXT.md.

**Load-bearing, decisively.** Four sites read:
`src/host/mod.rs:118-125` justifies why `within_millis` is on the *request*
rather than the Host, by contrasting the two callers 0039 created;
`src/host/mod.rs:394-401` justifies `current_exe` following links because "every
Channel points at the same Release and installs the same bytes … the path it sits
at has to"; `pages/public/install.sh:131` quotes 0039's provenance sentence
inside the installer that enacts it; `src/commands/upgrade.rs:134-152` uses 0039
+ 0040 to explain a nine-line block about restarting the Service. None of these
is a bare tag; each is an argument that would be unreconstructable without the
document.

## ADR 0040 — The Watcher may be run for you by the machine's service manager


### 1. The decision, in one sentence

Perch will write, start and take back a per-user login unit — LaunchAgent,
`systemd --user`, or a Windows logon Scheduled Task — that runs the **same
loop**, unchanged, rather than a scheduled Check, because a Check has no memory
for the Back-off; and to make one Watcher per person per machine true, a Watcher
takes a lock, holds (rather than exits) on a withdrawn permission or a contended
lock, and says an identical hold once an hour instead of every round.

### 2. Does it still hold?

**Holds** in every clause, with one path rename (ADR 0047: `perch service
install` is now `perch watcher install`).

- *Three arrangements, one behavior.* `src/service.rs:14-20` tabulates them;
  `src/commands/watcher.rs:8-12` says the surface is one noun and five verbs.
  The Service runs `keep_watching`, not a timer over `check`.
- *Per-user, at login, never at boot; refused as root.*
  `commands::service::refuse_as_root` (`src/commands/service.rs:692`), called
  first in `install` (`:37`); `src/service.rs:22-25` restates the home-directory
  and macOS-keychain arguments.
- *Exactly one Watcher, by a lock.* `registry::watcher_lock_spec`
  (`src/registry.rs:1783`); the loop takes it *before* the opening line
  (`src/commands/watch.rs:181-189`) and **renews it twice a round**
  (`:174` and `:273`) — a refinement the ADR did not foresee, whose comment says
  a healthy Watcher was otherwise declared dead by the next Check.
- *A loop that cannot take the lock holds; a Check exits `20`.*
  `take_the_watch` (`src/commands/watch.rs:350-374`) vs. `check`
  (`src/commands/watch.rs:110-114`).
- *Permission holds rather than stops.* `Ok(Turn::NotArranged(why))` →
  `Spoken::held` (`src/commands/watch.rs:243-250`); a Check still raises
  (`:145-152`).
- *Identical holds said once an hour.* `watch::STILL_HOLDING_MILLIS`
  (`src/watch.rs:156`) and `Holding::holding`/`Holding::released`
  (`src/watch.rs:231-300`), whose doc is 0040's paragraph restated.
- *Standard output is the only sink; the log path is inside `$PERCH_HOME`.*
  `service::log_path` (`src/service.rs:137-141`) returns
  `perch_home(host).join("watch.log")` off Linux and `None` on Linux (systemd
  journals it).
- *The unit records the binary and the environment.*
  `service::binary_for_the_unit` (`src/service.rs:189-192`) has the Homebrew
  arm returning `prefix/bin/perch` rather than the resolved Cellar path, and the
  npm arm resolving the shim through; `service::CARRIED = ["PERCH_HOME",
  "CLAUDE_CONFIG_DIR"]` (`src/service.rs:127`), written only where set
  (`commands/service.rs:485-488`).
- *`status` re-checks the recorded binary.* `commands::service::status`
  (`src/commands/service.rs:190`).
- *A Purge stops and removes the Service first, and refuses if it cannot.*
  `take_back_before_a_purge` (`src/commands/service.rs:317-366`), called from
  `commands::purge` (`src/commands/purge.rs:154`) before anything is deleted.
- *An Upgrade re-installs and a failure is a warning.*
  `refreshed_after_an_upgrade` (`src/commands/service.rs:449-473`), called from
  `commands/upgrade.rs:152`, returning a warning string rather than an error.
- *`SIGTERM` sets the same flag as `SIGINT`, and the stop grace is thirty
  seconds everywhere.* `src/host/real.rs:1541-1580` (both signals wired to the
  same handler, with 0040 cited) and `service::STOP_GRACE_SECONDS = 30`
  (`src/service.rs:67`).
- *No exit code is new.* `src/error.rs` shows no code above `20`.

The trailing amendment is honest and still accurate:

> "**Amended by ADR 0041.** Both phases went with the Dogfood suite (#148), so
> nothing automated proves the three arrangements now — the paragraph above
> records what proved them, not what does."

`tests/servicing.rs` drives install/uninstall/status against `FakeHost` and
proves the Purge and Upgrade obligations; nothing proves launchd, systemd and
Task Scheduler behave as claimed, exactly as the amendment says.

Two constants the ADR did not name were derived from its reasoning and are worth
recording as its consequences: `service::RESTART_SECONDS = 30`
(`src/service.rs:78`) and a start-limit window, both justified in-file by "a
Watcher holds rather than exiting (ADR 0040)".

### 3. Supersession

**Supersedes ADR 0013 in part** — see 0013 field 3 for the exact clauses and
quotes. The scope sentence is 0040's first: *"Supersedes the part of ADR 0013
that rejected a managed background daemon. The rest of that record stands."*

**Amended in part by ADR 0041** (the Dogfood/Attended proof), by the header block
quoted above: *"Nothing else here is repealed: the decision, the exit codes and
the three arrangements all stand."*

**Explicitly not amended by ADR 0051**: *"**ADR 0047 and ADR 0040 are
unamended.** … 0040's held Service still holds on a grant it still reads, at a
Scope it already asks about."* (0051:262-264).

**Answered, not superseded, by ADR 0047**, which renamed `perch service *` to
`perch watcher *`. 0047:313 records the path change without touching the
decision.

### 4. Reasoning that outlives the document

The strongest general claim in the slot, and one that would be lost outright:

> "A Check scheduled at 02:00 mutates credentials with nobody watching, by
> exactly the same mechanism, with exactly the same hazards. Whatever unattended
> switching costs, Perch has been charging it since ADR 0013 shipped; a Service
> does not add a risk, it adds a way of arranging one that was already
> permitted."

The distinction 0013 conflated:

> "ADR 0013 conflated stopping *acting* with stopping *existing*, which was
> correct when the only Watcher was a loop in front of a person … and is
> incorrect for one nobody is sitting in front of."

Why one implementation beats two agreeing ones:

> "There is one Watcher, with three ways of being run, rather than two Watchers
> that have to be kept in agreement."

Why proof-of-life by repetition is worse than proof-of-life by duration:

> "a Watcher that has been held since 09:14 is obviously stuck in a way that the
> same sentence four hundred times is not."

The unit-path rule, which is a claim about Channels rather than about Perch:

> "The rule is the most stable path that runs without a shell, which is a
> different answer per Channel and not a single call to `canonicalize`."

And the trade that makes a long staleness window affordable:

> "Holding turns 'this machine is unusable for twenty minutes' into some held
> rounds that say why."

### 5. Others deciding the same thing

- **ADR 0013** — its partial predecessor. Merging 0013's surviving arithmetic,
  0040's arrangements and 0046's constants into one Watcher record is the
  obvious consolidation; 0046 refused it once, on the specific ground that a
  third *correction header* would be worse, which a rewrite does not incur.
- **ADR 0039** (`an-upgrade-goes-through-the-channel-that-made-the-installation`)
  owns the other half of the Upgrade obligation, and 0040's Homebrew/npm
  binary-path reasoning is 0039's Channel model applied. Related, but 0039 is
  about replacing an Installation and 0040 about a unit that points at one — two
  decisions.
- **ADR 0033** (`the-installer-writes-path-only-where-it-can-be-unwritten`) is
  the same *shape* of decision one level over — Perch writes only what it can
  take back. Worth quoting into a survivor as a shared principle; not a merge.
- **ADR 0047** decides the surface these commands sit on. Naming, not behavior.

### 6. Citations

**70 total — 10 in `docs/`, 60 in code and elsewhere.** `src/service.rs`,
`src/commands/{service,watch,watcher,purge,upgrade}.rs`, `src/watch.rs`,
`src/switch.rs`, `src/registry.rs`, `src/config.rs`, `src/main.rs`,
`src/host/{mod,real,fake}.rs`, `tests/{servicing,watching,configuring}.rs`,
`pages/src/content/docs/{watching,configuration}.md`, and five ADRs.

**Load-bearing.** `src/host/mod.rs:620-627` cites it for why `SIGTERM` is
claimed at all (*"a loop that did not claim it would be killed mid Switch"*).
`src/commands/watch.rs:233-238` cites it for why a withdrawn grant holds
(*"a supervisor respawns a deliberate exit until it gives up on the unit, and
launchd cannot be told otherwise"*). `src/service.rs:70-77` cites it to explain
why `RESTART_SECONDS` should almost never be used. `src/commands/watch.rs:181-189`
cites it for the two-Watcher state the lock renewal prevents. These are not tags;
in three of the four cases the ADR is the only place the constraint is written
down.

---

## ADR 0041 — A real machine proves Perch by being used, not by a suite


### 1. The decision, in one sentence

Delete the Dogfood subsystem entire rather than keeping a reduced core, because
three of the four gaps it stood in for are now covered by a person actually
living with the tool, the fourth (argv, exit codes, rendered lines) was never
Dogfood's to hold, and the conceptual cost of the machinery is the same at one
phase as at ten.

### 2. Does it still hold?

**Holds** as to the removal, and the replacement claim is the interesting half.

Removal: verified absent above (see 0037 §2). The freeing of the word **Marker**
landed — `CONTEXT.md:339-345` gives it to the session marker Claude Code leaves.
The **Repair** entry at `CONTEXT.md:220-225` has no Phase-zero clause. There is
no `Proving it works` entry for Dogfood; the heading exists at `CONTEXT.md:459`
holding **Behavior** alone (put back by #204, per 0045's own reversal note).

The replacement claim, checked against `tests/your_machine.rs` as the brief
asks — **the claim survives, but only because `your_machine.rs` is a different
kind of thing, and one clause of 0041 has become false in letter**:

- 0041: *"The repo is left with no test that drives `perch` as a process."*
  That is **no longer true** — `tests/invoking.rs` does exactly that
  (`Command::new(env!("CARGO_BIN_EXE_perch"))`, `tests/invoking.rs:50`), which is
  0044 answering the question 0041 deliberately left open. `your_machine.rs`
  does **not** drive `perch`: its only `Command::new` is
  `tests/your_machine.rs:488`, launching the *installed Claude Code* to ask
  `claude auth status`.
- `your_machine.rs` is not a revived Dogfood, and the evidence is structural. It
  has ten `#[test]`s, no Preflight, no phase vocabulary, no ordering, no wizard,
  no marker file, no `PERCH_DOGFOOD_ATTENDED`, no attendance, no Attestation, no
  report writer, no unwind. It never runs a Perch command; it calls library
  functions (`probe::claude_version`, `probe::credentials_file_for`,
  `perch::keychain`, `RealHost`) and asserts *beliefs about upstream*. Under
  0045's taxonomy it is a Correspondence, not Behavior and not Surface, and
  `tests/conformance.rs:24-31` says so out loud.
- It does not touch the four Dogfood gaps. It never runs `add` → `switch` →
  `run` → `status` in sequence; it never asks Anthropic to renew a token; it
  never launches a client against a Profile Perch made. The nearest approach,
  `claude_code_reads_the_credential_perch_would_write_for_a_config_directory`
  (`tests/your_machine.rs:447`), plants a fake credential in a scratch config
  directory and asks `claude auth status` whether it reads as logged in — that
  is a claim about a *path*, not about a Run.
- The bargain has, however, been inverted in one respect worth recording. 0037
  refused a hermetic suite and paid with determinism; 0050 refused a *default-on*
  suite and paid with a feature gate for the opposite reason — consent, not
  determinism. `Cargo.toml`'s `your-machine` comment: *"its outcome is not this
  repository's to determine (ADR 0050)"*. Same machine, opposite justification.

**Load-bearing tension the assembler should see.** 0041's closing sentence is
*"**This decision is paid for by the commitment to actually use Perch.** That is
the honest dependency. If nobody lives with the tool, the four gaps of ADR 0037
are not covered by real use *or* by a suite, and this ADR is a straight loss
rather than a trade."* `CLAUDE.md`'s opening section is headed **"Nobody is using
this yet"** and reads *"Perch has no installed base — not the author, not anyone
else."* The two documents state opposite facts about the same premise. Nothing in
the tree settles which is current; this is a finding, not a verdict.

### 3. Supersession

**Supersedes ADR 0037 and ADR 0038, whole** as to their decisions: *"Both are
repealed here, and the subsystem they describe … is to be removed entire."* The
wholeness is argued for rather than assumed — the §"Why there is no reduced core"
exists precisely to refuse a partial repeal, and its yardstick sentence is *"A
subsystem's cost here is what it makes a reader learn, and that cost is the same
at one phase as at ten."*

Two preservations that are *not* partial supersession: `Host::is_interactive`
stays (a primitive, not a decision), and 0037/0038 are *"Left here as written"*
by their own headers so their reasoning stays readable.

**Amends ADR 0040** — 0040's Dogfood phases went with the suite
(`docs/adr/0040:270`). **Nothing supersedes 0041.** ADR 0044 answers the question
0041 deliberately left open, which is a continuation rather than a supersession:
*"This ADR deliberately does not answer it."*

### 4. Reasoning that outlives the document

Four, all general:

- *"a stand-in loses to what it stands in for."* — quoted back verbatim by
  ADR 0044 when it refuses a gated binary-driving suite.
- *"A subsystem's cost here is what it makes a reader learn, and that cost is the
  same at one phase as at ten. So the choice is binary, and it goes the whole
  way."*
- The two ideas 0041 explicitly preserves so they can be found without the
  superseded documents: **the figure** — *"By counting what this machine can
  prove and saying the number out loud before anything acts, so a skip is a
  number rather than a line somebody scrolls past"* — and **attested versus
  asserted** — *"By letting a Phase ask a human to *do* things and never to
  *decide* whether it passed."* 0041 says why it wrote them down: *"releasing an
  idea and forgetting one should not leave the same trace."*
- The naming argument: *"The good noun is held by the throwaway and the
  load-bearing idea is unnamed."*

### 5. Others deciding the same thing

**ADR 0037 + ADR 0038** — the obvious merge: three documents, one subsystem, and
two of them are already headed by supersession notices. **ADR 0044** — the direct
continuation (0044 opens by quoting 0041's fourth gap and calling itself
"elsewhere"). **ADR 0050** — takes over the live question of what a test may do
to a real machine. **ADR 0042** and **ADR 0049** are the sibling deletions in the
same sweep and share the conceptual-surface yardstick, but decide different
subsystems.

### 6. Citations

21 total — **all 21 in `docs/`, none in code**. Cited by 0037, 0038, 0040, 0042,
0044, 0045, 0046, 0047, 0048, 0049, 0050, 0051, 0052, 0054.

**Decorative in form but structurally significant**: most of the 21 are the
formula *"Like ADR 0041, ADR 0042, ADR 0044 …, this is the artifact of a planning
effort rather than of a change"* in other ADRs' carried-out headers — a citation
of precedent, not of reasoning. The load-bearing ones are 0044's opening quotation
of 0041's fourth gap and 0044's *"ADR 0041 removed exactly that shape eleven days
ago"*. Zero code citations, which is correct: there is no code left to comment.

---

## ADR 0042 — The TUI does not write, and reversibility was the wrong line


### 1. The decision, in one sentence

The Config tab and every mechanism ADR 0034 built to make its writes safe are
removed and `perch tui` returns to the two acts ADR 0011 gave it — not because
0034's reversibility argument was refuted, but because the cost sat on the
writing itself; `perch config unset` is kept on merits.

### 2. Does it still hold?

**Dead as a decision about a surface; alive as a rule about writing.** Both halves
are void by 0049's own banner, and the code confirms it:

- The Config tab's removal is moot — the whole view went. No `src/tui/`, no
  `tests/browsing.rs`, no `ratatui`/`crossterm`.
- "The registry goes back to having one writer among Perch's interactive
  surfaces" is now vacuous: **Perch has no interactive surface at all** (0049's
  Consequences), and nothing in `src/` touches raw mode.
- The `Host` port narrowing 0049 ordered did happen: `src/host/mod.rs`'s trait
  ends at `fn note(&self, line: &str)` (`:676`) with no `print_remarks`, no
  `remarks`, no `aloud` cell.
- The thing it kept — `perch config unset` — is gone, deleted by ADR 0051. See
  0034 above. **0042 is therefore in the same position as 0034: its decision is
  void and its one exemption has since been deleted.**
- `registry::save`'s refusal against a lost hold, which 0042 said was "untouched",
  is untouched and live at `src/registry.rs:2345-2361`.

### 3. Supersession

Supersedes **ADR 0034**, partially: "One thing here is not repealed: `perch config
unset` stays" (0034's banner) / "What ADR 0034 keeps — One thing, and it is kept
on merits rather than by inheritance" (0042 §). It also explicitly declines to
touch ADR 0011: "**ADR 0011 is re-affirmed, not rewritten.** Its text stands
unamended. … Its *reason* is corrected here rather than there, because the
episode is worth keeping."

Superseded by **ADR 0049, in full, with one sentence carried forward.** 0049:
"ADR 0042 is superseded in full — both halves of its decision are void, since the
tab it removed goes with the view it preserved — but one sentence is carried
forward rather than buried, because it generalized past the thing it was written
about". 0042's own banner says the same from below: "**Superseded by ADR 0049.**
Both halves of the decision below are void … One sentence here outlives the panel
and is carried forward by 0049 rather than buried with it".

This is the cleanest whole-vs-partial case in the set: **full supersession of the
decision, explicit carry-forward of one sentence of the reasoning.** Note the
asymmetry with 0034 — 0034 is *partially* superseded (an exempted clause), 0042 is
*fully* superseded (a quoted rule, but no exempted clause).

### 4. Reasoning that outlives the document

The sentence, verbatim, as 0049 block-quotes it:

> a surface which writes at all pulls in locking, deferral, refusal and
> rollback, and a surface that only reads and acts does not.

**Is it still true of anything Perch currently has? Yes — three of its four terms
have direct live referents, and the asymmetry it asserts is visible in one file.**

The sharpest live instance is `src/commands/status.rs:37-50`. Bare `perch status`
reads and takes a non-exclusive read (`adopt::ensure_adopted`); `--refresh`
writes the cache and therefore takes the exclusive lock
(`adopt::ensure_adopted_exclusively`), with the comment giving the reason in the
sentence's own shape: "Exclusively only when there is something to write, which
is `--refresh` and nothing else." One command, both sides of the line, the lock
following the writing.

- **Locking** — `src/commands/mod.rs:150-165`: "Perch holds the registry lock in
  three shapes", every one of them keyed to a write.
- **Refusal** — `still_ours` (`src/commands/mod.rs:136`) and `registry::save`'s
  refusal against a lost hold (`src/registry.rs:2345-2361`). Both exist only for
  writers.
- **Rollback** — `Placed::undo` in `src/import.rs:350-379`; and `purge`'s
  Credentials-first-home-last ordering, which is rollback's cousin
  (re-runnability) at `src/purge.rs:11-15`.
- **Deferral** — this is the one term whose referent died with the panel. The
  debounce and its `Pending` are gone and nothing replaced them; the nearest live
  analogue is a lock held *across* an unbounded human wait, which is exactly what
  `still_ours` guards ("A question put to a person is the one wait in Perch with
  no bound on it").

So the sentence is not merely still true — it is the strongest surviving claim in
either document, and a survivor should carry it. Independent confirmation that it
already has readers outside this file: **ADR 0057:128-133** rebuilds a whole
cohort on it — "**Reversibility as the axis.** It is the obvious way to describe
the cohort … and it is wrong for the reason ADR 0042 already found it wrong:
'reversibility really was the wrong axis.'"

Two further general sentences worth keeping:

- *"'where am I' is a different question from 'what may I change', and a single
  page answering both answers neither well"* (quoted by 0042 from
  `src/tui/model.rs`, a file that no longer exists — this ADR is now the only
  place it survives).
- *"Judged together they get a verdict neither earns: the picker is worth having,
  therefore the TUI stays smuggles the panel past the yardstick without ever
  weighing it."*
- *"if the machinery is what costs, the machinery is what the decision is about"*
  (0042's form of it; 0049 quotes the same reasoning back when refusing a smaller
  picker).

### 5. Others deciding the same thing

- **ADR 0049** — the same subject, superseding this in full and already carrying
  its one live sentence. A merge here is nearly free: 0049 quotes the sentence,
  restates the reasoning, and covers the ground.
- **ADR 0057** — the strongest *outside-slice* partner. It is 0042's rule applied
  to the CLI: what a command reaches (the registry and nothing else) rather than
  whether it is reversible, with `only_the_registry` and `still_ours` as the
  carry-out. If 0042's sentence needs a home other than 0049, this is it.
- **ADR 0034** — its predecessor, and now equally empty.
- **ADR 0016** — the other picker ADR, superseded in part by 0049.

### 6. Citations

18 total, **18 in `docs/`, 0 in code or tests** — ADRs 0034, 0044, 0045, 0046,
0047, 0048, 0049, 0050, 0057.

**Load-bearing in the ADR corpus, absent from the source.** Nothing in `src/` or
`tests/` names this document; the subsystem it is about is gone. Where it is
cited, the citation carries weight rather than tags: 0057 rests an axis choice on
it, 0049 block-quotes it, 0045 uses its line count. It is the one ADR in this
slice whose whole readership is other ADRs.

---

## ADR 0043 — A sentence is asserted by the claim it makes, not by whether it changed


### 1. The decision, in one sentence

Decline snapshot baselines (`insta` or equivalent) — not for non-determinism,
which `FakeHost` removes — and adopt instead a rule about what an assertion must
claim: when the sentence is the claim, assert the whole sentence; when the datum
is the claim, assert the datum; with no sweep of the existing assertions.

### 2. Does it still hold?

**Holds in substance, with one mechanism dead.**

Holds: no snapshot dependency exists. `Cargo.toml`'s `[dev-dependencies]` is one
line — `perch = { path = ".", features = ["fakes"] }` — with no `insta`, no
`expect-test`, no `predicates`, no `assert_cmd`. The rule is cited at the point
of use and in its own terms: `tests/adding.rs:139` (*"Asserted whole, because the
claim *is* the sentence (ADR 0043)"*), `tests/exporting.rs:300` (*"One line,
asserted whole (ADR 0043)"*), and the datum half at `src/listing.rs:217` (*"A
machine reading a shape is not a person reading a sentence (ADR 0043)"*). Seven
test binaries carry the tag. The "no sweep" clause held: `contains(` now appears
918 times in `tests/` and 356 in `src/` — the count grew rather than being
audited down.

Dead in its particulars: **`said()` is gone**. 0043's §"The mechanism a baseline
would have replaced already exists" rests on *"`said()` in `tests/browsing.rs`
reflows a frame into one run of words"* and quotes
`assert_eq!(said(frame).matches("as of 4m ago").count(), 2)` as the stronger
claim a baseline would only make by accident. `tests/browsing.rs` was deleted in
`7c36fa1` (ADR 0049, *"Perch does not draw"*); `grep -rn "said(" tests/` returns
nothing. The two example assertions quoted at 0043:59-60 no longer exist in that
form either. The rule survives its example.

Also unfinished: §"What a baseline could see that this does not" promises a
hostile-width test tracked at #153. Nothing in `tests/` renders at a hostile
width — the only `hostile` in the suite is `tests/servicing.rs:328`, about shell
injection.

Also stale: the counts (933 `contains`, 720 literals, 303 single-word). ADR 0061
already re-counted at 1,241; today it is 1,274 across `src` and `tests`.

### 3. Supersession

Supersedes nothing and is superseded by nothing. It states its own boundary and
0045 and 0050 both re-state it as a clean axis: *"ADR 0043 governs what an
assertion must claim, ADR 0044 governs the level a claim is made at, and this
governs where a test lives and what its file is called. Three axes, no overlap"*
(0045); the same sentence with a fourth axis in 0050. 0043 itself defers two
questions rather than deciding them: *"Where output is captured. #142 asks
whether the suite should drive the real binary"* — answered by 0044.

### 4. Reasoning that outlives the document

The strongest set in this slice:

- *"A baseline is an excellent answer to *this used to be right and something
  broke it*. It has nothing to say about *this was never right*, which is the
  only failure mode with a track record here."*
- *"Determinism was free. Snapshots are declined despite that, not because of
  it."* — a discipline about naming the real reason rather than the convenient
  one.
- The rule itself, which is the thing to carry: **"When the sentence is the
  claim, assert the sentence"** / **"When the datum is the claim, assert the
  datum"**, with *"The distinction is what the test is about, and the test's name
  almost always already says which."*
- *"It is written down here so that the next person to propose a baseline finds
  the reasoning that declined it rather than 933 `contains` calls and the
  assumption that nobody thought about it."* — the general case for writing a
  refusal down at all.

### 5. Others deciding the same thing

**ADR 0061** is the closest thing to a merge partner outside this slice: it
decides what Perch says and when it explains itself, and it argues *against* a
line-count cap using 0043's own instrument (*"This is ADR 0043's instrument
pointed at…"*, `0061:147`) and re-counts 0043's figure. They are two halves of
"what a sentence is" — 0043 for the test, 0061 for the output — and both decline a
mechanical check for the same reason. **ADR 0044** and **ADR 0045** are the
sibling axes and explicitly disclaim overlap. **ADR 0053** and **ADR 0052** each
apply 0043's sentence/datum line to a specific surface (`0053:146`, `0052:162`),
which is evidence the rule generalizes rather than that they duplicate it.

### 6. Citations

21 total — 11 in `docs/`, 10 elsewhere: `tests/exporting.rs`, `tests/adding.rs`,
`tests/relogging_in.rs`, `tests/importing.rs`, `tests/purging.rs`,
`tests/removing.rs`, `tests/servicing.rs`, `src/listing.rs`, plus `docs/adr/`
0044, 0045, 0050, 0052, 0053, 0058, 0061.

**Load-bearing, and the best in the slice.** Read three: `tests/adding.rs:139-142`
explains why a line is asserted whole *and* why it is said once;
`tests/exporting.rs:300-303` says which line and why the claim is the sentence;
`src/listing.rs:216-219` invokes the *datum* half to justify emitting a key
unconditionally in JSON. None is a bare tag — each one explains a shape a reader
would otherwise be entitled to change.

---

## ADR 0044 — The binary is driven to prove its surface, and never the behavior behind it


### 1. The decision, in one sentence

Add one ungated integration suite that spawns the built binary via
`CARGO_BIN_EXE_perch` to prove the dispatch arms, the exit codes reaching a
shell, and `--help` — and hold a hard line at the probe: if a case would need a
real Claude Code installed, it has crossed into behavior, which stays with the
fakes.

### 2. Does it still hold?

**Holds.**

- `tests/invoking.rs` exists, ungated (it is not in `Cargo.toml`'s `[[test]]`
  exclusion list, which names only `your_machine`), and drives the binary:
  `Command::new(env!("CARGO_BIN_EXE_perch"))` at `tests/invoking.rs:50`.
- No `assert_cmd`, no `predicates` in `[dev-dependencies]` — the declining
  argument held.
- The line is restated in the file's own header, not just in the ADR
  (`tests/invoking.rs:11-18`): *"So this suite claims the surface and never the
  behavior behind it. The line is operational: **if a case needs a real Claude
  Code installed, it has crossed.** … which is why `switch`, `add`, `run`,
  `relogin` and `watcher run` are absent."*
- The two-level division holds: `src/main.rs:475` still opens a `#[cfg(test)]`
  module with 11 `#[test]`s asserting the parse tables, and `invoking.rs` has 13
  asserting what the process does.

Two details have drifted without touching the decision. 0044 counts *"Twenty
match arms"* in a 795-line `main.rs`; `main.rs` is 788 lines and the top-level
`Command::` arms are 15 (`src/main.rs:374-465`), after 0047/0054/0057 redrew the
command surface. And 0044's example refusal `perch tui --json` names a command
that no longer exists (ADR 0049); `grep -n tui src/main.rs` returns nothing.

### 3. Supersession

Supersedes nothing, is superseded by nothing. It is a **continuation of ADR
0041**, which it opens by quoting: *"argv, exit codes and rendered lines do not
close at all … they need a suite that drives the binary, and they are decided
elsewhere. This is elsewhere."* It also invokes 0041 as the reason it stops where
it does — *"That is refused, and refused by name, because it is Dogfood. Not
something like Dogfood — the same object."* It declines to change ADR 0043's rule
and declines to add to `CONTEXT.md`. 0045 places its output in a taxonomy without
superseding it: *"`invoking.rs` keeps the name ADR 0044 gave it."*

### 4. Reasoning that outlives the document

- The evidence rule, which is the most transportable thing here: *"An empty
  ledger here says nobody has looked, which is not the same claim at all, and
  treating it as one would be the reasoning that lets an unlit room stay unlit
  because nothing has been seen in it."* — an explicit correction to 0043's
  count-the-findings method, with the difference named.
- The boundary marker: *"The probe is not an incidental obstacle to be worked
  around with a fixture or a stub binary on `PATH`. It is the boundary marker. A
  command that must be told what Claude Code is has behavior to prove, and
  behavior is proved with the fakes."*
- *"A stand-in loses to what it stands in for"* re-applied to a second subject —
  evidence the sentence is general rather than about Dogfood.
- *"Making dispatch a pure function so it could be asserted in-process would
  introduce a seam to serve a test."*

### 5. Others deciding the same thing

**ADR 0041** — the direct predecessor; 0044 is the answer to a question 0041
posed and declined. Merging them would put the gap and its closure in one place,
at the cost of mixing a deletion with an addition. **ADR 0045** — places
`invoking.rs` as the sole "Surface" suite and quotes 0044's line as its
definition; the taxonomy and the level are two views of one arrangement. **ADR
0050** — the fourth axis of the same set, and the one that re-gated everything
0044's neighbors touch.

### 6. Citations

15 total — 14 in `docs/`, 1 elsewhere: `tests/invoking.rs`, plus `docs/adr/`
0045, 0046, 0047, 0048, 0050, 0055, 0056, 0058, 0060.

**Load-bearing.** The single code citation is the file header, which restates the
operational line in full so a contributor extending the suite meets the rule
without opening the ADR. The docs citations divide: `0060:101` and `0058:140` use
it as a live constraint (*"no rendered line (ADR 0044)"*, *"No argv, no flag, no
exit code changes (ADR 0044)"*), which is load-bearing; the carried-out headers
citing it as precedent are decorative.

---

## ADR 0045 — A suite is named for what it asserts, and the ratio was never the question


### 1. The decision, in one sentence

There is no size problem — the honest ratio is 1.15 to 1, not 2 to 1, and no
suite is removed — only a naming and placement one: test binaries sort into
Behavior, Correspondence and Surface, are named with a gerund for what Perch does
and a noun for a correspondence, three files are renamed to match, and the rule
for whether a test lives in `src`'s `mod tests` or in `tests/` is **what the test
names**.

### 2. Does it still hold?

**Holds**, with the numbers moved and one clause reversed by a later ticket.

- The three renames landed: `tests/adopting.rs`, `tests/reporting.rs` and
  `tests/publication.rs` all exist; `adoption.rs`, `status.rs` and
  `publishing.rs` do not.
- The where-a-test-lives rule is in the header it was promised to,
  `tests/common/mod.rs:3-8`: *"Where a test lives is decided by what it names
  (ADR 0045). A `mod tests` in `src` asserts a module's own vocabulary through
  the module's own API. A binary in `tests/` asserts what a *command* does. The
  fake is not the discriminator…"* No `tests/README.md` exists, as decided.
- The blanket allow and its justification are still there,
  `tests/common/mod.rs:10-12`.
- The taxonomy still sorts the tree, though the membership moved. 30 binaries
  now (was 33): 25 Behavior, 4 Correspondence — `conformance.rs`,
  `corroboration.rs`, `publication.rs`, `your_machine.rs` — and 1 Surface,
  `invoking.rs`. The four `contract_*` binaries 0045 placed were consolidated
  into two by ADR 0050, which says the placement survives the move: *"the two
  survivors are correspondences still, and the cases that move into
  `conformance.rs` move within the same kind."* `tests/conformance.rs:24-31`
  draws the gated/ungated line 0045 credited it with.
- `browsing.rs` did not survive — 0045 said *"nothing here objects"* if the
  picker went, and it went (ADR 0049).
- The numbers are all stale: `tests/` is 26,844 lines against 42,213 in `src`
  (0045 said 24,565 and 47,980); 33 files carry a `mod tests` rather than 38.
  The direction of the argument is unchanged.
- The `CONTEXT.md` clause is **reversed in part by its own amendment**, and the
  tree matches the amendment: *"Reversed in part by #204. The heading is back,
  holding **Behavior** alone."* `CONTEXT.md:459-470` is exactly that — a
  `## Proving it works` heading with one **Behavior** entry naming the other two
  kinds inside it, citing ADR 0045 at line 464.

### 3. Supersession

**Supersedes nothing, and says so:** *"This ADR supersedes nothing. ADR 0043
governs what an assertion must claim, ADR 0044 governs the level a claim is made
at, and this governs where a test lives and what its file is called. Three axes,
no overlap."* ADR 0050 later adds itself as a fourth axis using the same sentence
shape and likewise supersedes nothing.

Not superseded — **amended by itself**, in-file, via the `#204` note quoted above.
This is the only self-reversal in the slice, and it is partial and precisely
scoped: the taxonomy is untouched, only the glossary refusal is reversed, and the
note says why — *"a term defined in an ADR and nowhere else is a term the next
agent writes an issue without."*

### 4. Reasoning that outlives the document

- The rule, which is the durable artifact: **"a gerund for what Perch does, a
  noun for a correspondence"**, and *"The line actually being drawn is **what the
  test names**."*
- The corrected-arithmetic discipline: *"`47,980` is `src` including `src`'s own
  tests"* — a general warning that a ratio argument is only as good as what is on
  each side of the slash.
- *"a repository that breaks freely is one that leans on its suite to notice."*
  This is the one sentence tying the harness's size to `CLAUDE.md`'s
  break-anything policy, and it would be lost if the document were deleted.
- The coinage warning: *"The industry word is *contract test*, which this
  repository has already spent on a subset … Anyone reaching for the obvious word
  will name the part after the whole."*
- *"A reader who counts 24,565 lines and thirty-three binaries and finds no
  reasoning is entitled to assume nobody chose it."*
- The naming-for-behavior payoff, quoted approvingly by 0053: *"a rename of
  `perch status` does not invalidate `reporting.rs`, because the file was never
  named for the command."*

### 5. Others deciding the same thing

**ADR 0050** — the strongest partner. It renamed two of the binaries 0045 placed,
re-sorted their cases into `conformance.rs`, and its closing paragraph is
0045's closing paragraph with a fourth axis added. Read together they are one
decision about how `tests/` is arranged and when each part runs. **ADR 0043** and
**ADR 0044** — the other two axes, mutually disclaiming overlap; a merge would
have to argue against three ADRs that each say the axes are separate. **ADR
0056** cites 0045's shape-not-size finding as a precedent (`0056:13`) but decides
the `Host` port.

### 6. Citations

17 total — 15 in `docs/`, 2 elsewhere: `CONTEXT.md`, `tests/common/mod.rs`, plus
`docs/adr/` 0046, 0047, 0048, 0049, 0050, 0053, 0054, 0056.

**Load-bearing.** Both code-side citations carry the decision rather than tagging
it: `tests/common/mod.rs:3-8` is the six-line statement of the rule at the file
0045 nominated, including the trap it warns about; `CONTEXT.md:461-470` is the
**Behavior** entry that the #204 reversal created, which cites 0045 for the
taxonomy the entry summarizes. On the docs side, `0053:231` and `0050:215` are
load-bearing (they defer to 0045's naming rule to settle a live question); the
carried-out-header mentions are decorative.

---

## ADR 0046 — The Watcher's numbers are arithmetic, and only the Threshold is a preference


### 1. The decision, in one sentence

Of the Watcher's four pacing knobs, `watcher-no-return` is deleted entirely
(it could never change what Perch did), `watcher-cooldown-minutes` and
`watcher-margin-percent` become constants (15 minutes and 10 points relative to
the Threshold) because they are arithmetic rather than taste, and
`watcher-threshold-percent` survives as the only genuine preference — taking
`perch config` from six Settings to three, while all four pacing *concepts* keep
their glossary entries.

### 2. Does it still hold?

**Holds**, and the carry-out header is accurate.

- *`perch config` carries three Settings.* `registry::Settings` has exactly
  `strategy`, `watcher_may_act`, `watcher_threshold_percent`
  (`src/registry.rs:332-342`), beside `UngroupedConfig::interchangeable`.
- *The cooldown and margin are constants in `src/watch.rs`.*
  `COOLDOWN_MINUTES = 15` (`src/watch.rs:347`) and `MARGIN_PERCENT = 10`
  (`src/watch.rs:362`), both with the ADR's own arguments in their doc comments.
  `Policy` is now a one-field struct (`src/watch.rs:378-381`).
- *The Margin is relative to the Threshold and saturating.* `Policy::ceiling`
  (`src/watch.rs:402`) is `threshold.saturating_sub(MARGIN_PERCENT)`, with the
  ADR's "a threshold of 5 is a coherent thing to ask for" preserved.
- *`Recently::barred`, the `Overrides` field, the threading, the four advisory
  branches: gone.* No occurrence of `barred`, `watcher_no_return`,
  `watcher_cooldown_minutes` or `watcher_margin_percent` anywhere in `src/`.
- *A departed Setting is refused rather than ignored.*
  `tests/configuring.rs:390-419`
  (`the_settings_the_watcher_shed_are_no_longer_keys_a_scope_carries`) asserts
  `EXIT_INVALID` for all three keys on both `set` and `get`, and that none
  appears on the listing.
- *Where the cooldown is kept is untouched.* `watch::Recently` in memory,
  `registry::Checked` in the registry (`src/registry.rs:517-541`), with the ADR
  cited: *"A constant still has to be paced somewhere (ADR 0046)."*
- *`MAX_WATCHER_COOLDOWN_MINUTES` and `registry::a_cooldown` retired.* Neither
  name exists.
- *The forward-looking guard is honored.* `registry::Checked`'s doc
  (`src/registry.rs:531-535`) records that the "which Account was left" field was
  removed too, and that it is *"not kept against the day ADR 0046's guard fires
  and a no-return comes back: breaking the registry's format is free
  (`CLAUDE.md`)"* — the CLAUDE.md "no migrations" rule applied to 0046's own
  guard.
- *The glossary changes landed.* `CONTEXT.md`'s **Margin** entry now reads *"What
  refuses a destination nearly as full as the Account being left"*; **Cooldown**
  has no no-return sentence; **Back-off** reads *"a Cooldown paces Switches the
  Watcher makes, and a Back-off paces questions nobody is answering"* with the
  "the Group's to set" clause gone.
- *`Cooling` stops calling the wait "this Group's cooldown".*
  `Recently::resting` (`src/watch.rs:594-610`) says *"the last Switch was … ago
  and the cooldown leaves at …"* with no Group in it.

One clause of "What this does not decide" decayed, and ADR 0051 says so itself
(see field 3).

### 3. Supersession

**Supersedes ADR 0013's "Amended: the numbers this asked for" section in full and
nothing else** — see 0013 field 3 for the full quotes, the explicit refusal to
supersede 0013 whole, and the boundary contradiction between 0046's own
"everything above it stands" list and where those clauses actually sit in 0013.

**Answered rather than superseded by ADR 0051:**

> "**ADR 0046 is answered, not superseded.** This is the question it produced and
> stopped at. One clause of its 'What this does not decide' decays —
> `watcher-threshold-percent` is still per-Scope, but no longer 'Overridable per
> Scope exactly as it is' — and its decision, which knobs survive, is untouched."

0051 also cites 0046 as the **precedent for partial supersession**: *"Superseding
ADR 0017 whole was considered and refused, on ADR 0046's precedent: supersede the
section that decayed and leave a mostly-correct record standing rather than
restating it at length."* That makes 0046's refusal a load-bearing precedent in
the corpus, not just a local choice.

### 4. Reasoning that outlives the document

The yardstick, which is the whole reason 0046 exists and which any survivor must
carry:

> "one dies because it has no effect, two become constants because they are
> arithmetic rather than taste, and one survives as the only genuine preference
> in the loop."

The finding that corrects everyone's mental model of the Margin:

> "Usage on the Account you are on climbs. Usage on the Accounts you are not on
> does not. Two Accounts therefore do not trade places — **they walk upward
> together**."

The test for whether a Setting deserves to exist:

> "A Setting that prints a lie about itself is the clearest possible failure of
> the yardstick this decision is taken by."

The forward guard, which is preserved verbatim in `registry.rs` and must survive
the document:

> "**If the cooldown ever stops gating Switches outright — becoming per-Account,
> or pacing rather than barring — a no-return has to come back.** It is absent
> because the cooldown subsumes it, not because returning immediately is
> acceptable."

Why unsettable is not invisible:

> "a person still meets these words in a `Cooling` line and in a held round, and
> a term you meet but cannot look up is worse conceptual surface than one you
> can."

And the category error that protects grants from tuning-count arguments:

> "measuring a permission by how much tuning it offers is a category error."

### 5. Others deciding the same thing

- **ADR 0013's amendment**, which it superseded whole. Merge target by
  definition.
- **ADR 0051** (`a-setting-is-said-about-the-scope-it-governs…`) decides the
  *shape* of the Setting surface (Scope, Override, Inherit) where 0046 decides
  the *count*. 0051 says it is "the question [0046] produced and stopped at" —
  two consecutive halves of one configuration decision, and the strongest merge
  pair outside my slice.
- **ADR 0017** (`ungrouped-accounts-cycle-only-when-asked`) is the third leg —
  the two-yeses rule 0046 declines to judge and 0051 restructures.
- **ADR 0002** (`groups-scope-cycling-and-carry-configuration`) is where
  `watcher-may-act` originally lived; 0046 re-affirms it there.

### 6. Citations

39 total — 13 in `docs/`, 26 in code and elsewhere. `src/watch.rs`,
`src/cycle.rs`, `src/registry.rs`, `src/commands/{watch,config,group}.rs`,
`tests/{watching,scheduling,configuring}.rs`,
`pages/src/content/docs/{watching,configuration}.md`, and five ADRs.

**Load-bearing.** `src/watch.rs:338-346` and `:349-361` are the ADR's arguments
transcribed onto the constants they justify. `src/registry.rs:517-535` cites it
twice — once for "a constant still has to be paced somewhere", once for the
deleted field and the guard. `src/watch.rs:406-412` cites it for why
`cooldown()` is a free function rather than a `Policy` method (*"the dead
parameter ADR 0046 has just finished removing four of"*). `tests/watching.rs:14`
names it as the fourth property the suite exists to protect. No bare tags found.

---

## ADR 0047 — A command names the noun it is about, and the Account is the one left unsaid


### 1. The decision, in one sentence
A command is placed by the noun it is about; the Account is elided because it is
the subject of the product; every other noun is written at depth two and depth is
capped at two — from which follow `holdings export|import|purge`, `watcher
run|check|install|uninstall|status` (absorbing `watch`, `--once` and the
`service` tree), `upgrade` staying top-level, one name per capability with no
aliasing, a flag-or-verb test ("if it changes the meaning of the exit code or the
lifetime of the command, it is a verb"), and a four- (now five-) clause procedure
for admitting a command later.

### 2. Does it still hold?
**Holds**, with its arithmetic and one table row overtaken.
- The placement rule and the four written nouns: `tests/invoking.rs:144-153` is
  the assertion — "Fifteen names, because a command is placed by the noun it is
  about and the Account is the one left unsaid (ADR 0047). The ten that elide it,
  `perch` itself, and the four nouns that are written: `config`, `group`,
  `holdings` and `watcher`."
- `holdings`: `src/commands/holdings.rs:1-5`, cited in the first line.
- `watcher` with `check` as a verb: `src/commands/watcher.rs` dispatches
  `Run`/`Check`/`Install`/`Uninstall`/`Status`; `service` keeps the glossary
  entry and lost the tree, stated at `src/commands/service.rs:10-12`.
- `Holdings` is a glossary entry (`CONTEXT.md`, Holdings), as the carried-out
  banner claims.
- The rule is still cited to settle naming questions elsewhere:
  `src/registry.rs:1786-1788` names the watcher lock for the Watcher rather than
  an arrangement "(ADR 0047)"; `src/service.rs:1096-1103` asserts every unit file
  invokes the verb the binary still answers to, "which is how two of them come to
  name a verb the binary stopped answering to (ADR 0047)".
- Overtaken but not contradicted: the banner and Consequences say "sixteen"
  names / "twenty-eight" forms and the table lists `tui` and `config unset`.
  ADR 0049 removed `tui`, ADR 0051 removed `config unset`; the true figures are
  fifteen and twenty-six. ADR 0053 corrected the first half of this in 0052's
  body and got it wrong itself.
- Clause 1 has an unadjudicated case: `perch group list` is answered *before* the
  lock and outside the door (`src/commands/group.rs:70-73`), which is a locking
  matter for ADR 0057 rather than a placement one, but it is the only leaf that
  reaches neither shape the two documents describe.

### 3. Supersession
Supersedes nothing, and says so with a reason worth keeping: "**This supersedes
nothing.** It is the first record of a rule the surface has been following
imperfectly since ADR 0011, and there is no prior decision to correct — which is
itself the finding." It is **amended twice, each in exactly one clause, and both
amendments say so in the same words**:

- ADR 0052: "This amends ADR 0047 in that one clause and nothing else — the
  precedent is ADR 0050 amending ADR 0007 in a single sentence. Its decision, its
  table and its counts are untouched." (adds clause 4, a reversal takes its own
  name; ADR 0047's own text has already been edited to carry it and to say
  "Clause 4 was added by ADR 0052".)
- ADR 0054: "This amends ADR 0047 in that one clause and nothing else, in the
  style ADR 0050 set and ADR 0052 followed. Its decision, its table and its
  counts are untouched." (adds clause 5, a command is named for what it does in
  every case.) **Note: ADR 0047's file does not carry clause 5.** Its "Admitting
  a command later" list stops at four. Clause 4 was written back into 0047 and
  clause 5 was not — a documented amendment with no trace in the amended file.
- ADR 0053 is careful *not* to be an amendment: "**This supersedes nothing, and
  ADR 0047 is answered rather than amended.** Its 'What this does not decide'
  section posed both halves of this question — the count and the naming — and
  deferred them on purpose. A decision that supplies an answer a prior decision
  asked for is not correcting it."
- ADR 0047 also re-affirms ADR 0011 without touching it, and explicitly declines
  to amend ten older ADRs whose cited paths it moves: "**None is amended.** A
  citation going stale is not a decision decaying, and this repository supersedes
  records rather than editing them."

### 4. Reasoning that outlives the document
Several, and this is the richest of the eight:

> **A command is placed by the noun it is about. The Account is elided, because
> the Account is the subject of the product. Every other noun is written, at
> depth two.**

> A citation going stale is not a decision decaying, and this repository
> supersedes records rather than editing them.

> **If it changes the meaning of the exit code or the lifetime of the command, it
> is a verb. Otherwise it is a flag.**

> **The noun must already be in `CONTEXT.md`.** A command may not invent one.

> The count was never the defect — nineteen looked-up names cost one idea if a
> rule places them and nineteen facts if nothing does.

The second is the repository's own policy on stale citations and matters directly
to what this inventory recommends.

### 5. Others deciding the same thing
This is the hub of the cluster. **ADR 0052 and ADR 0054 are each, in their own
words, one clause of ADR 0047** — they say so in identical sentences. **ADR 0053**
answers two questions 0047 deferred by name. A merged "command surface" record is
the obvious shape: 0047 as the rule, with 0052's clause 4, 0054's clause 5 and
0053's count as sections rather than documents. **ADR 0011** is where the surface
started following the rule imperfectly, per 0047's own diagnosis. **ADR 0049** is
what invalidated 0047's arithmetic. **ADR 0057** is the only one of the cluster
that is *not* about the surface — it is about the registry lock — and should not
be swept in on the strength of touching the same files.

### 6. Citations
33 total — 28 in `docs/`, 5 in code and elsewhere. Files: `tests/invoking.rs`,
`src/registry.rs`, `src/service.rs`, `src/commands/holdings.rs`,
`src/commands/service.rs`, ADRs 0048, 0049, 0051, 0052, 0053, 0054.

**Load-bearing in code, though the mass of citations is ADR-to-ADR.** The five
code sites all explain a shape: `tests/invoking.rs:144-153` derives a fifteen-name
constant from it and asserts `--help` against it; `src/commands/holdings.rs:1-5`
is a module that exists because of it; `src/service.rs:1100` cites it for a test
guarding against unit files naming a retired verb — the most concrete consequence
of the `watch` → `watcher run` rename anywhere. `src/registry.rs:1788` is
supporting rather than structural. Twenty-eight of the thirty-three citations are
other ADRs referring to the rule, which is what a rule-of-record looks like.

---

## ADR 0048 — A Switch is written down before it moves anything, and the Landing is what it writes


### 1. The decision, in one sentence
Perch does not move the Credential until it has written down that it is about to:
`registry.active` becomes one field with three states (nobody / settled / a
Landing naming leaving and arriving), the Landing is written after the Capture and
before the Credential write by *both* doors (`switch::perform` and
`switch::make_live`), every Switch path resolves a found Landing first by
byte-equality against held copies in a fixed order and refuses when nothing
matches, `perch status` says a Switch was in flight and exits 0, and the Watcher
holds rather than stops.

### 2. Does it still hold?
**Holds**, in full, and it is the most thoroughly implemented ADR in this slice.
- One field, three states: `enum Active { Nobody, Settled(String), Landing {
  leaving: Option<String>, arriving: String } }`, `src/registry.rs:739-763`, with
  `#[serde(deny_unknown_fields)]`. `Active::whose()` returns the Account being
  left during a Landing (`:772-778`), exactly as the ADR specifies.
- Written between Capture and Credential move: `src/switch.rs:236-259` and
  `:495` ("a Landing written earlier would be one every refusal at step one…").
- Both doors: `make_live` "writes a Landing, as [`perform`] does and for the
  reason ADR 0048 gives" — `src/switch.rs:584-601`.
- Resolution as its own step, in the ADR's order, refusing at the end:
  `pub fn resolve_a_landing` at `src/switch.rs:750-810`, with
  `whose_the_live_credential_is` doing the ordered comparison. It goes further
  than the ADR asked, taking Claude Code's own locks around read-decide-record
  (`:761-773`) — a strengthening, not a departure.
- Every Switch path resolves first: `src/commands/remove.rs:82`,
  `src/commands/relogin.rs:100-104` (with the deliberate `Conflict` exemption the
  ADR's "`perch relogin` as the way through either way" implies, argued at
  `:88-99`), `src/commands/watch.rs:730-735`.
- `perch status` says it and exits 0: `src/commands/status.rs:52-56`,
  `src/registry.rs:796-846` (the line and the `--json` field).
- Watcher holds rather than stops: `WatchOutcome::NotArranged`,
  `src/commands/watch.rs:920-923` — "a Switch left in flight that nothing here can
  settle (ADR 0048)".
- Dangling-pointer check covers both ends of a Landing: `src/registry.rs:1519-1529`,
  `:2028-2037`.

### 3. Supersession
Supersedes nothing, amends nothing, and says so twice. "**This supersedes nothing
and amends nothing.** ADR 0006 is untouched: Capture before every Switch stands,
the three steps stand, and its Consequences paragraph … stays true word for word.
What changes is that Perch now refuses on the way into that state instead of
causing it." Nothing supersedes it. It changes one `CONTEXT.md` entry (**Landing**)
by widening it, and that widening is in the file today.

### 4. Reasoning that outlives the document
> **The live Credential carries no owner.**

> this is not a decision being revisited, it is one that was never made.

> The bar is *never silently*, not *never*: Perch cannot always tell what
> happened, but after this it always knows when it cannot.

> A sidecar could be written outside the perch lock, which is the only argument
> for one, and that argument is self-defeating: it puts two writers on one fact.

The last is a general rule about where a fact lives, and the second is the same
finding ADR 0047 reached about the command surface — the two documents state it
independently, which is itself evidence about how they relate.

### 5. Others deciding the same thing
- **ADR 0006** is the parent and is deliberately preserved intact; 0048 is best
  read as the second half of 0006 rather than as a rival. If anything merges here
  it is these two.
- **ADR 0022** (a live Profile corroborated by process start) and **ADR 0027**
  share the "what evidence settles who is live" question.
- **ADR 0055** cites 0048 for the ordering-as-a-type argument (`Settled` as a
  witness) and is the closest thing to a successor in spirit.
- **ADR 0023** and **ADR 0024** are the two commands that reach `make_live`, and
  0048 changes both without amending either.

### 6. Citations
47 total — 2 in `docs/`, 45 in code and tests, the highest code-citation count in
this slice by a wide margin. Files: `tests/{refreshing,listing,removing,
relogging_in,exporting,reporting,switching,watching,common/mod}.rs`,
`src/{export,carry,lock,registry,switch,import,observe}.rs`,
`src/commands/{watch,export,relogin,status,remove,list}.rs`, ADR 0055.

**Load-bearing, emphatically.** Sampled sites: `src/commands/relogin.rs:88-99`
cites 0048 to justify an exemption *from* 0048's own refusal, quoting the ADR
back at itself; `src/commands/watch.rs:623-628` cites it for an ordering
constraint that "was a comment above the call; it is an argument now";
`src/registry.rs:796-802` justifies printing a line at all by 0048's "nobody
looks" finding; `src/commands/status.rs:52-56` derives the exit code from it.
None of the sampled sites is a bare tag.

---

## ADR 0049 — Perch does not draw, and the ranking belongs to the listing


### 1. The decision, in one sentence

`perch tui` is deleted entire — 8,418 lines, both remaining tabs, both terminal
crates and the two `Host` methods it alone widened the port for — and the one
thing it uniquely rendered moves into `perch list` unconditionally: the Cycle
ordering, a Headroom column beside it, and Ungrouped Accounts shown *held* rather
than ranked.

### 2. Does it still hold?

**Holds, in every clause.**

- Removal: no `src/tui/`, no `src/commands/tui.rs`, no `tests/browsing.rs`, no
  `pages/src/content/docs/tui.md`, no `perch tui` anywhere in `pages/`, `README.md`
  or `CHANGELOG.md`.
- Port narrowing: `src/host/mod.rs` — the `Terminal` surface ends at `note`
  (`:676`); `print_remarks`, `remarks`, `keeping_its_remarks` and the `aloud`
  branch are all absent. "the deduplication `note` performs is not the picker's
  and stays" — `src/host/mod.rs:672-675` still says "Said once."
- Crates: `ratatui` and `crossterm` are out of `Cargo.toml`; `unicode-width` is
  declared with exactly the rewritten justification 0049 specified
  (`Cargo.toml:88-95`).
- The ranking moved: `Section::of` (`src/listing.rs:64-74`) picks
  `cycle::ranked(…)` when `cycle::may_cycle_within` says so and
  `scope.accounts(registry)` when it does not; `HEADERS` at
  `src/commands/list.rs:278` is `["Account", "Alias", "Group", "State",
  "Headroom"]`; `Section::order()` returns `"ranked" | "held"`
  (`src/listing.rs:78-84`) and is asserted at
  `src/listing.rs:a_section_is_ranked_only_where_something_declared_its_accounts_a_set`.
- "Always, and not behind a flag" — there is no flag; `ListArgs` carries only
  `scope`, `refresh`, `json` (`src/main.rs:181-201`).
- "Perch has no interactive surface left" — true; the only prompts left are
  passphrases and confirmations inside acting commands, and `README.md:150-152`
  states the property.
- The 18-names-to-15 arithmetic is confirmed downstream by ADR 0053's
  Consequences ("fifteen names, twenty-seven forms").

One residue: `tests/listing.rs:720` still says "`perch group list` and the TUI
both say which way the Setting is set" in a test's doc comment — a historical
sentence about a surface that is gone.

### 3. Supersession

Supersedes three documents, at three different depths — this ADR is the corpus's
main worked example of graded supersession:

- **ADR 0011 — in part.** 0011's banner: "**Superseded by ADR 0049.** The picker
  is gone". 0049 dismantles 0011's picker argument ("the argument reaching two
  conclusions that cannot both hold") while adopting 0011's own statement of the
  job: "the ranking `perch switch` makes should be visible rather than hidden,
  'so the two surfaces cannot come to disagree about which account is better.'"
- **ADR 0016 — explicitly in part**, and the split is named: "ADR 0016 is three
  decisions in one file and only two of them are the picker's. Its
  ratatui-over-crossterm choice and its second Amended section … are superseded
  here. **Its color-eyre repeal and the two-error-idiom rule stand** … superseding
  the file whole would have taken `report.rs`'s charter with it."
- **ADR 0042 — in full, with a carry-forward.** "ADR 0042 is superseded in full …
  but one sentence is carried forward rather than buried".

And it declines several: "**ADR 0025 is not amended**", "ADR 0047 is **not**
amended", "ADR 0034 stays superseded by 0042".

Superseded by nothing. ADR 0053 says so from its side: "**ADR 0049 gains a reader
rather than an amendment.**"

### 4. Reasoning that outlives the document

- *"a ranking of Accounts Perch would refuse to choose between is the hidden claim
  ADR 0011 built this listing to prevent, and it is just as false in a plain-text
  table as in a drawn one."* This is the repo's most-reused sentence: it is
  restated at `src/listing.rs:36-50`, `src/cycle.rs:628-631`, `src/cycle.rs:700-704`,
  `tests/listing.rs:238-242` and `docs/adr/0060`'s Considered Options.
- *"The argument was never that the ranking should be available; it was that the
  two surfaces must not disagree, and an optional agreement is not one."*
- *"if the machinery is what costs, the machinery is what the decision is about."*
- *"It lands with the removal, in one change. Separated, there is a window in
  which nothing in Perch shows the Cycle's judgment — which is the disagreement
  this whole thread exists to prevent, arrived at by scheduling."* A general rule
  about sequencing a removal and its replacement.
- *"An 8,418-line subsystem whose removal changes not one word of the vocabulary
  is the clearest evidence available that it carried no idea a person had to
  hold."* — a reusable test, and one 0053 then reuses in reverse for the glossary.
- *"the port is what this repository has been least willing to disturb, and the
  picker is the only subsystem that has made it wider"* — with the general form,
  that widening the one seam nothing else may widen is a cost even where each
  widening was correct.

### 5. Others deciding the same thing

- **ADR 0053** — the two together are "what a listing is and who shows it": 0049
  puts the ranking in `perch list`, 0053 puts *every* breadth there and takes
  `--group` off `status`. 0053 says 0049 "already wrote the sentence" and quotes
  "`perch list` is the whole of looking" to close its own case. Strongest merge
  pair in this slice.
- **ADR 0058** (a Reserve attaches where a heading names its Scope) — opens by
  reciting 0049 and finishes the same listing; it is the third installment of one
  decision about what `perch list` prints.
- **ADR 0060** — moves what 0049 built into `src/listing.rs`. Placement rather
  than policy, but its whole subject is 0049's Section.
- **ADR 0017** — the held-versus-ranked distinction is 0017's rule; 0049 calls it
  its weightiest piece and `src/listing.rs:37-45` cites 0017 for it. A survivor
  needs both or neither.
- **ADR 0011 and ADR 0016** — its two partial predecessors, one of which (0016)
  keeps a live, unrelated charter and must not be folded in whole.

### 6. Citations

35 total — 18 in `docs/`, 17 elsewhere: `src/listing.rs`, `src/cycle.rs`,
`src/registry.rs`, `src/commands/list.rs`, `src/commands/switch.rs`,
`tests/listing.rs`, `README.md`, `pages/src/content/docs/status.md`, and ADRs
0011, 0016, 0034, 0042, 0052, 0053, 0058, 0060.

**Load-bearing throughout.** `src/cycle.rs:628-631` explains why the Cycle and the
listing share one measurement ("two orders would be a listing that put one Account
at the top and a `perch switch` that landed on another"); `src/cycle.rs:700-704`
records a shipped bug against it; `src/registry.rs:2290` uses it to justify
declaring a Group an Account claims; `tests/listing.rs:129` names it as what the
test is for. Not one bare tag among the sites read.

---

## ADR 0050 — A gate asks for consent, and drift was never what it caught


Note: this ADR is in slot C by file adjacency (`src/probe.rs`, `src/lock.rs`),
not by subject. It is a test-gating decision and touches the Watcher not at all.

### 1. The decision, in one sentence

The `contract` build feature is kept but re-defined and renamed **`your-machine`**,
meaning *this test touches state the developer owns and did not offer* — a read
of their real keychain, `~/.claude` or open clients counts as much as a write —
which re-sorts twenty-one tests into nine gated, eleven ungated (mostly folded
into `conformance.rs`'s two-adapter table) and one moved out of `tests/`
entirely, collapsing four binaries into two; the two wall-clock tests ungate
because cost is not consent; and drift is caught by the weekly CI schedule, the
separate CI step and `probe.rs`, none of which was ever the flag.

### 2. Does it still hold?

**Holds**, in every checkable clause.

- *The feature is `your-machine`, with the criterion written on it.*
  `Cargo.toml:120-129`: *"A test behind this one touches state the developer owns
  and did not offer … Off unless asked for; CI turns it on for the one step that
  follows the install."*
- *Four binaries became two.* `tests/` contains `your_machine.rs` and
  `corroboration.rs`; no `contract*.rs` file exists.
- *Gated per test, not per file.* `tests/your_machine.rs:15-18` states the rule
  and the reason — *"A file-level `#![cfg(target_os = \"macos\")]` is what once
  narrowed a claim about every filesystem to one platform without anybody
  choosing it."*
- *`corroboration.rs` is ungated and says why.* Its header (`:17-20`): *"Every
  process these read is one of their own … Nothing here is the developer's to
  consent to (ADR 0050)."*
- *The cases folded into `conformance.rs`.* The table now holds **43** `Case`
  entries (`tests/conformance.rs:143ff`) against the twenty-seven 0050 counted
  and the thirty-two its carry-out header claims — it has grown since, which is
  the decision working rather than failing.
- *The two slow tests ungated.* `lock::exclusivity` is `#[cfg(test)]` only
  (`src/lock.rs:1358-1359`) with 0050's argument above it —
  *"this is the only execution of the exclusivity claim anywhere in the
  repository, and it had already gone unexecuted once"*. Same for
  `touch_moves_a_directorys_modification_time_forward`
  (`src/host/real.rs:2305-2312`).
- *The one test that moved out of `tests/` did.*
  `the_default_store_is_where_perch_believes_it_is` is now in `probe.rs`'s own
  `mod tests` (`src/probe.rs:1637`).
- *CI.* `.github/workflows/ci.yml:215` runs
  `cargo test --locked --features your-machine --test your_machine` as a
  separate step with no `--lib`; the weekly `schedule` survives at
  `ci.yml:13-17` carrying the exact sentence 0050 re-affirmed: *"A weekly run is
  what turns 'we would have noticed' into 'we did'."*
- *ADR 0007 got its one amendment and no supersession.* `docs/adr/0007*.md:3-9`
  carries a header naming 0050 and rewriting the closing sentence.

### 3. Supersession

**Supersedes nothing, by its own explicit statement:**

> "This ADR supersedes nothing. ADR 0043 governs what an assertion claims, ADR
> 0044 the level it is made at, ADR 0045 where a test lives and what its file is
> called, and this one **whether a test may run without being asked**. Four
> axes, no overlap."

It **amends ADR 0007 in one sentence**:

> "`ADR 0007` gains **one amendment and no supersession**. Its closing sentence —
> *'Contract tests assert the same shapes against the installed bundle in CI, to
> find drift before users do'* — names an artifact that will not exist by that
> name, so it is rewritten to name the scheduled run."

And it **inherits from ADR 0045 rather than revising it**: *"ADR 0045 placed the
four `contract_*` suites as Correspondence and explicitly left the gate to this
ticket. The placement survives."*

Nothing supersedes 0050. ADR 0054 and ADR 0051 cite it in passing.

### 4. Reasoning that outlives the document

The criterion itself, stated as a general rule about test suites:

> "**This test touches state the developer owns and did not offer.**"

> "The reason to hold a test back is not that it might damage something. It is
> that **its outcome is not the repository's to determine**. A suite that reports
> green on Tuesday and skipped on Wednesday, on the same machine at the same
> commit, according to whether a client happened to be open, is the one thing a
> default-on suite must never be."

The refusal of a cost-named flag:

> "`slow` names a price rather than a claim, so nothing ever tells you when a
> test has stopped qualifying."

The observation about which half of a two-adapter claim can actually be wrong:

> "The platform is not going to stop sharing bytes across a hard link; the
> hand-maintained fake might stop modeling it."

And the refusal to let a metric choose a design:

> "A coverage figure is not a claim about correctness, and letting it choose a
> gate would be the figure choosing the test shape."

Plus the a-priori honesty:

> "The repository is **twelve days old**, the scheduled run has fired **once**,
> and `INSTALL_SHA` has never been bumped. There is no ledger here in either
> direction."

### 5. Others deciding the same thing

0050 pre-empts this question and names the answer: **ADRs 0043, 0044, 0045 and
0050 are four axes of one testing model** — what an assertion claims, at what
level, where it lives, and whether it may run unasked. It argues they do not
overlap, which is an argument *for* co-locating them and *against* merging them
into one decision. Any consolidation ticket should treat that paragraph as the
map's own answer.

- **ADR 0007** (`assumptions-about-claude-code-are-probed-not-assumed`) is the
  other half of the drift story and now carries 0050's amendment; the two read
  as one record of "how Perch stays honest about upstream".
- **ADR 0041** (`a-real-machine-proves-perch-by-being-used-not-by-a-suite`)
  decides the adjacent question of what a real machine proves. Related; 0050
  does not cite it.

### 6. Citations

13 total — 4 in `docs/`, 9 in code and elsewhere: `Cargo.toml`,
`tests/{conformance,your_machine,corroboration}.rs`, `src/probe.rs`,
`src/lock.rs`, `src/host/real.rs`, `.github/workflows/ci.yml`, plus ADRs 0007,
0051 and 0054.

**Load-bearing, and unusually so for such a small count.** `Cargo.toml:121-129`
is the criterion itself living on the feature it defines. `src/lock.rs:1351-1357`
is the ADR being used to *reverse* a prior gate, with the argument spelled out.
`tests/conformance.rs:27-30` uses it to say why one suite is ungated and its
neighbor is not. `tests/corroboration.rs:17-20` likewise. There is no site where
0050 is a bare tag; the ADR's whole content is a criterion, and the citations are
that criterion being applied.

---

## ADR 0051 — A Setting is said about the Scope it governs, and the case for Overrides never defended the fallback


### 1. The decision, in one sentence
There is no layer: **each Scope — every Group, and the Ungrouped Accounts —
holds its own full Settings, defaults are compiled-in constants**, and `Global`,
`Override`, `Inherit`, `perch config unset` and the word-count idiom all go with
it; the per-Scope *value* was never in question, only the fallback, and no
argument on the record ever defended the fallback.

### 2. Does it still hold?
**Holds**, and is fully carried out — the banner's claim ("Carried out in #173")
checks out line by line.

- `Global`, `Override`, `Inherit`: `grep -rn "Global\|Override\|Inherit" src/`
  returns **6 hits, none of them the concept** — four are the word "inherit" in
  unrelated prose (`src/service.rs:938,957`, `src/registry.rs:1294,3237`,
  `src/watch.rs:580,927`), one is `perch_claude_bin_overrides_the_search`
  (`src/probe.rs:1494`), and one is a *stale comment inside a test*
  (`src/registry.rs:3509-3513`) still explaining the `global` reservation by the
  old reason — "Global is addressed by naming none". That comment is the one
  place in `src/` where the dead layer is still described as live; the live code
  it guards has the rewritten reason at `src/registry.rs:685-693`.
- `registry::Scope` and `cycle::Scope` are one type: `src/registry.rs:389-407`,
  and its doc closes with 0051's own prediction — *"There is no such value to be
  handed any more, so that is a mistake nobody can make."*
- `perch config unset` is gone: `ConfigCommand` is `Set` and `Get` only
  (`src/commands/config.rs:56-79`). `perch alias <name> --unset` survives, as the
  ADR said it would (`src/main.rs:63-69`).
- Every `set` is three words: `src/commands/config.rs:64-67`,
  `"<scope> <key> <value>"`.
- Bare `perch config get` still prints every Scope in full:
  `src/commands/config.rs:70-78`.
- `cycle-ungrouped` → `interchangeable`, carried by the Ungrouped Scope alone:
  `src/config.rs:32`, `:51`, `:61-67` (`carried_by`), refusal text at `:73-84`.
- `global` stays reserved with the reason rewritten:
  `src/registry.rs:504-513` (*"There is no such Scope […] Kept reserved so the
  refusal is where they find out Perch has no everywhere-layer"*), enforced at
  `:685-693`, and the `perch config` side at `src/commands/config.rs:227-232`.
- Registry `version` bumped: `src/registry.rs:29`, `CURRENT_VERSION: u32 = 2`,
  with the newer-Perch guard at `:1849-1857`.
- The struct correction landed: `Registry.ungrouped` carries `interchangeable`
  alongside Settings (`src/cycle.rs:59`, `src/config.rs:108`), and `groups` is a
  map of `Settings` (`src/registry.rs:331`).
- **The fourth row of 0051's table is unwritable**, as promised:
  `src/commands/watch.rs:630-684` gates on `may_cycle_within` and then reads
  `registry.settings(&scope).watcher_may_act` — a Scope's own field, with nothing
  to fall through from.
- The glossary matches: `CONTEXT.md`'s Configuration section holds exactly
  Setting, Config, Scope, Ungrouped — four, down from seven. **Ungrouped** carries
  the declaration; **Rename** says "its Settings"; **Scope** keeps "The only
  levels at which a Setting means anything".

Residual decay, all cosmetic: the ADR writes `perch group declare X`; the verb is
`perch group add` (`src/commands/group.rs:31-38`). `tests/configuring.rs:10` still
says "the global ungrouped-Cycling setting" in a module doc.

### 3. Supersession
Superseded by nothing. It supersedes with unusual precision — **two amendments in
full, one body sentence, and an explicit refusal to take either parent whole:**

> **ADR 0002's amendment, "Global carries the defaults and a Group Overrides
> them" — superseded in full.** […] **ADR 0002's body is untouched**, and better
> than untouched […]

> **ADR 0017's amendment, "the Ungrouped Accounts are a Scope that reads Global" —
> superseded in full.** Its core claim survives and is strengthened […]

> **ADR 0017's body loses one sentence.** "The setting is global rather than
> per-Group" is now false. The two sentences of reasoning under it survive intact
> […]

> Superseding ADR 0017 whole was considered and refused, on ADR 0046's precedent:
> supersede the section that decayed and leave a mostly-correct record standing
> rather than restating it at length. […] superseding it entire to correct one
> sentence whose *reasoning* survives would bury three Considered Options that
> are the only written record of why ungrouped Accounts do not Cycle freely.

And two nodes it deliberately did **not** touch:
> **ADR 0046 is answered, not superseded.** This is the question it produced and
> stopped at. One clause of its "What this does not decide" decays […] and its
> decision, which knobs survive, is untouched.

> **ADR 0047 and ADR 0040 are unamended.**

### 4. Reasoning that outlives the document
The one that most clearly outlives it, because it is a claim about consent rather
than about configuration:

> "`strategy` and `watcher-threshold-percent` are taste. `watcher-may-act` is
> consent, and layering consent is what produced the hole above."

and, in the same register:

> "a brake that works by blanket inheritance is the wrong brake for consent."

The one that outlives it as a claim about *process*, and which no other document
in the repository states:

> "**The rule is kept, and the finding is that it could not survive being
> hand-maintained.** An exception written into a uniform layer is an exception
> somebody has to keep re-deciding, and the record shows what happens: it was
> described in four places, tested in two, and enforced in none."

And the one that generalizes past this feature entirely:

> "**Reading is not writing.** Bare `perch config get` survives and prints every
> Scope's Config in full. A read has no subject to be wrong about; a write does."

Plus the reason `unset` could not be kept, which is a general trap:

> "**a value that knows whether it was set is Inherit under another name**"

### 5. Others deciding the same thing — including the 0002/0017/0051 judgment
**The brief's question: are 0002, 0017 and 0051 three decisions about scoping, or
one decision found three times? My reading is two, not three and not one.**

0002 and 0017 answer one question — *which Accounts may a Cycle move between?* —
for the two cases there are, and 0017 opens by saying so ("This is ADR 0002's
rule followed to its conclusion"). They share a premise sentence, share a code
path (`scope_for` → `may_cycle_within`), and neither is comprehensible without
the other. That is one decision found twice.

0051 answers a different question — *where does a Setting live, and does it fall
back?* — about an object 0002 and 0017 had already defined. The evidence that it
is genuinely separate is internal to 0051: it opens by *disqualifying* Group from
the list of nouns on trial, on the grounds that "a Group is the Cycling boundary
(ADR 0002), and it exists whether or not it carries a value", and it keeps
Ungrouped for the same reason. It puts three nouns on trial — Global, Override,
Inherit — and all three are the layer, none of them the scoping. Its supersession
of 0002 and 0017 reaches only their *amendments*, which is exactly the part of
each that was about the layer.

The counter-argument, which I do not think wins: 0051 does merge `registry::Scope`
with `cycle::Scope` and does relocate `interchangeable` onto the Ungrouped Scope,
so it touches the scoping object. But it touches it by *removing an obstruction*
— the reason the two types were separate was Global — rather than by re-deciding
which Accounts are interchangeable, which it explicitly leaves alone.

So the shape I would hand the merge ticket is: **0002 + 0017 → one document about
the Scope as the Cycling boundary; 0051 stands beside it as the document about
where a Setting is said.** They share a vocabulary, not a decision.

Other partners:
- **ADR 0046** — 0051 calls itself the answer to the question 0046 stopped at,
  and 0046's carry-out (#161) is 0051's stated prerequisite. If any two documents
  in the configuration area are one decision split across two planning rounds,
  it is these.
- **ADR 0047** ("a command names the noun it is about and the account is the one
  left unsaid") — 0051's "An implicit Scope was considered and refused" is
  0047's rule applied and found not to transfer, with the reason spelled out
  ("0047's elision works because the unsaid noun is *always the same one*"). Two
  applications of one principle about elision.
- **ADR 0050** — the same a-priori-with-limits epistemics ("this is decided a
  priori as #151 and ADR 0050 were"), and the same move of counting before
  deciding.

### 6. Citations
40 total — 8 in `docs/`, 32 in code and elsewhere:
`tests/{configuring,grouping,scheduling,watching}.rs`, four other ADRs,
`src/{cycle,main,config,registry}.rs`,
`src/commands/{group,watch,config}.rs`, and two guide pages.

**Load-bearing, and disproportionately so for a document this recent.**
`src/registry.rs:389-398` cites it in the type's own doc to explain why two types
became one and why the old warning is now about an impossible mistake.
`src/registry.rs:504-513` cites it for a reserved word whose reason was rewritten
rather than deleted. `src/commands/watch.rs:671-673` cites it for the single line
that closes the hole 0051 found. `src/commands/group.rs:32-34` cites it on the
`Add` subcommand's help text, so the ADR is visible in `--help`. The test-side
sites (`tests/configuring.rs:439`, `:495`, `:531`) each restate the rule being
asserted. No bare tags found.

---

## ADR 0052 — A reversal is its own command, and the state it restores has no name


### 1. The decision, in one sentence
`perch disable` and `perch enable` both stay, because a reversal is an act a
person performs even when the state it restores has no name — from which follow
ADR 0047's new clause 4, the rule that "a flag may mark an argument's absence, it
may not carry a verb's polarity", a boundary on ADR 0047's flag-or-verb test, and
three spellings changed outside the surface (`Account.enabled` → present-only
`disabled`, the Listing's state cell emptying, `--json` carrying `"disabled"`).

### 2. Does it still hold?
**Holds**, every clause.
- Both verbs, one implementation: `EnableCommand { Enable, Disable }` and one
  `run`, `src/commands/enable.rs:32-53`; two dispatch arms in `main.rs`.
- Registry field: `pub disabled: bool` with `#[serde(default,
  skip_serializing_if = "std::ops::Not::not")]`, `src/registry.rs:238-246`, and
  its doc comment carries the finding — "The positive state has no name to write
  down — it is the absence of this one (ADR 0052)." `enabled_by_default` is gone;
  `an_account_nobody_has_disabled_records_no_disable_at_all`
  (`src/registry.rs:3298-3326`) pins the serialization.
- Listing cell: `state_of` at `src/commands/list.rs:365-372` emits only
  `disabled`, `quarantined`, `disabled, quarantined`, cited to 0052 at `:367` and
  `:640`; asserted at `tests/listing.rs:371-373` and `tests/invoking.rs:570-576`.
- JSON key present unconditionally: `tests/listing.rs:578-583`.
- Idempotence and never exit 15:
  `saying_it_twice_is_not_an_error_and_says_it_was_already_so`,
  `tests/enabling.rs:160`.
- `_Avoid_` line: `CONTEXT.md`'s **Disabled** entry reads "_Avoid_: excluded,
  paused, off, archived, reserved" — the one word this decision added.
- The `Reserve` collision is cleaned: `src/commands/enable.rs:1-2` now reads
  "keeping an Account out of Cycling" rather than "reserving an Account".
- Stale only in arithmetic: "invocable forms at twenty-seven" is one too many
  (see the header of this file), and ADR 0053 corrected 0052's *other* number
  while leaving this one.

### 3. Supersession
Supersedes nothing. Amends **ADR 0047 in exactly one clause**: "**This supersedes
nothing and amends ADR 0047 in one clause.**" and, earlier, "This amends ADR 0047
in that one clause and nothing else — the precedent is ADR 0050 amending ADR 0007
in a single sentence. Its decision, its table and its counts are untouched." The
amendment landed: ADR 0047's file carries clause 4 and the sentence "Clause 4 was
added by ADR 0052, which amends this decision in that one clause and nothing
else." It also puts a **boundary** on 0047's flag-or-verb test without amending
it: "So the test stands unamended and gains a boundary: it adjudicates a
capability *within* a command's scope, and never how many commands one capability
is reached by." It expressly leaves ADR 0012, ADR 0024 and ADR 0002 untouched.
ADR 0053 later "corrects one sentence of ADR 0052's body" — the count — and says
"ADR 0052's decision is untouched".

### 4. Reasoning that outlives the document
The sentence the brief asked for, verbatim from ADR 0052:

> `deny_unknown_fields` means a registry carrying `enabled` is refused rather
> than migrated. `CLAUDE.md` settles that: there are no users, and reading what
> an older Perch wrote is not the kind of guard worth keeping.

Others:

> A glossary names states; a command names acts.

> **A flag may mark an argument's absence. It may not carry a verb's polarity.**

> Two verbs cost **one** idea, not two: a person holds *Disabled*, and knowing
> `perch disable` tells them `perch enable` without a lookup.

> a wrong number left standing is read as a right one

(the last is ADR 0053's phrasing, quoted in 0052's Consequences — and it is
ironically the thing 0052's own surviving "twenty-seven" now demonstrates.)

### 5. Others deciding the same thing
- **ADR 0047** — 0052 is, by its own account, one clause of 0047 plus its
  reasoning. Strongest merge candidate in the repository.
- **ADR 0054** — the identical relationship to 0047, stated in the identical
  sentence, and 0054 leans on 0052 twice (the `--unset` argument, "a flag needs a
  verb to hang on"). 0052 and 0054 together are two clauses of one procedure.
- **ADR 0053** — same sweep, same yardstick, corrects 0052's arithmetic.
- **ADR 0043** supplies the machine-vs-person distinction 0052 uses for the JSON
  key and is cited for it.
- **ADR 0058** owns **Reserve** proper, the word 0052 disambiguated in passing.

### 6. Citations
16 total — 8 in `docs/`, 8 in code and tests. Files: `tests/listing.rs`,
`tests/invoking.rs`, `src/registry.rs`, `src/commands/list.rs`, ADRs 0047, 0053,
0054.

**Load-bearing.** `src/registry.rs:238-246` cites it in the doc comment that
explains why the field is present-only; `src/commands/list.rs:365-369` and
`:638-642` explain why a cell empties; `tests/listing.rs:578-583` and
`tests/invoking.rs:572-576` are assertions whose failure messages *are* the ADR's
argument ("a script testing for a key's presence to learn a bool has a worse
contract, not a truer one (ADR 0052)"). No bare tags found.

---

## ADR 0053 — `status` is about one Account, and every set was already the listing


### 1. The decision, in one sentence

`perch status` answers about the active Account only and `perch list` becomes the
listing at every breadth: `--group` is deleted from `status` rather than moved,
`perch list [<scope>]` takes a Group name or `ungrouped` as a positional
argument, `--refresh` follows the breadth on both, `status --json` loses its
duplicated top-level `utilization`, and a `--json` document must say what its
order is or not have one.

### 2. Does it still hold?

**Holds.**

- `StatusArgs` is `{ json, refresh }` and nothing else (`src/commands/status.rs:30-36`);
  the `Status` variant in `src/main.rs:223` takes only `--refresh` and `--json`,
  documented as "The Account you are on and nothing else (ADR 0053)".
- `perch list [<scope>]` is positional: `src/main.rs:181-186`, `#[arg(value_name =
  "SCOPE")] scope: Option<String>`.
- One shape per command: `src/commands/status.rs:1-14` — "One Account in detail,
  and that is the whole of it (ADR 0053)"; `src/commands/list.rs:18` — "This is
  the listing at every breadth (ADR 0053)."
- The duplicated key is gone: `render_json` (`src/commands/status.rs:180-195`)
  emits `{active, landing, refresh}` with Utilization only under `active`, and its
  doc comment reproduces the ADR's argument.
- "a refresh reads the Accounts it is about to show and no others" is implemented
  at `ListArgs`' email set (`src/commands/list.rs:104-110`) and cited for the
  refusal wording at `src/cycle.rs:242-247`.
- "a document says what its order is, or it does not have one": `sections` with
  `"order": "ranked" | "held"` (`src/listing.rs:78-84`, `:117-132`), and
  `src/commands/list.rs:564` carries the rule as a doc heading.
- The names stayed: `perch status` and `perch watcher status` both exist
  (`src/commands/status.rs`, `src/commands/watcher.rs`).
- Docs match: `pages/src/content/docs/reference.md:11-12` are exactly the two rows
  the ADR specified.
- `tests/reporting.rs` and `tests/listing.rs` both exist, per the ADR's closing
  paragraph.

The load-bearing premise it named — "**if the block goes, this decision is
wrong**" — still holds: `perch status`'s labeled block is live, with the
Quarantine sentence above the figures at `src/commands/status.rs:150-160`.

One thing has moved *past* the ADR without contradicting it: 0053 argued the two
JSON shapes differ ("Its JSON is one object where the listing's is an array"), and
ADR 0060 has since made the *Account* object identical in both
(`listing::document`, used at `src/commands/status.rs:191`). The distinction 0053
rested on — one Account in detail versus a set as a table — survives intact; it is
now carried by `active` vs `sections` rather than by divergent key sets, and
`src/commands/status.rs:166-175` says so.

### 3. Supersession

**Supersedes nothing, by its own explicit statement:** "**This supersedes
nothing, and ADR 0047 is answered rather than amended.** Its 'What this does not
decide' section posed both halves of this question — the count and the naming —
and deferred them on purpose. A decision that supplies an answer a prior decision
asked for is not correcting it."

It corrects one sentence of another document without touching its decision:
"**This corrects one sentence of ADR 0052's body.** It closes with 'sixteen
names, twenty-eight forms' … The correct count then and now is fifteen and
twenty-seven. ADR 0052's decision is untouched; the arithmetic was never its
finding, and a wrong number left standing is read as a right one."

And it declines two more: "**ADR 0015 and ADR 0018 are untouched**" (their
citations go stale and the decisions do not decay), "**ADR 0049 gains a reader
rather than an amendment.**" Superseded by nothing.

### 4. Reasoning that outlives the document

- *"Two names each with one shape is fewer ideas than one name with two
  renderings."* The general form of its whole argument, and reusable anywhere.
- *"a refresh reads the Accounts it is about to show and no others"* — stated as
  the rule that replaces the false one ("keep the network away from listings"),
  and live in code at two call sites.
- *"a flag may mark an argument's absence, and may not carry what an argument
  carries"* (attributed to ADR 0052). *"A Scope name is an argument."*
- *"a document says what its order is, or it does not have one"* — and its
  corollary, *"when the shape is the claim, the shape has to make it."*
- *"The glossary is the vocabulary of somebody using Perch, not of somebody
  maintaining it."* A new discriminator, stated as such ("This is the sixth ADR in
  this sweep to decline a `CONTEXT.md` entry and the first to decline one for this
  reason"). Note: **superseded in effect** — ADR 0060 added `Listing` and
  `Section` to `CONTEXT.md` anyway, on the grounds that `Section` carries the
  ranked-versus-held distinction and `--json`'s contract. 0053's discriminator
  survives as an argument; its conclusion about these two words does not.
- *"Naming the load-bearing premise is half of recording the decision: if the
  block goes, this decision is wrong."* A rule about how to write an ADR.
- *"a wrong number left standing is read as a right one."*

### 5. Others deciding the same thing

- **ADR 0049** — see above; 0053 explicitly extends 0049's "`perch list` is the
  whole of looking" from drawing to listings. One decision in two installments.
- **ADR 0058** — the third installment (what the listing says beneath the table).
- **ADR 0060** — the fourth, and it moves the shared Account document the two
  commands write. `src/listing.rs`'s header cites 0053 and 0060 in consecutive
  paragraphs.
- **ADR 0047** — 0053 answers the question 0047 deferred; a merged record would
  read as one decision about the command surface's count and names.
- **ADR 0052** — supplies the flag-versus-argument rule 0053 applies, and is the
  document whose arithmetic 0053 corrects.

### 6. Citations

27 total — 9 in `docs/`, 18 elsewhere: `src/listing.rs`, `src/cycle.rs`,
`src/registry.rs`, `src/main.rs`, `src/commands/status.rs`, `src/commands/list.rs`,
`tests/{listing,reporting,invoking}.rs`, `CHANGELOG.md`,
`pages/src/content/docs/status.md`, and ADRs 0052, 0054, 0058.

**Load-bearing, with one exception.** `src/commands/list.rs:107` explains why the
refresh set lives on `ListArgs`; `src/registry.rs:807` explains why an active-
Account qualifier is in `registry` rather than in either command; `src/cycle.rs:245`
justifies which Scope a refusal names. The `CHANGELOG.md` entry is decorative by
nature, and `src/main.rs:220` is close to a tag, though it does state the rule it
is tagging.

---

## ADR 0054 — A command is named for what it does in every case, and the thing acted on comes first


### 1. The decision, in one sentence
`perch alias` and `perch relogin` both stay — `add --alias` was never a rival
because it cannot change an existing Account's name, and `relogin` correctly
declines the glossary's word **Repair** because the command is wider than the act
— and the one thing that changes is `perch alias`'s argument order, flipped to
`<target> <name>` to obey the previously unwritten rule that the thing a command
acts on is its first argument.

### 2. Does it still hold?
**Holds.**
- Argument order flipped: `main.rs:58-69` declares `target` then `name` with
  `required_unless_present = "unset"`, and `AliasCommand::{Set { target, name },
  Unset { target }}` at `src/commands/alias.rs:26-31`. The module doc cites the
  rule: "Both forms are told the Account first and the name second, because the
  thing a command acts on is its first argument (ADR 0054)" (`:11-12`).
- The superset claim is asserted:
  `unsetting_by_email_frees_the_alias_the_account_answers_to`,
  `tests/naming.rs:445-449`.
- `--unset` stays a flag: `main.rs:66-68`.
- `relogin` keeps its name and the healthy-Account behavior it is named for:
  `src/commands/relogin.rs:17-19`, and the command is still handed to users by
  name in prose from several places (`how_to_repair`, `src/registry.rs:201`;
  `Quarantine::refusal`; `src/commands/relogin.rs:328,365`;
  `pages/src/content/docs/reference.md:46`).
- `add --alias` and `add --group` both survive: `main.rs:41-52`.
- Stale only in the count ("fifteen names, twenty-seven forms").
- Its clause 5 is **not** written into ADR 0047, unlike ADR 0052's clause 4 — see
  0047 §3 above. That is a documentation defect rather than a code one.

### 3. Supersession
Supersedes nothing. Amends **ADR 0047 in exactly one clause**: "**This supersedes
nothing and amends ADR 0047 in one clause.**" and "This amends ADR 0047 in that
one clause and nothing else, in the style ADR 0050 set and ADR 0052 followed. Its
decision, its table and its counts are untouched." It explicitly leaves **ADR
0023** governing ("ADR 0023 is untouched and still governing") and leaves
`CONTEXT.md` alone ("**`CONTEXT.md` gains nothing — the seventh consecutive
decline**").

### 4. Reasoning that outlives the document
> **The thing a command acts on is its first argument. What it is being given
> comes after.**

> **A command takes the glossary's word for its act only where the command and
> the act are the same size. Where the command is wider, it is named for what it
> does in every case rather than for the case that matters most.**

> **The shortcut earns its place because the long way costs the very thing the
> feature exists to remove.**

> A next step printed in an error message has to be one typeable word, not a mode
> of another command.

> It has governed every other command since the beginning and was never recorded,
> which is exactly why the one command that broke it did so unnoticed.

### 5. Others deciding the same thing
- **ADR 0047** — same relationship, same sentence, as ADR 0052. 0052 and 0054
  are the two amendment-clauses of 0047 and read as one document split in two.
- **ADR 0052** — 0054 depends on it for `--unset` and for "a flag needs a verb to
  hang on"; the two share a yardstick and a refusal-to-churn conclusion.
- **ADR 0053** — same sweep, same "re-affirmed rather than churned" outcome, and
  0054 cites it as precedent for a noun-shaped name and against the `add`
  collapse.
- **ADR 0023** supplies the whole of the `relogin` naming argument from its
  Consequences; a merged "perch relogin" record would hold both.
- **ADR 0058** and **ADR 0060** are the nearest non-slice neighbors on
  vocabulary-vs-surface questions.

### 6. Citations
3 total — 0 in `docs/`, 3 in code and tests. Files: `tests/common/mod.rs`,
`tests/naming.rs`, `src/commands/alias.rs`. **The lowest count in this slice, and
the only assigned ADR with no ADR-to-ADR citation at all.**

**Load-bearing, all three.** `src/commands/alias.rs:11-14` states the rule as the
reason for the signature; `tests/naming.rs:445-447` cites it for the specific
claim that the flip is a superset; `tests/common/mod.rs:602-605` cites it to
explain why a *helper* deliberately keeps the old order ("the command line is what
ADR 0054's rule is about") — a comment that only makes sense if the reader can
reach the rule. Three sites, no tags.

---

## ADR 0055 — The Watcher's round stays where it is, and the ordering is a type it cannot skip


### 1. The decision, in one sentence

Six orderings in the Watcher's round that had been comments (and each of which
had cost a shipped bug) become **witnesses** — `Settled`, `Idle`, `Cooled`,
values only the ask that earns them can construct — taken as arguments by the
steps that must follow, so ordering becomes arity and a violation does not
compile; the split between `src/watch.rs` (what a round *means*) and
`src/commands/watch.rs` (what a round *does*) is explicitly kept, and a step
protocol, a typestate chain and a `WatchPorts` port were each designed and
rejected.

### 2. Does it still hold?

**Holds.**

- *The witnesses exist with private constructors.* `switch::Settled(())`
  (`src/switch.rs:707`), `switch::Idle(())` (`src/switch.rs:1364`),
  `watch::Cooled<'a>(&'a Crossed)` (`src/watch.rs:518`) — the last borrowing the
  `Crossed` it was earned from, exactly as the ADR describes.
- *Ordering is arity, at both signatures the ADR wrote out.*
  `fn permitted(registry: &Registry, _settled: &Settled) -> Result<Watching>`
  (`src/commands/watch.rs:630`) and
  `fn considered(registry, watching, _cooled: &Cooled<'_>, _idle: &Idle) -> Vec<Considered>`
  (`src/commands/watch.rs:1209`).
- *(d) is an exhaustive match with no catch-all.* `watch::refused_or_raised`
  (`src/watch.rs:872-887`) matches `NotIdle::Live`, `NotIdle::SessionsUnreadable`
  and `NotIdle::Unnameable` by name, with the doc saying *"a fourth way for the
  ask to fail breaks the build here."*
- *(e) and (f) are behind `switch_to`.* `record_the_switch`
  (`src/switch.rs:205`) records the Check before the Switch;
  `NotSwitched { moved }` is read identically on both ways out
  (`src/commands/watch.rs:1159`).
- *The round did not move, and `watch.rs`'s module doc still says why.*
  `src/watch.rs:23-25`: *"Nothing here reaches the network or the filesystem.
  What a round does is `crate::commands::watch`'s; what a round means is here."*
  The ADR promises *"`watch.rs:16-18` stands unamended"* — the sentence is
  unamended but has slid to `:23-25` because ADR 0061 inserted a paragraph
  above it.
- *The second minter and the output change it caused.*
  `switch::nothing_in_flight` (`src/switch.rs:724`) is the only other producer of
  a `Settled`, and the opening line now says nothing about the Account when a
  Landing is in flight (`src/commands/watch.rs:481-508`), falling back to
  *"Started. Nothing is being decided yet…"*.
- *No port was built.* No `WatchPorts` anywhere; `&dyn Host` remains Perch's only
  port.
- *The named gap is still a gap.* *"'The three arrangements differ in exactly
  three places' remains a rule no test checks."* Nothing in `tests/` asserts it;
  `Watcher::{Loop,Check}` (`src/commands/watch.rs:523-530`) concentrates the
  difference but does not prove the count.

### 3. Supersession

Supersedes nothing; superseded by nothing. It builds on **ADR 0048** (*"since the
Switch got one door (ADR 0048, #200)"*) and defers to **ADR 0025** (*"`&dyn Host`
remains the only port Perch has, as ADR 0025 decided"*) and **ADR 0044**
(*"`host::fake` remains the only way behavior is driven, as ADR 0044 decided"*)
without amending either. ADR 0056 and ADR 0057 cite it forward.

### 4. Reasoning that outlives the document

The definition, which is the vocabulary a survivor must carry — it is the
project's only definition of the word:

> "A **witness** is a value that exists only as proof that an ask was made:
> nothing to substitute, and nothing in it that the ask did not establish."

Why a witness may hold one thing and no more:

> "`Cooled` borrows the `Crossed` it was earned from, because it is proof about
> *that* crossing rather than about crossings in general — which is the one
> thing a witness may carry, and the reason `Crossed` itself is not one: it
> holds the figure, and a value that holds the figure is a reading rather than a
> proof."

The problem statement, general to any codebase:

> "A rule that can only be checked by observing traffic is a rule a refactor can
> quietly break."

The disqualifier that killed the step protocol:

> "A design whose purpose is eliminating runtime ordering failures should not
> introduce one."

The disqualifier that killed the port:

> "One adapter is a hypothetical seam."

And the glossary rule, which is a general principle about where domain words come
from:

> "the glossary would be learning a word only the source uses, which is the drift
> the vocabulary exists to prevent."

### 5. Others deciding the same thing

- **ADR 0048** (`a-switch-is-written-down-before-it-moves-anything…`) owns two of
  0055's six orderings outright — 0055 says so — and both are decisions about
  making a Switch's sequence unskippable. Closest merge partner.
- **ADR 0025** (`a-crate-is-taken-where-it-does-not-cost-a-seam`) and **ADR 0044**
  (`the-binary-is-driven-to-prove-its-surface…`) are the two records 0055's
  "rejected: a port" section defers to. 0055's rejection is the best surviving
  evidence for 0025's rule; a merged record would want it.
- **ADR 0056** (`the-host-port-is-wide-because-the-machine-is…`) is the same
  seam question asked of a different module. Sibling, not duplicate.
- **ADR 0059** (`the-fakes-world-is-held-by-concern…`) similarly.

### 6. Citations

17 total — 3 in `docs/`, 14 in code and elsewhere: `src/watch.rs`,
`src/switch.rs`, `src/commands/{watch,run}.rs`, `tests/{running,watching}.rs`,
plus ADRs 0056 and 0057.

**Load-bearing.** `src/switch.rs:1361` (*"A witness on the terms `Settled` sets
out (ADR 0055)"*) and `src/watch.rs:512-517` are the type definitions pointing
at the ADR as their sole justification — 0055 says explicitly that the word
*witness* is *"defined once in the source — at `switch::Settled` — and every other
use points at that and at this ADR"*, so these citations are the mechanism the
ADR designed, not incidental tags. `src/commands/watch.rs:623` and
`src/watch.rs:864` both carry the ordering argument inline. This is the one ADR
in the slot whose citations are part of its decision.

## ADR 0056 — The Host port is wide because the machine is, and the concerns are named rather than separated


### 1. The decision, in one sentence
A proposed split of `Host` into eight consumer-shaped traits is rejected on the
finding that **no consumer of this port narrows to one concern**, and the port is
instead given nine non-overlapping supertraits named for kinds of effect
(`Clock`, `Environment`, `Files`, `Links`, `Keys`, `Processes`, `Waiting`,
`Terminal`, `Network`, plus the intermediate `Filesystem: Files + Links`) with
`Host` declaring nothing of its own, so narrowing becomes opt-in and no
production consumer changes.

### 2. Does it still hold?
**Holds, precisely.** The trait declarations are at `src/host/mod.rs:360`
(`Clock`), `:368` (`Environment`), `:424` (`Files`), `:504` (`Links`), `:544`
(`Filesystem: Files + Links`), `:549` (`Keys`), `:558` (`Processes`), `:610`
(`Waiting`), `:642` (`Terminal`), `:679` (`Network`), and
`src/host/mod.rs:692-695`:

```rust
pub trait Host:
    Clock + Environment + Filesystem + Keys + Processes + Waiting + Terminal + Network
{
}
```

Counted from the source, the methods per trait are Clock 1, Environment 6,
Files 16, Links 3, Keys 3, Processes 5, Waiting 3, Terminal 4, Network 1 — **42,
exactly the ADR's table**, with `Host` itself declaring none.

The two corrected placements are in place and both carry the reasoning at the
site: `user_id` under `Environment` (`src/host/mod.rs:410-415`, naming both the
root refusal and the `gui/<uid>` domain), and `sleep` under `Waiting`
(`src/host/mod.rs:609` — *"contention wait and has nothing to do with a process
(ADR 0056)"*).

`host::prelude` exists as decided — `src/host/mod.rs:713-719`, a glob of the nine
plus `Host`, with the doc comment carrying the whole asymmetry argument
(*"A `&dyn Host` needs none of it … Holding a `FakeHost` or a `RealHost` is the
other case"*). Used by `tests/your_machine.rs:29` among others.

The conformance narrowing landed as described: `tests/conformance.rs` imports
`Clock, FakeHost, Files, Filesystem, …` and its table is
`asserts: fn(&dyn Filesystem, &Path, &str, DateTime<Utc>)` (`:141`), with the
cross-concern sentence argued in the field's own doc at `:130-140` and the header's
thirty-line claim about reach at `:18-25` — quoted by the ADR verbatim and still
present. Cases that do not need the clock take `_now`.

Both adapters are still one file each (`src/host/fake.rs` 2495 lines,
`src/host/real.rs` 2505), as decided.

Amended in place in its own Consequences by ADR 0059 — the amendment block is
present in the file and corrects the "38 ways" claim.

### 3. Supersession
Supersedes nothing; superseded by nothing. **Amended in place by ADR 0059, and
0059 says explicitly that this is a refusal to supersede:**

> ADR 0056's Consequences is amended in place rather than superseded. Its core
> finding is strengthened by what the fake's state turned out to look like, and
> ADR 0046 and ADR 0051 both set the precedent for refusing to supersede a
> mostly-correct record.

0056 in turn **explicitly refuses to amend ADR 0025**: *"ADR 0025 stands
unamended, and so does ADR 0055's restatement of it: `&dyn Host` remains the
only port Perch has."* Two refusals to supersede, one in each direction.

### 4. Reasoning that outlives the document
The central finding, stated as a general claim about ports over machines:

> The port is wide because the machine is wide, and anything that touches the
> machine touches several of its surfaces at once.

and the restatement in the Consequences that is written to be the answer to a
future reviewer:

> the finding is not *42 is fine*, it is *no consumer of this port narrows to
> one concern, and the width is the machine's rather than the interface's.*

and two rules that are about Rust and testing generally rather than about this
port:

> the cost of splitting a trait is paid by whoever holds the concrete type,
> which in this repository is only ever a test.

> a stub at this port is admissible only where `conformance.rs` has already
> declared it cannot adjudicate *and* the trait is thin enough that the stub is
> the whole contract.

### 5. Others deciding the same thing
- **ADR 0059** — the same subject at the state layer, already amending this one
  in place. The two read as one decision taken in two passes, and 0059's core
  argument is *"this is ADR 0056's own thesis arriving in the state."* Strongest
  merge candidate in the slice after 0008/0021/0025.
- **ADR 0025** — decides that the Host port is one of Perch's two seams, which is
  the premise 0056 reasons from; 0056 already restates it.
- **ADR 0055** (*the Watcher's round stays where it is and the ordering is a
  type it cannot skip*) — cited here for its restatement of the one-port claim,
  and it is the same kind of decision (a type replacing a claim).
- **ADR 0045** — 0056 opens by naming it as the same shape of review
  (*"the arithmetic was wrong, and once fixed there was no size question left"*),
  and 0045 governs `tests/`, which is where 0056's cost actually landed.
- **ADR 0044** — cited for the drive-real-code-against-the-fake shape that
  makes the narrow-stub rejection stand.

### 6. Citations
19 total: 7 in `docs/`, 12 in code — `src/host/mod.rs`, `src/host/fake.rs`,
`tests/conformance.rs`, plus ADR 0059. Notably narrow: it is cited only in the
three files it is about.

**Load-bearing, and in several places the citation is the only record of why a
line reads as it does.** `src/host/mod.rs:543` explains why `Filesystem` is empty
and what would not compile; `:609` explains why `sleep` is filed under waiting;
`:712` justifies a glob import; `src/host/fake.rs:183-188` and
`tests/conformance.rs:130-140` each reproduce a paragraph of the ADR's argument
at the site it governs. `src/host/fake.rs:29` cites it for a naming choice
(`use super as port;`). Nothing decorative.

---

## ADR 0057 — A command that changes only the registry goes through one door, and the ceremony was never one


### 1. The decision, in one sentence
Perch holds the registry lock in exactly three shapes (no wait; a wait with
nothing yet to write, so the lock is deferred past it; a wait about what is
already held, so the lock is held across it and guarded by `still_ours`), and
shape 1 gets a door — `commands::only_the_registry`, whose closure is handed no
`Host` so that a command coming through it provably cannot reach a Credential
Store, a Profile or the Default Profile — while the proposed shared "destructive
ceremony" over `remove`/`purge`/`relogin` is refused because those three do not
share a sequence.

### 2. Does it still hold?
**Holds.**
- The door exists with the signature the ADR specifies and the withheld `Host` as
  its stated point: `src/commands/mod.rs:154-190` — "**`change` is handed no
  [`Host`], and that is the point rather than a convenience.**"
- Four call sites covering the eight arms the ADR names:
  `src/commands/enable.rs:48`, `src/commands/alias.rs:34` (2 arms),
  `src/commands/group.rs:76` (4 write arms), `src/commands/config.rs:94`.
- `still_ours` is adjacent and cross-referenced, with the doc comment teaching
  the ADR at the point of use: `src/commands/mod.rs:107-114` ("The counterpart to
  [`only_the_registry`] … ADR 0057 sets out why a third set — `add` and
  `relogin`, whose wait is a browser rather than a person — needs neither").
- Shape 3's cohort is intact: `still_ours` callers are `remove.rs:139`,
  `purge.rs:138` (plus `src/purge.rs:199`), `import.rs:61`, `export.rs:111`.
  `relogin` calls it nowhere, as the ADR requires.
- Shape 2 intact: `relogin` reads shared, logs in, then takes the exclusive lock
  (`src/commands/relogin.rs:55`, `:74`, `:85`); `add` likewise.
- `add` is still the deliberate near-miss: `src/commands/add.rs:105-114`
  hand-rolls the shape with a `profile::discard` compensation beneath it, exactly
  as the ADR predicts, and `src/commands/mod.rs:172-179` names it in prose.
- The position-of-save argument is intact: `src/commands/remove.rs:369-379`
  writes `active` mid-destruction with its own `with_note`.
- One number is stale: "There are 21 `registry::save` call sites" and "Every one
  of the 21 sites is in one of them" read as present tense; there are now **14**
  in production code, because the eight arms the door absorbed collapsed to the
  one save inside `only_the_registry` (`src/commands/mod.rs:191`). 21 − 8 + 1 =
  14, so the arithmetic is consistent with the carry-out; the sentence is written
  in a tense the file no longer occupies.
- One case sits outside all three shapes: `perch group list` takes no lock at all
  — "answered before the lock, because it writes nothing and taking the write
  lock to read is the wait this command refuses to make"
  (`src/commands/group.rs:70-73`, `:99-101`). It is a read, not a save site, so it
  does not falsify "every one of the sites is in one of them"; but the ADR's
  three shapes do not name it, and its own Consequences anticipate the shape of
  this ("If a future one takes the lock by hand and grows the fourth shape, this
  is the ADR that failed to prevent it").

### 3. Supersession
Supersedes nothing, is superseded by nothing, amends nothing, is amended by
nothing. It cites **ADR 0042** for the reversibility axis being wrong —
"reversibility really was the wrong axis" — and **ADR 0055** for why the three
shapes stay out of `CONTEXT.md`. It has no supersession edges at all, in either
direction.

### 4. Reasoning that outlives the document
> A rule nobody wrote down is a rule the next command discovers by having a bug.

> an interface more complex than the code it hides is the shallow module the
> review exists to find, arrived at from the other side.

> What separates the eight is not that they can be undone but that they touch
> nothing outside the registry, which is a statement about reach rather than
> about consequence, and is the one a signature can hold.

> A test asserting otherwise would be a lint wearing a suite's clothes

> The realistic ceiling is that the right way is shorter than the wrong way and
> this document says why; it is accepted rather than papered over.

The third is the most general: reach, not consequence, as the axis a signature
can enforce.

### 5. Others deciding the same thing
- **ADR 0042** — 0057 borrows its "reversibility was the wrong axis" finding
  wholesale and reaches it from a second direction. Note ADR 0042 is superseded
  in full by ADR 0049, with only the "a surface which writes at all pulls in
  locking…" sentence carried forward — so **0057 currently cites a superseded
  document for a sentence that is not the one 0049 preserved**. That is a
  cross-slice edge the assembler needs: whatever survives of 0042 must carry
  *two* sentences, not one.
- **ADR 0024** — the sequencing of `perch remove` is decided there and its
  lock/guard placement here; the two are the complete record of that command.
- **ADR 0025** ("a crate is taken where it does not cost a seam") and **ADR
  0056**/**ADR 0059** share the deep-module / interface-cost yardstick, and 0056
  and 0059 use the same "of the last twenty-five commits" evidence style.
- It is **not** a member of the 0047/0052/0053/0054 naming cluster despite
  touching the same files.

### 6. Citations
4 total — 0 in `docs/`, 4 in code. Files: `src/commands/mod.rs` (×2),
`src/commands/group.rs`, `src/commands/config.rs`. No ADR cites it.

**Load-bearing.** `src/commands/mod.rs:154` — "The counterpart to [`still_ours`],
and the pair is the whole of ADR 0057" — makes the document the definition of a
pair of functions; `:112` cites it for why a third set of commands needs neither
guard; `src/commands/config.rs:90-92` cites it to record that the shape was
written there first and three other commands were spelling it out by hand;
`src/commands/group.rs:283` cites it to explain why a helper had the shape it
had. Four sites, four arguments, no tags. This is the ADR whose deletion would do
the most damage per citation.

## ADR 0058 — A Reserve attaches where a heading names its Scope, and the table has none


### 1. The decision, in one sentence
The Reserve is said **only where a heading has already named the Scope it is
about** — a narrowed `perch list <group>` or `perch list ungrouped`, and nowhere
else on the human surface — while `--json` carries it at every breadth as fields,
because a section document already names its own Scope in a key; and that
divergence is a rendering constraint rather than a disagreement about a judgment.

### 2. Does it still hold?
**Holds**, in every clause including the ones about what is *deleted*.

- The attachment rule, gated on the heading rather than on the breadth:
  `src/commands/list.rs:496-520` (`reserve_lines`) — `if scope.heading().is_none()
  { return Vec::new(); }`, with the doc comment reproducing the argument:
  *"Off `Scope::heading` rather than off the breadth, because the heading is the
  condition rather than a proxy for it."*
- `src/reserve.rs:15-18` states it as the module's rule.
- `--json` at every breadth, as fields: `src/listing.rs:117-131`, with `null` for
  a Scope holding nobody and `null` where `may_cycle_within` is false, and the
  comment giving 0058's own disambiguation (*"`accounts` sits beside it: an empty
  array distinguishes 'nobody is here' from 'nobody has declared these a set'"*).
  The fields-not-prose ruling is at `src/reserve.rs:189-191`.
- The Ungrouped stay silent until `interchangeable`, off **one** answer rather
  than two: `src/listing.rs:88-98` — `reserve` is `self.ranked.then(…)`, the same
  boolean `order` reads.
- The footer is collected before it is written, so the blank line is decided by
  whether there is anything there: `src/commands/list.rs:462-490`, with the
  Consequences' own reasoning in the comment.
- The Reserve line sits before the `Cycling …` clause and the clause qualifies
  it: `src/commands/list.rs:477-483`.
- **The deletions held.** `window_lines` and `window_kinds` do not exist anywhere
  in `src/`. The empty-Scope branch is gone and its reason is written down at
  `src/reserve.rs:130-140`, explaining why the surviving branch beneath it cannot
  be reached by an empty Scope.
- `cycle::out_of_the_running` is public again with its one caller:
  `src/cycle.rs:780`, doc at `:776-779` restating why it is shared.
- `CONTEXT.md`'s **Reserve** says Scope rather than Group, as the Consequences
  require: *"A Scope rather than a Group, because the Accounts in no Group have
  one too once somebody has declared them interchangeable."*
- No new argv or exit code: `perch list` is unchanged in `src/main.rs:181`.

### 3. Supersession
**None in either direction**, and it is careful to say so about the document it
comes closest to:

> **ADR 0049 gains a reader rather than an amendment.** Its ban is on the two
> surfaces disagreeing about a judgment, and neither of these contradicts the
> other.

It also records a *removal* it is undoing rather than superseding: e5e6c6f
deleted `src/reserve.rs`, and this returns it "recovered rather than rewritten,
six of its eleven tests with it".

### 4. Reasoning that outlives the document
> "**The table's silence is a rendering constraint, not a domain one.**"

That is the general claim — a surface may decline to say a true thing for want of
somewhere to put it without that being a disagreement — and it is the sentence
that lets a human surface and a JSON surface differ without either lying.

Second, the rule about where a scope-level sentence may attach at all:

> "it names its own Scope in the sentence, which is the heading the table
> declined, badly"

Third, the reason the `won't do` was refused, which is a claim about what a
column can and cannot say:

> "the count is the one thing they cannot say: 'two of these three are worth
> switching to' is a fact about the set, and reading it off the column is
> arithmetic the reader does rather than a sentence Perch says."

### 5. Others deciding the same thing
- **ADR 0053** ("`status` is about one Account, and every set was already the
  listing") — the same rule from the other side: a fact about a *set* belongs to
  the listing and nowhere else. 0058's Considered Options refuse "A Reserve on
  `perch status`" by citing 0053 directly. Together they are one decision — *the
  listing owns every claim about a set* — applied to two candidate homes.
- **ADR 0049** ("Perch does not draw, and the ranking belongs to the listing") —
  the third instance of the same rule, and the one that created the vacancy 0058
  fills. 0049, 0053 and 0058 look like one document about what the listing owns.
- **ADR 0043** — 0058 cites it for the human-vs-machine surface split (*"a machine
  reading a shape is not a person reading a sentence"*), which is the same claim
  0058's central sentence makes.
- **ADR 0012** — supplies the measurement the Reserve refuses to aggregate; see
  0012 §5.

### 6. Citations
12 total — 2 in `docs/`, 10 in code and elsewhere: `tests/listing.rs`,
`src/listing.rs`, `src/reserve.rs`, `src/commands/list.rs`,
`pages/src/content/docs/status.md`.

**Load-bearing.** `src/commands/list.rs:496-505` is a seven-line doc comment
whose entire content is the ADR's argument, cited at the top.
`tests/listing.rs:824` reproduces the "heading smuggled into a sentence" phrasing
to say what the test is protecting; `tests/listing.rs:440` cites it for a
negative — a listing with nothing in it has no Reserve to say.
`src/reserve.rs:189-191` and `:406` cite it for the fields-not-prose ruling. The
narrowest footprint in this slice, and the highest density of explanation per
citation.

## ADR 0059 — The fake's world is held by concern, and the clock was never one


### 1. The decision, in one sentence
`FakeHost`'s 40 flat fields are grouped into nine — seven concern structs, a
`Stall`, and the bare `effects` recorder — with **no `Clock` struct**, because
`now` was found to be written at four sites that all immediately hand control to
`while_waiting`, making the clock and the interruption one mechanism rather than
two concerns' state; the structs carry no behavior, every `RefCell` stays
per-field, and no builder or call site changed.

### 2. Does it still hold?
**Holds.** `src/host/fake.rs:194-207` declares exactly the nine fields the ADR's
table names: `environment`, `fs`, `keys`, `processes`, `waiting`, `terminal`,
`network`, `stall`, and `effects` — with `effects` kept bare and its doc saying
why (*"Written from seven of the nine concerns and read back whole … so it is
cross-cutting on purpose and stays one bare field"*).

- No `Clock` struct exists; `Stall` (`:350-355`) holds exactly two fields,
  `now` and `somebody_else`.
- Both renames landed: `env` → `vars` (`:216`) and `while_waiting` →
  `somebody_else` (`:354`), while `pub type WhileWaiting` keeps its name
  (`:166`).
- `somebody_else_arrives` exists as the extracted private helper
  (`:1309-1314`), and it does take the closure out before running it, which is
  the documented re-entrancy the per-field `RefCell` decision protects.
- `Filesystem` keeps all twelve fields (`:234-…`), with the doc at `:229-233`
  restating the both-ways-traffic argument and citing ADR 0056.
- `home` is a bare `PathBuf` (`:211`); everything else is a `RefCell`.
- The structs carry no behavior — helpers such as `mark_written` (`:1316-1321`)
  are still on `FakeHost` and reach across structs in one expression
  (`self.fs.modified` and `self.stall.now` in the same statement), exactly as
  the ADR requires.
- The file's own preamble at `:172-193` is a faithful restatement of the whole
  decision including the three-crossings measurement.

`fake.rs` is now 2495 lines against `real.rs`'s 2505 — grown from the 2057/1904
the ADR measured, which is the ADR's own point about growth being
per-arrangement rather than per-method, still running.

### 3. Supersession
Supersedes nothing. Superseded by nothing. **It amends ADR 0056's Consequences in
place, and says explicitly that this is not a supersession** — quoted in full
under 0056 above (*"amended in place rather than superseded … ADR 0046 and ADR
0051 both set the precedent for refusing to supersede a mostly-correct
record"*). The corresponding amendment block is physically present in
`docs/adr/0056-…md`'s Consequences, so the edge is recorded from both ends.

### 4. Reasoning that outlives the document
The finding, which is a general claim about fakes rather than about this one:

> Time does not pass in this fake because a clock ticks. It passes because an
> effect took time, and while that effect was in flight somebody else touched
> the machine.

with its directional qualification, which is the part that would be lost:

> The rule is *whoever moves the clock lets somebody else in*, not the converse.

and the reusable test, which is stated as a rule for future fields:

> This also supplies the test for what is cross-cutting: **mutated by more than
> one concern**, not merely read by several.

and the measurement instruction, which is a claim about how to evaluate this
class of refactor at all:

> The cost this ADR pays down is per *arrangement*, not per method. That is the
> measurement to re-run if this is ever revisited: count the fields that arrive
> without a `Host` method, not the methods.

### 5. Others deciding the same thing
- **ADR 0056** — the obvious partner: same subject, same finding, one already
  amending the other in place. A merged document would be one decision about the
  Host port's shape at both the interface and the state layer, and 0059's own
  framing (*"ADR 0056's thesis arriving in the state"*) argues for it.
- **ADR 0045 / ADR 0044** — both rule on the shape of `tests/`, which is where
  the fake's growth comes from and what 0059's per-arrangement measurement is
  about.
- **ADR 0046 and ADR 0051** — cited here only as precedent for refusing to
  supersede; a merge partner for the *practice*, not the subject.

### 6. Citations
6 total: 1 in `docs/` (ADR 0056), 5 in code — all in `src/host/fake.rs`. The
lowest cite count in the slice and the most concentrated: it is cited nowhere
outside the single file it rewrote.

**Load-bearing.** `src/host/fake.rs:349` justifies the absence of a `Clock`
struct at the point where a reader would look for one; `:395` carries the
per-arrangement multiplier finding; `:491` marks the take-once semantics;
`:1305` is on `somebody_else_arrives` itself, which is the mechanism the whole
ADR turns on. `:188` cites 0056 and 0059 together in one sentence about why the
sub-structs have no methods. All four read are explanatory.

## ADR 0060 — A document is written where its parts can be reached, and `Account` is below both of them


### 1. The decision, in one sentence

The Account `--json` document moves out of `commands/list.rs` into a new
`src/listing.rs` rather than onto `Account` in `registry.rs`, under the general
rule that **a document is written in the lowest module that already reaches
everything it names** — because putting it on `Account` would have `registry`
importing the two modules (`cycle`, `utilization`) that import it.

### 2. Does it still hold?

**Holds, completely and recently.**

- `src/listing.rs` exists (403 lines) and holds exactly what the ADR listed:
  `Section` with the ranked-versus-held distinction (`:56-98`), `scopes`
  (`:163-190`), `scope_json` (`:155-160`), and `document` (`:192-238`).
- `src/commands/status.rs:25` is `use crate::listing;` — no module in the tree
  reaches sideways into a command. (Verified: no `use crate::commands::` in any
  non-command module.)
- `From<&registry::Scope> for Scope` is gone; `listing::scope_json` takes a
  `registry::Scope` directly and `commands::list::Scope` keeps its
  `Everything` arm (`src/commands/list.rs:133`, `:152-160`).
- `CONTEXT.md` gained **Listing** and **Section** — both present in the "Showing
  what you have" section, with Section's entry carrying "That distinction is the
  load-bearing part".
- `commands/list.rs` is 692 lines (the ADR predicted 705; it has since shrunk
  further, consistent with ADR 0061's prose cuts).
- The four unit tests moved with it and are at `src/listing.rs:240-403`.
- "Nothing about the output changed" — `tests/listing.rs` and
  `tests/configuring.rs` are intact.

### 3. Supersession

Supersedes nothing and is superseded by nothing. It does not amend anything
either; it names `Quarantine::document` and `Active::document` as staying exactly
where they are and explains why they are not exceptions.

### 4. Reasoning that outlives the document

- The rule itself, block-quoted in the original: *"**A document is written in the
  lowest module that already reaches everything it names.**"* It is stated as a
  general convention and reproduced in `src/listing.rs:19-24`.
- *"Rust would compile it. … That is exactly why it is worth a decision rather
  than a lint: the thing that stops it is a reason, and a reason that is not
  written down is one that gets overturned by whoever next finds the convention
  more compelling than the direction."* The general claim: a constraint no
  compiler enforces has to be written down or it will be undone.
- *"The direction is worth more than the convention here."*
- *"`registry` is what Perch stores … The Account document is not stored state.
  It is a view composed of stored state plus two figures derived from it."*
- *"a module that pulled `Quarantine::document` away from `Quarantine` would be
  trading a real locality for a filing system"* — the general case against
  by-kind directories.
- Reuses 0049's rule in a new place: *"A second spelling of it in `registry.rs`
  is the two-surfaces-disagreeing failure ADR 0049 exists to prevent, arriving in
  a document instead of on a screen."*

### 5. Others deciding the same thing

- **The placement cluster.** ADR **0055** ("The Watcher's round stays where it is,
  and the ordering is a type it cannot skip" — `src/watch.rs` means, `src/commands/watch.rs`
  does), ADR **0056** (the Host port is wide because the machine is, concerns
  named rather than separated) and ADR **0059** (the fake's world is held by
  concern). All four are "where does this code live and why", all four decide it
  by reach or concern rather than by kind, and 0060 is the shortest of them. A
  merged record of module-placement rules is the obvious survivor shape.
- **ADR 0049 / 0053 / 0058** — 0060 is the carry-out of the listing those three
  designed, and shares `src/listing.rs` with them.
- **ADR 0025** — a different seam question (crates), same test shape ("does it
  cost a seam"). Weaker partner.

### 6. Citations

1 total — 0 in `docs/`, 1 elsewhere: `src/listing.rs`.

**Load-bearing, and it is the ADR's whole point.** The single site is
`src/listing.rs:19-24`, the module header, which reproduces the argument in full:
"Here rather than on [`crate::registry::Account`] … This module is the first
place all three can be reached at once (ADR 0060)." That comment is the guard the
ADR says a lint cannot be; one citation is the right number for a decision whose
subject is one file's existence.

---

## ADR 0061 — Perch says what it did, and explains itself only when it refused


### 1. The decision, in one sentence

Perch reports what it did and what the person could not have predicted, and
explains itself only when it refused — applied at three budgets (acting commands
report tersely, the Watcher's `waiting` and `switched` rounds become data while
`held`, `nowhere` and `cooling` keep prose, showing commands keep the table and
say each invariant fact once beneath it) — with a verbosity flag, a relocation of
the cut prose, a line-count test and a suite-wide assertion sweep all explicitly
rejected.

### 2. Does it still hold?

**Holds.** All four carry-out tickets have landed — the last two are the tip of
the branch (`5c542ad` "the remaining acting commands say what they did, and eight
of them were explaining themselves", `3a2b789` "each invariant fact is said once
beneath a Listing", `18e9565` "the Watcher's round line is data, and only its
refusals argue").

- Acting commands: `src/commands/switch.rs:199-283`. The Capture line is cut for
  `Captured::Copied` with the ADR's reason inline (`:207-213`); the other five
  outcomes all still speak (`:216-263`); the Cycling-within-Group sentence has
  folded into the landing line — `format!("Switched to {named}, {chosen}.")`
  (`:271-278`) — and the figures keep their age via `utilization::write_figures`.
- The Watcher: `src/watch.rs:13` — "What follows the figure is where ADR 0061
  cut. `waiting` and `switched` are …"; the threshold is off the round line
  (`src/watch.rs:474`, `:1268`), and `tests/common/mod.rs:571-575` records that
  the harness's round-finder had to stop looking for the word `threshold`
  because of it.
- Showing commands: `registry::how_to_repair_them` (`src/registry.rs:209`) — "The
  same repair, for however many Accounts are in that state — said once, because
  it is the same repair (ADR 0061)" — against `Quarantine::shown_of`, the varying
  half (`src/registry.rs:134-140`). Asserted at `tests/listing.rs:905-907`.
- No verbosity mechanism exists: no `--quiet`, no `--verbose`, no terminal
  branching on output volume anywhere in `src/`.
- The guide was audited with the work: `pages/src/content/docs/switching.md` and
  `accounts.md` both carry 0061 citations.

### 3. Supersession

**Supersedes nothing, explicitly and with reasons for each near-miss:** "**Nothing
is superseded, and three ADRs look like collisions from a distance.** ADR 0043
says the sentence is the designed artifact and is about how a sentence is
asserted, not how many there are; it also says wording is changed here
deliberately and often, which is the permission this decision uses. ADR 0015 is
why every figure carries its age, and every figure still does. ADR 0036 is why a
hold says what is holding it, and this decision is what keeps those lines long
while the ones around them shrink."

Superseded by nothing. It is the newest ADR in this slice and the most recently
carried out.

### 4. Reasoning that outlives the document

The richest in the slice — nearly all of it is general:

- *"Perch says what it did, and what the person could not have predicted. It
  explains itself only when it refused."* The rule itself.
- *"A thing that happens on every single run is the definition of predictable, and
  predictability is earned by the guide, not re-earned by every invocation. A
  refusal is the opposite: nothing happened, the person cannot see why, and the
  next step is not obvious. That is the one moment the prose is the product."*
- *"the ordinary case announcing that it was ordinary"* — reused verbatim as a
  diagnosis at `src/commands/switch.rs:210` and `src/commands/add.rs:324`.
- *"silence on the path that always runs, prose on the paths that do not."*
- On the rejected flag: *"It is also how a project avoids deciding what its output
  should be — the sentences stay, unexamined, behind a flag nobody types. … If a
  sentence is worth printing, print it; if it is not, delete it."*
- On the rejected relocation: *"That is the flag answer wearing a different hat:
  the sentence survives without anyone having to defend it, and a second command
  inherits an explanation that was written for a first."*
- On the rejected cap: *"it would bless whatever sits under the cap and say
  nothing about whether a sentence earned its place. … Prose stays defended by
  somebody reading it, which is the only thing that has ever caught one of these
  here."*
- *"each varying fact is said once — … and each invariant fact is said once in
  total"* — the rule now implemented as the `shown_of` / `how_to_repair_them`
  pair.
- *"Guide edits ride inside each ticket rather than following behind. A guide that
  documents output is wrong from the moment the output changes, and separating the
  two ships a knowingly false guide for however long the gap lasts."*

### 5. Others deciding the same thing

- **ADR 0043** (a sentence is asserted by the claim it makes) — the strongest
  merge partner in the corpus for this document. They are the two halves of one
  subject: 0043 decides how Perch's prose is *asserted*, 0061 decides what prose
  there *is*, and 0061 cites 0043 four times as its permission and its instrument.
  The tell: 0043's opening specimen is the exact seven-line `perch switch` output
  0061 deleted, so 0043's body now quotes output that no longer exists —
  cross-slice, and worth the assembler's attention.
- **ADR 0036** (held promises nothing was changed) — 0061 is what keeps the
  Watcher's `held` line long; 0036 is why it says anything at all. Adjacent
  rather than duplicative, but they decide the same three Watcher statuses.
- **ADR 0046** (the Watcher's numbers are arithmetic and only the threshold is a
  preference) — the threshold leaving the round line is 0061's cut applied to
  0046's one preference.
- **ADR 0015** (a figure carries its age) — 0061 explicitly protects it
  ("shortening output is not license to start making it"), so a survivor merging
  them must keep the carve-out.
- **ADR 0018** (a refresh degrades the display rather than failing the command) —
  a refusal-versus-report rule in the same register.

### 6. Citations

73 total — 3 in `docs/`, **70 in code, tests and the guide**: the most-cited ADR
in this slice by a wide margin. `src/watch.rs` (9 sites), `src/commands/watch.rs`,
`src/commands/switch.rs`, `src/cycle.rs`, `src/registry.rs`, `src/config.rs`,
`src/login.rs`, and ten of the twenty command modules; twelve test suites plus
`tests/common/mod.rs`; three guide pages.

**Load-bearing, uniformly.** Every site read states what was cut and why:
`src/commands/switch.rs:207-213` explains the Capture line's silence,
`src/registry.rs:139` explains why the repair is printed for the Listing rather
than per Account, `tests/purging.rs:125-128` explains why a sentence appears in
the question and not again in the report, and `tests/common/mod.rs:573` records a
harness change forced by the cut ("left as it was, this would have found no
decisions at all"). The density is a consequence of the ADR's own instruction that
assertions be re-pointed one at a time under 0043's rule rather than swept.

## ADR 0062 — The site is rendered by one thing, and a stylesheet was never the answer to a theme


### 1. The decision, in one sentence

Replace mdBook and the hand-written landing page with one Astro + Starlight
project in `pages/`, moving the guide to `pages/src/content/docs/` because
Starlight's `docsLoader()` takes no directory option — accepting a `node_modules`
tree and a package manager in the installer deploy path, fenced inside `pages/`,
with the lockfile replacing the mdBook `sha256` pin.

### 2. Does it still hold?

**Holds**, comprehensively, with one clause voided by 0063 and one stale comment.

- `pages/` is one Astro + Starlight project: `pages/astro.config.ts` configures
  `starlight({...})`, `pages/package.json` depends on `astro ^7.2.2` and
  `@astrojs/starlight ^0.41.7`, `packageManager` is `pnpm@11.22.0`.
- The guide is `pages/src/content/docs/` — 9 `.md` pages plus `index.mdx`, the
  one `.mdx`, exactly as the ADR describes. `pages/src/content.config.ts:5-7`
  gives the reason in the ADR's own terms.
- Sidebar autogenerated (no `sidebar` key, and the comment in `astro.config.ts`
  says why); per-page ToC from Starlight; search is Pagefind via Starlight.
- The pin replacement is in place and cited: `ci.yml:315-319` — *"`--frozen-lockfile`
  is what replaced the mdBook tarball's `sha256`"*; `pages.yml` and `ci.yml` both
  read `packageManager` through corepack.
- `pages/public/install.sh` and `install.ps1` exist; `packaging/pages/` is gone;
  `packaging/install-test.sh` is back in `packaging/` and says why at line 26-28.
- The assertion changes landed: `tests/publication.rs` has no
  `the_summary_lists_only_pages_that_exist`; it has nine tests including
  `the_installers_stay_at_the_root_of_the_site` (line 214) and the two title
  tests the frontmatter decision required
  (`every_page_says_its_title_in_frontmatter`, line 392;
  `no_page_says_its_title_a_second_time_as_a_heading`, line 410).
- The stylesheet decision held to the letter: `pages/src/styles/perch.css`
  carries the accent pair in Starlight's own variables and says *"Nothing else
  from that file is here"*; the wrap moved to
  `expressiveCode.defaultProps.wrap` in `astro.config.ts`.
- Fencing held: `pages/.gitignore`, `pages/package.json`, `pages/pnpm-lock.yaml`
  all under `pages/`; `.github/dependabot.yml:59-60` is
  `package-ecosystem: npm`, `directory: /pages`, with the "npm covers pnpm"
  reason in the comment. The `site` job is in `ci.yml:292` and is named in the
  `success` gate at `ci.yml:403`.
- Three lists → one, mostly. The README's command table survives (`README.md:109-123`)
  and `every_guide_page_is_named_by_every_index_of_the_guide` now has a
  one-element index list. The splash card grid is asserted separately and in both
  directions by `the_landing_page_leads_into_the_guide`
  (`tests/publication.rs:309`) — so there are still two artifacts naming the
  pages, but the ADR's claim was that the splash *is* the guide's index rather
  than a copy of one, and the test's doc comment says exactly that.
- Void: *"the site deploys from `main`"*, killed by 0063's amendment note at
  `0062:53-59`.
- Stale, minor: `astro.config.ts`'s `editLink` comment says *"the branch the site
  deploys from"*. The site no longer deploys from the branch (0063); the edit
  link pointing at `main` is still right, the reason given for it is not.
- Versions moved as the ADR predicted they would: TypeScript `^7.0.2` matches,
  Astro/Starlight ranges are carets rather than pins, per
  `docs/releasing.md:206-207`.

### 3. Supersession

**Supersedes ADR 0035 in part**, and the partition is written into 0035's header
rather than left to a reader (quoted in full under 0035 §3). 0062's own statement
of what it keeps: *"What is carried forward from ADR 0035 entire, and is the
reason this change is small: **the guide is written once and the site renders
it.**"*

**Amended by ADR 0063**, in part and in place, at `0062:53-59`: *"The clause
about deploying from `main` is void … Everything else here stands, the installers
at the root of the deployment included — that is the half of the sentence that
was right."* An amendment rather than a supersession, and 0063 does not claim
otherwise.

### 4. Reasoning that outlives the document

- The diagnosis, which is the general lesson: *"The problem was never that one of
  the site's two artifacts was plain. It was that there were two."* And *"The
  carve-out is the defect."*
- *"A renderer that makes the author write navigation by hand is not one whose
  navigation was the reason to keep it."*
- The typing argument, which generalizes past this site: *"The site's
  configuration is the one place in this repository where a mistyped object key
  produces a build that succeeds and a site that is wrong."*
- *"re-applying corrections to a theme chosen for its defaults is how a site
  arrives back where it started."*
- **The sentence the other ticket wants, verbatim:**

  > There is no installed base and no compatibility to keep, so an older version
  > is never the safer one; it is only the older one.

  (`docs/adr/0062-…md:162-163`; the full sentence spans those two wrapped lines.
  The clause the ticket is likely after is *"There is no installed base and no
  compatibility to keep"*.) Its context is the version policy: *"Written down
  because a docs site is exactly the kind of thing that gets stood up once
  against whatever was current that afternoon and then pinned there by nobody's
  decision."* Note this is the same premise `CLAUDE.md`'s "Nobody is using this
  yet" states, and the opposite of the premise ADR 0041 says its decision is paid
  for by.
- *"a dozen open pull requests a week is training to rubber-stamp."*

### 5. Others deciding the same thing

**ADR 0035** — already superseded in part; the two are one decision about the
site's renderer taken twice. **ADR 0063** — amends it and is the other half of
"how the site is published"; the two are a plausible single document, since 0063
is short and is entirely about a clause of 0062. **ADR 0028** and **ADR 0031** —
cited by both as the origin of the root-URL constraint, but they decide releases
and channels. **ADR 0025** (a crate is taken where it does not cost a seam) is
the closest analogue for the dependency-cost reasoning, applied to a different
ecosystem.

### 6. Citations

18 total — 2 in `docs/`, **16 elsewhere**, the most code-side of any ADR in this
slice: `tests/publication.rs`, `docs/releasing.md`, `docs/adr/0035`,
`.github/dependabot.yml`, `pages/.oxfmtrc.json`, `pages/src/styles/perch.css`,
`pages/src/plugins/guide-links.ts`, `pages/.gitignore`,
`packaging/install-test.sh`, `.github/workflows/ci.yml`,
`pages/src/content.config.ts`, `pages/package.json`,
`.github/workflows/pages.yml`, `pages/astro.config.ts`.

**Load-bearing throughout.** Read four of the thinnest-looking:
`pages/src/content.config.ts:5-7` explains why the guide is not in `docs/guide/`;
`pages/.oxfmtrc.json:2-5` explains why the formatter is scoped away from the
guide; `pages/.gitignore:1-2` explains why the ignore file is here rather than at
the root; `packaging/install-test.sh:26-28` explains why the script and the
installer it tests live in different directories. Every one answers "why is this
file shaped like this" rather than tagging it.

---

## ADR 0063 — The guide describes a Perch you can install, and only the installers answer to a merge


### 1. The decision, in one sentence

Split the site's publication by clock: the guide is deployed from the newest
release that carries a site and moves only when `release.yml` calls the Pages
workflow as its last job, while the two installers are overlaid from `main` and
move on a merge — one deployment assembled from two refs, with the overlay
diffed before it publishes.

### 2. Does it still hold?

**Holds**, and it is the most literally implemented ADR in the slice.

- `.github/workflows/pages.yml` has exactly the three triggers: `workflow_call`,
  `push` on `main` limited to `paths: [pages/public/**]`, and
  `workflow_dispatch`. It is **not** triggered by `release: [published]`, and the
  comment explains why in the ADR's words.
- `release.yml:417-434`: job `site`, `needs: release`, `uses:
  ./.github/workflows/pages.yml`, with the three permissions named by the caller
  (`contents: read`, `pages: write`, `id-token: write`) — matching *"A workflow
  called by another gets no more permission than the caller grants it."*
- The two-ref assembly is there step by step: checkout `ref: main` with
  `fetch-depth: 0`; record `main`'s sha; the *"Which Perch the guide describes"*
  step walking `git tag --sort=-v:refname` for the newest tag carrying
  `pages/astro.config.ts` and falling back to `main` with a `::notice::`; a
  detached checkout of that ref; then `git checkout <main-sha> -- pages/public/install.sh
  pages/public/install.ps1`.
- The release is checked out **entire**, as the ADR insists, and the workflow
  comment gives the ADR's reason (the logo `astro.config.ts` imports from
  `docs/assets/`) — verified: `logo: { src: "../docs/assets/icon.svg" }` in
  `pages/astro.config.ts`.
- The silent-failure guard the ADR demanded exists: step *"The installers really
  are main's"* pipes `git show <main-sha>:pages/public/<installer>` through
  `diff -` against `pages/dist/<installer>`.
- `ci.yml`'s `site` job still builds `pages/` on every pull request
  (`ci.yml:292`, `pnpm build` at 324) and is still in the `success` gate
  (`ci.yml:403`) — matching *"that the *next* release's site builds, not the one
  being served."*
- `docs/releasing.md:153-194` restates the whole arrangement for a human.

Nothing in the tree contradicts it. The one thing to note is that the fallback
branch is still live: no tag yet carries `pages/`, so today's deploy builds the
guide from `main` — which the ADR anticipated and called *"a rule and not a
stopgap"*.

### 3. Supersession

Supersedes nothing. **Amends ADR 0062 in part**, by writing the amendment note
into 0062 itself (`0062:53-59`) rather than by claiming supersession. It also
reaches back through 0062 to 0035: *"Two documents said the site deploys from
`main`, and both said it as an aside. ADR 0035 said it because everything on the
site was fixed by a merge, and ADR 0062 carried the sentence forward without
reopening it."* So one clause of two earlier ADRs dies here; nothing else in
either does. Nothing supersedes 0063.

### 4. Reasoning that outlives the document

- The general principle, which is the document's title and survives whatever
  renders the site: **the guide describes a Perch somebody has**, i.e. *"The two
  halves keep different time."*
- The carried-forward-clause warning, which is a lesson about ADRs rather than
  about deploys: *"carrying it forward from ADR 0035 without reopening it was the
  mistake."* This is the most transportable sentence in the slice for whoever is
  ruling on supersessions.
- The `GITHUB_TOKEN` rule, stated generally: *"`release.yml` creates the GitHub
  Release with `GITHUB_TOKEN`, and GitHub will not start a workflow from an event
  that token made. So a `release: [published]` trigger on the Pages workflow is
  not merely inelegant — it never fires, and it fails by doing nothing at all,
  which is the failure this repository keeps writing tests to avoid."*
- The overlay-verification principle: *"An overlay that silently failed would
  deploy the release's copies and look identical to one that worked."*
- The refusal of the banner: *"telling a reader that the page they are on
  describes software they do not have is a worse answer than giving them the page
  that describes the software they do."*
- *"`workflow_dispatch` … is the difference between a deploy somebody chose and a
  deploy that happened."*

### 5. Others deciding the same thing

**ADR 0062** — it amends 0062 in place and decides one clause of it; the two
would merge cleanly, with 0063 becoming a section of 0062. **ADR 0035** — the
originator of the voided clause. **ADR 0028** (*a release is assembled by
workflows this repository owns*) is the closest partner outside the site chain:
0063 is 0028's reasoning extended to a second artifact, and 0063 cites 0028 and
0031 for why the installers must stay at the root. **ADR 0032** (*Perch does not
look for its own updates*) shares the "what does a versionless URL promise"
question but decides a different thing.

### 6. Citations

7 total — 2 in `docs/`, 5 elsewhere: `docs/releasing.md`,
`docs/adr/0062`, `.github/workflows/pages.yml`, `.github/workflows/release.yml`,
`.github/workflows/ci.yml`.

**Load-bearing, unusually so for a seven-citation ADR.** `pages.yml`'s header is
a twenty-line restatement of the decision including the `GITHUB_TOKEN` reasoning
and the deliberate exclusion of the workflow file from the trigger paths;
`release.yml:417-425` explains why the deploy is a called job rather than an
event; `docs/releasing.md:161-186` is the human-facing version. Not one of the
five is a bare tag.
