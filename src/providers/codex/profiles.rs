//! Codex profiles implementation.

use super::auth::identity;
use super::{CONFIG, refused};
use crate::providers::provider::ProfileRef as Account;
use crate::providers::provider::{self};
use crate::{Host, PerchError, Result};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub fn credential(host: &dyn Host, account: &Account) -> Result<Option<Zeroizing<String>>> {
    let path = account.profile_dir(host)?.join(super::AUTH_FILE);
    if !host.path_exists(&path) {
        return Ok(None);
    }
    let text = Zeroizing::new(
        host.read_file(&path)
            .map_err(|_| refused("Credential could not be read"))?,
    );
    let (found, _, _) = identity(&text)?;
    if account.provider_identity.as_ref() != Some(&found) {
        return Err(refused(
            "Credential belongs to another Account or Workspace",
        ));
    }
    Ok(Some(text))
}

pub fn write_credential(host: &dyn Host, account: &Account, document: &str) -> Result<()> {
    let (found, _, _) = identity(document)?;
    if account.provider_identity.as_ref() != Some(&found) {
        return Err(refused(
            "Credential belongs to another Account or Workspace",
        ));
    }
    let home = account.profile_dir(host)?;
    host.create_private_dir_all(&home)
        .map_err(|_| refused("Profile could not be created"))?;
    if !host.path_exists(&home.join("config.toml")) {
        crate::host::write_atomically(host, &home.join("config.toml"), CONFIG)
            .map_err(|_| refused("Profile config could not be written"))?;
    }
    crate::host::write_atomically(host, &home.join(super::AUTH_FILE), document)
        .map_err(|_| refused("Credential could not be written"))?;
    if credential(host, account)?.as_deref().map(String::as_str) != Some(document) {
        return Err(refused("Credential read-back failed"));
    }
    Ok(())
}

pub(crate) struct Restore<'a> {
    host: &'a dyn Host,
    accounts: Vec<(Account, Option<&'a str>, Option<&'a str>)>,
    created: Vec<PathBuf>,
}

impl<'a> Restore<'a> {
    pub(crate) fn prepare(
        host: &'a dyn Host,
        request: provider::RestoreRequest<'a>,
    ) -> Result<Self> {
        if let Some(bundle) = request.bundle {
            bundle.expect(&[
                (
                    super::AUTH_FILE,
                    crate::providers::provider::ArtifactPurpose::Credential,
                ),
                (
                    "config.toml",
                    crate::providers::provider::ArtifactPurpose::Configuration,
                ),
            ])?;
        }

        let mut accounts = Vec::new();
        {
            let account = request.profile;
            let home = account.profile_dir(host)?;
            if host.path_exists(&home) {
                return Err(PerchError::Conflict(format!(
                    "{} already exists; nothing was imported",
                    home.display()
                )));
            }
            let config = request.bundle.and_then(|bundle| bundle.get("config.toml"));
            // A Profile's login is the auth.json Perch writes, so a configuration
            // naming another store restores a Profile Codex would never read.
            if config
                .and_then(super::layout::store_named)
                .is_some_and(|store| store != "file")
            {
                return Err(refused(
                    "Export configuration keeps the login outside the file store; nothing was imported",
                ));
            }
            let document = request
                .bundle
                .and_then(|bundle| bundle.get(super::AUTH_FILE));
            if let Some(document) = document {
                let (found, _, _) = identity(document)?;
                if account.provider_identity.as_ref() != Some(&found) {
                    return Err(refused(
                        "Export Credential belongs to another Account or Workspace",
                    ));
                }
            }
            accounts.push((account, document, config));
        }
        Ok(Self {
            host,
            accounts,
            created: Vec::new(),
        })
    }

    pub(crate) fn write(&mut self) -> Result<()> {
        for (account, document, config) in &self.accounts {
            let home = account.profile_dir(self.host)?;
            self.host
                .create_private_dir_all(home.parent().unwrap())
                .map_err(|_| refused("Profile parent could not be created"))?;
            self.host
                .create_dir_exclusive(&home)
                .map_err(|_| refused("Profile already exists or could not be created"))?;
            self.created.push(home.clone());
            self.host
                .make_private(&home)
                .map_err(|_| refused("Profile could not be made private"))?;
            crate::host::write_atomically(
                self.host,
                &home.join("config.toml"),
                config.unwrap_or(CONFIG),
            )
            .map_err(|_| refused("Profile config could not be restored"))?;
            if let Some(document) = document {
                write_credential(self.host, account, document)?;
            }
        }
        Ok(())
    }

    pub(crate) fn commit(&mut self) {
        self.created.clear();
    }
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        for home in &self.created {
            let _ = self.host.remove_dir_all(home);
        }
    }
}

pub(super) fn refuse_live(host: &dyn Host, home: &Path) -> Result<()> {
    if matches!(
        crate::live::ask(
            host,
            &[crate::live::Place::new(
                crate::providers::provider::Id::Codex,
                "Codex Profile",
                home
            )]
        ),
        crate::live::Answer::NotIdle(_)
    ) {
        return Err(PerchError::Busy(
            "Codex is running against this Profile; close it before replacing its Credential"
                .into(),
        ));
    }
    Ok(())
}

impl provider::Restore for Restore<'_> {
    fn write(&mut self) -> Result<()> {
        Restore::write(self)
    }
    fn commit(&mut self) {
        Restore::commit(self)
    }
    fn rollback(&mut self) -> Result<()> {
        let mut cleanup = provider::Cleanup::default();
        for home in self.created.drain(..).rev() {
            cleanup.record(
                self.host
                    .remove_dir_all(&home)
                    .map_err(|error| crate::PerchError::file_write(&home, error)),
            );
        }
        cleanup.result()
    }
}
