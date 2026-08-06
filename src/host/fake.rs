//! A Host that keeps the world in memory and records what it was asked to do.
//!
//! Behaviour tests drive real command code against this and assert on
//! observable outcomes: what was printed, what ended up in the keychain, and
//! what went out to the network. A machine with no arranged replies has no
//! network at all, so a command that fetches when it should not fails here
//! rather than quietly passing (ADR 0015).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

use super::{
    Execution, Host, HostError, HttpRequest, HttpResponse, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE,
    Platform,
};
use crate::keychain::KeychainError;

/// One effect the fake was asked to perform, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    ReadFile(PathBuf),
    WroteFile(PathBuf),
    /// A file created for its owner alone: a Credential, or nothing.
    WrotePrivateFile(PathBuf),
    /// A file that was there already and has been narrowed to its owner.
    MadePrivate(PathBuf),
    CreatedDir(PathBuf),
    /// A directory created only if nobody had: an acquired lock.
    Took(PathBuf),
    RemovedDir(PathBuf),
    RemovedFile(PathBuf),
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
    Touched(PathBuf),
    Slept {
        millis: u64,
    },
    KeychainGet {
        service: String,
        account: String,
    },
    KeychainSet {
        service: String,
        account: String,
    },
    KeychainDelete {
        service: String,
        account: String,
    },
    Exec {
        program: String,
        args: Vec<String>,
    },
    ExecInteractive {
        program: String,
        config_dir: PathBuf,
    },
    /// A request that went out to the network.
    Http {
        url: String,
    },
    /// A question put to the person at the terminal. Recorded so a command that
    /// must never ask one — bare `perch switch` (ADR 0011) — can be held to it.
    Asked,
}

/// One request the fake was asked to send, kept whole so a test can say what
/// went out as well as where it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl Sent {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The access token the request carried, if it carried one.
    pub fn bearer(&self) -> Option<&str> {
        self.header("Authorization")?.strip_prefix("Bearer ")
    }
}

/// What an ordinary directory is created as — `mkdir` under the usual umask.
/// A directory that will hold a Credential is not this (ADR 0020).
const ORDINARY_DIR_MODE: u32 = 0o755;

/// Why the keychain refuses everything, when a test asks it to.
#[derive(Debug, Clone)]
pub struct KeychainLock {
    pub detail: String,
}

/// What a login does when Perch launches one: whatever Claude Code would have
/// left behind in the config directory it was pointed at, and the status it
/// exited with. Written as a closure so the derivation of a Profile's keychain
/// namespace stays in [`crate::probe`] and out of the fake.
pub type Login = Box<dyn Fn(&FakeHost, &Path) -> i32>;

/// Something that happens while Perch waits.
///
/// Contending for a lock is the only thing Perch waits on, and the world does
/// not stand still meanwhile: a `claude` started during that wait is exactly
/// what the questions asked *under* the locks are asked again for. Without this
/// the fake can only present a world that was already settled before the
/// command began.
pub type WhileWaiting = Box<dyn Fn(&FakeHost)>;

