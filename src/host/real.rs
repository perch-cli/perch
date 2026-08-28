//! The Host implementation that actually touches the machine.
//!
//! Two rules run through the whole file. A mode is set through a *handle* and
//! never through a name: `chmod(2)` on a path follows a link, so anything that
//! can write the directory can redirect it — and `CLAUDE_CONFIG_DIR` is taken
//! verbatim and can name a shared location. And every secret travels on stdin
//! rather than in `argv`, because the process table is readable by anything
//! running as the same user.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use zeroize::{Zeroize, Zeroizing};

#[cfg(unix)]
use super::PRIVATE_DIR_MODE;
use super::{
    Clock, Environment, Execution, Files, Filesystem, Host, HostError, HttpRequest, HttpResponse,
    Keys, Link, Links, Network, PRIVATE_FILE_MODE, Platform, Processes, Terminal, Waited, Waiting,
};
use crate::keychain::{
    self, KeychainError, SECURITY_BIN, WritePath, classify, decode_password_output,
};
use crate::secret::Secret;

/// The `curl` binary. Perch shells out for the same reason it shells out to
/// `security` (ADR a-crate-must-not-cost-a-seam), and always by absolute path,
/// because the path is a security property rather than a convenience:
/// `Command::new("curl")` would let anything earlier on `PATH` receive an
/// `Authorization: Bearer` header.
#[cfg(not(windows))]
fn curl_bin() -> Result<PathBuf, HostError> {
    const USUALLY: &str = "/usr/bin/curl";

    // The walk is reached only where the absolute path is absent — a machine
    // that does not populate `/usr/bin` — and there the only other answer is
    // not working.
    curl_at(
        Path::new(USUALLY),
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

/// The choice itself, given the two places to look. Split from the caller so a
/// test can drive all three answers: a machine with `curl` where it is expected
/// can never reach the branch that matters.
#[cfg(not(windows))]
fn curl_at(usually: &Path, path: &std::ffi::OsStr) -> Result<PathBuf, HostError> {
    if usually.is_file() {
        return Ok(usually.to_path_buf());
    }

    // Absolute directories only. An empty `PATH` element — `""`, or the one a
    // trailing `:` leaves — means the working directory, so a bare `curl` beside
    // whatever Perch was run in would be handed the `Authorization` header.
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("curl"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            HostError::Other(format!(
                "curl is not at {} and is not on PATH, so Perch cannot reach \
                 Anthropic. Install curl, or put it on PATH.",
                usually.display()
            ))
        })
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

/// The options that are the same for every request, none of which is a secret.
/// `-q` is first because it is only obeyed there: without it `curl` reads
/// `~/.curlrc`, which can set `proxy` and `insecure` — a machine-local way of
/// receiving an `Authorization: Bearer` header — and `output`, which diverts the
/// body to a file.
const CURL_ARGS: [&str; 7] = [
    "-q",
    "--silent",
    "--show-error",
    "--write-out",
    "\n%{http_code}",
    "--config",
    "-",
];

/// How long a request gets when it does not say. The two timeouts are what make
/// a hung endpoint a *refusal*: `perch watcher run` waits out every read in its
/// round, so a connection that is open and silent would stop it indefinitely.
/// In the configuration rather than in [`CURL_ARGS`], so that a request
/// carrying its own bound has one place to put it.
const CONNECT_TIMEOUT_SECONDS: u64 = 10;
const MAX_TIME_SECONDS: u64 = super::ORDINARY_BOUND_MILLIS / 1_000;

/// The other bound a reply needs. The timeouts make a *silent* endpoint a
/// refusal and neither makes a talkative one: a reply that keeps arriving is
/// buffered whole by `Command::output` and copied again into
/// `HttpResponse::body`. Four orders of magnitude above the largest thing Perch
/// reads, so this is defense in depth rather than a live exposure.
const MAX_REPLY_BYTES: u64 = 8 * 1024 * 1024;

/// The request as a `curl` configuration file, which is what goes in on stdin.
///
/// The URL, the headers and the body all arrive this way so that none of them
/// is ever an argument: an `Authorization` header holds an access token, and
/// argv is readable by every process on the machine.
fn curl_config(request: &HttpRequest<'_>) -> Result<Secret, HostError> {
    let quoted = super::write_double_quoted;

    // A configuration file is read a line at a time and has no escape a newline
    // could be quoted into, so a token carrying one would end the `header` line
    // and begin whatever the rest of it spelled.
    super::sendable(request)?;

    // Whole seconds, because that is the only unit `curl` takes here, and at
    // least one: a bound that rounded down to zero would mean *no* bound.
    let (connect, whole) = match request.within_millis {
        Some(millis) => {
            let seconds = millis.div_ceil(1000).max(1);
            (seconds, seconds)
        }
        None => (CONNECT_TIMEOUT_SECONDS, MAX_TIME_SECONDS),
    };
    // Every secret escaped straight in: a `String` of its own is a copy this
    // buffer's wipe does not reach.
    let mut config = Secret::with_room_for(width_of(request));
    let out = &mut config;
    // The only line holding no secret, so the one that may go through `format!`.
    out.push_str(&format!(
        "connect-timeout = {connect}\nmax-time = {whole}\nmax-filesize = {MAX_REPLY_BYTES}\n"
    ));
    out.push_str("url = ");
    quoted(out, request.url);
    out.push('\n');
    for (name, value) in request.headers {
        // One quoted token spanning both, because that is what `curl` reads a
        // `header` line as: the pair is escaped into it and never joined first.
        out.push_str("header = \"");
        super::write_escaped(out, name);
        out.push_str(": ");
        super::write_escaped(out, value);
        out.push_str("\"\n");
    }
    // Giving `curl` data is what makes the request a POST; there is no verb.
    if let Some(body) = request.body {
        out.push_str("data-binary = ");
        quoted(out, body);
        out.push('\n');
    }
    Ok(config)
}

/// Room for every line [`curl_config`] writes, over-counted rather than exact.
/// A count that came up short costs one copy — `Secret` wipes what it grew out
/// of — so this saves the copy rather than deciding whether a token leaks.
fn width_of(request: &HttpRequest<'_>) -> usize {
    const PER_LINE: usize = 32;
    /// Escaping doubles `\` and `"`, so a value made of nothing else is twice
    /// its own length once quoted. Counted at the worst case rather than
    /// measured, since measuring is the work the reservation exists to skip.
    fn quoted(value: &str) -> usize {
        2 * value.len() + 2
    }
    let headers: usize = request
        .headers
        .iter()
        .map(|(name, value)| quoted(name) + quoted(value) + PER_LINE)
        .sum();
    quoted(request.url) + headers + request.body.map_or(0, quoted) + 4 * PER_LINE
}

/// Runs a program, optionally with something on its stdin, and reads the whole
/// of what it said. The one place Perch spawns anything: stdin is where every
/// secret travels, so the choice between a pipe and `/dev/null` is one choice
/// made once.
fn run(program: &Path, args: &[&str], stdin: Option<&str>) -> std::io::Result<Execution> {
    // `Command::spawn`'s error carries no path, so the name is added here
    // rather than by each caller. The kind is kept, so anything matching on
    // `NotFound` still does.
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            std::io::Error::new(
                err.kind(),
                format!("could not run {}: {err}", program.display()),
            )
        })?;

    if let Some(input) = stdin {
        use std::io::Write;
        let mut pipe = child.stdin.take().expect("stdin was piped");
        let written = pipe.write_all(input.as_bytes());
        drop(pipe);
        // Reaped even when the write failed, because `Child::drop` neither
        // kills nor waits. Reported in the child's own words: the likeliest
        // failure is an `EPIPE` from one that has already exited.
        if let Err(err) = written {
            let said = child
                .wait_with_output()
                .map(|output| String::from_utf8_lossy(&output.stderr).trim().to_string())
                .unwrap_or_default();
            return Err(match said.is_empty() {
                true => err,
                false => std::io::Error::new(
                    err.kind(),
                    format!("could not write to {}: {err}: {said}", program.display()),
                ),
            });
        }
    }
    Ok(Execution::from(child.wait_with_output()?))
}

/// How a process that has finished ended, as the one number a caller can pass on
/// as its own exit code. A death by signal is 128 plus the signal, which is what
/// `$?` reports for that same death and so what anything wrapping `perch run`
/// already reads (ADR a-run-is-one-shot). The `-1` [`Execution`] uses is for the
/// executions Perch only ever asks `succeeded()` of.
#[cfg(unix)]
fn ended_as(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(-1)
}

/// Windows has no signals, so a process that has finished has a code.
#[cfg(not(unix))]
fn ended_as(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

/// Runs `curl` with the request on its stdin.
fn curl(config: &str) -> Result<Execution, HostError> {
    Ok(run(&curl_bin()?, &CURL_ARGS, Some(config))?)
}

/// Runs `security` and turns anything short of success into the distinction that
/// matters: not found, or locked and denied. `security -i` reports a failed
/// sub-command on stderr while still exiting 0, so a clean exit is not on its
/// own evidence that the item was written.
fn security(
    args: &[&str],
    stdin: Option<&str>,
    service: &str,
    account: &str,
) -> Result<Execution, KeychainError> {
    let execution =
        run(Path::new(SECURITY_BIN), args, stdin).map_err(|err| KeychainError::Unavailable {
            detail: format!("could not run {SECURITY_BIN}: {err}"),
        })?;

    // The stderr check belongs to `-i` and nothing else: every other invocation
    // says so with its exit code, and reading their stderr for a complaint
    // would turn a warning into a failure.
    let complained = args == ["-i"] && said_something_went_wrong(&execution.stderr);
    if execution.succeeded() && !complained {
        Ok(execution)
    } else {
        Err(classify(&execution, service, account))
    }
}

/// Whether `security` wrote a diagnostic of its own, matched on the `security:`
/// prefix rather than on the word "error" — its failure lines routinely carry no
/// such word, as in `security: -25299: The specified item already exists in the
/// keychain.`
fn said_something_went_wrong(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.trim_start().starts_with("security:"))
}

