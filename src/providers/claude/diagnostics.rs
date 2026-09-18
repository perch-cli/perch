//! Claude's native assumptions and diagnostic evidence.

use crate::providers::claude::probe;
use crate::providers::provider::{
    Assumption, AssumptionStatus, DiagnosticReport, Finding, Installation,
    diagnostic_code as finding,
};
use crate::{Host, PerchError, Result};

/// The assumptions in the order the probe reaches them, so everything after the
/// one that broke is honestly reported as never having been asked.
const REACHED: [&str; 6] = [
    probe::assumption::INSTALLED,
    probe::assumption::CREDENTIAL_LOCATION,
    probe::assumption::ACCOUNT_NAME,
    probe::assumption::CREDENTIAL_SHAPE,
    probe::assumption::IDENTITY_BLOCK,
    probe::assumption::SESSION_MARKER,
];

pub(super) fn gather(host: &dyn Host, installation: Result<Installation>) -> DiagnosticReport {
    let path = installation
        .as_ref()
        .ok()
        .map(|installation| installation.executable().to_path_buf());
    let installed = installation.and_then(|installation| {
        probe::version_at(host, installation.executable()).map(probe::Installed::Said)
    });
    let mut findings = Vec::new();
    let version = match &installed {
        Ok(installed) => Ok(installed.version().to_string()),
        Err(error) => {
            findings.push(Finding::refused(finding::PROVIDER_UNREADABLE, error));
            Err(error.to_string())
        }
    };
    let assumptions = REACHED
        .into_iter()
        .zip(asked(host, installed, &mut findings))
        .map(|(name, status)| Assumption {
            name: name.into(),
            status,
        })
        .collect();
    DiagnosticReport {
        version,
        path,
        assumptions,
        findings,
    }
}

/// Runs the one probe there is against the Default Profile's store, and reads
/// its outcome back onto the list of assumptions it passes through.
fn asked(
    host: &dyn Host,
    installed: Result<probe::Installed<'_>>,
    findings: &mut Vec<Finding>,
) -> [AssumptionStatus; REACHED.len()] {
    let mut held = [AssumptionStatus::Unread; REACHED.len()];
    let store = match probe::default_profile_store(host) {
        Ok(store) => store,
        Err(_) => return held,
    };

    match installed.and_then(|installed| probe::probe_with(host, store, &installed)) {
        // A Probe claims no Profile, so the last one is never reached.
        Ok(_) => {
            held = [AssumptionStatus::Held; REACHED.len()];
            held[REACHED.len() - 1] = AssumptionStatus::Unread;
        }
        Err(PerchError::ProbeRefused(refusal)) => {
            let broke = REACHED
                .iter()
                .position(|named| *named == refusal.assumption)
                .unwrap_or(0);
            for (at, one) in held.iter_mut().enumerate() {
                *one = match at.cmp(&broke) {
                    std::cmp::Ordering::Less => AssumptionStatus::Held,
                    std::cmp::Ordering::Equal => AssumptionStatus::Broke,
                    std::cmp::Ordering::Greater => AssumptionStatus::Unread,
                };
            }
            findings.push(Finding {
                provider: None,
                code: finding::ASSUMPTION_BROKE,
                exit_code: Some(crate::error::EXIT_PROBE_REFUSED),
                said: refusal.to_string(),
            });
        }
        Err(err) => {
            held[0] = AssumptionStatus::Held;
            let code = match err {
                PerchError::KeychainUnavailable(_) => finding::KEYCHAIN_UNAVAILABLE,
                _ => finding::STORE_UNREADABLE,
            };
            findings.push(Finding::refused(code, &err));
        }
    }
    held
}
