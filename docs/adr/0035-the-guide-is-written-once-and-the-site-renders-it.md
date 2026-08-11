# The guide is written once and the site renders it

The site existed because the installers needed a versionless URL to be served
from (ADR 0028, ADR 0031). That is a real job and it went on doing it, but it
meant the front page of a program somebody might want to read about was an
install page: here are four ways to get it, and nothing at all about what it is
for. The documentation was in the repository, where you had to already be
looking.

So the site renders the guide. What that decides is where the guide *lives*, and
the answer is: exactly where it lived before. `docs/guide/` is markdown, read on
GitHub by anyone who clones or browses, and read in a browser by anyone who does
not. mdBook is pointed at that directory as its source rather than at a copy of
it, so there is no step in which the two versions could disagree and nothing to
keep in step by hand. A page is added by writing a markdown file and naming it
where the guide is indexed.

## What renders it

mdBook, which is the Rust ecosystem's default and gives navigation and full-text
search for nothing. The two costs are real and are paid: a build step in the
deploy path, and a theme that looks like every other Rust project's docs. The
second is answered by `docs/theme/perch.css`, which is small on purpose — it
carries the landing page's type and its one accent colour so that moving between
the two is not a change of voice, and leaves the navigation and the search
alone, since those are the reason mdBook is here.

The first is answered by pinning. `.github/actions/mdbook` fetches one version,
verifies its hash, and is used by both the Pages workflow and CI — so the
renderer a pull request is checked against is the one the site is built with,
and the pin is one number rather than two that have to agree.

## The landing page is not part of the book

`packaging/pages/index.html` is hand-written, and stays hand-written. It is the
page that has to say what Perch does before it asks anybody to paste a command
into a shell, and mdBook does not want to be that page. It is also where the
constraint lives: `packaging/pages/` is copied to the *root* of the deployment,
so `install.sh` and `install.ps1` are served from the URLs that are already in
terminal histories, in the README, and in the install guide. The book goes to
`/guide/`. Whatever else the site grows, those two files do not move.

The one thing that does not follow the directory is `install-test.sh`, which
runs the installer against a fabricated release from a checkout and is no part
of what anybody downloads. The workflow names the three files it publishes
rather than copying the directory.

## What holds it together

None of this fails loudly on its own. mdBook drops a page nobody listed in
`SUMMARY.md` without a word; a link to a heading that has been reworded is a 404
somebody else finds; an installer URL that moves is a command already pasted
into shells that now downloads nothing. So each of those is asserted:
`create-missing = false` and a book build in CI catch a chapter pointing at
nothing, and `tests/publishing.rs` catches the rest — a guide page missing from
`SUMMARY.md`, a relative link that leaves the guide and would 404 on the site, a
cross-page anchor that no longer names a heading, an installer URL that is not
the versionless one, and a landing page that asks for an install before it shows
what it is installing.

## Where it is not single-source, and why

The pages are written once. The *list* of them is written three times:
`SUMMARY.md`, which is what mdBook publishes from; the table in
`docs/guide/README.md`, which is what somebody browsing the repository reads;
and the README, which is what npm shows. None of the three is derivable from the
others — they are three audiences, and mdBook will not accept a generated
`SUMMARY.md` without a build step of its own, which is a second renderer to
answer for.

Three lists kept in step by hand is what this ADR says not to do. They are kept
in step by `tests/publishing.rs` instead: adding a page and forgetting one index
fails on the next run, and names which index and what it would have cost. That
is the compromise — the duplication is real, and nothing silently tolerates it.

## What is not decided here

The README stays as it is. It is also the npm package's landing page —
`npm/build.mjs` copies it in — so a command table replaced by links to a website
would be a worse page in the place npm shows it, and an offline clone would lose
its getting-started. It links to the guide sources, and the site renders the same
files: nothing is written twice either way.

Nothing here is served from a Release, and the site goes on deploying from
`main`. That is ADR 0028's reasoning unchanged and now covers the guide too: a
sentence that is wrong is fixed by a merge rather than by cutting a version.