#[derive(Debug)]
pub struct RealHost {
    /// What has already been said, so a remark about the machine is made once
    /// however many Accounts provoke it.
    noted: RefCell<BTreeSet<String>>,
}

impl Default for RealHost {
    fn default() -> Self {
        RealHost::new()
    }
}

impl RealHost {
    pub fn new() -> Self {
        RealHost {
            noted: RefCell::new(BTreeSet::new()),
        }
    }
}

impl Clock for RealHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

impl Environment for RealHost {
    fn home_dir(&self) -> Result<PathBuf, HostError> {
        home_from(HOME_VARIABLE, std::env::var_os(HOME_VARIABLE))
    }

    fn current_dir(&self) -> Result<PathBuf, HostError> {
        std::env::current_dir().map_err(|err| HostError::Other(err.to_string()))
    }

    fn env_var(&self, key: &str) -> Option<String> {
        // Bytes rather than `std::env::var`, so a value that is present and not
        // text is told apart from one that is not there — still `None`, because
        // a path Perch cannot spell cannot be joined, but said out loud.
        let held = std::env::var_os(key)?;
        if held.is_empty() {
            return None;
        }
        match held.into_string() {
            Ok(value) => Some(value),
            Err(_) => {
                self.note(&format!(
                    "{key} is set to something that is not text, so Perch cannot \
                     read it and is carrying on as though it were unset."
                ));
                None
            }
        }
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

    /// Canonicalized, because `std::env::current_exe` on macOS hands back the
    /// path the process was launched with — for Homebrew `<prefix>/bin/perch`,
    /// which says nothing about a Cellar. A path that will not canonicalize is
    /// handed back as it came, which is less informative and better than
    /// refusing to run.
    fn current_exe(&self) -> Result<PathBuf, HostError> {
        let launched = std::env::current_exe()?;
        Ok(std::fs::canonicalize(&launched).unwrap_or(launched))
    }

    /// Linked rather than shelled out to: the whole of it is one `geteuid`, and
    /// `id -u` would be a process spawned for a question the C library holds.
    /// The *effective* uid rather than the real one, because that is the
    /// identity the filesystem judges every write by, and the one launchd files
    /// a session under.
    #[cfg(unix)]
    fn user_id(&self) -> Option<u32> {
        // SAFETY: `geteuid` takes nothing, cannot fail, and touches no memory.
        Some(unsafe { libc::geteuid() })
    }

    /// Windows has no uid: a logon task names the user it runs as, and there is
    /// nothing here to quote or to refuse.
    #[cfg(not(unix))]
    fn user_id(&self) -> Option<u32> {
        None
    }
}

/// A syscall's "no such file" as [`HostError::NotFound`], naming the path it was
/// asked about; anything else as itself. One copy, because every read here has
/// the same three arms and nine copies of a classification is how a tenth call
/// comes to disagree about what "not there" is: `credentials` reads
/// `NotFound` as "this store holds nothing" rather than as a broken machine.
fn or_not_found<T>(result: std::io::Result<T>, path: &Path) -> Result<T, HostError> {
    match result {
        Ok(value) => Ok(value),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(HostError::NotFound {
            path: path.to_path_buf(),
        }),
        Err(err) => Err(HostError::Io(err)),
    }
}

/// A `SystemTime` as the pair `DateTime::from_timestamp` takes, on either side
/// of the epoch. `None` where the offset does not fit an `i64` of seconds.
///
/// Every filesystem holding an `i64` of them takes a stamp no clock wrote, so
/// an age is a question with an answer rather than one that ends the process.
fn seconds_and_nanos_since_epoch(time: std::time::SystemTime) -> Option<(i64, u32)> {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => Some((i64::try_from(since.as_secs()).ok()?, since.subsec_nanos())),
        // Before the epoch, where `duration_since` reports the distance back to
        // it. The nanoseconds are borrowed from the second below, which is what
        // makes the pair one instant rather than two.
        Err(before) => {
            let back = before.duration();
            let seconds = i64::try_from(back.as_secs()).ok()?.checked_neg()?;
            match back.subsec_nanos() {
                0 => Some((seconds, 0)),
                nanos => Some((seconds.checked_sub(1)?, 1_000_000_000 - nanos)),
            }
        }
    }
}

/// The same, for the answer a lock turns on: something is already at this name.
/// `HostError::AlreadyExists` is what makes a lock a lock, so a `?` that
/// flattened `EEXIST` into `HostError::Io` here would answer differently from
/// the fake, which reports the variant.
fn or_already_exists<T>(result: std::io::Result<T>, path: &Path) -> Result<T, HostError> {
    match result {
        Ok(value) => Ok(value),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(HostError::AlreadyExists {
                path: path.to_path_buf(),
            })
        }
        Err(err) => Err(HostError::Io(err)),
    }
}

/// The same three arms where a missing file is not a failure at all: `None`.
/// What every removal here wants, since a path that is already gone is the state
/// the caller asked for — and what the two `remove_link`s want one step earlier,
/// to decide whether there is anything to inspect.
fn if_it_is_there<T>(result: std::io::Result<T>) -> Result<Option<T>, HostError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(HostError::Io(err)),
    }
}

impl Files for RealHost {
    fn read_file(&self, path: &Path) -> Result<String, HostError> {
        or_not_found(std::fs::read_to_string(path), path)
    }

    fn create_file_with_mode(
        &self,
        path: &Path,
        contents: &str,
        mode: u32,
    ) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        create_file_with_mode(path, contents, mode)
    }

    /// Written beside and moved into place, so the file that ends up at `path`
    /// was created 0600 rather than tightened afterwards — even where something
    /// looser was already there. All this adds to
    /// [`super::replace_via_tmp`] is the parent directory, which has to be
    /// private before the file lands in it.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            create_private_dir_all(parent)?;
        }
        super::replace_via_tmp(self, path, contents, PRIVATE_FILE_MODE)
    }

    fn append_private_line(&self, path: &Path, line: &str) -> Result<u64, HostError> {
        if let Some(parent) = path.parent() {
            create_private_dir_all(parent)?;
        }
        append_private_line(path, line, PRIVATE_FILE_MODE)
    }

    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError> {
        create_private_dir_all(path)
    }

    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError> {
        let metadata = or_not_found(std::fs::metadata(path), path)?;
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
        if_it_is_there(std::fs::remove_dir_all(path)).map(|_| ())
    }

    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(HostError::AlreadyExists {
                    path: path.to_path_buf(),
                })
            }
            // A parent that is not there is `ENOENT`, which is the other
            // variant this port names — and named for the *parent*, because the
            // path itself is certainly absent and saying so would be no news.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(HostError::NotFound {
                path: path.parent().unwrap_or(path).to_path_buf(),
            }),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError> {
        let modified = or_not_found(
            std::fs::metadata(path).and_then(|metadata| metadata.modified()),
            path,
        )?;
        // Not `DateTime::<Utc>::from`, which panics on a stamp outside chrono's
        // range where this port answers with a `Result`.
        seconds_and_nanos_since_epoch(modified)
            .and_then(|(seconds, nanos)| DateTime::from_timestamp(seconds, nanos))
            .ok_or_else(|| {
                HostError::Other(format!(
                    "{} was last modified at a time outside the range Perch can \
                     represent, so its age cannot be read",
                    path.display()
                ))
            })
    }

    fn touch(&self, path: &Path) -> Result<(), HostError> {
        touch_now(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError> {
        rename_replacing(from, to)?;
        // The bytes are durable by the time anything is renamed over another
        // file here; this is what makes the new name durable too.
        sync_directory_of(to);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        if_it_is_there(std::fs::remove_file(path)).map(|_| ())
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        let entries = or_not_found(std::fs::read_dir(path), path)?;

        let mut found = Vec::new();
        for entry in entries {
            found.push(entry?.path());
        }
        found.sort();
        Ok(found)
    }
}

impl Links for RealHost {
    fn link(&self, kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
        make_link(kind, target, at)
    }

    /// `symlink_metadata` rather than `read_link` alone, so the three answers
    /// stay three — a link, something that is not a link, and nothing there.
    /// `read_link` collapses the last two into one error.
    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        let metadata = or_not_found(std::fs::symlink_metadata(path), path)?;
        if !metadata.file_type().is_symlink() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_link(path)?))
    }

    fn remove_link(&self, path: &Path) -> Result<(), HostError> {
        remove_link(path)
    }
}

impl Keys for RealHost {
    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError> {
        let mut execution = security(
            &["find-generic-password", "-s", service, "-a", account, "-w"],
            None,
            service,
            account,
        )?;
        // Taken out of the `Execution` and wiped there: `security -w` answers
        // with the Credential on stdout, and an `Execution` would drop that
        // buffer back to the allocator untouched.
        let mut stdout = std::mem::take(&mut execution.stdout);
        let decoded = decode_password_output(&stdout);
        stdout.zeroize();
        Ok(decoded)
    }

