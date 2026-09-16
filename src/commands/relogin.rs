//! Provider Credential repair (ADR a-broken-account-is-repaired).

use std::io::Write;

use crate::adopt;
use crate::domain::Identity;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::Held;
use crate::name;
use crate::providers::provider::{Authenticated, InstallMode};
use crate::registry::{self, Account, Registry};
use crate::say;
use crate::switch;
use crate::wait;

/// Why this command writes into the Default Profile, named for the two places
/// that have to agree about it: the refusal somebody meets *before* the browser
/// round trip, and the one [`switch::make_live`] raises after it. Two literals
/// matching by hand is how the two come to say different things.
const WHY_THE_DEFAULT_PROFILE: &str = "the Default Profile, which is where this Account's repaired Credential has \
     to land";

#[derive(Debug, Clone, clap::Args)]
pub struct ReloginArgs {
    /// An Alias or email address
    pub target: String,
}

pub fn run(host: &dyn Host, args: ReloginArgs, out: &mut dyn Write) -> Result<()> {
    // Read rather than held: the lock is taken below, against a Registry read
    // fresh once the login has come back.
    let mut registry = adopt::ensure_adopted(host)?;
    let found = crate::target::resolve_account(&registry, &args.target)?;
    let account = registry.held(&found.email)?.clone();
    let provider = account.provider();
    let installation = provider.adapter().configured(host)?.installation(host)?;
    registry.select_provider(provider);
    // Asked before the login rather than after, like everything else here: a
    // repair writing into a store another Account is also kept in destroys that
    // Account's refresh token (ADR a-switch-is-written-down-first).
    switch::refuse_a_shared_profile(&account, &registry)?;

    // Asked before the login rather than after: a Profile Perch may not write
    // to is one no browser round trip was going to repair.
    let landing_in_the_default_profile = will_land_in_the_default_profile(&registry, &account);
    provider.adapter().check_replacement(
        host,
        &account.profile(host)?,
        landing_in_the_default_profile.then_some(WHY_THE_DEFAULT_PROFILE),
        &crate::live::NOTHING_WAS_CHANGED,
    )?;

    // Not `still_ours`, alone among the waits: no hold is taken before the
    // browser round trip, so the re-establish takes a fresh exclusive lock
    // rather than renewing a stale one.
    let (produced, (mut perch, mut registry, landing_in_the_default_profile), fresh) =
        wait::across(
            &mut (),
            |_| {
                say::line(out, &announcement(&account))?;
                if let Some(quit) = installation.provider().adapter().login_instruction() {
                    say::line(out, quit)?;
                }
                let produced = installation.authenticate(host)?;
                refuse_a_different_account(&registry, &account, &produced)?;
                Ok(produced)
            },
            |_| {
                // The Registry read before the login is however many commands out
                // of date, so it is dropped for the one on disk now.
                let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
                registry.select_provider(provider);
                if registry.account(account.key()).is_none() {
                    return Err(PerchError::NotFound(format!(
                        "{} was removed during that login. `perch add` holds the \
                         login as a new Account.",
                        account.email()
                    )));
                }

                let current = registry.held(account.key())?;
                if current.provider() != provider
                    || current.provider_identity != account.provider_identity
                    || current.identity.account_uuid != account.identity.account_uuid
                    || current.identity.organization_uuid != account.identity.organization_uuid
                {
                    return Err(PerchError::Conflict(format!(
                        "{} changed identity during that login. `perch relogin {}` \
                         again repairs the current Account.",
                        registry.named_for_the_user(account.key()),
                        registry.target_of(account.key()),
                    )));
                }

                // A Switch path, so it resolves a Landing before reading which
                // Account is active. A Conflict is the one failure this command may
                // not be stopped by: it offers `perch relogin` as its own way out.
                if let Err(unresolved) =
                    crate::commands::a_settled_landing(host, &mut perch, &mut registry)
                    && !matches!(unresolved, PerchError::Conflict(_))
                {
                    // Anything else is a store that would not answer rather than
                    // evidence that disagrees with itself, and a repair decided on
                    // a Profile nobody could read is not one to go through with.
                    return Err(unresolved);
                }

                // Whether the Default Profile is among the Profiles written below
                // is re-read too: another terminal may have switched away.
                let landing_in_the_default_profile =
                    will_land_in_the_default_profile(&registry, &account);
                provider.adapter().check_replacement(
                    host,
                    &account.profile(host)?,
                    landing_in_the_default_profile.then_some(WHY_THE_DEFAULT_PROFILE),
                    &crate::live::NOTHING_WAS_CHANGED,
                )?;
                Ok((perch, registry, landing_in_the_default_profile))
            },
        )?;

    let account = registry.held(account.key())?.clone();
    refuse_a_different_account(&registry, &account, &produced)?;
    settle_into_its_own_profile(host, &account, &produced, &fresh)?;

    // Recorded before the Credential is made live, because the repair is true
    // by now whatever happens next: the Account has a working Credential in its
    // own Profile, which is the whole of what a Quarantine said it did not have.
    let was_quarantined = record(&mut registry, &account, produced)?;
    registry::save(host, &mut perch, &mut registry)
        .map_err(|error| unrecorded(&registry, &account, landing_in_the_default_profile, error))?;

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
    );
    // `make_live` writes the Credential and then patches the Identity, so a
    // failure between the two has still made this Credential the live one. Both
    // things said below are only true on one side of that.
    match landed {
        // The held failure first: a stdout that will not take the line above
        // will not take this one either.
        Ok(()) => said.map_err(the_repair_stands),
        Err(stopped) if stopped.moved => Err(stopped.error.with_note(&format!(
            "The repair stands. `perch relogin {}` again finishes the job.",
            registry.target_of(account.key()),
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
    error.with_note("The repair finished. Only the report could not be printed.")
}

/// Refuses a login that authenticated somebody else.
///
/// The whole point of repairing in place is that the Alias, the Group and the
/// position belong to *this* Account. A login as a different person would hand
/// all three to whoever happened to be signed into the browser.
fn refuse_a_different_account(
    registry: &Registry,
    account: &Account,
    logged_in: &Authenticated,
) -> Result<()> {
    // Over the whole of Unicode, as `add` and `target` both ask it. An ASCII
    // fold here would refuse the very repair `add` sends people to, after the
    // browser round trip had been spent.
    let matches = match (&account.provider_identity, logged_in.subject()) {
        (Some(expected), Some(found)) => expected == found,
        (None, None) => {
            name::same_name(&logged_in.identity().email, account.email())
                && account.identity.account_uuid == logged_in.identity().account_uuid
                && account.identity.organization_uuid == logged_in.identity().organization_uuid
        }
        _ => false,
    };
    if matches {
        return Ok(());
    }
    Err(PerchError::Conflict(format!(
        "That login was {}, not {}. `perch add` holds it as a new Account.",
        logged_in.identity().email,
        registry.named_for_the_user(account.key()),
    )))
}

/// Puts the fresh Credential where this Account's Credential lives, which is
/// the Profile it already had. `KeepWhatLanded`, alone among the placements:
/// the write goes over the broken Credential, so there is no old copy an undo
/// could put back, and taking the fresh one out would leave the Account more
/// broken than it was.
fn settle_into_its_own_profile(
    host: &dyn Host,
    account: &Account,
    produced: &Authenticated,
    _fresh: &wait::Fresh,
) -> Result<()> {
    account
        .provider()
        .adapter()
        .install(host, &account.profile(host)?, produced, InstallMode::Repair)?
        .commit();
    Ok(())
}

/// Records the repair, keeping everything about the Account that is not the
/// Credential.
///
/// Only the three things a login settles: who the provider says this is, what they
/// are paying for, and that the Credential works again.
fn record(registry: &mut Registry, account: &Account, fresh: Authenticated) -> Result<bool> {
    let was_quarantined = registry.release(account.key()).is_some();
    let held = registry.held_mut(account.key())?;

    held.identity = if held.provider_identity.is_some() {
        fresh.identity().clone()
    } else {
        Identity {
            email: held.identity.email.clone(),
            ..fresh.identity().clone()
        }
    };
    held.plan = fresh.plan().clone();
    held.utilization = None;
    Ok(was_quarantined)
}

/// What is on the machine when the repaired Credential did not become the live
/// one at all.
///
/// Only for that side of [`switch::NotSwitched::moved`]: the live store still
/// holds the Credential that stopped working.
fn not_made_live(account: &Account, error: PerchError) -> PerchError {
    error.with_note(&format!(
        "The repair stands, and the live Credential was not replaced. `perch \
         relogin {}` again finishes the job.",
        account.key(),
    ))
}

/// Says what is on the machine after a repair that worked and was not recorded.
///
/// Repairing the Account you are on is the dangerous half: the broken Credential
/// is still live and `active` still names it, so the next Switch would Capture it
/// over the fresh one. The defense is a Registry write, which is what failed.
fn unrecorded(
    registry: &Registry,
    account: &Account,
    landing_in_the_default_profile: bool,
    error: PerchError,
) -> PerchError {
    let target = registry.target_of(account.key());
    if !landing_in_the_default_profile {
        return error.with_note(&format!(
            "The repair stands. Only the record is behind; `perch relogin \
             {target}` again finishes the job."
        ));
    }
    error.with_note(&format!(
        "The repair stands, and Perch still records {} as Quarantined.\n\
         `perch relogin {target}` again finishes the job. A `perch switch` \
         before then would Capture the broken Credential over the fresh one.",
        registry.named_for_the_user(account.key())
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
            registry.target_of(account.key()),
        ),
        Err(unsaved) => format!(
            "Perch could not stop recording {} as active ({unsaved}), so do not \
             run `perch switch` until `perch relogin {}` has worked: a Switch \
             would Capture the Credential that stopped working over the fresh \
             one.",
            registry.named_for_the_user(account.key()),
            registry.target_of(account.key()),
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
    registry.active().names(account.key())
}

/// What the login is for.
fn announcement(account: &Account) -> String {
    format!("Logging in again to repair {}.", account.key())
}

fn report(
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    was_quarantined: bool,
) -> Result<()> {
    let named = registry.named_for_the_user(account.key());
    // The reason is not repeated here. Every surface said it while it was true,
    // and it has just stopped being true: an outcome that recites what was wrong
    // with a Credential that no longer exists reads as a state, not an ending.
    if was_quarantined {
        say::line(
            out,
            &format!("\nRepaired {named}. It is no longer Quarantined."),
        )?;
    } else {
        // Which is the news. That the Account now holds a fresh Credential is
        // what every relogin does; that it did not need one is not.
        say::line(
            out,
            &format!("\nLogged {named} in again. It was not Quarantined."),
        )?;
    }

    // The Alias, the Group and a Cycling line are not said: a repair leaves all
    // three as it found them (ADR perch-says-what-it-did). Disabled is the one
    // that can still surprise — the Credential works and Cycling still passes.
    let held = registry
        .account(account.key())
        .expect("the Account was just recorded");
    if held.disabled {
        say::line(
            out,
            &format!(
                "Note: it is disabled, so Cycling will not choose it. `perch enable {}` \
                 undoes that.",
                registry.target_of(account.key())
            ),
        )?;
    }
    Ok(())
}
