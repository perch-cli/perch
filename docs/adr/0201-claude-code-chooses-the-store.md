# Claude Code chooses the Store

A Credential lives exactly where the installed Claude Code would keep one, and
nowhere else. Perch gives each Account a directory Claude Code would treat as
its whole configuration — a Profile — and the Credential Store follows from that
directory rather than from anything Perch records.

Where that store is, is the platform's answer. On macOS it is the operating
system's keychain, under a service name Claude Code derives from the config
directory: `Claude Code-credentials-<sha256(CLAUDE_CONFIG_DIR)[0:8]>`, or the
bare `Claude Code-credentials` when `CLAUDE_CONFIG_DIR` is unset. Every config
directory therefore has a private credential namespace. On Windows and Linux it
is plaintext JSON at `<config dir>/.credentials.json`, in the same
`claudeAiOauth` shape; the 2.1.222 builds of both name that backend `plaintext`
in their own source, the Windows one gates `find-generic-password` behind a
Darwin branch and does not contain the string `Claude Code-credentials` at all,
and the Linux one writes the file at mode 0600 and then `chmod`s it to the same.
There is no Secret Service primary in either.

Off macOS the service-name derivation is simply irrelevant: the Credential sits
inside the Profile directory, so a Profile's isolation comes from the directory
itself rather than from a namespace derived out of it. Either way the rule is
one rule — Perch reads what Claude Code wrote, and writes what Claude Code will
read — and that is what makes Adoption possible at all
(ADR a-login-perch-does-not-need).

Because each Profile is a real config directory, Perch can also point a single
client at one by setting `CLAUDE_CONFIG_DIR` for that process alone. That is the
Run path (ADR a-run-is-one-shot), and it is what lets two Accounts work
concurrently in different terminals.

## The platform says where Perch writes, and evidence says what it believes

Claude Code is not a single-store reader, so Perch is not either. Its store is a
composite with a primary and a fallback, reporting `primary_and_fallback_failed`
only when both refuse, and deleting from both.

The two halves are decided by different things. The **platform** chooses where
Perch *writes*, because a Profile that `perch add` created a moment ago holds no
evidence to read. **Evidence** chooses what Perch *believes*, because a store
that already exists can be looked at. A mac whose keychain is locked has a
working Claude Code backed by a plaintext file, and a Perch that read only the
keychain would report no login about a machine that is logged in. This is
ADR an-assumption-is-probed's rule one level down: probed, not assumed.

A locked keychain is therefore a failure when it is the whole story and a detail
when it is not. A store that is there and will not say what it holds is never
promoted to "no Credential", which is terminal — it Quarantines an Account
(ADR a-switch-is-written-down-first).

## The keychain is reached through /usr/bin/security

macOS anchors a keychain item's access control to the binary that created it.
The in-process approach would make each Perch build a different creator, so a
Homebrew upgrade, a `cargo install` rebuild or a fresh binary from the install
script could turn a silent read into a modal prompt or a denial — a failure mode
that surfaces only after release, for any tool whose installer replaces the
binary owning its keychain items.

Perch drives `/usr/bin/security`, as Claude Code does. That binary never
changes, so creator and reader agree across upgrades of both tools, which is
also what lets Perch read the item Claude Code wrote.

Four constraints come with that CLI. Writes hex-encode with `-X` and pipe
through `-i` so secrets never reach `argv`. Its stdin buffer is 4096 bytes and
overflow truncates mid-argument, silently corrupting the entry — Claude Code
issue #30337 — so writes near the limit fall back to `argv`, and every write is
read back before it is trusted. Exit code 44 means "not found"; every other
non-zero exit means locked, denied or unavailable and is reported differently,
since conflating them reads as an Account having vanished. And `-w` returns hex
for non-printable data, so the wrapper is safe for ASCII JSON Credentials only.

## One store holds a Profile's Credential at a time

A successful write to the primary removes any copy in the fallback, and a write
that had to fall back removes the copy in the primary. A Credential left in the
other store is several Rotations behind within minutes, so leaving it there arms
the composite reader to hand a retired refresh token to Claude Code the next
time the keychain locks — the silent poisoning Capture exists to prevent,
arriving by the back door.

The two directions are not symmetric. A copy that loses to what was just written
is untidy and worth a remark. A copy that *beats* it means every later read
hands back the Credential the write replaced, so that one fails.

## What was not chosen

**Protecting a parked Credential with the best each platform offers** — the
keychain, DPAPI, the Secret Service. Its benefit is largely illusory against the
threat that matters: DPAPI decrypts for any process running as the user, and the
Secret Service unlocks with the login session, so malware running as the user
either has the Credential already or can wait one Switch for it. It costs two
more backends, a hard dependency on a Secret Service that headless Ubuntu and
WSL frequently lack, and — decisively — the property that Perch reads what
Claude Code wrote and writes what Claude Code will read.

