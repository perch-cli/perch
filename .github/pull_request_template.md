## What changes

<!-- One paragraph. What a person can do now that they could not, or what stops
happening. If it is a fix, say what was wrong rather than what you edited. -->

## Why

<!-- The problem, not the patch. Closes #123 if there is an issue. -->

## How you know it works

<!-- The test that fails without this, or the commands you ran and what they
printed. -->

## Checklist

- [ ] The title is a Conventional Commit: `feat:`, `fix:`, `docs:`, `refactor:`, `perf:`, `test:`, `build:`, `ci:`, `chore:`, `revert:`, with `!` for a break. It is the squashed commit subject, and release-plz reads it to pick the next version.
- [ ] `CHANGELOG.md` has an entry under `## [Unreleased]`, marked `[**breaking**]` if a command, a flag, an exit code or a `--json` shape moved. Nothing else warns a script.
- [ ] If the shape of the registry or of an Export moved, its `version` moved with it, and the change lands as a migration or as a refusal naming the version that wrote the file.
- [ ] `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --locked`.
- [ ] `typos`, `typos - < Cargo.toml`, `typos - < typos.toml`. Perch writes American English.
- [ ] Comments say one of the four things `CLAUDE.md` lists and stay inside the caps. A decision that is new to the repository has an ADR under `docs/adr/`, cited once per file as `ADR <slug>`.
