//! The Host implementation that actually touches the machine.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};

#[cfg(unix)]
use super::PRIVATE_DIR_MODE;
use super::{
    Execution, Host, HostError, HttpRequest, HttpResponse, Link, PRIVATE_FILE_MODE, Platform,
    Waited,
};
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
///
/// The two timeouts are what make a hung endpoint a *refusal* rather than a
/// hang. Perch has no thread it can abandon a request from: `perch watcher run`
/// waits out every read in its round, and `perch tui` refuses Enter and `r` for
/// as long as a Refresh is outstanding. A connection that is open and silent
/// would otherwise stop both indefinitely — which is worse than the network
/// being down, because ADR 0018 has an answer for that one and no answer for a
/// loop that never comes back to be told.
///
/// Generous rather than tight: what is on the other side is a request Perch
/// would rather complete than retry, and both numbers are far longer than a
/// reply that is coming ever takes.
///
/// `-q` is first because it is only obeyed there, and it is here for the reason
/// [`curl_bin`] names an absolute path: without it `curl` reads `~/.curlrc`
/// before anything else, and that file can set `proxy` and `insecure` — which
/// is a machine-local way of receiving an `Authorization: Bearer` header,
/// arriving through a different door than `PATH`. It can also set `output`,
/// which diverts the body to a file and leaves Perch parsing nothing.
const CURL_ARGS: [&str; 7] = [
    "-q",
    "--silent",
    "--show-error",
    "--write-out",
    "\n%{http_code}",
    "--config",
    "-",
];

/// How long a request gets when it does not say: long enough for a Refresh
/// somebody is waiting on, over a connection that may be slow.
///
/// In the configuration rather than in [`CURL_ARGS`], where both of these used
/// to be, so that a request carrying its own bound has one place to put it —
/// and so that every part of a request still travels the same way (see
/// [`HttpRequest`]).
const CONNECT_TIMEOUT_SECONDS: u64 = 10;
const MAX_TIME_SECONDS: u64 = 30;

/// The request as a `curl` configuration file, which is what goes in on stdin.
///
/// The URL, the headers and the body all arrive this way so that none of them
/// is ever an argument: an `Authorization` header holds an access token, and
/// argv is readable by every process on the machine.
fn curl_config(request: &HttpRequest<'_>) -> Result<String, HostError> {
    let quoted = super::double_quoted;

    // `double_quoted` makes a value a token and says so: it does not make one
    // inert, because a configuration file is read a line at a time and there is
    // no escape for a newline to be quoted into. `security` has refused these
    // where they enter since ADR 0008; the `curl` half of the same invariant
    // was never written, and an access token is a value Perch reads out of a
    // JSON file it does not own — where `\n` is a legal escape. A token
    // carrying one would end the `header` line and begin whatever the rest of
    // it spelled, and `output =` writes a file while `url =` fetches one.
    super::inert("the URL", request.url)?;
    for (name, value) in request.headers {
        super::inert(&format!("the {name} header"), value)?;
    }
    if let Some(body) = request.body {
        super::inert("the request body", body)?;
    }

    // Whole seconds, because that is the only unit `curl` takes here, and at
    // least one: a bound that rounded down to zero would mean *no* bound, which
    // is the opposite of what a caller asking for a short one wants.
    let (connect, whole) = match request.within_millis {
        Some(millis) => {
            let seconds = millis.div_ceil(1000).max(1);
            (seconds, seconds)
        }
        None => (CONNECT_TIMEOUT_SECONDS, MAX_TIME_SECONDS),
    };
    let mut config = format!("connect-timeout = {connect}\nmax-time = {whole}\n");
    config.push_str(&format!("url = {}\n", quoted(request.url)));
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
    Ok(config)
}

