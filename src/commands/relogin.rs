//! `perch relogin <target>` — the way back from a Quarantine.
//!
//! An Account whose Credential stopped working is never dropped: it stays
//! listed, keeps its Alias, its Group, its place and whether Cycling may choose
//! it, and waits to be logged into again (ADR 0006). This is that login, and it
//! happens **in place** — the Account that comes back is the same Account, not a
//! new one wearing its name. Removing and re-adding would lose every one of
//! those settings and hand the user the job of putting them back.
//!
//! The login runs in a directory of its own, so the Account you are working in
//! is untouched throughout — including when the login is abandoned, which
//! changes nothing at all. The one exception is deliberate: repairing the
//! Account you are *on* writes its fresh Credential to the Default Profile too,
//! because otherwise the repair would sit in a Profile nothing reads and the
//! Account would go on being broken everywhere it is actually used.
//!
//! A healthy Account may be relogged in. Nothing about the command depends on
//! the Quarantine, and a Credential somebody suspects is going wrong should not
//! have to break first before it can be replaced.

use std::io::Write;

use crate::adopt;
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::Held;
use crate::login::{self, Produced};
use crate::probe::{self, Identity};
use crate::profile;
use crate::registry::{self, Account, Registry};
use crate::switch;
use crate::target;

#[derive(Debug, Clone)]
pub struct ReloginArgs {
    /// The Account to log in again: its Alias, or its email address.
    pub target: String,
}

pub fn run(host: &dyn Host, args: ReloginArgs, out: &mut dyn Write) -> Result<()> {
    // Read rather than held: the registry lock is not carried across a browser
    // login. It is taken below, against a registry read fresh once the login
    // has come back.
    let registry = adopt::ensure_adopted(host)?;

    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    let account = registry
        .account(&found.email)
        .cloned()
        .expect("resolution named an Account Perch holds");

    // Asked before the login rather than after: a Profile Perch may not write
    // to is one no browser round trip was going to repair (ADR 0005).
    let version = probe::claude_version(host)?;
    let repairing_the_account_you_are_on = registry.active.as_deref() == Some(account.email());
    refuse_while_anything_is_running(host, &account, repairing_the_account_you_are_on, &version)?;

    let produced = login::perform(
        host,
        out,
        &announcement(&registry, &account, repairing_the_account_you_are_on),
    )?;
    refuse_a_different_account(&registry, &account, &produced.identity)?;
    drop(registry);

    // From here the registry is the one on disk now, with the other Perches shut
    // out: the copy read before the login is however many commands out of date,
    // and writing it back would revert them.
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
    if registry.account(account.email()).is_none() {
        return Err(PerchError::NotFound(format!(
            "{} was removed while that login was happening, so there is nothing \
             left to repair.\n\
             The login itself worked — `perch add` holds it as a new Account.",
            account.email()
        )));
    }

    // Asked again, because the first answer is minutes old. A browser round
    // trip is the longest wait in Perch, and what follows writes a Credential
    // into both of those Profiles — so a `perch run`, or a `claude`, started
    // while the person was logging in would be written under (ADR 0027). The
    // login itself ran against a directory of its own, so it can never be what
    // this finds.
    //
    // Whether the Default Profile is written is asked of the registry as it is
    // now: another terminal may have switched away during the login, and then
    // this repair lands in the Account's own Profile alone.
    let repairing_the_account_you_are_on = registry.active.as_deref() == Some(account.email());
    refuse_while_anything_is_running(host, &account, repairing_the_account_you_are_on, &version)?;

    settle_into_its_own_profile(host, &account, &produced)?;

    // Recorded before the Credential is made live, because the repair is true
    // by now whatever happens next: the Account has a working Credential in its
    // own Profile, which is the whole of what a Quarantine said it did not have.
    let was_quarantined = record(&mut registry, &account, produced);
    registry::save(host, &mut perch, &registry)
        .map_err(|error| unrecorded(&account, repairing_the_account_you_are_on, error))?;

    report(out, &registry, &account, was_quarantined)?;

    // The same answer the liveness check above was given, deliberately: if
    // another terminal switched away during the login, the live Credential is
    // somebody else's now, and putting this one over it without a Capture would
    // destroy theirs (ADR 0006). Reading it twice is how the Profile that gets
    // written comes to be one that was never checked.
    if !repairing_the_account_you_are_on {
        return Ok(());
    }
    if let Err(error) = make_it_live(host, &account) {
        return Err(no_longer_on_anybody(
            host,
            &mut perch,
            &mut registry,
            &account,
            error,
        ));
    }
    say(
        out,
        &format!(
            "Its fresh Credential is the live one, so {} is working again \
             everywhere without a Switch.",
            account.email()
        ),
    )
}

