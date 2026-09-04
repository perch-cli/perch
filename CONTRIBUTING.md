# Contributing to Perch

Perch is for one person moving between Claude logins they already hold, on their
own machine. It creates no accounts and authenticates nobody. A change that
makes it a way to share one subscription between people is out of scope, and so
is anything that needs Perch to hold an account of its own.

The project is pre-1.0 and maintained by one person. Expect a reply within a
week; expect a fix to take longer than that.

## Where to start

- **A bug.** Open an issue with the bug report form. It asks you to run
  `perch probe` and paste it, which gathers the version, the platform, the
  installed Claude Code and what Perch holds, with names and paths replaced by
  placeholders.
- **An idea.** [Discussions](https://github.com/perch-cli/perch/discussions/categories/ideas),
  not an issue. Say what you were trying to get done, not which flag you want.
- **A security problem.** Private vulnerability reporting, never a public
  issue. `SECURITY.md` says what is in scope and what is not.
- **A pull request.** A typo fix or an obvious bug fix can arrive as one. For
  anything larger, open an issue first, so the design argument happens before
  you have written it.

## Building and testing

The toolchain is pinned in `rust-toolchain.toml`, and rustup installs it on
first use. Nothing else is needed to build.

```sh
cargo build
cargo test --locked
```

`--locked` is not optional in CI: a resolver free to update `Cargo.lock` tests a
dependency set nobody has committed.

Most of the suite drives the real command code against a fake machine, so it
needs no Claude Code, no keychain and no network. One suite does need your
machine, and is held back behind a feature for that reason:

```sh
cargo test --locked --features your-machine --test your_machine -- --nocapture
```

It touches state you own and did not offer: the real keychain, the real
`claude` on your `PATH`. Read the case for gating it before you run it
(ADR a-suite-is-named-and-gated), or leave it to CI, which runs it on a machine
that exists to be written to.

## What CI checks

Run these before you push and the pull request goes green on the first try.
`typos` is the one tool of them rustup does not bring:
`cargo install typos-cli`.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --locked --bin perch          # the shipped binary carries no fake
typos && typos - < Cargo.toml && typos - < typos.toml
```

CI also builds against the minimum supported Rust version, audits the
dependency set, checks that no dependency has stopped being imported, and
exercises the installers and the packages on Linux and Windows. The Rust jobs
run on macOS, Linux and Windows.

## Four rules that decide a review

`AGENTS.md` states these in full and is the copy that binds. They are named here
because they are what a first pull request is most often sent back over, and
each links to where the case is made rather than restating it.

**The Holdings survive an upgrade.** The command line moves freely, marked
`[**breaking**]` in `CHANGELOG.md`. A Profile, a Credential, the Registry naming
them and an Export carrying all three do not: when their shape changes, the
`version` changes with it, and the change lands as a migration or as a refusal
naming the version that wrote the file (ADR the-holdings-outlive-a-perch).

**Perch writes American English.** `typos.toml` is the rule and the three
commands above are the gate.

**A comment earns its place.** Four things a comment may say, three tiers with a
line cap each, and over the cap means a decision with no ADR. `tests/comment.rs`
holds the half a script can check; `docs/agents/comments.md` works through the
rest.

**A citation names a slug.** `(ADR perch-does-not-draw)`, one form in every file
type, never a path and never a link. The guide, `README.md`, `SECURITY.md` and
everything Perch says at a terminal address a reader with no checkout and carry
no citations at all.

## The words

`CONTEXT.md` is the dictionary: Account, Profile, Credential, Group, Headroom,
Quota Window, Cycling, the Watcher, the Holdings. Code, comments, commits and
output all use those words for those things. If your change needs a word that is
not in there, that is worth saying in the pull request, because it usually means
the design is new rather than the naming.

`docs/adr/` is where a decision is argued and `src/` is where it is stated.
A new decision gets a document in its band, appended at the end of that band's
numbers.

## Pull requests

If this is your first change to this repository, no check will start on its
own. GitHub holds a fork's workflow runs until a maintainer approves them, and
re-arms that hold on every push until your first pull request merges. Until then
both required checks report nothing at all, which looks like a broken repository
and is not one (ADR a-gate-lives-outside-the-tree).

The repository squash-merges, and the pull request title becomes the commit
subject that release-plz reads to pick the next version. So the title is a
Conventional Commit, and a workflow checks it on every edit:

```
feat: run a client against a Group
fix(watcher): a round that spent nothing reported as if it had
feat!: switch takes a Scope rather than a Group
```

Write the `CHANGELOG.md` entry yourself, under `## [Unreleased]`. Say what a
person can now do or what stops going wrong, in the vocabulary of `CONTEXT.md`.
Entries in that file are written for somebody deciding whether to upgrade, which
is why they are longer than a commit subject.

One change per pull request. A refactor that arrives alongside a fix makes the
fix impossible to read, and impossible to revert on its own.

## Layout

| path | what is there |
| --- | --- |
| `src/` | the library and the `perch` binary; `src/commands/` is one file per command |
| `src/host/` | everything that touches the machine, and the fake that stands in for it |
| `tests/` | one file per behavior, named for the behavior |
| `docs/adr/` | the decisions, numbered in bands that follow `CONTEXT.md` |
| `docs/agents/` | conventions for agents working in this repository |
| `pages/` | the guide, built with Astro and served from GitHub Pages |
| `packaging/` | the installers, the Homebrew formula and the npm packages |

## License

Contributions are dual-licensed under MIT and Apache-2.0, the same as Perch.