    fn keychain_set(
        &self,
        service: &str,
        account: &str,
        secret: &str,
    ) -> Result<(), KeychainError> {
        let command_line = keychain::add_command_line(service, account, secret)?;
        match keychain::write_path_for(&command_line) {
            WritePath::Stdin => security(&["-i"], Some(&command_line), service, account)?,
            WritePath::Argv => {
                // The one time a Credential reaches `argv`, and so the process
                // table. Said out loud, because an invariant with a silent
                // exception is not one.
                self.note(
                    "A Credential was too large for `security`'s stdin buffer, so it was \
                     given to it as a command-line argument instead. While that ran, any \
                     process on this machine running as you could have read it off the \
                     process table.",
                );
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
}

impl Processes for RealHost {
    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError> {
        Ok(run(Path::new(program), args, None)?)
    }

    fn exec_interactive(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<i32, HostError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        for (key, value) in env {
            command.env(key, value);
        }
        // Ctrl-C reaches every process in the foreground group, and belongs to
        // the child while it runs. The child must not *inherit* the ignoring:
        // `SIG_IGN` survives `exec`, so it would never see Ctrl-C at all.
        #[cfg(unix)]
        let guarding = {
            use std::os::unix::process::CommandExt;

            // SAFETY: `signal` is async-signal-safe and is the whole of what
            // this closure calls, which is the rule between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    libc::signal(libc::SIGINT, libc::SIG_DFL);
                    libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                    Ok(())
                });
            }

            // SAFETY: replacing this process's own dispositions with `SIG_IGN`,
            // keeping what was there to put back below.
            let guarding = [libc::SIGINT, libc::SIGQUIT];
            (
                guarding,
                guarding.map(|signal| unsafe { libc::signal(signal, libc::SIG_IGN) }),
            )
        };

        // Named here for the reason `run` names it: `Command::status`'s error
        // carries no path either, and a `claude` uninstalled between being
        // found and being launched is a real state.
        let ran = command.status();

        // However the launch ended, including the one that never started.
        // `SIG_ERR` is not a disposition but what `signal` answers when it could
        // not install one, so handing it back installs an invalid handler.
        #[cfg(unix)]
        // SAFETY: restoring exactly the dispositions `signal` reported above.
        unsafe {
            let (guarding, previously) = guarding;
            for (signal, was) in guarding.into_iter().zip(previously) {
                if was != libc::SIG_ERR {
                    libc::signal(signal, was);
                }
            }
        }

        let status = ran.map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not run {program}: {err}"))
        })?;
        Ok(ended_as(status))
    }

    fn process_id(&self) -> u32 {
        std::process::id()
    }

    fn process_alive(&self, pid: u32) -> bool {
        super::is_a_pid(pid) && process_alive(pid)
    }

    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>> {
        super::is_a_pid(pid)
            .then(|| process_started_at(pid))
            .flatten()
    }
}

impl Waiting for RealHost {
    fn sleep(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }

    fn asked_to_stop(&self) -> bool {
        interrupted()
    }

    fn listen_for_interrupts(&self) {
        listen_for_interrupts();
    }

    /// On something the handler can wake rather than in slices that check a flag
    /// between naps: this is where a Service spends its life, and a wait of two
    /// and a half minutes was fifteen hundred wakeups.
    fn wait(&self, millis: u64) -> Waited {
        // Asked before the wait as well as after, so an interrupt that landed
        // while the round was working is not one this wait sits through.
        if interrupted() {
            return Waited::Interrupted;
        }
        waited_out(millis);
        match interrupted() {
            true => Waited::Interrupted,
            false => Waited::Fully,
        }
    }
}

impl Terminal for RealHost {
    fn is_interactive(&self) -> bool {
        use std::io::IsTerminal;
        // Both ends: a question needs somewhere to be shown as well as
        // somewhere to be answered from.
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    }

    /// To stderr, so a note never lands in the middle of the JSON a script is
    /// reading off stdout — and once each, however many Accounts provoke it.
    fn note(&self, line: &str) {
        if self.noted.borrow_mut().insert(line.to_string()) {
            eprintln!("perch: {}", super::Shown::in_prose(line));
        }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        read_a_line()
    }

    fn read_secret(&self) -> Result<Option<Zeroizing<String>>, HostError> {
        read_without_echo()
    }
}

impl Network for RealHost {
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError> {
        let mut execution = curl(&curl_config(request)?)?;
        // Taken out before the status is read, because curl can write the
        // rotated refresh token this body carries and *then* trip its own
        // `max-time`, and an `Execution` drops it to the allocator whole.
        let stdout = Zeroizing::new(std::mem::take(&mut execution.stdout));
        if !execution.succeeded() {
            return Err(HostError::Other(format!(
                "curl exited {}: {}",
                execution.status,
                execution.stderr.trim()
            )));
        }
        split_reply(stdout)
    }
}

impl Filesystem for RealHost {}

impl Host for RealHost {}

/// The body and the status code out of what `curl` wrote. Apart from the caller
/// so it can be asserted on, since `FakeHost::http` answers with a
/// `HttpResponse` already built. Takes the buffer rather than borrowing it: the
/// body is what curl wrote less its trailing status, so it is that buffer
/// truncated, where a second one holds a second copy of the whole reply.
fn split_reply(mut stdout: Zeroizing<String>) -> Result<HttpResponse, HostError> {
    // Safe because `Zeroizing` wipes the whole capacity rather than the live
    // length, so the status digits now past the end still go.
    let (body, status) = status_after(&stdout)?;
    stdout.truncate(body);
    Ok(HttpResponse {
        status,
        body: stdout,
    })
}

/// Where the body ends and what status `curl` appended after it. A status code
/// that will not parse is said rather than read as zero, which is a status no
/// server sends about a request that may never have been made.
fn status_after(stdout: &str) -> Result<(usize, u16), HostError> {
    let (body, code) = stdout
        .rsplit_once('\n')
        .ok_or_else(|| HostError::Other("curl produced no status code".into()))?;
    let status = code.trim().parse().map_err(|_| {
        HostError::Other(format!(
            "curl reported `{}` where a status code was expected",
            code.trim()
        ))
    })?;
    Ok((body.len(), status))
}

/// One line from standard input, or `None` at end of it. Through the same reader
/// the secret prompt uses, because one descriptor has one reader: a buffered
/// reader beside an unbuffered one swallows bytes the unbuffered one never sees,
/// and `perch holdings purge` asks a word, then a path, then a passphrase.
fn read_a_line() -> Result<Option<String>, HostError> {
    Ok(a_line_from(one_byte_of_standard_input)?.map(|line| line.to_string()))
}

/// One line from wherever the bytes come from, into one buffer reserved up
/// front, grown only by hand — wiping what it abandons — and trimmed in place.
/// `read_line` into a `String` is two copies: it grows by reallocating, and a
/// trim after it allocates afresh, so a long passphrase leaves fragments of
/// itself in freed heap.
fn a_line_from(
    mut next: impl FnMut() -> Result<Option<u8>, HostError>,
) -> Result<Option<Zeroizing<String>>, HostError> {
    // Longer than any passphrase anybody types, so the growth below guards
    // against being wrong rather than being the ordinary path.
    const ROOM: usize = 512;

    let mut bytes = Zeroizing::new(Vec::with_capacity(ROOM));
    let mut anything = false;
    while let Some(byte) = next()? {
        anything = true;
        if byte == b'\n' {
            break;
        }
        if bytes.len() == bytes.capacity() {
            // `Vec` would copy into the new allocation and free the old one
            // untouched, so the move is made here and the buffer left behind is
            // wiped.
            let mut grown = Vec::with_capacity(bytes.capacity() * 2);
            grown.extend_from_slice(&bytes);
            let mut abandoned = std::mem::replace(&mut *bytes, grown);
            abandoned.zeroize();
        }
        bytes.push(byte);
    }
    if !anything {
        return Ok(None);
    }

    // In place, so the trimmed bytes are gone rather than left behind a
    // shorter copy of themselves.
    while bytes.last() == Some(&b'\r') {
        bytes.pop();
    }

    let text = std::str::from_utf8(&bytes)
        .map_err(|err| HostError::Other(format!("what was typed is not text: {err}")))?;
    let mut secret = Zeroizing::new(String::with_capacity(text.len()));
    secret.push_str(text);
    Ok(Some(secret))
}

/// One byte of standard input, straight off the descriptor rather than through
/// `stdin().lock()`, whose 8 KiB `BufReader` belongs to the process: read that
/// way, the passphrase stays in a buffer nothing wipes for the rest of the run.
#[cfg(unix)]
fn one_byte_of_standard_input() -> Result<Option<u8>, HostError> {
    one_byte_of(libc::STDIN_FILENO)
}

/// The read itself, off whichever descriptor. Split from the caller so a test
/// can drive it: standard input is not something a test may take over, since the
/// behavior suite links this library and a `dup2` onto fd 0 would reach into
/// every other test in the process.
#[cfg(unix)]
fn one_byte_of(fd: libc::c_int) -> Result<Option<u8>, HostError> {
    let mut byte = 0u8;
    // SAFETY: a one-byte buffer this frame owns, and a descriptor the caller
    // holds open for the duration of the call.
    let read = unsafe { libc::read(fd, std::ptr::from_mut(&mut byte).cast::<libc::c_void>(), 1) };
    match read {
        -1 => Err(HostError::Io(std::io::Error::last_os_error())),
        0 => Ok(None),
        _ => Ok(Some(byte)),
    }
}

/// The same on Windows, which has no descriptor to read and so goes through the
/// buffered handle. The copy that leaves in `std`'s own buffer is a gap the unix
/// arm does not have, and there is no unbuffered handle to ask for.
#[cfg(windows)]
fn one_byte_of_standard_input() -> Result<Option<u8>, HostError> {
    use std::io::Read;

    let mut byte = [0u8; 1];
    match std::io::stdin().lock().read(&mut byte)? {
        0 => Ok(None),
        _ => Ok(Some(byte[0])),
    }
}

