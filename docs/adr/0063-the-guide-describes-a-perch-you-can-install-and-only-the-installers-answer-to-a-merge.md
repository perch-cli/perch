# The guide describes a Perch you can install, and only the installers answer to a merge

Two documents said the site deploys from `main`, and both said it as an aside.
ADR 0035 said it because everything on the site was fixed by a merge, and ADR
0062 carried the sentence forward without reopening it. Neither noticed that it
had stopped being true of half of what the site publishes.

What it costs was measurable rather than theoretical. Five days after v0.2.0 was
released, `main` was fifty commits past it and five of those were breaking:
`perch alias` had reversed its arguments, `status` had narrowed to one Account,
the Settings had moved onto Scopes, `holdings` had been named, and the TUI was
gone. Every one of the ten guide pages had changed. So the site taught the syntax
of a Perch nobody could install, to a reader who had just installed the one it
was not describing — and it was confident about it, because there is no version
selector on that site and nothing on the page saying which Perch it means.

## The two halves keep different time

The guide describes a Perch somebody has. It is published when a release is
published, and between releases it does not move.

The installers are pasted into a terminal from a URL with no version in it, and
that URL is in terminal histories, in the README, and in the install guide. They
are published when a merge fixes one. This is not a preference: an installer that
had to wait for a release to be corrected would stay broken until then, which is
the whole reason the site exists at the root rather than inside a release
(ADR 0028, ADR 0031).

Nothing else is published by a merge. A typo fixed in the guide waits for the
next release, and that is the cost of this document, stated where somebody will
find it when they wonder why their fix is not live. Releases here are a merge of
a standing pull request, so the wait is a decision rather than an obstacle.

## One artifact, assembled from two refs

GitHub Pages serves one deployment, so both halves are built by one run: the
newest release is checked out entire, and then `main`'s two installers are put
back on top. What deploys is the release's site carrying the current installers.

The release is checked out *entire* rather than as `pages/` alone, because
`astro.config.ts` imports the logo from `docs/assets/`, and a site assembled from
two commits is one that no checkout reproduces. What crosses the boundary is
exactly the two files whose paths may never move — which is the same pair
`the_installers_stay_at_the_root_of_the_site` has been asserting all along, and
the deploy now diffs them against `main` before it publishes. An overlay that
silently failed would deploy the release's copies and look identical to one that
worked.

"The newest release" is really the newest release that carries a site, and the
distinction earns its place: the site moved into `pages/` after v0.2.0 was cut,
so for one release cycle the honest answer is `main`, and the workflow says so in
its own log rather than failing. It is a rule and not a stopgap — a repository
whose tags were all deleted gets the same sensible answer.

## The release calls it rather than announcing it

`release.yml` creates the GitHub Release with `GITHUB_TOKEN`, and GitHub will not
start a workflow from an event that token made. So a `release: [published]` trigger
on the Pages workflow is not merely inelegant — it never fires, and it fails by
doing nothing at all, which is the failure this repository keeps writing tests to
avoid. It was written that way first and caught by somebody asking whether the
thing would work.

The rule was already documented here. `release-plz.yml` says it about pushes, and
it is the reason the tag is pushed with a real token rather than the automatic one:
a tag pushed with `GITHUB_TOKEN` would sit on `main` with nothing watching. The
same sentence covers Releases.

So `release.yml` calls this workflow as its last job, and that is better than the
event would have been anyway: the call comes after the artifacts are uploaded, so
the guide is never live describing a release nobody can download yet. A workflow
called by another gets no more permission than the caller grants it, so the calling
job names the three the deploy needs.

## What still watches it

`ci.yml`'s `site` job goes on building `pages/` from the branch on every pull
request, and it is worth being clear about what that now proves: that the *next*
release's site builds, not the one being served. That is the check that was
wanted either way — a site that cannot build is found on the pull request that
broke it rather than by a release that cannot deploy.

## What was not chosen

**A banner saying the guide documents `main`.** It is four lines and it tells the
truth, and telling a reader that the page they are on describes software they do
not have is a worse answer than giving them the page that describes the software
they do.

**Versioned documentation.** Starlight has none built in, one version of the
guide is what this project has needed so far, and a version selector is a
decision to make when somebody needs to read the docs for a release they are not
on.

**Deploying the whole site from the release.** That is this document, minus the
installers, and the installers are the reason the site is served from the root of
a deployment at all.

**Publishing when the workflow itself changes.** It was among the trigger paths
while the site was built from the branch, where it belonged: the file was part of
what produced the output. It is not any more. What goes out is the release's site
carrying `main`'s installers, and changing how those two are assembled changes
neither — so the run would replace the live site with a byte-identical copy of
itself, and spend a deployment of the thing that serves the installers to do it.
Applying a change to the assembly is `workflow_dispatch`, which is the difference
between a deploy somebody chose and a deploy that happened.