/// Runs a program, optionally with something on its stdin, and reads the whole
/// of what it said.
///
/// The one place Perch spawns anything. Stdin is where every secret travels —
/// `curl`'s configuration and `security`'s command lines both — so the choice
/// between a pipe and `/dev/null` is the same choice in both cases and is made
/// once.
fn run(program: &Path, args: &[&str], stdin: Option<&str>) -> std::io::Result<Execution> {
    // Named here rather than by each caller. `Command::spawn`'s error carries
    // no path, so a machine without `/usr/bin/curl` failed `perch status
    // --refresh` with "No such file or directory (os error 2)" and nothing at
    // all about what was missing or where Perch looked. The kind is kept, so
    // anything matching on `NotFound` still does.
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
        // Reaped even when the write failed, and reported in the child's own
        // words where it has any.
        //
        // `Child::drop` neither kills nor waits, so returning straight out of
        // here left a zombie for the life of the process — and `perch watcher
        // run` is a loop that spawns `curl` every round for as long as somebody
        // leaves it running. The likeliest failure is an `EPIPE` from a child
        // that has already exited, which is precisely the case where its stderr
        // says what went wrong and "Broken pipe (os error 32)" does not: the
        // same information loss the naming above was written to fix for a spawn
        // that failed.
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

/// How a process that has finished ended, as the one number a caller can pass
/// on as its own exit code.
///
/// A code where the process exited with one. Where it did not it was killed by a
/// signal, and the shell's own convention is the honest answer: 128 plus the
/// signal, which is what `$?` reports for that same death and therefore what
/// anything wrapping `perch run` is already written to read (ADR 0010). The `-1`
/// [`Execution`] uses is for the captured executions Perch only ever asks
/// `succeeded()` of; a status that *becomes* Perch's exit code has to say more
/// than "not zero", and -1 leaves the shell reporting 255 for a Ctrl-C.
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
        run(Path::new(SECURITY_BIN), args, stdin).map_err(|err| KeychainError::Unavailable {
            detail: format!("could not run {SECURITY_BIN}: {err}"),
        })?;

    // The stderr check belongs to `-i` and nothing else. That is the mode where
    // a failed sub-command is reported on stderr while the process still exits
    // 0; every other invocation says so with its exit code, and reading their
    // stderr for a complaint would turn a warning into a failure.
    let complained = args == ["-i"] && said_something_went_wrong(&execution.stderr);
    if execution.succeeded() && !complained {
        Ok(execution)
    } else {
        Err(classify(&execution, service, account))
    }
}

/// Whether `security` wrote a diagnostic of its own.
///
/// Matched on the `security:` prefix it writes those with, rather than on the
/// word "error". Its failure lines routinely carry no such word —
///
/// ```text
/// security: -25299: The specified item already exists in the keychain.
/// security: SecKeychainItemCreateFromContent (<NULL>): The user name or passphrase you entered is not correct.
/// ```
///
/// — so a substring search for "error" misses exactly the failures the check
/// exists to catch, which is the silent-write case ADR 0008 is about.
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
    /// Whether a remark is printed as it is made, or only kept. A cell because
    /// `perch tui` turns it off for as long as it holds the screen and back on
    /// afterwards, through a `&dyn Host` (see [`Host::print_remarks`]).
    aloud: Cell<bool>,
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
            aloud: Cell::new(true),
        }
    }

    /// A Host that keeps its remarks instead of printing them, for the one
    /// caller that owns the screen.
    ///
    /// A remark goes to stderr, which is exactly where `perch tui` is drawing:
    /// a line about a Credential written to a store Perch would rather not have
    /// used would land in the middle of a frame and stay there until something
    /// redrew over it. So the Refresh the TUI runs collects them from
    /// [`RealHost::remarks`] and shows them where it shows everything else it
    /// could not do.
    pub fn keeping_its_remarks() -> Self {
        RealHost {
            aloud: Cell::new(false),
            ..RealHost::new()
        }
    }
}

