# A Credential lives wherever Claude Code puts it

ADR 0001 and ADR 0008 were written against a macOS-only Perch, and both assume
a Credential is held in the operating system's keychain. That assumption is
macOS's, not Claude Code's. On Windows — and, on present evidence, on Linux —
Claude Code writes the Credential as plaintext JSON to
`<config dir>/.credentials.json`, in the same `claudeAiOauth` shape. The 2.1.222
Windows build contains the `find-generic-password` call gated behind the Darwin
branch and does not contain the string `Claude Code-credentials` at all; its
non-Darwin store is named, in its own source, `plaintext`.

Perch therefore stores a Credential exactly where the installed Claude Code
would store it, and nowhere else. Off macOS the service-name derivation of
ADR 0001 — `sha256(CLAUDE_CONFIG_DIR)[0:8]` — is simply irrelevant: the
Credential lives inside the Profile directory, so a Profile's isolation comes
from the directory itself rather than from a namespace derived out of it. The
conclusion of ADR 0001 survives unchanged; only its mechanism is macOS's.

The rejected alternative was to protect a parked Credential with the best each
platform offers — the keychain, DPAPI, the Secret Service. It was rejected
because its benefit is largely illusory against the threat that matters. DPAPI
decrypts for any process running as the user, and the Secret Service unlocks
with the login session, so malware running as the user either has the Credential
already or can wait one Switch for it. Meanwhile it costs two more backends, a
hard dependency on a Secret Service that headless Ubuntu and WSL frequently lack,
and — decisively — the property that makes adoption work at all: that Perch reads
what Claude Code wrote, and writes what Claude Code will read.

Claude Code is not a single-store reader, so Perch is not either. Its store is a
composite with a primary and a fallback, reporting `primary_and_fallback_failed`
only when both refuse and deleting from both. That means a mac whose keychain is
locked has a working Claude Code backed by a plaintext file — and a Perch that
read only the keychain would report exit 11, or no login, about a machine that is
logged in. So the platform chooses where Perch *writes*, because a Profile that
`perch add` has just created holds no evidence to read; and evidence chooses what
Perch *believes*, because an existing store can be looked at. This is ADR 0007's
rule applied one level down: probed, not assumed.

A successful write to the primary store removes any copy in the fallback. One
store holds a Profile's Credential at a time, and the reason is ADR 0006's: a
Credential left in the fallback is several Rotations behind within minutes, so
leaving it there arms the composite reader to hand a retired refresh token to
Claude Code the next time the keychain locks — the silent poisoning that Capture
exists to prevent, arriving by the back door.

## Consequences

On macOS this means Perch deletes `~/.claude/.credentials.json`, a file Claude
Code owns and Perch did not create. It is defensible — the file is stale by
definition the moment the primary write succeeds, it is a live secret in
plaintext, and Perch only touches it inside the three locks a Switch already
takes — but it is Perch reaching outside its own house, and it was decided
deliberately rather than discovered.

Off macOS, Perch multiplies the number of plaintext secrets on disk. Claude Code
alone keeps one Credential in one file; a person holding four Accounts through
Perch has four, under `~/.config/perch/profiles/`. Perch does not lower the protection
of any single Credential, but it does widen what a leaked backup of `$HOME`
yields, and that is the strongest argument for the rejected alternative. The
portable case — a Credential leaving the machine — is covered separately and
differently by ADR 0014's required passphrase.

Files holding a Credential are created 0600 and their directories 0700, matching
what Claude Code does at 39 sites in its own binary. The mode is set at creation
rather than by a `chmod` afterwards: `write_atomically` writes beside the target
first, and a create-then-chmod would leave a window in which a refresh token is
world-readable. The directory half has a trap the file half does not: `mkdir -p`
leaves an existing directory's mode alone, so a Profile made by an ordinary
`create_dir_all` and only then written into privately stays 0755, and every test
that asks after the fact still sees a 0600 file inside it. Whatever creates a
Profile has to be the thing that makes it private. A credential file found looser
than 0600 is tightened and
mentioned once rather than refused, because Claude Code is using that file
perfectly happily and refusing would make Perch broken while the thing it wraps
works. Windows has no mode and relies on the profile ACL; a `$PERCH_HOME` pointed
at a shared or removable drive therefore gets that drive's permissions and Perch
will not notice.

Because a Credential Store is derived rather than recorded, `registry.json` stops
carrying `keychain_service` and `keychain_account` — and, by the same argument,
`dir`, which is `perch_home/profiles/slug(email)`. `Profile` collapses into
values the registry already keys on. The fields are simply dropped: nobody is
running Perch, so there is no registry an older one wrote to migrate and the
format stays at version 1. The cost of deriving rather than recording is that
a future change to Claude Code's derivation would send Perch looking in an empty
namespace instead of at a remembered one; ADR 0007's answer to drift is to refuse
loudly rather than to guess, and `short_hash` is pinned by test so the derivation
cannot move by accident.

Linux was inference when this was written and is now observation: the 2.1.222
Linux build was read the way the Windows one had been. Its store resolves to
`join(<config dir>, ".credentials.json")`, it is written with mode `384` — 0600
— and then `chmod`ed to the same, and the backend is named `plaintext` in its own
source, as on Windows. There is no Secret Service primary in it. A distribution's
own build, or a later one, may still bring a third backend; the composite reader
accommodates that without redesign, and what would change is which store the
platform names as primary.

The belief that the plaintext store honours `CLAUDE_CONFIG_DIR` remains the
single assumption the whole non-macOS design rests on, and it earns the loudest
contract test. That test plants a Credential that is not one where Perch would
write it and asks the installed Claude Code who it is, having first watched it
answer "nobody" for the same directory — so what is asserted is Claude Code's
behaviour rather than Perch's own derivation restated.

Reading that build answered two neighbouring questions and raised one.
`CLAUDE_CODE_HOST_CREDS_FILE` names a credentials file supplied by whatever is
hosting Claude Code, which it ignores "with group/other-readable mode or wrong
owner" — a path into Claude Code rather than one out of it, and not a store Perch
writes. `CLAUDE_SECURESTORAGE_CONFIG_DIR` is the one that would hurt: it
overrides the config directory for the credential store alone, in both the
plaintext path and the keychain service name, so a machine that set it would
collapse every Profile onto one store. Nothing sets it, and Perch does not read
it — a Profile is isolated by `CLAUDE_CONFIG_DIR` or the design does not hold at
all, and encoding a second config path would hide that rather than fix it.
