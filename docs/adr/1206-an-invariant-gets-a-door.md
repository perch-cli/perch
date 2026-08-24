# An invariant gets a door

Six deep reviews of the whole tree returned about a hundred and thirty findings.
Sorted by what actually went wrong they are not a hundred and thirty problems.
Seven shapes account for nearly all of them, and six of the seven appeared in
five or six consecutive reviews:

| shape | found in |
|---|---|
| `validate` gains a name rule, `migration::forward` does not, every command refuses | all six |
| the Watcher's loop asks the interrupt flag or the lease only at the loop's edges | five |
| a secret built with `format!` inside a `Zeroizing` buffer, prefixes abandoned on growth | five |
| `FakeHost` answers something no real machine answers | five |
| a second entry into one write sequence, with the guard on only the first | five |
| a path guard comparing spelling where it means place | three |
| the Back-off charged for a round that asked Anthropic nothing | two |

Two findings were made *by* the previous review's fix. A Group name that bricked
every command got there because the review before had added a rule to
`validate_name` with no matching step forward, and a Purge that deleted the
Export it had just offered was the relative half of a symlinked hole the review
before had closed.

Each rule was stated once, in prose, in one file's header — and honored by hand
at ten call sites. That is the whole of the pattern. The design is not the
problem: the enforcement is a reader's, and a reader is what a tenth call site
does not have.

## The rule

> **An invariant with more than one call site is enforced by a type or by a
> lint, and the sites reach it through one door.**

Not "documented at the widest scope that owns it" — that is the comment
standard, and it is what these six reviews found insufficient. A comment is
addressed to somebody adding the eleventh call site, and the eleventh call site
is written by somebody who did not read it.

## The four doors

| invariant | door |
|---|---|
| a containment guard follows every link on both sides | `host::Settled`, reached through `host::settled` and `host::is_inside`; `Path::starts_with` is a `disallowed_methods` entry in `clippy.toml` |
| a buffer holding a secret never frees a copy of one | `secret::Secret`, whose growth wipes what it grew out of, and which `host::write_double_quoted` and `write_escaped` take instead of a `String` |
| a Watcher acts only while it holds the watch and has not been asked to stop | `anthropic::send`, the only path from that module to the network, whose ask is a required parameter; the port's `Network::http` is a `disallowed_methods` entry in `clippy.toml` naming the three senders that reach it, and `commands::watch::Watch::goes_on` is what a Watcher answers the ask with |
| a step forward lands on the shape this build reads | `migration::CARRIED_TO`, a literal with `const _: () = assert!` against `registry::CURRENT_VERSION` |

Three of the four are types, and the first is a lint beside its type because
there is no type for "a path somebody resolved" that `std` will not also hand out
unresolved. The third is a lint beside its type for a narrower reason: the
signature stops a *sender* from reaching the network unasked, and the lint stops
a caller from going round the senders. Each door carries the reasoning; each site
that genuinely wants the raw form carries an `#[allow]` with a `reason`, which is
the record that the question was asked rather than missed.

## What a door is worth, and what it is not

A door removes the class of bug where a site forgot the rule. It does not remove
the class where the rule itself is wrong. `Secret` cannot tell whether a caller
built a token in a plain `String` before handing it over; `is_inside` cannot
decide that an unresolvable path should count as inside — it decides it *once*,
which is the whole of what a door buys.

The third door moved twice before it reached the only path out, and that is this
document's own rule read back to it. It began as `Watch::goes_on` called by hand
at five sites; a review added the closure `observe::refresh` takes; two further
reviews found requests still going out past both, because a door decides which
sites are correct and not which steps ask. It is a parameter of `anthropic::send`
now — the one function every Anthropic request passes through — and a sender that
does not take one does not compile. Each move was towards the narrowest place the
invariant could be stated, and the two reviews in between are what stating it
anywhere wider cost.

So the fourth entry above is the weakest of the four and worth naming as such.
`CARRIED_TO` is a compile-time assertion rather than a door: nothing forces a
new shape to arrive with a step, only that the two numbers agree. The test
`every_version_short_of_the_current_one_is_carried_forward` is the other half,
and it is a test rather than a type because a version range is a runtime fact.

## What is rejected

**A door per rule, and no lint.** `Path::starts_with` is on `std`'s own type
and cannot be taken away, so a wrapper only helps the sites that reach for the
wrapper. The lint is what makes the raw form a build failure, and the wrapper is
what makes it unnecessary. Both, or the wrapper is a suggestion.

**Panicking when a `Secret` under-reserves.** The width a caller counts would
then be load-bearing again, which is what five reviews found it cannot be:
`width_of` in `host::real` was "correct by about thirty bytes of coincidence"
the last time a review read it. Growth that wipes makes the arithmetic an
optimization, so a miscount costs a copy rather than a token.

**Making `Watch::goes_on` the only way to renew.** `act` renews once after the
Switch has landed, where the answer changes nothing — there is no step left to
stop before. That call is `Watch::kept_up`, named for the absence of the
question, so a reader can see that the one place not asking is the one place
with nothing to ask about.

## Consequences

- A new containment guard that reaches for `Path::starts_with` does not build.
- A new request to Anthropic asks before it goes out, because a function in
  `anthropic` that does not take the ask cannot reach the port. A new step that
  spends time *without* sending a request still asks by hand, and there this is a
  door rather than a wall: the alternative, a step protocol, stays refused in
  ADR an-ordering-is-a-type.
- A stopped ask travels back as `Refused::Stopped` rather than as an unreachable
  network. The two would otherwise pace a Back-off the same way, and one of them
  is a question nobody was asked.
- `observe::refresh` no longer asks the Host whether this process was asked to
  stop. Its callers answer, which is why `perch list --refresh` answers `Ok(())`
  unconditionally: it installs no handler, so the old call was always false.
- Adding a tenth trait to the port, or a method to one of the nine, is a failing
  conformance test until the table asks it or `UNASKED` says why it cannot
  (ADR a-class-not-its-instances).
