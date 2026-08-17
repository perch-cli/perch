# Releasing Perch

A release is a tag, and a tag is a version somebody reviewed and merged. Nothing
else starts one.

## What happens

1. Every push to `main` runs `release-plz.yml`, which keeps a standing
   `chore: release vX.Y.Z` pull request open. Its title is the version the
   merged commits add up to; its diff is the version bump and the changelog
   entry. It updates as work merges.
2. Merging it makes release-plz tag `vX.Y.Z`. That is release-plz's last act.
3. The tag starts `release.yml`, which checks the tag agrees with `Cargo.toml`
   and that CI passed on that exact commit, builds five targets, writes
   `SHA256SUMS`, attests the archives, and publishes the GitHub Release.
4. The `npm` job then waits for someone to approve the `release` environment
   before it publishes the six npm packages.

The version comes from the commits, so the commits have to say what they did.
Pull request titles are Conventional Commits and CI refuses the ones that are
not — see `pr-title.yml`.

Only the title. This repository squash-merges, so the one commit that reaches
`main` has the pull request title as its subject and the branch's commit
messages as its body, and release-plz takes the type from the subject alone:
a `chore:` title over a body full of `* feat:` bullets is a patch release. The
one thing the body can still do is escalate — a `BREAKING CHANGE:` footer
anywhere in it is honored wherever it came from, which is worth knowing when
a branch commit carries one nobody put in the title.

The previous version comes from the newest `v*` tag rather than from crates.io
(`git_only` in `release-plz.toml`), which is what makes the whole thing work for
a project that is never published — see the comment there for what happens
without it.

## One-time setup

None of this is automatic, and until it is done the workflows fail rather than
misbehave quietly.

### `RELEASE_PLZ_TOKEN`

A repository secret holding a **fine-grained personal access token scoped to
this repository only**, with:

- **Contents**: read and write — to push the release branch and the tag
- **Pull requests**: read and write — to open the release pull request

It cannot be `GITHUB_TOKEN`. GitHub deliberately refuses to start a workflow
from a push its own token made, so a tag pushed with `GITHUB_TOKEN` would sit on
`main` with nothing watching it, and a release pull request opened with it would
never run CI — which `release.yml` insists on having seen go green.

Set the expiry somewhere you will notice. When it lapses, `release-plz.yml`
fails on the next push to `main`.

### The `release` environment

A GitHub Actions environment named `release`, with **required reviewers** set to
you. It gates the `npm` job and nothing else: the GitHub Release publishes on
its own, and putting Perch on a public registry waits for a person.

### npm

The packages are `perch-cli` and five platform packages under the `@perch-cli`
scope. The scope needs an npm **organization** named `perch-cli`, which is free
for public packages.

Publishing uses **trusted publishing** — the registry checks this workflow's
OIDC identity, so there is no npm token in this repository for anyone to steal.
That creates one ordering problem: a trusted publisher is configured *on a
package*, and a package that has never been published does not exist to
configure. So the first publish of each of the six is manual:

```sh
node npm/build.mjs 0.1.0 <binaries> npm-dist   # or take them from a built release
npm publish ./npm-dist/@perch-cli-darwin-arm64 --access public
# ... the other four, then:
npm publish ./npm-dist/perch-cli --access public
```

The `./` is not decoration. npm reads a bare `npm-dist/perch-cli` as GitHub
shorthand for a repository of that name and tries to clone it over ssh; the
leading `./` is what makes it a path. No `--provenance` on these, either — that
is signed against the workflow's OIDC identity and a laptop does not have one.

Then, on npmjs.com, for each of the six packages: **Settings → Trusted
publisher**, pointing at `perch-cli/perch` and the workflow `release.yml`.
Every release after that publishes itself.

Check what `latest` points at once the first publish is done:

```sh
npm dist-tag ls perch-cli
```

It should be the version you just published. `release.yml` asserts the same
thing after every publish, because `npm publish` succeeding and `latest` moving
are two different claims and only the second is what anybody installs.

Do not try to remove `latest`. npm attaches it to a package's first version
whatever `--tag` said and then refuses to delete it — the request comes back
`400` after authenticating successfully, and no public package on the registry
is without one. ADR 0031 covers what that cost.

### The Homebrew tap

A tap is a repository, and Homebrew hardcodes what it is called: `brew tap
perch-cli/perch` looks for **`perch-cli/homebrew-perch`**. The prefix is
mandatory and users never type it. Create it public, with a `Formula/`
directory and `main` as the default branch — `release.yml` writes
`Formula/perch.rb` into it on every release and nothing else.

Then a repository secret `HOMEBREW_TAP_TOKEN`: a **fine-grained PAT scoped to
`perch-cli/homebrew-perch` only**, with **Contents: read and write**.
`GITHUB_TOKEN` is scoped to this repository and cannot reach another one, so
this is the one place a stored credential is unavoidable.

It is not behind the `release` environment. A tap is opt-in, nobody adds one by
accident, and the formula can only point at assets whose checksums this release
published — the blast radius is a formula file, not a public registry.

Installing, once it exists:

```sh
brew tap perch-cli/perch
brew install perch
```

## Channels, and which of them are opt-in

Perch is pre-1.0. Where a Channel has a way of saying so, it says so; where it
does not, the version number does the work:

| Channel | During 0.x | At 1.0 |
| ------- | ---------- | ------ |
| GitHub Releases | published normally — it genuinely is the latest release, and marking it a prerelease would empty the `releases/latest` endpoint the installers ask | unchanged |
| npm | published normally, `latest` following the newest release — npm does not permit a package without a `latest` (ADR 0031) | unchanged |
| Homebrew | a tap, which nobody adds by accident | consider homebrew-core |

Only one of the three is actually opt-in, which is worth being clear-eyed about:
the one-liner on GitHub Pages hands a pre-1.0 binary to anyone who pastes it,
and always did.

## Cutting 1.0

The floor is the weekly scheduled CI run green on all three platforms for four
consecutive weeks — that is the check that catches Claude Code drift, which is
the failure that cannot be tested for in advance. Beyond the floor it is a
judgment call. Worth revisiting at the same time: Apple notarization, which
0.x deliberately skips.

## The site, and the installers on it

<https://perch-cli.github.io/perch/> is built by `pages.yml` on every push to
`main` that touches what it is made of, out of two sources (ADR 0035):
`packaging/pages/` is copied to the root, and `docs/guide/` is rendered by
mdBook into `/guide/`. Nothing on the site is written twice — the guide the
site serves is the markdown GitHub shows.

The installers live on `main` rather than inside a release on purpose: the URL
somebody pastes into a terminal should not carry a version, and an installer
that has to be released to be fixed stays broken until the next release. They
sit at the root of the deployment for the same reason — `install.sh` and
`install.ps1` have already been pasted into terminals at those paths, so
whatever else the site grows, those two do not move.

Enable it once, under **Settings → Pages → Source: GitHub Actions**.

mdBook is pinned by version and by hash in `.github/actions/mdbook`, which both
`pages.yml` and CI use, so the renderer a pull request is checked with is the
one the site is built with. To render it locally:

```sh
mdbook serve docs --open
```

Both installers verify the archive against the release's `SHA256SUMS`, and
then, only if `gh` is installed *and logged in*, against the signed build
provenance — and that second check is binding. The download has already
succeeded by then, so a provenance failure is saying something about the file
rather than about the network, and an installer that shrugs at that is doing
the check for decoration.

`PERCH_API_BASE` and `PERCH_DOWNLOAD_BASE` exist so the scripts can be run
against a fabricated release served locally. Nothing in normal use sets them.
