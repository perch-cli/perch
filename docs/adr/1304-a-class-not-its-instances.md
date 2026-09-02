# A class not its instances

Every fix in six deep reviews arrived with the test that catches it, and the
suite went from 1,292 to 1,442 cases. The same seven shapes came back anyway
(ADR an-invariant-gets-a-door), because a test written against a finding asserts
the finding. The next instance of the shape is a spelling nobody wrote a case
for.

Two of the seven are checkable as a class rather than as a list, and those two
are here. The other five are types, and a type needs no suite.

## The Registry, over the name space

> **Every version 1 Registry a published Perch could have written comes forward
> into one this build reads.**

`every_name_a_published_perch_accepted_comes_forward_into_one_that_loads`
generates the names — the reserved words, both folds of a Greek sigma, an escape
a terminal obeys, and every string up to three characters over the alphabet each
rule so far has turned on — and puts each in all three places a Group name is
written down and the one an Alias is.

What makes this sound rather than a second copy of `validate_name` is that its
filter is *history*. v0.1.0, v0.1.1 and v0.2.0 are the only builds that ever
stamped version 1, and what they let somebody type cannot change now. The filter
is v0.1.0's four checks and its own `to_lowercase` fold, written out in the
suite as the record it is.

`the_corpus_holds_names_this_build_refuses` is the guard on the guard: a corpus
of names this build already accepts would go on passing, having asserted that
the names Perch accepts are names Perch accepts.

## The port, over its own methods

> **Every method on the Host port is asked of both adapters, or says why not.**

`every_method_on_the_port_is_asked_of_both_adapters` reads the nine traits off
`Host`'s supertraits in `src/host/mod.rs`, reads every `host.<method>(` the
table writes, and refuses the difference. Both halves are read out of the tree,
so neither is a list to keep in step: a case that named the method it asks in a
field would be a case that can name the wrong one.

`UNASKED` is the exemptions, each naming a machine effect a suite may not have —
a terminal handed to a child, a request to Anthropic, the macOS keychain dialog,
this process's stdin. Nine of forty-three, and the list is asserted against the
port too, so an entry excusing a method that no longer exists fails.

The table was asking nineteen of forty-three when a review last counted it,
twenty-three when this was written, and thirty-four after it. The gap was not a
decision anybody made; it was a table with no reader.

## Why only two

A class is worth asserting where the instances are enumerable and the rule is
not. Names are enumerable and the rule moves; port methods are enumerable and
the table drifts. The other five shapes have neither property — there is no
enumeration of "places a secret buffer could be built" to sweep — which is why
they are doors instead.

## What is rejected

**A property-testing crate.** `proptest` would shrink a failing case to its
smallest spelling, which is worth something when a corpus is a random walk. The
corpus here is a cross-product small enough to print, and the failure names the
spelling outright. A crate must not cost a seam
(ADR a-crate-must-not-cost-a-seam), and this one would cost a build-time
dependency for shrinking that nothing needs.

**Generating Registries structurally as well as by name.** Two Groups differing
only in case, a Group that is also an Alias, a `checks` key naming nothing — the
generator could reach all three, and every one of them is a state v0.1.0's own
`validate` refused on load. A Registry no published Perch could read is a hand
edit, and refusing it with a sentence is this build's deliberate answer. The
generator therefore spells its two names apart, and that exclusion is the one
place this suite trusts a reader.

## Consequences

- A rule joining `validate_name` that no step forward can satisfy is a red test
  rather than a machine with no working command.
- Adding a method to the port fails `every_method_on_the_port_is_asked_of_both_adapters`
  until the table asks it or `UNASKED` says why it cannot.
- `UNASKED` is nine entries and will grow if somebody excuses a method rather
  than writing its case. The reason field is the only thing standing against
  that, and a reason is a reader's to judge.
- The name corpus is bounded at three characters. A rule that only a longer
  spelling can break is one this passes without asking about.
