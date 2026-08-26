//! `perch remove <target>` — giving up an Account when a subscription is
//! retired.
//!
//! It forgets the Account, deletes the Credential Perch holds for it and takes
//! its Profile with it, so it stops being listed, stops being a Cycle candidate,
//! and frees the Alias it answered to. None of that can be undone, which shapes
//! the rest of the command: removing the **active** Account means the live
//! Credential belongs to the Account being given up, so Perch names the Account
//! it will leave active and lands on it first
//! (ADR a-removal-lands-first).

use std::io::Write;

use crate::adopt;
use crate::commands::{Presumed, said_yes, say, still_ours};
use crate::credentials;
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::Held;
use crate::probe::Installed;
use crate::registry::{self, Account, Registry};
use crate::switch;
use crate::target;

/// Why this command writes into the Default Profile, named for the two places
/// that have to agree about it: the refusal somebody meets *before* the
/// question, and the one [`switch::make_live`] raises after it. A constant
/// because two literals that must match by hand is how a user comes to be told
/// one thing when Perch asks and another when it acts.
const WHY_THE_DEFAULT_PROFILE: &str = "the Default Profile, which is where the Account Perch would land on has to \
     be written";

#[derive(Debug, Clone, clap::Args)]
pub struct RemoveArgs {
    /// The Account: its Alias, or its email address.
    pub target: String,

    /// Remove it without being asked to confirm.
    #[arg(long)]
    pub yes: bool,
}

/// What removing this Account would leave behind, worked out before anything is
/// destroyed — because it is what the user is being asked to agree to.
struct Consequence {
    /// Whether this is the Account whose Credential is live.
    is_active: bool,
    /// The Account that will be made active in its place. Only ever set for the
    /// active Account: removing any other one switches nothing.
    successor: Option<Account>,
    /// How many Accounts Perch would hold afterwards.
    remaining: usize,
}

impl Consequence {
    /// Whether the user is asked before this happens.
    ///
    /// The two cases are the two that change what the machine is: giving up the
    /// Account you are on, and giving up the last one Perch holds.
    fn is_asked_about(&self) -> bool {
        self.is_active || self.remaining == 0
    }
}

pub fn run(host: &dyn Host, args: RemoveArgs, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    // Removing the active Account lands somewhere first, which reaches
    // `make_live` — so this is a Switch path, and it resolves a Landing before
    // anything reads who is active (ADR a-switch-is-written-down-first).
    crate::commands::a_settled_landing(host, &mut perch, &mut registry)?;

    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    let account = registry.held(&found.email)?.clone();

    // Before the question rather than after: an Account Perch may not touch is
    // not one to ask about giving up (ADR a-profile-is-live-by-evidence).
    let consequence = consequence_of(&registry, &account);

    // Once per command, which is [`Installed`]'s own rule, and tolerated where
    // it fails: the liveness question is answered by Markers rather than by the
    // version, and giving up a lapsed subscription outlives the uninstall.
    let installed =
        Installed::probed(host).unwrap_or_else(|_| Installed::unknown("(not installed)"));

    refuse_while_anything_is_running(host, &account, &consequence, &installed)?;

    if !agreed(host, out, &registry, &account, &consequence, args.yes)? {
        return say(out, "Nothing was removed.");
    }

    // Asked again: somebody may have started a client while the question sat
    // there, and an answer about the machine as it was before lunch says nothing
    // about the Profile this is about to delete.
    refuse_while_anything_is_running(host, &account, &consequence, &installed)?;

    // The question above is the one wait in Perch with no bound on it, so it is
    // the one place the registry lock can go stale under a command that is
    // otherwise behaving. Asked before the first thing that cannot be undone.
    still_ours(&mut perch, "removed")?;

    // Somewhere to land before anything is deleted. A failure here has cost
    // nothing: the Account is still held, and its Credential is still live.
    if let Some(successor) = &consequence.successor {
        land_on(
            host,
            &mut perch,
            out,
            &mut registry,
            successor,
            &account,
            &installed,
        )?;
    }

    let deleted = delete_the_credential_and_its_profile(host, &registry, &account)?;

    let named = registry.named_for_the_user(account.email());
    let alias = registry.alias_of(account.email()).map(str::to_string);
    registry.forget(account.email());
    registry::save(host, &mut perch, &mut registry).map_err(|error| {
        error.with_note(&format!(
            "The Credential Perch held for {} is already deleted, so the Account \
             it still records is one it can no longer switch to.",
            account.email()
        ))
    })?;

    // On disk by here, so an unnoted failure sends a script back to give up an
    // Account it has already given up.
    report(host, out, &named, alias.as_deref(), &consequence, &deleted).map_err(|error| {
        error.with_note(
            "The Remove itself finished: the Account is given up and its \
             Credential deleted, and only the report could not be printed.",
        )
    })
}