pub struct FakeHost {
    home: PathBuf,
    now: RefCell<DateTime<Utc>>,
    platform: RefCell<Platform>,
    env: RefCell<BTreeMap<String, String>>,
    files: RefCell<BTreeMap<PathBuf, String>>,
    /// The permissions of everything that has any, so a test can say that a
    /// Credential was never briefly readable by anyone else.
    modes: RefCell<BTreeMap<PathBuf, u32>>,
    notes: RefCell<Vec<String>>,
    unreadable: RefCell<BTreeMap<PathBuf, String>>,
    unwritable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths that will not give up what they hold. Distinct from an unwritable
    /// one: a store that refuses a write is routinely one a superseded copy can
    /// still be cleared out of.
    undeletable: RefCell<BTreeMap<PathBuf, String>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    modified: RefCell<BTreeMap<PathBuf, DateTime<Utc>>>,
    keychain: RefCell<BTreeMap<(String, String), String>>,
    keychain_lock: RefCell<Option<KeychainLock>>,
    executions: RefCell<BTreeMap<String, Execution>>,
    login: RefCell<Option<Login>>,
    /// What happens the first time Perch waits, and then does not happen again.
    while_waiting: RefCell<Option<WhileWaiting>>,
    /// The running processes, each with when it began — or `None` for one whose
    /// start the operating system will not say.
    live_processes: RefCell<BTreeMap<u32, Option<DateTime<Utc>>>>,
    interactive: RefCell<bool>,
    answers: RefCell<VecDeque<String>>,
    /// What each endpoint answers, by URL and by the access token that asked.
    replies: RefCell<BTreeMap<(String, Option<String>), HttpResponse>>,
    sent: RefCell<Vec<Sent>>,
    effects: RefCell<Vec<Effect>>,
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    pub fn new() -> Self {
        let home = PathBuf::from("/Users/someone");
        let mut env = BTreeMap::new();
        env.insert("USER".to_string(), "someone".to_string());
        FakeHost {
            home,
            now: RefCell::new(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()),
            // The platform Perch was written for first, so a test says which
            // Credential Store it is about only when that is what it is about.
            platform: RefCell::new(Platform::MacOs),
            env: RefCell::new(env),
            files: RefCell::new(BTreeMap::new()),
            modes: RefCell::new(BTreeMap::new()),
            notes: RefCell::new(Vec::new()),
            unreadable: RefCell::new(BTreeMap::new()),
            unwritable: RefCell::new(BTreeMap::new()),
            undeletable: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            modified: RefCell::new(BTreeMap::new()),
            keychain: RefCell::new(BTreeMap::new()),
            keychain_lock: RefCell::new(None),
            executions: RefCell::new(BTreeMap::new()),
            login: RefCell::new(None),
            while_waiting: RefCell::new(None),
            live_processes: RefCell::new(BTreeMap::new()),
            interactive: RefCell::new(true),
            answers: RefCell::new(VecDeque::new()),
            replies: RefCell::new(BTreeMap::new()),
            sent: RefCell::new(Vec::new()),
            effects: RefCell::new(Vec::new()),
        }
    }

    // ---- arranging the world -------------------------------------------

    pub fn with_now(self, now: DateTime<Utc>) -> Self {
        self.set_now(now);
        self
    }

    /// Moves the clock, for a test where two commands run at different times —
    /// a figure read now and looked at again three minutes later.
    pub fn set_now(&self, now: DateTime<Utc>) {
        *self.now.borrow_mut() = now;
    }

    /// A machine that is not a Mac, where a Credential lives in a file rather
    /// than in a keychain (ADR 0020).
    pub fn with_platform(self, platform: Platform) -> Self {
        *self.platform.borrow_mut() = platform;
        self
    }

    pub fn with_file(self, path: impl AsRef<Path>, contents: &str) -> Self {
        self.set_file(path, contents);
        self
    }

    /// An environment variable the machine has set.
    pub fn with_env(self, key: &str, value: &str) -> Self {
        self.env
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
        self
    }

    /// A variable the machine does not have — `USER` on Windows, most notably.
    pub fn without_env(self, key: &str) -> Self {
        self.env.borrow_mut().remove(key);
        self
    }

