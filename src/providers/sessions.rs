//! Claude session evidence and Perch's compatible session markers.

use crate::{PerchError, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

use crate::host::{Host, HostError};
use crate::live::Unsure;

use super::provider::SessionEvidence;

#[derive(serde::Deserialize)]
struct Marker {
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
}

#[derive(serde::Deserialize)]
struct Owner {
    #[serde(rename = "writtenBy")]
    written_by: Option<String>,
}

pub(super) fn read(
    host: &dyn Host,
    directory: &Path,
    perch_only: bool,
) -> std::result::Result<Vec<SessionEvidence>, Unsure> {
    let dir = directory.join("sessions");
    let markers = match host.list_dir(&dir) {
        Ok(markers) => markers,
        Err(HostError::NotFound { .. }) => return Ok(Vec::new()),
        Err(why) => return Err(Unsure::Unlistable { dir, why }),
    };
    let mut evidence = Vec::new();
    for marker in markers {
        let Some(pid) = marker
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse::<u32>().ok())
            .filter(|pid| *pid != 0)
        else {
            continue;
        };
        let started_at = match host.read_file(&marker) {
            Ok(contents) => {
                let Ok(record) = serde_json::from_str::<Marker>(&contents) else {
                    continue;
                };
                if perch_only
                    && !serde_json::from_str::<Owner>(&contents)
                        .is_ok_and(|owner| owner.written_by.as_deref() == Some("perch"))
                {
                    continue;
                }
                let Some(started_at) = record.started_at else {
                    continue;
                };
                Some(started_at)
            }
            Err(HostError::NotFound { .. }) => continue,
            Err(_) => None,
        };
        evidence.push(SessionEvidence {
            pid,
            marker,
            started_at,
        });
    }
    Ok(evidence)
}

pub(super) const SESSIONS: &str = "sessions";

/// Where Claude Code records the sessions it is running: one `<pid>.json` per
/// client, in the config directory it was launched against.
pub(super) fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(SESSIONS)
}

/// The marker for one process running against a config directory.
pub(super) fn session_marker_at(config_dir: &Path, pid: u32) -> PathBuf {
    sessions_dir(config_dir).join(format!("{pid}.json"))
}

/// The marker a Run writes to say a Profile is Live, in the shape Claude Code
/// writes one and this module reads one back. Three fields and no more: the two
/// that make it evidence, and one that says who wrote it. `started_at` is when
/// the Run began rather than when the process did, which is what makes the file
/// corroborate itself — the process it names began strictly earlier.
pub(super) fn session_marker(pid: u32, started_at: DateTime<Utc>) -> String {
    serde_json::json!({
        "pid": pid,
        "startedAt": started_at.timestamp_millis(),
        "writtenBy": "perch",
    })
    .to_string()
}

/// A config directory this process has made Live, for as long as this value is
/// held. Perch writes session markers as well as reading them: a Run makes the
/// Profile it launches Live (ADR a-run-is-one-shot), and a login makes the
/// directory it is driving Live. A value with a `Drop` rather than a call to
/// make, because a bare removal is the line the next early return walks past.
pub(super) struct Claim<'a> {
    host: &'a dyn Host,
    marker: PathBuf,
}

impl Drop for Claim<'_> {
    /// However the operation ended, including not having started. A login's
    /// directory is gone by now — `profile::discard` takes it whole — so this is
    /// removing a file inside a directory that is not there, which the port says
    /// is not a failure.
    fn drop(&mut self) {
        let _ = self.host.remove_file(&self.marker);
    }
}

/// Makes a config directory Live, naming this process. Perch's own pid, because
/// Perch waits for what it started, so the marker holds for exactly as long as
/// the operation. Written atomically, because a plain write truncates and then
/// fills and a file that can be read whole and says nothing settles as "not
/// Live". Whether a claim that fails is fatal is the caller's to decide.
pub(super) fn claim<'a>(host: &'a dyn Host, config_dir: &Path) -> Result<Claim<'a>> {
    let pid = host.process_id();
    let marker = session_marker_at(config_dir, pid);
    let sessions = sessions_dir(config_dir);

    // A `sessions` that is a link is refused rather than written through and
    // rather than repaired, which is Reconcile's: `create_dir_all` at a link
    // uses the target, so the Marker would land in the Default Profile.
    if matches!(host.link_target(&sessions), Ok(Some(_))) {
        return Err(PerchError::Other(format!(
            "{} is a link, and Perch will not record a running client through \
             one. Replace it with a directory.",
            sessions.display()
        )));
    }

    // Private, because this is the third path that brings a Profile directory
    // into being and 0700 is what a Profile owes. One already there is left as
    // it is, so the Default Profile keeps whatever mode it has.
    host.create_private_dir_all(&sessions)
        .and_then(|()| {
            crate::host::write_atomically(host, &marker, &session_marker(pid, host.now()))
        })
        .map_err(|err| {
            PerchError::Other(format!(
                "{} could not be written ({err}), so Perch cannot record that a \
                 client is running against this Profile.",
                marker.display()
            ))
        })?;

    Ok(Claim { host, marker })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;
    use crate::host::prelude::*;
    use crate::live::{self, Place};
    use crate::providers::provider::Id;

    #[test]
    fn a_claim_makes_each_provider_live_for_exactly_its_lifetime() {
        for provider in [Id::Claude, Id::Codex] {
            let host = FakeHost::new();
            let dir = provider.home(&host).unwrap().join("profiles/one");
            let places = [Place::at(provider, &dir)];
            assert!(!live::ask(&host, &places).counts_as_live());
            let claimed = claim(&host, &dir).unwrap();
            assert!(live::ask(&host, &places).counts_as_live());
            let evidence = provider
                .adapter()
                .session_evidence(&host, &dir)
                .unwrap_or_else(|_| panic!("the claim is readable"));
            assert_eq!(evidence.len(), 1);
            assert_eq!(evidence[0].started_at, Some(host.now().timestamp_millis()));
            drop(claimed);
            assert!(!live::ask(&host, &places).counts_as_live());
        }
    }

    #[test]
    fn ambiguous_timestamps_are_not_session_evidence() {
        for provider in [Id::Claude, Id::Codex] {
            let host = FakeHost::new();
            let dir = provider.home(&host).unwrap().join("profiles/one");
            let at = host.now().timestamp_millis();
            host.set_file(
                session_marker_at(&dir, host.process_id()),
                &format!(r#"{{"startedAt":{at},"startedAt":{at},"writtenBy":"perch"}}"#),
            );
            assert!(!live::ask(&host, &[Place::at(provider, &dir)]).counts_as_live());
        }
    }
}