**Reading `CLAUDE_SECURESTORAGE_CONFIG_DIR`.** It overrides the config directory
for the credential store alone, in both the plaintext path and the keychain
service name, so a machine that set it would collapse every Profile onto one
store. A Profile is isolated by `CLAUDE_CONFIG_DIR` or the design does not hold
at all, and encoding a second config path would hide that rather than fix it.
`CLAUDE_CODE_HOST_CREDS_FILE` is the neighboring variable and not a store Perch
writes: it names a credentials file supplied by whatever is hosting Claude Code,
which ignores it with group- or other-readable mode or the wrong owner — a path
into Claude Code rather than one out of it.

## Consequences

On macOS Perch deletes `~/.claude/.credentials.json`, a file Claude Code owns
and Perch did not create. The file is stale by definition the moment the primary
write succeeds, it is a live secret in plaintext, and Perch only touches it
inside the three locks a Switch already takes — but it is Perch reaching outside
its own house.

Off macOS Perch multiplies the number of plaintext secrets on disk. Claude Code
alone keeps one Credential in one file; somebody holding four Accounts through
Perch has four, under `~/.config/perch/profiles/`. No single Credential is less
protected, but a leaked backup of `$HOME` yields more, and that is the strongest
argument for the rejected alternative. A Credential *leaving* the machine is
covered differently, by ADR the-holdings-go-out-sealed's required passphrase.

Files holding a Credential are created 0600 and their directories 0700, which is
what Claude Code does wherever it writes one — and so is any other file
Perch is the first to create inside a Profile, `.claude.json` among them, since
an MCP server entry routinely carries an API key in its `env` block. The mode is
set at creation rather than by a `chmod` afterwards, because `write_atomically`
writes beside the target first and a create-then-chmod leaves a window in which
a refresh token is world-readable. The directory half has a trap the file half
does not: `mkdir -p` leaves an existing directory's mode alone, so whatever
creates a Profile has to be the thing that makes it private. A credential file
found looser than 0600 is tightened and mentioned once rather than refused,
because Claude Code is using that file perfectly happily and refusing would make
Perch broken while the thing it wraps works. Windows has no mode and relies on
the profile ACL, so a `$PERCH_HOME` on a shared or removable drive gets that
drive's permissions and Perch will not notice.

Because a Credential Store is derived rather than recorded, the Registry names
neither the keychain service nor the keychain account, and by the same argument
not the Profile's directory either — that is `perch_home/profiles/slug(email)`.
`Profile` collapses into values the Registry already keys on, and two statements
of one fact cannot disagree. The directory is therefore the only thing that can
still name a Profile's Credential Store, so anything discarding a Profile takes
the Credential first and the directory last: a keychain locked mid-`perch add`
would otherwise leave a live refresh token under the hash of a path that no
longer exists, for an Account nobody believes was ever added.

Naming the directory after the address makes the slug load-bearing twice over.
Two addresses that flatten to one slug are one Profile, and plus-addressing on a
single inbox is exactly how somebody comes to hold several Accounts — so a login
landing in a Profile Perch already holds one in is refused rather than stored
over. And an address with no alphanumeric character in it slugs to nothing,
which would make that Account's Profile *be* `profiles/`, one removal away from
taking every Credential Perch holds with it; that is refused at the one place
every store is derived from rather than trusted to whatever wrote the address.

What deriving costs is that a change to Claude Code's derivation would send
Perch looking in an empty namespace instead of at a remembered one; the answer
to drift is to refuse loudly rather than to guess, and `short_hash` is pinned by
test so the derivation cannot move by accident. The shape of the Registry itself
is ADR the-holdings-outlive-a-perch's to move.

Switching writes a Credential a running Claude Code may hold, so it cooperates
with Claude Code's own OAuth refresh locks rather than inventing a scheme of its
own.

A store asked to give a Credential up says whether it held one, because the
caller is sometimes about to tell somebody which happened. The keychain item's
account name is `$USER`, which is not the same today as it was under `sudo -u`,
after a login rename, or in a launchd context — and there a delete is a no-op
that reads as success while the keychain goes on holding a working Credential
for an Account `perch remove` has just said it destroyed.

The belief that the plaintext store honors `CLAUDE_CONFIG_DIR` is the single
assumption the whole non-macOS design rests on, and it earns the loudest
contract test. That test plants a Credential that is not one where Perch would
write it and asks the installed Claude Code who it is, having first watched it
answer "nobody" for the same directory — so what is asserted is Claude Code's
behavior rather than Perch's own derivation restated. A distribution's own build
may still bring a third backend; the composite reader accommodates that without
redesign, and what changes is which store the platform names as primary.
