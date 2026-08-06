//! `perch remove <target>` — giving up an Account when a subscription is
//! retired.
//!
//! The only command that destroys anything. It forgets the Account, deletes the
//! Credential Perch holds for it and takes its Profile with it, so it stops
//! being listed, stops being a Cycle candidate, and frees the Alias it answered
//! to. Everything else about Perch is reversible; this is not, which is what
//! shapes the rest of the command.
//!
//! Removing an Account nobody is on is unremarkable and asks nothing. Removing
//! the **active** Account is the case that needs care, because the live
//! Credential belongs to the Account being given up: Perch names the Account it
//! will leave active, lands on it first, and only then deletes anything. So the
//! machine is never left running as an Account Perch has forgotten (ADR 0024).
//!
//! Where there is nowhere to land — the last Account, or nothing left that
//! Cycling would ever choose — the removal is still allowed and still confirmed,
//! and says plainly that Perch will hold no active Account afterwards. What it
//! does not do is log anybody out: the live Credential in the Default Profile is
//! not Perch's to take away.

use std::io::Write;

use crate::adopt;
use crate::commands::{ask, say};
use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe;
use crate::registry::{self, Account, Registry};
use crate::switch;
use crate::target;

#[derive(Debug, Clone)]
pub struct RemoveArgs {
    /// The Account to give up: its Alias, or its email address.
    pub target: String,
    /// Answer the confirmation yes, for the removals that would ask.
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
    /// Account you are on, and giving up the last one Perch holds. Every other
    /// removal takes away something nothing is using.
    fn is_asked_about(&self) -> bool {
        self.is_active || self.remaining == 0
    }
}

pub fn run(host: &dyn Host, args: RemoveArgs, out: &mut dyn Write) -> Result<()> {
    let (_perch, mut registry) = adopt::ensure_adopted_exclusively(host, out)?;

    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    let account = registry
        .account(&found.email)
        .cloned()
        .expect("resolution named an Account Perch holds");

    // Before the question rather than after: an Account Perch may not touch is
    // not one to ask about giving up (ADR 0005).
    let version = probe::claude_version(host)?;
    switch::refuse_if_live(host, &account, &version)?;

    let consequence = consequence_of(&registry, &account);
    if consequence.successor.is_some() {
        // The Default Profile is written on the way out, and its Credential is
        // the one a running client is holding. Replacing it out from under a
        // session logs that session out mid-task (ADR 0005), and this removal
        // would replace it rather than renew it.
        switch::refuse_if_live_in(
            host,
            &probe::default_store(host)?.config_dir,
            "the Default Profile, which is where the Account Perch would land on \
             has to be written",
            &version,
        )?;
    }

    if !agreed(host, out, &registry, &account, &consequence, args.yes)? {
        return say(out, "Nothing was removed.");
    }

    // Somewhere to land before anything is deleted. A failure here has cost
    // nothing: the Account is still held, and its Credential is still live.
    if let Some(successor) = &consequence.successor {
        land_on(host, out, &mut registry, successor, &account)?;
    }

    let deleted = delete_the_credential_and_its_profile(host, &registry, &account)?;

    let named = registry.named_for_the_user(account.email());
    let alias = registry.alias_of(account.email()).map(str::to_string);
    registry.forget(account.email());
    registry::save(host, &registry).map_err(|error| {
        error.with_note(&format!(
            "The Credential Perch held for {} is already deleted, so the Account \
             it still records is one it can no longer switch to.",
            account.email()
        ))
    })?;

    report(out, &named, alias.as_deref(), &consequence, &deleted)
}

fn consequence_of(registry: &Registry, account: &Account) -> Consequence {
    let is_active = registry.active.as_deref() == Some(account.email());
    Consequence {
        is_active,
        successor: is_active
            .then(|| successor(registry, account).cloned())
            .flatten(),
        remaining: registry.accounts.len() - 1,
    }
}

/// The Account that will be left active when the active one is given up.
///
/// The Group comes first: Accounts in one Group are the ones the user has
/// declared interchangeable (ADR 0002), so landing there is the only landing
/// they have endorsed in advance. Failing that, any Account Perch holds will do
/// — which is not a Cycle leaving its scope but a forced choice, made in front
/// of the user and agreed to before it happens.
///
/// Never a Quarantined Account, whose Credential does not work, and never a
/// disabled one: never being chosen for you is the whole of what disabled means,
/// and this is Perch choosing.
fn successor<'a>(registry: &'a Registry, leaving: &Account) -> Option<&'a Account> {
    let candidates = || {
        registry
            .accounts
            .iter()
            .filter(|held| held.email() != leaving.email() && held.enabled && !held.quarantined())
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
/// the active Account" is only half of what the user needs to agree to — the
/// other half is where they will be standing afterwards.
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
        return Err(PerchError::Other(format!(
            "There is no terminal to confirm on, and this removal is one Perch \
             asks about: {}\n\
             Pass `--yes` to remove it without being asked.",
            what_it_would_leave(registry, account, consequence),
        )));
    }

    let named = registry.named_for_the_user(account.email());
    say(out, &what_it_would_leave(registry, account, consequence))?;
    let answered = ask(host, out, &format!("Remove {named}? [y/N]: "))?;

    // Anything that is not a yes is a no, and so is end of input: a pipe that
    // closed is the one thing that must never read as agreement to delete a
    // Credential.
    Ok(matches!(
        answered
            .as_deref()
            .map(str::trim)
            .map(str::to_lowercase)
            .as_deref(),
        Some("y" | "yes")
    ))
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
        None => format!(
            "Nothing Perch holds can be left active in its place, so it will \
             hold no active Account afterwards. Claude Code goes on running as \
             {} — the live Credential is not Perch's to take away — but the \
             Credential Perch holds is deleted, so anything that replaces the \
             live one ends that login for good.",
            account.email(),
        ),
    });
    said
}