/// The two Profiles this removal writes into, refused while a client is holding
/// either. One place, because it is asked twice — once so a machine that was
/// never going to allow this is not put through the question, and once after the
/// answer, because the question is unbounded and two spellings of one pair of
/// checks is how the second ask comes to be weaker than the first.
fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &Account,
    consequence: &Consequence,
    installed: &Installed,
) -> Result<()> {
    switch::refuse_if_live_anywhere(
        host,
        account,
        consequence
            .successor
            .is_some()
            .then_some(WHY_THE_DEFAULT_PROFILE),
        installed,
    )
}

fn consequence_of(registry: &Registry, account: &Account) -> Consequence {
    let is_active = registry.is_active(account.email());
    Consequence {
        is_active,
        successor: is_active
            .then(|| successor(registry, account).cloned())
            .flatten(),
        // Saturating. The subtraction is sound only because
        // `target::resolve_account` has matched, and the release profile sets no
        // `overflow-checks`, so a regression would wrap and stop asking.
        remaining: registry.accounts.len().saturating_sub(1),
    }
}

/// The Account that will be left active when the active one is given up: one in
/// the same Group first, because those are the ones the user has declared
/// interchangeable (ADR a-group-is-a-declaration), then any
/// [`cycle::is_a_candidate`]. Not filed under `cycle`, because a Cycle stays
/// inside its scope and this leaves one when it has to.
fn successor<'a>(registry: &'a Registry, leaving: &Account) -> Option<&'a Account> {
    let candidates = || {
        registry.accounts.iter().filter(|held| {
            !registry::same_name(held.email(), leaving.email())
                // A sharer is not a candidate, so landing nowhere is what a
                // Remove does with one: the removal still goes through.
                && cycle::is_a_candidate(registry, held)
        })
    };
    let in_its_group = leaving
        .group
        .as_deref()
        .and_then(|group| candidates().find(|held| held.group.as_deref() == Some(group)));
    in_its_group.or_else(|| candidates().next())
}

/// Whether this removal is to go ahead.
///
/// Everything that will happen is said before the question, because "removing
/// the active Account" is only half of what the user needs to agree to.
fn agreed(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    account: &Account,
    consequence: &Consequence,
    yes: bool,
) -> Result<bool> {
    if !consequence.is_asked_about() || yes {
        return Ok(true);
    }
    if !host.is_interactive() {
        return Err(PerchError::Invalid(format!(
            "There is no terminal to confirm on, and this removal is one Perch \
             asks about: {}\n\
             Pass `--yes` to remove it without being asked.",
            what_it_would_leave(registry, account, consequence),
        )));
    }

    let named = registry.named_for_the_user(account.email());
    say(out, &what_it_would_leave(registry, account, consequence))?;
    said_yes(host, out, &format!("Remove {named}? [y/N]: "), Presumed::No)
}

