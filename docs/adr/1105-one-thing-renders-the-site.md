# One thing renders the site

https://perch-cli.github.io/perch/ is one Astro and Starlight project in
`pages/`: the splash front page, the ten guide pages, and the two installers
`public/` carries to the root of the output. One project rather than two
artifacts sharing an accent color.

The guide is written once and the site renders it. `pages/src/content/docs/`
holds one copy of markdown, read on GitHub by anybody who clones or browses and
in a browser by anybody who does not, and a page is added by writing a file.
Nothing is generated from a copy, so there is no step at which two versions
could disagree.

## Why one thing rather than two

A site can be assembled from a hand-written landing page and a documentation
renderer beside it, and the seam is not free. The landing page carries its own
inline style; the renderer carries a stylesheet correcting the renderer. They
share an accent color and nothing else — not the type scale, not the
navigation, not the header, not the search — so moving between them is leaving
one site and arriving at another, and no quantity of CSS on the second closes
that. A documentation renderer does not want to be the front page, which is the
reason such a landing page gets carved out in the first place; the carve-out is
the defect.

The cost of the seam is not only a change of voice. A renderer that makes the
author write navigation by hand is not one whose navigation was the reason to
keep it: a chapter list in an `<iframe>`, a per-page table of contents typed out
as nineteen links kept in step by hand across five pages, previous and next at
the foot of the document, and a theme picker offering four palettes none of
which is Perch's. Starlight generates every one of those.

So Search is Pagefind and stays static, the sidebar is in the page, and each
page gets a table of contents on the right. Nothing renders the front page that
does not also render the guide.

## Where the guide lives

`pages/src/content/docs/`, and the path is a consequence rather than a
preference. Starlight's `docsLoader()` reads `src/content/docs/` and takes no
directory option — only `generateId()`. Defining the collection by hand with
Astro's generic glob loader and pointing it somewhere else is plausible and is
not in Starlight's documentation, and paying an unverified mechanism to keep a
directory name is the wrong trade when the whole build rests on it.

What matters about the guide's directory is that it holds one copy of markdown a
person or an agent can read without a browser. That is a property of the file
rather than of the path. `docs/adr/`, `docs/agents/` and `CONTEXT.md` are not
part of the site and do not live under it.

## The title, and the heading that is not in the file

Starlight requires `title` in frontmatter on every page and renders it as the
page's `h1`, so no guide page carries a `# Heading`. On GitHub a page opens with
a small frontmatter table and goes straight into prose.

That is a real loss and is accepted rather than glossed over. What this document
promises is one copy of the content, not the absence of metadata, and a title
rendered as a table cell is still a title in the file. What would break the
promise is `.mdx`, which makes one document into two sources. The guide is
`.md`. Exactly one file is `.mdx` — the splash page, which is the one place a
component earns anything.

## One list of the pages

A sidebar is not configured, so Starlight generates it from the pages that
exist and cannot omit one; the order is `sidebar.order` in each page's
frontmatter, which is the one thing the file system does not know. The splash
page's card grid is the guide's index, so the front page and the table of
contents are the same artifact rather than two that have to agree.

One list is written a second time and stays: the README's command table.
`npm/build.mjs` copies the README into the package, so it is npm's landing page
too, and an offline clone would lose its getting-started if that table became
links to a website. Three audiences, none derivable from the others, is what
`tests/publication.rs` is for — it fails when a page and an index disagree, and
names which index.

## Moving between two pages is a swap rather than a load

A click swaps this document instead of loading another one, and only the part
that changed moves. That is `<ClientRouter />` in the head plus cross-document
view transitions in the stylesheet, and the two are deliberately separate: the
transition is what the site does on its own, so a reader whose browser runs no
JavaScript, or who arrives at a page cold, gets it too.

