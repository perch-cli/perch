# Code lives where it reaches

> **A document is written in the lowest module that already reaches everything it
> names.**

Perch spells documents on or beside the type they are about:
`Quarantine::document`, `Active::document`, `Report::document`,
`Reserve::document`, `utilization::document`, `cycle::headroom_document`. Each
of those names only what its own module already holds, which is why the
convention fits them.

The Account document — the shape `perch list --json` puts under `accounts` and
`perch status --json` puts under `active` — does not. `Account::document` in
`registry.rs` looks like the convention finally being applied, and it is the
answer to reach for, so the reason it is wrong is written down here.

## Why the obvious placement is wrong

`registry.rs` imports `error`, `host`, `lock` and `probe`, and nothing else. The
Account document names two things from outside that set: the Account's Headroom,
which is `cycle::headroom_document`, and its Utilization, which is
`utilization::document`. Both of those modules import `registry` already —
`cycle` because a ranking is made over Accounts, `utilization` because a figure
is read off one. So `Account::document` would have `registry` importing the two
modules that import it.

**Rust would compile it.** Modules within a crate may refer to each other
freely, and there is no build error waiting to catch this. That is why it is a
decision rather than a lint: the thing that stops it is a reason, and a reason
that is not written down is one that gets overturned by whoever next finds the
convention more compelling.

The direction is worth more than the convention. `registry` is what Perch
*stores* — the state a `load` validates and a `save` writes, which is the one
thing every other module is written in terms of. The Account document is not
stored state: it is a view composed of stored state plus two figures derived
from it, and the derivations are what the other two modules exist to make.

For an Account, the lowest module that reaches everything it names is `listing`
— which imports `registry`, `cycle`, `utilization` and `reserve`, and is
imported by the two commands that write the document.

## Considered options

**Inline the two figures into `registry.rs`.** The document would name only
`Account`'s own fields. Refused: the Headroom in the document is the figure a
Section's order was made on, and `cycle::headroom_document` is where that figure
is decided. A second spelling of it in `registry.rs` is the
two-surfaces-disagreeing failure ADR the-listing-owns-the-set exists to prevent,
arriving in a document instead of on a screen — with the added unpleasantness
that the copy would sit in the module the ranking is not made in.

**Invert `cycle` and `utilization` onto `registry`.** Would let the document go
on `Account`, at the cost of moving a ranking and a quota figure into the module
that stores state — precisely the derived material `registry.rs` has stayed
clear of, in the largest file in the tree.

**Leave the document in `commands/list.rs` and have `status` keep importing it.**
It works, and it is the reason the tree once had one command importing another:
the shape both commands write was owned by one of them, so the other had no way
to reach it except through its sibling. No module in the tree reaches sideways
into a command.

**A `document` module for every document Perch writes.** Would gather the six
into one place and answer the question once. Refused because five of the six are
correctly placed already — a module that pulled `Quarantine::document` away from
`Quarantine` would be trading a real locality for a filing system.

## What a thing means, and what it does

The same rule read one level up: a subsystem is split by whether a function
decides something or performs it. `src/watch.rs` holds what a round *means* and
`src/commands/watch.rs` holds what a round *does*. What makes that line hold is
that nothing in `watch.rs` reaches the network or the filesystem, so everything
in it can be argued with in a unit test.

## A seam with one adapter is a hypothetical seam

The pressure this line comes under is a proposal to make the meaning half
testable by giving it a port of its own. For the round that was `WatchPorts`,
covering refresh, refuse-if-live, choose and perform, so the round could be
driven against a recording adapter.

What it buys is one real adapter of four one-line pass-throughs, one test-only
recorder that re-records what the fake Host already records, and a function that
performs no IO sitting behind a port. `&dyn Host` is the only port Perch has
(ADR a-crate-must-not-cost-a-seam), and the fake Host is the only way behavior
is driven (ADR the-binary-proves-its-surface). A second port earns its place by
having a second real adapter.

## A word about how Perch is built stays out of the glossary

`CONTEXT.md` is the domain's. The discriminator is whether the word is an idea
somebody has to hold in order to use Perch — or, equivalently, whether a script
branches on it.

**Listing** and **Section** are in, and `Section` is the one carrying weight:
ranked-versus-held lives there, ADR the-listing-owns-the-set calls that
distinction its weightiest, and `--json` puts `"order": "ranked" | "held"` in a
contract scripts branch on. A rendering is not disqualified by being a
rendering.

**Round** and **witness** are out. A load-bearing noun in an interface does have
to be a word the project knows — but the round has no module and no interface of
its own, so the glossary would be learning a word only the source uses, which is
the drift the vocabulary exists to prevent. If the round is ever given its own
interface, the term comes with it. **witness** is defined once in the source, at
`switch::Settled`, and every other use points there
(ADR an-ordering-is-a-type).

## Consequences

- **`src/listing.rs` exists**, holding what a Listing *is*: `Section` with the
  ranked-versus-held distinction, `scopes`, `scope_json`, and the Account
  `document`. `commands/list.rs` keeps what it *does* — the breadth somebody
  asked for, the table, and the sentences under it.
- `commands/status.rs` imports `listing` rather than `commands::list`.
- `From<&registry::Scope> for Scope` does not exist. A Section spells its own
  Scope as a listing's for one call site; with the Section on the other side of
  the boundary, `listing::scope_json` names a Scope directly and
  `commands::list`'s own `Scope` adds the one arm a Cycle's Scope must never
  have.
- Nothing about the output turns on any of this: no argv, no flag, no exit code,
  no key and no rendered line (ADR the-binary-proves-its-surface). What a move
  under this rule changes is where the code lives and which tests come with it.
