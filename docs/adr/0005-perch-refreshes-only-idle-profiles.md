# Perch refreshes tokens only for profiles with no client running

Reading an account's remaining quota needs a valid access token for it, so
ranking auto-switch candidates means refreshing tokens for accounts nobody is
using. But Anthropic rotates refresh tokens — a refresh may return a new one,
invalidating the token family — so refreshing a credential that a running Claude
Code still holds in memory logs that session out, silently, mid-task.

Perch therefore refreshes only profiles with no client running, and writes the
rotated credential back under the same lock Claude Code takes. Nothing is lost:
auto-switch candidates are idle by definition, and an account actually in use
has a fresh access token already, so its usage can be read without refreshing
anything.

## Consequences

The liveness check this requires is the same precondition ADR 0003 already
placed on writing `projects[<cwd>]`. Both features depend on answering "is a
client running for this profile?" correctly, which makes that check worth
building carefully and exactly once.