/// Whether the terminal was echoing before Perch turned it off, and so whether
/// an interrupted read owes it an `ECHO` back on. A static, because the only
/// thing that can act between the two `tcsetattr` calls is a signal handler and
/// a handler reaches nothing else. Asked rather than assumed, so a terminal
/// already in `stty -echo` is left as somebody left it.
static WAS_ECHOING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The same, with the terminal not showing what is typed: `ECHO` off, one line,
/// `ECHO` back on. Restored however the read ended, Ctrl-C included — a terminal
/// left with its echo off outlives the process that turned it off, so the window
/// is held with a handler that repairs the terminal and re-raises, which is four
/// async-signal-safe calls and a parent that still sees a death by SIGINT.
#[cfg(unix)]
fn read_without_echo() -> Result<Option<Zeroizing<String>>, HostError> {
    use std::os::fd::AsRawFd;
    use std::sync::atomic::Ordering::Relaxed;

    /// Puts the echo back, then dies of the signal as it would have anyway.
    unsafe extern "C" fn show_again_and_stop(signal: libc::c_int) {
        if WAS_ECHOING.load(Relaxed) {
            // SAFETY: a `termios` owned by this frame and this process's own
            // standard input; both calls are async-signal-safe. `TCSANOW`,
            // because there is no read left for pending input to confuse.
            unsafe {
                let mut mode: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut mode) == 0 {
                    mode.c_lflag |= libc::ECHO;
                    libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &mode);
                }
            }
        }
        // SAFETY: both async-signal-safe. The default disposition is put back
        // first, so the signal raised next ends the process rather than
        // arriving here again.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    let terminal = std::io::stdin().as_raw_fd();
    let mut showing: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `showing` is a `termios` this call fills in, and the descriptor is
    // this process's own standard input.
    if unsafe { libc::tcgetattr(terminal, &mut showing) } != 0 {
        return Err(HostError::Io(std::io::Error::last_os_error()));
    }

    WAS_ECHOING.store(showing.c_lflag & libc::ECHO != 0, Relaxed);
    // Installed before the echo goes off, so there is no instant where it is off
    // and nothing would put it back. Both signals, because Ctrl-\ kills too.
    // SAFETY: four async-signal-safe calls and a store to an atomic.
    let guarding = [libc::SIGINT, libc::SIGQUIT];
    let previously = guarding.map(|signal| unsafe {
        libc::signal(
            signal,
            show_again_and_stop as *const () as libc::sighandler_t,
        )
    });

    let mut hiding = showing;
    hiding.c_lflag &= !libc::ECHO;
    // `TCSAFLUSH`, because anything typed ahead of the question was typed while
    // the terminal was still echoing.
    // SAFETY: `hiding` is the mode just read back with one flag cleared.
    let hidden = unsafe { libc::tcsetattr(terminal, libc::TCSAFLUSH, &hiding) } == 0;

    let typed = if hidden {
        a_line_from(one_byte_of_standard_input)
    } else {
        Err(HostError::Io(std::io::Error::last_os_error()))
    };

    // SAFETY: restoring what `tcgetattr` and `signal` reported, however the
    // read ended. `SIG_ERR` is what `signal` answers when it could not install
    // one, so that arm leaves the original disposition alone.
    unsafe {
        libc::tcsetattr(terminal, libc::TCSAFLUSH, &showing);
        for (signal, was) in guarding.into_iter().zip(previously) {
            if was != libc::SIG_ERR {
                libc::signal(signal, was);
            }
        }
    }
    typed
}

/// The same on Windows, where the console has one mode word and
/// `ENABLE_ECHO_INPUT` is the bit in it. Ctrl-C is guarded by a console handler
/// claimed for the length of the read; it answers `FALSE`, so the default
/// handler still ends the process and all this one does is put the echo back
/// first.
#[cfg(windows)]
fn read_without_echo() -> Result<Option<Zeroizing<String>>, HostError> {
    use std::sync::atomic::Ordering::Relaxed;
    use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE, TRUE};
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle,
        STD_INPUT_HANDLE, SetConsoleCtrlHandler, SetConsoleMode,
    };

    /// Puts the echo back, and claims nothing: `FALSE` leaves the event to the
    /// default handler, which ends the process as it always did.
    unsafe extern "system" fn show_again(event: u32) -> windows_sys::core::BOOL {
        if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) && WAS_ECHOING.load(Relaxed) {
            // SAFETY: a handle this process owns and a mode word owned by this
            // frame, which is the whole of what the calls touch.
            unsafe {
                let console = GetStdHandle(STD_INPUT_HANDLE);
                let mut mode = 0u32;
                if console != INVALID_HANDLE_VALUE
                    && !console.is_null()
                    && GetConsoleMode(console, &mut mode) != 0
                {
                    SetConsoleMode(console, mode | ENABLE_ECHO_INPUT);
                }
            }
        }
        FALSE
    }

    // SAFETY: every call here takes a handle this process owns and a mode word
    // owned by this frame. The handler reads its argument and an atomic, and
    // stays valid for the life of the process.
    unsafe {
        let console = GetStdHandle(STD_INPUT_HANDLE);
        if console == INVALID_HANDLE_VALUE || console.is_null() {
            return Err(HostError::Io(std::io::Error::last_os_error()));
        }

        let mut showing = 0u32;
        if GetConsoleMode(console, &mut showing) == 0 {
            return Err(HostError::Io(std::io::Error::last_os_error()));
        }

        WAS_ECHOING.store(showing & ENABLE_ECHO_INPUT != 0, Relaxed);
        // Claimed before the echo goes off, so there is no instant where it is
        // off and nothing would put it back.
        SetConsoleCtrlHandler(Some(show_again), TRUE);

        let hidden = SetConsoleMode(console, showing & !ENABLE_ECHO_INPUT) != 0;
        let typed = if hidden {
            a_line_from(one_byte_of_standard_input)
        } else {
            Err(HostError::Io(std::io::Error::last_os_error()))
        };

        SetConsoleMode(console, showing);
        SetConsoleCtrlHandler(Some(show_again), FALSE);
        typed
    }
}

/// A platform with no way to stop the terminal showing what is typed refuses
/// rather than showing it. Perch runs on three platforms and two of them are
/// above; this is the branch that keeps the third from silently printing an
/// export passphrase into somebody's scrollback.
#[cfg(not(any(unix, windows)))]
fn read_without_echo() -> Result<Option<Zeroizing<String>>, HostError> {
    Err(HostError::Other(
        "this platform has no way to stop the terminal showing what is typed, \
         and a passphrase must never be shown"
            .to_string(),
    ))
}

/// Moves a path over another, replacing it — `std::fs::rename`, everywhere it
/// is that simple.
#[cfg(not(windows))]
fn rename_replacing(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// The same on Windows, where a rename fails while anything holds a handle on
/// the target — routinely Windows Defender, transiently — so it is retried
/// briefly and then reported as it would have been on the first try. Both
/// codes, because Windows reports the one phenomenon as either
/// `ERROR_SHARING_VIOLATION` or `ERROR_ACCESS_DENIED`.
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

/// Creates a directory, and every directory above it, that nobody but its owner
/// may enter. The mode is given to `mkdir` rather than applied afterwards, so a
/// directory that will hold a Credential is never briefly open.
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

/// Creates a file with its mode, never writing *into* one already there:
/// anything at the name is unlinked first and the file created afresh with
/// `O_EXCL`, so it cannot be left at whatever mode it had. Synced before it is
/// closed, because a rename is atomic against other *processes* and not against
/// a crash — it can land while the data blocks have not.
#[cfg(unix)]
fn create_file_with_mode(path: &Path, contents: &str, mode: u32) -> Result<(), HostError> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    // Anything already here is what a write that died left behind, and keeping
    // it would mean writing into a file of unknown mode.
    let _ = std::fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    // `open`'s mode is what the file is created *at most* as, since the kernel
    // takes the umask out of it. Safe here and nowhere later: the file is
    // `O_EXCL` and still empty, so an instant of widening reveals nothing.
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// The same where permission bits mean nothing: the mode is not a thing that
/// can be asked for, so the file is simply created afresh.
#[cfg(not(unix))]
fn create_file_with_mode(path: &Path, contents: &str, _mode: u32) -> Result<(), HostError> {
    use std::io::Write;

    let _ = std::fs::remove_file(path);
    let mut file = std::fs::File::create_new(path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// Adds a line to the end of a file, creating it at `mode` if it is not there.
/// `O_APPEND`, so taking the offset and writing is one operation the kernel does
/// not interleave. The newline goes out *in* that write rather than after it:
/// two calls are two appends, and a Watcher landing between them merges its line
/// into somebody else's and leaves a blank one behind.
#[cfg(unix)]
fn append_private_line(path: &Path, line: &str, mode: u32) -> Result<u64, HostError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(mode)
        .open(path)?;
    file.write_all(with_its_newline(line).as_bytes())?;
    Ok(file.metadata()?.len())
}

/// One buffer holding the line and the newline that ends it, reserved at the
/// width it takes so the copy is the only one.
fn with_its_newline(line: &str) -> String {
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    record
}

/// The same where permission bits mean nothing, the file being covered by the
/// profile's own ACL.
#[cfg(not(unix))]
fn append_private_line(path: &Path, line: &str, _mode: u32) -> Result<u64, HostError> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    file.write_all(with_its_newline(line).as_bytes())?;
    Ok(file.metadata()?.len())
}

/// Makes a link, where a symbolic link is a symbolic link and a junction is
/// something this platform has never heard of.
#[cfg(not(windows))]
fn make_link(kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
    match kind {
        Link::Symbolic => or_already_exists(std::os::unix::fs::symlink(target, at), at),
        Link::Hard => or_already_exists(std::fs::hard_link(target, at), at),
        Link::Junction => Err(super::junctions_are_windows_only()),
    }
}

/// The same on Windows, where the kinds actually differ. A symbolic link needs
/// Developer Mode or elevation and fails without either, which is the failure
/// the other two kinds exist to be tried after. A junction is a reparse point
/// the standard library does not write, so that one is the `junction` crate's.
#[cfg(windows)]
fn make_link(kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
    match kind {
        // Which of the two symlink calls is the target's business: Windows
        // records in the link whether it names a directory, and a file symlink
        // pointing at a directory does not resolve.
        Link::Symbolic if target.is_dir() => {
            or_already_exists(std::os::windows::fs::symlink_dir(target, at), at)
        }
        Link::Symbolic => or_already_exists(std::os::windows::fs::symlink_file(target, at), at),
        Link::Junction => or_already_exists(junction::create(target, at), at),
        Link::Hard => or_already_exists(std::fs::hard_link(target, at), at),
    }
}

/// Removes a link and nothing else, checked rather than assumed: a bare
/// `remove_file` would take away the person's file at the same name, and the
/// caller's `link_target` is a separate syscall. A symbolic link and no other
/// kind, because `reconcile::make` offers a hard link on Windows alone and a
/// hard link is a name for the file that cannot be told from one.
#[cfg(not(windows))]
fn remove_link(path: &Path) -> Result<(), HostError> {
    let Some(metadata) = if_it_is_there(std::fs::symlink_metadata(path))? else {
        return Ok(());
    };
    if !metadata.file_type().is_symlink() {
        return Err(super::not_a_link(path));
    }
    if_it_is_there(std::fs::remove_file(path)).map(|_| ())
}

/// The same on Windows, where which call removes a link depends on whether it
/// names a directory. Read off the attributes rather than off `FileType`, which
/// reports a junction as a symlink and not as a directory — and the refusal
/// turns on a reparse point rather than a symlink, which is the one thing both
/// kinds Windows makes have in common.
#[cfg(windows)]
fn remove_link(path: &Path) -> Result<(), HostError> {
    use std::os::windows::fs::MetadataExt;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let Some(metadata) = if_it_is_there(std::fs::symlink_metadata(path))? else {
        return Ok(());
    };

    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Err(super::not_a_link(path));
    }

    let removed = if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    };
    if_it_is_there(removed).map(|_| ())
}

