//! The Host implementation that actually touches the machine.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

use super::{Execution, Host, HostError, HttpResponse};
use crate::keychain::{
    self, classify, decode_password_output, KeychainError, WritePath, SECURITY_BIN,
};

/// Runs `security` and turns anything short of success into the distinction
/// that matters: not found, or locked and denied.
///
/// `security -i` reports a failed sub-command on stderr while still exiting 0,
/// so a clean exit is not on its own evidence that the item was written.
fn security(
    args: &[&str],
    stdin: Option<&str>,
    service: &str,
    account: &str,
) -> Result<Execution, KeychainError> {
    let execution =
        keychain::run_security(args, stdin).map_err(|err| KeychainError::Unavailable {
            detail: format!("could not run {SECURITY_BIN}: {err}"),
        })?;

    if execution.succeeded() && !execution.stderr.to_lowercase().contains("error") {
        Ok(execution)
    } else {
        Err(classify(&execution, service, account))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealHost;

impl RealHost {
    pub fn new() -> Self {
        RealHost
    }
}

impl Host for RealHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn home_dir(&self) -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    fn read_file(&self, path: &Path) -> Result<String, HostError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(contents),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(HostError::NotFound {
                path: path.to_path_buf(),
            }),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError> {
        let execution = security(
            &["find-generic-password", "-s", service, "-a", account, "-w"],
            None,
            service,
            account,
        )?;
        Ok(decode_password_output(&execution.stdout))
    }

    fn keychain_set(
        &self,
        service: &str,
        account: &str,
        secret: &str,
    ) -> Result<(), KeychainError> {
        let command_line = keychain::add_command_line(service, account, secret);
        match keychain::write_path_for(&command_line) {
            WritePath::Stdin => security(&["-i"], Some(&command_line), service, account)?,
            WritePath::Argv => {
                let hex = keychain::hex_encode(secret.as_bytes());
                let args = [
                    "add-generic-password",
                    "-U",
                    "-s",
                    service,
                    "-a",
                    account,
                    "-X",
                    &hex,
                ];
                security(&args, None, service, account)?
            }
        };
        Ok(())
    }

    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        security(
            &["delete-generic-password", "-s", service, "-a", account],
            None,
            service,
            account,
        )?;
        Ok(())
    }

    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()?;
        Ok(Execution {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn process_alive(&self, pid: u32) -> bool {
        // Signal 0 performs the permission and existence checks without
        // delivering anything.
        unsafe { libc_kill(pid as i32, 0) == 0 }
    }

    fn http_get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HostError> {
        let mut args: Vec<String> = vec![
            "--silent".into(),
            "--show-error".into(),
            "--write-out".into(),
            "\n%{http_code}".into(),
        ];
        for (name, value) in headers {
            args.push("-H".into());
            args.push(format!("{name}: {value}"));
        }
        args.push(url.to_string());

        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let execution = self.exec("/usr/bin/curl", &borrowed)?;
        if !execution.succeeded() {
            return Err(HostError::Other(format!(
                "curl exited {}: {}",
                execution.status,
                execution.stderr.trim()
            )));
        }

        let (body, code) = execution
            .stdout
            .rsplit_once('\n')
            .ok_or_else(|| HostError::Other("curl produced no status code".into()))?;
        Ok(HttpResponse {
            status: code.trim().parse().unwrap_or(0),
            body: body.to_string(),
        })
    }
}

extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
