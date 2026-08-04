//! The Host port: every effect Perch has outside its own process.
//!
//! Commands take a `&dyn Host` and nothing else, so behaviour tests drive the
//! real command code against [`fake::FakeHost`] and assert on outcomes rather
//! than on mocks. One trait, two implementations: [`real::RealHost`] and the
//! fake that records what it was asked to do.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::keychain::KeychainError;

pub mod fake;
pub mod real;

pub use fake::FakeHost;
pub use real::RealHost;

/// The result of running another program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Execution {
    pub fn succeeded(&self) -> bool {
        self.status == 0
    }
}

/// A reply from an HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{path} does not exist")]
    NotFound { path: PathBuf },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub trait Host {
    // ---- clock ----------------------------------------------------------

    /// The current instant. Utilization is displayed as an observation with an
    /// age (ADR 0015), so the clock is an effect like any other.
    fn now(&self) -> DateTime<Utc>;

    // ---- environment ----------------------------------------------------

    fn home_dir(&self) -> PathBuf;
    fn env_var(&self, key: &str) -> Option<String>;

    // ---- filesystem -----------------------------------------------------

    fn read_file(&self, path: &Path) -> Result<String, HostError>;
    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError>;
    fn create_dir_all(&self, path: &Path) -> Result<(), HostError>;
    fn path_exists(&self, path: &Path) -> bool;

    // ---- keychain -------------------------------------------------------

    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError>;
    fn keychain_set(&self, service: &str, account: &str, secret: &str)
        -> Result<(), KeychainError>;
    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;

    // ---- processes ------------------------------------------------------

    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError>;

    /// Whether a process is still running. A Live Profile's Credential is
    /// untouchable because something else is holding it.
    fn process_alive(&self, pid: u32) -> bool;

    // ---- network --------------------------------------------------------

    fn http_get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HostError>;
}