The rule the transition follows is that nothing which did not change may move.
Left alone a browser snapshots the whole viewport and cross-fades it, header and
sidebar included, and what a reader notices is those two flashing — they are the
parts that did not change. So each is named out of the root snapshot and then
told not to animate at all, because a named element still cross-fades its own
two snapshots and the sidebar's differ by exactly the one highlight that was
supposed to move.

The cost is two component overrides. Starlight builds its Pagefind dialog inside
a `DOMContentLoaded` listener, which fires once per document, so a swapped-in
search box is one nothing ever fills; the override persists the element that was
built. Each override renders Starlight's own component rather than replacing it,
so there is no copy of Starlight's markup in this repository to fall behind.

## What says which bytes

`pnpm-lock.yaml` carries integrity hashes for the whole tree and `pnpm install
--frozen-lockfile` refuses to build anything else. The package manager's own
version is `packageManager` in `package.json` rather than whatever the runner
ships, so one place says which bytes and both CI and the deploy read that place:
a pull request is checked against the renderer the site is built with.

The cost is the largest thing this document buys and is not disguised: a
`node_modules` tree in a Rust repository, a weekly dependabot stream, and a
package manager in the deploy path that publishes the installers. It is fenced
rather than absorbed. `pages/` holds the `package.json`, the lockfile and its
own `.gitignore`, and nothing at the root of the repository suggests Perch is a
JavaScript project. Dependabot is told `package-ecosystem: npm`, which is the
value that covers pnpm — there is no `pnpm` to write there — and the group is
shaped the way cargo's already is, because a dozen open pull requests a week is
training to rubber-stamp.

## What the site is written in

TypeScript, and JavaScript only where something outside the project insists on
it. The Astro configuration, the content collection definition and anything with
logic in it are typed, on `astro/tsconfigs/strict`.

The reason is not ceremony. The site's configuration is the one place in this
repository where a mistyped object key produces a build that succeeds and a site
that is wrong — a sidebar silently autogenerated from nothing, a `base` that
does not match where Pages serves from, a loader pointed at an empty directory.
Perch's own build has a compiler between a typo and a green run, and the site is
not the single directory that does without one.

`npm/build.mjs` is not part of the site. It stays plain JavaScript, where it has
no dependencies and nothing to check.

## Versions are taken at the latest

Every dependency is taken at the highest version that works, and this document
was written against Astro 7.2.3, Starlight 0.41.7, TypeScript 7.0.2 and pnpm
11.22.0. Astro 7 wants Node 22.12 or newer, which the workflows already clear —
`release.yml` is on 24.

Written down because a docs site is exactly the kind of thing that gets stood up
once against whatever was current that afternoon and then pinned there by
nobody's decision. Nothing this build produces is kept: the output is
regenerated from source on every deploy, and the two files whose URLs may never
move are copied verbatim rather than rendered. So a version here carries no
forward commitment the way a Holding's does (ADR the-holdings-outlive-a-perch),
and an older one is never the safer one — it is only the older one.

What makes "the latest" safe to mean is pnpm's own default, left unset rather
than overridden: it will not install a package published within the last
twenty-four hours. That is the same kind of guard as the checksums the installers
verify and the commit SHAs the workflows pin — a compromised release is usually
found on its first day — and it is why every dependency here is a caret range
rather than an exact pin. The range says what the site needs and the gate picks
the newest version inside it that it is willing to install.

## The two halves keep different time

The guide describes a Perch somebody has. It is published when a release is
published, and between releases it does not move.

The installers are pasted into a terminal from a URL with no version in it, and
that URL is in terminal histories, in the README and in the install guide. They
are published when a merge fixes one. An installer that had to wait for a
release to be corrected would stay broken until then, which is the whole reason
the site is served from the root of a deployment rather than from inside a
release.

Nothing else is published by a merge. A typo fixed in the guide waits for the
next release, and that is the cost of this decision, stated where somebody will
find it when they wonder why their fix is not live. A release here is a merge of
a standing pull request, so the wait is a decision rather than an obstacle.

## One artifact, assembled from two refs