    /// A file somebody else could read: one written by an older Claude Code,
    /// or restored from a backup that forgot its mode.
    pub fn with_file_mode(self, path: impl AsRef<Path>, mode: u32) -> Self {
        self.modes
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), mode);
        self
    }

    /// The same, for a world being arranged from inside a [`Login`], where the
    /// fake is already borrowed and cannot be consumed.
    pub fn set_file(&self, path: impl AsRef<Path>, contents: &str) {
        self.note_directories_of(path.as_ref());
        self.files
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), contents.to_string());
    }

    /// A file exists in the directories above it, so they exist too. Without
    /// this a directory could hold something and still not be there to list.
    fn note_directories_of(&self, path: &Path) {
        let mut dirs = self.dirs.borrow_mut();
        let mut at = path.parent();
        while let Some(dir) = at {
            if dir.as_os_str().is_empty() {
                break;
            }
            dirs.insert(dir.to_path_buf());
            at = dir.parent();
        }
    }

    /// A file that is there but cannot be read — the wrong permissions, most
    /// often. Distinct from a file that is simply absent.
    pub fn with_unreadable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.unreadable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// A path that cannot be written to, so a test can fail one step of a
    /// multi-step write and see what is left behind.
    pub fn with_unwritable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.unwritable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// A file that is there and will not go — a directory whose permissions
    /// forbid it, a lock some other process holds on Windows. What a Credential
    /// Store that cannot be emptied looks like.
    pub fn with_undeletable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.undeletable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// Whatever stopped a path being written to has been put right — the
    /// permission fixed, the disk freed — so a test can carry on from a failure
    /// with the world it left rather than a fresh one.
    pub fn writable_again(&self, path: impl AsRef<Path>) {
        self.unwritable.borrow_mut().remove(path.as_ref());
    }

    pub fn with_keychain_item(self, service: &str, account: &str, secret: &str) -> Self {
        self.set_keychain_item(service, account, secret);
        self
    }

    pub fn set_keychain_item(&self, service: &str, account: &str, secret: &str) {
        self.keychain.borrow_mut().insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
    }

    /// An item that is gone from the keychain — an Account whose Credential
    /// nothing Perch holds can recover.
    pub fn forget_keychain_item(&self, service: &str, account: &str) {
        self.keychain
            .borrow_mut()
            .remove(&(service.to_string(), account.to_string()));
    }

    /// Every keychain operation now fails as locked or denied, rather than as
    /// "not found" — the distinction ADR 0008 insists on.
    pub fn with_locked_keychain(self, detail: &str) -> Self {
        self.lock_keychain(detail);
        self
    }

    /// The same, for a world that is already built.
    pub fn lock_keychain(&self, detail: &str) {
        *self.keychain_lock.borrow_mut() = Some(KeychainLock {
            detail: detail.to_string(),
        });
    }

    pub fn with_exec(self, program: &str, args: &[&str], execution: Execution) -> Self {
        self.executions
            .borrow_mut()
            .insert(exec_key(program, args), execution);
        self
    }

    /// What an interactive login leaves behind in the directory it was pointed
    /// at. Without one, a launched login does nothing and exits cleanly — an
    /// abandoned login.
    pub fn with_login(self, login: impl Fn(&FakeHost, &Path) -> i32 + 'static) -> Self {
        *self.login.borrow_mut() = Some(Box::new(login));
        self
    }

    /// What the rest of the machine does the first time Perch waits for a lock
    /// — the one point in a command where something else can get in.
    ///
    /// Once, because it stands for a thing that happens rather than for a
    /// condition: a client starting, a lock being given back.
    pub fn once_while_waiting(self, happens: impl Fn(&FakeHost) + 'static) -> Self {
        *self.while_waiting.borrow_mut() = Some(Box::new(happens));
        self
    }

    /// A process that is running, for a world that is already built.
    pub fn set_live_process(&self, pid: u32) {
        self.live_processes
            .borrow_mut()
            .insert(pid, Some(DateTime::<Utc>::MIN_UTC));
    }

    /// A directory somebody else already holds, last written when they say.
    /// A lock artifact, in other words: the age is what decides whether the
    /// holder is taken to be alive or to have died holding it.
    pub fn with_dir_held_since(self, path: impl AsRef<Path>, since: DateTime<Utc>) -> Self {
        self.dirs.borrow_mut().insert(path.as_ref().to_path_buf());
        self.modified
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), since);
        self
    }

    /// A process that is running, and has been since before any session a
    /// fixture records — so a marker naming it means a Live Profile rather
    /// than one a client left behind when it died.
    pub fn with_live_process(self, pid: u32) -> Self {
        self.live_processes
            .borrow_mut()
            .insert(pid, Some(DateTime::<Utc>::MIN_UTC));
        self
    }

    /// A process that is running and began at `at` — a recycled PID is one
    /// wearing a marker that was written before the process began (ADR 0022).
    pub fn with_live_process_started_at(self, pid: u32, at: DateTime<Utc>) -> Self {
        self.live_processes.borrow_mut().insert(pid, Some(at));
        self
    }

    /// A process that is running but whose start the operating system will not
    /// say — the one situation a session marker naming it can be neither
    /// corroborated nor dismissed in.
    pub fn with_live_process_of_unknown_start(self, pid: u32) -> Self {
        self.live_processes.borrow_mut().insert(pid, None);
        self
    }

    /// A machine with no one at the terminal: over SSH, or in CI.
    pub fn without_terminal(self) -> Self {
        *self.interactive.borrow_mut() = false;
        self
    }

    /// What the person at the terminal types, in order. Running out of answers
    /// is end of input, not a hang.
    pub fn with_answers(self, answers: &[&str]) -> Self {
        *self.answers.borrow_mut() = answers.iter().map(|line| line.to_string()).collect();
        self
    }

    /// What an endpoint answers, whoever asks.
    pub fn with_reply(self, url: &str, status: u16, body: &str) -> Self {
        self.reply(url, None, status, body);
        self
    }

    /// What an endpoint answers the holder of one access token.
    ///
    /// One URL serves every Account — which of them is asking is the Bearer
    /// token — so a test about two Accounts answering differently keys on the
    /// token rather than on the address.
    pub fn with_reply_to(self, url: &str, bearer: &str, status: u16, body: &str) -> Self {
        self.reply(url, Some(bearer), status, body);
        self
    }

    /// The same, for a world that is already built — a token that only exists
    /// once a Rotation has handed it over, for instance.
    pub fn reply(&self, url: &str, bearer: Option<&str>, status: u16, body: &str) {
        self.replies.borrow_mut().insert(
            (url.to_string(), bearer.map(str::to_string)),
            HttpResponse {
                status,
                body: body.to_string(),
            },
        );
    }

    // ---- inspecting what happened --------------------------------------

    pub fn effects(&self) -> Vec<Effect> {
        self.effects.borrow().clone()
    }

    /// Forgets what has happened so far, so a test that asserts on the order of
    /// effects sees the command it is about rather than the fixtures that set
    /// the machine up for it.
    pub fn forget_effects(&self) {
        self.effects.borrow_mut().clear();
    }

    pub fn http_calls(&self) -> Vec<String> {
        self.effects
            .borrow()
            .iter()
            .filter_map(|effect| match effect {
                Effect::Http { url, .. } => Some(url.clone()),
                _ => None,
            })
            .collect()
    }

    /// The requests that went to one endpoint, whole and in order.
    pub fn sent_to(&self, url: &str) -> Vec<Sent> {
        self.sent
            .borrow()
            .iter()
            .filter(|request| request.url == url)
            .cloned()
            .collect()
    }

    pub fn file(&self, path: impl AsRef<Path>) -> Option<String> {
        self.files.borrow().get(path.as_ref()).cloned()
    }

    /// The permissions a path ended up with, so a test can say that a file
    /// holding a Credential was created for its owner alone.
    pub fn mode_of(&self, path: impl AsRef<Path>) -> Option<u32> {
        self.modes.borrow().get(path.as_ref()).copied()
    }

    /// What the user was told that they did not ask about, in order and
    /// without repeats.
    pub fn notes(&self) -> Vec<String> {
        self.notes.borrow().clone()
    }

    pub fn keychain_item(&self, service: &str, account: &str) -> Option<String> {
        self.keychain
            .borrow()
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }

    /// Every service name the keychain holds an item under, so a test can say
    /// that a Profile Perch abandoned left nothing behind.
    pub fn keychain_services(&self) -> Vec<String> {
        self.keychain
            .borrow()
            .keys()
            .map(|(service, _)| service.clone())
            .collect()
    }

    /// Creates a directory and those above it, giving `mode` to the ones that
    /// were not already there.
    ///
    /// `mkdir -p` leaves an existing directory's mode alone, and a fake that
    /// stamped every parent instead would report a Profile as private however
    /// it had been created — which is the one thing these modes are here to
    /// tell apart (ADR 0020).
    fn make_dirs(&self, path: &Path, mode: u32) {
        let mut missing = Vec::new();
        let mut at = Some(path);
        while let Some(dir) = at {
            if dir.as_os_str().is_empty() || self.dirs.borrow().contains(dir) {
                break;
            }
            missing.push(dir.to_path_buf());
            at = dir.parent();
        }

        let mut dirs = self.dirs.borrow_mut();
        let mut modes = self.modes.borrow_mut();
        for dir in missing {
            modes.entry(dir.clone()).or_insert(mode);
            dirs.insert(dir);
        }
    }

    fn record(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
    }

    fn mark_written(&self, path: &Path) {
        self.modified
            .borrow_mut()
            .insert(path.to_path_buf(), *self.now.borrow());
    }

    fn lock_error(&self) -> Option<KeychainError> {
        self.keychain_lock
            .borrow()
            .as_ref()
            .map(|lock| KeychainError::Unavailable {
                detail: lock.detail.clone(),
            })
    }
}