/// The Profiles this repair writes into, refused while a client is holding one.
///
/// Repairing the Account you are on writes its fresh Credential into the Default
/// Profile as well, and that Credential is the one a running client is holding —
/// so both are asked about, and both are asked about twice: once before the
/// login, so a Profile Perch may not write to never costs a browser round trip
/// (ADR 0005), and once after, because a browser round trip is the longest wait
/// in Perch and the machine has had minutes to move on.
fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &Account,
    repairing_the_account_you_are_on: bool,
    version: &str,
) -> Result<()> {
    switch::refuse_if_live_anywhere(
        host,
        account,
        repairing_the_account_you_are_on.then_some(
            "the Default Profile, which is where this Account's repaired \
             Credential has to land",
        ),
        version,
    )
}

/// Refuses a login that authenticated somebody else.
///
/// The whole point of repairing in place is that the Alias, the Group and the
/// position belong to *this* Account. A login as a different person would hand
/// all three to whoever happened to be signed into the browser — silently, and
/// under a name the user chose for someone else. Adding them is a different
/// command and says so.
fn refuse_a_different_account(
    registry: &Registry,
    account: &Account,
    logged_in: &Identity,
) -> Result<()> {
    // Over the whole of Unicode, as `add` and `target` both ask it. An ASCII
    // fold was the one comparison left disagreeing with them, and it disagreed
    // on the path `add` sends people down: told that Perch already holds
    // `CAFÉ@example.com`, `add` refuses the second login — `same_profile` and
    // `same_name` both decided over the whole of Unicode — and says to run
    // `perch relogin CAFÉ@example.com` instead. Resolution then succeeded, the
    // browser round trip was spent, and this compared `café@` to `CAFÉ@` under
    // ASCII folding, decided they were different people, and refused. The
    // Account could not be repaired by either command.
    if registry::same_name(&logged_in.email, account.email()) {
        return Ok(());
    }
    Err(PerchError::Conflict(format!(
        "That login was {}, but `perch relogin` was asked to repair {}.\n\
         Nothing was changed — {} keeps its Alias, its Group and its place, and \
         handing those to another Account is not a repair.\n\
         To hold {} as well, run `perch add`.",
        logged_in.email,
        registry.named_for_the_user(account.email()),
        account.email(),
        logged_in.email,
    )))
}

/// Puts the fresh Credential where this Account's Credential lives, which is
/// the Profile it already had.
///
/// The Profile is written rather than replaced: nothing is removed first, so a
/// login that produced something Perch cannot store leaves the Account exactly
/// as broken as it was rather than more so.
fn settle_into_its_own_profile(host: &dyn Host, account: &Account, fresh: &Produced) -> Result<()> {
    let dir = account.profile_dir(host)?;
    let store = profile::create(host, &dir, fresh.credential.as_str())?;
    login::carry_identity_file(host, &fresh.identity_json, &store)
}

/// Records the repair, keeping everything about the Account that is not the
/// Credential.
///
/// Only the three things a login actually settles are written: who Anthropic
/// says this is, what they are paying for, and that the Credential works again.
/// The Alias, the Group, whether Cycling may choose it and where it sits in the
/// listing are all untouched — they are the user's decisions, and a login is not
/// a chance to revisit them.
fn record(registry: &mut Registry, account: &Account, fresh: Produced) -> bool {
    let was_quarantined = registry.release(account.email()).is_some();
    let held = registry
        .account_mut(account.email())
        .expect("the Account was just resolved");

    // The email is kept as Perch already holds it. It is the identifier every
    // Alias, Group and Profile path is derived from, so adopting a differently
    // capitalised spelling of the same address would move the Account's Profile
    // out from under it to no purpose.
    held.identity = Identity {
        email: held.identity.email.clone(),
        ..fresh.identity
    };
    held.plan = fresh.credential.subscription_type.clone();
    was_quarantined
}

/// Makes the repaired Credential the live one, for the Account that is already
/// the active one.
///
/// Never a Capture: what is live is the very Credential the login replaced, and
/// Capturing it would write the broken copy over the fresh one (ADR 0006).
fn make_it_live(host: &dyn Host, account: &Account) -> Result<()> {
    switch::make_live(host, account).map_err(|stopped| {
        stopped.error.with_note(&format!(
            "The repair itself stands: {} has a working Credential in its own \
             Profile and is no longer Quarantined. It is the live Credential \
             that was not replaced, so Claude Code goes on using the one that \
             stopped working. Run `perch relogin {}` again to finish the job.",
            account.email(),
            account.email(),
        ))
    })
}

