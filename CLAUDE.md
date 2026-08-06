## Nobody is using this yet

Perch has no installed base — not the author, not anyone else. Nothing on any
machine has to keep working.

So breaking changes are free, and are the preferred answer. Do not write
migration code, compatibility shims, deprecation periods, format upgraders, or
"a registry an older Perch wrote" fallbacks. Change the format, change the
paths, change the flags, and update the tests and docs to match. If a change
would need a migration to be safe for users, it does not — there are none.

Forward-looking guards that cost nothing are still worth it: refusing a registry
written by a *newer* Perch, for instance, is about the future rather than the
past. Reading what an older Perch wrote is not.

## Agent skills

### Issue tracker

Issues and PRDs live as GitHub issues on `mschieller/perch`, managed via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
