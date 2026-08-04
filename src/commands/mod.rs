//! The commands themselves. Each one takes a `&dyn Host` and a writer and
//! returns a `Result`, so behaviour tests drive the real code with no process,
//! no filesystem, and no keychain.

pub mod add;
pub mod alias;
pub mod group;
pub mod status;

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
