//! Synthetic Claude state arranged through Host, independently of the native adapter.

#![allow(
    dead_code,
    reason = "each test suite uses a subset of the native fixtures"
)]

use super::fixture_crate as perch;
use perch::host::{Host, Platform};
use perch::{PerchError, Result};
use std::path::{Path, PathBuf};

pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";

pub struct Store {
    pub config_dir: PathBuf,
    pub identity_file: PathBuf,
    pub credentials_file: PathBuf,
    pub keychain_service: String,
    pub keychain_account: String,
}

pub fn store_for_profile(host: &dyn Host, directory: &Path) -> Result<Store> {
    use sha2::{Digest, Sha256};
    let directory: PathBuf = directory.components().collect();
    let hash: String = Sha256::digest(directory.to_string_lossy().as_bytes())
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(Store {
        identity_file: directory.join(".claude.json"),
        credentials_file: directory.join(".credentials.json"),
        keychain_service: format!("{DEFAULT_SERVICE}-{}", &hash[..8]),
        keychain_account: host
            .env_var("USER")
            .or_else(|| host.env_var("USERNAME"))
            .unwrap_or_else(|| "(no keychain)".into()),
        config_dir: directory,
    })
}

pub fn default_profile_store(host: &dyn Host) -> Result<Store> {
    let home = host
        .home_dir()
        .map_err(|error| PerchError::Other(error.to_string()))?;
    let mut store = store_for_profile(host, &home.join(".claude"))?;
    store.keychain_service = DEFAULT_SERVICE.into();
    store.identity_file = home.join(".claude.json");
    Ok(store)
}

pub fn default_store(host: &dyn Host) -> Result<Store> {
    match host.env_var("CLAUDE_CONFIG_DIR") {
        Some(directory) => store_for_profile(host, Path::new(&directory)),
        None => default_profile_store(host),
    }
}

pub fn oauth_account_block(contents: &str) -> Option<&str> {
    perch::json::object_at(contents, "oauthAccount")
}

pub fn credentials_file_for(directory: &Path) -> PathBuf {
    directory.join(".credentials.json")
}

pub enum Placement {
    Keychain { service: String, account: String },
    File(PathBuf),
}

impl Placement {
    pub fn write(&self, host: &dyn Host, contents: &str) -> Result<()> {
        match self {
            Self::Keychain { service, account } => host
                .keychain_set(service, account, contents)
                .map_err(|error| PerchError::Other(error.to_string())),
            Self::File(path) => host
                .create_private_dir_all(path.parent().unwrap())
                .and_then(|()| host.write_private_file(path, contents))
                .map_err(|error| PerchError::Other(error.to_string())),
        }
    }
}

pub fn stores_for(host: &dyn Host, store: &Store) -> [Placement; 2] {
    let keychain = Placement::Keychain {
        service: store.keychain_service.clone(),
        account: store.keychain_account.clone(),
    };
    let file = Placement::File(store.credentials_file.clone());
    if host.platform() == Platform::MacOs {
        [keychain, file]
    } else {
        [file, keychain]
    }
}

#[derive(Debug, PartialEq)]
pub struct Stored {
    pub credential: String,
}

pub fn read(host: &dyn Host, store: &Store) -> Result<Option<Stored>> {
    let mut failure = None;
    for placement in stores_for(host, store) {
        let content = match placement {
            Placement::Keychain { service, account } => match host.keychain_get(&service, &account)
            {
                Ok(value) => Some(value),
                Err(perch::keychain::KeychainError::NotFound { .. }) => None,
                Err(_) if host.platform() != Platform::MacOs => None,
                Err(error) => {
                    failure.get_or_insert(error.to_string());
                    None
                }
            },
            Placement::File(path) => match host.read_file(&path) {
                Ok(value) => Some(value),
                Err(perch::host::HostError::NotFound { .. }) => None,
                Err(error) => {
                    failure.get_or_insert(error.to_string());
                    None
                }
            },
        };
        if let Some(credential) = content {
            return Ok(Some(Stored { credential }));
        }
    }
    match failure {
        Some(error) => Err(PerchError::Other(error)),
        None => Ok(None),
    }
}