fn exec_key(program: &str, args: &[&str]) -> String {
    let mut key = program.to_string();
    for arg in args {
        key.push(' ');
        key.push_str(arg);
    }
    key
}

impl Host for FakeHost {
    fn now(&self) -> DateTime<Utc> {
        *self.now.borrow()
    }

    fn home_dir(&self) -> Result<PathBuf, HostError> {
        Ok(self.home.clone())
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.env.borrow().get(key).cloned()
    }

    fn platform(&self) -> Platform {
        *self.platform.borrow()
    }

    fn read_file(&self, path: &Path) -> Result<String, HostError> {
        self.record(Effect::ReadFile(path.to_path_buf()));
        if let Some(detail) = self.unreadable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| HostError::NotFound {
                path: path.to_path_buf(),
            })
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        self.record(Effect::WroteFile(path.to_path_buf()));
        if let Some(detail) = self.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.note_directories_of(path);
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        self.mark_written(path);
        Ok(())
    }

    /// Records the mode the file was *created* with, which is the whole point
    /// of the call: a test can then say that a replacement for a narrow file
    /// came back just as narrow.
    fn create_file_with_mode(
        &self,
        path: &Path,
        contents: &str,
        mode: u32,
    ) -> Result<(), HostError> {
        self.record(Effect::WroteFile(path.to_path_buf()));
        if let Some(detail) = self.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.note_directories_of(path);
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        self.modes.borrow_mut().insert(path.to_path_buf(), mode);
        self.mark_written(path);
        Ok(())
    }

    /// Records the mode as well as the contents, because "created private" and
    /// "made private afterwards" are the distinction ADR 0020 turns on and a
    /// fake that only kept the bytes could not tell them apart.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        self.record(Effect::WrotePrivateFile(path.to_path_buf()));
        if let Some(detail) = self.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        if let Some(parent) = path.parent() {
            self.make_dirs(parent, PRIVATE_DIR_MODE);
        }
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        self.modes
            .borrow_mut()
            .insert(path.to_path_buf(), PRIVATE_FILE_MODE);
        self.mark_written(path);
        Ok(())
    }

    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::CreatedDir(path.to_path_buf()));
        self.make_dirs(path, PRIVATE_DIR_MODE);
        self.mark_written(path);
        Ok(())
    }

    /// A path Perch did not create has whatever mode it was given: an ordinary
    /// one for a directory, and — since a file the fixtures put there stands in
    /// for one Claude Code wrote — the owner alone for a file.
    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError> {
        if !self.path_exists(path) {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        }
        Ok(Some(self.mode_of(path).unwrap_or(
            if self.dirs.borrow().contains(path) {
                ORDINARY_DIR_MODE
            } else {
                PRIVATE_FILE_MODE
            },
        )))
    }

    fn make_private(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::MadePrivate(path.to_path_buf()));
        if !self.path_exists(path) {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        }
        self.modes
            .borrow_mut()
            .insert(path.to_path_buf(), PRIVATE_FILE_MODE);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::CreatedDir(path.to_path_buf()));
        self.make_dirs(path, ORDINARY_DIR_MODE);
        self.mark_written(path);
        Ok(())
    }

    fn path_exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedDir(path.to_path_buf()));
        self.dirs.borrow_mut().retain(|dir| !dir.starts_with(path));
        self.files
            .borrow_mut()
            .retain(|file, _| !file.starts_with(path));
        self.modified
            .borrow_mut()
            .retain(|written, _| !written.starts_with(path));
        self.modes
            .borrow_mut()
            .retain(|at, _| !at.starts_with(path));
        Ok(())
    }

    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::Took(path.to_path_buf()));
        if self.path_exists(path) {
            return Err(HostError::AlreadyExists {
                path: path.to_path_buf(),
            });
        }
        self.dirs.borrow_mut().insert(path.to_path_buf());
        self.mark_written(path);
        Ok(())
    }

    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError> {
        self.modified
            .borrow()
            .get(path)
            .copied()
            .ok_or_else(|| HostError::NotFound {
                path: path.to_path_buf(),
            })
    }

    fn touch(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::Touched(path.to_path_buf()));
        if !self.path_exists(path) {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        }
        self.mark_written(path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError> {
        self.record(Effect::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
        if let Some(detail) = self.unwritable.borrow().get(to) {
            return Err(HostError::Other(detail.clone()));
        }
        let moved = self
            .files
            .borrow_mut()
            .remove(from)
            .ok_or_else(|| HostError::NotFound {
                path: from.to_path_buf(),
            })?;
        self.files.borrow_mut().insert(to.to_path_buf(), moved);
        // A rename moves the file, mode and all: what ends up at the target is
        // the file that was created beside it, not the one it replaced.
        let mode = self.modes.borrow_mut().remove(from);
        match mode {
            Some(mode) => self.modes.borrow_mut().insert(to.to_path_buf(), mode),
            None => self.modes.borrow_mut().remove(to),
        };
        self.mark_written(to);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedFile(path.to_path_buf()));
        if let Some(detail) = self.undeletable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.files.borrow_mut().remove(path);
        self.modified.borrow_mut().remove(path);
        self.modes.borrow_mut().remove(path);
        Ok(())
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if !self.dirs.borrow().contains(path) {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        }
        let held = |candidate: &PathBuf| candidate.parent() == Some(path);
        let mut found: BTreeSet<PathBuf> = self
            .files
            .borrow()
            .keys()
            .filter(|file| held(file))
            .cloned()
            .collect();
        found.extend(self.dirs.borrow().iter().filter(|dir| held(dir)).cloned());
        Ok(found.into_iter().collect())
    }

    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError> {
        self.record(Effect::KeychainGet {
            service: service.to_string(),
            account: account.to_string(),
        });
        if let Some(error) = self.lock_error() {
            return Err(error);
        }
        self.keychain
            .borrow()
            .get(&(service.to_string(), account.to_string()))
            .cloned()
            .ok_or_else(|| KeychainError::NotFound {
                service: service.to_string(),
                account: account.to_string(),
            })
    }

    fn keychain_set(
        &self,
        service: &str,
        account: &str,
        secret: &str,
    ) -> Result<(), KeychainError> {
        self.record(Effect::KeychainSet {
            service: service.to_string(),
            account: account.to_string(),
        });
        if let Some(error) = self.lock_error() {
            return Err(error);
        }
        self.keychain.borrow_mut().insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
        Ok(())
    }

    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        self.record(Effect::KeychainDelete {
            service: service.to_string(),
            account: account.to_string(),
        });
        if let Some(error) = self.lock_error() {
            return Err(error);
        }
        self.keychain
            .borrow_mut()
            .remove(&(service.to_string(), account.to_string()))
            .map(|_| ())
            .ok_or_else(|| KeychainError::NotFound {
                service: service.to_string(),
                account: account.to_string(),
            })
    }

    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError> {
        self.record(Effect::Exec {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
        self.executions
            .borrow()
            .get(&exec_key(program, args))
            .cloned()
            .ok_or_else(|| HostError::Other(format!("no such program: {program}")))
    }

    /// Stands in for the login the user drives: it writes whatever the
    /// configured [`Login`] leaves in the directory it was pointed at.
    fn exec_interactive(&self, program: &str, env: &[(&str, &str)]) -> Result<i32, HostError> {
        let config_dir = env
            .iter()
            .find(|(key, _)| *key == "CLAUDE_CONFIG_DIR")
            .map(|(_, value)| PathBuf::from(value))
            .unwrap_or_else(|| self.home.join(".claude"));

        self.record(Effect::ExecInteractive {
            program: program.to_string(),
            config_dir: config_dir.clone(),
        });

        let login = self.login.borrow();
        match login.as_ref() {
            Some(login) => Ok(login(self, &config_dir)),
            None => Ok(0),
        }
    }

    fn process_alive(&self, pid: u32) -> bool {
        self.live_processes.borrow().contains_key(&pid)
    }

    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>> {
        self.live_processes.borrow().get(&pid).copied().flatten()
    }

    /// Costs no time, but does pass it: waiting for a lock somebody else holds
    /// is how that lock comes to be stale, and a test should be able to reach
    /// that without sitting through it.
    fn sleep(&self, millis: u64) {
        self.record(Effect::Slept { millis });
        let waited = *self.now.borrow() + chrono::Duration::milliseconds(millis as i64);
        *self.now.borrow_mut() = waited;

        // Taken out before it runs, so it can reach back into the fake without
        // meeting a borrow this call is still holding.
        let happens = self.while_waiting.borrow_mut().take();
        if let Some(happens) = happens {
            happens(self);
        }
    }

    fn is_interactive(&self) -> bool {
        *self.interactive.borrow()
    }

    fn note(&self, line: &str) {
        let mut notes = self.notes.borrow_mut();
        if !notes.iter().any(|said| said == line) {
            notes.push(line.to_string());
        }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        self.record(Effect::Asked);
        Ok(self.answers.borrow_mut().pop_front())
    }

    /// Answers with whatever the test arranged for this endpoint, and with
    /// nothing at all otherwise.
    ///
    /// A machine with no arranged replies has no network, so a command that
    /// fetches when it should not fails rather than quietly succeeding — and
    /// every request is recorded either way, for `http_calls` to report.
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError> {
        let sent = Sent {
            url: request.url.to_string(),
            headers: request
                .headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            body: request.body.map(str::to_string),
        };
        self.record(Effect::Http {
            url: sent.url.clone(),
        });
        let asked = sent.url.clone();
        let bearer = sent.bearer().map(str::to_string);
        self.sent.borrow_mut().push(sent);

        let replies = self.replies.borrow();
        replies
            .get(&(asked.clone(), bearer))
            .or_else(|| replies.get(&(asked.clone(), None)))
            .cloned()
            .ok_or_else(|| HostError::Other(format!("the fake Host has no network: {asked}")))
    }
}
