//! `perch version` — which Perch is installed, and whether a newer Release
//! exists.
//!
//! The report itself is `upgrade::version_report`, because the second line is
//! an Upgrade's question and is bounded where an Upgrade bounds it
//! (ADR an-upgrade-asks-its-channel).

use std::io::Write;

use crate::error::Result;
use crate::host::Host;
use crate::say;
use crate::upgrade;

/// Written rather than said: the report carries its own newline and may be two
/// lines, where `say` is one line with a newline put on it.
pub fn run(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    write!(out, "{}", upgrade::version_report(host)).map_err(say::failed)
}
