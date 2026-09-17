//! Codex auth implementation.

use super::process::environment;
use super::refused;
use crate::domain::Identity;
use crate::providers::provider::{AccountIdentity, Authenticated, Id};
use crate::{Host, PerchError, Result, holdings};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Auth<'a> {
    #[serde(borrow)]
    tokens: Tokens<'a>,
    auth_mode: Option<&'a str>,
}

#[derive(Deserialize)]
struct Tokens<'a> {
    id_token: &'a str,
    account_id: Option<&'a str>,
}

/// Claims label the CLI-produced Credential; decoding them does not authenticate it.
pub fn identity(document: &str) -> Result<(AccountIdentity, Identity, Option<String>)> {
    let auth: Auth<'_> =
        serde_json::from_str(document).map_err(|_| refused("Credential format is unsupported"))?;
    if auth.auth_mode.is_some_and(|mode| mode != "chatgpt") {
        return Err(refused(
            "requires subscription-backed ChatGPT authentication",
        ));
    }
    let encoded = auth
        .tokens
        .id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| refused("identity token is malformed"))?;
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| refused("identity token is malformed"))?,
    );
    let claims: Value =
        serde_json::from_slice(&decoded).map_err(|_| refused("identity claims are malformed"))?;
    let account = claims
        .get("https://api.openai.com/auth")
        .ok_or_else(|| refused("identity claims are missing"))?;
    let user = account
        .get("chatgpt_user_id")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("user identity is missing"))?;
    let workspace = account
        .get("chatgpt_account_id")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("Workspace identity is missing"))?;
    if auth.tokens.account_id != Some(workspace) {
        return Err(refused("Workspace identity disagrees with its Credential"));
    }
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .filter(|email| !email.trim().is_empty())
        .ok_or_else(|| refused("identity names no email address"))?;
    let identity = AccountIdentity::new(Id::Codex, user.into(), workspace.into())?;
    // No Organization name: the Workspace is a UUID, and the token's
    // `organizations` are API organizations rather than it.
    let description = Identity {
        email: email.into(),
        account_uuid: Some(user.into()),
        organization_uuid: Some(workspace.into()),
        organization_name: None,
    };
    let plan = account
        .get("chatgpt_plan_type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((identity, description, plan))
}

/// The login the Default home holds, for adoption. `None` wherever there is no
/// file login to take: no `auth.json`, a Default kept in another store, or a
/// Credential that is not a ChatGPT Account's. Not a refusal, because adoption
/// runs ahead of every command and a refusal here would stop all of them.
pub(super) fn discover(host: &dyn Host) -> Result<Option<Authenticated>> {
    let home = super::layout::default_home(host)?;
    if super::layout::refuse_unless_file_backed(host, &home).is_err() {
        return Ok(None);
    }
    let path = home.join(super::AUTH_FILE);
    let document = match host.read_file(&path) {
        Ok(document) => Zeroizing::new(document),
        Err(crate::host::HostError::NotFound { .. }) => return Ok(None),
        Err(error) => return Err(PerchError::file_read(path, error)),
    };
    let Ok((subject, identity, plan)) = identity(&document) else {
        return Ok(None);
    };
    Ok(Some(Authenticated {
        provider: Id::Codex,
        identity,
        subject: Some(subject),
        plan,
        credential: document,
        configuration: None,
    }))
}

struct Temporary<'a> {
    host: &'a dyn Host,
    path: PathBuf,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        let _ = self.host.remove_dir_all(&self.path);
    }
}

pub(super) fn login(host: &dyn Host, executable: &std::path::Path) -> Result<Zeroizing<String>> {
    let held = holdings::lock(host)?;
    let path = holdings::pending_logins_dir(Id::Codex, host)?.join(format!(
        "{}-{}",
        host.process_id(),
        host.now().timestamp_millis()
    ));
    host.create_dir_exclusive(&path)
        .or_else(|_| {
            host.create_private_dir_all(path.parent().unwrap())?;
            host.create_dir_exclusive(&path)
        })
        .map_err(|_| refused("login directory could not be created"))?;
    let temporary = Temporary { host, path };
    host.make_private(&temporary.path)
        .map_err(|_| refused("login directory could not be made private"))?;
    let _claim = crate::providers::sessions::claim(host, &temporary.path)?;
    drop(held);
    let env = environment(host, &temporary.path);
    let env: Vec<_> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let status = host
        .exec_interactive_under(
            &executable.to_string_lossy(),
            &[
                "-c",
                "cli_auth_credentials_store=\"file\"",
                "-c",
                "forced_login_method=\"chatgpt\"",
                "login",
            ],
            &env,
        )
        .map_err(|_| refused("login could not be started"))?;
    if status != 0 {
        return Err(refused("login did not complete"));
    }
    let document = Zeroizing::new(
        host.read_file(&temporary.path.join(super::AUTH_FILE))
            .map_err(|_| refused("login did not write a file Credential"))?,
    );
    Ok(document)
}
