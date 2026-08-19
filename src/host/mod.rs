//! The Host port: every effect Perch has outside its own process.
//!
//! Commands take a `&dyn Host` and nothing else, so behavior tests drive the
//! real command code against [`fake::FakeHost`] and assert on outcomes rather
//! than on mocks. One port, two implementations: [`real::RealHost`] and the
//! fake that records what it was asked to do.
//!
//! [`Host`] declares nothing itself. It is the sum of nine traits, one per kind
//! of effect, and a reader who wants to know what a module touches of the
//! machine can read it off that module's signature — where one ever narrows.
//! Today none does: every consumer of this port reaches across several
//! concerns, because anything that touches the machine touches several of its
//! surfaces at once. The port is 42 methods wide because the machine is, and
//! ADR 0056 carries the table that says so, consumer by consumer. It is the
//! answer to the next reading that counts the methods and proposes cutting them
//! up to make somebody's signature smaller.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::keychain::KeychainError;
use zeroize::Zeroizing;

/// Behind a feature so it stays out of the binary somebody downloads. It is
/// only ever reached from a test, and the tests are integration tests — they
/// link this library as any other crate would, so `#[cfg(test)]` could not
/// carry it (see the `fakes` feature in `Cargo.toml`).
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
pub mod real;

#[cfg(any(test, feature = "fakes"))]
pub use fake::FakeHost;
pub use real::RealHost;

/// The result of running another program.
#[derive(Clone, PartialEq, Eq)]
pub struct Execution {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl std::fmt::Debug for Execution {
    /// Names the status and the sizes, never the bytes — the same redaction
    /// [`crate::credentials::StoredCredential`] and [`crate::anthropic::Fresh`]
    /// already hand-write, for the same secret one step earlier.
    ///
    /// This is the *first* shape a Credential takes: `security
    /// find-generic-password -w` answers with one on stdout, so an `Execution`
    /// returning from a keychain read is a struct holding a refresh token in a
    /// public field. A derived `Debug` would print it, and the two types that
    /// carry it afterwards had already been fixed while the one that carries it
    /// first was still derived.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Execution {{ status: {}, stdout: <{} bytes>, stderr: <{} bytes> }}",
            self.status,
            self.stdout.len(),
            self.stderr.len()
        )
    }
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
            stdout: taken_over(output.stdout),
            stderr: taken_over(output.stderr),
        }
    }
}

/// A child's output as a `String`, moving the buffer where it can.
///
/// `String::from_utf8_lossy(..).into_owned()` allocates and copies even when the
/// bytes are already valid UTF-8, which they are for every program Perch runs.
/// This takes the `Vec` the child's output already is and hands it straight over
/// in that case, falling back to the lossy read only where it has to.
///
/// The copy was worth removing for more than its cost. `security find-generic-password`
/// answers with a Credential on stdout, so the copy was a second buffer holding
/// a refresh token that nothing wipes and nothing reads again — one more place
/// for it to outlive the process in a core dump or a swap file.
fn taken_over(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(not_utf8) => String::from_utf8_lossy(not_utf8.as_bytes()).into_owned(),
    }
}

/// One HTTP request, whole.
///
/// A request rather than a URL and some arguments beside it, because every part
/// of one has to travel the same way: an access token is a Credential, and a
/// header passed on a command line sits in `argv` where any process on the
/// machine can read it off the process table — the same reason a Credential
/// never reaches `security`'s command line (ADR 0008).
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest<'a> {
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    /// The body to send, which is what makes the request a POST. `None` is a
    /// GET.
    pub body: Option<&'a str>,
    /// How long the whole request may take, or `None` for the ordinary bound.
    ///
    /// Part of the request rather than a setting on the Host for the same
    /// reason the headers are: the two callers want different answers. A
    /// Refresh is a figure somebody asked for and is worth waiting on; the
    /// upgrade check on `perch --version` is a line nobody asked for, and a
    /// black-holed network must cost it nothing (ADR 0039).
    pub within_millis: Option<u64>,
}

impl std::fmt::Debug for HttpRequest<'_> {
    /// The url and the header *names*, never a header value and never the body.
    /// The doc comment above says an access token is a Credential and travels
    /// as a header; the renewal's body is a refresh token outright
    /// ([`crate::anthropic::renew`]). A derived `Debug` printed both.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| *name).collect();
        write!(
            formatter,
            "HttpRequest {{ url: {:?}, headers: {:?}, body: <{} bytes>, within_millis: {:?} }}",
            self.url,
            names,
            self.body.map_or(0, str::len),
            self.within_millis
        )
    }
}

