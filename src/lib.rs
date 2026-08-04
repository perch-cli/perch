//! Perch runs Claude Code as whichever Claude account you want, without going
//! through the login flow again.
//!
//! The layout follows the seams the design calls for: [`host`] is the only way
//! out of the process, [`probe`] is the only place that knows anything about
//! Claude Code's internals, [`keychain`] is the only place that knows about
//! `/usr/bin/security`, and [`registry`] is Perch's own state.

pub mod adopt;
pub mod commands;
pub mod error;
pub mod host;
pub mod keychain;
pub mod probe;
pub mod registry;
pub mod report;

pub use error::{PerchError, Result};
pub use host::Host;