impl Host for RealHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn home_dir(&self) -> Result<PathBuf, HostError> {
        home_from(HOME_VARIABLE, std::env::var_os(HOME_VARIABLE))
    }

    fn current_dir(&self) -> Result<PathBuf, HostError> {
        std::env::current_dir().map_err(|err| HostError::Other(err.to_string()))
    }

    fn env_var(&self, key: &str) -> Option<String> {
        // Read as bytes and converted here, rather than `std::env::var`, so a
        // value that is present and not text is told apart from one that is not
        // there. `var().ok()` folds them together, and the two mean opposite
        // things to every caller: `CLAUDE_CONFIG_DIR` read as unset sends a
        // Credential to `~/.claude` instead of to the directory somebody
        // pointed at, and `PERCH_HOME` read as unset puts Perch's own state
        // somewhere they did not ask for.
        //
        // Still `None`, because there is nothing usable to hand back — a path
        // Perch cannot spell cannot be joined or compared — but said out loud,
        // because being ignored silently is what makes it a mystery rather than
        // a mistake.
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

    /// Canonicalised, because the path is read as evidence of a Channel and
    /// the Homebrew case arrives as a symlink: `std::env::current_exe` on macOS
    /// hands back the path the process was launched with, which for a
    /// Homebrew install is `<prefix>/bin/perch` and says nothing about a
    /// Cellar. A path that will not canonicalise is handed back as it came —
    /// less informative, and better than refusing to run.
    fn current_exe(&self) -> Result<PathBuf, HostError> {
        let launched = std::env::current_exe()?;
        Ok(std::fs::canonicalize(&launched).unwrap_or(launched))
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
    /// is one that was created 0600 rather than one that was tightened
    /// afterwards — even where something looser was already there (ADR 0020).
    ///
    /// The choreography itself is [`super::replace_via_tmp`], shared with the
    /// non-secret writes: two copies of it would let the failure cleanup drift
    /// apart between the path that handles Credentials and the path that does
    /// not, which is the wrong pair to let drift. All this adds is the parent
    /// directory, which has to be private before the file lands in it.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            create_private_dir_all(parent)?;
        }
        super::replace_via_tmp(self, path, contents, PRIVATE_FILE_MODE)
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
        // The bytes are already durable by the time anything is renamed over
        // another file here; this is what makes the new name durable too.
        sync_directory_of(to);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(HostError::Io(err)),
        }
    }

    fn link(&self, kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
        make_link(kind, target, at)
    }

    /// `symlink_metadata` rather than `read_link` alone, so the three answers
    /// stay three: a link, something that is not a link, and nothing there.
    /// `read_link` collapses the last two into one error.
    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(HostError::NotFound {
                    path: path.to_path_buf(),
                });
            }
            Err(err) => return Err(HostError::Io(err)),
        };
        if !metadata.file_type().is_symlink() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_link(path)?))
    }

    fn remove_link(&self, path: &Path) -> Result<(), HostError> {
        remove_link(path)
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
        let command_line = keychain::add_command_line(service, account, secret)?;
        match keychain::write_path_for(&command_line) {
            WritePath::Stdin => security(&["-i"], Some(&command_line), service, account)?,
            WritePath::Argv => {
                // The one time a Credential reaches `argv`, and therefore the
                // process table. Hex is an encoding, not a protection: anything
                // that can read `argv` recovers the bytes with `xxd -r -p`. It
                // is done anyway because the alternative is `security`
                // truncating the write mid-argument without saying so, which
                // stores a corrupt Credential nothing notices until some Switch
                // later (ADR 0008) — but it is said out loud, because an
                // invariant with a silent exception is not an invariant.
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
        // Named here for the reason `run` names it: `Command::status`'s error
        // carries no path, so a `claude` that had been uninstalled between
        // Perch finding it and launching it failed a login with "could not
        // launch a login: No such file or directory (os error 2)" — nothing
        // about what was missing or where Perch looked. `commands::run` adds
        // the name back for its own launch; the login path had no equivalent.
        // The kind is kept, so anything matching on `NotFound` still does.
        let status = command.status().map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not run {program}: {err}"))
        })?;
        Ok(ended_as(status))
    }

    fn process_id(&self) -> u32 {
        std::process::id()
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

    fn listen_for_interrupts(&self) {
        listen_for_interrupts();
    }

    /// Linked rather than shelled out to (ADR 0021): the whole of it is one
    /// `geteuid`, and `id -u` would be a process spawned to answer a question
    /// the C library already holds.
    ///
    /// The *effective* uid rather than the real one, because that is the
    /// identity the filesystem will judge every write by, and the one launchd
    /// files a session under.
    #[cfg(unix)]
    fn user_id(&self) -> Option<u32> {
        // SAFETY: `geteuid` takes nothing, cannot fail, and touches no memory.
        Some(unsafe { libc::geteuid() })
    }

    /// Windows has no uid: a logon task names the user it runs as, and there is
    /// nothing here to quote or to refuse (ADR 0040).
    #[cfg(not(unix))]
    fn user_id(&self) -> Option<u32> {
        None
    }

    /// In slices, checking between them, because the platforms do not agree on
    /// what a signal does to a sleeping thread and none of them can be relied
    /// on to cut one short: `nanosleep` reports how much was left and the
    /// standard library goes back round for the rest. A watcher that noticed
    /// Ctrl-C only when its poll interval happened to end would take two and a
    /// half minutes to die.
    ///
    /// The slice is what the person waits for after pressing Ctrl-C, and the
    /// only cost of a shorter one is a wakeup that does nothing.
    fn wait(&self, millis: u64) -> Waited {
        const SLICE_MILLIS: u64 = 100;

        let mut left = millis;
        while left > 0 {
            if interrupted() {
                return Waited::Interrupted;
            }
            let slice = left.min(SLICE_MILLIS);
            std::thread::sleep(std::time::Duration::from_millis(slice));
            left -= slice;
        }
        // Asked once more at the end, so a Ctrl-C arriving inside the last
        // slice ends this wait rather than being carried into the next one.
        match interrupted() {
            true => Waited::Interrupted,
            false => Waited::Fully,
        }
    }

    fn is_interactive(&self) -> bool {
        use std::io::IsTerminal;
        // Both ends matter: a question needs somewhere to be shown as well as
        // somewhere to be answered from.
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    }

    /// To stderr, so a note never lands in the middle of the JSON a script is
    /// reading off stdout — unless this is a Host built to keep them, whose
    /// caller has the screen and will say them itself.
    fn note(&self, line: &str) {
        if self.noted.borrow_mut().insert(line.to_string()) && self.aloud.get() {
            eprintln!("perch: {line}");
        }
    }

    fn print_remarks(&self, aloud: bool) {
        self.aloud.set(aloud);
    }

    /// In the order a `BTreeSet` keeps, and each of them once.
    fn remarks(&self) -> Vec<String> {
        self.noted.borrow().iter().cloned().collect()
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        read_a_line()
    }

    fn read_secret(&self) -> Result<Option<String>, HostError> {
        read_without_echo()
    }

    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError> {
        let execution = curl(&curl_config(request)?)?;
        if !execution.succeeded() {
            return Err(HostError::Other(format!(
                "curl exited {}: {}",
                execution.status,
                execution.stderr.trim()
            )));
        }

        split_reply(&execution.stdout)
    }
}

