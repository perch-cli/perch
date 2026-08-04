# Assumptions about Claude Code are probed at runtime, and Perch refuses when they fail

Every decision recorded so far rests on reverse-engineered internals: the
keychain service-name hash, the two-lock refresh protocol, the shape of
`sessions/*.json`, symlink handling in the atomic writer, the layout of
`.claude.json`, and three undocumented endpoints. None is a public contract, and
the drift is continuous — the lock protocol was established by reading bundle
2.1.218, and this machine already runs 2.1.220.

Perch therefore probes what it depends on before acting, and when something is
unrecognised it refuses the dangerous operation and says which assumption
failed. The failure a user experiences becomes "Perch declined to swap:
unrecognised credential store, Claude Code 2.3.0" rather than being silently
logged out by a background auto-update. Contract tests assert the same shapes
against the installed bundle in CI, to find drift before users do.

## Consequences

These assumptions cannot be scattered through the codebase as bare paths and
struct definitions. They belong behind a single module that owns the question
"what do we believe about the installed Claude Code, and how confident are we?",
with every dangerous operation gated on its answer. That seam has to exist from
the first commit; retrofitting it means touching everything.
