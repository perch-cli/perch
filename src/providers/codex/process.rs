//! Codex process implementation.

use super::profiles::{credential, refuse_live};
use crate::providers::provider::{self, LaunchKind};
use crate::{Host, PerchError, Result};
use std::path::Path;

pub fn environment(host: &dyn Host, home: &Path) -> Vec<(String, String)> {
    let mut env: Vec<_> = [
        "HOME",
        "USERPROFILE",
        "USER",
        "LOGNAME",
        "PATH",
        "SHELL",
        "TERM",
        "COLORTERM",
        "LANG",
        "LC_ALL",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SystemRoot",
        "COMSPEC",
        "PATHEXT",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ]
    .into_iter()
    .filter_map(|key| host.env_var(key).map(|value| (key.to_string(), value)))
    .collect();
    env.push(("CODEX_HOME".into(), home.to_string_lossy().into_owned()));
    env
}

pub fn run_environment(host: &dyn Host, home: &Path) -> Vec<(String, String)> {
    let mut env: Vec<_> = host
        .inherited_env()
        .into_iter()
        .filter(|(key, _)| {
            let key = key.to_ascii_uppercase();
            !key.starts_with("OPENAI_") && !key.starts_with("CODEX_")
        })
        .collect();
    env.push(("CODEX_HOME".into(), home.to_string_lossy().into_owned()));
    env
}

pub(super) fn prepare_launch<'a>(
    host: &'a dyn Host,
    request: &provider::LaunchRequest<'_>,
) -> Result<provider::PreparedLaunch<'a>> {
    use provider::{LaunchEnvironment, PreparedLaunch};
    let account = request.account;
    let command = request.arguments;
    let home = account.profile_dir(host)?;
    credential(host, account)?.ok_or_else(|| {
        PerchError::NotFound(
            "No Codex Credential is held for it. `perch relogin <target>` logs it in again.".into(),
        )
    })?;
    refuse_live(host, &home)?;
    let claim = crate::providers::sessions::claim(host, &home)?;
    let (program, arguments, environment) = match request.kind {
        LaunchKind::Custom(program) => (
            program.to_string(),
            command.to_vec(),
            LaunchEnvironment::Overlay(vec![(
                "CODEX_HOME".into(),
                home.to_string_lossy().into_owned(),
            )]),
        ),
        LaunchKind::Client(installed) => {
            let mut args = vec![
                "-c".into(),
                "cli_auth_credentials_store=\"file\"".into(),
                "-c".into(),
                "forced_login_method=\"chatgpt\"".into(),
            ];
            args.extend_from_slice(command);
            (
                installed.executable().to_string_lossy().into_owned(),
                args,
                LaunchEnvironment::Exact(run_environment(host, &home)),
            )
        }
    };
    Ok(PreparedLaunch {
        program,
        arguments,
        environment,
        _claim: Some(Box::new(claim)),
    })
}