/// The body and the status code out of what `curl` wrote, which is the body
/// followed by `--write-out '\n%{http_code}'`.
///
/// Apart from the caller so it can be asserted on. Nothing else can reach it —
/// `FakeHost::http` answers with a `HttpResponse` already built — so the one
/// piece of parsing on the path every Renewal and every Utilization read goes
/// through had no test at all.
///
/// A status code that will not parse is said rather than swallowed. It was
/// `unwrap_or(0)`, which turned "curl printed something Perch does not
/// understand" into an HTTP status of zero: `anthropic::understand` has no arm
/// for it, so the user was told their Credential was refused with `HTTP 0` —
/// a number no server ever sends, about a request that may never have been
/// made.
fn split_reply(stdout: &str) -> Result<HttpResponse, HostError> {
    let (body, code) = stdout
        .rsplit_once('\n')
        .ok_or_else(|| HostError::Other("curl produced no status code".into()))?;
    let status = code.trim().parse().map_err(|_| {
        HostError::Other(format!(
            "curl reported `{}` where a status code was expected",
            code.trim()
        ))
    })?;
    Ok(HttpResponse {
        status,
        body: body.to_string(),
    })
}

/// One line from standard input, or `None` at end of it.
fn read_a_line() -> Result<Option<String>, HostError> {
    use std::io::BufRead;

    let mut line = String::new();
    let read = std::io::stdin().lock().read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

/// Whether the terminal was echoing before Perch turned it off, and so whether
/// an interrupted read owes it an `ECHO` back on.
///
/// A static because the only thing that can act between turning echo off and
/// turning it back on is a signal handler, and a handler reaches nothing else.
/// A terminal somebody had already put in `stty -echo` is left as they left it,
/// which is why this is the answer rather than an assumption.
static WAS_ECHOING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The same, with the terminal not showing what is typed.
///
/// The whole of it is `ECHO` off, one line, `ECHO` back on — a platform
/// primitive linked rather than taken as a crate, which is what ADR 0021
/// decided for the last two of these. The mode is restored whatever the read
/// did, because a terminal left with its echo off outlives the process that
/// turned it off and every shell after it types blind.
///
/// "Whatever the read did" has to include Ctrl-C, which is not something the
/// read does. Only `perch watcher run` takes SIGINT over
/// ([`listen_for_interrupts`]), so at the one prompt where echo is off the
/// signal otherwise arrives at the default disposition and kills the process in
/// the window between the two `tcsetattr` calls — and backing out of a
/// passphrase prompt is a thing people do, not an accident. So the window is
/// held with a handler that repairs the terminal and then hands the signal
/// straight back: `tcgetattr`, `tcsetattr`, `signal` and `raise` are all
/// async-signal-safe, and re-raising rather than exiting means a parent still
/// sees a process that died of SIGINT.
#[cfg(unix)]
fn read_without_echo() -> Result<Option<String>, HostError> {
    use std::os::fd::AsRawFd;
    use std::sync::atomic::Ordering::Relaxed;

    /// Puts the echo back, then dies of the signal as it would have anyway.
    unsafe extern "C" fn show_again_and_stop(signal: libc::c_int) {
        if WAS_ECHOING.load(Relaxed) {
            // SAFETY: a `termios` owned by this frame and this process's own
            // standard input, and both calls are async-signal-safe. `TCSANOW`
            // rather than `TCSAFLUSH`: there is no longer a read for pending
            // input to confuse, and flushing is work this frame need not do.
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
    // Installed before the echo goes off rather than after, so there is no
    // instant where it is off and nothing would put it back. A signal arriving
    // early finds a terminal that is already echoing and leaves it that way.
    //
    // Both signals the same terminal delivers at this prompt with a default
    // disposition that kills. SIGINT is the one somebody means; SIGQUIT is one
    // key over, arrives the same way, and left exactly the blind shell this
    // function exists to prevent — the guard was written for "backing out of a
    // passphrase prompt is a thing people do", and Ctrl-\ is that too.
    // SAFETY: the handler stores to an atomic and calls four async-signal-safe
    // functions. `signal` hands back the disposition being replaced, which is
    // put back below.
    let guarding = [libc::SIGINT, libc::SIGQUIT];
    let previously = guarding.map(|signal| unsafe {
        libc::signal(
            signal,
            show_again_and_stop as *const () as libc::sighandler_t,
        )
    });

    let mut hiding = showing;
    hiding.c_lflag &= !libc::ECHO;
    // `TCSAFLUSH` rather than `TCSANOW`: anything typed ahead of the question was
    // typed while the terminal was still echoing, and would be read as part of
    // the passphrase after having been shown on the way in.
    // SAFETY: both arguments are as above, and `hiding` is the mode just read
    // back with one flag cleared.
    let hidden = unsafe { libc::tcsetattr(terminal, libc::TCSAFLUSH, &hiding) } == 0;

    let typed = if hidden {
        read_a_line()
    } else {
        Err(HostError::Io(std::io::Error::last_os_error()))
    };

    // SAFETY: as above, restoring exactly the mode `tcgetattr` reported and the
    // dispositions `signal` reported. Both happen however the read ended, which
    // is the whole of what this function promises.
    //
    // `SIG_ERR` is not a disposition and is what `signal` answers when it could
    // not install one. Handed straight back it installs an invalid handler over
    // whatever was really there, so an install that failed was turned into a
    // second, worse failure — the one case where this is reached is the one
    // where the original disposition is still in place and wants leaving alone.
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

/// The same on Windows, where the console has one mode word and `ENABLE_ECHO_INPUT`
/// is the bit in it.
///
/// Ctrl-C is guarded for the reason the unix half is, and in the shape
/// [`listen_for_interrupts`] already uses over there: a console handler, claimed
/// for the length of the read and given back afterwards. It answers `FALSE`, so
/// the default handler still ends the process — all this one does is put the
/// echo back before it does.
#[cfg(windows)]
fn read_without_echo() -> Result<Option<String>, HostError> {
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
            read_a_line()
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
fn read_without_echo() -> Result<Option<String>, HostError> {
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
///
/// Synced before it is closed. A rename is atomic against other *processes*,
/// not against a crash: the rename can reach the disk while the data blocks
/// have not, leaving a file that is present, named, and empty. What this writes
/// is a Credential Anthropic has already Rotated to, or a `.claude.json` full
/// of somebody's project history — neither of which can be written again.
#[cfg(unix)]
fn create_file_with_mode(path: &Path, contents: &str, mode: u32) -> Result<(), HostError> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    // Anything already here is what a write that died left behind, and holding
    // on to it would mean writing into a file of unknown mode.
    let _ = std::fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    // `open`'s mode is what the file is created *at most* as: the kernel takes
    // the process umask out of it, so `.mode(0o644)` under `umask 077` is a
    // file at 0600 and the port's promise of "exactly this mode" is not kept.
    // The direction is always narrowing, so nothing was ever exposed by it —
    // what it broke is `write_atomically`, which reads the target's mode and
    // writes the replacement with it: a `.claude.json` the person keeps at 0644
    // came back at 0600 after a Switch run from a shell with a tight umask, and
    // the conformance case asserting the mode passed or failed depending on the
    // shell that launched the suite.
    //
    // Set here rather than asked for at `open`, because there is nowhere else
    // to ask. Safe at this point and nowhere later: the file is `O_EXCL` and
    // still empty, so the instant this widens is an instant there is nothing in
    // it to read — which is the whole of what "at creation" was protecting.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
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

/// Makes a link, where a symbolic link is a symbolic link and a junction is
/// something this platform has never heard of.
#[cfg(not(windows))]
fn make_link(kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
    match kind {
        Link::Symbolic => Ok(std::os::unix::fs::symlink(target, at)?),
        Link::Hard => Ok(std::fs::hard_link(target, at)?),
        Link::Junction => Err(HostError::Other(
            "a directory junction is a Windows link, and this is not Windows".to_string(),
        )),
    }
}

/// The same on Windows, where the kinds actually differ.
///
/// A symbolic link needs Developer Mode or elevation and fails without either,
/// which is the failure the other two kinds exist to be tried after. Junctions
/// are a reparse point the standard library does not write, so that one is the
/// `junction` crate's: hand-writing a `REPARSE_DATA_BUFFER` is exactly the
/// class of `unsafe` ADR 0021 declined to write, and it sits behind this port
/// either way (ADR 0025).
#[cfg(windows)]
fn make_link(kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
    match kind {
        // Which of the two symlink calls is the target's business: Windows
        // records in the link whether it names a directory, and a file symlink
        // pointing at a directory does not resolve.
        Link::Symbolic if target.is_dir() => Ok(std::os::windows::fs::symlink_dir(target, at)?),
        Link::Symbolic => Ok(std::os::windows::fs::symlink_file(target, at)?),
        Link::Junction => Ok(junction::create(target, at)?),
        Link::Hard => Ok(std::fs::hard_link(target, at)?),
    }
}

/// Removes a link and nothing else. A link that has already gone is not a
/// failure, as with every other removal here.
#[cfg(not(windows))]
fn remove_link(path: &Path) -> Result<(), HostError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(HostError::Io(err)),
    }
}

/// The same on Windows, where which call removes a link depends on whether it
/// names a directory.
///
/// Read off the attributes rather than off `FileType`, which reports a junction
/// as a symlink and not as a directory — true, and not what the call to make
/// turns on.
#[cfg(windows)]
fn remove_link(path: &Path) -> Result<(), HostError> {
    use std::os::windows::fs::MetadataExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(HostError::Io(err)),
    };

    let removed = if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    };
    match removed {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(HostError::Io(err)),
    }
}

/// Whether the person at the terminal has asked a loop to stop.
///
/// A `static` rather than something the Host owns, because the handler that
/// sets it is the operating system's to call and it is handed no context to
/// find a Host through. Everything a signal handler may touch has to be
/// async-signal-safe, and a single atomic flag is the whole of what this one
/// touches.
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn interrupted() -> bool {
    INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Takes Ctrl-C — and the signal a service manager stops a process with — over
/// from the default handler, which would kill the process where it stands.
///
/// Perch links the platform's own primitive rather than taking a crate for it
/// (ADR 0021): the whole of the unix half is two `signal` calls, and the whole
/// of the Windows half is one `SetConsoleCtrlHandler`.
///
/// **`SIGTERM` as well as `SIGINT`, and this is what a Service rests on** (ADR
/// 0040). `SIGINT` is what a person types; `SIGTERM` is what systemd and launchd
/// send to stop a unit, and its default action is immediate death. Unhandled, a
/// `systemctl --user stop` arriving mid-Switch would kill Perch between the
/// incoming Credential reaching the Default Profile and the Identity being
/// patched — a Landing nobody wrote down, which is the one state ADR 0006's
/// ordering exists to make impossible. Both mean the same thing to the loop, so
/// both set the same flag and are answered at the same place: the wait.
///
/// The handler stands down after one signal, so a **second** of either kills the
/// process the way it always did. The first asks the loop to finish what it is
/// doing, and there is no bound on how long that takes — a round waiting on a
/// keychain prompt or on a network that has gone away could sit there — so
/// neither the person watching nor the service manager is left with a program
/// that has taken the only way of stopping it and given nothing back. That
/// second signal is what a service manager sends at the end of its stop grace,
/// which `install` pins to thirty seconds on every platform.
///
/// Idempotent, and safe to call more than once: installing the same handler
/// twice installs the same handler.
#[cfg(unix)]
fn listen_for_interrupts() {
    unsafe extern "C" fn stop(signal: libc::c_int) {
        INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: `signal` is async-signal-safe, and this hands the signal back
        // to the default handler rather than installing anything of Perch's.
        //
        // Only the signal that arrived is stood down, which is deliberate: a
        // first Ctrl-C followed by a `systemctl stop` should still be able to
        // kill a loop that is wedged finishing its Switch, and standing both
        // down together would leave one of them re-arming the other's default.
        unsafe { libc::signal(signal, libc::SIG_DFL) };
    }

    // SAFETY: the handler stores to an atomic and restores the default
    // disposition, both async-signal-safe. `signal` returns the previous
    // handler, which nothing here needs.
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
}

/// The same on Windows, where Ctrl-C is a console event delivered on a thread
/// of its own rather than a signal.
///
/// Only the two events that mean "stop this program" are claimed, and only the
/// first of them: the second is answered `FALSE` and left to the default
/// handler, which ends the process — the same standing-down the unix half does,
/// for the same reason. A console being closed or a user logging out is never
/// claimed at all, because those are not requests to finish what you were
/// doing, and Windows kills a handler that claims them seconds later anyway.
#[cfg(windows)]
fn listen_for_interrupts() {
    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
    };

    unsafe extern "system" fn stop(event: u32) -> windows_sys::core::BOOL {
        match event {
            CTRL_C_EVENT | CTRL_BREAK_EVENT
                if !INTERRUPTED.swap(true, std::sync::atomic::Ordering::Relaxed) =>
            {
                TRUE
            }
            _ => FALSE,
        }
    }

    // SAFETY: the handler stores to an atomic and reads its argument, and the
    // routine stays valid for the life of the process. A registration that
    // fails leaves the default handler in place, which ends the process on
    // Ctrl-C — the behaviour of every other Perch command.
    unsafe {
        SetConsoleCtrlHandler(Some(stop), TRUE);
    }
}

/// A platform with neither is one where Ctrl-C keeps its default meaning, and
/// the watcher is killed rather than asked to stop.
///
/// Nothing is lost by that: the wait is where the loop holds no lock and no
/// marker, which is exactly why it is safe to be killed there.
#[cfg(not(any(unix, windows)))]
fn listen_for_interrupts() {}

/// Makes the *name* durable, once the bytes behind it are.
///
/// `sync_all` on the file promises its contents survive; the directory entry
/// the rename created is a separate write, and on a crash a file can be there
/// with its old name or with neither. Best-effort: a directory that will not
/// open for reading — Windows, most of all — is not a reason to fail a write
/// that has already landed.
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

    // SAFETY: `sysconf` reads a value the C library holds and takes no pointer.
    // A name it does not know answers -1, which the check below treats as no
    // answer rather than as a rate.
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
    // SAFETY: `proc_bsdinfo` is plain old data, so an all-zero value is a valid
    // one for the kernel to fill in.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;

    // SAFETY: the buffer is the `info` above and `size` is that same type's
    // size, so the kernel cannot write past it. That the two agree is exactly
    // what the vetted `libc` declaration buys — a hand-written `kinfo_proc`
    // getting the size wrong is the class of mistake ADR 0021 declined to risk.
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
///
/// Only for a process that is still running. A Windows process object outlives
/// its exit for as long as anything holds a handle, and `GetProcessTimes`
/// answers for it the whole while — but an exited process is a gone one here,
/// as it is on unix once reaped, because a start time exists to corroborate a
/// session marker (ADR 0022) and a process that has exited corroborates
/// nothing.
#[cfg(windows)]
fn process_started_at(pid: u32) -> Option<DateTime<Utc>> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The epoch, in milliseconds after the point `FILETIME` counts from.
    const EPOCH_MILLIS_AFTER_1601: i64 = 11_644_473_600_000;

    // `STILL_ACTIVE` is 259, so a process that exits with code 259 reads as
    // running. That is the safe direction here and deliberately so: "running"
    // means Perch leaves the Profile alone, and a marker corroborated by a
    // process that has in fact exited costs a refusal the user can clear by
    // deleting the file.
    //
    // SAFETY: every call takes either a handle this block opened or a value
    // owned by this frame, and the handle is closed on every path out —
    // including the two early returns below.
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

/// Whether a process exists. Signal 0 performs the permission and existence
/// checks without delivering anything.
///
/// The identifier is narrowed rather than cast, because `kill` reads the values
/// a cast can produce as something else entirely: `0` is the caller's own
/// process group and `-1` is every process it may signal, and both answer "yes"
/// to a signal that is never delivered. A marker file is named after a number
/// somebody else wrote (`probe::clients_in` parses any `u32`), so `4294967295`
/// and `0` are both reachable, and both would otherwise report a live client
/// that is not there — which refuses every Switch against that Profile forever.
///
/// `EPERM` is alive. It means the process exists and belongs to another user,
/// which is the same reasoning the Windows half already writes down for a
/// handle it may not open: a Profile that looks Live is one Perch leaves alone,
/// and that is the safe direction. Only `ESRCH` is dead.
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
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
    // whose result is documented to be the bytes the operating system will
    // receive, which is exactly what a `CString` for `utimes` has to hold. The
    // encoding behind `as_encoded_bytes` is deliberately unspecified, and it
    // happening to be the same thing on unix today is not the promise this
    // needs.
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::CString::new(path.as_os_str().as_bytes())
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

    /// The one piece of parsing on the path every Renewal and every Utilization
    /// read goes through, and nothing could reach it: `FakeHost::http` answers
    /// with a response already built, so no behaviour test ever splits a reply.
    #[test]
    fn a_reply_is_split_into_a_body_and_a_status_and_says_so_when_it_cannot_be() {
        let reply = split_reply("{\"five_hour\":{}}\n200").expect("that is a reply");
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, "{\"five_hour\":{}}");

        // A body with newlines in it: the split is the *last* one, because the
        // status is what curl appends.
        let reply = split_reply("first\nsecond\n429").expect("that is a reply too");
        assert_eq!(reply.status, 429);
        assert_eq!(reply.body, "first\nsecond");

        // An empty body still carries a status, which is what a 204 looks like.
        assert_eq!(split_reply("\n204").expect("a bodyless reply").status, 204);

        // And what curl did not write is said as itself. `unwrap_or(0)` turned
        // this into an HTTP status of zero, which `anthropic::understand` has no
        // arm for — so somebody was told their Credential was refused with
        // `HTTP 0`, a number no server sends, about a request that may never
        // have been made.
        let refused = split_reply("something went wrong\nnot a number")
            .expect_err("that is not a status code");
        assert!(
            refused.to_string().contains("not a number"),
            "and it quotes what curl actually printed: {refused}"
        );

        let refused = split_reply("no newline at all").expect_err("that is not a reply");
        assert!(refused.to_string().contains("no status code"), "{refused}");
    }

    #[test]
    fn the_access_token_travels_on_stdin_rather_than_in_argv() {
        let headers = [("Authorization", "Bearer sk-ant-oat01-secret")];
        let config =
            curl_config(&HttpRequest::get("https://example.test/usage", &headers)).unwrap();
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
    ///
    /// Behind the `contract` feature, because it has to outwait a coarse
    /// filesystem timestamp and there is no faking that: a second of wall clock
    /// in every `cargo test` is a second nobody chose to spend.
    #[cfg(feature = "contract")]
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

    /// `security` reports a failed sub-command of `-i` on stderr while still
    /// exiting 0, which is the whole reason its stderr is read at all. The
    /// check used to be for the word "error", and its own failure lines
    /// routinely do not carry one — so the failures it exists to catch were
    /// exactly the ones it let through.
    #[test]
    fn a_complaint_from_security_is_recognised_without_the_word_error() {
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

    /// The temp path a replacement is written at sits beside the target and
    /// carries this process's id, so it is guessable rather than secret.
    /// `CLAUDE_CONFIG_DIR` is taken verbatim and can name a directory somebody
    /// else may write to, so a symlink planted there would have Perch write the
    /// Identity through it to a file of the attacker's choosing. What stops it
    /// is not the name: the file is unlinked and created afresh with `O_EXCL`,
    /// which is the shape the Credential writer always used.
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
        let expected = r#"data-binary = "{\"refresh_token\":\"sk-ant-ort01-\\\"odd\\\"\"}""#;

        assert!(config.contains("url = \"https://example.test/token\""));
        assert!(config.contains(expected), "{config}");
        // The two bounds, the URL and the body: one option per line, and a body
        // carrying quotes and backslashes still occupying exactly one of them.
        assert_eq!(config.lines().count(), 4, "one option per line: {config}");
    }

    /// A request that says how long it may take gets that, and one that does
    /// not gets the ordinary bound.
    ///
    /// Both in the configuration rather than in `CURL_ARGS`, which is where
    /// they were: the upgrade check on `perch --version` is a line nobody asked
    /// for, and thirty seconds of a black-holed network is a great deal to
    /// spend on one (ADR 0039).
    #[test]
    fn a_request_may_carry_its_own_bound_and_otherwise_gets_the_ordinary_one() {
        let ordinary = curl_config(&HttpRequest::get("https://example.test/usage", &[])).unwrap();
        assert!(ordinary.contains("connect-timeout = 10"), "{ordinary}");
        assert!(ordinary.contains("max-time = 30"), "{ordinary}");

        let brief =
            curl_config(&HttpRequest::get("https://example.test/latest", &[]).within(2_000))
                .unwrap();
        assert!(brief.contains("connect-timeout = 2"), "{brief}");
        assert!(brief.contains("max-time = 2"), "{brief}");

        // Rounded up rather than down. A bound that became zero would be `curl`
        // reading it as no bound at all, which is the opposite of what asking
        // for a short one means.
        let sub_second =
            curl_config(&HttpRequest::get("https://example.test/latest", &[]).within(200)).unwrap();
        assert!(sub_second.contains("max-time = 1"), "{sub_second}");
    }

    /// The invariant `double_quoted` documents and only `security` was keeping:
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
}
