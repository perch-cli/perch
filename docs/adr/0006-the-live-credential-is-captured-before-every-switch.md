# The live credential is captured back into its profile before every switch

A Switch leaves two copies of one credential: the one in the account's profile
and the live one on the Default Profile. Only the live copy is used, and Claude
Code rotates it whenever it likes — so by the time you switch away, the copy in
the outgoing account's profile can be several rotations behind, and a retired
refresh token is dead.

So a Switch is three steps, not one. Perch captures the live credential back
into the outgoing account's profile, writes the incoming account's credential
to the live store, and patches `oauthAccount` to match. Skipping the capture
means every switch quietly poisons the account being left behind, with the
damage only surfacing when you switch back to it.

All three steps run under Claude Code's own OAuth refresh locks, which is what
stops a refresh landing between the capture and the write.

## Consequences

Capture also settles the coherence question without any mtime or hash
comparison: the live copy is authoritative while it is live, and it is written
back at the one moment Perch controls. A profile's stored credential is
understood to be stale whenever that account is the active one.

If a credential is rotated and lost before it can be captured — a crash between
the two writes, or a machine that dies mid-refresh — that account needs a fresh
login. Quarantining an account in that state is therefore not a feature to defer
past v1; it is the terminal state of this design and has to exist from the start.
