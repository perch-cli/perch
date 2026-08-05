//! The Host implementation that actually touches the machine.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

use super::{
    Execution, Host, HostError, HttpRequest, HttpResponse, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE,
    Platform,
};
use crate::keychain::{
    self, KeychainError, SECURITY_BIN, WritePath, classify, decode_password_output,
};

/// The `curl` binary. Perch shells out for the same reason it shells out to
/// `security` (ADR 0008): the machine already has one, and a linked HTTP client
/// would be a second TLS story to keep current.
const CURL_BIN: &str = "/usr/bin/curl";

/// The options that are the same for every request, and none of which is a
/// secret. Everything that is one goes in on stdin instead.
const CURL_ARGS: [&str; 6] = [
    "--silent",
    "--show-error",
    "--write-out",
    "\n%{http_code}",
    "--config",
    "-",
];

/// The request as a `curl` configuration file, which is what goes in on stdin.
///
/// The URL, the headers and the body all arrive this way so that none of them
/// is ever an argument: an `Authorization` header holds an access token, and
/// argv is readable by every process on the machine.
fn curl_config(request: &HttpRequest<'_>) -> String {
    let mut config = format!("url = {}\n", quoted(request.url));
    for (name, value) in request.headers {
        config.push_str(&format!(
            "header = {}\n",
            quoted(&format!("{name}: {value}"))
        ));
    }
    // Giving `curl` data is what makes the request a POST; there is no verb to
    // set separately.
    if let Some(body) = request.body {
        config.push_str(&format!("data-binary = {}\n", quoted(body)));
    }
    config
}

/// A value as a `curl` configuration file quotes one.
///
/// Quoted so that spaces and `#` are part of the value rather than punctuation,
/// with backslashes and quotes escaped so a body full of JSON arrives as it was
/// written. A literal newline cannot appear inside one — Perch sends compact
/// JSON and single-line headers, so none ever does.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Runs `curl` with the request on its stdin.
fn curl(config: &str) -> Result<Execution, HostError> {
    let mut child = Command::new(CURL_BIN)
        .args(CURL_ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        use std::io::Write;
        let mut pipe = child.stdin.take().expect("stdin was piped");
        pipe.write_all(config.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    Ok(Execution {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

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

#[derive(Debug, Default)]
pub struct RealHost {
    /// What has already been said, so a remark about the machine is made once
    /// however many Accounts provoke it.
    noted: RefCell<BTreeSet<String>>,
}

impl RealHost {
    pub fn new() -> Self {
        RealHost::default()
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

    fn platform(&self) -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Other
        }
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

    /// Written beside and moved into place, so the file that ends up at `path`
    /// is one that was created 0600 rather than one that was tightened
    /// afterwards — even where something looser was already there (ADR 0020).
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            create_private_dir_all(parent)?;
        }

        let mut beside = path.as_os_str().to_os_string();
        beside.push(".perch-tmp");
        let beside = PathBuf::from(beside);

        let written = create_private_file(&beside, contents)
            .and_then(|()| std::fs::rename(&beside, path).map_err(HostError::Io));
        if written.is_err() {
            // A half-written Credential is not something to leave lying about,
            // whatever went wrong.
            let _ = std::fs::remove_file(&beside);
        }
        written
    }

    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError> {
        create_private_dir_all(path)
    }

    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(HostError::NotFound {
                    path: path.to_path_buf(),
                });
            }
            Err(err) => return Err(HostError::Io(err)),
        };
        Ok(mode_of(&metadata))
    }

    fn make_private(&self, path: &Path) -> Result<(), HostError> {
        set_private_mode(path)
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

    /// To stderr, so a note never lands in the middle of the JSON a script is
    /// reading off stdout.
    fn note(&self, line: &str) {
        if self.noted.borrow_mut().insert(line.to_string()) {
            eprintln!("perch: {line}");
        }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }

    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError> {
        let execution = curl(&curl_config(request))?;
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

/// Creates a directory, and every directory above it, that nobody but its
/// owner may enter.
///
/// The mode is given to `mkdir` rather than applied afterwards, so a directory
/// that will hold a Credential is never briefly open (ADR 0020).
#[cfg(unix)]
fn create_private_dir_all(path: &Path) -> Result<(), HostError> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir_all(path: &Path) -> Result<(), HostError> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

/// Creates a file with its mode, refusing to write into one that is already
/// there: an existing file's mode is whatever it was, and `open` would not
/// change it.
#[cfg(unix)]
fn create_private_file(path: &Path, contents: &str) -> Result<(), HostError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    // Anything already here is what a write that died left behind, and holding
    // on to it would mean writing a Credential into a file of unknown mode.
    let _ = std::fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_file(path: &Path, contents: &str) -> Result<(), HostError> {
    let _ = std::fs::remove_file(path);
    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(unix)]
fn mode_of(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode() & 0o777)
}

/// A platform that does not describe a file's privacy in permission bits says
/// nothing rather than something misleading.
#[cfg(not(unix))]
fn mode_of(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_private_mode(path: &Path) -> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_mode(_path: &Path) -> Result<(), HostError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_access_token_travels_on_stdin_rather_than_in_argv() {
        let headers = [("Authorization", "Bearer sk-ant-oat01-secret")];
        let config = curl_config(&HttpRequest::get("https://example.test/usage", &headers));
        let expected = "header = \"Authorization: Bearer sk-ant-oat01-secret\"";

        assert!(config.contains(expected), "{config}");
        assert!(
            !CURL_ARGS.iter().any(|arg| arg.contains("sk-ant")),
            "nothing about a request may reach the command line"
        );
    }

    #[test]
    fn a_json_body_survives_being_quoted() {
        let body = r#"{"refresh_token":"sk-ant-ort01-\"odd\""}"#;
        let config = curl_config(&HttpRequest::post("https://example.test/token", &[], body));
        let expected = r#"data-binary = "{\"refresh_token\":\"sk-ant-ort01-\\\"odd\\\"\"}""#;

        assert!(config.contains("url = \"https://example.test/token\""));
        assert!(config.contains(expected), "{config}");
        assert_eq!(config.lines().count(), 2, "one option per line: {config}");
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
