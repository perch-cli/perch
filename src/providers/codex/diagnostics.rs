//! Codex installation diagnostics use the CLI's own version response.

use crate::providers::provider::{DiagnosticReport, Finding, Installation, diagnostic_code};
use crate::{Host, PerchError, Result};

pub(super) fn gather(host: &dyn Host, installation: Result<Installation>) -> DiagnosticReport {
    let executable = installation.map(|installation| installation.executable().to_path_buf());
    let path = executable.as_ref().ok().cloned();
    let version = executable.and_then(|path| {
        let output = host
            .exec(&path.to_string_lossy(), &["--version"])
            .map_err(|error| {
                PerchError::Other(format!("Could not read Codex's version: {error}"))
            })?;
        if output.status != 0 || output.stdout.trim().is_empty() {
            return Err(PerchError::Other("Codex did not report a version".into()));
        }
        Ok(output.stdout.trim().to_string())
    });
    let findings = match &version {
        Ok(_) => Vec::new(),
        Err(error) => vec![Finding::refused(
            diagnostic_code::PROVIDER_UNREADABLE,
            error,
        )],
    };
    DiagnosticReport {
        version: version.map_err(|error| error.to_string()),
        path,
        assumptions: Vec::new(),
        findings,
    }
}
