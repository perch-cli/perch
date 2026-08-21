# A removal lands first

`perch remove` is the only command that destroys something. It forgets the
Account, deletes the Credential Perch holds for it and takes its Profile with
it, so the Account stops being listed, stops being a Cycle candidate, and frees
the Alias it answered to.

Removing an Account nobody is on takes away something nothing is using, and asks
nothing. Removing the **active** Account is different in kind: the Credential in
the Default Profile belongs to the Account being given up, so a removal that only
deleted rows would leave the machine running as an Account Perch no longer holds
— usable until the next Switch, and then gone with no way back short of a login.

So Perch names the Account it will leave active, and lands on it first. The
successor is an Account in the same Group where there is one, because a Group is
the user's own statement that these Accounts are interchangeable
(ADR a-group-is-a-declaration); otherwise it is any Account Perch holds, which
is a forced choice rather than a Cycle leaving its scope — it is named in front
of the user and does not happen until they agree to it. A Quarantined Account is
never the successor, because its Credential does not work, and neither is a
disabled one, because never being chosen for you is the whole of what disabled
means and this is Perch choosing.

The successor is not ranked on how full it is. Cycling ranks because it chooses
unasked and has to justify itself (ADR headroom-is-the-worst-window); this
choice is named in front of the user and does not happen until they agree to it,
so `perch switch` is the answer to wanting a different one and a ranking here
would only be a second place for the ranking to disagree with itself.

Landing is a `make_live` and not a Switch: no Capture. What is live is the
Credential of the Account being given up, and Capturing it would copy it into a
Profile that is about to be deleted — work that can only fail, on the way to a
directory that will not exist.

The order is landing, then deleting the Credential, then writing the registry.
Each step is only taken once the one that could still be undone has succeeded: a
landing that fails has cost nothing, and a store that will not give up its
Credential stops the removal while the Account can still be named and the command
run again. The registry is written last, so the one thing that cannot be retried
— an Account dropped from the registry with a Credential still on disk and no
name left to reach it by — cannot happen.

Two Profiles are refused rather than written: the Account's own, and — when
there is a successor to land on — the Default Profile, which is where the
landing has to be written and which a client may be running against
(ADR a-profile-is-live-by-evidence). It is the same pair `perch relogin` refuses
for the same reason.

## Considered Options

Deleting the Account and leaving the live Credential alone was considered. It is
simpler and it is what "forget this Account" literally means, but it ends with
the user running as somebody Perch cannot switch back to, which is the state the
command exists to leave them out of.

Refusing to remove the active Account, and telling the user to switch first, was
also considered and rejected. It is a refusal Perch could always have avoided by
doing the switch itself, and retiring the subscription you are on is the ordinary
case rather than a corner of one.

## Consequences

Where there is nowhere to land — the last Account Perch holds, or nothing left
that is neither Quarantined nor disabled — the removal is still allowed. It is
confirmed like any other, and says plainly that Perch will hold no active
Account afterwards. It does not log anybody out: the live Credential in the
Default Profile is not Perch's to take away, and the user is told that the
Credential Perch holds is going, so whatever replaces the live one ends that
login for good.

Both cases ask before they act, which makes `perch remove` the second command
after `perch add` that has something to say on a machine with nobody at the
terminal. It is answered the same way: `--yes` agrees in advance, and without a
terminal and without the flag the removal is refused rather than assumed
(ADR perch-does-not-draw). End of input is a no — a pipe that closed must never
read as agreement to delete a Credential.
