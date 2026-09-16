//! Claude Profile payloads for sealed backups.

use super::profile;
use crate::providers::claude::{credentials, probe};
use crate::providers::provider::{DefaultRelation, ProfileContext, ProfileRef as Account};
use crate::{Host, Result, live};

/// The Credential to write for one Account: the live one where that is what the
/// Account's Credential *is*, and the copy in its own Profile otherwise.
///
/// A Renewal Rotates the live copy and Anthropic retires what it replaced, so
/// the active Account's Profile copy is the token likeliest to be dead already.
pub(super) fn credential(
    host: &dyn Host,
    context: &ProfileContext,
    account: &Account,
    installed: &crate::providers::claude::probe::Installed,
) -> Result<Option<String>> {
    // The live store first, and its own Profile as a fallback rather than the
    // answer: `claude /logout` empties the live store and leaves the Account
    // active, holding a Credential Perch has perfectly well.
    if let Some(live) = the_live_store(host, context, account, installed)?
        && let Some(credential) = read_from(host, &live, account)?
    {
        return Ok(Some(credential));
    }
    // An address no Profile could be named after has no store to read, and it
    // reaches the Registry only by hand. `perch holdings purge` takes such an
    // Account out and offers an Export on the way, so this must not stop either.
    let Ok(store) = account.store(host) else {
        return Ok(None);
    };
    read_from(host, &store, account)
}

/// The Default Profile, where what is live in it is this Account's Credential.
///
/// On the evidence [`crate::switch::capture`] wants before it copies that same
/// Credential anywhere (ADR a-switch-is-written-down-first): an Identity naming
/// this Account. A stable subject requires matching native identity evidence.
fn the_live_store(
    host: &dyn Host,
    context: &ProfileContext,
    account: &Account,
    installed: &crate::providers::claude::probe::Installed,
) -> Result<Option<crate::providers::claude::probe::Store>> {
    // A *settled* Registry rather than `is_active`, which during a Landing
    // answers with the Account being **left** while the live store may hold the
    // arriving one's — one token under two addresses, and a Renewal kills one.
    if context.default != DefaultRelation::Active {
        return Ok(None);
    }
    let live = crate::providers::claude::layout::default_profile(host)?;
    let identity = crate::providers::claude::probe::read_identity(host, &live, installed)
        .ok()
        .flatten();
    let belongs = identity
        .as_ref()
        .map_or(account.provider_identity.is_none(), |identity| {
            super::identity::names(identity, account)
        });
    Ok(belongs.then_some(live))
}

fn read_from(
    host: &dyn Host,
    store: &crate::providers::claude::probe::Store,
    account: &Account,
) -> Result<Option<String>> {
    let held = credentials::read(host, store).map_err(|error| {
        error.with_note(&format!(
            "Nothing was written. An Export that left {} out would be a partial \
             restore, which is the whole of what this file exists to prevent.",
            account.key(),
        ))
    })?;
    // Both the source buffer and the artifact holding this copy wipe on drop.
    Ok(held.map(|held| held.credential.to_string()))
}

pub(super) fn config(host: &dyn Host, account: &Account) -> Result<Option<String>> {
    let path = account.store(host)?.identity_file;
    match host.read_file(&path) {
        Ok(contents) => Ok(Some(contents)),
        Err(crate::host::HostError::NotFound { .. }) => Ok(None),
        Err(error) => Err(crate::PerchError::file_read(&path, error)),
    }
}

use zeroize::Zeroizing;

