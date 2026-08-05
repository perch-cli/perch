//! The Host implementation that actually touches the machine.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

use super::{Execution, Host, HostError, HttpRequest, HttpResponse, Platform};
#[cfg(unix)]
use super::{PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
use crate::keychain::{
    self, KeychainError, SECURITY_BIN, WritePath, classify, decode_password_output,
};

/// The `curl` binary. Perch shells out for the same reason it shells out to
/// `security` (ADR 0008): the machine already has one, and a linked HTTP client
/// would be a second TLS story to keep current.
///
/// Always by absolute path, because the path is a security property rather
/// than a convenience: `Command::new("curl")` would let anything earlier on
/// `PATH` receive an `Authorization: Bearer` header.
#[cfg(not(windows))]
fn curl_bin() -> Result<PathBuf, HostError> {
    Ok(PathBuf::from("/usr/bin/curl"))
}

/// The same on Windows, where `curl.exe` ships in `System32`. `%SystemRoot%`
/// comes from the environment rather than being hardcoded, because Windows
/// need not be installed at `C:\Windows` — and a machine that cannot say where
/// it is gets an error, not a walk of `PATH`.
#[cfg(windows)]
fn curl_bin() -> Result<PathBuf, HostError> {
    let root = std::env::var_os("SystemRoot")
        .filter(|root| !root.is_empty())
        .ok_or_else(|| {
            HostError::Other("SystemRoot is unset, so curl cannot be located".to_string())
        })?;
    Ok(PathBuf::from(root).join("System32").join("curl.exe"))
}

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
    let mut child = Command::new(curl_bin()?)
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

    fn home_dir(&self) -> Result<PathBuf, HostError> {
        home_from(HOME_VARIABLE, std::env::var_os(HOME_VARIABLE))
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    fn platform(&self) -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
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
            .and_then(|()| rename_replacing(&beside, path).map_err(HostError::Io));
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

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
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

    fn touch(&self, path: &Path) -> Result<(), HostError> {
        touch_now(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError> {
        rename_replacing(from, to)?;
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
        process_alive(pid)
    }

    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>> {
        process_started_at(pid)
    }

    fn sleep(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }

    fn is_interactive(&self) -> bool {
        use std::io::IsTerminal;
        // Both ends matter: a question needs somewhere to be shown as well as
        // somewhere to be answered from.
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
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

/// Moves a path over another, replacing it — `std::fs::rename`, everywhere it
/// is that simple.
#[cfg(not(windows))]
fn rename_replacing(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// The same on Windows, where a rename fails while anything holds a handle on
/// the target — routinely Windows Defender, transiently. The target here can
/// be `.claude.json`, the file the design goes out of its way not to lose, so
/// the failure is retried briefly and then reported exactly as it would have
/// been on the first try.
///
/// Both codes, because Windows reports the one phenomenon as either:
/// `ERROR_SHARING_VIOLATION` from some paths through the kernel, and
/// `ERROR_ACCESS_DENIED` from `MoveFileEx` replacing an open file — the code
/// CI actually observed. A genuine permission failure spends the half second
/// and then fails as it always did.
#[cfg(windows)]
fn rename_replacing(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};

    const ATTEMPTS: u32 = 10;
    const BETWEEN_MILLIS: u64 = 50;
    const TRANSIENT: [i32; 2] = [ERROR_SHARING_VIOLATION as i32, ERROR_ACCESS_DENIED as i32];

    let mut outcome = std::fs::rename(from, to);
    for _ in 1..ATTEMPTS {
        match &outcome {
            Err(err)
                if err
                    .raw_os_error()
                    .is_some_and(|code| TRANSIENT.contains(&code)) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(BETWEEN_MILLIS));
                outcome = std::fs::rename(from, to);
            }
            _ => break,
        }
    }
    outcome
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

/// When a process began, in the terms `/proc` speaks: field 22 of
/// `/proc/<pid>/stat` is the start in clock ticks since boot, and boot itself
/// is `btime` in `/proc/stat`, in seconds since the epoch.
///
/// Both figures round down, so the answer can only run early — the safe
/// direction, since an answer that ran late could make a genuine client look
/// like a recycled PID.
#[cfg(target_os = "linux")]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name sits in parentheses and may hold anything, spaces and
    // parentheses included; the numbered fields resume after the last ')'.
    let after_command = stat.rsplit_once(')')?.1;
    let ticks_after_boot: i64 = after_command.split_whitespace().nth(19)?.parse().ok()?;

    let boot: i64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;

    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    DateTime::from_timestamp_millis(boot * 1_000 + ticks_after_boot * 1_000 / ticks_per_second)
}

/// When a process began, as libproc reports it: the `proc_bsdinfo` for one
/// pid, whose `pbi_start_tvsec`/`pbi_start_tvusec` are the start as a
/// microsecond-resolution timestamp.
///
/// `proc_pidinfo` rather than `sysctl KERN_PROC_PID`, because the libc crate
/// carries a vetted declaration of the former for Apple and none of the
/// latter's `kinfo_proc` — and hand-writing that struct is precisely the
/// `unsafe` ADR 0021 exists to avoid.
#[cfg(target_os = "macos")]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;

    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    // The return is how many bytes were written; anything short of the whole
    // struct is a process that is gone, or one that will not be described.
    if written < size {
        return None;
    }

    let seconds = i64::try_from(info.pbi_start_tvsec).ok()?;
    DateTime::from_timestamp(seconds, u32::try_from(info.pbi_start_tvusec).ok()? * 1_000)
}

/// When a process began, as `GetProcessTimes` reports it: a creation `FILETIME`
/// counting 100-nanosecond ticks from 1601.
#[cfg(windows)]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The epoch, in milliseconds after the point `FILETIME` counts from.
    const EPOCH_MILLIS_AFTER_1601: i64 = 11_644_473_600_000;

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let told = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(process);
        if told == 0 {
            return None;
        }

        let ticks = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        DateTime::from_timestamp_millis((ticks / 10_000) as i64 - EPOCH_MILLIS_AFTER_1601)
    }
}