impl<'a> HttpRequest<'a> {
    pub fn get(url: &'a str, headers: &'a [(&'a str, &'a str)]) -> Self {
        HttpRequest {
            url,
            headers,
            body: None,
            within_millis: None,
        }
    }

    pub fn post(url: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a str) -> Self {
        HttpRequest {
            url,
            headers,
            body: Some(body),
            within_millis: None,
        }
    }

    /// The same request, given at most this long to finish.
    pub fn within(mut self, millis: u64) -> Self {
        self.within_millis = Some(millis);
        self
    }
}

/// A reply from an HTTP request.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl std::fmt::Debug for HttpResponse {
    /// The status and the size, never the body. The token endpoint answers a
    /// renewal with the new access token and the rotated refresh token in that
    /// body ([`crate::anthropic::renew`]), so this is the shape a Credential
    /// arrives in — redacted for the same reason the request that asked for it
    /// is.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "HttpResponse {{ status: {}, body: <{} bytes> }}",
            self.status,
            self.body.len()
        )
    }
}

/// The machine, to the resolution Perch cares about: macOS keeps secrets in a
/// keychain and no other platform does (ADR 0020), and Windows finds programs
/// through `PATHEXT` where everything else marks them executable.
///
/// An effect rather than a `cfg!`, so the behavior tests can drive every
/// platform's Credential Store and program search whatever they are running
/// on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Other,
}

/// How one path is made to stand for another, which is the whole of how Shared
/// State reaches the Profile a Run launches (ADR 0026).
///
/// Three kinds rather than one, because the platforms do not agree on which of
/// them a person may make: only symbolic links need Developer Mode or elevation
/// on Windows, and junctions and hard links need neither. Which kind is used
/// where is [`crate::reconcile`]'s decision; making one is the Host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// A path that names another path. What everything but Windows uses, and
    /// the kind that survives its target being replaced rather than merely
    /// edited — a new file at the same name is still the file it points at.
    Symbolic,
    /// Windows' link for a directory. Works without Developer Mode and without
    /// elevation, and exists nowhere else.
    Junction,
    /// A second name for the same file, which is why it is a share rather than
    /// a copy — and why it stops being one the moment the file it names is
    /// replaced rather than written through.
    Hard,
}

impl Link {
    /// How to say what could not be made, in a refusal a person has to act on.
    pub fn describe(self) -> &'static str {
        match self {
            Link::Symbolic => "a symbolic link",
            Link::Junction => "a directory junction",
            Link::Hard => "a hard link",
        }
    }
}

/// How a wait ended: on its own, or because the person at the terminal asked
/// the loop to stop.
///
/// Its own type rather than a bare `bool`, because the two callers of a bare
/// one would read it in opposite directions — a wait that "returned true" is
/// either one that completed or one that was cut short, and only the loop that
/// wrote it would know which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The whole of the wait passed.
    Fully,
    /// Ctrl-C arrived. Whatever the loop was doing is finished; it must not
    /// start anything else.
    Interrupted,
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

/// The refusal [`double_quoted`] says has to happen somewhere else.
///
/// A control character is what neither protocol can be told to treat as data,
/// so a value carrying one stops being a value and becomes further instructions:
/// a newline in a `curl` configuration ends the option it was in and begins
/// another, and `output =` writes a file while a second `url =` fetches one.
///
/// Perch does not author most of what goes through here. An access token is
/// read out of a JSON file Perch does not own, where `\n` is an ordinary
/// escape, so the value arriving with one is a thing that can happen rather
/// than a thing that would have to be contrived.
pub fn inert(what: &str, value: &str) -> Result<(), HostError> {
    match control_character_in(value) {
        Some(said) => Err(HostError::Other(format!(
            "{what} carries {said}, which the line it would be written on has \
             no way to hold as part of a value"
        ))),
        None => Ok(()),
    }
}

/// The first control character in a value, named the way a refusal names one.
///
/// Shared with [`crate::keychain`]'s own refusal, which is the same check about
/// the same hazard for the other line-oriented protocol Perch writes — its
/// companion [`double_quoted`] was deliberately made one copy on the reasoning
/// that "two copies is how one of them gets fixed and the other does not", and
/// this half was left as two. What each caller *says* about it stays theirs:
/// the two protocols break differently and the sentence explaining that is the
/// part worth having twice.
pub fn control_character_in(value: &str) -> Option<String> {
    value
        .chars()
        .find(|c| c.is_control())
        .map(|control| format!("a control character (U+{:04X})", control as u32))
}