pub(super) struct Restore<'a> {
    host: &'a dyn Host,
    placements: Vec<(
        String,
        crate::providers::claude::probe::Store,
        Option<&'a str>,
        zeroize::Zeroizing<String>,
    )>,
    placed: Vec<super::profile::Placed>,
}
impl<'a> Restore<'a> {
    pub(super) fn prepare(
        host: &'a dyn Host,
        request: crate::providers::provider::RestoreRequest<'a>,
    ) -> Result<Self> {
        if let Some(bundle) = request.bundle {
            bundle.expect(&[
                (
                    "oauth",
                    crate::providers::provider::ArtifactPurpose::Credential,
                ),
                (
                    ".claude.json",
                    crate::providers::provider::ArtifactPurpose::Configuration,
                ),
            ])?;
        }

        let mut placements = Vec::new();
        {
            let account = &request.profile;
            let store = account.store(host)?;
            // Keyed the way the guard above asked the question: `profile_for`
            // folds case and a `BTreeMap` lookup does not, so a key spelled
            // `ONE@example.com` is listed by the one and missed by the other.
            let credential = request.bundle.and_then(|bundle| bundle.get("oauth"));
            let carried = request.bundle.and_then(|bundle| bundle.get(".claude.json"));
            // Either one is a Profile to make. A Quarantined Account travels with no
            // Credential and with the `.claude.json` naming it, and dropping that
            // makes the next Export smaller than the one that fed this Import.
            if credential.is_none() && carried.is_none() {
                return Ok(Self {
                    host,
                    placements,
                    placed: Vec::new(),
                });
            }
            // Verbatim where the Export carries one, because Claude Code's
            // `oauthAccount` block holds fields the Registry does not record and a
            // Switch prefers it (ADR everything-but-the-account).

            // Native configuration may contain API keys, so this copy also wipes on drop.
            let identity_file = Zeroizing::new(carried.map(str::to_string).unwrap_or_else(|| {
                probe::fresh_identity_file(&super::identity::compose(&account.identity))
            }));
            placements.push((account.key().to_string(), store, credential, identity_file));
        }

        // A Profile something is running against is one nothing writes into, which
        // `profile::store_credential` names as the obligation it cannot check for
        // itself. Asked over every placement before the first of them is written.
        let places: Vec<live::Place> = placements
            .iter()
            .map(|(email, store, _, _)| {
                live::Place::new(
                    crate::providers::provider::Id::Claude,
                    format!("{email}'s Profile at {}", store.config_dir.display()),
                    &store.config_dir,
                )
            })
            .collect();
        if let live::Answer::NotIdle(not_idle) = live::ask(host, &places) {
            return Err(not_idle.refusal(&NOTHING_WAS_IMPORTED));
        }

        Ok(Self {
            host,
            placements,
            placed: Vec::new(),
        })
    }
}
impl crate::providers::provider::Restore for Restore<'_> {
    fn write(&mut self) -> Result<()> {
        for (email, store, credential, identity_file) in &self.placements {
            // A Quarantined Account travels with no Credential and with the
            // `.claude.json` that names it, so the Profile is made for the file
            // alone: dropped, it is a re-Export smaller than the one that made it.
            match profile::place(
                self.host,
                &store.config_dir,
                *credential,
                Some(identity_file),
                profile::IfItFails::TakeBack,
            ) {
                Ok(one) => self.placed.push(one),
                Err(error) => {
                    return Err(
                        error.with_note(&format!("{email}'s Credential could not be stored."))
                    );
                }
            }
        }
        Ok(())
    }
    fn commit(&mut self) {
        self.placed.clear();
    }
    fn rollback(&mut self) -> Result<()> {
        let mut cleanup = crate::providers::provider::Cleanup::default();
        for placed in self.placed.drain(..).rev() {
            cleanup.record(placed.take_back(self.host));
        }
        cleanup.result()
    }
}
impl Drop for Restore<'_> {
    fn drop(&mut self) {
        for placed in &self.placed {
            let _ = placed.take_back(self.host);
        }
    }
}

/// What an Import leaves behind when it will not write: nothing at all, an
/// Import being whole or not having happened.
const NOTHING_WAS_IMPORTED: live::Consequence = live::Consequence {
    nothing_happened: "Nothing was imported.",
    quit_it: "That Credential would be replaced underneath the session holding \
              it. Close it and run this again.",
};