/// Whether the person at the terminal has asked a loop to stop. A `static`
/// rather than something the Host owns, because the handler that sets it is the
/// operating system's to call and is handed no context to find a Host through —
/// and a single atomic flag is the whole of what a signal handler may touch.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn interrupted() -> bool {
    INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Takes Ctrl-C — and the signal a service manager stops a process with — over
/// from the default handler. **`SIGTERM` as well as `SIGINT`, and this is what a
/// Service rests on** (ADR the-machine-runs-the-watcher): unhandled, a
/// `systemctl --user stop` mid-Switch leaves a Landing nobody wrote down. It
/// stands down after one signal, so a **second** of either kills the process.
#[cfg(unix)]
fn listen_for_interrupts() {
    unsafe extern "C" fn stop(signal: libc::c_int) {
        INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
        let end = WOKEN[1].load(std::sync::atomic::Ordering::Relaxed);
        if end >= 0 {
            // SAFETY: `write` is async-signal-safe, and the descriptor is one
            // this process opened and never closes. A full pipe or a short
            // write costs nothing: the flag above is what is read.
            let _ = unsafe { libc::write(end, [0u8].as_ptr().cast(), 1) };
        }
        // Only the signal that arrived is stood down, so a first Ctrl-C followed
        // by a `systemctl stop` still kills a wedged loop.
        // SAFETY: `signal` is async-signal-safe and installs the default.
        unsafe { libc::signal(signal, libc::SIG_DFL) };
    }

    open_the_pipe();
    // SAFETY: the handler stores to an atomic, writes a byte to a pipe and
    // restores the default disposition, all three async-signal-safe. `signal`
    // returns the previous handler, which nothing here needs.
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
}

/// The ends of the pipe a handler writes to, or `-1` where none is open.
///
/// A pipe rather than a condition variable, which a handler may not lock, and
/// rather than the signal alone cutting the sleep short: `nanosleep` reports how
/// much was left and the standard library goes back round for the rest.
#[cfg(unix)]
static WOKEN: [std::sync::atomic::AtomicI32; 2] = [
    std::sync::atomic::AtomicI32::new(-1),
    std::sync::atomic::AtomicI32::new(-1),
];

/// Opens it, once. Close-on-exec so a Run's client does not inherit two
/// descriptors it has no use for, and non-blocking so a handler firing more
/// often than a wait drains it still returns.
#[cfg(unix)]
fn open_the_pipe() {
    let mut ends = [-1 as libc::c_int; 2];
    // SAFETY: `pipe` writes two descriptors into the array it is handed, and
    // writes none where it fails.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return;
    }
    for end in ends {
        // SAFETY: each is a descriptor `pipe` has just answered with.
        unsafe {
            libc::fcntl(end, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(end, libc::F_SETFL, libc::O_NONBLOCK);
        }
    }
    WOKEN[0].store(ends[0], std::sync::atomic::Ordering::Relaxed);
    WOKEN[1].store(ends[1], std::sync::atomic::Ordering::Relaxed);
}

/// Waits out `millis`, or until a handler writes to the pipe.
///
/// Round again on a `poll` cut short, because the only handler installed here
/// sets the flag the caller reads next — so a return with it unset is a signal
/// belonging to somebody else and the wait it interrupted is still owed.
#[cfg(unix)]
fn waited_out(millis: u64) {
    let end = WOKEN[0].load(std::sync::atomic::Ordering::Relaxed);
    if end < 0 {
        std::thread::sleep(std::time::Duration::from_millis(millis));
        return;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || interrupted() {
            return;
        }
        let mut watched = libc::pollfd {
            fd: end,
            events: libc::POLLIN,
            revents: 0,
        };
        // `poll` counts in milliseconds and takes an `int`, which tops out at
        // about twenty-four days — far past the longest wait the watcher takes.
        let capped = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: one initialized `pollfd`, naming a descriptor this process
        // owns, and a count matching the array it is handed.
        let answered = unsafe { libc::poll(&mut watched, 1, capped) };
        if answered > 0 {
            return;
        }
        // `EINTR` is the cut-short this rounds again on. Any other failure is
        // one nothing here can mend, and rounding on it would spin — so the
        // wait that is still owed is slept out instead.
        if answered < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            std::thread::sleep(left);
            return;
        }
    }
}

/// The same on Windows, where Ctrl-C is a console event on a thread of its own.
/// Only the two events that mean "stop this program" are claimed, and only the
/// first of them. A console being closed or a user logging out is never claimed
/// at all: those are not requests to finish what you were doing, and Windows
/// kills a handler that claims them seconds later anyway.
#[cfg(windows)]
fn listen_for_interrupts() {
    use windows_sys::Win32::Foundation::{FALSE, HANDLE, TRUE};
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
    };
    use windows_sys::Win32::System::Threading::SetEvent;

    unsafe extern "system" fn stop(event: u32) -> windows_sys::core::BOOL {
        match event {
            CTRL_C_EVENT | CTRL_BREAK_EVENT
                if !INTERRUPTED.swap(true, std::sync::atomic::Ordering::Relaxed) =>
            {
                let woken = WOKEN.load(std::sync::atomic::Ordering::Relaxed);
                if woken != 0 {
                    // SAFETY: an event handle this process created and never
                    // closes. A failure leaves the wait to run its timeout out,
                    // which is what it did before there was an event at all.
                    unsafe { SetEvent(woken as HANDLE) };
                }
                TRUE
            }
            _ => FALSE,
        }
    }

    open_the_event();
    // SAFETY: the handler stores to an atomic, signals an event and reads its
    // argument, and stays valid for the life of the process. A registration that
    // fails leaves the default handler in place, which is every other command's.
    unsafe {
        SetConsoleCtrlHandler(Some(stop), TRUE);
    }
}

/// The event a handler signals, or nought where none is open.
///
/// Manual reset, because the flag beside it is never cleared either: once the
/// loop has been asked to stop, every wait after it is one that ends at once.
#[cfg(windows)]
static WOKEN: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
fn open_the_event() {
    use windows_sys::Win32::Foundation::FALSE;
    use windows_sys::Win32::System::Threading::CreateEventW;

    // SAFETY: default security, manual reset, unsignalled, unnamed — four
    // arguments that borrow nothing.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, FALSE, std::ptr::null()) };
    WOKEN.store(event as isize, std::sync::atomic::Ordering::Relaxed);
}

/// Waits out `millis`, or until the handler signals the event.
#[cfg(windows)]
fn waited_out(millis: u64) {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    let woken = WOKEN.load(std::sync::atomic::Ordering::Relaxed);
    if woken == 0 {
        std::thread::sleep(std::time::Duration::from_millis(millis));
        return;
    }
    // `INFINITE` is `u32::MAX`, so a wait is capped one short of it rather than
    // becoming a wait with no end.
    let capped = u32::try_from(millis)
        .unwrap_or(u32::MAX - 1)
        .min(u32::MAX - 1);
    // SAFETY: an event handle this process created and never closes.
    unsafe { WaitForSingleObject(woken as HANDLE, capped) };
}

/// A platform with neither is one where Ctrl-C keeps its default meaning, and
/// the watcher is killed rather than asked to stop.
///
/// Nothing is lost by that: the wait is where the loop holds no lock and no
/// marker, which is exactly why it is safe to be killed there.
#[cfg(not(any(unix, windows)))]
fn listen_for_interrupts() {}

