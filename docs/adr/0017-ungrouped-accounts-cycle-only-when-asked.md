# Ungrouped accounts cycle only when asked

An Account need not be in a Group. Adoption records the existing login with no
Group at all (ADR 0009), and `perch add` offers the organization as a default
the user may decline, so an ungrouped Account is the ordinary starting state
rather than an edge case.

Bare `perch switch` from an ungrouped Account Cycles among the other ungrouped
Accounts **only when a setting says it may**. That setting is off by default.
With it off, Perch switches nowhere and says why — that these Accounts have not
been declared interchangeable, and that the fix is either `perch group move` or
turning the setting on.

This is ADR 0002's rule followed to its conclusion. A Group is the declaration
that a set of Accounts is interchangeable; being ungrouped is the absence of
that declaration, not a weaker form of it. Cycling freely among ungrouped
Accounts would move someone from their work subscription onto their personal
one without ever having been told the two were substitutable — precisely the
failure Groups exist to prevent. The watcher already defaults to off for the
same reason: nothing changes underneath someone because they did not say it
could.

The setting is global rather than per-Group. An ungrouped Account has no Group
to carry it, and modelling the ungrouped pool as a Group with a reserved name
would make a Group mean two contradictory things — a declaration the user made,
and one Perch made for them.

## Considered Options

**Each ungrouped Account is its own singleton.** Bare `perch switch` would then
always report "already on the best Account". Rejected: adoption leaves the first
Account ungrouped, so out of the box the command the whole tool exists for would
do nothing, and say nothing that explained why.

**Ungrouped Accounts form one implicit pool that Cycles freely.** Rejected on
ADR 0002's own grounds, above. It is the convenient reading and the wrong one.

**Refuse bare `perch switch` from an ungrouped Account outright, with no setting
to change it.** Rejected: someone holding three subscriptions bought personally,
who has no interest in Groups, would have to learn the concept to reach the one
command that matters to them. The setting lets them say "yes, these are
interchangeable" once, which is the same declaration a Group makes, without the
machinery.

## Consequences

Perch gains global configuration, where before all configuration hung off a
Group. `perch config` needs a form that addresses a setting belonging to no
Group; the shape of that form is for the configuration spec to settle, along
with the key's name.

Bare `perch switch` gains a third honest non-outcome, alongside "every Account
in the Group is exhausted" and "you are already on the best one" (ADR 0012).
Like both of those it performs no Switch, explains itself, and exits with a
distinct code rather than pretending to have worked.

Turning the setting on is a declaration about every ungrouped Account at once,
present and future — including the next one `perch add` creates. Anyone wanting
a narrower statement than that wants a Group, which is what Groups are for.
