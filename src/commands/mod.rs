//! The commands themselves. Each one takes a `&dyn Host` and a writer and
//! returns a `Result`, so behaviour tests drive the real code with no process,
//! no filesystem, and no keychain.

pub mod add;
pub mod alias;
pub mod group;
pub mod list;
pub mod status;
pub mod switch;

use std::io::Write;

use crate::error::{PerchError, Result};

/// A command's output going nowhere — a closed pipe, most often. Not a failure
/// of the thing the command was asked to do, but the command cannot finish
/// saying what it did.
pub fn write_failed(err: std::io::Error) -> PerchError {
    PerchError::Other(err.to_string())
}

/// One line to the person running the command.
pub fn say(out: &mut dyn Write, line: &str) -> Result<()> {
    writeln!(out, "{line}").map_err(write_failed)
}

/// What the Accounts in no Group are shown under. Being in no Group is not a
/// Group (ADR 0017), so it is never a heading that reads like one.
pub const IN_NO_GROUP: &str = "In no Group";

/// What Cycling will not do with those Accounts until it is told it may (ADR
/// 0017), as a clause both surfaces that show them finish a sentence with. One
/// sentence, because two would sooner or later say two different things.
pub const CYCLING_AMONG_UNGROUPED: &str = "only moves between these when you say it may";