/// A platform with no way to ask says so, rather than guessing: an
/// uncorroborable marker becomes a refusal there, never a belief.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn process_started_at(_pid: u32) -> Option<DateTime<Utc>> {
    None
}

/// The one variable the home directory comes from — `USERPROFILE` on Windows
/// and `HOME` everywhere else, and never `HOME` on Windows even when it is
/// set: Git Bash sets it to `/c/Users/...` while PowerShell has
/// `C:\Users\...`, and a path Perch records from one shell has to resolve
/// from the other.
#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";
#[cfg(not(windows))]
const HOME_VARIABLE: &str = "HOME";

/// The home directory a variable names, or a refusal when it names none — an
/// unknown home must never quietly become the filesystem root, because
/// everything Perch reads and writes hangs off it.
fn home_from(variable: &str, value: Option<std::ffi::OsString>) -> Result<PathBuf, HostError> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            HostError::Other(format!(
                "{variable} is unset, so there is no home directory to work under"
            ))
        })
}

/// Whether a process exists. Signal 0 performs the permission and existence
/// checks without delivering anything.
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Whether a process exists, as `GetExitCodeProcess` tells it: a handle that
/// can be opened and reports no exit code yet is a running process.
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            // A process that exists but may not be opened still exists, and
            // "alive" is the safe direction: it makes a Profile look Live,
            // which stops Perch touching its Credential.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let told = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        told != 0 && code == STILL_ACTIVE as u32
    }
}

/// Marks a path as written now. `utimes` with no times is "now", which is the
/// whole of what a lock holder has to say — and it works on a directory, which
/// `File::set_times` cannot be relied on to.
#[cfg(unix)]
fn touch_now(path: &Path) -> Result<(), HostError> {
    let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|err| HostError::Other(format!("{} is not a path: {err}", path.display())))?;
    let outcome = unsafe { libc::utimes(raw.as_ptr(), std::ptr::null()) };
    if outcome == 0 {
        Ok(())
    } else {
        Err(HostError::Io(std::io::Error::last_os_error()))
    }
}

/// The same, in the terms Windows speaks: a directory's handle can only be
/// opened with `FILE_FLAG_BACKUP_SEMANTICS`, and `SetFileTime` then stamps the
/// modification time through it.
#[cfg(windows)]
fn touch_now(path: &Path) -> Result<(), HostError> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, OPEN_EXISTING, SetFileTime,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(HostError::Io(std::io::Error::last_os_error()));
        }

        let mut now: FILETIME = std::mem::zeroed();
        GetSystemTimeAsFileTime(&mut now);
        let stamped = SetFileTime(handle, std::ptr::null(), std::ptr::null(), &now);
        let failure = std::io::Error::last_os_error();
        CloseHandle(handle);
        if stamped == 0 {
            return Err(HostError::Io(failure));
        }
    }
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

    /// The holder here is what Windows Defender amounts to: a handle on the
    /// target, shared with nobody, gone a moment later.
    #[cfg(windows)]
    #[test]
    fn a_rename_outwaits_a_briefly_held_target() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = std::env::temp_dir().join(format!("perch-rename-retry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let from = dir.join("incoming");
        let to = dir.join("target");
        std::fs::write(&from, "new").unwrap();
        std::fs::write(&to, "old").unwrap();

        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&to)
            .unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            drop(held);
        });

        let outcome = rename_replacing(&from, &to);
        releaser.join().unwrap();
        let contents = std::fs::read_to_string(&to);
        let _ = std::fs::remove_dir_all(&dir);

        outcome.expect("a transient holder is outwaited rather than failed on");
        assert_eq!(contents.unwrap(), "new");
    }

    /// On a real filesystem, because this is the primitive the lock protocol
    /// leans on: a lock artifact is a **directory**, and its modification time
    /// is what says whether the holder is alive or died holding it. The
    /// platform split in `touch_now` (backup semantics on Windows) exists for
    /// exactly this case.
    #[test]
    fn touch_moves_a_directorys_modification_time_forward() {
        let host = RealHost::new();
        let dir = std::env::temp_dir().join(format!("perch-touch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let created = host.modified_at(&dir).unwrap();
        // Long enough that even a coarse filesystem timestamp has to move.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        host.touch(&dir).expect("a directory can be touched");
        let touched = host.modified_at(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(touched > created, "touched {touched}, created {created}");
    }

    #[test]
    fn an_unset_home_is_a_refusal_rather_than_the_filesystem_root() {
        assert!(home_from("HOME", None).is_err());
        assert!(home_from("USERPROFILE", Some("".into())).is_err());
        assert_eq!(
            home_from("HOME", Some("/Users/someone".into())).unwrap(),
            PathBuf::from("/Users/someone")
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