/// With nothing to be woken by, the wait is the sleep it always was.
#[cfg(not(any(unix, windows)))]
fn waited_out(millis: u64) {
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

/// Makes the *name* durable, once the bytes behind it are. `sync_all` on the
/// file promises its contents survive; the directory entry a rename created is a
/// separate write, and on a crash a file can be there with its old name or with
/// neither. Best-effort: a directory that will not open for reading is not a
/// reason to fail a write that has already landed.
fn sync_directory_of(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
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

/// The one `chmod` Perch performs, on a handle rather than a name. `O_NOFOLLOW`
/// fails a symlink at the last component with `ELOOP`, and `O_NONBLOCK` a FIFO
/// that would otherwise wait for a writer for ever — both become the remark
/// `tighten_if_loose` already makes about a file it could not narrow. Read-only,
/// or opening it would truncate the Credential.
#[cfg(unix)]
fn set_private_mode(path: &Path) -> Result<(), HostError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_mode(_path: &Path) -> Result<(), HostError> {
    Ok(())
}

/// When a process began, in the terms `/proc` speaks: field 22 of
/// `/proc/<pid>/stat` is the start in clock ticks since boot, and boot itself is
/// `btime` in `/proc/stat`. Both figures round down, so the answer can only run
/// early — the safe direction, since one that ran late would make a genuine
/// client look like a recycled PID.
#[cfg(target_os = "linux")]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name sits in parentheses and may hold anything; the numbered
    // fields resume after the last ')'.
    let after_command = stat.rsplit_once(')')?.1;
    let ticks_after_boot: i64 = after_command.split_whitespace().nth(19)?.parse().ok()?;

    let boot: i64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;

    // SAFETY: `sysconf` reads a value the C library holds and takes no pointer.
    // A name it does not know answers -1, which the check below treats as no
    // answer.
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    DateTime::from_timestamp_millis(boot * 1_000 + ticks_after_boot * 1_000 / ticks_per_second)
}

/// When a process began, as libproc reports it: the `proc_bsdinfo` for one pid,
/// whose `pbi_start_tvsec`/`pbi_start_tvusec` are the start to the microsecond.
/// `proc_pidinfo` rather than `sysctl KERN_PROC_PID`, because `libc` carries a
/// vetted declaration of the former for Apple and none of the latter's
/// `kinfo_proc`.
#[cfg(target_os = "macos")]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    // SAFETY: `proc_bsdinfo` is plain old data, so an all-zero value is a valid
    // one for the kernel to fill in.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;

    // SAFETY: the buffer is the `info` above and `size` is that same type's
    // size, so the kernel cannot write past it. That the two agree is what the
    // vetted declaration buys.
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
    // `checked_mul`, because a `tv_usec` above 4,294,967 overflows `u32`: a
    // panic in a debug build and a wrapped nanosecond count in a release one.
    // A real one is under a million, which is an expectation and not a promise.
    let nanoseconds = u32::try_from(info.pbi_start_tvusec)
        .ok()?
        .checked_mul(1_000)?;
    DateTime::from_timestamp(seconds, nanoseconds)
}

