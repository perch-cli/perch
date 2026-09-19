# Carried means rehearsed

The unit `perch watcher install` writes carries each enabled Provider's client
under that Provider's variable — a Claude Code under `PERCH_CLAUDE_BIN`, a Codex
under `PERCH_CODEX_BIN` — because the service manager's PATH holds neither
(ADR the-machine-runs-the-watcher). This record decides *which* of them that is:
not the first hit on the installing shell's PATH, but the first hit that runs
where the Service will run it. One rehearsal, run once per Provider, against the
candidates and the version argument that Provider declares.

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

Each candidate on the shell's PATH, in PATH's own order, is run with the
Provider's own version argument under the environment the unit provides: the
manager's own PATH instead of the shell's, the home directory, and the variables
the unit carries — including any client already rehearsed this run, so a
Provider resolved earlier is visible to one resolved after it. The first that
exits 0 is written into the unit; when anything was passed over on the way, the
first of it is named in the install's output, with the exit that damned it. When nothing answers, the
unit carries none for that Provider — the Service holds and says why, as it does
when nothing was found at all — and the install names the exit and the repair
variable. Still not a refusal, for the reason finding no client is not one: the
repair is ordinary and the re-install is idempotent. Each Provider is answered
on its own, so one that has nothing to carry never withholds another's answer.

The manager's PATH is a fixed value per arrangement rather than a question put
to the machine: launchd's is pinned in launchd, and systemd's compiled default
is strict enough that rehearsing under it only ever passes over more. A
stricter rehearsal errs toward carrying a binary that runs anywhere, which is
the property being bought. Windows is exempt: a Scheduled Task inherits the
user's registry environment, which no fixed value stands for and which is close
to the shell's anyway, so the first hit is carried as before.

An explicit `PERCH_CLAUDE_BIN` or `PERCH_CODEX_BIN`, and a `cli-path` Setting,
still pass through verbatim, unrehearsed. Each is somebody's word, each is the
escape hatch this decision's own refusal message names, and rehearsing it would
refuse the person deliberately pointing at a setup the rehearsal misjudges.

## The alternatives

**Carry the shell's PATH into the unit.** It would make the wrapper work — the
wrapper re-resolves through whatever PATH it gets — but it bakes a session's
PATH into a file read at every login, and it hands that PATH to everything else
the Watcher runs: `security`, `launchctl`, the service manager's own tools,
each now resolvable to whatever a user directory shadows them with. The one
thing the Watcher needs from PATH is where each client is, and that already fits
in a variable per Provider (ADR the-machine-runs-the-watcher).

**Reject shims by inspection.** Classifying a candidate means reading and
understanding arbitrary shell script, and a false positive refuses a working
machine. Running the candidate asks the only question that matters and asks it
exactly.

**Refuse the install when the first hit fails.** The reported machine had a
working Claude Code further down PATH; refusing would hand the user a manual
search the install can perform itself in PATH's own order.

## Consequences

An install and an upgrade's re-install now run each candidate once until one
answers, so a machine whose first hits are broken pays a version read per broken
hit, per Provider. The sentence the install prints is the moment this is told:
which client the unit carries for each Provider, what was passed over and why,
or why nothing was carried at all. A machine where the shell and the Service
genuinely disagree — the class of failure this and ADR
the-machine-runs-the-watcher both orbit — is now reported by the command that
creates the disagreement, rather than discovered by the first round that acts on
it.