/// What the machine looks like afterwards, in the terms the user is deciding in.
fn what_it_would_leave(
    registry: &Registry,
    account: &Account,
    consequence: &Consequence,
) -> String {
    let named = registry.named_for_the_user(account.email());
    let mut said = if consequence.is_active {
        format!("{named} is the active Account. ")
    } else {
        format!("{named} is the only Account Perch holds. ")
    };

    said.push_str(&match &consequence.successor {
        Some(successor) => format!(
            "{} will be made active first, so nothing is left running as an \
             Account Perch has forgotten — `perch switch <target>` first if you \
             would rather land somewhere else. The login being given up goes \
             with it: holding it again would mean `perch add`.",
            registry.named_for_the_user(successor.email()),
        ),
        // Removing the *active* Account with nowhere to land leaves the machine
        // running as it, which is worth saying. Removing the last Account when
        // Perch is on nobody describes a state that is not theirs.
        None if consequence.is_active => format!(
            "Nothing Perch holds can be left active in its place, so it will \
             hold no active Account afterwards. Claude Code goes on running as \
             {} — the live Credential is not Perch's to take away — but the \
             Credential Perch holds is deleted, so anything that replaces the \
             live one ends that login for good.",
            account.email(),
        ),
        None => format!(
            "Perch is on no Account, so nothing is switched away from. The \
             Credential Perch holds for {} is deleted, and Perch will hold no \
             Accounts at all afterwards — whatever Claude Code is logged in as \
             is not Perch's to take away, and not Perch's to give back either.",
            account.email(),
        ),
    });
    said
}

/// Makes the successor's Credential the live one before the Account being given
/// up is destroyed, and records it as active the moment it becomes so — here
/// rather than with the rest of the registry at the end, because everything
/// between is destructive and can fail. Written whether the landing finished or
/// not: a Credential that reached the Default Profile is live either way.
fn land_on(
    host: &dyn Host,
    perch: &mut Held<'_>,
    out: &mut dyn Write,
    registry: &mut Registry,
    successor: &Account,
    leaving: &Account,
    installed: &Installed,
) -> Result<()> {
    let landed = switch::make_live(
        host,
        perch,
        registry,
        successor,
        WHY_THE_DEFAULT_PROFILE,
        installed,
    );
    let is_live = landed.as_ref().err().is_none_or(|stopped| stopped.is_live);

    if is_live {
        registry.settle(Some(successor.email().to_string()));
        registry::save(host, perch, registry).map_err(|error| {
            error.with_note(&format!(
                "Nothing was removed. {}'s Credential is the live one now, so \
                 `perch switch {}` puts the record right before anything else is \
                 tried.",
                successor.email(),
                successor.email(),
            ))
        })?;
    }

    landed.map_err(|stopped| {
        let held = format!("Nothing was removed — {} is still held", leaving.email());
        stopped.error.with_note(&if stopped.is_live {
            format!(
                "{held}. Its Credential is no longer the live one: {}'s is, and \
                 Perch records it as active. Run `perch switch {}` to finish \
                 landing there, then `perch remove` again.",
                successor.email(),
                successor.email(),
            )
        } else {
            format!("{held}, and its Credential is still the live one.")
        })
    })?;

    // Which Account, and nothing about its Credential being the live one: a
    // Landing that succeeded always made it so, and the two arms above are where
    // one that did not says which half is behind (ADR perch-says-what-it-did).
    say(
        out,
        &format!(
            "{} is the active Account now.",
            registry.named_for_the_user(successor.email())
        ),
    )
}

/// What became of the Credential Perch was holding, so the outcome says what
/// actually happened rather than what the command usually does.
enum Deleted {
    /// Gone from both of the Profile's Credential Stores, and the Profile with
    /// it.
    Credential,
    /// Both stores were asked and neither held anything to take. The Profile is
    /// gone, but nothing was destroyed — and saying otherwise would tell
    /// somebody a Credential is beyond recovery when it may be in a keychain
    /// under a different `$USER`.
    NothingWasThere,
    /// Left where it is, because another Account Perch holds keeps its own
    /// Credential in the same Profile.
    NothingSharedWith(String),
}