/// When a process began, as `GetProcessTimes` reports it: a creation `FILETIME`
/// counting 100-nanosecond ticks from 1601. Only for a process that is still
/// running — a Windows process object outlives its exit while anything holds a
/// handle, and a start time exists to corroborate a session marker
/// (ADR a-profile-is-live-by-evidence), which an exited process cannot.
#[cfg(windows)]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The epoch, in milliseconds after the point `FILETIME` counts from.
    const EPOCH_MILLIS_AFTER_1601: i64 = 11_644_473_600_000;

    // `STILL_ACTIVE` is 259, so a process that exits with 259 reads as running.
    // The safe direction: "running" means Perch leaves the Profile alone.
    // SAFETY: a handle this block opened, closed on every path out.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }

        let mut code = 0u32;
        let running = GetExitCodeProcess(process, &mut code) != 0 && code == STILL_ACTIVE as u32;
        if !running {
            CloseHandle(process);
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

/// Whether a process exists. Signal 0 performs the checks without delivering
/// anything, and the identifier is narrowed rather than cast: what `super::is_a_pid`
/// lets through is still a `u32`, and every value above `i32::MAX` is a process
/// *group* to `kill`. `EPERM` is alive and only `ESRCH` is dead, because a
/// Profile that looks Live is one Perch leaves alone.
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: `kill` takes no pointer and touches no memory Perch owns, signal
    // `0` delivers nothing, and the guards above make the argument a pid rather
    // than a group.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
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
    // `OsStrExt::as_bytes` rather than `as_encoded_bytes`: this is the call
    // documented to give the bytes the operating system will receive, which is
    // what a `CString` for `utimes` has to hold.
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|err| HostError::Other(format!("{} is not a path: {err}", path.display())))?;
    // SAFETY: `raw` outlives the call and holds a nul-terminated path, which is
    // what `CString::new` promises; a null `times` is `utimes`'s documented
    // "now" and reads no second buffer.
    let outcome = unsafe { libc::utimes(raw.as_ptr(), std::ptr::null()) };
    if outcome == 0 {
        Ok(())
    } else {
        // Through `or_not_found`, because a path that is not there is the one
        // failure this port names — and it is the variant a lock reads as "the
        // artifact has gone", which both adapters have to answer alike.
        or_not_found(Err(std::io::Error::last_os_error()), path)
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
    // SAFETY: `wide` outlives the block and is nul-terminated by the `chain`
    // above; the handle it opens is closed on every path out, and the error is
    // read before `CloseHandle` can replace it.
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
            // `or_not_found` for the reason the unix arm gives: a path that is
            // not there is the variant a lock reads as "the artifact has gone",
            // and `ERROR_FILE_NOT_FOUND` arrives here as `ErrorKind::NotFound`.
            return or_not_found(Err(std::io::Error::last_os_error()), path);
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
    /// Sent to this process directly rather than to a group, which is what a
    /// test may do: what is asserted is the disposition, and a signal raised
    /// here arrives at the same handler a terminal's would.
    #[cfg(unix)]
    #[test]
    fn an_interrupt_during_a_launch_belongs_to_the_client_and_perch_lives_through_it() {
        let host = RealHost::new();
        // A child that outlives the signal below, so the disposition under test
        // is the one in force *while* something is running.
        let ran = host.exec_interactive("/bin/sh", &["-c", "kill -INT $PPID; sleep 0"], &[]);

        assert!(
            ran.is_ok(),
            "a SIGINT arriving while the child runs is the child's: {ran:?}"
        );

        // Put back afterwards, or every later Ctrl-C in this process would be
        // swallowed too.
        // SAFETY: reading the disposition by replacing it and putting it back.
        let restored = unsafe {
            let was = libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGINT, was);
            was
        };
        assert_ne!(
            restored,
            libc::SIG_IGN,
            "the ignoring is for the launch, not for the rest of the process"
        );
    }

    /// All three answers, driven off a scratch directory rather than off this
    /// machine — which has `curl` at `/usr/bin`, and so can never reach the two
    /// branches that matter.
    #[cfg(not(windows))]
    #[test]
    fn curl_is_taken_from_the_absolute_path_and_then_from_the_walk() {
        let root = std::env::temp_dir().join(format!("perch-curl-{}", std::process::id()));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("a scratch directory");
        let usually = root.join("usr-bin-curl");
        let on_path = bin.join("curl");

        // Neither: a refusal that names both places rather than one.
        let refused = super::curl_at(&usually, bin.as_os_str())
            .expect_err("no curl anywhere is not something to guess at");
        let said = refused.to_string();
        assert!(said.contains("PATH"), "{said}");
        assert!(said.contains(&usually.display().to_string()), "{said}");

        // On PATH only: the walk is what the machines without `/usr/bin` need.
        std::fs::write(&on_path, "#!/bin/sh\n").expect("a curl on PATH");
        assert_eq!(
            super::curl_at(&usually, bin.as_os_str()).expect("the walk finds it"),
            on_path
        );

        // Both: the absolute path wins, which is what keeps anything earlier on
        // PATH from being handed a request bearing an access token.
        std::fs::write(&usually, "#!/bin/sh\n").expect("a curl where it belongs");
        assert_eq!(
            super::curl_at(&usually, bin.as_os_str()).expect("it is there"),
            usually
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// An empty element is how `PATH` spells the working directory, and a
    /// relative candidate is one `Command::new` resolves the same way.
    #[cfg(not(windows))]
    #[test]
    fn the_walk_passes_over_an_element_naming_the_working_directory() {
        let root = std::env::temp_dir().join(format!("perch-curl-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch directory");
        let usually = root.join("usr-bin-curl");
        std::fs::write(root.join("curl"), "#!/bin/sh\n").expect("a curl to be passed over");

        // What a trailing separator leaves, with the directory holding a `curl`
        // as the process Perch would be run from.
        let previous = std::env::current_dir().expect("a working directory");
        std::env::set_current_dir(&root).expect("somewhere to stand");
        let refused = super::curl_at(&usually, std::ffi::OsStr::new(":"));
        std::env::set_current_dir(previous).expect("back where we were");

        let said = refused
            .expect_err("a curl in the working directory is not one to hand a token to")
            .to_string();
        assert!(said.contains("PATH"), "{said}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Driven off a pipe rather than off standard input, which the suite may not
    /// take over: every other test in the process is using it. All three answers,
    /// because a byte, the end of input and a descriptor that will not be read
    /// are three different things to the caller.
    #[cfg(unix)]
    #[test]
    fn a_byte_the_end_of_input_and_a_refusal_are_three_answers() {
        let mut ends = [0 as libc::c_int; 2];
        // SAFETY: a two-element array this frame owns, which is what `pipe`
        // documents it fills.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0, "a pipe");
        let (reading, writing) = (ends[0], ends[1]);

        let wrote = b"h";
        // SAFETY: a one-byte buffer this frame owns and the write end of the
        // pipe just opened.
        unsafe { libc::write(writing, wrote.as_ptr().cast::<libc::c_void>(), 1) };
        assert!(matches!(super::one_byte_of(reading), Ok(Some(b'h'))));

        // Nothing more is coming, which is not the same as nothing was typed.
        // SAFETY: a descriptor this frame opened and has not closed.
        unsafe { libc::close(writing) };
        assert!(matches!(super::one_byte_of(reading), Ok(None)));

        // SAFETY: as above.
        unsafe { libc::close(reading) };
        assert!(
            matches!(super::one_byte_of(reading), Err(HostError::Io(_))),
            "a descriptor that is not open is a failure rather than an ending"
        );
    }

    /// The three arms of the mapping a lock turns on. `AlreadyExists` is
    /// contention and is waited out; everything else is the filesystem refusing
    /// and is reported — so a mapping that answered the first for both would
    /// make Perch wait on a failure that is never going to clear.
    #[test]
    fn only_an_occupied_name_is_reported_as_contention() {
        use std::io::{Error, ErrorKind};

        let at = Path::new("/somewhere/taken");
        assert!(matches!(
            super::or_already_exists::<()>(Err(Error::from(ErrorKind::AlreadyExists)), at),
            Err(HostError::AlreadyExists { .. })
        ));
        assert!(matches!(
            super::or_already_exists::<()>(Err(Error::from(ErrorKind::PermissionDenied)), at),
            Err(HostError::Io(_))
        ));
        assert!(super::or_already_exists(Ok(7), at).is_ok_and(|value| value == 7));
    }

    /// The buffer arithmetic a passphrase goes through, driven byte by byte.
    fn typed(input: &str) -> Option<String> {
        let mut bytes = input.bytes().collect::<std::collections::VecDeque<u8>>();
        super::a_line_from(|| Ok(bytes.pop_front()))
            .expect("the bytes are text")
            .map(|secret| secret.to_string())
    }

    #[test]
    fn a_secret_line_is_what_was_typed_without_what_the_return_key_left() {
        assert_eq!(typed("hunter2\n"), Some("hunter2".to_string()));
        assert_eq!(typed("hunter2\r\n"), Some("hunter2".to_string()));
        // No newline at all: the input ended, and what was typed still counts.
        assert_eq!(typed("hunter2"), Some("hunter2".to_string()));
    }

    #[test]
    fn end_of_input_is_nobody_answering_rather_than_an_empty_answer() {
        assert_eq!(typed(""), None);
        // A bare Return is somebody answering with nothing, which is a
        // different thing and the one `perch holdings purge` reads as a yes.
        assert_eq!(typed("\n"), Some(String::new()));
    }

    /// The growth path, which is the part with something to get wrong: past the
    /// reserved room the buffer is moved by hand so the one it leaves can be
    /// wiped, and a `Vec` that reallocated itself would have skipped that.
    #[test]
    fn a_passphrase_longer_than_the_buffer_survives_being_grown() {
        let long = "correct horse battery staple ".repeat(50);
        let past_the_reserved_room = long.len() > 512;
        assert!(past_the_reserved_room, "the growth path is the point");
        assert_eq!(typed(&format!("{long}\n")), Some(long));
    }

    /// Multi-byte text is read a byte at a time, so a character split across
    /// two reads has to survive being reassembled.
    #[test]
    fn a_passphrase_is_not_assumed_to_be_ascii() {
        assert_eq!(typed("pässwörd–✓\n"), Some("pässwörd–✓".to_string()));
    }

    use super::*;

    /// The one piece of parsing on the path every Renewal and every Utilization
    /// read goes through, and nothing could reach it: `FakeHost::http` answers
    /// with a response already built, so no behavior test ever splits a reply.
    #[test]
    fn a_reply_is_split_into_a_body_and_a_status_and_says_so_when_it_cannot_be() {
        let split = |wrote: &str| split_reply(Zeroizing::new(wrote.to_string()));

        let reply = split("{\"five_hour\":{}}\n200").expect("that is a reply");
        assert_eq!(reply.status, 200);
        assert_eq!(*reply.body, "{\"five_hour\":{}}");

        // A body with newlines in it: the split is the *last* one, because the
        // status is what curl appends.
        let reply = split("first\nsecond\n429").expect("that is a reply too");
        assert_eq!(reply.status, 429);
        assert_eq!(*reply.body, "first\nsecond");

        // An empty body still carries a status, which is what a 204 looks like.
        let reply = split("\n204").expect("a bodyless reply");
        assert_eq!((reply.status, reply.body.as_str()), (204, ""));

        // And what curl did not write is said as itself rather than read as a
        // status of zero, which `anthropic::understand` has no arm for.
        let refused =
            split("something went wrong\nnot a number").expect_err("that is not a status code");
        assert!(
            refused.to_string().contains("not a number"),
            "and it quotes what curl actually printed: {refused}"
        );

        let refused = split("no newline at all").expect_err("that is not a reply");
        assert!(refused.to_string().contains("no status code"), "{refused}");
    }

    #[test]
    fn the_access_token_travels_on_stdin_rather_than_in_argv() {
        let headers = [("Authorization", "Bearer sk-ant-oat01-secret")];
        let config =
            curl_config(&HttpRequest::get("https://example.test/usage", &headers)).unwrap();
        let config = &*config;
        let expected = "header = \"Authorization: Bearer sk-ant-oat01-secret\"";

        assert!(config.contains(expected), "{config}");
        assert!(
            !CURL_ARGS.iter().any(|arg| arg.contains("sk-ant")),
            "nothing about a request may reach the command line"
        );
    }

    /// The other half of "an access token only ever goes where Perch put it".
    /// `curl` reads `~/.curlrc` unless told not to, and it only takes the
    /// telling as the first argument — a `-q` in the middle is read as a
    /// request option and the file has already been obeyed. So the position is
    /// the assertion, not merely the presence.
    #[test]
    fn the_users_own_curl_configuration_is_never_read_for_a_request_carrying_a_token() {
        assert_eq!(
            CURL_ARGS.first(),
            Some(&"-q"),
            "`-q` is only obeyed as the first argument"
        );
    }

    /// The two identifiers a `u32` can carry that `kill` reads as something
    /// other than a process. A session marker is named after a number Perch did
    /// not write, so both are reachable, and both would otherwise report a
    /// client that is not there — which refuses every Switch against that
    /// Profile for as long as the marker sits there.
    #[cfg(unix)]
    #[test]
    fn a_process_id_that_is_not_one_is_dead_rather_than_a_process_group() {
        assert!(
            !process_alive(0),
            "0 is the caller's own process group, not a process"
        );
        assert!(
            !process_alive(u32::MAX),
            "4294967295 narrows to -1, which is every process the caller may signal"
        );
    }

    /// The corroboration the liveness check is for: this process is running, so
    /// nothing may conclude otherwise about it.
    #[cfg(unix)]
    #[test]
    fn the_running_process_is_alive() {
        assert!(process_alive(std::process::id()));
    }

    /// The write the Trail makes, on a real filesystem rather than a fake: the
    /// directory above it, the mode it is created at, and the width it answers
    /// with — a second line landing after the first rather than over it.
    #[cfg(unix)]
    #[test]
    fn a_line_lands_after_the_last_one_in_a_file_only_its_owner_may_read() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("perch-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let at = root.join("under").join("trail.log");
        let host = RealHost::new();

        let first = host.append_private_line(&at, "one").expect("a first line");
        let second = host.append_private_line(&at, "two").expect("a second");

        assert_eq!(first, 4, "the line and the newline that ends it");
        assert_eq!(second, 8, "and the next one lands after it");
        assert_eq!(std::fs::read_to_string(&at).unwrap(), "one\ntwo\n");
        assert_eq!(
            std::fs::metadata(&at).unwrap().permissions().mode() & 0o777,
            PRIVATE_FILE_MODE
        );

        // A file where the directory above would have to go, which is the
        // failure the Trail answers by writing nothing.
        assert!(
            host.append_private_line(&at.join("deeper"), "three")
                .is_err()
        );

        let _ = std::fs::remove_dir_all(&root);
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
    /// leans on: a lock artifact is a directory whose modification time says
    /// whether its holder is alive. It outwaits a coarse filesystem timestamp,
    /// which is a second of wall clock and a price rather than a claim — and it
    /// touches nothing outside `temp_dir` (ADR a-suite-is-named-and-gated).
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

    /// Both sides of the epoch, and the borrow that makes the pair one instant:
    /// a stamp a quarter-second before 1970 is `-1` second plus 750 million
    /// nanoseconds, never `0` seconds minus a quarter. A lock artifact's age is
    /// read off this, and one measured from the wrong second is a lock taken
    /// over early or held for ever.
    #[test]
    fn a_stamp_either_side_of_the_epoch_is_one_instant_rather_than_two() {
        use std::time::{Duration, UNIX_EPOCH};

        let pair = |time| seconds_and_nanos_since_epoch(time).expect("a stamp in range");

        assert_eq!(pair(UNIX_EPOCH), (0, 0));
        assert_eq!(
            pair(UNIX_EPOCH + Duration::new(90, 250_000_000)),
            (90, 250_000_000)
        );
        assert_eq!(pair(UNIX_EPOCH - Duration::from_secs(90)), (-90, 0));
        assert_eq!(
            pair(UNIX_EPOCH - Duration::new(0, 250_000_000)),
            (-1, 750_000_000),
            "a quarter-second before the epoch is the second below it, most of \
             the way through"
        );
        assert_eq!(
            pair(UNIX_EPOCH - Duration::new(90, 250_000_000)),
            (-91, 750_000_000)
        );

        // Round-tripped: the pair is only right if chrono reads it back as the
        // instant it was built from. Quarter-seconds because a Windows
        // `SystemTime` is a FILETIME, which rounds anything under 100ns away.
        for offset in [
            Duration::new(0, 250_000_000),
            Duration::new(90, 250_000_000),
        ] {
            let (seconds, nanos) = pair(UNIX_EPOCH - offset);
            let read = DateTime::from_timestamp(seconds, nanos).expect("in range");
            assert_eq!(
                read.timestamp_nanos_opt(),
                Some(-(offset.as_nanos() as i64)),
                "{offset:?} before the epoch"
            );
        }
    }

    /// The port hands back a `Result`, and every caller of this one — `carry`'s
    /// ranking of Profiles by age, each of `lock`'s reads — treats a failure as
    /// an answer, where a stamp chrono cannot represent used to end the process.
    ///
    /// Unix, because `utimes` sets one and an `i64` of seconds holds it.
    #[cfg(unix)]
    #[test]
    fn a_modification_time_out_of_range_is_refused_rather_than_panicked_on() {
        let host = RealHost::new();
        let dir = std::env::temp_dir().join(format!("perch-mtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("stamped");
        std::fs::write(&file, "x").unwrap();

        // Roughly a quarter of a million years past chrono's ceiling, and a
        // value every filesystem holding an `i64` of seconds accepts.
        let far = std::time::UNIX_EPOCH + std::time::Duration::from_secs(9_000_000_000_000);
        let set = set_modification_time(&file, far);
        let read = host.modified_at(&file);
        let _ = std::fs::remove_dir_all(&dir);

        // A filesystem that clamped the stamp instead has nothing to say here.
        if set.is_ok() && read.is_ok() {
            return;
        }
        assert!(
            matches!(read, Err(HostError::Other(_))),
            "an unrepresentable stamp is an error, got {read:?}"
        );
    }

    /// Sets a file's modification time, which no standard-library call reaches.
    /// Test-only: the primitives Perch links are `touch_now`'s, and this is the
    /// inverse nothing in `src` has a use for.
    #[cfg(unix)]
    fn set_modification_time(path: &Path, at: std::time::SystemTime) -> std::io::Result<()> {
        let seconds = at
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a stamp after the epoch")
            .as_secs() as libc::time_t;
        let times = [
            libc::timeval {
                tv_sec: seconds,
                tv_usec: 0,
            },
            libc::timeval {
                tv_sec: seconds,
                tv_usec: 0,
            },
        ];
        let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .expect("a path with no interior nul");
        // SAFETY: `raw` and `times` both outlive the call, and `utimes` reads
        // two `timeval`s from the array it is handed.
        let told = unsafe { libc::utimes(raw.as_ptr(), times.as_ptr()) };
        if told == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// `security` reports a failed sub-command of `-i` on stderr while still
    /// exiting 0, which is the whole reason its stderr is read at all. The
    /// check used to be for the word "error", and its own failure lines
    /// routinely do not carry one — so the failures it exists to catch were
    /// exactly the ones it let through.
    #[test]
    fn a_complaint_from_security_is_recognized_without_the_word_error() {
        assert!(said_something_went_wrong(
            "security: -25299: The specified item already exists in the keychain.\n"
        ));
        assert!(said_something_went_wrong(
            "security: SecKeychainItemCreateFromContent (<NULL>): The user name or \
             passphrase you entered is not correct.\n"
        ));
        assert!(said_something_went_wrong(
            "some other line\nsecurity: something went wrong\n"
        ));

        assert!(!said_something_went_wrong(""));
        assert!(
            !said_something_went_wrong("password has been deleted.\n"),
            "an ordinary remark is not a complaint"
        );
    }

    /// The temp path is guessable rather than secret, and
    /// `CLAUDE_CONFIG_DIR` can name a directory somebody else may write to. What
    /// stops a symlink planted there is not the name: the file is unlinked and
    /// created afresh with `O_EXCL`.
    #[cfg(unix)]
    #[test]
    fn a_replacement_is_never_written_through_something_left_at_the_temp_path() {
        let host = RealHost::new();
        let dir = std::env::temp_dir().join(format!("perch-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let target = dir.join("config.json");
        let elsewhere = dir.join("elsewhere");
        std::fs::write(&elsewhere, "not Perch's to write").unwrap();
        std::os::unix::fs::symlink(&elsewhere, crate::host::temp_beside(&host, &target)).unwrap();

        let written = crate::host::write_atomically(&host, &target, "{\"mine\":true}");

        let victim = std::fs::read_to_string(&elsewhere);
        let landed = std::fs::read_to_string(&target);
        let _ = std::fs::remove_dir_all(&dir);

        written.expect("the write lands");
        assert_eq!(landed.unwrap(), "{\"mine\":true}");
        assert_eq!(
            victim.unwrap(),
            "not Perch's to write",
            "the file the symlink pointed at is untouched"
        );
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
        let config =
            curl_config(&HttpRequest::post("https://example.test/token", &[], body)).unwrap();
        let config = &*config;
        let expected = r#"data-binary = "{\"refresh_token\":\"sk-ant-ort01-\\\"odd\\\"\"}""#;

        assert!(config.contains("url = \"https://example.test/token\""));
        assert!(config.contains(expected), "{config}");
        // The three bounds, the URL and the body: one option per line, and a
        // body carrying quotes and backslashes still occupying exactly one of
        // them.
        assert_eq!(config.lines().count(), 5, "one option per line: {config}");
    }

    /// The reservation `curl_config` writes into may not come up short, because
    /// growing it abandons a half-built request holding a token in freed heap.
    #[test]
    fn the_reservation_covers_a_request_that_is_nothing_but_escaping() {
        // Every character doubles, in all three places a value is quoted: the
        // worst case the count has to hold.
        let worst = r#"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\""#;
        let headers = [("Authorization", worst), (worst, worst)];
        let request = HttpRequest::post(worst, &headers, worst);
        let config = curl_config(&request).unwrap();
        assert!(
            config.len() <= width_of(&request),
            "{} written into {} reserved",
            config.len(),
            width_of(&request)
        );
    }

    /// A request that says how long it may take gets that, and one that does not
    /// gets the ordinary bound. The upgrade check on `perch version` is a line
    /// nobody asked for, and thirty seconds of a black-holed network is a great
    /// deal to spend on one (ADR an-upgrade-asks-its-channel).
    #[test]
    fn a_request_may_carry_its_own_bound_and_otherwise_gets_the_ordinary_one() {
        let ordinary = curl_config(&HttpRequest::get("https://example.test/usage", &[])).unwrap();
        let ordinary = &*ordinary;
        assert!(ordinary.contains("connect-timeout = 10"), "{ordinary}");
        assert!(ordinary.contains("max-time = 30"), "{ordinary}");
        // The bound the two timeouts do not give: a reply that keeps arriving
        // is bounded by `max-time` alone, and the whole of it is buffered.
        assert!(
            ordinary.contains(&format!("max-filesize = {MAX_REPLY_BYTES}")),
            "{ordinary}"
        );

        let brief =
            curl_config(&HttpRequest::get("https://example.test/latest", &[]).within(2_000))
                .unwrap();

        let brief = &*brief;
        assert!(brief.contains("connect-timeout = 2"), "{brief}");
        assert!(brief.contains("max-time = 2"), "{brief}");
        assert!(
            brief.contains(&format!("max-filesize = {MAX_REPLY_BYTES}")),
            "a request carrying its own time bound still gets the size one: {brief}"
        );

        // Rounded up rather than down. A bound that became zero would be `curl`
        // reading it as no bound at all, which is the opposite of what asking
        // for a short one means.
        let sub_second =
            curl_config(&HttpRequest::get("https://example.test/latest", &[]).within(200)).unwrap();
        let sub_second = &*sub_second;
        assert!(sub_second.contains("max-time = 1"), "{sub_second}");
    }

    /// The invariant `write_double_quoted` documents and only `security` was keeping:
    /// a configuration file is read a line at a time, so a value that could end
    /// its line is refused rather than quoted. An access token comes out of a
    /// JSON file Perch does not own, where `\n` is an ordinary escape.
    #[test]
    fn a_header_that_could_end_its_own_line_is_refused_rather_than_quoted() {
        let headers = [("Authorization", "Bearer sk-ant\noutput = /tmp/taken")];
        let refused = curl_config(&HttpRequest::get("https://example.test/usage", &headers));

        let said = refused.expect_err("a newline in a header must not reach curl");
        assert!(
            said.to_string().contains("Authorization"),
            "the refusal names what carried it: {said}"
        );
    }

    /// The same for the other two ways into the file, so the check cannot be
    /// the one that lives on a single field.
    #[test]
    fn a_url_or_a_body_that_carries_a_newline_is_refused_too() {
        assert!(curl_config(&HttpRequest::get("https://example.test/a\nurl = b", &[])).is_err());
        assert!(
            curl_config(&HttpRequest::post(
                "https://example.test/token",
                &[],
                "{}\noutput = /tmp/taken"
            ))
            .is_err()
        );
    }

    /// `curl`'s own escape, which quoting does not disarm: the configuration
    /// parser strips the quotes and the option parser then reads a leading `@`
    /// in `data-binary` as a filename. `curl` would post that file's contents
    /// to an endpoint Perch authenticates to, or fail naming a path.
    #[test]
    fn a_body_that_curl_would_read_as_a_filename_is_refused() {
        let refused = curl_config(&HttpRequest::post(
            "https://example.test/token",
            &[],
            "@/etc/passwd",
        ));

        let said = refused.expect_err("that is a filename to curl, not data");
        assert!(said.to_string().contains("filename"), "{said}");
        // Only the body: `@` is data to `url` and to `header`, and refusing it
        // there would be a rule about nothing.
        assert!(
            curl_config(&HttpRequest::get(
                "https://example.test/usage",
                &[("X-Thing", "@value")]
            ))
            .is_ok()
        );
    }
}
