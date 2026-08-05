//! Perch runs Claude Code as whichever Claude account you want, without going
//! through the login flow again.
//!
//! The layout follows the seams the design calls for: [`host`] is the only way
//! out of the process, [`probe`] is the only place that knows anything about
//! Claude Code's internals, [`keychain`] is the only place that knows about
//! `/usr/bin/security`, [`credentials`] is the only place that knows a
//! Credential can be in more than one kind of store, [`anthropic`] is the only
//! place that knows an endpoint, and [`registry`] is Perch's own state.

pub mod adopt;
pub mod anthropic;
pub mod commands;
pub mod credentials;
pub mod error;
pub mod host;
pub mod json;
pub mod keychain;
pub mod lock;
pub mod observe;
pub mod probe;
pub mod profile;
pub mod registry;
pub mod report;
pub mod switch;
pub mod target;
pub mod utilization;

pub use error::{PerchError, Result};
pub use host::Host;