/// Deletes the Credential Perch holds for an Account, and the Profile that held
/// it. A store that will not give its Credential up stops the removal, while the
/// Account is still in the registry: one dropped first could not be named again
/// to try it. The directory is the other way round — emptied of both stores it
/// carries nothing secret, so one that will not go is a remark, not a refusal.
fn delete_the_credential_and_its_profile(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
) -> Result<Deleted> {
    // Two Accounts whose addresses slug to one directory share a Credential
    // Store, so deleting it would take the Credential of an Account nobody asked
    // to give up. The Account is still forgotten; the outcome says which.
    if let Some(sharer) = registry::sharing_a_profile_with(registry, account) {
        return Ok(Deleted::NothingSharedWith(
            registry.named_for_the_user(sharer.email()),
        ));
    }

    let store = account.store(host)?;
    let mut anything_was_there = false;
    for kept_in in credentials::stores_for(host, &store) {
        // A Profile has two stores emptied in order, so by the time the second
        // refuses the first may be empty already — and "Nothing was removed"
        // there is a claim about a Credential that is gone.
        let forgotten = kept_in.forget(host).map_err(|error| {
            let so_far = match anything_was_there {
                // Said as the state it is rather than as a Quarantine, which
                // this is not: a Quarantine is a thing the registry *records*,
                // and nothing here records one.
                true => format!(
                    "{}'s Credential has already been taken out of its other \
                     store, so a Switch onto it may no longer work",
                    account.email(),
                ),
                false => format!("Nothing was removed — {} is still held", account.email()),
            };
            error.with_note(&format!(
                "{so_far}, so `perch remove` can be run again once {} can be \
                 written to.",
                kept_in.describe(),
            ))
        })?;
        anything_was_there |= forgotten == credentials::Forgotten::Credential;
    }

    if host.remove_dir_all(&store.config_dir).is_err() {
        host.note(&format!(
            "{} held no Credential by the end and could not be removed. Nothing \
             in it is secret, and deleting it by hand is safe.",
            store.config_dir.display()
        ));
    }
    Ok(if anything_was_there {
        Deleted::Credential
    } else {
        Deleted::NothingWasThere
    })
}

/// What was given up, and what the user is standing on now.
fn report(
    host: &dyn Host,
    out: &mut dyn Write,
    named: &str,
    alias: Option<&str>,
    consequence: &Consequence,
    deleted: &Deleted,
) -> Result<()> {
    // Silent on the ordinary outcome, which is every Remove that found a
    // Credential and deleted it: that is what a Remove *is*. The two that are
    // not still speak in full.
    let credential = match deleted {
        Deleted::Credential => String::new(),
        Deleted::NothingWasThere => format!(
            " Neither of its Credential Stores held anything to delete — {}.",
            crate::commands::a_store_that_held_nothing(host),
        ),
        Deleted::NothingSharedWith(sharer) => format!(
            " The Credential Perch held for it is still there, because {sharer} \
             keeps its own in the same Profile and deleting one would take both."
        ),
    };
    say(out, &format!("Removed {named}.{credential}"))?;
    if let Some(alias) = alias {
        say(out, &format!("The Alias `{alias}` is free to use again."))?;
    }

    match (&consequence.successor, consequence.remaining) {
        (Some(_), _) => Ok(()),
        (None, 0) => say(
            out,
            "Perch now holds no Accounts and no active Account. `perch add` logs one in.",
        ),
        (None, remaining) if consequence.is_active => say(
            out,
            &format!(
                "Perch holds no active Account now — `perch switch <target>` \
                 makes {} active.",
                if remaining == 1 {
                    "the one it still holds".to_string()
                } else {
                    format!("one of the {remaining} it still holds")
                }
            ),
        ),
        (None, _) => Ok(()),
    }
}
