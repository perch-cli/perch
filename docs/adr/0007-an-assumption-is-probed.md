# An assumption is probed

Every decision Perch has taken rests on reverse-engineered internals: the
keychain service-name hash, the three-lock refresh protocol, the shape of
`sessions/*.json`, symlink handling in the atomic writer, the layout of
`.claude.json`, and three undocumented endpoints. None is a public contract, and
the drift is continuous — the lock protocol was established by reading bundle
2.1.218, and the machine it was written on already ran 2.1.220.

Perch therefore probes what it depends on before acting, and when something is
unrecognized it refuses the dangerous operation and says which assumption
failed. The failure a user experiences becomes "Perch declined to swap:
unrecognized credential store, Claude Code 2.3.0" rather than being silently
logged out by a background auto-update.

## Drift is caught by three things

Naming a thing for drift is not one of them, and that is a finding rather than
an omission: a feature, a flag or a suite named after the purpose does not
acquire the purpose.

**The trigger is a schedule.** Claude Code updates continuously while this
repository may not change for a week, so CI carries a weekly run against
whatever Claude Code is current. That is what turns *we would have noticed* into
*we did*. Weekly against `latest` is the cadence, chosen deliberately rather
than derived; re-opening it without a drift event to reason from would be churn.

**The reading is a step of its own.** The assertions against the installed
bundle are kept apart from the rest of CI, so a failure there reads as upstream
drift rather than as a fault in the pull request that happened to be open.

**The protection is this module.** It is the only part of any of this a user can
feel: one place that stops recognizing Claude Code, and every dangerous
operation gated on the verdict it returns.

Not everything Perch believes can be probed at runtime. Where a Credential lives
is derived rather than checked — a store that holds nothing is a logged-out
Profile, not a broken belief — so what guards that derivation is a test that puts
a Credential where Perch says one goes and asks the installed Claude Code to
read it back (ADR a-suite-is-named-and-gated).

## Consequences

These assumptions cannot be scattered through the codebase as bare paths and
struct definitions. They belong behind a single module that owns the question
"what do we believe about the installed Claude Code, and how confident are we?",
with every dangerous operation gated on its answer. That seam has to exist from
the first commit; retrofitting it means touching everything.

A refusal names the assumption *and* the Claude Code it was reading. An
assumption about an unnamed version is a bug report nobody can act on.
