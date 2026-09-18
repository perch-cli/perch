//! Diagnostic results and the interactive model-and-prompt convention.

use std::path::PathBuf;

use super::provider::{Id, Installation, LaunchEnvironment, PreparedLaunch};
use crate::{PerchError, Result};

pub struct DiagnosticSession<'a> {
    pub model: Option<&'a str>,
    pub prompt: &'a str,
}

/// Whether an assumption held.
///
/// `Unread` is not a doubt about the assumption: it is the probe having stopped
/// before it got there, which is a different thing from a belief that failed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssumptionStatus {
    Held,
    Broke,
    Unread,
}

impl AssumptionStatus {
    pub fn said(self) -> &'static str {
        match self {
            AssumptionStatus::Held => "held",
            AssumptionStatus::Broke => "broke",
            AssumptionStatus::Unread => "unread",
        }
    }
}

/// Something that would make a command refuse, with the code it would refuse
/// with. `code` is what a script counts; `said` is the same thing for a person.
#[derive(Clone)]
pub struct Finding {
    pub provider: Option<Id>,
    pub code: &'static str,
    pub exit_code: Option<i32>,
    pub said: String,
}

impl Finding {
    /// A failure Perch already knows how to refuse over, said as it says it.
    pub fn refused(code: &'static str, err: &PerchError) -> Finding {
        Finding {
            provider: None,
            code,
            exit_code: Some(err.exit_code()),
            said: err.to_string(),
        }
    }

    /// Something true of the machine that no single failure carries a code for.
    pub fn noticed(code: &'static str, said: String) -> Finding {
        Finding {
            provider: None,
            code,
            exit_code: None,
            said,
        }
    }
}

pub struct Assumption {
    pub name: String,
    pub status: AssumptionStatus,
}

pub struct DiagnosticReport {
    pub version: std::result::Result<String, String>,
    pub path: Option<PathBuf>,
    pub assumptions: Vec<Assumption>,
    pub findings: Vec<Finding>,
}

pub mod diagnostic_code {
    pub const PROVIDER_UNREADABLE: &str = "provider-unreadable";
    pub const ASSUMPTION_BROKE: &str = "assumption-broke";
    pub const KEYCHAIN_UNAVAILABLE: &str = "keychain-unavailable";
    pub const STORE_UNREADABLE: &str = "store-unreadable";
}

pub(super) fn session(
    installation: &Installation,
    request: &DiagnosticSession<'_>,
) -> Result<PreparedLaunch<'static>> {
    let program = installation.executable().to_string_lossy().into_owned();
    let mut arguments = Vec::new();
    if let Some(model) = request.model {
        arguments.extend(["--model".into(), model.into()]);
    }
    arguments.push(request.prompt.into());
    Ok(PreparedLaunch {
        program,
        arguments,
        environment: LaunchEnvironment::Overlay(Vec::new()),
        _claim: None,
    })
}
