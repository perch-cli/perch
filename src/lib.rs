//! Perch manages provider Accounts, isolated client Runs, and quota-aware Cycling.
//!
//! Commands own workflows; providers own native tool behavior through
//! [`providers::provider`]. [`host`] owns effects, [`registry`] owns Account and
//! Scope policy, and [`storage`] persists the configuration and provider runtime.

pub mod act;
pub mod adopt;
pub mod ask;
pub mod column;
pub mod commands;
pub mod config;
pub mod cycle;
pub mod domain;
pub mod error;
pub mod export;
pub mod holdings;
pub mod host;
pub mod import;
pub mod json;
pub mod keychain;
pub mod listing;
pub mod live;
pub mod lock;
pub mod name;
pub mod observe;
pub mod providers;
pub mod purge;
pub mod redact;
pub mod registry;
pub mod report;
pub mod reserve;
pub mod round;
pub mod say;
pub mod secret;
pub mod service;
pub mod storage;
pub mod switch;
pub mod target;
#[cfg(test)]
mod test_support;
pub mod trail;
pub mod upgrade;
pub mod utilization;
pub mod wait;
pub mod watch;

pub use error::{PerchError, Result};
pub use host::Host;

#[cfg(test)]
use crate as fixture_crate;
#[cfg(test)]
#[path = "../tests/fixtures/claude.rs"]
mod claude_fixture;