GitHub Pages serves one deployment, so both halves are built by one run: the
newest release is checked out entire, and then `main`'s two installers are put
back on top. What deploys is the release's site carrying the current installers.

The release is checked out *entire* rather than as `pages/` alone, because
`astro.config.ts` imports the logo from `docs/assets/`, and a site assembled
from two commits is one no checkout reproduces. What crosses the boundary is
exactly the two files whose paths may never move, and the deploy diffs them
against `main` before it publishes: an overlay that silently failed would deploy
the release's copies and look identical to one that worked.

"The newest release" is the newest release that carries a site, and the
distinction earns its place. A repository whose tags have all been deleted, or
one whose site is newer than every tag, gets the same sensible answer — `main`,
said out loud in the run's own log rather than failed on.

## The release calls it rather than announcing it

`release.yml` creates the GitHub Release with `GITHUB_TOKEN`, and GitHub will
not start a workflow from an event that token made. So a `release: [published]`
trigger on the Pages workflow is not merely inelegant — it never fires, and it
fails by doing nothing at all, which is the failure this repository keeps
writing tests to avoid. The same rule is why the tag is pushed with a real token
rather than the automatic one.

So `release.yml` calls the Pages workflow as its last job, which is better than
the event would have been anyway: the call comes after the artifacts are
uploaded, so the guide is never live describing a release nobody can download
yet. A workflow called by another gets no more permission than the caller
grants, so the calling job names the three the deploy needs.

`ci.yml`'s `site` job builds `pages/` from the branch on every pull request, and
what it proves is the *next* release's site rather than the one being served.
That is the check that is wanted either way: a site that cannot build is found
on the pull request that broke it rather than by a release that cannot deploy.

## What the site must go on doing

`pages/public/` holds the installers and Astro copies it to the root of the
output verbatim. `install.sh` and `install.ps1` are served from the URLs that
are already in terminal histories, in the README and in the install guide, and
`the_installers_stay_at_the_root_of_the_site` says so on every pull request.
That constraint is older than any renderer here, is the reason the site exists
at all (ADR this-repo-assembles-a-release), and is not what this document
decides. `install-test.sh` is in `packaging/`, which is where a script that runs
the installer against a fabricated release belongs — it is no part of what
anybody downloads.

Guide pages are `/perch/accounts/`, with no `/guide/` prefix. A prefix belongs
to a book that is a guest on a site whose root is somebody else's; one renderer
owns both here, and the only URLs that may not move are at the root rather than
under it.

## What is not decided here

**A grouped sidebar.** The guide's ten pages stay flat. Grouping means
subdirectories, changed URLs and a different README table, and it is worth
deciding after somebody has looked at ten flat pages in Starlight and knows
which of them want to be sections.

**A banner saying which Perch the guide describes.** It is four lines and it
tells the truth, and telling a reader that the page they are on describes
software they do not have is a worse answer than giving them the page that
describes the software they do.

**Versioned documentation.** Starlight has none built in, one version of the
guide is what this project has needed, and a version selector is a decision to
make when somebody needs the docs for a release they are not on.

**Deploying the whole site from the release.** That is this document minus the
installers, and the installers are the reason the site is served from the root
of a deployment at all.

**Publishing when the Pages workflow itself changes.** What goes out is the
release's site carrying `main`'s installers, so a change to how those two are
assembled changes neither: the run would replace the live site with a
byte-identical copy of itself and spend a deployment of the thing that serves
the installers to do it. Applying a change to the assembly is
`workflow_dispatch`, which is the difference between a deploy somebody chose and
a deploy that happened.

**Re-styling the theme.** Two rules of Perch's own are carried: the accent pair,
and transcripts that wrap rather than scroll, because Perch prints sentences and
a sentence cut off at the right edge of a box is one nobody reads. A font stack,
a rule under every `h2`, heading weights — corrections written for another
renderer's defaults — are not, because re-applying them to a theme chosen for
its defaults is how a site arrives back where it started.
