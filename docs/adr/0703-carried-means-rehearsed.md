# Carried means rehearsed

The unit `perch watcher install` writes carries a Claude Code under
`PERCH_CLAUDE_BIN`, because the service manager's PATH holds no `claude` anybody
installs today (ADR the-machine-runs-the-watcher). This record decides *which*
`claude` that is: not the first hit on the installing shell's PATH, but the
first hit that runs where the Service will run it.

The gap it closes was reported from a machine whose first PATH hit was a 4 KB
bash wrapper shipped by a terminal app. The wrapper is a real, executable file
that answers `--version` from any interactive shell — by walking `$PATH`,
skipping its own directory, and `exec`ing the first other `claude` it finds.
Under the PATH launchd hands a LaunchAgent, that walk finds nothing and the
wrapper exits 127. The install carried it, said so in a sentence that read as
success, and every health command agreed, because every health command runs
from a shell where the wrapper works. The user's own Claude Code sat
twenty-fourth on the same PATH and would have satisfied every assumption.

No property of the file separates the two. Both are executable, both print the
same version, and reading a candidate to classify it means parsing arbitrary
shell script. The one test that separates them is the one the Service will
apply anyway: run it, there.

## The rehearsal

Each `claude` on the shell's PATH, in PATH's own order, is run with `--version`
under the environment the unit provides: the manager's own PATH instead of the
shell's, the home directory, and the variables the unit carries. The first that
exits 0 is written into the unit; when anything was passed over on the way, the
first of it is named in the install's output, with the exit that damned it. When nothing answers, the
unit carries none — the Service holds and says why, as it does when nothing was
found at all — and the install names the exit and the `PERCH_CLAUDE_BIN`
repair. Still not a refusal, for the reason finding no `claude` is not one:
the repair is ordinary and the re-install is idempotent.

The manager's PATH is a fixed value per arrangement rather than a question put
to the machine: launchd's is pinned in launchd, and systemd's compiled default
is strict enough that rehearsing under it only ever passes over more. A
stricter rehearsal errs toward carrying a binary that runs anywhere, which is
the property being bought. Windows is exempt: a Scheduled Task inherits the
user's registry environment, which no fixed value stands for and which is close
to the shell's anyway, so the first hit is carried as before.

An explicit `PERCH_CLAUDE_BIN` still passes through verbatim, unrehearsed. It
is somebody's word, it is the escape hatch this decision's own refusal message
names, and rehearsing it would refuse the person deliberately pointing at a
setup the rehearsal misjudges.

## The alternatives

**Carry the shell's PATH into the unit.** It would make the wrapper work — the
wrapper re-resolves through whatever PATH it gets — but it bakes a session's
PATH into a file read at every login, and it hands that PATH to everything else
the Watcher runs: `security`, `launchctl`, the service manager's own tools,
each now resolvable to whatever a user directory shadows them with. The one
thing the Watcher needs from PATH is where `claude` is, and that already fits
in a variable (ADR the-machine-runs-the-watcher).

**Reject shims by inspection.** Classifying a candidate means reading and
understanding arbitrary shell script, and a false positive refuses a working
machine. Running the candidate asks the only question that matters and asks it
exactly.

**Refuse the install when the first hit fails.** The reported machine had a
working Claude Code further down PATH; refusing would hand the user a manual
search the install can perform itself in PATH's own order.

## Consequences

An install and an upgrade's re-install now run each candidate once until one
answers, so a machine whose first hits are broken pays a `--version` per broken
hit. The sentence the install prints is the moment this is told: which `claude`
the unit carries, what was passed over and why, or why nothing was carried at
all. A machine where the shell and the Service genuinely disagree — the class
of failure this and ADR the-machine-runs-the-watcher both orbit — is now
reported by the command that creates the disagreement, rather than discovered
by the first round that acts on it.
