# The site is rendered by one thing, and a stylesheet was never the answer to a theme

ADR 0035 got the diagnosis right and the treatment wrong. It named two costs of
rendering the guide with mdBook and paid them both openly, and the second one was
"a theme that looks like every other Rust project's docs". The answer it wrote
for that cost was `docs/theme/perch.css`: sixty-five lines carrying the landing
page's type and its one accent color, small on purpose, leaving the navigation
and the search alone because those were the reason mdBook was there.

The answer failed, and it failed for a reason worth writing down rather than
retrying. The problem was never that one of the site's two artifacts was plain.
It was that there were two.

## What the seam cost

The landing page was hand-written HTML with its own inline `<style>` block. The
guide was mdBook plus a stylesheet correcting mdBook. They shared an accent color
and nothing else — not the type scale, not the navigation, not the header, not
the search. Moving from one to the other was leaving one site and arriving at
another, and no quantity of CSS on the second closes that, because mdBook does
not want to be the front page. ADR 0035 says so itself, and carved the landing
page out for exactly that reason. The carve-out is the defect. It was written
down as a concession to one.

Four things were also plainly broken, and they are recorded because each was
found by looking rather than assumed:

The chapter list shipped in an `<iframe src="toc.html">` rather than in the page
that needed it. There was no per-page table of contents, so five of the ten guide
pages opened with a hand-typed list of their own headings — nineteen links, kept
in step by hand, standing in for navigation a renderer should generate. The
previous and next links sat at the end of the document. And the theme picker
offered a reader Rust, Coal, Navy and Ayu, which is the cost of ADR 0035's second
paragraph stated as a menu.

The nineteen hand-written links are the sharpest of the four. A renderer that
makes the author write navigation by hand is not one whose navigation was the
reason to keep it.

## What renders it now

Astro and Starlight, as one project in `pages/`, owning the splash landing page
and the ten guide pages together. Search is Pagefind and stays static. The
sidebar is in the page. Each page gets a table of contents on the right, so the
nineteen hand-written links are deleted — the headings stay, which is what every
cross-page anchor was pointing at.

What is carried forward from ADR 0035 entire, and is the reason this change is
small: **the guide is written once and the site renders it.** Also unchanged: the
site deploys from `main`, because everything here is still fixed by a merge, and
the installers are still served from the root of the deployment.

> **Amended by ADR 0063.** The clause about deploying from `main` is void, and
> carrying it forward from ADR 0035 without reopening it was the mistake: it was
> true of the installers and had stopped being true of the guide, which was
> describing a Perch nobody could install. The guide is published by a release
> now, the installers still by a merge, and one run assembles both. Everything
> else here stands, the installers at the root of the deployment included — that
> is the half of the sentence that was right.

## Where the guide lives

`pages/src/content/docs/`, which is a move, and the move is the point of this
section rather than an aside.

Starlight's `docsLoader()` reads `src/content/docs/` and takes no directory
option — only `generateId()`. Defining the collection by hand with Astro's
generic glob loader and pointing it at `docs/guide/` is plausible and is not in
Starlight's documentation, and paying an unverified mechanism to keep a directory
name is the wrong trade when the whole build rests on it.

What actually mattered about `docs/guide/` was that it held one copy of markdown
a person or an agent could read without a browser. That is a property of the
file, not of the path, and it survived the move intact. `docs/adr/`,
`docs/agents/` and `CONTEXT.md` did not move.

## The title, and the heading that came out

Starlight requires `title` in frontmatter on every page and renders it as the
page's `h1`, so the `# Heading` came out of all ten files. On GitHub a guide page
now opens with a small frontmatter table and goes straight into prose.

That is a real loss and is recorded rather than glossed over. It is accepted on
the ground ADR 0035 actually stood on: what that document promised was one copy
of the content, not the absence of metadata, and a title rendered as a table cell
is still a title in the file. What would have broken the promise is `.mdx`, which
would have made ten documents into ten sources. The guide is `.md`. Exactly one
file is `.mdx` — the splash page, which is the one place a component earns
anything.

## Three lists became one

ADR 0035's closing section was an apology. The pages were written once; the
*list* of them was written three times, in `SUMMARY.md`, in the table in
`docs/guide/README.md`, and in the README — three audiences, none derivable from
the others, kept in step by `tests/publication.rs` because keeping them in step
by hand is what that document said not to do.

