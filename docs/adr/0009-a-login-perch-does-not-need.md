# A login Perch does not need

Anyone installing Perch already has an account logged into Claude Code. Perch
copies that credential into a new profile and records that account as the active
one, rather than asking for a login it does not need.

The obvious objection is that adoption leaves two copies of one credential. But
that is exactly the state ADR a-switch-is-written-down-first exists to handle —
it is where every Switch leaves things — so adoption does not add a case, it
starts the system in its steady state.

## Consequences

Every profile after the first is created by launching Claude Code inside the new
profile's directory to log in there, leaving the current session untouched.

This falls out of profiles being real config directories
(ADR claude-code-chooses-the-store), and it is worth protecting. A design that
only ever snapshots the live credential has just one place to log in, so adding
an account means first logging out of the one you are using — losing your
session to gain an account. Perch never requires that.