/// Says what is on the machine after a repair that worked and could not be
/// recorded.
///
/// The Credential in the Account's own Profile is the fresh one; the registry
/// on disk still says Quarantined. Running the command again finishes the job,
/// because writing that Profile is idempotent — so the news is that the browser
/// round trip need not be repeated for nothing.
///
/// Repairing the Account you are on is the dangerous half, and it is the same
/// hazard [`no_longer_on_anybody`] exists for, reached one step earlier: the
/// broken Credential is still the live one, `active` still names this Account,
/// and the very next `perch switch` would Capture that broken copy over the
/// fresh one (ADR 0023). The defence there is to stop recording the Account as
/// active — which is a registry write, and a registry write is what just
/// failed. So this warns in the same words that path uses when its own save
/// fails, because it is the same state and the same thing not to do.
///
/// A bare `?` here said only that a file could not be written. It is the one
/// place in the command with an irreversible write behind it and nothing said
/// about it; `add`, `remove`, `import` and `switch` all note theirs.
fn unrecorded(
    account: &Account,
    repairing_the_account_you_are_on: bool,
    error: PerchError,
) -> PerchError {
    let email = account.email();
    if !repairing_the_account_you_are_on {
        return error.with_note(&format!(
            "The login itself worked and its Credential is in {email}'s own \
             Profile, so nothing was lost — only the record of it is behind. \
             Run `perch relogin {email}` again to finish the job."
        ));
    }
    error.with_note(&format!(
        "The login itself worked and its Credential is in {email}'s own \
         Profile, but Perch still records {email} as Quarantined and as the \
         Account you are on.\n\
         Do not run `perch switch` until `perch relogin {email}` has worked: a \
         Switch would Capture the Credential that stopped working over the \
         fresh one."
    ))
}

/// Stops Perch claiming to be on anybody, after a repair that could not be made
/// live.
///
/// What is left is a fresh Credential in the Account's own Profile and the
/// broken one it replaced still live. If `active` went on naming this Account,
/// the very next `perch switch` would Capture that broken copy over the fresh
/// one — ADR 0023 names this hazard and defends against it inside `relogin`,
/// but not on the ordinary command a user reaches for next. With nothing
/// active there is nothing to Capture into, so the repair survives whatever
/// they do.
fn no_longer_on_anybody(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &mut Registry,
    account: &Account,
    error: PerchError,
) -> PerchError {
    registry.active = None;
    let recorded = match registry::save(host, perch, registry) {
        Ok(()) => format!(
            "Perch holds no active Account now, so nothing will Capture the \
             Credential that stopped working over the fresh one. \
             `perch switch {}` puts you back on it.",
            account.email(),
        ),
        Err(unsaved) => format!(
            "Perch could not stop recording {} as active ({unsaved}), so do not \
             run `perch switch` until `perch relogin {}` has worked: a Switch \
             would Capture the Credential that stopped working over the fresh \
             one.",
            account.email(),
            account.email(),
        ),
    };
    error.with_note(&recorded)
}

fn announcement(registry: &Registry, account: &Account, is_the_active_one: bool) -> String {
    let repairing = format!("Logging in again to repair {}.", account.email());
    if is_the_active_one {
        return format!(
            "{repairing} It is the Account you are on, so its fresh Credential \
             becomes the live one."
        );
    }
    format!(
        "{repairing}{}",
        login::leaving_the_active_account_alone(registry.active.as_deref())
    )
}

fn report(
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    was_quarantined: bool,
) -> Result<()> {
    let named = registry.named_for_the_user(account.email());
    // The reason is not repeated here. Every surface said it while it was true,
    // and it has just stopped being true: an outcome that recites what was wrong
    // with a Credential that no longer exists reads as a state, not an ending.
    if was_quarantined {
        say(
            out,
            &format!(
                "\nRepaired {named} — it is no longer Quarantined, and is a Cycle \
                 candidate again if it was one before."
            ),
        )?;
    } else {
        say(
            out,
            &format!(
                "\nLogged {named} in again. It was not Quarantined; now it has a fresh Credential."
            ),
        )?;
    }

    let held = registry
        .account(account.email())
        .expect("the Account was just recorded");
    labelled(
        out,
        "Alias",
        registry.alias_of(account.email()).unwrap_or("-"),
    )?;
    labelled(out, "Group", held.group.as_deref().unwrap_or("none"))?;
    labelled(
        out,
        "Cycling",
        if held.enabled {
            "may choose it"
        } else {
            "will not choose it — it is disabled"
        },
    )
}

/// The three things a repair leaves exactly as it found them, in a column of
/// their own so they read as a list of what was kept.
fn labelled(out: &mut dyn Write, label: &str, value: &str) -> Result<()> {
    say(out, &format!("{:<9}{value}", format!("{label}:")))
}
