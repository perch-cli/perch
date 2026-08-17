# Accounts

An Account is one Claude login Perch holds. This is how you gain one, name one,
keep one out of Cycling, repair one and give one up.

- [Adopting the login you already have](#adopting-the-login-you-already-have)
- [Adding an Account](#adding-an-account)
- [Naming an Account](#naming-an-account)
- [Keeping an Account out of Cycling](#keeping-an-account-out-of-cycling)
- [When an Account breaks](#when-an-account-breaks)
- [Giving up an Account](#giving-up-an-account)

## Adopting the login you already have

The first command you run adopts the Claude Code login already on the machine as
your first Profile, so nothing is lost and nothing has to be logged into again.

```
$ perch status
Adopted the Claude Code login as your first Profile: you@example.com (Acme, pro)
It is now the active Account. Claude Code 2.1.221.

Account       you@example.com
Organization  Acme
Plan          pro
Utilization   never observed
```

Adoption leaves that first Account ungrouped, which matters for
[Cycling](switching.md#cycling): being ungrouped is the *absence* of a
declaration that Accounts are interchangeable rather than a weaker form of one
(ADR 0017).

## Adding an Account

`perch add` gains an Account by running a login in a Profile of its own, so the
Account you are using stays active and its session is untouched (ADR 0009).

```
$ perch add --group work --alias overflow
```

`--group <name>` says which Group the new Account joins, and `--no-group` says
it joins none. One of the two is required where there is no terminal, because
the Group is otherwise a question — the Account's organization is offered as a
default for you to confirm — and a script has nobody to answer it. `--alias
<name>` names the Account in the same breath, so it never has to be typed as an
email address.

## Naming an Account

`perch alias <target> <name>` gives an Account a short name to reach it by, and
`perch alias <target> --unset` frees the name again. The Account comes first,
the way it does everywhere else in Perch, and both forms reach it the same way
every command does: by the name it already answers to, or by its email address.
So a name you have forgotten is freed by naming the Account it is on.

```
$ perch alias overflow@example.com overflow
$ perch alias overflow --unset
$ perch alias overflow@example.com --unset
```

Aliases and Group names share one namespace, so a name the other half already
answers to is refused — which is what keeps a Target from ever being ambiguous.

## Keeping an Account out of Cycling

`perch disable` keeps an Account out of Cycling without giving it up — for the
subscription you are holding for one particular thing and would rather Perch did
not spend on something else.

```
$ perch disable spare
`spare` is an Alias for spare@example.com.
Disabled spare@example.com (as `spare`). Cycling will not choose it — it stays listed and named, and `perch switch` still switches to it when you name it.

$ perch enable spare
`spare` is an Alias for spare@example.com.
Enabled spare@example.com (as `spare`). It is a Cycle candidate again.
```

A disabled Account is excluded from Cycling and from nothing else. It keeps its
Alias, its Group and its stored Credential, `perch list` shows it as `disabled`,
and naming it on `perch switch` still switches to it — so putting it back needs
no login, only `perch enable`. Removing the Account is the blunt instrument this
exists to avoid.

Disabling every Account in a Group is allowed. A bare `perch switch` there then
reports having no candidate (exit 17) rather than quietly landing you on
something you had taken out of Cycling.

## When an Account breaks

A Credential can stop working for good: Anthropic retires a refresh token, a
Rotation is lost between two writes, a login is ended somewhere else. Perch
never drops such an Account — an Account that vanishes reads as data loss, and a
broken one reads as something needing attention. It is **Quarantined**: still
listed, still named, shown as broken, and shown with the reason.

```
$ perch status --refresh
overflow@example.com is Quarantined: Anthropic would not renew its Credential. `perch relogin overflow@example.com` logs it in again in place, keeping its Alias, its Group and whether Cycling may choose it.
```

Cycling never chooses a Quarantined Account, and naming one on `perch switch` is
refused with exit code 19 rather than making a Credential live that does not
work — which would cost you the Account you are on. Enabling one does not repair
it: whether Cycling may choose an Account and whether its Credential works are
separate facts with separate fixes, so both are always said.

`perch relogin <target>` repairs it, and repairs it **in place**.

```
$ perch relogin overflow
`overflow` is an Alias for overflow@example.com.
Logging in again to repair overflow@example.com. someone@example.com stays active and its session is untouched.
Quit Claude Code when the login is done to come back here.

Repaired overflow@example.com (as `overflow`) — it is no longer Quarantined, and is a Cycle candidate again if it was one before.
Alias:   overflow
Group:   work
Cycling: may choose it
```

The Account keeps its Alias, its Group, whether Cycling may choose it and its
place in the listing — only the Credential is replaced. The login runs in a
directory of its own, so the Account you are working in is untouched throughout,
including when the login is abandoned, which changes nothing at all. A login as
a different Account is refused: an Alias you chose for one Account is not handed
to another because a browser was signed into somebody else.

Relogging in the Account you are **on** also makes its fresh Credential the live
one, because a repair only its own Profile can see would leave the Account broken
everywhere it is actually used (ADR 0023). A healthy Account may be relogged in
too — nothing about the command depends on the Quarantine.

## Giving up an Account

`perch remove <target>` is for the subscription that has been retired. It forgets
the Account and deletes the Credential Perch holds for it, so it stops being
listed, stops being a Cycle candidate, and the Alias it answered to comes free.

```
$ perch remove spare
`spare` is an Alias for spare@example.com.
Removed spare@example.com (as `spare`). The Credential Perch held for it is deleted, and nothing lists it or Cycles to it now.
The Alias `spare` is free to use again.
```

Removing the Account you are **on** is the case that needs care, because the live
Credential belongs to it. Perch names the Account it will leave active, lands on
it first, and asks before any of it happens (ADR 0024).

```
$ perch remove work
`work` is an Alias for someone@example.com.
someone@example.com (as `work`) is the active Account. overflow@example.com (as `overflow`) will be made active first, so nothing is left running as an Account Perch has forgotten — `perch switch <target>` first if you would rather land somewhere else. The login being given up goes with it: holding it again would mean `perch add`.
Remove someone@example.com (as `work`)? [y/N]: y
overflow@example.com (as `overflow`) is the active Account now — its Credential is the live one.
Removed someone@example.com (as `work`). The Credential Perch held for it is deleted, and nothing lists it or Cycles to it now.
The Alias `work` is free to use again.
```

The Account it lands on is one in the same Group where there is one, because a
Group is your own statement that those Accounts are interchangeable — never a
Quarantined Account, whose Credential does not work, and never a disabled one,
which is an Account you have said should not be chosen for you. It is not ranked
on how full it is: it is named before you agree to it, and `perch switch` is how
you choose differently.

Removing the last Account, or the active one when nothing is left that Perch
would land on, is allowed and confirmed the same way. It says that Perch will
hold no active Account afterwards, and it does not log you out: the live
Credential is not Perch's to take away, but the copy Perch holds is deleted, so
whatever replaces the live one ends that login for good.

`--yes` agrees in advance. Without a terminal and without the flag, a removal
that would have asked is refused rather than assumed, and end of input is a no.
The Group the Account was in stays declared — a Group is something you said, not
a summary of where the Accounts happen to be.
