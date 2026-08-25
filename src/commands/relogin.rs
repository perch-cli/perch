//! `perch relogin <target>` — the way back from a Quarantine
//! (ADR a-broken-account-is-repaired).
//!
//! The login happens **in place**: the Account that comes back is the same
//! Account, keeping its Alias, its Group, its place and whether Cycling may
//! choose it. It runs in a directory of its own, so the Account being worked in
//! is untouched — except where it *is* the Account being repaired, which writes
//! the fresh Credential to the Default Profile too.
//!
//! A healthy Account may be relogged in.

use std::io::Write;

use crate::adopt;
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::Held;
use crate::login::{self, Produced};
use crate::probe::{Identity, Installed};
use crate::profile;
use crate::registry::{self, Account, Registry};
use crate::switch;
use crate::target;

/// Why this command writes into the Default Profile, named for the two places
/// that have to agree about it: the refusal somebody meets *before* the browser
/// round trip, and the one [`switch::make_live`] raises after it. Two literals
/// matching by hand is how the two come to say different things.
const WHY_THE_DEFAULT_PROFILE: &str = "the Default Profile, which is where this Account's repaired Credential has \
     to land";

#[derive(Debug, Clone)]
pub struct ReloginArgs {
    /// The Account to log in again: its Alias, or its email address.
    pub target: String,
}

pub fn run(host: &dyn Host, args: ReloginArgs, out: &mut dyn Write) -> Result<()> {
    // Read rather than held: the lock is taken below, against a registry read
    // fresh once the login has come back.
    let registry = adopt::ensure_adopted(host)?;

    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    let account = registry.held(&found.email)?.clone();

    // Asked before the login rather than after, like everything else here: a
    // repair writing into a store another Account is also kept in destroys that
    // Account's refresh token (ADR a-switch-is-written-down-first).
    switch::refuse_a_shared_profile(&account, &registry)?;

    // Asked before the login rather than after: a Profile Perch may not write
    // to is one no browser round trip was going to repair.
    let installed = Installed::probed(host)?;
    let landing_in_the_default_profile = will_land_in_the_default_profile(&registry, &account);
    refuse_while_anything_is_running(host, &account, landing_in_the_default_profile, &installed)?;

    let produced = login::perform(
        host,
        out,
        &announcement(&registry, &account, landing_in_the_default_profile),
    )?;
    refuse_a_different_account(&registry, &account, &produced.identity)?;
    drop(registry);

    // From here the registry is the one on disk now: the copy read before the
    // login is however many commands out of date.
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    // A Switch path, so it resolves a Landing before reading which Account is
    // active. A Conflict is the one failure this command may not be stopped by:
    // `perch relogin` is what that state offers as the way out of itself.
    if let Err(unresolved) = crate::commands::a_settled_landing(host, &mut perch, &mut registry)
        && !matches!(unresolved, PerchError::Conflict(_))
    {
        // Anything else is a store that would not answer rather than evidence
        // that disagrees with itself, and a repair decided on a Profile nobody
        // could read is not one to go through with.
        return Err(unresolved);
    }

    if registry.account(account.email()).is_none() {
        return Err(PerchError::NotFound(format!(
            "{} was removed while that login was happening, so there is nothing \
             left to repair.\n\
             The login itself worked — `perch add` holds it as a new Account.",
            account.email()
        )));
    }

    // Asked again, because the first answer is minutes old and what follows
    // writes both Profiles. Whether the Default Profile is one of them is
    // re-read for the same reason: another terminal may have switched away.
    let landing_in_the_default_profile = will_land_in_the_default_profile(&registry, &account);
    refuse_while_anything_is_running(host, &account, landing_in_the_default_profile, &installed)?;

    settle_into_its_own_profile(host, &account, &produced)?;

    // Recorded before the Credential is made live, because the repair is true
    // by now whatever happens next: the Account has a working Credential in its
    // own Profile, which is the whole of what a Quarantine said it did not have.
    let was_quarantined = record(&mut registry, &account, produced)?;
    registry::save(host, &mut perch, &mut registry)
        .map_err(|error| unrecorded(&account, landing_in_the_default_profile, error))?;

    // Announced before the landing line, but its failure is *held*: a closed
    // stdout must not return before `make_live` and `no_longer_on_anybody`,
    // which are what make the repair safe.
    let said = report(out, &registry, &account, was_quarantined);

    // The same answer the liveness check above was given, deliberately: reading
    // it twice is how the Profile that gets written comes to be one that was
    // never checked.
    if !landing_in_the_default_profile {
        return said.map_err(the_repair_stands);
    }
    let landed = switch::make_live(
        host,
        &mut perch,
        &mut registry,
        &account,
        WHY_THE_DEFAULT_PROFILE,
        &installed,
    );
    // `make_live` writes the Credential and then patches the Identity, so a
    // failure between the two has still made this Credential the live one. Both
    // things said below are only true on one side of that.
    match landed {
        // The held failure first: a stdout that will not take the line above
        // will not take this one either.
        Ok(()) => said
            .and_then(|()| say(out, "Its fresh Credential is the live one."))
            .map_err(the_repair_stands),
        Err(stopped) if stopped.is_live => Err(stopped.error.with_note(&format!(
            "The repair stands and {} is working again: its fresh Credential is \
             the live one. What is behind is Claude Code's own record of which \
             Account that Credential belongs to, so it may display the wrong \
             one until `perch relogin {}` has finished the job.",
            account.email(),
            account.email(),
        ))),
        Err(stopped) => Err(no_longer_on_anybody(
            host,
            &mut perch,
            &mut registry,
            &account,
            not_made_live(&account, stopped.error),
        )),
    }
}