Two of the three are gone. `SUMMARY.md` is an autogenerated sidebar, which cannot
omit a page. `docs/guide/README.md`'s table of nine destinations is the splash
page's card grid, so the guide's index and the site's front page are the same
artifact instead of two that agree. What remains is the README's command table,
and it remains for ADR 0035's reason unchanged: `npm/build.mjs` copies the README
into the package, so it is npm's landing page too, and an offline clone would
lose its getting-started if that table became links to a website.

The assertions followed. `the_summary_lists_only_pages_that_exist` is deleted,
because the thing it guarded no longer exists. The other seven live, retargeted:
what the site promises is not a property of which generator rendered it, and the
file that asserts it is Rust either way.

## The pin, and what replaced it

`.github/actions/mdbook` fetched one version, verified its `sha256`, and was used
by both CI and the Pages workflow — one number rather than two that have to
agree, so a pull request was checked against the renderer the site is built with.

That property survives in a different form. `pnpm-lock.yaml` carries integrity
hashes for the whole tree rather than for one tarball, and `pnpm install
--frozen-lockfile` refuses to build anything else. The package manager is pnpm,
and its own version is not left to whatever the runner happens to ship:
`packageManager` in `package.json` names it, which is the same discipline the
`sha256` pin was — one place says which bytes, and CI and the deploy read that
one place. The `guide` job became `site`, still runs on every pull request, and
is still named in the `success` gate.

The cost is the largest thing this document buys and is not disguised: a
`node_modules` tree in a Rust repository, a weekly dependabot stream, and a
package manager in the deploy path that publishes the installers. It is fenced
rather than absorbed. `pages/` holds the `package.json`, the lockfile and its own
`.gitignore`, and nothing at the root of the repository suggests Perch is a
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
that is wrong — a sidebar that silently autogenerates from nothing, a `base` that
does not match where Pages serves from, a loader pointed at a directory that is
empty. Perch's own build has a compiler standing between a typo and a green run,
and the site should not be the single directory that does without one.

`npm/build.mjs` is not part of the site and does not change. It stays plain
JavaScript, where it has no dependencies and nothing to check.

## Versions are taken at the latest

Every dependency is taken at the highest version that works, and this document
was written against Astro 7.2.3, Starlight 0.41.7, TypeScript 7.0.2 and pnpm
11.22.0. Astro 7 wants Node 22.12 or newer, which the workflows already clear —
`release.yml` is on 24.

Written down because a docs site is exactly the kind of thing that gets stood up
once against whatever was current that afternoon and then pinned there by
nobody's decision. There is no installed base and no compatibility to keep, so an
older version is never the safer one; it is only the older one.

## What the site must go on doing

`packaging/pages/` is gone and `pages/public/` took the installers, which Astro
copies to the root of the output verbatim. `install.sh` and `install.ps1` are
served from the URLs that are already in terminal histories, in the README and in
the install guide, and `the_installers_stay_at_the_root_of_the_site` still says
so on every pull request. That constraint is older than either renderer, is the
reason the site exists at all (ADR 0028, ADR 0031), and is not what this document
reopened. `install-test.sh` went back to `packaging/`, which is where a script
that runs the installer against a fabricated release belongs — it is no part of
what anybody downloads.

`/guide/` is gone from the URLs. Guide pages are `/perch/accounts/` rather than
`/perch/guide/accounts.html`. The prefix existed because the book was a guest on
a site whose root belonged to somebody else; one renderer owns both now, and a
leftover prefix is length without meaning. Nothing was at risk in dropping it:
the only URLs that must not move are at the root, not under it.

## What is not decided here

The guide's ten pages stay flat. A grouped sidebar is what the sites that
prompted this change have, and grouping means subdirectories, changed URLs and a
different README table — a second variable in a change that already has enough.
It is a separate decision, worth making after somebody has looked at ten flat
pages in Starlight and knows which of them want to be sections.

`docs/theme/perch.css` is not ported. Two of its rules are: the accent pair, and
the transcripts that wrap rather than scroll, because Perch prints sentences and
a sentence cut off at the right edge of a box is one nobody reads. The rest of
that file — the font stack, the rule under every `h2`, the heading weights — was
written to correct mdBook's defaults, and re-applying corrections to a theme
chosen for its defaults is how a site arrives back where it started.
