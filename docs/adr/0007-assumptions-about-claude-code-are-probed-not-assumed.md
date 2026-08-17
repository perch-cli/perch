# Assumptions about Claude Code are probed at runtime, and Perch refuses when they fail

> **Amended by ADR 0050**, in one sentence and with no supersession. The closing
> paragraph named "contract tests" as what finds drift in CI. That suite has
> been re-gated on consent and no longer exists under that name, and the flag it
> sat behind was never what caught drift anyway — the weekly scheduled run is.
> The sentence below is rewritten to name it. Everything else here stands:
> probing rather than assuming, and refusing at runtime, is exactly as sound
> after that decision as before.

Every decision recorded so far rests on reverse-engineered internals: the
keychain service-name hash, the three-lock refresh protocol, the shape of
`sessions/*.json`, symlink handling in the atomic writer, the layout of
`.claude.json`, and three undocumented endpoints. None is a public contract, and
the drift is continuous — the lock protocol was established by reading bundle
2.1.218, and this machine already runs 2.1.220.

Perch therefore probes what it depends on before acting, and when something is
unrecognized it refuses the dangerous operation and says which assumption
failed. The failure a user experiences becomes "Perch declined to swap:
unrecognized credential store, Claude Code 2.3.0" rather than being silently
logged out by a background auto-update. A weekly scheduled CI run asserts the
same shapes against whatever Claude Code is current, to find drift before users
do.

## Consequences

These assumptions cannot be scattered through the codebase as bare paths and
struct definitions. They belong behind a single module that owns the question
"what do we believe about the installed Claude Code, and how confident are we?",
with every dangerous operation gated on its answer. That seam has to exist from
the first commit; retrofitting it means touching everything.
