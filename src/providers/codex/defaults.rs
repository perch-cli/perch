//! Codex Default Credential changes: a Capture of the outgoing copy, then one
//! write of `auth.json`. The file carries its own identity, so whose the live
//! Credential is comes off the file rather than off a separate Identity
//! (ADR a-switch-is-written-down-first).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

use super::auth::identity;
use super::profiles::{credential, write_credential};
use super::{AUTH_FILE, layout};
use crate::domain::Quarantine;
use crate::providers::provider::{
    Captured, DefaultChange, DefaultFailure, DefaultInspection, DefaultObservation, DefaultRequest,
    Id, ProfileRef as Account,
};
use crate::{Host, PerchError, Result, host, live, lock, name};

pub(super) fn begin<'h>(
    host: &'h dyn Host,
    request: DefaultRequest,
) -> Result<Box<dyn DefaultChange + 'h>> {
    let home = layout::default_home(host)?;
    layout::refuse_unless_file_backed(host, &home)?;
    if let Some(whose) = &request.overwrite {
        live::ask(host, &[live::Place::new(Id::Codex, whose.clone(), &home)])
            .idle_or(&live::NOTHING_WAS_CHANGED)?;
    }
    // Only of the Profile written to: the incoming Account's is read, and
    // reading takes nothing from a session using it.
    if let Some(outgoing) = &request.outgoing {
        live::ask(
            host,
            &[live::Place::new(
                Id::Codex,
                format!("{}'s Profile", outgoing.key()),
                outgoing.directory(),
            )],
        )
        .idle_or(&live::NOTHING_WAS_CHANGED)?;
    }
    let credential =
        credential(host, &request.incoming)?.ok_or_else(|| PerchError::Quarantined {
            why: Quarantine::NoCredential,
            said: format!(
                "Perch holds no Credential for {}, so it is Quarantined. `perch relogin {}` \
                 repairs it.",
                request.incoming.key(),
                request.incoming.key(),
            ),
        })?;
    Ok(Box::new(Edit {
        host,
        home,
        credential,
        request,
    }))
}

struct Edit<'h> {
    host: &'h dyn Host,
    home: PathBuf,
    /// What `apply` writes: the incoming Profile's copy, or the live one where
    /// a repair found the same Account Rotated since.
    credential: Zeroizing<String>,
    request: DefaultRequest,
}

impl DefaultChange for Edit<'_> {
    fn capture(&mut self, _perch: &mut lock::Held<'_>) -> Result<Captured> {
        let Some(outgoing) = &self.request.outgoing else {
            return Ok(Captured::NoOutgoing);
        };
        let live = read_live(self.host, &self.home).map_err(|error| {
            error.with_note(&format!(
                "The live Credential could not be read, so it was not Captured for {}. \
                 Make that file readable and run this again.",
                outgoing.key(),
            ))
        })?;
        let Some(live) = live else {
            return Ok(Captured::NothingLive);
        };
        if *live == *self.credential {
            return Ok(Captured::NothingToSave);
        }
        // Bytes that are not a Credential are not a Rotation to lose.
        let (found, described, _) = match identity(&live) {
            Ok(read) => read,
            Err(why) => {
                return Ok(Captured::Unreadable {
                    outgoing: outgoing.key().to_string(),
                    why: why.to_string(),
                });
            }
        };
        if outgoing.provider_identity.as_ref() != Some(&found) {
            return Ok(Captured::NotTheirs {
                outgoing: outgoing.key().to_string(),
                live: described.email,
            });
        }
        // A held copy Perch cannot read is overwritten rather than refused: the
        // live one is verified as this Account's and is what Codex is using.
        let held = credential(self.host, outgoing).ok().flatten();
        if held.as_deref() == Some(&*live) {
            return Ok(Captured::NothingToSave);
        }
        if held.is_some_and(|held| supersedes(&held, &live)) {
            return Ok(Captured::Superseded {
                outgoing: outgoing.key().to_string(),
            });
        }
        write_credential(self.host, outgoing, &live)?;
        // A repair of the Account that is already live: the copy just filed is
        // the newest, so it is the one written back rather than the older one.
        if name::same_name(self.request.incoming.key(), outgoing.key()) {
            self.credential = live;
        }
        Ok(Captured::Copied {
            from: outgoing.key().to_string(),
        })
    }

    fn apply(&mut self, _perch: &mut lock::Held<'_>) -> std::result::Result<(), DefaultFailure> {
        let path = self.home.join(AUTH_FILE);
        let config = self.home.join(layout::CONFIG_FILE);
        let written = self
            .host
            .create_private_dir_all(&self.home)
            .map_err(|error| PerchError::file_write(&self.home, error))
            .and_then(|()| {
                // A home Codex has never configured is pinned to the file store
                // this write is, so the next Codex reads what was written.
                if self.host.path_exists(&config) {
                    return Ok(());
                }
                host::write_atomically(self.host, &config, &format!("{}\n", layout::PIN))
                    .map_err(|error| PerchError::file_write(&config, error))
            })
            .and_then(|()| {
                host::write_atomically(self.host, &path, &self.credential)
                    .map_err(|error| PerchError::file_write(&path, error))
            });
        written.map_err(|error| DefaultFailure {
            error,
            moved: false,
        })
    }
}

