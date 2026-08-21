# Code lives where it reaches

The Account document — the shape `perch list --json` puts under `accounts` and
`perch status --json` puts under `active` — was a free function in
`commands/list.rs`, and `commands/status.rs` imported a sibling command to reach
it. That is the one place in the tree where a module reached sideways into a
command, and moving it was never in question.

Where it moved to was. The obvious answer is wrong for a reason worth writing
down, because the next person to look will reach for it as quickly as this one
did.

## The obvious answer

Perch already spells this. `Quarantine::document`, `Active::document`,
`Report::document`, `Reserve::document`, `utilization::document`,
`cycle::headroom_document` — six of them, every one sitting on or beside the
type it is about. An Account document that was a free function in a command file
was the only exception, so `Account::document` in `registry.rs` looks like the
convention finally being applied.

## Why it cannot go there

`registry.rs` imports `error`, `host`, `lock` and `probe`, and nothing else. The
Account document names two things from outside that set: the Account's Headroom,
which is `cycle::headroom_document`, and its Utilization, which is
`utilization::document`. Both of those modules import `registry` already —
`cycle` because a ranking is made over Accounts, `utilization` because a figure
is read off one.

So `Account::document` would have `registry` importing the two modules that
import it.

**Rust would compile it.** Modules within a crate may refer to each other
freely, and there is no build error waiting to catch this. That is exactly why
it is worth a decision rather than a lint: the thing that stops it is a reason,
and a reason that is not written down is one that gets overturned by whoever
next finds the convention more compelling than the direction.

The direction is worth more than the convention here. `registry` is what Perch
*stores* — the state a `load` validates and a `save` writes, which is the one
thing every other module is written in terms of. The Account document is not
stored state. It is a view composed of stored state plus two figures derived from
it, and the derivations are what the other two modules exist to make.

> **A document is written in the lowest module that already reaches everything it
> names.**

For an Account, that is `listing` — which imports `registry`, `cycle`,
`utilization` and `reserve`, and is imported by the two commands that write the
document. `Quarantine::document` and `Active::document` stay exactly where they
are, and are not exceptions to this: each names only what its own module already
holds, which is why the convention fit them in the first place.

## Considered Options

**Inline the two figures into `registry.rs`.** The document would name only
`Account`'s own fields, and `Account::document` becomes possible. Refused: the
Headroom in the document is the figure a Section's order was made on, and
`cycle::headroom_document` is where that figure is decided. A second spelling of
it in `registry.rs` is the two-surfaces-disagreeing failure ADR 0049 exists to
prevent, arriving in a document instead of on a screen — with the added
unpleasantness that the copy would sit in the module the ranking is not made in.

**Invert `cycle` and `utilization` onto `registry`.** Would let the document go
on `Account`, at the cost of moving a ranking and a quota figure into the module
that stores state. `registry.rs` is 3,758 lines before anything is added to it,
and what would be added is precisely the derived material it has stayed clear of.

**Leave the document in `commands/list.rs` and have `status` keep importing it.**
The state this replaces. It works, and it is the reason the tree had one command
importing another: the shape both commands write was owned by one of them, so the
other had no way to reach it except through its sibling.

**A `document` module for every document Perch writes.** Would gather the six
into one place and answer the question once. Refused because five of the six are
correctly placed already — a module that pulled `Quarantine::document` away from
`Quarantine` would be trading a real locality for a filing system.

## Consequences

**`src/listing.rs` exists**, holding what a Listing *is*: `Section` with the
ranked-versus-held distinction, `scopes`, `scope_json`, and the Account
`document`. `commands/list.rs` keeps what it *does* — the breadth somebody asked
for, the table, and the sentences under it — and drops from 933 lines to 705.

**`commands/status.rs` imports `listing` rather than `commands::list`.** No
module in the tree now reaches sideways into a command.

**`From<&registry::Scope> for Scope` is deleted.** It existed so a Section could
spell its own Scope as a listing's, for one call site. With the Section on the
other side of the boundary, `listing::scope_json` names a Scope directly and
`commands::list`'s own `Scope` adds the one arm a Cycle's Scope must never have.

**`Listing` and `Section` are `CONTEXT.md` terms.** `Section` is the one carrying
weight: it is where ranked-versus-held lives, ADR 0049 calls that distinction its
weightiest, and `--json` puts `"order": "ranked" | "held"` in a contract scripts
branch on.

**Nothing about the output changed.** No argv, no flag, no exit code, no key and
no rendered line (ADR 0044), which is why the whole of `tests/listing.rs` and
`tests/configuring.rs` passes untouched. What moved is where the code lives, and
four unit tests came with it — three about a Section and the partition it rests
on, one about the Scope that leads.
