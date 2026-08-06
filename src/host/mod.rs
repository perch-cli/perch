//! The Host port: every effect Perch has outside its own process.
//!
//! Commands take a `&dyn Host` and nothing else, so behaviour tests drive the
//! real command code against [`fake::FakeHost`] and assert on outcomes rather
//! than on mocks. One trait, two implementations: [`real::RealHost`] and the
//! fake that records what it was asked to do.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::keychain::KeychainError;

pub mod fake;
pub mod real;

pub use fake::FakeHost;
pub use real::RealHost;

/// The result of running another program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Execution {
    pub fn succeeded(&self) -> bool {
        self.status == 0
    }
}

impl From<std::process::Output> for Execution {
    /// A killed process has no exit code, and `-1` is what Perch reads it as
    /// everywhere — said here so the three programs Perch runs cannot come to
    /// disagree about it.
    fn from(output: std::process::Output) -> Execution {
        Execution {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

/// One HTTP request, whole.
///
/// A request rather than a URL and some arguments beside it, because every part
/// of one has to travel the same way: an access token is a Credential, and a
/// header passed on a command line sits in `argv` where any process on the
/// machine can read it off the process table — the same reason a Credential
/// never reaches `security`'s command line (ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest<'a> {
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    /// The body to send, which is what makes the request a POST. `None` is a
    /// GET.
    pub body: Option<&'a str>,
}

impl<'a> HttpRequest<'a> {
    pub fn get(url: &'a str, headers: &'a [(&'a str, &'a str)]) -> Self {
        HttpRequest {
            url,
            headers,
            body: None,
        }
    }

    pub fn post(url: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a str) -> Self {
        HttpRequest {
            url,
            headers,
            body: Some(body),
        }
    }
}

/// A reply from an HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// The machine, to the resolution Perch cares about: macOS keeps secrets in a
/// keychain and no other platform does (ADR 0020), and Windows finds programs
/// through `PATHEXT` where everything else marks them executable.
///
/// An effect rather than a `cfg!`, so the behaviour tests can drive every
/// platform's Credential Store and program search whatever they are running
/// on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Other,
}

/// The permissions a file holding a Credential is created with, and the mode
/// anything looser is tightened to: the owner, and nobody else (ADR 0020).
pub const PRIVATE_FILE_MODE: u32 = 0o600;

/// The same for the directory it sits in. A directory others may enter is a
/// directory whose contents others may open.
pub const PRIVATE_DIR_MODE: u32 = 0o700;

/// A value as a double-quoted token, for the two line-oriented protocols Perch
/// writes: `curl`'s configuration file and `security -i`'s command lines.
///
/// Quoted so that spaces and `#` are part of the value rather than punctuation,
/// with backslashes and quotes escaped so JSON arrives as it was written. One
/// copy rather than two identical ones, because two copies is how one of them
/// gets fixed and the other does not.
///
/// It makes a value a *token*. It does not make it inert: neither protocol has
/// an escape for a newline, so a value that could carry one is refused where it
/// enters rather than quoted here.
pub fn double_quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Whether a mode lets anybody but the owner near the file.
pub fn is_private(mode: u32) -> bool {
    mode & 0o077 == 0
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{path} does not exist")]
    NotFound { path: PathBuf },

    /// A directory that was to be created by whoever got there first, and
    /// somebody else did. The whole of what makes a lock a lock.
    #[error("{path} already exists")]
    AlreadyExists { path: PathBuf },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub trait Host {
    // ---- clock ----------------------------------------------------------

    /// The current instant. Utilization is displayed as an observation with an
    /// age (ADR 0015), so the clock is an effect like any other.
    fn now(&self) -> DateTime<Utc>;

    // ---- environment ----------------------------------------------------

    /// The home directory, from `USERPROFILE` on Windows and `HOME` elsewhere
    /// — or an error when the platform's variable is unset, because a machine
    /// that cannot say where home is must be refused rather than quietly
    /// worked under the filesystem root.
    fn home_dir(&self) -> Result<PathBuf, HostError>;
    fn env_var(&self, key: &str) -> Option<String>;

    /// Which platform this is, which is what decides where a Credential is
    /// written (ADR 0020).
    fn platform(&self) -> Platform;

    // ---- filesystem -----------------------------------------------------

    fn read_file(&self, path: &Path) -> Result<String, HostError>;
    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError>;

    /// Writes a file created with exactly this mode, rather than with whatever
    /// the process umask happens to be.
    ///
    /// The mode belongs to the *creation* for the same reason it does in
    /// [`Host::write_private_file`]: a file opened and then `chmod`ed is a file
    /// that was briefly readable. Where a mode means nothing this is an
    /// ordinary write.
    fn create_file_with_mode(
        &self,
        path: &Path,
        contents: &str,
        mode: u32,
    ) -> Result<(), HostError>;

    /// Writes a file nobody but its owner can read, creating it — and any
    /// directory above it — with that mode rather than tightening it
    /// afterwards.
    ///
    /// A `chmod` after the fact leaves the secret on disk and readable for as
    /// long as the two calls take, which is the whole of what the mode is for
    /// (ADR 0020).
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError>;

    /// Creates a directory, and any above it, that nobody but its owner may
    /// enter — for the directories that will come to hold a Credential.
    ///
    /// Like `mkdir -p`, a directory that is already there keeps the mode it
    /// has: this sets a mode at creation and is not a `chmod` in disguise.
    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError>;

    /// A file's permission bits, or `None` on a platform that does not answer
    /// in those terms.
    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError>;

    /// Narrows an existing file to its owner. The one `chmod` Perch performs:
    /// files it creates get their mode at creation.
    fn make_private(&self, path: &Path) -> Result<(), HostError>;
    fn create_dir_all(&self, path: &Path) -> Result<(), HostError>;
    fn path_exists(&self, path: &Path) -> bool;

    /// Whether a path is a file, rather than absent or a directory — what a
    /// program search means by "found", where `path_exists` would let a
    /// directory that happens to carry the name win the walk.
    fn is_file(&self, path: &Path) -> bool;
    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError>;

    /// Creates a directory only if nobody else has, reporting
    /// [`HostError::AlreadyExists`] when somebody has. `mkdir` either creates a
    /// directory or fails, with no window in between, which is why Claude
    /// Code's lock artifacts are directories and why Perch's are too.
    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError>;

    /// When a path was last written. A lock artifact's age is what says whether
    /// the process that took it is still alive or died holding it.
    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError>;

    /// Marks a path as written now, without changing it. How a lock holder says
    /// it is still there within the interval the protocol allows.
    fn touch(&self, path: &Path) -> Result<(), HostError>;

    /// What a directory holds, as full paths. Absent directories report
    /// [`HostError::NotFound`] rather than emptiness: "nothing is running" and
    /// "nowhere to look" are different answers.
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, HostError>;

    /// Moves a path over another, replacing it. Within one filesystem this is
    /// the operation that has no half-way state, which is what
    /// [`write_atomically`] is built out of.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError>;

    /// Removes a file. A file that was not there is not a failure.
    fn remove_file(&self, path: &Path) -> Result<(), HostError>;

    // ---- keychain -------------------------------------------------------

    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError>;
    fn keychain_set(&self, service: &str, account: &str, secret: &str)
    -> Result<(), KeychainError>;
    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;

    // ---- processes ------------------------------------------------------

    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError>;

    /// Runs a program with the terminal attached and `env` added to its
    /// environment, returning its exit status. A login is a browser round trip
    /// the user drives, so this is the one execution Perch does not capture.
    fn exec_interactive(&self, program: &str, env: &[(&str, &str)]) -> Result<i32, HostError>;

    /// Whether a process is still running. A Live Profile's Credential is
    /// untouchable because something else is holding it.
    fn process_alive(&self, pid: u32) -> bool;

    /// When a process began, by the operating system's own account — or `None`
    /// when there is no saying, because the process is gone or the operating
    /// system will not answer for it.
    ///
    /// What corroborates a session marker (ADR 0022): a marker is evidence of
    /// a client only when the process it names began no later than the marker
    /// says the session did, because a recycled PID necessarily belongs to a
    /// process that began after the marker was written.
    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>>;

    /// Waits. Contending for a lock is the only thing Perch waits on, and it is
    /// an effect like any other so that tests do not spend the time.
    fn sleep(&self, millis: u64);

    // ---- the person at the terminal -------------------------------------

    /// Whether there is someone to answer a question. Every capability is
    /// available non-interactively, so a command that would have asked has to
    /// know when it cannot.
    fn is_interactive(&self) -> bool;

    /// One line of input, or `None` at end of input.
    fn read_line(&self) -> Result<Option<String>, HostError>;

    /// Says something the user should know that is not the answer to what they
    /// asked: a Credential written to the store Perch would rather not have
    /// used, a file found looser than it should be (ADR 0020).
    ///
    /// Said once. These are remarks about the state of the machine rather than
    /// about the command, and the same remark repeated for each of five
    /// Accounts teaches nobody anything the first one did not.
    fn note(&self, line: &str);

    // ---- network --------------------------------------------------------

    /// Sends one request and reads the whole reply. The only way out to
    /// Anthropic, and reached by nothing but `--refresh` (ADR 0015).
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError>;
}

/// Where the replacement for a file is written before it is moved over it.
///
/// Named after the process that is writing it. A single fixed name is one two
/// Perches writing the same file would collide on, and one anybody who can
/// write the directory can pre-plant something at — `CLAUDE_CONFIG_DIR` is
/// taken verbatim and can name a shared location. The pid is what randomness
/// buys here; a crate that generates a better one would want a real filesystem,
/// and this sits behind the Host port where there is not always one (ADR 0025).
pub fn temp_beside(path: &Path) -> PathBuf {
    let mut beside = path.as_os_str().to_os_string();
    beside.push(format!(".perch-tmp.{}", std::process::id()));
    PathBuf::from(beside)
}

/// The mode a replacement for this file should be created with: the one the
/// file already has, and the Credential mode for one that is not there yet.
///
/// A rename puts a *new* file at the path, so the mode of the old one is not
/// inherited — it has to be carried across deliberately. `.claude.json` holds
/// MCP configuration, and an MCP server entry routinely carries an API key in
/// its `env` block, so a file the user had narrowed must not come back at the
/// process umask; and a file Perch is the first to create is created closed
/// rather than open (ADR 0020).
fn mode_to_carry_across(host: &dyn Host, path: &Path) -> u32 {
    host.file_mode(path)
        .ok()
        .flatten()
        .unwrap_or(PRIVATE_FILE_MODE)
}

/// Replaces a file's contents in one step, or not at all.
///
/// Used for the files Perch does not own. `.claude.json` holds project history,
/// MCP configuration and settings — everything that belongs to the person
/// rather than the Account — and a Switch that died half way through writing it
/// would cost them all of that. Writing beside it and moving it into place
/// means the file is either the old one or the new one.
pub fn write_atomically(host: &dyn Host, path: &Path, contents: &str) -> Result<(), HostError> {
    // The temp file carries the target's mode from the moment it exists: it
    // holds the whole of the target's contents, so a temp file at the umask
    // would leak everything the mode on the target is protecting.
    replace_via_tmp(host, path, contents, mode_to_carry_across(host, path))
}

/// The whole of writing beside a file and moving the result over it, including
/// what is done about a write that did not land.
///
/// Shared by the secret and non-secret writes. Two copies of this is how the
/// failure cleanup comes to differ between the path that handles Credentials
/// and the path that does not — which is the wrong pair to let drift.
pub fn replace_via_tmp(
    host: &dyn Host,
    path: &Path,
    contents: &str,
    mode: u32,
) -> Result<(), HostError> {
    let beside = temp_beside(path);
    host.create_file_with_mode(&beside, contents, mode)?;
    match host.rename(&beside, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Nothing has replaced the original, so the half-written copy is
            // just litter — and litter beside a file Claude Code reads, or one
            // holding a Credential, is worth clearing even on the way out.
            let _ = host.remove_file(&beside);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "/Users/someone/.claude.json";

    #[test]
    fn a_replacement_is_created_with_the_mode_the_file_already_had() {
        for mode in [0o600, 0o644, 0o640] {
            let host = FakeHost::new()
                .with_file(PATH, "{}")
                .with_file_mode(PATH, mode);

            write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is replaced");

            assert_eq!(host.file(PATH).as_deref(), Some("{\"a\":1}"));
            assert_eq!(host.mode_of(PATH), Some(mode));
        }
    }

    /// Nothing is being carried across here, so the file is created closed:
    /// the alternative is deciding to publish, at the umask, a file whose
    /// contents Perch cannot see the end of.
    #[test]
    fn a_file_that_was_not_there_is_created_for_its_owner_alone() {
        let host = FakeHost::new();

        write_atomically(&host, Path::new(PATH), "{}").expect("it is written");

        assert_eq!(host.mode_of(PATH), Some(PRIVATE_FILE_MODE));
    }
}