/// The note for a repair that landed and could not be reported, which is the
/// whole of what a non-zero exit would otherwise say.
fn the_repair_stands(error: PerchError) -> PerchError {
    error.with_note(
        "The repair itself finished: the Account has a working Credential in \
         its own Profile again, and only the report could not be printed.",
    )
}

/// The Profiles this repair writes into, refused while a client is holding one.
///
/// Both, because repairing the Account you are on writes the Default Profile as
/// well — and both twice, once before the login so a Profile Perch may not write
/// to costs no browser round trip (ADR a-profile-is-live-by-evidence).
fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &Account,
    landing_in_the_default_profile: bool,
    installed: &Installed,
) -> Result<()> {
    switch::refuse_if_live_anywhere(
        host,
        account,
        landing_in_the_default_profile.then_some(WHY_THE_DEFAULT_PROFILE),
        installed,
    )
}

/// Refuses a login that authenticated somebody else.
///
/// The whole point of repairing in place is that the Alias, the Group and the
/// position belong to *this* Account. A login as a different person would hand
/// all three to whoever happened to be signed into the browser.
fn refuse_a_different_account(
    registry: &Registry,
    account: &Account,
    logged_in: &Identity,
) -> Result<()> {
    // Over the whole of Unicode, as `add` and `target` both ask it. An ASCII
    // fold here would refuse the very repair `add` sends people to, after the
    // browser round trip had been spent.
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

/// Puts the fresh Credential where this Account's Credential lives, which is the
/// Profile it already had.
///
/// Written rather than replaced: nothing is removed first, so a login producing
/// something Perch cannot store leaves the Account exactly as broken as it was.
fn settle_into_its_own_profile(host: &dyn Host, account: &Account, fresh: &Produced) -> Result<()> {
    let dir = account.profile_dir(host)?;
    let store = profile::create(host, &dir, fresh.credential.as_str())?;
    login::carry_identity_file(host, &fresh.identity_json, &store)
}

/// Records the repair, keeping everything about the Account that is not the
/// Credential.
///
/// Only the three things a login settles: who Anthropic says this is, what they
/// are paying for, and that the Credential works again.
fn record(registry: &mut Registry, account: &Account, fresh: Produced) -> Result<bool> {
    let was_quarantined = registry.release(account.email()).is_some();
    let held = registry.held_mut(account.email())?;

    // The email is kept as Perch already holds it: every Alias, Group and Profile
    // path derives from it, so adopting another capitalization of the same
    // address would move the Account's Profile out from under it.
    held.identity = Identity {
        email: held.identity.email.clone(),
        ..fresh.identity
    };
    held.plan = fresh.credential.subscription_type.clone();
    Ok(was_quarantined)
}

/// What is on the machine when the repaired Credential did not become the live
/// one at all.
///
/// Only for that side of [`switch::NotLanded::is_live`]: the live store still
/// holds the Credential that stopped working.
fn not_made_live(account: &Account, error: PerchError) -> PerchError {
    error.with_note(&format!(
        "The repair itself stands: {} has a working Credential in its own \
         Profile and is no longer Quarantined. It is the live Credential that \
         was not replaced, so Claude Code goes on using the one that stopped \
         working. Run `perch relogin {}` again to finish the job.",
        account.email(),
        account.email(),
    ))
}

/// Says what is on the machine after a repair that worked and was not recorded.
///
/// Repairing the Account you are on is the dangerous half: the broken Credential
/// is still live and `active` still names it, so the next Switch would Capture it
/// over the fresh one. The defense is a registry write, which is what failed.
fn unrecorded(
    account: &Account,
    landing_in_the_default_profile: bool,
    error: PerchError,
) -> PerchError {
    let email = account.email();
    if !landing_in_the_default_profile {
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
/// With nothing active there is nothing to Capture into, so the fresh Credential
/// in the Account's own Profile survives whatever is run next.
fn no_longer_on_anybody(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &mut Registry,
    account: &Account,
    error: PerchError,
) -> PerchError {
    registry.settle(None);
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

/// Whether this repair writes the Default Profile as well as the Account's own.
///
/// The Account you are on — and the one state where Perch cannot say which that
/// is: a Landing names two, and repairing **either** of them lands.
fn will_land_in_the_default_profile(registry: &Registry, account: &Account) -> bool {
    // `names` alone: `is_active` asks `whose`, which for a Landing answers with
    // the Account being *left* — a subset of the two this already covers.
    registry.active().names(account.email())
}

fn announcement(registry: &Registry, account: &Account, landing_here: bool) -> String {
    let repairing = format!("Logging in again to repair {}.", account.email());
    if !landing_here {
        return format!(
            "{repairing}{}",
            login::leaving_the_active_account_alone(registry.active().whose())
        );
    }
    // Which of the two it is, said apart, because "the Account you are on" is
    // exactly what Perch cannot claim about half of a Landing.
    if registry.is_active(account.email()) {
        return format!(
            "{repairing} It is the Account you are on, so its fresh Credential \
             becomes the live one."
        );
    }
    format!(
        "{repairing} A Switch to it was left in flight, so its fresh Credential \
         becomes the live one and settles which Account is active."
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
            &format!("\nRepaired {named} — it is no longer Quarantined."),
        )?;
    } else {
        // Which is the news. That the Account now holds a fresh Credential is
        // what every relogin does; that it did not need one is not.
        say(
            out,
            &format!("\nLogged {named} in again. It was not Quarantined."),
        )?;
    }

    // The Alias, the Group and a Cycling line are not said: a repair leaves all
    // three as it found them (ADR perch-says-what-it-did). Disabled is the one
    // that can still surprise — the Credential works and Cycling still passes.
    let held = registry
        .account(account.email())
        .expect("the Account was just recorded");
    if held.disabled {
        say(
            out,
            "Cycling still will not choose it — it is disabled, which a repair \
             does not undo.",
        )?;
    }
    Ok(())
}
