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
use crate::commands::{ask_a_word, say, still_ours};
use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::Held;
use crate::probe::Installed;
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
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    let found = target::resolve_account(&registry, &args.target)?;
    say(out, &found.matched)?;
    let account = registry.held(&found.email)?.clone();

    // Before the question rather than after: an Account Perch may not touch is
    // not one to ask about giving up (ADR 0005).
    let consequence = consequence_of(&registry, &account);
    refuse_while_anything_is_running(host, &account, &consequence)?;

    if !agreed(host, out, &registry, &account, &consequence, args.yes)? {
        return say(out, "Nothing was removed.");
    }

    // Asked again, for the same reason the hold below is re-checked and over
    // the same window: somebody may have started a client while the question
    // sat there, and an answer about the machine as it was before lunch says
    // nothing about the Profile this is about to delete. `perch purge` and
    // `perch relogin` both ask twice; this is the only command that deletes a
    // Credential, and it was asking once.
    refuse_while_anything_is_running(host, &account, &consequence)?;

    // The question above is the one wait in Perch with no bound on it — somebody
    // may answer it in a second or walk away and answer it after lunch — so it
    // is the one place the registry lock can go stale under a command that is
    // otherwise behaving. Asked here, before the first irreversible thing:
    // everything from this line on deletes Credentials, and finding out
    // afterwards that the registry recording them was never ours to write is
    // finding out too late.
    still_ours(&mut perch, "removed")?;

    // Somewhere to land before anything is deleted. A failure here has cost
    // nothing: the Account is still held, and its Credential is still live.
    if let Some(successor) = &consequence.successor {
        land_on(host, &mut perch, out, &mut registry, successor, &account)?;
    }

    let deleted = delete_the_credential_and_its_profile(host, &registry, &account)?;

    let named = registry.named_for_the_user(account.email());
    let alias = registry.alias_of(account.email()).map(str::to_string);
    registry.forget(account.email());
    registry::save(host, &mut perch, &registry).map_err(|error| {
        error.with_note(&format!(
            "The Credential Perch held for {} is already deleted, so the Account \
             it still records is one it can no longer switch to.",
            account.email()
        ))
    })?;

    report(host, out, &named, alias.as_deref(), &consequence, &deleted)
}

/// The two Profiles this removal writes into, refused while a client is holding
/// either.
///
/// One place, because it is asked twice — once so a machine that was never
/// going to allow this is not put through the question, and once after the
/// answer, because the question is unbounded and the first answer is about a
/// machine that has moved on. Two spellings of the same pair of checks is how
/// the second ask comes to be weaker than the first.
fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &Account,
    consequence: &Consequence,
) -> Result<()> {
    // A machine that cannot say which Claude Code it has is still a machine an
    // Account can be given up on. The version is not what answers this question
    // — session markers are, and they are read straight off the Profile — it is
    // only what a refusal quotes when the markers cannot be read. Propagated, it
    // refused the whole removal on a machine where Claude Code had been
    // uninstalled, leaving `perch purge` as the only way to give up one lapsed
    // subscription. `export::the_live_store` swallows it for the same reason.
    let installed =
        Installed::probed(host).unwrap_or_else(|_| Installed::unknown("(not installed)"));
    switch::refuse_if_live_anywhere(
        host,
        account,
        consequence.successor.is_some().then_some(
            "the Default Profile, which is where the Account Perch would land on \
             has to be written",
        ),
        &installed,
    )
}

fn consequence_of(registry: &Registry, account: &Account) -> Consequence {
    let is_active = registry.is_active(account.email());
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
        return Err(PerchError::Invalid(format!(
            "There is no terminal to confirm on, and this removal is one Perch \
             asks about: {}\n\
             Pass `--yes` to remove it without being asked.",
            what_it_would_leave(registry, account, consequence),
        )));
    }

    let named = registry.named_for_the_user(account.email());
    say(out, &what_it_would_leave(registry, account, consequence))?;
    let answered = ask_a_word(host, out, &format!("Remove {named}? [y/N]: "))?;

    // Anything that is not a yes is a no, and so is end of input: a pipe that
    // closed is the one thing that must never read as agreement to delete a
    // Credential.
    Ok(matches!(answered.as_deref(), Some("y" | "yes")))
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
        // Two different states wear the same shape here. Removing the *active*
        // Account with nowhere to land leaves the machine running as it, which
        // is worth saying. Removing the last Account when Perch is on nobody
        // does not: the live Credential may be another Account's, or there may
        // be none, and asking the user to agree to a description of a state
        // that is not theirs is asking them to agree to nothing.
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
    perch: &mut Held<'_>,
    out: &mut dyn Write,
    registry: &mut Registry,
    successor: &Account,
    leaving: &Account,
) -> Result<()> {
    let landed = switch::make_live(
        host,
        perch,
        successor,
        "the Default Profile, which is where the Account Perch would land on has \
         to be written",
    );
    let is_live = landed.as_ref().err().is_none_or(|stopped| stopped.is_live);

    if is_live {
        registry.active = Some(successor.email().to_string());
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
        // A Profile has two stores and they are emptied in order, so by the time
        // the second refuses the first may already be empty. Saying "Nothing was
        // removed" there is a claim about a Credential that is gone — on macOS
        // the keychain item goes first and the file second, and a Profile with
        // one of the two emptied is one `perch switch` may already be unable to
        // use. The Account stays in the registry either way, so running the
        // command again is still the answer; what changes is whether the user
        // has been told they are now relying on it.
        let forgotten = kept_in.forget(host).map_err(|error| {
            let so_far = match anything_was_there {
                true => format!(
                    "{}'s Credential has already been taken out of its other \
                     store, so it is Quarantined until this finishes",
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
    let credential = match deleted {
        Deleted::Credential => "The Credential Perch held for it is deleted, and nothing lists it \
             or Cycles to it now."
            .to_string(),
        Deleted::NothingWasThere => format!(
            "Nothing lists it or Cycles to it now. Neither of its Credential \
             Stores held anything to delete — {}.",
            crate::commands::a_store_that_held_nothing(host),
        ),
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
