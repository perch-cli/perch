# A gate lives outside the tree

Perch has been public since the first push, and `pull_request_creation_policy`
was `collaborators_only` the whole time. A stranger could read it, fork it, push
to their fork, and then find the button gone. That field is now `all`, and it is
the only setting here that opens anything; the rest are gates.

Which is why this document exists. Every one of them is a GitHub setting, and a
setting is invisible from a checkout. A reader holding this repository can see
`ci.yml`, the ruleset's effects on their own branch, and nothing else. The
closed door was invisible for the same reason, and to the maintainer too: it
took viewing the repository as an account without write access to find it.

The ones that are in the tree stay where they are. `CONTRIBUTING.md` says what
to send and `.github/pull_request_template.md` says what to tick.

## What the ruleset already did

`main` carries a branch ruleset with no bypass actors, and the owner of this
repository cannot bypass it either. A change reaches `main` as a squashed pull
request whose title is a Conventional Commit, with `success` and `title` green
and the branch up to date. Required approvals are zero, which is the only
setting on the list that exists because there is one maintainer rather than
because it is right.

So the protection a first outside contribution might have threatened was never
weaker than the protection the maintainer works under. Opening up changed
nothing about merging. It changed what runs, and when.

## Approval is per run, not per pull request

A fork's pull request runs `ci.yml` on GitHub's runners. That workflow executes
the contributor's own code: `cargo test --features your-machine` against the
runner's real keychain, and the installer scripts from the pull request's own
tree. GitHub withholds every secret from a fork's run and hands it a read-only
token, which was watched happening: the coverage job read `present=false` and
warned that it had measured a report and uploaded nothing. So the blast radius
is a runner that is destroyed afterwards. It is not nothing, and it is not
credentials.

What the gate holds is every workflow the pull request triggers, not only the
one that runs fork code. `pr-title.yml` reads a title and executes nothing, and
waits alongside `ci.yml` anyway. Both required checks therefore report nothing
at all until the click, so what an outside contributor first sees is not a
pending check but an absent one.

What it does not hold is a GitHub App. An App is not a workflow run and has no
approval to wait for, so the Socket Security checks report on a stranger's
branch before anybody has looked at it. Whatever an App is allowed to see on
this repository, a fork's pull request is enough to show it.

GitHub offers three answers and this repository takes the middle one, requiring
approval from anyone who has never had a commit or pull request merged here.
The narrowest option only catches accounts new to GitHub, which is a property an
attacker can wait out. The strictest holds every fork's run behind a click
forever, and was what this repository ran before this decision: correct for a
project nobody was contributing to, and wrong for one asking to be.

The cost is paid per push, not per pull request. GitHub re-arms the gate on
every run until that contributor's first merge lands, so a stranger iterating on
review feedback waits for a click each time. That was measured rather than read
about: a second commit on an already-approved pull request was held again. It is
the price of the middle option and there is no setting that lowers it. It buys
the guarantee that an account created this morning cannot reach the runner that
holds a keychain.

## What an action is allowed to be

Actions are restricted to an allowlist, and the allowlist names what is used:
GitHub's own, `Swatinem/rust-cache`, `codecov/codecov-action`. Verified creators
as a class are not allowed, because "verified" is a set this repository does not
maintain and cannot enumerate, and every action in every workflow here is in one
of those three groups anyway. Every `uses:` is pinned to a commit SHA and GitHub
rejects a workflow that is not.

Adding an action later takes a settings change first. That is the point: the
allowlist is a claim about what runs, and a claim that can be checked is worth
the one edit a year it costs.

## CODEOWNERS routes, it does not gate

`.github/CODEOWNERS` names one owner and auto-requests a review. It changes no
merge condition, and `require_code_owner_review` is off.

Turning it on would make this repository unmergeable. GitHub refuses a
self-approval, so a sole code owner who requires code owner review has blocked
every pull request they open, which is nearly all of them. The file exists ahead
of the need because the day a second maintainer arrives it is a one-line change
rather than a new decision.

There is no maintainer team for the same reason. A team of one buys nothing
GitHub does not already give the owner, and it would make `CONTRIBUTING.md` and
`SECURITY.md` false: both promise a single maintainer and set a response time
against that promise. The team is created when somebody accepts, not before.

Two-factor authentication is required across the organization, restricted to
methods GitHub classes as secure. That is the one gate here protecting something
that cannot be undone, which is the ability to push to a repository people
install binaries from.

## The release environment is a pause, not a second pair of eyes

The `release` environment holds the npm publish behind a required reviewer, and
that reviewer is the person who tagged. Self-review is permitted and an
administrator can bypass the prompt, both necessarily: with one maintainer, a
gate that refused a self-approval would be a gate that never opened.

So it stops nothing an attacker does and it is not claimed to. What it stops is
a tag pushed in the wrong minute reaching a public registry before anybody has
looked, which is the failure that actually happens. It becomes a real approval
the day there is a second reviewer to name, and nothing about it changes then
except `prevent_self_review`.

The environment is restricted to tags matching `v*`. Without that restriction
any ref running a job that names the environment could request it, which is a
door left open for no reason: only the tag-triggered job in `release.yml` has
ever needed it (ADR this-repo-assembles-a-release).

The tag ruleset refuses deletion and force-pushes and does not restrict
creation. It cannot: release-plz pushes the tag with `RELEASE_PLZ_TOKEN`, the
ruleset has no bypass actors, and a creation rule would stop every release. What
guards tag creation is that write access is held by one person.

## What is deliberately absent

No CLA and no Developer Certificate of Origin check. Apache-2.0 § 5 already
places a submitted contribution under the license, `CONTRIBUTING.md` says so,
and a sign-off check spends its enforcement on first-time contributors failing a
push for a reason they have to go and read about.

No scanning for non-provider patterns and no validity checks on found secrets.
Both are wanted and neither is available: they need GitHub Secret Protection,
which is a paid plan. Secret scanning itself and push protection are on, being
free on a public repository. Revisit at the same time as anything else that
turns on paying GitHub.

Nothing in this document is in `CONTEXT.md`. A Contributor is a person in a
workflow, not a noun Perch has any concept of, and that file is Perch's
dictionary.
