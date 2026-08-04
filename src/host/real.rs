//! The Host implementation that actually touches the machine.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

use super::{Execution, Host, HostError, HttpResponse};
use crate::keychain::{
    self, KeychainError, SECURITY_BIN, WritePath, classify, decode_password_output,
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

    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(HostError::AlreadyExists {
                    path: path.to_path_buf(),
                })
            }
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError> {
        match std::fs::metadata(path).and_then(|metadata| metadata.modified()) {
            Ok(modified) => Ok(DateTime::<Utc>::from(modified)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(HostError::NotFound {
                path: path.to_path_buf(),
            }),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    /// `utimes` with no times is "now", which is the whole of what a lock
    /// holder has to say. It works on a directory, which `File::set_times`
    /// cannot be relied on to.
    fn touch(&self, path: &Path) -> Result<(), HostError> {
        let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|err| HostError::Other(format!("{} is not a path: {err}", path.display())))?;
        let outcome = unsafe { libc_utimes(raw.as_ptr(), std::ptr::null()) };
        if outcome == 0 {
            Ok(())
        } else {
            Err(HostError::Io(std::io::Error::last_os_error()))
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError> {
        std::fs::rename(from, to)?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(HostError::NotFound {
                    path: path.to_path_buf(),
                });
            }
            Err(err) => return Err(HostError::Io(err)),
        };

        let mut found = Vec::new();
        for entry in entries {
            found.push(entry?.path());
        }
        found.sort();
        Ok(found)
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

    fn exec_interactive(&self, program: &str, env: &[(&str, &str)]) -> Result<i32, HostError> {
        let mut command = Command::new(program);
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        for (key, value) in env {
            command.env(key, value);
        }
        let status = command.status()?;
        Ok(status.code().unwrap_or(-1))
    }

    fn process_alive(&self, pid: u32) -> bool {
        // Signal 0 performs the permission and existence checks without
        // delivering anything.
        unsafe { libc_kill(pid as i32, 0) == 0 }
    }

    fn sleep(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }

    fn is_interactive(&self) -> bool {
        // Both ends matter: a question needs somewhere to be shown as well as
        // somewhere to be answered from.
        unsafe { libc_isatty(0) == 1 && libc_isatty(1) == 1 }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
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

unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;

    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;

    #[link_name = "utimes"]
    fn libc_utimes(path: *const std::ffi::c_char, times: *const std::ffi::c_void) -> i32;
}