/// A command as somebody would have typed it, for the line printed before it is
/// run or the one that says what failed.
///
/// Here because it is about what goes through [`Processes::exec`], and because
/// it was written twice — once in `upgrade` for the installer it drives and
/// once in `service` for the service manager it drives — with the same doc
/// comment reasoning above each. The two are read side by side in
/// `perch upgrade` and `perch watcher install`, so two spellings of "as
/// somebody would have typed it" is two answers to one question.
pub fn as_typed(program: &Path, args: &[String]) -> String {
    let mut line = program.to_string_lossy().into_owned();
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    line
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

/// What time it is, as an effect like any other so that a test can arrange the
/// age of an observation rather than wait for one.
pub trait Clock {
    /// The current instant. Utilization is displayed as an observation with an
    /// age (ADR 0015), so the clock is an effect like any other.
    fn now(&self) -> DateTime<Utc>;
}

/// Where this Perch is running: whose machine, which platform, and what it was
/// told by the environment it was started in.
pub trait Environment {
    /// The home directory, from `USERPROFILE` on Windows and `HOME` elsewhere
    /// — or an error when the platform's variable is unset, because a machine
    /// that cannot say where home is must be refused rather than quietly
    /// worked under the filesystem root.
    fn home_dir(&self) -> Result<PathBuf, HostError>;

    /// The directory this command was typed in, which is the project a Run is
    /// about: Claude Code keys the trust it was given and the tools it was
    /// allowed by exactly this path (ADR 0003).
    fn current_dir(&self) -> Result<PathBuf, HostError>;

    /// What a variable is set to, or `None` for one that is unset, empty, or
    /// set to something that is not text.
    ///
    /// Those last two are folded in with "unset" because there is nothing
    /// usable to hand back either way — every caller here wants a path or a
    /// name, and one Perch cannot spell cannot be joined or compared. A value
    /// that was *there* and could not be read is a remark on the way past, so
    /// being ignored is a mistake somebody can see rather than a mystery.
    fn env_var(&self, key: &str) -> Option<String>;

    /// Which platform this is, which is what decides where a Credential is
    /// written (ADR 0020).
    fn platform(&self) -> Platform;

    /// Where this Perch's own binary sits, with links followed.
    ///
    /// The only evidence there is about which Channel installed this machine's
    /// Installation: every Channel points at the same Release and installs the
    /// same bytes, so nothing inside the binary can say where it came from and
    /// the path it sits at has to (ADR 0039).
    ///
    /// Links followed because Homebrew's is the case that matters — what a
    /// person runs is `<prefix>/bin/perch`, which is a symlink into the Cellar,
    /// and the Cellar is the half that names the Channel.
    fn current_exe(&self) -> Result<PathBuf, HostError>;

    /// Which user this process is running as, or `None` where the platform has
    /// no such number.
    ///
    /// Two things need it, and both belong to `perch watcher` (ADR 0040). It is
    /// what the root refusal is read off — every Profile Perch holds is under
    /// one person's home directory, so a Service installed by root is one
    /// watching a registry it does not own, and on macOS one with no unlocked
    /// keychain to read (ADR 0008). And it names the domain a LaunchAgent is
    /// bootstrapped into: `gui/<uid>` is the logged-in session, which is the
    /// only place a Service belongs.
    ///
    /// `None` on Windows, where a logon task is registered against a named user
    /// and there is neither a uid to quote nor a root to refuse.
    fn user_id(&self) -> Option<u32>;
}

/// The filesystem, to the resolution a Credential's mode and a lock's
/// atomicity need of it.
pub trait Files {
    fn read_file(&self, path: &Path) -> Result<String, HostError>;

    /// Writes a file created with exactly this mode, rather than with whatever
    /// the process umask happens to be.
    ///
    /// The mode belongs to the *creation* for the same reason it does in
    /// [`Files::write_private_file`]: a file opened and then `chmod`ed is a file
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
}

/// How one path is made to stand for another. [`Link`] says which kinds there
/// are and [`crate::reconcile`] chooses between them; making, reading and
/// removing one is the machine's.
pub trait Links {
    /// Makes `at` a link of `kind` standing for `target`.
    ///
    /// The one way Shared State reaches a Run's Profile, because it is the only
    /// one that cannot diverge (ADR 0026). A kind the platform will not make is
    /// an error rather than a quieter kind substituted for it: which link was
    /// made decides what happens when the target is replaced, so the caller
    /// chooses and hears about it.
    fn link(&self, kind: Link, target: &Path, at: &Path) -> Result<(), HostError>;

    /// What a path links to, or `None` when what is there is not a link.
    ///
    /// Answers for the link itself rather than for what it points at, so a link
    /// whose target has gone is still a link — which is the whole of how a
    /// broken one is found and repaired. [`HostError::NotFound`] means nothing
    /// is there at all, which is a third answer and a different repair.
    ///
    /// A hard link is not a link here, and cannot be: it is a name for a file,
    /// indistinguishable from the file's first name.
    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError>;

    /// Removes a link without touching what it points at.
    ///
    /// Its own operation because the platforms disagree about which call takes
    /// one: a Windows junction is removed as a directory and a file symlink as
    /// a file, and `remove_dir_all` on either would be a walk into somebody
    /// else's directory.
    fn remove_link(&self, path: &Path) -> Result<(), HostError>;
}

/// The filesystem and the links into it, together.
///
/// The one narrowing of this port that anything takes: `tests/conformance.rs`
/// drives both adapters through a scratch directory, and these two concerns are
/// exactly what a scratch directory can drive — nineteen methods, against the
/// clock, the keychain, the processes, the terminal and the network, which are
/// either the machine's own state or the very things a fake exists to invent.
/// That suite's table takes a `&dyn Filesystem` so its claim about its own reach
/// is a signature rather than a paragraph, and a case there that reaches for
/// [`Clock::now`] does not compile (ADR 0056).
pub trait Filesystem: Files + Links {}

/// The Credential Store macOS keeps, as `/usr/bin/security` presents it — which
/// is the binary Claude Code drives, and so the one Perch has to drive too
/// (ADR 0008).
pub trait Keys {
    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError>;
    fn keychain_set(&self, service: &str, account: &str, secret: &str)
    -> Result<(), KeychainError>;
    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;
}

/// Other programs, and the processes Perch reads to find out what is holding
/// what.
pub trait Processes {
    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError>;

    /// Runs a program with the terminal attached and `env` added to its
    /// environment, returning its exit status.
    ///
    /// The one execution Perch does not capture, because both callers are
    /// somebody's session rather than something Perch reads: a login is a
    /// browser round trip the user drives, and a Run is whatever they asked to
    /// have launched as an Account.
    ///
    /// `args` reach the program as they were given, one word per element:
    /// nothing here re-quotes them, splits them or reads them, because a Run
    /// forwards what somebody typed after `--` and a wrapper that interpreted it
    /// would be a second parser between them and their own command line.
    fn exec_interactive(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<i32, HostError>;

    /// This process, as the operating system names it.
    ///
    /// What a Run marks its Profile Live with (ADR 0027): Perch waits for the
    /// program it launched, so its own pid is alive for exactly as long as the
    /// Run and no longer — and it is knowable *before* the launch, where the
    /// child's is not.
    fn process_id(&self) -> u32;

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
}

/// Time passing, and being asked to stop waiting.
///
/// One trait rather than two, because [`Waited::Interrupted`] means nothing
/// until [`Waiting::listen_for_interrupts`] has been called — so either the two
/// sit together or the ordering between them is unsayable. `sleep` is filed here
/// rather than among the processes it used to sit with: it is [`crate::lock`]'s
/// contention wait and has nothing to do with a process (ADR 0056).
pub trait Waiting {
    /// Waits. Contending for a lock is the only thing Perch waits on, and it is
    /// an effect like any other so that tests do not spend the time.
    fn sleep(&self, millis: u64);

    /// Starts listening for a loop being asked to stop — by the person at the
    /// terminal, or by the service manager running it.
    ///
    /// `perch watcher run` calls this and nothing else does: every other
    /// command is over long before anybody could ask, and Ctrl-C during one of
    /// them is a process killed where it stands (ADR 0013).
    ///
    /// Both askings are the same request and are answered in the same place.
    /// Ctrl-C is the person's; `SIGTERM` is what systemd and launchd send to
    /// stop a Service, and a loop that did not claim it would be killed mid
    /// Switch (ADR 0040).
    fn listen_for_interrupts(&self);

    /// Waits up to `millis`, and stops waiting the moment that has been asked
    /// for.
    ///
    /// Its own effect rather than a [`Waiting::sleep`] with a check around it,
    /// because the whole of what makes a foreground watcher killable is that
    /// the wait ends when Ctrl-C arrives rather than two and a half minutes
    /// later. Nothing may be held across it: what the watcher does between
    /// waits is a Switch under Claude Code's locks, and the wait is where it
    /// holds nothing at all, so a process killed here leaves no marker, no lock
    /// and no half-written Credential.
    fn wait(&self, millis: u64) -> Waited;
}

/// The person at the terminal, where there is one.
pub trait Terminal {
    /// Whether there is someone to answer a question. Every capability is
    /// available non-interactively, so a command that would have asked has to
    /// know when it cannot.
    fn is_interactive(&self) -> bool;

    /// One line of input, or `None` at end of input.
    fn read_line(&self) -> Result<Option<String>, HostError>;

    /// One line of input that is never shown as it is typed, or `None` at end of
    /// input.
    ///
    /// Its own effect rather than a flag on [`Terminal::read_line`], because the
    /// caller who forgets the flag is the caller who writes somebody's export
    /// passphrase into their scrollback — and because turning the terminal's
    /// echo off and back on again is a platform primitive, which is what this
    /// port is for. A platform with no way to hide what is typed refuses rather
    /// than showing it (ADR 0014).
    ///
    /// `Zeroizing` in the signature rather than left to the caller, which is
    /// where it used to be: `commands::ask_secret` wrapped the answer and its
    /// comment claimed the wrapped buffer was the one the terminal was read
    /// into. It was not, and nothing about the type said so. An adapter owes
    /// this now, and the obligation is written where an adapter has to read it.
    fn read_secret(&self) -> Result<Option<Zeroizing<String>>, HostError>;

    /// Says something the user should know that is not the answer to what they
    /// asked: a Credential written to the store Perch would rather not have
    /// used, a file found looser than it should be (ADR 0020).
    ///
    /// Said once. These are remarks about the state of the machine rather than
    /// about the command, and the same remark repeated for each of five
    /// Accounts teaches nobody anything the first one did not.
    fn note(&self, line: &str);
}

/// The way out to Anthropic, and the only one.
pub trait Network {
    /// Sends one request and reads the whole reply. The only way out to
    /// Anthropic, and reached by nothing but `--refresh` (ADR 0015).
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError>;
}

/// Every effect Perch has outside its own process.
///
/// The sum of the nine concerns above and nothing of its own. Commands and
/// domain modules take a `&dyn Host`, which is the one port Perch has
/// (ADR 0025) — the nine are names for its surfaces rather than nine ports, and
/// a `&dyn Host` still reaches every one of them. ADR 0056 has the reasoning,
/// including why no consumer of this port names fewer than three of the nine.
pub trait Host:
    Clock + Environment + Filesystem + Keys + Processes + Waiting + Terminal + Network
{
}

/// Everything that has to be in scope to reach this port's methods on a
/// **concrete** adapter.
///
/// A `&dyn Host` needs none of it. A trait object finds its supertraits' methods
/// without them being imported, which is why nine traits cost the 233 places
/// that hold a `&dyn Host` exactly nothing. Holding a `FakeHost` or a
/// `RealHost` is the other case: `host.now()` on a concrete type is
/// [`Clock::now`], and `Clock` has to be in scope to be found. That is every
/// test in the tree and nothing else, since production builds a `RealHost` once
/// and passes a `&dyn Host` from there on.
///
/// A glob rather than nine imports per file, because the list a file would write
/// out is not a fact about the file — a test that arranges a world and reads
/// back what happened to it touches most of the machine by definition, and
/// spelling out which nine-tenths would be noise a reader has to check against
/// the body (ADR 0056).
pub mod prelude {
    pub use super::{
        Clock, Environment, Files, Filesystem, Host, Keys, Links, Network, Processes, Terminal,
        Waiting,
    };
}

/// Where the replacement for a file is written before it is moved over it.
///
/// Named after the process that is writing it. A single fixed name is one two
/// Perches writing the same file would collide on, and one anybody who can
/// write the directory can pre-plant something at — `CLAUDE_CONFIG_DIR` is
/// taken verbatim and can name a shared location. The pid is what randomness
/// buys here; a crate that generates a better one would want a real filesystem,
/// and this sits behind the Host port where there is not always one (ADR 0025).
///
/// The pid comes through the port rather than from `std::process::id`, because
/// the one thing this name has to be is *this process's*, and behind a fake
/// there is a different answer to who that is. Taken from the process directly,
/// a fake whose Runs write session markers naming pid 700 wrote its temp files
/// under the pid of the test binary — so the collision this name exists to
/// prevent was the one thing it could not model, and no two runs of the suite
/// produced the same paths.
pub fn temp_beside(host: &dyn Host, path: &Path) -> PathBuf {
    let mut beside = path.as_os_str().to_os_string();
    beside.push(format!(".perch-tmp.{}", host.process_id()));
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
    let path = &through_any_link(host, path);
    // The temp file carries the target's mode from the moment it exists: it
    // holds the whole of the target's contents, so a temp file at the umask
    // would leak everything the mode on the target is protecting.
    replace_via_tmp(host, path, contents, mode_to_carry_across(host, path))
}

/// What a path names, following one symbolic link if the last component is one.
///
/// `rename` replaces the **link**, not what it points at, so without this the
/// first `perch switch` on a machine where `~/.claude.json` is managed by stow,
/// chezmoi or yadm silently turned that link into an ordinary file: the copy in
/// the user's dotfiles repository stops being the live one, and every edit they
/// make there afterwards does nothing. Nothing says so, and the repair is to
/// notice.
///
/// One hop rather than a full canonicalization, because one hop is what a
/// dotfile manager makes and a walk would need a loop guard for no case anybody
/// has. A relative target is resolved against the directory the link sits in,
/// which is what the operating system does with it.
///
/// Deliberately not in [`replace_via_tmp`]: that is shared with the write that
/// stores a Credential, and following a link to decide where a *secret* lands
/// is how a planted link redirects one. `.claude.json` is the user's own
/// configuration and the link is theirs.
pub(crate) fn through_any_link(host: &dyn Host, path: &Path) -> PathBuf {
    let Ok(Some(target)) = host.link_target(path) else {
        return path.to_path_buf();
    };
    match target.is_absolute() {
        true => target,
        false => path.parent().unwrap_or(Path::new("")).join(target),
    }
}

/// The same, following every link on the path rather than only one at the end.
///
/// [`through_any_link`] answers about the last component, which is the question
/// a Reconcile asks — it holds the entry it is about. "Is this path inside that
/// directory?" is a different question, and a link *above* the last component
/// defeats a components-wise `starts_with` just as thoroughly: `~/claude` linked
/// at `~/.config/perch/profiles` makes `~/claude/work` and
/// `~/.config/perch/profiles/work` one directory with two spellings.
///
/// Bounded rather than trusting the filesystem to be acyclic, the way
/// `FakeHost::resolved` is: two links pointing at each other are a loop, and a
/// path this cannot settle in eight passes is answered as far as it got. The
/// caller is comparing rather than opening, so a partial answer is a comparison
/// that fails closed.
pub(crate) fn through_every_link(host: &dyn Host, path: &Path) -> PathBuf {
    const FOLLOWED: usize = 8;

    let mut at = path.to_path_buf();
    for _ in 0..FOLLOWED {
        match deepest_link_on(host, &at) {
            Some(followed) => at = followed,
            None => return at,
        }
    }
    at
}

/// The deepest component of `path` that is a link, replaced by what it points
/// at and the rest of the path put back on the end. `None` when none of them is
/// a link, which is what ends the walk above.
fn deepest_link_on(host: &dyn Host, path: &Path) -> Option<PathBuf> {
    let mut beneath = PathBuf::new();
    let mut at = path.to_path_buf();
    loop {
        if let Ok(Some(target)) = host.link_target(&at) {
            let resolved = match target.is_absolute() {
                true => target,
                false => at.parent().unwrap_or(Path::new("")).join(target),
            };
            return Some(resolved.join(&beneath));
        }
        let name = at.file_name()?;
        beneath = Path::new(name).join(&beneath);
        at = at.parent()?.to_path_buf();
        if at.as_os_str().is_empty() {
            return None;
        }
    }
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
    let beside = temp_beside(host, path);
    // Nothing has replaced the original, so whatever is at `beside` is just
    // litter — and litter beside a file Claude Code reads, or one holding a
    // Credential, is worth clearing even on the way out.
    //
    // Both failures, not only the rename. A write is `create_new`, then
    // `write_all`, then `sync_all`, and a disk that fills between the first and
    // the last leaves a real, partially-written file at
    // `<target>.perch-tmp.<pid>` that nothing removed: a truncated
    // `.credentials.json` holding a fragment of a refresh token, a truncated
    // `.claude.json`, or a truncated Export holding some of every Credential on
    // the machine. None of them is ever mentioned, and nothing looks at that
    // path again — `perch holdings export` refuses an occupied path by looking
    // at the target alone.
    let written = host.create_file_with_mode(&beside, contents, mode);
    match written.and_then(|()| host.rename(&beside, path)) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = host.remove_file(&beside);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::fake::Effect;

    const PATH: &str = "/Users/someone/.claude.json";

    /// The three shapes a Credential passes through on its way in and out of
    /// this port. Each one is asserted on the secret's absence rather than on
    /// the exact wording, so a `Debug` somebody reformats stays honest.
    #[test]
    fn nothing_that_carries_a_credential_prints_one() {
        const SECRET: &str = "sk-ant-oat01-a-live-refresh-token";

        let read_from_the_keychain = Execution {
            status: 0,
            stdout: SECRET.to_string(),
            stderr: String::new(),
        };
        let authorization = format!("Bearer {SECRET}");
        let headers = [("Authorization", authorization.as_str())];
        let renewal = HttpRequest::post(
            "https://console.anthropic.com/v1/oauth/token",
            &headers,
            SECRET,
        );
        let rotated = HttpResponse {
            status: 200,
            body: format!("{{\"refresh_token\":\"{SECRET}\"}}"),
        };

        for printed in [
            format!("{read_from_the_keychain:?}"),
            format!("{renewal:?}"),
            format!("{rotated:?}"),
        ] {
            assert!(!printed.contains(SECRET), "printed a Credential: {printed}");
        }

        // The header *name* still prints, because knowing a request carried an
        // Authorization is the half of it that is worth having.
        assert!(format!("{renewal:?}").contains("Authorization"));
    }

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

    /// A managed `.claude.json` is written *through*, not replaced.
    ///
    /// `rename` replaces the link rather than what it points at, so the first
    /// `perch switch` on a machine using stow, chezmoi or yadm turned the link
    /// into an ordinary file — the copy in the dotfiles repository stops being
    /// the live one, silently, and every edit made there afterwards does
    /// nothing.
    #[test]
    fn a_file_that_is_a_link_is_written_through_rather_than_replaced() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new()
            .with_file(real, "{}")
            .with_link(Link::Symbolic, real, PATH);

        write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is written");

        assert_eq!(
            host.file(real).as_deref(),
            Some("{\"a\":1}"),
            "the file the link names is the one that changed"
        );
        assert!(
            host.link_at(PATH).is_some(),
            "and the link is still a link, so what manages it goes on managing it"
        );
    }

    /// `remove_link` takes away a link and nothing else, and the "nothing else"
    /// is the part that has to be checked.
    ///
    /// It was `remove_file` on a bare path, which deletes whatever is there —
    /// so the one call whose whole contract is that it only ever takes away
    /// Perch's own share would have taken away the person's file at the same
    /// name. Its only caller asks `link_target` first, but that is a separate
    /// syscall, and the fake answered `Ok` untouched for a plain file: a
    /// regression that dropped the guard passed every behavior test and deleted
    /// a file on the machine.
    #[test]
    fn removing_a_link_refuses_anything_that_is_not_one() {
        let host = FakeHost::new().with_file(PATH, "{\"mine\":true}");

        host.remove_link(Path::new(PATH))
            .expect_err("a file is not a link");

        assert_eq!(
            host.file(PATH).as_deref(),
            Some("{\"mine\":true}"),
            "and it is still there"
        );
    }

    /// The two ends of the same rule: a link goes, and a path with nothing at it
    /// is not a failure — a share that has already gone is a share that is gone.
    #[test]
    fn removing_a_link_takes_the_link_and_leaves_what_it_points_at() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new()
            .with_file(real, "{}")
            .with_link(Link::Symbolic, real, PATH);

        host.remove_link(Path::new(PATH)).expect("a link goes");
        host.remove_link(Path::new(PATH))
            .expect("and one that has already gone is not a failure");

        assert!(host.link_at(PATH).is_none(), "the link is gone");
        assert_eq!(
            host.file(real).as_deref(),
            Some("{}"),
            "and what it pointed at is untouched"
        );
    }

    /// A private write is the same choreography, and the fake performs it
    /// rather than describing it.
    ///
    /// Worth asserting because the fake used to write straight to the path. The
    /// real host's failure and the fake's landed in different places — a full
    /// disk stops the real one at the copy *beside* the target, leaving the
    /// target untouched, while the fake stopped at the target itself — so the
    /// registry's "no half-written copy is left beside it" test was asserting
    /// the absence of something the fake could not have created, and the whole
    /// suite would have stayed green through a `RealHost::write_private_file`
    /// rewritten as a plain truncate-and-write.
    #[test]
    fn a_private_write_is_created_beside_the_file_and_moved_over_it() {
        let host = FakeHost::new().with_file(PATH, "{}");
        let beside = temp_beside(&host, Path::new(PATH));
        host.forget_effects();

        host.write_private_file(Path::new(PATH), "{\"a\":1}")
            .expect("it is written");

        assert!(
            host.effects().iter().any(|effect| matches!(
                effect,
                Effect::Renamed { from, to } if from == &beside && to == Path::new(PATH)
            )),
            "the file at the path is one that arrived whole: {:?}",
            host.effects()
        );
        assert_eq!(host.file(PATH).as_deref(), Some("{\"a\":1}"));
        assert_eq!(host.mode_of(PATH), Some(PRIVATE_FILE_MODE));
        assert_eq!(host.file(&beside), None, "and nothing is left beside it");
    }

    /// The write dying partway, rather than never starting.
    ///
    /// The cleanup only ran on a failed *rename*, so a disk that filled between
    /// the open and the `sync_all` left a real, partially-written file at
    /// `<target>.perch-tmp.<pid>` that nothing removed and nothing mentions —
    /// a fragment of a refresh token beside `.credentials.json`, or some of
    /// every Credential on the machine beside the path an Export names. The
    /// original is untouched either way, which is what makes the leftover
    /// litter rather than a half-applied write.
    #[test]
    fn a_write_that_dies_partway_leaves_nothing_beside_the_file_either() {
        let host = FakeHost::new()
            .with_file(PATH, "{}")
            .with_a_disk_that_fills_writing(PATH);
        let beside = temp_beside(&host, Path::new(PATH));

        let failed = host
            .write_private_file(Path::new(PATH), "{\"a\":1}")
            .expect_err("the disk fills partway through");

        assert!(failed.to_string().contains("No space left"), "{failed}");
        assert_eq!(
            host.file(&beside),
            None,
            "the bytes that fitted are cleared rather than left beside the file"
        );
        assert_eq!(
            host.file(PATH).as_deref(),
            Some("{}"),
            "and the original is exactly as it was"
        );
    }

    /// A relative target is resolved the way the operating system resolves it:
    /// against the directory the link sits in, not the working directory.
    #[test]
    fn a_link_pointing_somewhere_relative_is_followed_from_where_it_sits() {
        let real = "/Users/someone/dotfiles/claude.json";
        let host = FakeHost::new().with_file(real, "{}").with_link(
            Link::Symbolic,
            "dotfiles/claude.json",
            PATH,
        );

        write_atomically(&host, Path::new(PATH), "{\"a\":1}").expect("it is written");

        assert_eq!(host.file(real).as_deref(), Some("{\"a\":1}"));
    }

    /// A child's output is the buffer it already is wherever it can be, and the
    /// lossy read only where it has to be.
    ///
    /// `security find-generic-password` answers with a Credential on stdout, so
    /// the copy this avoids was a second heap buffer holding a refresh token
    /// that nothing wipes — and the fallback still has to hand back *something*
    /// for a reply that is not UTF-8, because a caller that got no answer would
    /// read it as a store holding nothing.
    #[test]
    fn the_output_of_a_child_is_taken_over_where_it_is_already_text() {
        assert_eq!(
            taken_over(b"sk-ant-oat01-test".to_vec()),
            "sk-ant-oat01-test"
        );
        assert_eq!(taken_over(Vec::new()), "");
        // A lone continuation byte, which no UTF-8 string holds.
        assert_eq!(taken_over(vec![b'a', 0x80, b'b']), "a\u{fffd}b");
    }
}