/// Whether the copy an Account's own Profile holds is newer than the live one.
/// `last_refresh` is what a Rotation moves; strictly later, and only where both
/// say so.
fn supersedes(held: &str, live: &str) -> bool {
    matches!(
        (last_refresh(held), last_refresh(live)),
        (Some(held), Some(live)) if held > live
    )
}

fn last_refresh(document: &str) -> Option<DateTime<Utc>> {
    let value: serde_json::Value = serde_json::from_str(document).ok()?;
    let at = value.get("last_refresh")?.as_str()?;
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// The live Credential, or nothing where Codex is logged out.
fn read_live(host: &dyn Host, home: &Path) -> Result<Option<Zeroizing<String>>> {
    let path = home.join(AUTH_FILE);
    match host.read_file(&path) {
        Ok(contents) => Ok(Some(Zeroizing::new(contents))),
        Err(host::HostError::NotFound { .. }) => Ok(None),
        Err(error) => Err(PerchError::file_read(path, error)),
    }
}

/// Whether the Default already carries this Account's login, whichever Rotation
/// of it. Not the bytes: a Codex that Renewed since a Switch still displays
/// the Account the Switch landed on.
pub(super) fn already_landed(host: &dyn Host, account: &Account) -> Result<bool> {
    let Some(live) = read_live(host, &layout::default_home(host)?)? else {
        return Ok(false);
    };
    Ok(identity(&live)
        .ok()
        .is_some_and(|(found, _, _)| account.provider_identity.as_ref() == Some(&found)))
}

pub(super) fn inspect(host: &dyn Host) -> Result<Box<dyn DefaultInspection + '_>> {
    Ok(Box::new(Inspection {
        host,
        home: layout::default_home(host)?,
    }))
}

struct Inspection<'h> {
    host: &'h dyn Host,
    home: PathBuf,
}

impl DefaultInspection for Inspection<'_> {
    fn resolve(
        &mut self,
        _perch: &mut lock::Held<'_>,
        profiles: &[Account],
        leaving: Option<&str>,
        arriving: &str,
        may_continue: &mut dyn FnMut() -> bool,
    ) -> Result<DefaultObservation> {
        if !may_continue() {
            return Ok(DefaultObservation::Stopped);
        }
        let live = read_live(self.host, &self.home).map_err(|error| {
            error.with_note(&format!(
                "A Switch to {arriving} was in flight and was not recorded, and the live \
                 Credential is the only thing that says whether it happened.\nMake that \
                 file readable and run this again."
            ))
        })?;
        let Some(live) = live else {
            return Ok(DefaultObservation::Settled(leaving.map(str::to_string)));
        };
        // Whose it is comes off the identity the file carries, whichever
        // Rotation of it: a held copy that is byte-equal has the same identity,
        // and no two Accounts share one.
        let Ok((found, _, _)) = identity(&live) else {
            return Ok(DefaultObservation::Unknown);
        };
        Ok(profiles
            .iter()
            .find(|profile| profile.provider_identity.as_ref() == Some(&found))
            .map_or(DefaultObservation::Unknown, |profile| {
                DefaultObservation::Settled(Some(profile.key().to_string()))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_supersedes_only_where_both_refreshes_are_said_and_the_held_is_later() {
        let earlier = r#"{"last_refresh":"2026-09-01T00:00:00Z"}"#;
        let later = r#"{"last_refresh":"2026-09-02T00:00:00Z"}"#;
        let silent = r#"{}"#;
        assert!(supersedes(later, earlier));
        assert!(!supersedes(earlier, later));
        assert!(!supersedes(earlier, earlier), "equal is not strictly later");
        assert!(!supersedes(silent, earlier), "silence is no evidence");
        assert!(!supersedes(later, silent));
    }
}