/// Makes the successor's Credential the live one before the Account being given
/// up is destroyed, and records it as the active one the moment it becomes so.
///
/// Not a Switch, which Captures the outgoing Credential first (ADR 0006). What
/// is live here is the Credential of the Account being removed, and Capturing it
/// would copy it into a Profile that is about to be deleted — work that can only
/// fail, on the way to a directory that will not exist.
///
/// The active pointer is written here rather than with the rest of the registry
/// at the end, because everything between the two is destructive and any of it
/// can fail. Being active is a fact about which Credential is in the Default
/// Profile, not a wish: a `remove` that stopped after the landing would
/// otherwise leave `active` naming an Account whose Credential is no longer
/// live, and the next Switch would Capture the successor's live Credential over
/// that Account's own copy and destroy it (ADR 0006). Written whether the
/// landing finished or not, for the same reason — a Credential that reached the
/// Default Profile is live even if the Identity beside it was never patched.
fn land_on(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &mut Registry,
    successor: &Account,
    leaving: &Account,
) -> Result<()> {
    let landed = switch::make_live(host, successor);
    let is_live = landed.as_ref().err().is_none_or(|stopped| stopped.is_live);

    if is_live {
        registry.active = Some(successor.email().to_string());
        registry::save(host, registry).map_err(|error| {
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

    say(
        out,
        &format!(
            "{} is the active Account now — its Credential is the live one.",
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
    /// somebody a Credential is beyond recovery when it may be sitting in a
    /// keychain under a different `$USER` than this shell has.
    NothingWasThere,
    /// Left where it is, because another Account Perch holds keeps its own
    /// Credential in the same Profile.
    NothingSharedWith(String),
}

/// Deletes the Credential Perch holds for an Account, and the Profile that held
/// it.
///
/// A store that will not give its Credential up stops the removal rather than
/// being shrugged off. This is the whole of what `perch remove` promises beyond
/// forgetting a row, and it happens while the Account is still in the registry
/// on purpose: an Account dropped first could not be named again to try it.
///
/// The directory is the other way round. Once both stores have given up what
/// they hold it carries nothing secret — only the Identity block Claude Code
/// wrote — so one that will not go is worth a remark rather than a refusal with
/// nothing left to protect.
fn delete_the_credential_and_its_profile(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
) -> Result<Deleted> {
    // Two Accounts whose email addresses slug to one directory share a Profile,
    // and with it a Credential Store. Deleting it would take the Credential of
    // an Account nobody asked to give up — so the Account is still forgotten,
    // and the outcome says the Credential is not gone rather than claiming a
    // deletion that did not happen.
    if let Some(sharer) = registry.accounts.iter().find(|held| {
        held.email() != account.email() && registry::same_profile(held.email(), account.email())
    }) {
        return Ok(Deleted::NothingSharedWith(
            registry.named_for_the_user(sharer.email()),
        ));
    }

    let store = account.store(host)?;
    let mut anything_was_there = false;
    for kept_in in credentials::stores_for(host, &store) {
        let forgotten = kept_in.forget(host).map_err(|error| {
            error.with_note(&format!(
                "Nothing was removed — {} is still held, so `perch remove` can be \
                 run again once {} can be written to.",
                account.email(),
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
    out: &mut dyn Write,
    named: &str,
    alias: Option<&str>,
    consequence: &Consequence,
    deleted: &Deleted,
) -> Result<()> {
    let credential = match deleted {
        Deleted::Credential => "The Credential Perch held for it is deleted, and nothing lists it \
             or Cycles to it now."
            .to_string(),
        Deleted::NothingWasThere => "Nothing lists it or Cycles to it now. Neither of its \
             Credential Stores held anything to delete — on macOS a keychain item is filed under \
             `$USER`, so one written under a different login name is still there."
            .to_string(),
        Deleted::NothingSharedWith(sharer) => format!(
            "Nothing lists it or Cycles to it now. The Credential Perch held for \
             it is still there, because {sharer} keeps its own in the same \
             Profile and deleting one would take both."
        ),
    };
    say(out, &format!("Removed {named}. {credential}"))?;
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
